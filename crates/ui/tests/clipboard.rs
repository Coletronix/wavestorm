//! Copy and paste of diagram nodes: the platform's clipboard events and the right-click menus.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::kittest::Queryable;
use wavestorm_core::project::NodeType;
use wavestorm_ui::{WaveStormApp, script};

type H = egui_kittest::Harness<'static, WaveStormApp>;

fn harness() -> H {
    let mut h = egui_kittest::Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut WaveStormApp| app.draw(ui), WaveStormApp::headless());
    h.run_steps(3);
    script(h.state_mut(), "new");
    h.run_steps(2);
    h.state_mut().doc().windows.clear();
    h
}

/// A new project with one pond at diagram (300, 200), selected.
fn with_pond() -> H {
    let mut h = harness();
    let doc = h.state_mut().doc();
    let mut id = 0;
    doc.edit("Node Create", |p| id = p.add_node(NodeType::Pond, [300.0, 200.0]));
    doc.selection = vec![id];
    h.run_steps(2);
    // Keep the diagram clear of the calculation messages window.
    h.state_mut().doc().windows.clear();
    h.run_steps(2);
    h
}

fn move_to(h: &mut H, pos: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.step();
}

fn right_click(h: &mut H, pos: Pos2) {
    move_to(h, pos);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(2);
}

fn ponds(h: &mut H) -> Vec<[f64; 2]> {
    h.state_mut().doc().project.nodes.iter().filter(|n| n.node_type() == NodeType::Pond).map(|n| n.pos).collect()
}

/// What the app last put on the system clipboard.
fn copied_text(h: &H) -> Option<String> {
    h.output().platform_output.commands.iter().rev().find_map(|c| match c {
        egui::OutputCommand::CopyText(t) => Some(t.clone()),
        _ => None,
    })
}

#[test]
fn ctrl_c_and_ctrl_v_copy_and_paste_nodes() {
    let mut h = with_pond();
    // The desktop delivers Ctrl+C / Ctrl+V as clipboard events, not key presses.
    h.input_mut().events.push(Event::Copy);
    h.step();
    let text = copied_text(&h).expect("nodes put on the system clipboard");
    assert!(text.contains("wavestorm-nodes"));
    h.run_steps(2);

    // Paste with the pointer on the diagram: the copy is centred on the pointer.
    let target = pos2(500.0, 350.0);
    let s = h.state_mut().world_to_screen(target);
    move_to(&mut h, s);
    h.input_mut().events.push(Event::Paste(text));
    h.run_steps(2);
    let p = ponds(&mut h);
    assert_eq!(p.len(), 2, "pasted a copy");
    assert!((p[1][0] - 500.0).abs() < 2.0 && (p[1][1] - 350.0).abs() < 2.0, "pasted at {:?}", p[1]);
    assert_eq!(h.state_mut().doc().selection.len(), 1, "the copy is selected");

    // Other text on the system clipboard still pastes what was copied here.
    h.input_mut().events.push(Event::Paste("some other text".into()));
    h.run_steps(2);
    assert_eq!(ponds(&mut h).len(), 3);

    // Cut removes the selection; an empty selection keeps the copied nodes.
    h.input_mut().events.push(Event::Cut);
    h.run_steps(2);
    assert_eq!(ponds(&mut h).len(), 2);
    h.input_mut().events.push(Event::Copy);
    h.run_steps(2);
    h.input_mut().events.push(Event::Paste("x".into()));
    h.run_steps(2);
    assert_eq!(ponds(&mut h).len(), 3);
}

#[test]
fn right_click_empty_space_offers_paste_there() {
    let mut h = with_pond();
    // Nothing copied yet: Paste is offered but disabled.
    let empty = h.state_mut().world_to_screen(pos2(600.0, 400.0));
    right_click(&mut h, empty);
    h.get_by_label("Create Here");
    h.get_by_label("Paste Ctrl+V").click();
    h.run_steps(2);
    assert_eq!(ponds(&mut h).len(), 1, "disabled Paste does nothing");
    h.input_mut().events.push(Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);

    // Copy from the pond's own menu, then paste from the empty-space menu.
    let pond = h.state_mut().world_to_screen(pos2(300.0, 200.0));
    right_click(&mut h, pond);
    h.get_by_label("Copy Ctrl+C").click();
    h.run_steps(2);
    right_click(&mut h, empty);
    h.get_by_label("Paste Ctrl+V").click();
    h.run_steps(2);
    let p = ponds(&mut h);
    assert_eq!(p.len(), 2);
    assert!((p[1][0] - 600.0).abs() < 2.0 && (p[1][1] - 400.0).abs() < 2.0, "pasted at {:?}", p[1]);
}

#[test]
fn right_click_empty_space_creates_a_node_there() {
    let mut h = harness();
    let at = h.state_mut().world_to_screen(pos2(250.0, 150.0));
    right_click(&mut h, at);
    h.get_by_label("Create Here").click();
    h.run_steps(3);
    h.get_by_label(NodeType::Reach.label()).click();
    h.run_steps(2);
    let doc = h.state_mut().doc();
    let reach = doc.project.nodes.iter().find(|n| n.node_type() == NodeType::Reach).expect("reach created");
    assert!((reach.pos[0] - 250.0).abs() < 2.0 && (reach.pos[1] - 150.0).abs() < 2.0, "at {:?}", reach.pos);
}

#[test]
fn clicking_elsewhere_closes_the_menu() {
    let mut h = harness();
    let at = h.state_mut().world_to_screen(pos2(250.0, 150.0));
    right_click(&mut h, at);
    assert!(h.query_by_label("Create Here").is_some());
    let away = h.state_mut().world_to_screen(pos2(450.0, 300.0));
    move_to(&mut h, away);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton {
            pos: away,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(2);
    assert!(h.query_by_label("Create Here").is_none());
}

#[test]
fn pasted_subcatchment_keeps_its_drawings_in_place() {
    use wavestorm_core::cn::SubArea;
    use wavestorm_core::project::NodeData;
    let mut h = harness();
    let doc = h.state_mut().doc();
    let mut id = 0;
    doc.edit("Node Create", |p| {
        id = p.add_node(NodeType::Subcat, [150.0, 150.0]);
        let n = p.node_mut(id).unwrap();
        n.outline = Some(vec![[100.0, 100.0], [200.0, 100.0], [200.0, 200.0], [100.0, 200.0]]);
        n.tc_path = Some(vec![[110.0, 110.0], [190.0, 190.0]]);
        let NodeData::Subcat(sc) = &mut n.data else { panic!() };
        let mut drawn = SubArea::new(1000.0, 98.0, "Paved");
        drawn.outline = Some(vec![[120.0, 120.0], [140.0, 120.0], [140.0, 140.0]]);
        sc.areas.push(drawn);
    });
    doc.selection = vec![id];
    h.run_steps(2);
    h.state_mut().doc().windows.clear();
    h.run_steps(2);
    h.input_mut().events.push(Event::Copy);
    h.run_steps(2);
    let s = h.state_mut().world_to_screen(pos2(450.0, 350.0));
    move_to(&mut h, s);
    h.input_mut().events.push(Event::Paste("x".into()));
    h.run_steps(2);

    let doc = h.state_mut().doc();
    let subs: Vec<_> = doc.project.nodes.iter().filter(|n| n.node_type() == NodeType::Subcat).collect();
    assert_eq!(subs.len(), 2);
    let (a, b) = (subs[0], subs[1]);
    let (dx, dy) = (b.pos[0] - a.pos[0], b.pos[1] - a.pos[1]);
    assert!(dx > 200.0 && dy > 150.0, "pasted elsewhere: moved by {dx}, {dy}");
    let moved =
        |o: &Option<Vec<[f64; 2]>>| o.as_ref().unwrap().iter().map(|q| [q[0] + dx, q[1] + dy]).collect::<Vec<_>>();
    assert_eq!(b.outline.clone().unwrap(), moved(&a.outline));
    assert_eq!(b.tc_path.clone().unwrap(), moved(&a.tc_path));
    let drawn = |n: &wavestorm_core::project::Node| match &n.data {
        NodeData::Subcat(sc) => sc.areas.iter().find_map(|r| r.outline.clone()),
        _ => None,
    };
    let (da, db) = (drawn(a), drawn(b));
    assert_eq!(db.unwrap(), moved(&da), "sub-area moved with its subcatchment");
}
