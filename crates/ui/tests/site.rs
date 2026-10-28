//! Placing the site and its soils: an address lookup and a map image arrive (as canned replies;
//! automated runs never go online), the background is georeferenced, soils arrive for an area
//! of interest, and applying them splits a subcatchment's sub-areas by soil group. Also the
//! two-point georeference of a background of one's own.

use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use wavestorm_core::cn::SubArea;
use wavestorm_core::geo::{self, MapSource};
use wavestorm_core::project::{NodeData, NodeType};
use wavestorm_ui::net::{Fetch, Reply};
use wavestorm_ui::platform::{Incoming, Purpose};
use wavestorm_ui::windows::Window;
use wavestorm_ui::{WaveStormApp, script};

fn harness() -> egui_kittest::Harness<'static, WaveStormApp> {
    egui_kittest::Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut WaveStormApp| app.draw(ui), WaveStormApp::headless())
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x % 256) as u8, (y % 256) as u8, 120, 255]));
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn click(h: &mut egui_kittest::Harness<'static, WaveStormApp>, pos: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(30);
}

const CENTER: [f64; 2] = [-93.6175, 42.032];

#[test]
fn aerial_photo_soils_and_apply() {
    let mut h = harness();
    h.run_steps(3);
    script(h.state_mut(), "new");
    script(h.state_mut(), "aerial");
    h.run_steps(3);

    // The address lookup answers; the Aerial Photo window takes the place.
    let reply = r#"[{"lat":"42.032","lon":"-93.6175","display_name":"Somewhere, Ames, Iowa"}]"#;
    h.state_mut().net.push(Reply { fetch: Fetch::Geocode("Ames".into()), result: Ok(reply.as_bytes().to_vec()) });
    h.run_steps(3);
    let found = h.state_mut().doc().windows.iter().find_map(|w| match w {
        Window::Locate(s) => s.place.clone(),
        _ => None,
    });
    assert_eq!(found.map(|p| p.0), Some(CENTER));

    // The map image arrives and becomes a georeferenced, calibrated background.
    let req = geo::map_request(MapSource::Imagery, CENTER, 1500.0, 1000.0).unwrap();
    h.state_mut().net.push(Reply {
        fetch: Fetch::Map {
            source: MapSource::Imagery,
            center: CENTER,
            width_ft: 1500.0,
            height_ft: 1000.0,
            place: "Ames".into(),
        },
        result: Ok(png(req.width_px, req.height_px)),
    });
    h.run_steps(3);
    let u = h.state_mut().doc().project.diagram.underlay.clone().expect("background loaded");
    assert!(u.geo.is_some());
    let fpu = u.feet_per_unit.unwrap();
    assert!((fpu * u.scale - req.feet_per_pixel).abs() < 1e-9);
    // The image centre is the site.
    let mid = geo::pixel_to_diagram(&u, [req.width_px as f64 / 2.0, req.height_px as f64 / 2.0]);
    let ll = geo::to_lon_lat(&u, mid).unwrap();
    assert!((ll[0] - CENTER[0]).abs() < 1e-9 && (ll[1] - CENTER[1]).abs() < 1e-9);

    // A 200 x 100 ft subcatchment centred on the site, one woods row.
    let at = |e: f64, n: f64| geo::from_lon_lat(&u, geo::offset_lon_lat(CENTER, [e, n])).unwrap();
    let outline = vec![at(-100.0, -50.0), at(100.0, -50.0), at(100.0, 50.0), at(-100.0, 50.0)];
    let doc = h.state_mut().doc();
    let mut id = 0;
    doc.edit("Test Subcatchment", |p| {
        id = p.add_node(NodeType::Subcat, at(0.0, 0.0));
        let n = p.node_mut(id).unwrap();
        n.outline = Some(outline.clone());
        let NodeData::Subcat(sc) = &mut n.data else { unreachable!() };
        sc.areas = vec![SubArea::new(20_000.0, 55.0, "Woods, Good, HSG B")];
    });

    // Soils arrive for an AOI around it: B/D west of the site's centre line + 20 ft, C east.
    script(h.state_mut(), "soils");
    h.run_steps(3);
    let ring = |e0: f64, e1: f64| {
        [[e0, -80.0], [e1, -80.0], [e1, 80.0], [e0, 80.0], [e0, -80.0]]
            .iter()
            .map(|q| {
                let p = geo::offset_lon_lat(CENTER, *q);
                format!("{} {}", p[0], p[1])
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let body = format!(
        r#"{{"Table":[["mukey","musym","muname","hydgrpdcd","wkt"],
            ["1","L55","Nicollet loam","B/D","POLYGON (({}))"],
            ["2","L107","Webster clay loam","C","POLYGON (({}))"]]}}"#,
        ring(-150.0, 20.0),
        ring(20.0, 150.0)
    );
    let aoi: Vec<[f64; 2]> = [[-150.0, -80.0], [150.0, -80.0], [150.0, 80.0], [-150.0, 80.0]]
        .iter()
        .map(|q| geo::offset_lon_lat(CENTER, *q))
        .collect();
    h.state_mut().net.push(Reply { fetch: Fetch::Soils { aoi, drained: false }, result: Ok(body.into_bytes()) });
    h.run_steps(3);
    let map = h.state_mut().doc().project.soils.clone().expect("soils stored");
    assert_eq!(map.units.len(), 2);

    // Apply: 60 % of the subcatchment is on B/D (= D undrained), 40 % on C.
    h.get_by_label("Apply to Subcatchments").click();
    h.run_steps(3);
    let rows: Vec<(String, f64, f64)> = match &h.state_mut().doc().project.node(id).unwrap().data {
        NodeData::Subcat(sc) => sc.areas.iter().map(|a| (a.description.clone(), a.area, a.cn)).collect(),
        _ => unreachable!(),
    };
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].0, "Woods, Good, HSG C");
    assert!((rows[0].1 / 20_000.0 - 0.4).abs() < 0.002, "{rows:?}");
    assert_eq!(rows[0].2, 70.0);
    assert_eq!(rows[1].0, "Woods, Good, HSG D");
    assert_eq!(rows[1].2, 77.0);

    // The print dialog offers the soil report, ticked.
    script(h.state_mut(), "print");
    h.run_steps(3);
    let ticked = h.state_mut().doc().windows.iter().any(|w| matches!(w, Window::Print(s) if s.opts.hsg_report));
    assert!(ticked);
}

#[test]
fn failed_requests_are_reported_not_fatal() {
    let mut h = harness();
    h.run_steps(3);
    script(h.state_mut(), "soils");
    h.run_steps(3);
    let aoi = vec![[-93.62, 42.03], [-93.61, 42.03], [-93.61, 42.04]];
    h.state_mut().net.push(Reply { fetch: Fetch::Soils { aoi, drained: false }, result: Ok(b"{}".to_vec()) });
    h.run_steps(3);
    let msg = h.state_mut().doc().windows.iter().find_map(|w| match w {
        Window::Soils(s) => Some(s.message.clone()),
        _ => None,
    });
    assert!(msg.unwrap().contains("no map units"));
    assert!(h.state_mut().doc().project.soils.is_none());
    // A network failure arrives as an error.
    h.state_mut().net.push(Reply { fetch: Fetch::Geocode("x".into()), result: Err("No connection".into()) });
    h.run_steps(3);
}

#[test]
fn georeference_two_points_of_own_background() {
    let mut h = harness();
    h.run_steps(3);
    script(h.state_mut(), "new");
    h.state_mut().platform.push(Incoming {
        purpose: Purpose::Underlay,
        name: "plan.png".into(),
        path: None,
        bytes: png(400, 300),
    });
    h.run_steps(5);
    // Leave the placement tool and close the calibration hint.
    h.state_mut().tool = wavestorm_ui::diagram::Tool::Select;
    h.state_mut().doc().windows.clear();
    script(h.state_mut(), "georef");
    h.run_steps(2);
    let u = h.state_mut().doc().project.diagram.underlay.clone().unwrap();
    // Two pixels 300 px apart horizontally, picked on screen.
    let p1 = geo::pixel_to_diagram(&u, [50.0, 150.0]);
    let p2 = geo::pixel_to_diagram(&u, [350.0, 150.0]);
    let s1 = h.state_mut().world_to_screen(Pos2::new(p1[0] as f32, p1[1] as f32));
    let s2 = h.state_mut().world_to_screen(Pos2::new(p2[0] as f32, p2[1] as f32));
    click(&mut h, s1);
    click(&mut h, s2);
    // They are 600 ft apart due east of each other.
    let g1 = CENTER;
    let g2 = geo::offset_lon_lat(CENTER, [600.0, 0.0]);
    let mut opened = false;
    for w in &mut h.state_mut().doc().windows {
        if let Window::Georef(s) = w {
            s.text_a = geo::format_lat_lon(g1);
            s.text_b = geo::format_lat_lon(g2);
            opened = true;
        }
    }
    assert!(opened, "the georeference dialog opens after two clicks");
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(3);
    let u = h.state_mut().doc().project.diagram.underlay.clone().unwrap();
    let g = u.geo.expect("georeferenced");
    // 600 ft over 300 px (clicks land within a pixel or so); north-up.
    let fpp = u.feet_per_unit.unwrap() * u.scale;
    assert!((fpp - 2.0).abs() < 0.05, "{fpp}");
    assert!(g.bearing.abs() < 0.02, "{}", g.bearing);
}

/// The real services (address lookup, NAIP image, Soil Data Access), through the same request
/// code the app uses. Needs the internet: `cargo test -p wavestorm-ui --test site -- --ignored`.
#[test]
#[ignore]
fn live_services() {
    let net = wavestorm_ui::net::Net::new(true);
    let ctx = egui::Context::default();
    let wait = |net: &wavestorm_ui::net::Net| {
        for _ in 0..1200 {
            let r = net.take();
            if let Some(reply) = r.into_iter().next() {
                return reply;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("no reply within 2 minutes");
    };
    net.start(&ctx, Fetch::Geocode("Iowa State University, Ames, Iowa".into()));
    let body = wait(&net).result.expect("geocode reply");
    let (place, name) = geo::parse_geocode(&String::from_utf8_lossy(&body)).expect("a place");
    assert!((place[1] - 42.03).abs() < 0.05 && (place[0] + 93.64).abs() < 0.05, "{place:?} {name}");

    let req = geo::map_request(MapSource::Imagery, place, 1500.0, 1000.0).unwrap();
    net.start(
        &ctx,
        Fetch::Map { source: MapSource::Imagery, center: place, width_ft: 1500.0, height_ft: 1000.0, place: name },
    );
    let img = wait(&net).result.expect("map image");
    let r = wavestorm_ui::underlay::rasterize(&img, 0, 150.0).expect("decodes");
    assert_eq!((r.width, r.height), (req.width_px, req.height_px));

    let aoi: Vec<[f64; 2]> = [[-300.0, -300.0], [300.0, -300.0], [300.0, 300.0], [-300.0, 300.0]]
        .iter()
        .map(|q| geo::offset_lon_lat(place, *q))
        .collect();
    net.start(&ctx, Fetch::Soils { aoi: aoi.clone(), drained: false });
    let body = wait(&net).result.expect("soils reply");
    let map = wavestorm_core::soils::parse_sda(&String::from_utf8_lossy(&body), &aoi, false, "today").expect("soils");
    assert!(!map.units.is_empty() && !map.polygons.is_empty());
    // The clipped map units cover the AOI (about 8.3 acres).
    let c = wavestorm_core::soils::center(&aoi);
    let covered: f64 = map.polygons.iter().map(|p| wavestorm_core::soils::lon_lat_area(&p.rings[0], c)).sum();
    assert!((covered / 360_000.0 - 1.0).abs() < 0.02, "{covered}");
}
