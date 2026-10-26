//! Solids from an OBJ file — for example Revit elements exported with `tools/revit/ExportGeometry.cs` — matched to
//! nodes by name and drawn in the 3D coordinates view in the node's colour. Each named object (`o`, or `g` when a
//! file has no `o`) is one solid. A node gets the solid whose name equals its value in the chosen column, so one
//! solid per element (names = element ids) and one solid shared by many nodes (names = family types) both work.

use crate::graph::Graph;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type V3 = [f32; 3];

pub struct GeoMesh {
    pub name: String,
    pub verts: Vec<V3>,
    pub tris: Vec<[u32; 3]>,
    pub lo: V3,
    pub hi: V3,
}

/// The solids of one OBJ file.
pub struct GeoLib {
    pub path: PathBuf,
    pub meshes: Vec<GeoMesh>,
    pub by_name: HashMap<String, u32>,
    pub triangles: usize,
}

/// Where a node's solid goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Place {
    /// The solid's own coordinates are model coordinates, the same as the node coordinate columns (one solid per
    /// element, exported at its place in the model).
    #[default]
    Model,
    /// The solid is in its own coordinates around 0, 0, 0 and is moved to the node (a library: one solid per type).
    AtNode,
    /// The solid is stretched to fill the node's bounding box.
    FillBox,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GeoSettings {
    /// The OBJ file (kept in sessions; loaded again when a session opens).
    pub path: Option<PathBuf>,
    /// The node column whose values are the object names ("id" for the node id).
    pub match_by: Option<String>,
    pub place: Place,
    /// Size factor for solids placed at their node.
    pub size: f32,
    pub show: bool,
}

impl Default for GeoSettings {
    fn default() -> Self {
        GeoSettings { path: None, match_by: None, place: Place::Model, size: 1.0, show: true }
    }
}

/// How well the objects matched the nodes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GeoInfo {
    /// Nodes that found a solid.
    pub nodes: usize,
    /// Objects used by at least one node.
    pub used: usize,
}

pub fn read_obj(path: &Path) -> Result<GeoLib> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let meshes = parse_obj(&text).with_context(|| format!("{} is not a readable OBJ file", path.display()))?;
    Ok(GeoLib::new(path.to_path_buf(), meshes))
}

impl GeoLib {
    pub fn new(path: PathBuf, meshes: Vec<GeoMesh>) -> GeoLib {
        let mut by_name = HashMap::with_capacity(meshes.len());
        for (k, m) in meshes.iter().enumerate() {
            by_name.entry(m.name.clone()).or_insert(k as u32);
        }
        let triangles = meshes.iter().map(|m| m.tris.len()).sum();
        GeoLib { path, meshes, by_name, triangles }
    }
}

/// Read the vertices, faces and object names of an OBJ text. Faces with more than three corners are split into a
/// fan; `v/vt/vn` corners and negative (relative) indices are understood; everything else is skipped.
pub fn parse_obj(text: &str) -> Result<Vec<GeoMesh>> {
    let mut all: Vec<V3> = Vec::new();
    // per object: name, its faces as indices into `all`
    let mut objects: Vec<(String, Vec<[u32; 3]>)> = Vec::new();
    let has_o = text.lines().any(|l| l.trim_start().starts_with("o "));
    for (no, line) in text.lines().enumerate() {
        let line = line.trim();
        let mut parts = line.split_whitespace();
        let Some(tag) = parts.next() else { continue };
        match tag {
            "v" => {
                let mut p = [0.0f32; 3];
                for k in 0..3 {
                    p[k] = parts.next().and_then(|s| s.parse::<f32>().ok()).with_context(|| format!("line {}: a vertex needs x y z", no + 1))?;
                }
                all.push(p);
            }
            "o" | "g" if tag == "o" || !has_o => {
                let name = line[tag.len()..].trim().to_string();
                objects.push((if name.is_empty() { format!("object {}", objects.len() + 1) } else { name }, Vec::new()));
            }
            "f" => {
                let mut idx: Vec<u32> = Vec::with_capacity(4);
                for c in parts {
                    let first = c.split('/').next().unwrap_or("");
                    let k: i64 = first.parse().with_context(|| format!("line {}: bad face corner {c}", no + 1))?;
                    let i = if k > 0 { k - 1 } else { all.len() as i64 + k };
                    if i < 0 || i as usize >= all.len() {
                        bail!("line {}: face corner {k} points at a vertex that does not exist", no + 1);
                    }
                    idx.push(i as u32);
                }
                if objects.is_empty() {
                    objects.push(("object 1".into(), Vec::new()));
                }
                let faces = &mut objects.last_mut().unwrap().1;
                for k in 1..idx.len().saturating_sub(1) {
                    faces.push([idx[0], idx[k], idx[k + 1]]);
                }
            }
            _ => {}
        }
    }
    // each object keeps only the vertices it uses, renumbered from 0
    let mut out = Vec::new();
    for (name, faces) in objects {
        if faces.is_empty() {
            continue;
        }
        let mut map: HashMap<u32, u32> = HashMap::new();
        let mut verts = Vec::new();
        let mut tris = Vec::with_capacity(faces.len());
        for f in faces {
            tris.push(f.map(|i| {
                *map.entry(i).or_insert_with(|| {
                    verts.push(all[i as usize]);
                    (verts.len() - 1) as u32
                })
            }));
        }
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in &verts {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
        out.push(GeoMesh { name, verts, tris, lo, hi });
    }
    if out.is_empty() {
        bail!("there are no faces in it");
    }
    Ok(out)
}

/// A node's value in the matching column ("id" is the node id).
fn key(g: &Graph, col: &str, i: usize) -> Option<String> {
    if col == "id" { Some(g.ids[i].clone()) } else { g.node_col(col).and_then(|c| c.label_at(i)) }
}

/// The solid of every node (an index into `lib.meshes`), and how many matched.
pub fn match_nodes(g: &Graph, lib: &GeoLib, col: &str) -> (Vec<Option<u32>>, GeoInfo) {
    let mut used = vec![false; lib.meshes.len()];
    let m: Vec<Option<u32>> = (0..g.n())
        .map(|i| {
            let k = key(g, col, i).and_then(|v| lib.by_name.get(v.trim()).copied());
            if let Some(k) = k {
                used[k as usize] = true;
            }
            k
        })
        .collect();
    let info = GeoInfo { nodes: m.iter().filter(|x| x.is_some()).count(), used: used.iter().filter(|u| **u).count() };
    (m, info)
}

/// The column whose values match the most object names: the node id or any attribute.
pub fn best_column(g: &Graph, lib: &GeoLib) -> Option<String> {
    let mut cands = vec!["id".to_string()];
    cands.extend(g.node_cols.iter().filter(|c| !c.is_metric).map(|c| c.name.clone()));
    cands.into_iter().map(|c| (match_nodes(g, lib, &c).1.nodes, c)).filter(|(n, _)| *n > 0).max_by_key(|(n, _)| *n).map(|(_, c)| c)
}

/// Shared, so cloning a session does not copy the meshes.
pub type SharedLib = Arc<GeoLib>;

/// A mesh's visible triangles on screen with shaded colours, far ones first. Both sides of every triangle are lit (an
/// exported mesh does not always wind its triangles the same way). `place` maps a mesh vertex to the 3D view's
/// world; `project` maps a world point to the screen and its depth.
pub fn mesh_on_screen(
    mesh: &GeoMesh,
    place: impl Fn(V3) -> V3,
    project: impl Fn(V3) -> Option<([f32; 2], f32)>,
    eye: V3,
    fwd: V3,
    ortho: bool,
    light: V3,
    rgba: [u8; 4],
) -> Vec<([[f32; 2]; 3], [u8; 4])> {
    let world: Vec<V3> = mesh.verts.iter().map(|&v| place(v)).collect();
    let screen: Vec<Option<([f32; 2], f32)>> = world.iter().map(|&p| project(p)).collect();
    let sub = |a: V3, b: V3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut out: Vec<([[f32; 2]; 3], [u8; 4], f32)> = Vec::with_capacity(mesh.tris.len());
    for t in &mesh.tris {
        let (Some(a), Some(b), Some(c)) = (screen[t[0] as usize], screen[t[1] as usize], screen[t[2] as usize]) else { continue };
        let (p, q, r) = (world[t[0] as usize], world[t[1] as usize], world[t[2] as usize]);
        let (u, v) = (sub(q, p), sub(r, p));
        let mut n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = dot(n, n).sqrt();
        if l < 1e-12 {
            continue;
        }
        n = [n[0] / l, n[1] / l, n[2] / l];
        // turn the normal towards the camera, so both sides are lit as front faces
        let view = if ortho { fwd } else { sub(p, eye) };
        if dot(n, view) > 0.0 {
            n = [-n[0], -n[1], -n[2]];
        }
        let lit = 0.38 + 0.62 * dot(n, light).max(0.0);
        let shade = |x: u8| ((x as f32 * lit).round()).clamp(0.0, 255.0) as u8;
        out.push(([a.0, b.0, c.0], [shade(rgba[0]), shade(rgba[1]), shade(rgba[2]), rgba[3]], (a.1 + b.1 + c.1) / 3.0));
    }
    out.sort_by(|x, y| y.2.total_cmp(&x.2));
    out.into_iter().map(|(t, c, _)| (t, c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBJ: &str = "# two solids\no 101\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\no 102\nv 5 5 5\nv 6 5 5\nv 5 6 5\nf -3/1/1 -2/2/2 -1/3/3\n";

    #[test]
    fn objects_faces_and_bounds_are_read() {
        let m = parse_obj(OBJ).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!((m[0].name.as_str(), m[0].tris.len(), m[0].verts.len()), ("101", 2, 4), "a quad is two triangles");
        assert_eq!((m[1].name.as_str(), m[1].tris.len(), m[1].verts.len()), ("102", 1, 3), "relative v/vt/vn corners");
        assert_eq!((m[1].lo, m[1].hi), ([5.0, 5.0, 5.0], [6.0, 6.0, 5.0]));
        // groups name the objects when there is no `o`
        let g = parse_obj("v 0 0 0\nv 1 0 0\nv 0 1 0\ng Door : 900\nf 1 2 3\n").unwrap();
        assert_eq!(g[0].name, "Door : 900");
        assert!(parse_obj("v 0 0 0\nf 1 2 9\n").is_err(), "a corner past the vertices");
        assert!(parse_obj("# nothing\n").is_err());
    }

    #[test]
    fn nodes_match_by_the_column_with_the_most_names() {
        use crate::data::{auto_edge_mapping, auto_node_mapping, parse_csv_text};
        let e = parse_csv_text("source,target\na,b\nb,c\n").unwrap();
        let n = parse_csv_text("id,ElementId,kind\na,101,x\nb,102,y\nc,999,x\n").unwrap();
        let g = crate::graph::build_graph(Some((&e, &auto_edge_mapping(&e))), Some((&n, &auto_node_mapping(&n))));
        let lib = GeoLib::new(PathBuf::from("t.obj"), parse_obj(OBJ).unwrap());
        assert_eq!(best_column(&g, &lib).as_deref(), Some("ElementId"));
        let (m, info) = match_nodes(&g, &lib, "ElementId");
        assert_eq!(info, GeoInfo { nodes: 2, used: 2 });
        assert_eq!(m[g.index["a"] as usize], Some(0));
        assert_eq!(m[g.index["c"] as usize], None);
    }
}
