//! Data exchange: sub-area tables as CSV, and merging projects.

use crate::cn::SubArea;
use crate::project::{NodeData, NodeId, NodeType, Project};
use std::collections::HashMap;

fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Every sub-area of every subcatchment: number, name, area (in the project's area units), CN,
/// description, land use.
pub fn export_subareas(project: &Project) -> String {
    let u = &project.units;
    let mut out = format!("Subcatchment,Name,Area ({}),CN,Description,Land Use\n", u.area_label());
    for n in &project.nodes {
        if let NodeData::Subcat(sc) = &n.data {
            for a in &sc.areas {
                out.push_str(&format!(
                    "{},{},{},{},{},{}\n",
                    csv_field(&n.number),
                    csv_field(&n.name),
                    u.area(a.area),
                    a.cn,
                    csv_field(&a.description),
                    csv_field(a.land_use.as_deref().unwrap_or(""))
                ));
            }
        }
    }
    out
}

/// Replaces the sub-areas of subcatchments listed in `text` (created when missing).
/// Returns (subcatchments created, subcatchments updated).
pub fn import_subareas(project: &mut Project, text: &str) -> Result<(usize, usize), String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = lines.next().ok_or("empty file")?;
    let heads: Vec<String> = split_csv(header).into_iter().map(|h| h.to_ascii_lowercase()).collect();
    let col = |name: &str| heads.iter().position(|h| h.starts_with(name));
    let (Some(c_num), Some(c_area), Some(c_cn)) = (col("subcat"), col("area"), col("cn")) else {
        return Err("expected columns Subcatchment, Area and CN".into());
    };
    let (c_name, c_desc, c_lu) = (col("name"), col("desc"), col("land"));
    // Area units from the header, defaulting to the project's.
    let area_head = heads.get(c_area).cloned().unwrap_or_default();
    let factor = if area_head.contains("(ac") {
        43_560.0
    } else if area_head.contains("(sf") || area_head.contains("sq-ft") {
        1.0
    } else if area_head.contains("(ha") {
        crate::units::SQFT_PER_HA
    } else if area_head.contains("(m") {
        crate::units::FT_PER_M * crate::units::FT_PER_M
    } else {
        project.units.area_in(1.0)
    };
    let mut groups: Vec<(String, String, Vec<SubArea>)> = Vec::new();
    for (k, line) in lines.enumerate() {
        let cells = split_csv(line);
        let get = |i: Option<usize>| i.and_then(|i| cells.get(i)).map(|s| s.trim().to_string()).unwrap_or_default();
        let number = get(Some(c_num));
        if number.is_empty() {
            continue;
        }
        let area: f64 =
            get(Some(c_area)).replace(',', "").parse().map_err(|_| format!("row {}: invalid area", k + 2))?;
        let cn: f64 = get(Some(c_cn)).parse().map_err(|_| format!("row {}: invalid CN", k + 2))?;
        if !(0.0..=100.0).contains(&cn) || !area.is_finite() || area < 0.0 {
            return Err(format!("row {}: area or CN out of range", k + 2));
        }
        let mut a = SubArea::new(area * factor, cn, &get(c_desc));
        a.land_use = Some(get(c_lu)).filter(|s| !s.is_empty());
        match groups.iter_mut().find(|g| g.0 == number) {
            Some(g) => g.2.push(a),
            None => groups.push((number, get(c_name), vec![a])),
        }
    }
    let (mut created, mut updated) = (0, 0);
    for (number, name, areas) in groups {
        let existing =
            project.nodes.iter().find(|n| n.node_type() == NodeType::Subcat && n.number == number).map(|n| n.id);
        let id = match existing {
            Some(id) => {
                updated += 1;
                id
            }
            None => {
                created += 1;
                let x = 60.0 + 90.0 * (created % 8) as f64;
                let y = 60.0 + 90.0 * (created / 8) as f64;
                let id = project.add_node(NodeType::Subcat, [x, y]);
                if let Some(n) = project.node_mut(id) {
                    n.number = number.clone();
                }
                id
            }
        };
        if let Some(n) = project.node_mut(id) {
            if n.locked {
                continue;
            }
            if !name.is_empty() {
                n.name = name;
            }
            if let NodeData::Subcat(sc) = &mut n.data {
                sc.areas = areas;
            }
        }
    }
    Ok((created, updated))
}

/// Adds every node of `other` to `project`, renumbering nodes whose labels clash and keeping
/// routing between the added nodes. Returns the number of nodes added.
pub fn merge(project: &mut Project, other: &Project) -> usize {
    let mut map: HashMap<NodeId, NodeId> = HashMap::new();
    let dx = project.nodes.iter().map(|n| n.pos[0]).fold(0.0, f64::max) + 150.0;
    for n in &other.nodes {
        let id = project.add_node(n.node_type(), [n.pos[0] + dx, n.pos[1]]);
        let clash = project.nodes.iter().any(|m| m.id != id && m.node_type() == n.node_type() && m.number == n.number);
        let number = project.node(id).map(|m| m.number.clone()).unwrap_or_default();
        if let Some(m) = project.node_mut(id) {
            m.number = if clash { number } else { n.number.clone() };
            m.name = n.name.clone();
            m.notes = n.notes.clone();
            m.data = n.data.clone();
            m.outline = n.outline.as_ref().map(|o| o.iter().map(|p| [p[0] + dx, p[1]]).collect());
        }
        map.insert(n.id, id);
    }
    for n in &other.nodes {
        let Some(&id) = map.get(&n.id) else { continue };
        let outs: Vec<Option<NodeId>> = n.outflows.iter().map(|o| o.and_then(|t| map.get(&t).copied())).collect();
        if let Some(m) = project.node_mut(id) {
            for (k, o) in outs.into_iter().enumerate() {
                if let Some(slot) = m.outflows.get_mut(k) {
                    *slot = o;
                }
            }
        }
    }
    for s in &other.storms {
        if !project.storms.iter().any(|t| t.name == s.name) {
            project.storms.push(s.clone());
        }
    }
    map.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subareas_round_trip() {
        let mut p = Project::default();
        let id = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        if let Some(n) = p.node_mut(id) {
            n.name = "Lots, north".into();
            n.data = NodeData::Subcat(crate::project::Subcatchment {
                areas: vec![
                    SubArea::new(2.0 * 43_560.0, 75.0, "1/4 acre lots, 38% imp, HSG B"),
                    SubArea::new(43_560.0, 98.0, "Roofs, \"flat\", HSG B"),
                ],
                ..Default::default()
            });
        }
        let csv = export_subareas(&p);
        let mut q = Project::default();
        let (created, updated) = import_subareas(&mut q, &csv).unwrap();
        assert_eq!((created, updated), (1, 0));
        let NodeData::Subcat(sc) = &q.nodes[0].data else { panic!() };
        assert_eq!(sc.areas.len(), 2);
        assert!((sc.areas[0].area - 2.0 * 43_560.0).abs() < 1e-6);
        assert_eq!(sc.areas[1].description, "Roofs, \"flat\", HSG B");
        assert_eq!(q.nodes[0].name, "Lots, north");
        // Re-import updates in place.
        assert_eq!(import_subareas(&mut q, &csv).unwrap(), (0, 1));
        assert!(import_subareas(&mut q, "a,b\n1,2").is_err());
        assert!(import_subareas(&mut q, "Subcatchment,Area (ac),CN\n1,x,70").is_err());
    }

    #[test]
    fn merge_renumbers_and_keeps_routing() {
        let mut a = Project::default();
        a.add_node(NodeType::Subcat, [0.0, 0.0]);
        let mut b = Project::default();
        let s = b.add_node(NodeType::Subcat, [0.0, 0.0]);
        let r = b.add_node(NodeType::Reach, [100.0, 0.0]);
        b.node_mut(s).unwrap().outflows[0] = Some(r);
        assert_eq!(merge(&mut a, &b), 2);
        let labels: Vec<String> = a.nodes.iter().map(|n| n.label()).collect();
        assert!(
            labels.contains(&"1S".to_string())
                && labels.contains(&"2S".to_string())
                && labels.contains(&"1R".to_string())
        );
        let new_s = a.by_label("2S").unwrap();
        assert_eq!(new_s.outflows[0], a.by_label("1R").map(|n| n.id));
    }
}
