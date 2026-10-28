//! Windows must keep their size once opened: a layout that asks for slightly more room than
//! it has makes a resizable window creep wider (or taller) every frame.

use egui::vec2;
use wavestorm_ui::WaveStormApp;

/// Sizes of every floating window (area), by layer id.
fn sizes(ctx: &egui::Context) -> Vec<(egui::Id, egui::Vec2)> {
    ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| l.order == egui::Order::Middle)
            .filter_map(|l| m.area_rect(l.id).map(|r| (l.id, r.size())))
            .collect()
    })
}

#[test]
fn windows_do_not_creep_larger() {
    let scripts = [
        "compare",
        "report:1P;tab:1",
        "report:1R;tab:0",
        "report:1S;tab:4",
        "report:1P;tab:4",
        "project-report:1",
        "project-report:6",
        "messages",
        "storms",
        "storms-events",
        "uhs",
        "idf",
        "settings",
        "units",
        "general",
        "landuse",
        "lookup-open",
        "edit:1S",
        "edit:1R",
        "edit:1P;tab:1",
        "edit:1P;tab:2",
        "edit:1P;tab:1;chamber",
        "edit:1L;tab:1",
    ];
    let mut failures = Vec::new();
    for script in scripts {
        let mut h = egui_kittest::Harness::builder()
            .with_size(vec2(1400.0, 900.0))
            .build_ui_state(|ui, app: &mut WaveStormApp| app.draw(ui), WaveStormApp::headless());
        h.run_steps(2);
        for cmd in script.split(';') {
            wavestorm_ui::script(h.state_mut(), cmd);
        }
        h.run_steps(4);
        let before = sizes(&h.ctx);
        h.run_steps(60);
        let after = sizes(&h.ctx);
        for (id, s0) in &before {
            if let Some((_, s1)) = after.iter().find(|(i, _)| i == id)
                && (s1.x > s0.x + 1.0 || s1.y > s0.y + 1.0)
            {
                failures.push(format!(
                    "{script}: window grew from {:.0} x {:.0} to {:.0} x {:.0} px in 60 frames",
                    s0.x, s0.y, s1.x, s1.y
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
