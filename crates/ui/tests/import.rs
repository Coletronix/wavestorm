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
 8          102.0      10.0      2.0
 8          104.0      60.0      4.0
 9 ENDTBL
 6 RUNOFF 1 001        1 0.5                 80.0           0.5
 6 RUNOFF 1 002        2 0.3                 70.0           0.4
 6 ADDHYD 4 003    1 2 3
 6 RESVOR 2     01 3   4
 7 INCREM 6                              0.1
 7 COMPUT 7 001    003                    0.0            4.0             1.07 2     1   1
   ENDJOB 2
";

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
