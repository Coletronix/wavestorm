//! What saving a project in the interchange format (`.hcp`) would lose.
//!
//! The format has no place for much of a WAVEStorm project (the background image, drawings,
//! notes, many settings), and the exporter grows over time, so rather than keep a list by hand
//! [`losses`] writes the project, reads it back and reports whatever did not survive.

use crate::project::{Node, Project};
use serde_json::Value;
use std::collections::BTreeMap;

/// Plain-language descriptions of what an `.hcp` save of `project` would not keep, one per
/// kind, each naming the nodes affected. Empty when everything survives.
pub fn losses(project: &Project) -> Vec<String> {
    let mut out = Vec::new();
    // Known to have no place in the format, and large: reported directly, left out of the
    // comparison.
    let mut p = project.clone();
    if p.diagram.underlay.take().is_some() {
        out.push("The background image".to_string());
    }
    if p.soils.take().is_some() {
        out.push("The soil map".to_string());
    }
    if p.terrain.take().is_some() {
        out.push("Ground elevations (terrain)".to_string());
    }
    let back = match crate::hcp::import_bytes(&crate::hcp::export_bytes(&p)) {
        Ok((q, _)) => q,
        Err(_) => {
            out.push("The project could not be read back from the interchange format".to_string());
            return out;
        }
    };
    let (Ok(a), Ok(b)) = (serde_json::to_value(&p), serde_json::to_value(&back)) else { return out };
    let mut found = Found::default();
    // Nodes are compared one by one (by position in the list) below.
    for (k, v) in a.as_object().into_iter().flatten() {
        if IGNORED.contains(&k.as_str()) || k == "nodes" {
            continue;
        }
        compare(&[k.as_str()], v, b.get(k), &mut |path| found.project(path));
    }
    if p.nodes.len() != back.nodes.len() {
        found.add("Some nodes", None);
    }
    let (an, bn) = (a.get("nodes").and_then(Value::as_array), b.get("nodes").and_then(Value::as_array));
    for (i, (n, q)) in p.nodes.iter().zip(&back.nodes).enumerate() {
        let (Some(av), Some(bv)) = (an.and_then(|x| x.get(i)), bn.and_then(|x| x.get(i))) else { continue };
        for (k, v) in av.as_object().into_iter().flatten() {
            if NODE_IGNORED.contains(&k.as_str()) || (k == "name" && n.node_type() == crate::project::NodeType::Text) {
                continue;
            }
            compare(&[k.as_str()], v, bv.get(k), &mut |path| found.add(&node_feature(path), Some(n.label())));
        }
        if outflow_labels(&p, n) != outflow_labels(&back, q) {
            found.add("Routing", Some(n.label()));
        }
    }
    out.extend(found.lines());
    out
}

/// Top-level fields that are bookkeeping, or handled elsewhere: `foreign` holds what the
/// interchange file carried that this model does not; storms and unit hydrographs are named in
/// the file and come back from the rainfall library when it is opened.
const IGNORED: &[&str] = &["format", "next_id", "foreign", "saved_by", "storms", "uhs"];
const NODE_IGNORED: &[&str] = &["id", "number", "outflows", "foreign"];

fn outflow_labels(p: &Project, n: &Node) -> Vec<Option<String>> {
    n.outflows.iter().map(|o| o.and_then(|id| p.node(id)).map(Node::label)).collect()
}

/// Calls `lost` with the path of each part of `a` that `b` does not match.
fn compare(path: &[&str], a: &Value, b: Option<&Value>, lost: &mut dyn FnMut(&[&str])) {
    match (a, b) {
        (_, None) => {
            if !is_empty(a) {
                lost(path);
            }
        }
        (Value::Object(x), Some(Value::Object(y))) => {
            for (k, v) in x {
                let mut p = path.to_vec();
                p.push(k);
                compare(&p, v, y.get(k), lost);
            }
        }
        (Value::Array(x), Some(Value::Array(y))) if x.len() == y.len() => {
            for (v, w) in x.iter().zip(y) {
                compare(path, v, Some(w), lost);
            }
        }
        (Value::Number(x), Some(Value::Number(y))) => {
            let (x, y) = (x.as_f64().unwrap_or(0.0), y.as_f64().unwrap_or(0.0));
            if (x - y).abs() > 1e-6 * x.abs().max(1.0) {
                lost(path);
            }
        }
        (a, Some(b)) => {
            if a != b {
                lost(path);
            }
        }
    }
}

fn is_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.values().all(is_empty),
    }
}

/// Lost things by description, with the nodes each affects.
#[derive(Default)]
struct Found(BTreeMap<String, Vec<String>>);

impl Found {
    fn add(&mut self, what: &str, node: Option<String>) {
        let nodes = self.0.entry(what.to_string()).or_default();
        if let Some(n) = node
            && !nodes.contains(&n)
        {
            nodes.push(n);
        }
    }

    fn project(&mut self, path: &[&str]) {
        let what = match path {
            ["title", ..] => "The project title".to_string(),
            ["notes", ..] => "Project notes".to_string(),
            ["prepared_by", ..] => "\"Prepared by\"".to_string(),
            ["units", ..] => "Unit display settings (Settings > Units)".to_string(),
            ["diagram", ..] => "Diagram settings (symbol size, grid)".to_string(),
            ["land_use", ..] => "Land-use settings".to_string(),
            ["settings", "events", ..] => "Rainfall event settings".to_string(),
            ["settings", field, ..] => format!("Calculation setting: {}", setting_name(field)),
            [other, ..] => pretty(other),
            [] => return,
        };
        self.add(&what, None);
    }

    fn lines(self) -> Vec<String> {
        self.0
            .into_iter()
            .map(|(what, nodes)| if nodes.is_empty() { what } else { format!("{what} ({})", nodes.join(", ")) })
            .collect()
    }
}

fn node_feature(path: &[&str]) -> String {
    match path {
        ["notes", ..] => "Node notes".into(),
        ["outline", ..] => "Drawn subcatchment outlines".into(),
        ["tc_path" | "tc_path_breaks", ..] => "Tc flow paths drawn on the diagram".into(),
        ["locked", ..] => "Node locks".into(),
        ["name", ..] => "Node names".into(),
        ["pos", ..] => "Node positions".into(),
        ["data", "Subcat", "areas", "outline" | "remainder", ..] => "Drawn sub-areas".into(),
        ["data", "Text", ..] => "Text label style".into(),
        ["data", kind, field, ..] => format!("{} {}", node_kind(kind), data_name(field)),
        _ => "Node details".into(),
    }
}

fn node_kind(kind: &str) -> &str {
    match kind {
        "Subcat" => "Subcatchment",
        other => other,
    }
}

fn data_name(field: &str) -> String {
    match field {
        "areas" => "areas and CN".into(),
        "tc" => "Tc segments".into(),
        "devices" => "outlet devices".into(),
        "storage" => "storage".into(),
        "section" => "cross-section".into(),
        other => pretty(other),
    }
}

fn setting_name(field: &str) -> String {
    match field {
        "ia_ratio" => "Ia/S ratio".into(),
        "round_cn" => "round CN".into(),
        "min_tc" => "minimum Tc".into(),
        "cn_weighting" => "CN weighting".into(),
        "rational_rise" | "rational_fall" => "Rational hydrograph shape".into(),
        "idf" => "IDF curve".into(),
        "table_step" => "table step".into(),
        "report_impervious" => "report impervious areas".into(),
        "resize" => "pipe resizing".into(),
        other => pretty(other),
    }
}

fn pretty(field: &str) -> String {
    field.replace('_', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{NodeType, Underlay};

    /// A project of plain nodes, the kind the interchange format holds completely.
    fn plain() -> Project {
        let mut p = Project::default();
        let s = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let r = p.add_node(NodeType::Pond, [120.0, 0.0]);
        if let Some(n) = p.node_mut(s) {
            n.outflows[0] = Some(r);
            n.name = "Site".into();
        }
        p
    }

    #[test]
    fn a_plain_project_loses_nothing() {
        assert_eq!(losses(&plain()), Vec::<String>::new());
    }

    #[test]
    fn wavestorm_only_features_are_reported() {
        let mut p = plain();
        p.notes = "Design notes".into();
        p.diagram.show_grid = true;
        p.settings.ia_ratio = 0.05;
        p.diagram.underlay = Some(Underlay {
            file_name: "site.png".into(),
            bytes: vec![1, 2, 3],
            page: 0,
            origin: [0.0, 0.0],
            scale: 1.0,
            rotation: 0.0,
            feet_per_unit: None,
            opacity: 0.5,
            visible: true,
            locked: false,
            geo: None,
        });
        let s = p.nodes[0].id;
        if let Some(n) = p.node_mut(s) {
            n.notes = "Check the CN".into();
            n.outline = Some(vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]);
        }
        let l = losses(&p);
        for want in [
            "The background image",
            "Project notes",
            "Diagram settings (symbol size, grid)",
            "Calculation setting: Ia/S ratio",
            "Node notes (1S)",
            "Drawn subcatchment outlines (1S)",
        ] {
            assert!(l.iter().any(|x| x == want), "missing {want:?} in {l:?}");
        }
    }
}
