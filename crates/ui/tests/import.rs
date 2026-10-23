//! Importing a TR-20 deck through the file inbox opens it as a new project with its notes.

use egui::vec2;
use stormwaves_core::project::NodeType;
use stormwaves_ui::StormwavesApp;
use stormwaves_ui::platform::{Incoming, Purpose};
use stormwaves_ui::windows::Window;

const DECK: &str = "\
JOB TR-20
TITLE    SMALL TEST JOB
 3 STRUCT       01
 8          100.0       0.0      0.0
 8          102.0      10.0    200.0
 8          104.0      60.0    400.0
 9 ENDTBL
 6 RUNOFF 1 001        1 0.5                 80.0           0.5
 6 RUNOFF 1 002       2 0.3                 70.0           0.4
 6 ADDHYD 4 003    1 2 3
 6 RESVOR 2     01 3   4
 7 INCREM 6                              0.1
 7 COMPUT 7 001    003                    0.0            4.0             1.07 2     1   1
   ENDJOB 2
";

/// NOAA Atlas 14 point estimates (public domain), trimmed to three durations.
const NOAA: &str = "Point precipitation frequency estimates (inches)
NOAA Atlas 14 Volume 10 Version 3
Data type: Precipitation depth
Project area: Northeastern States

PRECIPITATION FREQUENCY ESTIMATES
by duration for ARI (years):, 1,2,5,10,25,50,100,200,500,1000
60-min:, 0.858,1.02,1.30,1.53,1.84,2.07,2.32,2.60,3.00,3.32
24-hr:, 2.22,2.76,3.65,4.39,5.41,6.17,6.98,7.90,9.24,10.4
2-day:, 2.57,3.18,4.17,4.99,6.13,6.97,7.88,8.97,10.6,12.1
";

#[test]
fn noaa_lookup_creates_events() {
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1280.0, 800.0)).build_ui_state(|ui, app: &mut StormwavesApp| app.draw(ui), StormwavesApp::headless());
    h.run_steps(2);
    stormwaves_ui::script(h.state_mut(), "settings");
    h.state_mut().platform.push(Incoming { purpose: Purpose::NoaaCsv, name: "site.csv".into(), path: None, bytes: NOAA.as_bytes().to_vec() });
    h.run_steps(3);
    let app = h.state_mut();
    let lookup = app.doc().windows.iter().find_map(|w| if let Window::EventLookup(s) = w { Some((s.row, s.checked.clone(), s.storm.clone())) } else { None });
    let (row, checked, storm) = lookup.expect("lookup window opened with the table");
    assert_eq!(row, Some(1), "24-hr row preselected");
    assert_eq!(checked.iter().filter(|c| **c).count(), 6, "2, 5, 10, 25, 50 and 100-year");
    let picked: Vec<usize> = checked.iter().enumerate().filter(|c| *c.1).map(|c| c.0).collect();
    let table = stormwaves_core::noaa::parse_pfds_csv(NOAA).unwrap();
    app.set_events(stormwaves_core::noaa::events(&table, 1, &picked, &storm), true);
    let names: Vec<(String, f64)> = app.doc().project.settings.events.iter().map(|e| (e.name.clone(), e.depth)).collect();
    assert_eq!(names.first(), Some(&("2-Year".to_string(), 2.76)));
    assert_eq!(names.len(), 6);
    // The open Calculation Settings draft follows, so its OK does not undo the lookup.
    let draft = app.doc().windows.iter().find_map(|w| if let Window::Settings(s) = w { Some(s.draft.events.len()) } else { None });
    assert_eq!(draft, Some(6));
    h.run_steps(2);
}

#[test]
fn tr20_import_opens_a_new_project() {
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1280.0, 800.0)).build_ui_state(|ui, app: &mut StormwavesApp| app.draw(ui), StormwavesApp::headless());
    h.run_steps(2);
    let before = h.state().docs.len();
    h.state_mut().platform.push(Incoming { purpose: Purpose::ImportTr20, name: "small.inp".into(), path: None, bytes: DECK.as_bytes().to_vec() });
    h.run_steps(3);
    let app = h.state_mut();
    assert_eq!(app.docs.len(), before + 1);
    let doc = app.doc();
    assert_eq!(doc.project.title, "SMALL TEST JOB");
    let count = |t: NodeType| doc.project.nodes.iter().filter(|n| n.node_type() == t).count();
    assert_eq!((count(NodeType::Subcat), count(NodeType::Link), count(NodeType::Pond)), (2, 1, 1));
    // Rain table 7 is not a standard table, so the import has a note and shows the notes window.
    assert!(doc.windows.iter().any(|w| matches!(w, Window::Notes(_))));
    assert!(!doc.results.has_errors());
}
