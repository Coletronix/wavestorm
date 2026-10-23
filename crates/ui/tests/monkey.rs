//! Drives the whole UI with pseudo-random input to make sure nothing panics.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use wavestorm_ui::{WaveStormApp, script};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self, max: f32) -> f32 {
        (self.next() % 10_000) as f32 / 10_000.0 * max
    }
}

fn harness() -> egui_kittest::Harness<'static, WaveStormApp> {
    egui_kittest::Harness::builder().with_size(vec2(1280.0, 800.0)).build_ui_state(|ui, app: &mut WaveStormApp| app.draw(ui), WaveStormApp::headless())
}

fn press(h: &mut egui_kittest::Harness<'static, WaveStormApp>, pos: Pos2, button: PointerButton, modifiers: Modifiers) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.input_mut().events.push(Event::PointerButton { pos, button, pressed: true, modifiers });
    h.step();
    h.input_mut().events.push(Event::PointerButton { pos, button, pressed: false, modifiers });
    h.step();
}

fn drag(h: &mut egui_kittest::Harness<'static, WaveStormApp>, a: Pos2, b: Pos2, modifiers: Modifiers) {
    h.input_mut().events.push(Event::PointerMoved(a));
    h.input_mut().events.push(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers });
    h.step();
    for k in 1..=6 {
        let p = a + (b - a) * (k as f32 / 6.0);
        h.input_mut().events.push(Event::PointerMoved(p));
        h.step();
    }
    h.input_mut().events.push(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers });
    h.step();
}

#[test]
fn random_input_never_panics() {
    let windows = ["", "report:1P", "report:1S", "report:1R", "report:1L", "edit:1S", "edit:1P", "edit:1R", "edit:1L", "settings", "units", "messages", "project-report", "storms", "uhs", "compare", "general"];
    for (round, w) in windows.iter().enumerate() {
        let mut h = harness();
        h.run_steps(3);
        script(h.state_mut(), w);
        h.run_steps(2);
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (round as u64 + 1));
        for _ in 0..60 {
            let p = pos2(rng.f(1280.0), rng.f(800.0));
            match rng.next() % 9 {
                0 | 1 => press(&mut h, p, PointerButton::Primary, Modifiers::NONE),
                2 => press(&mut h, p, PointerButton::Secondary, Modifiers::NONE),
                3 => drag(&mut h, p, pos2(rng.f(1280.0), rng.f(800.0)), Modifiers::NONE),
                4 => drag(&mut h, p, pos2(rng.f(1280.0), rng.f(800.0)), Modifiers::COMMAND),
                5 => {
                    press(&mut h, p, PointerButton::Primary, Modifiers::NONE);
                    press(&mut h, p, PointerButton::Primary, Modifiers::NONE);
                }
                6 => {
                    h.input_mut().events.push(Event::PointerMoved(p));
                    h.input_mut().events.push(Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: vec2(0.0, if rng.next().is_multiple_of(2) { 2.0 } else { -2.0 }), modifiers: Modifiers::NONE, phase: egui::TouchPhase::Move });
                    h.step();
                }
                7 => {
                    let keys = [Key::Delete, Key::Escape, Key::Enter, Key::Z, Key::Y, Key::C, Key::V, Key::A, Key::Backspace];
                    let key = keys[(rng.next() % keys.len() as u64) as usize];
                    let modifiers = if rng.next().is_multiple_of(2) { Modifiers::COMMAND } else { Modifiers::NONE };
                    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
                    h.step();
                    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
                    h.step();
                }
                _ => {
                    h.input_mut().events.push(Event::Text(format!("{}", rng.next() % 1000)));
                    h.step();
                }
            }
        }
        // The project must still calculate after all that.
        let app = h.state_mut();
        let doc = app.doc();
        let _ = wavestorm_core::calc::calculate(&doc.project);
    }
}

#[test]
fn palette_drop_creates_a_node() {
    let mut h = harness();
    h.run_steps(3);
    let before = h.state_mut().doc().project.nodes.len();
    // Drag the Reach symbol from the palette onto the diagram.
    drag(&mut h, pos2(40.0, 143.0), pos2(640.0, 620.0), Modifiers::NONE);
    h.run_steps(2);
    let after = h.state_mut().doc().project.nodes.len();
    assert_eq!(after, before + 1, "palette drag should create a node");
    // Undo removes it again.
    let doc = h.state_mut().doc();
    assert!(doc.undo());
    assert_eq!(doc.project.nodes.len(), before);
}
