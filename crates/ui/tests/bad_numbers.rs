//! Clearing a number box stores NaN in the project, and users also type zeros, negative and
//! huge numbers. Every numeric field of a project with every kind of section, storage shape,
//! outlet device, Tc method and link source gets each such value in turn; the project must still
//! calculate, and every edit dialog and report tab must still draw, without a panic.
//!
//! Environment: `BAD_NUMBERS_FILTER=/section/Pipe` limits the run to fields whose JSON path
//! contains the text; `BAD_NUMBERS_THREADS=n` sets the worker threads; `BAD_NUMBERS_TRACE=1`
//! prints each case as it starts (with `--nocapture`), to find one that hangs or aborts.

use egui::vec2;
use serde_json::Value;
use wavestorm_core::cn::SubArea;
use wavestorm_core::geometry::{PipeShape, RatingPoint, Section, XsPoint};
use wavestorm_core::link::{LinkElevation, LinkSource, ManualHydrograph};
use wavestorm_core::outlets::{OutletDevice, Routing};
use wavestorm_core::project::*;
use wavestorm_core::storage::{CustomKind, PondStorage, StorageVolume, VolumeShape};
use wavestorm_core::tc::{TcKind, TcSegment};
use wavestorm_ui::dialogs::node::EditState;
use wavestorm_ui::dialogs::outlet::templates;
use wavestorm_ui::reports::{ReportState, Tab};
use wavestorm_ui::windows::Window;
use wavestorm_ui::{WaveStormApp, samples};

/// JSON pointers of the numbers in `v`. Long data tables (hydrograph ordinates, storm curves,
/// map pixels) are sampled at their first, second and last rows: every row is handled alike.
fn number_paths(v: &Value, path: String, out: &mut Vec<String>) {
    match v {
        Value::Number(_) => out.push(path),
        Value::Array(a) if a.len() > 3 && a.iter().all(|x| x.is_number() || x.is_array()) => {
            for i in [0, 1, a.len() - 1] {
                number_paths(&a[i], format!("{path}/{i}"), out);
            }
        }
        Value::Array(a) => a.iter().enumerate().for_each(|(i, x)| number_paths(x, format!("{path}/{i}"), out)),
        Value::Object(m) => m.iter().for_each(|(k, x)| number_paths(x, format!("{path}/{k}"), out)),
        _ => {}
    }
}

/// `base` with the number at `path` replaced by `bad` (NaN is saved as null, as a blank box is).
/// Integer fields (counts, indices, node ids) get `bad` as an integer when it is a whole
/// non-negative number, and are skipped otherwise.
fn poisoned(base: &Value, path: &str, bad: f64) -> Option<Project> {
    let mut v = base.clone();
    let slot = v.pointer_mut(path)?;
    *slot = if slot.is_u64() || slot.is_i64() {
        if !(bad >= 0.0 && bad.fract() == 0.0) {
            return None;
        }
        Value::from(bad as u64)
    } else {
        serde_json::Number::from_f64(bad).map_or(Value::Null, Value::Number)
    };
    wavestorm_core::json::from_value(v).ok()
}

/// The sample project plus nodes with every section, storage shape, outlet device, Tc method
/// and link source, so that each kind of number box is covered.
fn rich_project() -> Project {
    let mut p = samples::sample_project();
    let pos = |i: usize| [900.0 + 60.0 * i as f64, 600.0];
    let reach = |section: Section| Reach {
        section,
        n: 0.013,
        length: 300.0,
        inlet_invert: 100.0,
        outlet_invert: 99.0,
        exfiltration: 0.1,
        flood_depth: Some(1.0),
        auto_size: false,
    };
    let mut sections = vec![
        Section::Rectangular { width: 4.0, depth: 2.0 },
        Section::Parabolic { width: 6.0, depth: 1.5 },
        Section::Custom {
            points: vec![
                XsPoint { offset: 0.0, elev: 102.0, n: 0.03 },
                XsPoint { offset: 5.0, elev: 100.0, n: 0.03 },
                XsPoint { offset: 10.0, elev: 102.0, n: 0.03 },
            ],
        },
        Section::Rating {
            points: vec![
                RatingPoint { depth: 0.0, flow: 0.0, area: 0.0 },
                RatingPoint { depth: 1.0, flow: 10.0, area: 5.0 },
                RatingPoint { depth: 2.0, flow: 40.0, area: 12.0 },
            ],
            bank_full: 2.0,
        },
    ];
    for shape in PipeShape::ALL {
        sections.push(Section::Pipe { shape, width: 2.0, height: 1.5, fill: 0.1 });
    }
    let mut downstream = None;
    for (i, s) in sections.into_iter().enumerate() {
        let id = p.add_node(NodeType::Reach, pos(i));
        if let Some(n) = p.node_mut(id) {
            n.data = NodeData::Reach(reach(s));
            n.outflows[0] = downstream;
        }
        downstream = Some(id);
    }

    let pond = p.add_node(NodeType::Pond, pos(10));
    let mut volumes = vec![
        StorageVolume::new("Prism", 100.0, VolumeShape::Prismatoid { width: 20.0, length: 30.0, height: 4.0, z: 2.0 }),
        StorageVolume::new("Cone", 100.0, VolumeShape::VerticalCone { diameter: 20.0, height: 4.0, z: 1.0 }),
        StorageVolume::new("Arch", 100.0, VolumeShape::Arch { width: 4.0, height: 3.0, length: 50.0 }),
    ];
    for shape in PipeShape::ALL {
        volumes.push(StorageVolume::new(
            "Pipe",
            100.0,
            VolumeShape::Pipe { shape, width: 3.0, height: 2.0, length: 80.0, slope: 0.01 },
        ));
    }
    for kind in CustomKind::ALL {
        volumes.push(StorageVolume::new(
            "Custom",
            100.0,
            VolumeShape::Custom { kind, elevations: vec![100.0, 101.0, 102.0], values: vec![100.0, 200.0, 300.0] },
        ));
    }
    let devices = templates()
        .into_iter()
        .map(|(label, kind)| OutletDevice::new(label, 100.5, Routing::Primary, kind))
        .collect::<Vec<_>>();
    if let Some(n) = p.node_mut(pond) {
        if let NodeData::Pond(d) = &mut n.data {
            d.storage = PondStorage { volumes };
            d.devices = devices;
            d.start_elevation = Some(100.2);
            d.flood_elevation = Some(103.0);
        }
        n.outflows[0] = downstream;
    }

    let sub = p.add_node(NodeType::Subcat, pos(11));
    let kinds = vec![
        TcKind::Direct { minutes: 6.0, length: 100.0, velocity: 1.0 },
        TcKind::Sheet { length: 100.0, slope: 0.02, n: 0.24, p2: 3.1, surface: "Grass".into() },
        TcKind::SheetMcCuenSpiess { length: 100.0, slope: 0.02, n: 0.24, p2: 3.1, surface: "Grass".into() },
        TcKind::Shallow { length: 300.0, slope: 0.02, kv: 16.1345, surface: "Unpaved".into() },
        TcKind::Channel { length: 200.0, slope: 0.01, n: 0.013, section: Section::circular(1.5) },
        TcKind::Lag { length: 1000.0, slope: 0.02 },
        TcKind::Kirpich { length: 1000.0, slope: 0.02, factor: 1.0 },
        TcKind::Kerby { length: 300.0, slope: 0.02, retardance: 0.4 },
        TcKind::Lake { length: 500.0, mean_depth: 5.0 },
    ];
    if let Some(n) = p.node_mut(sub) {
        n.data = NodeData::Subcat(Subcatchment {
            areas: vec![SubArea::new(40_000.0, 80.0, "Lawn")],
            tc: kinds.into_iter().map(|k| TcSegment::new(k, "")).collect(),
            uh: None,
            large_units: false,
        });
        n.outflows[0] = Some(pond);
    }

    let link = p.add_node(NodeType::Link, pos(12));
    if let Some(n) = p.node_mut(link)
        && let NodeData::Link(l) = &mut n.data
    {
        l.source = LinkSource::Manual(ManualHydrograph {
            t0: 0.0,
            dt: 0.1,
            flows: vec![0.0, 5.0, 2.0, 0.0],
            per_event: Vec::new(),
        });
        l.adjust.threshold = Some(0.5);
        l.adjust.limit = Some(4.0);
        l.adjust.start_time = Some(0.0);
        l.adjust.end_time = Some(20.0);
        l.adjust.slice_volume = Some(1000.0);
        l.elevation = LinkElevation::Tidal { mean: 100.0, amplitude: 1.0, period: 12.0, phase: 0.0 };
        l.inflow_area = 10_000.0;
    }
    let link2 = p.add_node(NodeType::Link, pos(13));
    if let Some(n) = p.node_mut(link2)
        && let NodeData::Link(l) = &mut n.data
    {
        l.source = LinkSource::Table {
            name: "t".into(),
            times: vec![0.0, 1.0, 2.0],
            columns: vec![("a".into(), vec![0.0, 3.0, 0.0])],
            selected: None,
        };
        l.elevation = LinkElevation::Table(vec![(0.0, 100.0), (10.0, 101.0)]);
        n.outflows[0] = Some(link);
    }
    p
}

fn harness() -> egui_kittest::Harness<'static, WaveStormApp> {
    egui_kittest::Harness::builder()
        .with_size(vec2(1280.0, 900.0))
        .build_ui_state(|ui, app: &mut WaveStormApp| app.draw(ui), WaveStormApp::headless())
}

/// The node a JSON path is inside, with every node downstream of it (their results change too);
/// all nodes for a project-wide setting.
fn affected(p: &Project, path: &str) -> Vec<NodeId> {
    let index = path.strip_prefix("/nodes/").and_then(|r| r.split('/').next()).and_then(|i| i.parse::<usize>().ok());
    let Some(start) = index.and_then(|i| p.nodes.get(i)) else { return p.nodes.iter().map(|n| n.id).collect() };
    let mut out = vec![start.id];
    let mut k = 0;
    while let Some(id) = out.get(k).copied() {
        for next in p.node(id).map(|n| n.outflows).unwrap_or_default().into_iter().flatten() {
            if !out.contains(&next) {
                out.push(next);
            }
        }
        k += 1;
    }
    out
}

/// Calculates `p` and draws the edit dialog and the report of each `nodes`, all open at once
/// and switched through every tab.
fn exercise(h: &mut egui_kittest::Harness<'static, WaveStormApp>, p: &Project, nodes: &[NodeId]) {
    let doc = h.state_mut().doc();
    doc.project = p.clone();
    doc.all_results = None;
    doc.recalc();
    doc.windows.clear();
    for n in p.nodes.iter().filter(|n| nodes.contains(&n.id)) {
        doc.windows.push(Window::Edit(EditState::new(n)));
        doc.windows.push(Window::Report(ReportState::new(n.id)));
    }
    let tabs = [
        Tab::Summary,
        Tab::Hydrograph,
        Tab::Discharge,
        Tab::Storage,
        Tab::Events,
        Tab::Sizing,
        Tab::Profile,
        Tab::Tr55,
    ];
    for (i, tab) in tabs.into_iter().enumerate() {
        for table in [false, true] {
            for w in &mut h.state_mut().doc().windows {
                match w {
                    Window::Edit(e) => e.tab = i,
                    Window::Report(r) => {
                        r.tab = tab;
                        r.table = table;
                    }
                    _ => {}
                }
            }
            h.run_steps(1);
        }
    }
}

thread_local! {
    /// The message and location of the last panic on this thread.
    static LAST_PANIC: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Quick check run with every `cargo test`: blank (NaN) values in every eighth numeric field and
/// in every pipe field (the case first reported: clearing a pipe size in the reach dialog).
#[test]
fn blank_fields_never_panic() {
    sweep(&[f64::NAN], |i, path| i % 8 == 0 || path.contains("/Pipe/"));
}

/// The full sweep: blank, zero, negative and huge values in every numeric field. About ten
/// minutes on 16 threads, so it runs on request:
/// `cargo test -p wavestorm-ui --test bad_numbers -- --ignored`.
#[test]
#[ignore]
fn bad_numbers_never_panic() {
    sweep(&[f64::NAN, 0.0, -1.0, -1.0e6, 1.0e6], |_, _| true);
}

/// Puts each of `values` in turn into each numeric field whose index and JSON path pass `keep`
/// (and contain `BAD_NUMBERS_FILTER`, when set), and fails listing every case that panicked.
fn sweep(values: &[f64], keep: impl Fn(usize, &str) -> bool) {
    let mut project = rich_project();
    // One storm event keeps the sweep quick; the Events tabs still calculate it.
    project.settings.events.truncate(1);
    project.settings.current_event = 0;
    let base = serde_json::to_value(&project).expect("project serializes");
    // The project must come back intact through the reader that maps null to NaN.
    let back: Project = wavestorm_core::json::from_value(base.clone()).expect("project reads back");
    assert_eq!(serde_json::to_value(&back).expect("serializes"), base);

    let mut paths = Vec::new();
    number_paths(&base, String::new(), &mut paths);
    let filter = std::env::var("BAD_NUMBERS_FILTER").unwrap_or_default();
    let cases: Vec<(String, f64)> = paths
        .iter()
        .enumerate()
        .filter(|(i, p)| keep(*i, p) && p.contains(filter.as_str()))
        .flat_map(|(_, p)| values.iter().map(|bad| (p.clone(), *bad)))
        .collect();
    assert!(!cases.is_empty(), "no numeric field matched");

    // Panics are expected and reported together at the end, not printed one by one.
    std::panic::set_hook(Box::new(|info| {
        let msg = info
            .payload()
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| info.payload().downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default();
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        LAST_PANIC.with(|p| *p.borrow_mut() = format!("{msg} at {at}"));
    }));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failures = std::sync::Mutex::new(Vec::new());
    let trace = std::env::var_os("BAD_NUMBERS_TRACE").is_some();
    let threads = std::env::var("BAD_NUMBERS_THREADS")
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()))
        .min(cases.len());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut h = harness();
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((path, bad)) = cases.get(i) else { break };
                    let Some(p) = poisoned(&base, path, *bad) else { continue };
                    let nodes = affected(&p, path);
                    if trace {
                        eprintln!("CASE {path} = {bad}");
                    }
                    let started = std::time::Instant::now();
                    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let _ = wavestorm_core::calc::calculate(&p);
                        exercise(&mut h, &p, &nodes);
                    }))
                    .is_ok();
                    let secs = started.elapsed().as_secs_f64();
                    if secs > 20.0 {
                        eprintln!("SLOW {path} = {bad}: {secs:.1} s");
                    }
                    if trace {
                        eprintln!("DONE {path} = {bad}");
                    }
                    if !ok {
                        let why = LAST_PANIC.with(|p| p.borrow().clone());
                        if let Ok(mut f) = failures.lock() {
                            f.push(format!("{path} = {bad}: {why}"));
                        }
                        h = harness();
                    }
                }
            });
        }
    });
    let _ = std::panic::take_hook();
    let mut failures = failures.into_inner().unwrap_or_default();
    failures.sort();
    assert!(failures.is_empty(), "{} panics with:\n{}", failures.len(), failures.join("\n"));
}
