//! Windows must keep their size once opened: a layout that asks for slightly more width than
//! it has makes a resizable window creep wider every frame.

use egui::vec2;
use wavestorm_ui::WaveStormApp;

/// Widths of every floating window (area), by layer id.
fn widths(ctx: &egui::Context) -> Vec<(egui::Id, f32)> {
    ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| l.order == egui::Order::Middle)
            .filter_map(|l| m.area_rect(l.id).map(|r| (l.id, r.width())))
            .collect()
    })
}

#[test]
fn windows_do_not_creep_wider() {
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
        let mut h = egui_kittest::Harness::builder().with_size(vec2(1400.0, 900.0)).build_ui_state(|ui, app: &mut WaveStormApp| app.draw(ui), WaveStormApp::headless());
        h.run_steps(2);
        for cmd in script.split(';') {
            wavestorm_ui::script(h.state_mut(), cmd);
        }
        h.run_steps(4);
        let before = widths(&h.ctx);
        h.run_steps(60);
        let after = widths(&h.ctx);
        for (id, w0) in &before {
            if let Some((_, w1)) = after.iter().find(|(i, _)| i == id)
                && *w1 > w0 + 1.0
            {
                failures.push(format!("{script}: window grew from {w0:.0} to {w1:.0} px in 60 frames"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
