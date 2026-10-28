//! Placing the site in the real world and getting its soils: the aerial photo download, the
//! two-point georeference, and the Soils window (area of interest, Soil Data Access, applying
//! soil groups to subcatchments).

use super::{Buttons, conv_field, ok_cancel_apply};
use crate::doc::Doc;
use crate::net::Fetch;
use crate::theme::Tokens;
use crate::widgets;
use crate::windows::WindowAction;
use egui::{Pos2, RichText};
use wavestorm_core::drawing::{self, Pt};
use wavestorm_core::geo::{self, LonLat, MapSource};
use wavestorm_core::{soils, terrain};

/// Web Soil Survey, for checking the soils by hand.
pub const WSS_URL: &str = "https://websoilsurvey.nrcs.usda.gov/app/";

/// What the downloaded background shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backdrop {
    /// A map image.
    Map(MapSource),
    /// Shaded relief with contours, drawn from 3DEP lidar elevations.
    Relief,
}

impl Backdrop {
    pub const ALL: [Backdrop; 5] = [
        Backdrop::Map(MapSource::Recent),
        Backdrop::Map(MapSource::Imagery),
        Backdrop::Relief,
        Backdrop::Map(MapSource::ImageryLabels),
        Backdrop::Map(MapSource::Topo),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Backdrop::Map(s) => s.label(),
            Backdrop::Relief => "Lidar shaded relief and contours (USGS 3DEP)",
        }
    }
}

/// "Aerial Photo": find the site and download a georeferenced background and its elevations.
pub struct LocateState {
    pub query: String,
    /// The site found: its place and description.
    pub place: Option<(LonLat, String)>,
    pub backdrop: Backdrop,
    /// Also download the ground elevations (for Tc slopes) with a map image.
    pub elevations: bool,
    /// Ground size of the image (ft).
    pub width: f64,
    pub height: f64,
    /// Progress or the last problem.
    pub message: String,
}

impl LocateState {
    pub fn new(doc: &Doc) -> Self {
        // A georeferenced background suggests its own site.
        let place = doc.project.diagram.underlay.as_ref().and_then(|u| u.geo).map(|g| {
            let text = geo::format_lat_lon(g.lon_lat);
            (g.lon_lat, text)
        });
        LocateState {
            query: place.as_ref().map(|p| p.1.clone()).unwrap_or_default(),
            place,
            backdrop: Backdrop::Map(MapSource::default()),
            elevations: true,
            width: 1500.0,
            height: 1000.0,
            message: String::new(),
        }
    }

    /// Takes a place from the address lookup.
    pub fn found(&mut self, place: LonLat, name: String) {
        self.message = format!("Found: {name}");
        self.place = Some((place, name));
    }
}

pub fn locate(ctx: &egui::Context, doc: &mut Doc, st: &mut LocateState, actions: &mut Vec<WindowAction>) -> bool {
    let mut keep = true;
    let u = doc.project.units;
    crate::frame::Win::new("Aerial Photo").collapsible(false).resizable(false).show(ctx, |ui| {
        let t = Tokens::get(ui.ctx());
        ui.label("Find the site by address, or paste its latitude and longitude");
        ui.label(RichText::new("(decimal degrees, e.g. 42.0312, -93.6178 copied from a web map).").color(t.text_dim));
        ui.horizontal(|ui| {
            let resp = widgets::text_box(ui, &mut st.query, 300.0);
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (widgets::button(ui, "Find", 70.0).clicked() || enter) && !st.query.trim().is_empty() {
                match geo::parse_lat_lon(&st.query) {
                    Some(ll) => st.found(ll, geo::format_lat_lon(ll)),
                    None => {
                        st.message = "Looking up the address...".into();
                        actions.push(WindowAction::Fetch(Fetch::Geocode(st.query.trim().to_string())));
                    }
                }
            }
        });
        match &st.place {
            Some((ll, _)) => ui.label(format!("Site centre: {}", geo::format_lat_lon(*ll))),
            None => ui.label(RichText::new("No site found yet.").color(t.text_dim)),
        };
        ui.add_space(4.0);
        let options: Vec<(Backdrop, String)> = Backdrop::ALL.iter().map(|s| (*s, s.label().to_string())).collect();
        widgets::combo_field(ui, "map-source", "Map:", &mut st.backdrop, &options, 300.0);
        ui.horizontal(|ui| {
            let len = u.len_label();
            conv_field(ui, "aw", "Width:", len, &mut st.width, |x| u.len(x), |x| u.len_in(x), 0, 140.0);
            conv_field(ui, "ah", "Height:", len, &mut st.height, |x| u.len(x), |x| u.len_in(x), 0, 140.0);
        });
        let size = match st.backdrop {
            Backdrop::Map(source) => geo::map_request(source, [0.0, 40.0], st.width, st.height)
                .map(|r| format!("{} x {} pixels, about {:.2} {} per pixel", r.width_px, r.height_px, u.len(r.feet_per_pixel), u.len_label())),
            Backdrop::Relief => terrain::dem_request([0.0, 40.0], st.width, st.height).map(|r| {
                format!("{} x {} elevations, {:.1} {} apart", r.width_px, r.height_px, u.len(r.feet_per_pixel), u.len_label())
            }),
        };
        if let Ok(text) = size {
            ui.label(RichText::new(text).color(t.text_dim));
        }
        match st.backdrop {
            Backdrop::Map(_) => {
                widgets::check(ui, &mut st.elevations, "Also get lidar ground elevations for Tc slopes").on_hover_text(
                    "USGS 3DEP bare-earth elevations (1 m lidar where flown) for the same area: drawn Tc flow paths take \
                     each segment's slope from the ground.",
                );
            }
            Backdrop::Relief => {
                ui.label(
                    RichText::new("The elevations are kept too: drawn Tc flow paths take their slopes from them.")
                        .color(t.text_dim),
                );
            }
        }
        if st.backdrop == Backdrop::Map(MapSource::Recent) {
            ui.label(
                RichText::new(
                    "Esri World Imagery is the most recent in most places, but it is not public domain: keep the \
                     credit with printed copies and check Esri's terms of use for your work.",
                )
                .small()
                .color(t.text_dim),
            );
        }
        let has_scale = doc.project.diagram.underlay.as_ref().and_then(|ul| ul.feet_per_unit).is_some();
        if doc.project.diagram.underlay.is_some() {
            ui.colored_label(
                t.warning,
                if has_scale {
                    "The photo replaces the current background; the diagram scale is kept."
                } else {
                    "The photo replaces the current background."
                },
            );
        }
        if !st.message.is_empty() {
            ui.label(&st.message);
        }
        ui.add_space(4.0);
        let credit = match st.backdrop {
            Backdrop::Map(source) => source.credit(),
            Backdrop::Relief => terrain::CREDIT,
        };
        ui.label(
            RichText::new(format!(
                "Map: {credit}. Elevations: {} (public domain). Addresses: © OpenStreetMap contributors.",
                terrain::CREDIT
            ))
            .small()
            .color(t.text_dim),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let ready = st.place.is_some();
            if widgets::button_enabled(ui, "Download Background", 150.0, ready).clicked()
                && let Some((center, place)) = st.place.clone()
            {
                let (width_ft, height_ft) = (st.width, st.height);
                match st.backdrop {
                    Backdrop::Map(source) => {
                        st.message = "Downloading the map image...".into();
                        actions.push(WindowAction::Fetch(Fetch::Map {
                            source,
                            center,
                            width_ft,
                            height_ft,
                            place: place.clone(),
                        }));
                        if st.elevations {
                            actions.push(WindowAction::Fetch(Fetch::Terrain {
                                center,
                                width_ft,
                                height_ft,
                                place,
                                background: false,
                            }));
                        }
                    }
                    Backdrop::Relief => {
                        st.message = "Downloading the lidar elevations...".into();
                        actions.push(WindowAction::Fetch(Fetch::Terrain {
                            center,
                            width_ft,
                            height_ft,
                            place,
                            background: true,
                        }));
                    }
                }
            }
            if widgets::button_enabled(ui, "Elevations Only", 120.0, ready)
                .on_hover_text(
                    "Get the ground elevations of this area for Tc slopes and keep the background (it must be \
                     georeferenced)",
                )
                .clicked()
                && let Some((center, place)) = st.place.clone()
            {
                st.message = "Downloading the lidar elevations...".into();
                actions.push(WindowAction::Fetch(Fetch::Terrain {
                    center,
                    width_ft: st.width,
                    height_ft: st.height,
                    place,
                    background: false,
                }));
            }
            if widgets::button(ui, "Close", 80.0).clicked() {
                keep = false;
            }
            if widgets::button(ui, "Help", 70.0).clicked() {
                widgets::request_help(ui.ctx(), "site");
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep = false;
        }
    });
    keep
}

/// Two-point georeference: the latitude and longitude of two picked points.
pub struct GeorefState {
    pub a: Pos2,
    pub b: Pos2,
    pub text_a: String,
    pub text_b: String,
}

impl GeorefState {
    pub fn new(doc: &Doc, a: Pos2, b: Pos2) -> Self {
        // A background georeferenced before suggests the points' current places.
        let guess = |p: Pos2| {
            doc.project
                .diagram
                .underlay
                .as_ref()
                .and_then(|u| geo::to_lon_lat(u, [p.x as f64, p.y as f64]))
                .map(geo::format_lat_lon)
                .unwrap_or_default()
        };
        GeorefState { a, b, text_a: guess(a), text_b: guess(b) }
    }
}

pub fn georef(ctx: &egui::Context, doc: &mut Doc, st: &mut GeorefState, actions: &mut Vec<WindowAction>) -> bool {
    let mut keep = true;
    let u = doc.project.units;
    let Some(ul) = doc.project.diagram.underlay.clone() else { return false };
    crate::frame::Win::new("Georeference Background").collapsible(false).resizable(false).show(ctx, |ui| {
        let t = Tokens::get(ui.ctx());
        ui.label("Enter the latitude and longitude of the two points you picked, in decimal");
        ui.label("degrees (e.g. 42.0312, -93.6178). Property corners, road intersections or");
        ui.label("survey monuments read from a web map or a survey work well; far apart is best.");
        ui.add_space(4.0);
        widgets::text_field(ui, "Point 1 (latitude, longitude):", &mut st.text_a, 300.0);
        widgets::text_field(ui, "Point 2 (latitude, longitude):", &mut st.text_b, 300.0);
        let (a, b) = ([st.a.x as f64, st.a.y as f64], [st.b.x as f64, st.b.y as f64]);
        let parsed = (geo::parse_lat_lon(&st.text_a), geo::parse_lat_lon(&st.text_b));
        let solved = match parsed {
            (Some(ga), Some(gb)) => Some(geo::two_point(&ul, a, ga, b, gb)),
            _ => None,
        };
        match &solved {
            Some(Ok((g, fpu))) => {
                ui.label(format!("Scale: 1 diagram unit = {:.4} {}", u.len(*fpu), u.len_label()));
                ui.label(format!("North is {:.1}° from straight up on the background.", -g.bearing.to_degrees()));
                if let Some(old) = ul.feet_per_unit {
                    let diff = (fpu / old - 1.0) * 100.0;
                    let text = format!("This replaces the calibrated scale ({diff:+.1} %).");
                    if diff.abs() > 2.0 {
                        ui.colored_label(t.warning, format!("{text} Check the coordinates."));
                    } else {
                        ui.label(text);
                    }
                }
            }
            Some(Err(e)) => {
                ui.colored_label(t.warning, e);
            }
            None => {
                ui.label(RichText::new("Enter both points as \"latitude, longitude\".").color(t.text_dim));
            }
        }
        match ok_cancel_apply(ui, false, "site") {
            Buttons::Ok | Buttons::Apply => {
                if let Some(Ok((g, fpu))) = solved {
                    doc.edit("Georeference Background", |p| {
                        if let Some(ul) = &mut p.diagram.underlay {
                            ul.geo = Some(g);
                            ul.feet_per_unit = Some(fpu);
                        }
                        drawing::sync_all(p);
                    });
                    actions.push(WindowAction::Status("Background georeferenced; its scale is set too".into()));
                    keep = false;
                }
            }
            Buttons::Cancel => keep = false,
            Buttons::None => {}
        }
    });
    keep
}

/// The Soils window.
#[derive(Default)]
pub struct SoilsState {
    /// The area of interest being prepared (diagram units), before soils are fetched for it.
    pub aoi: Option<Vec<Pt>>,
    /// Treat dual groups as drained, for the next request.
    pub drained: bool,
    /// Progress, the last problem, or what Apply did.
    pub message: String,
}

impl SoilsState {
    pub fn new(doc: &Doc) -> Self {
        SoilsState { aoi: None, drained: doc.project.soils.as_ref().is_some_and(|m| m.drained), message: String::new() }
    }
}

/// Text describing what applying soils did.
pub fn applied_summary(done: &[soils::SubcatSoils], skipped: &[String]) -> String {
    let mut out =
        format!("Soil groups applied to {} subcatchment{}.", done.len(), if done.len() == 1 { "" } else { "s" });
    for d in done {
        if d.coverage < 0.99 {
            out.push_str(&format!(
                "\n{}: only {:.0} % of it is on rated mapped soil; the rest is shared among the groups found.",
                d.label,
                d.coverage * 100.0
            ));
        }
        if !d.custom.is_empty() {
            out.push_str(&format!("\n{}: kept as typed (not a TR-55 cover): {}.", d.label, d.custom.join("; ")));
        }
    }
    if !skipped.is_empty() {
        out.push_str(&format!("\nNo outline, so not placed on the soil map: {}.", skipped.join(", ")));
    }
    out
}

pub fn soils_window(ctx: &egui::Context, doc: &mut Doc, st: &mut SoilsState, actions: &mut Vec<WindowAction>) -> bool {
    let mut keep = true;
    let u = doc.project.units;
    crate::frame::Win::new("Soils").collapsible(false).resizable(false).show(ctx, |ui| {
        let t = Tokens::get(ui.ctx());
        let underlay = doc.project.diagram.underlay.clone();
        let placed = underlay.as_ref().is_some_and(|ul| ul.geo.is_some() && ul.feet_per_unit.is_some());
        if !placed {
            ui.colored_label(t.warning, "The background is not placed in the real world yet.");
            ui.label("Use Diagram > Background > Aerial Photo..., or Georeference Background");
            ui.label("to give two points of your own background their latitude and longitude.");
            ui.add_space(4.0);
        }
        let fpu = drawing::feet_per_unit(&doc.project);
        widgets::group(ui, "Area of interest", |ui| {
            let current: Option<Vec<Pt>> = st.aoi.clone().or_else(|| {
                let (m, ul) = (doc.project.soils.as_ref()?, underlay.as_ref()?);
                let to = geo::projector(ul)?;
                Some(m.aoi.iter().map(|q| to(*q)).collect())
            });
            match &current {
                Some(a) => ui.label(format!(
                    "{} area of interest: {}",
                    if st.aoi.is_some() { "New" } else { "Current" },
                    u.fmt_area(drawing::polygon_area(a) * fpu * fpu)
                )),
                None => {
                    ui.label(RichText::new("Not set: draw one, or use the subcatchment outlines.").color(t.text_dim))
                }
            };
            ui.horizontal(|ui| {
                if widgets::button_enabled(ui, "Draw...", 90.0, placed)
                    .on_hover_text("Click the corners of the area of interest on the diagram")
                    .clicked()
                {
                    actions.push(WindowAction::DrawAoi);
                }
                let around = soils::default_aoi(&doc.project);
                if widgets::button_enabled(ui, "Around Subcatchments", 160.0, placed && around.is_some())
                    .on_hover_text("All subcatchment outlines with a margin")
                    .clicked()
                {
                    st.aoi = around;
                }
            });
        });
        let aoi_ll: Option<Vec<LonLat>> = st.aoi.as_ref().and_then(|a| {
            let ul = underlay.as_ref()?;
            a.iter().map(|q| geo::to_lon_lat(ul, *q)).collect()
        });
        // With a soil map the setting is the map's (undoable); before one, it is kept for the request.
        let mut drained = doc.project.soils.as_ref().map_or(st.drained, |m| m.drained);
        if widgets::check(ui, &mut drained, "Site is drained")
            .on_hover_text("Dual groups (A/D, B/D, C/D) use their first letter when drained; otherwise group D (NRCS).")
            .changed()
        {
            st.drained = drained;
            if doc.project.soils.is_some() {
                doc.edit("Soils Drained", |p| {
                    if let Some(m) = &mut p.soils {
                        m.drained = drained;
                    }
                });
                st.message = "Drained setting changed: Apply to Subcatchments again to update them.".into();
            }
        }
        ui.horizontal(|ui| {
            let ll = aoi_ll.clone().or_else(|| doc.project.soils.as_ref().map(|m| m.aoi.clone()));
            if widgets::button_enabled(ui, "Get Soils", 100.0, placed && ll.is_some())
                .on_hover_text(
                    "Ask NRCS Soil Data Access (the data behind Web Soil Survey) for the soils of the area of interest",
                )
                .clicked()
                && let Some(aoi) = ll
            {
                st.message = "Asking NRCS Soil Data Access...".into();
                actions.push(WindowAction::Fetch(Fetch::Soils { aoi, drained }));
            }
            if widgets::button_enabled(ui, "Export AOI...", 110.0, aoi_ll.is_some() || doc.project.soils.is_some())
                .on_hover_text("Save the area of interest as a zipped shapefile for Web Soil Survey's Import AOI")
                .clicked()
            {
                let ring =
                    aoi_ll.clone().or_else(|| doc.project.soils.as_ref().map(|m| m.aoi.clone())).unwrap_or_default();
                actions.push(WindowAction::SaveFile {
                    name: "aoi.zip".into(),
                    bytes: wavestorm_core::shapefile::aoi_zip("aoi", &ring),
                    ext: "zip",
                });
            }
            if widgets::button(ui, "Web Soil Survey", 120.0).on_hover_text(WSS_URL).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(WSS_URL));
            }
        });
        if let Some(map) = doc.project.soils.clone() {
            ui.add_space(4.0);
            widgets::group(ui, "Map units", |ui| {
                ui.label(
                    RichText::new(format!("NRCS SSURGO via Soil Data Access, retrieved {}", map.retrieved))
                        .color(t.text_dim),
                );
                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                    egui::Grid::new("soil-units").striped(true).show(ui, |ui| {
                        ui.strong("Symbol");
                        ui.strong("Map unit name");
                        ui.strong("Rated");
                        ui.strong("Used");
                        ui.end_row();
                        for unit in &map.units {
                            let used = soils::effective_hsg(&unit.hydgrp, map.drained);
                            ui.label(&unit.symbol);
                            ui.label(&unit.name);
                            ui.label(if unit.hydgrp.is_empty() { "-" } else { &unit.hydgrp });
                            ui.colored_label(
                                crate::diagram::hsg_color(used),
                                used.map_or("-".to_string(), |g| g.letter().to_string()),
                            );
                            ui.end_row();
                        }
                    });
                });
                let mut visible = map.visible;
                if widgets::check(ui, &mut visible, "Show soils on the diagram").changed() {
                    doc.edit("Show Soils", |p| {
                        if let Some(m) = &mut p.soils {
                            m.visible = visible;
                        }
                    });
                }
                ui.add_enabled_ui(visible, |ui| {
                    widgets::check(ui, &mut doc.soil_snap, "Sub-areas snap to and follow soil boundaries")
                        .on_hover_text(
                            "While drawing a sub-area, points snap to soil boundaries; two points on the same \
                             boundary are joined along it. Press S while drawing to switch.",
                        );
                });
            });
            ui.horizontal(|ui| {
                if widgets::button_enabled(ui, "Apply to Subcatchments", 170.0, placed)
                    .on_hover_text(
                        "Split each outlined subcatchment's sub-areas by soil group, with TR-55 curve numbers",
                    )
                    .clicked()
                {
                    let mut result = Err(String::new());
                    doc.edit("Apply Soils", |p| result = soils::apply(p));
                    st.message = match result {
                        Ok((done, skipped)) => applied_summary(&done, &skipped),
                        Err(e) => e,
                    };
                    actions.push(WindowAction::RefreshDrafts);
                }
                if widgets::button(ui, "Remove Soils", 110.0).clicked() {
                    doc.edit("Remove Soils", |p| p.soils = None);
                    st.message = "Soil map removed (sub-areas are unchanged).".into();
                }
            });
        }
        if !st.message.is_empty() {
            ui.add_space(4.0);
            ui.label(&st.message);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if widgets::button(ui, "Close", 80.0).clicked() {
                keep = false;
            }
            if widgets::button(ui, "Help", 70.0).clicked() {
                widgets::request_help(ui.ctx(), "site");
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep = false;
        }
    });
    keep
}
