//! Copying and pasting diagram nodes.
//!
//! Copied nodes are kept by the app and also written to the system clipboard as tagged JSON, so
//! they paste into another WAVEStorm window too. Writing text there also means the platform
//! always has something to paste, which is what turns Ctrl+V into a paste event on the desktop.

use egui::Event;
use serde::{Deserialize, Serialize};
use wavestorm_core::project::Node;

/// Gap between copies pasted without a target point and their originals (diagram units).
pub const PASTE_OFFSET: f64 = 30.0;

#[derive(Serialize, Deserialize)]
struct ClipNodes {
    #[serde(rename = "wavestorm-nodes")]
    nodes: Vec<Node>,
}

/// The system-clipboard text for copied nodes.
pub fn to_text(nodes: &[Node]) -> String {
    serde_json::to_string(&ClipNodes { nodes: nodes.to_vec() }).unwrap_or_default()
}

/// Nodes from system-clipboard text written by [`to_text`]; `None` for any other text.
pub fn from_text(text: &str) -> Option<Vec<Node>> {
    if !text.trim_start().starts_with('{') || !text.contains("\"wavestorm-nodes\"") {
        return None;
    }
    wavestorm_core::json::from_str::<ClipNodes>(text).ok().map(|c| c.nodes).filter(|n| !n.is_empty())
}

/// How far to move copies of `nodes`: so their centre lands on `at`, or down-right of the
/// originals when there is no target point.
pub fn paste_shift(nodes: &[Node], at: Option<[f64; 2]>) -> [f64; 2] {
    match at {
        Some(at) if !nodes.is_empty() => {
            let k = nodes.len() as f64;
            let cx = nodes.iter().map(|n| n.pos[0]).sum::<f64>() / k;
            let cy = nodes.iter().map(|n| n.pos[1]).sum::<f64>() / k;
            [at[0] - cx, at[1] - cy]
        }
        _ => [PASTE_OFFSET, PASTE_OFFSET],
    }
}

/// A cut, copy or paste request from the keyboard.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Copy,
    Cut,
    /// With the system clipboard's text when the platform supplied it.
    Paste(Option<String>),
}

/// This frame's cut/copy/paste shortcut, if any.
///
/// On the desktop the windowing layer turns Ctrl+X/C/V (and the platform's other clipboard
/// keys) into clipboard events instead of key presses. In the browser the key presses arrive
/// (the matching clipboard events follow later, and only with text), so read the keys there.
pub fn command(ctx: &egui::Context) -> Option<Command> {
    if cfg!(target_arch = "wasm32") {
        ctx.input(|i| {
            let cmd = i.modifiers.command && !i.modifiers.shift && !i.modifiers.alt;
            if !cmd {
                None
            } else if i.key_pressed(egui::Key::C) {
                Some(Command::Copy)
            } else if i.key_pressed(egui::Key::X) {
                Some(Command::Cut)
            } else if i.key_pressed(egui::Key::V) {
                Some(Command::Paste(None))
            } else {
                None
            }
        })
    } else {
        ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                Event::Copy => Some(Command::Copy),
                Event::Cut => Some(Command::Cut),
                Event::Paste(text) => Some(Command::Paste(Some(text.clone()))),
                _ => None,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavestorm_core::project::{NodeType, Project};

    fn two_nodes() -> Vec<Node> {
        let mut p = Project::default();
        let a = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let b = p.add_node(NodeType::Pond, [100.0, 50.0]);
        p.node_mut(a).unwrap().outflows[0] = Some(b);
        p.nodes.clone()
    }

    #[test]
    fn clipboard_text_round_trips() {
        let nodes = two_nodes();
        let text = to_text(&nodes);
        assert_eq!(from_text(&text), Some(nodes));
    }

    #[test]
    fn other_text_is_not_nodes() {
        assert_eq!(from_text("hello"), None);
        assert_eq!(from_text("{\"a\": 1}"), None);
        assert_eq!(from_text("{\"wavestorm-nodes\": []}"), None);
    }

    #[test]
    fn paste_centres_on_the_target_point() {
        let nodes = two_nodes();
        assert_eq!(paste_shift(&nodes, Some([250.0, 125.0])), [200.0, 100.0]);
        assert_eq!(paste_shift(&nodes, None), [PASTE_OFFSET, PASTE_OFFSET]);
    }
}
