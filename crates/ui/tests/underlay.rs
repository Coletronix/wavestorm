//! Background underlay: load, calibrate with two clicks, trace a subcatchment.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use wavestorm_core::project::{NodeData, NodeType};
use wavestorm_ui::platform::{Incoming, Purpose};
use wavestorm_ui::{WaveStormApp, script};

fn click(h: &mut egui_kittest::Harness<'static, WaveStormApp>, pos: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.step();
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.step();
    // Let the double-click window expire so separate clicks stay separate.
    h.run_steps(30);
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, y| if (x / 20 + y / 20) % 2 == 0 { image::Rgba([220, 230, 210, 255]) } else { image::Rgba([250, 250, 240, 255]) });
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

/// A minimal one-page PDF with a filled rectangle, with a correct cross-reference table.
fn pdf() -> Vec<u8> {
    let content = "0.2 0.6 0.2 rg 100 100 300 200 re f";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), content),
    ];
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, o));
    }
    let xref = out.len();
    out.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1));
    for off in offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1));
    out.into_bytes()
}

fn harness() -> egui_kittest::Harness<'static, WaveStormApp> {
    egui_kittest::Harness::builder().with_size(vec2(1280.0, 800.0)).build_ui_state(|ui, app: &mut WaveStormApp| app.draw(ui), WaveStormApp::headless())
}

#[test]
fn pdf_page_renders() {
    let r = wavestorm_ui::underlay::rasterize(&pdf(), 0, 72.0).expect("pdf renders");
    assert_eq!((r.width, r.height), (612, 792));
    // The green rectangle is drawn (PDF y is up: rectangle spans y 100..300 from the bottom).
    let px = |x: u32, y: u32| &r.rgba[((y * r.width + x) * 4) as usize..((y * r.width + x) * 4 + 3) as usize];
    assert!(px(200, 792 - 200)[1] > px(200, 792 - 200)[0], "expected green inside the rectangle");
    assert_eq!(px(10, 10), &[255, 255, 255]);
}

#[test]
fn calibrate_and_trace_subcatchment() {
    for bytes in [png(600, 400), pdf()] {
        let mut h = harness();
        h.run_steps(3);
        script(h.state_mut(), "new");
        h.run_steps(2);
        h.state_mut().platform.push(Incoming { purpose: Purpose::Underlay, name: "site".into(), path: None, bytes });
        h.run_steps(4);
        assert!(h.state_mut().doc().project.diagram.underlay.is_some());
        assert!(h.state_mut().doc().underlay_tex.is_some(), "texture created");
        // Close the calibration hint and calibrate: 100 diagram units = 250 ft.
        h.state_mut().doc().windows.clear();
        script(h.state_mut(), "calibrate");
        h.step();
        let (a, b) = (h.state_mut().world_to_screen(pos2(100.0, 100.0)), h.state_mut().world_to_screen(pos2(200.0, 100.0)));
        click(&mut h, a);
        click(&mut h, b);
        h.run_steps(2);
        // The calibration dialog opened with the two points; enter 250 ft and press OK through the state.
        {
            let doc = h.state_mut().doc();
            let fpu = 250.0 / 100.0;
            doc.edit("Calibrate Background", |p| {
                if let Some(u) = &mut p.diagram.underlay {
                    u.feet_per_unit = Some(fpu);
                }
            });
            doc.windows.clear();
        }
        // Trace a 100 x 80 unit rectangle = 250 x 200 ft = 50,000 sq ft.
        script(h.state_mut(), "trace");
        h.step();
        let corners = [pos2(100.0, 100.0), pos2(200.0, 100.0), pos2(200.0, 180.0), pos2(100.0, 180.0)];
        for c in corners {
            let s = h.state_mut().world_to_screen(c);
            click(&mut h, s);
        }
        let first = h.state_mut().world_to_screen(corners[0]);
        click(&mut h, first);
        h.run_steps(2);
        let doc = h.state_mut().doc();
        let sub = doc.project.nodes.iter().find(|n| n.node_type() == NodeType::Subcat && n.outline.is_some()).expect("traced subcatchment");
        let NodeData::Subcat(sc) = &sub.data else { panic!() };
        let area: f64 = sc.areas.iter().map(|a| a.area).sum();
        // Clicks land on whole pixels, so allow a little slack.
        assert!((area - 50_000.0).abs() / 50_000.0 < 0.03, "traced area {area}");
        assert_eq!(sub.outline.as_ref().map(|o| o.len()), Some(4));

        // Draw its Tc flow path: 40 units (100 ft) down, then 80 units (200 ft) across.
        let (id, label) = (sub.id, sub.label());
        doc.windows.clear();
        script(h.state_mut(), &format!("tc-path:{label}"));
        h.step();
        for w in [pos2(110.0, 110.0), pos2(110.0, 150.0), pos2(190.0, 150.0)] {
            let s = h.state_mut().world_to_screen(w);
            click(&mut h, s);
        }
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        let app = h.state_mut();
        assert_eq!(app.tool.name(), "Select", "Enter finishes the path");
        let doc = app.doc();
        let st = doc
            .windows
            .iter()
            .find_map(|w| match w {
                wavestorm_ui::windows::Window::Edit(e) if e.node == id => Some(e),
                _ => None,
            })
            .expect("the subcatchment's edit dialog opens");
        assert_eq!(st.tab, 2, "on the Tc tab");
        let ps = st.tc_path.as_ref().expect("flow path dialog");
        assert_eq!(ps.legs.len(), 2);
        assert!((ps.legs[0].length - 100.0).abs() < 3.0 && (ps.legs[1].length - 200.0).abs() < 4.0, "legs {:?}", ps.legs);
        // First leg sheet flow (within the 100 ft limit), then shallow concentrated flow.
        let segs = ps.segments(&[]);
        let kinds: Vec<(&str, &str)> = segs.iter().map(|s| (s.kind.label(), s.description.as_str())).collect();
        assert_eq!(kinds, [("Sheet Flow", "A to B"), ("Shallow Concentrated Flow", "B to C")]);
        // OK writes the segments and the lettered path into the subcatchment.
        let mut node = doc.project.node(id).unwrap().clone();
        ps.apply(&mut node);
        assert_eq!(node.tc_path.as_ref().map(Vec::len), Some(3));
        assert_eq!(node.tc_path_breaks, [0, 1, 2], "every point ends a segment here");
        // Making the second leg sheet flow too merges the legs: only the ends are lettered.
        let mut merged = wavestorm_ui::dialogs::node::TcPathState::new(node.tc_path.clone().unwrap(), Some(2.5), &[]);
        merged.limit_sheet = false;
        merged.reset(&[]);
        merged.legs[1].flow = merged.legs[0].flow;
        let mut n2 = node.clone();
        merged.apply(&mut n2);
        assert_eq!(n2.tc_path_breaks, [0, 2]);
        let NodeData::Subcat(sc2) = &n2.data else { panic!() };
        assert_eq!(sc2.tc.iter().map(|s| s.description.as_str()).collect::<Vec<_>>(), ["A to B"]);
        let NodeData::Subcat(sc) = &mut node.data else { panic!() };
        assert_eq!(sc.tc.len(), 2);
        // Deleting one path segment keeps the path; deleting the last one removes it.
        sc.tc.remove(0);
        wavestorm_ui::dialogs::node::drop_orphan_path(&mut node);
        assert!(node.tc_path.is_some());
        if let NodeData::Subcat(sc) = &mut node.data {
            sc.tc.clear();
        }
        wavestorm_ui::dialogs::node::drop_orphan_path(&mut node);
        assert!(node.tc_path.is_none(), "the path goes with its last segment");
    }
}
