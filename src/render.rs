//! Off-screen rendering (PNG via tiny-skia, SVG as text). The on-screen view lives in `app`.

use crate::model::*;
use egui::Vec2 as V2;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// Triangle for a directed edge's arrow head, or None if the edge is too short.
pub fn arrow_points(a: [f32; 2], b: [f32; 2], target_r: f32) -> Option<[[f32; 2]; 3]> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 16.0 + target_r {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    let tip = [b[0] - ux * (target_r + 1.0), b[1] - uy * (target_r + 1.0)];
    let size = 5.0;
    let base = [tip[0] - ux * size * 1.6, tip[1] - uy * size * 1.6];
    let (nx, ny) = (-uy * size * 0.6, ux * size * 0.6);
    Some([tip, [base[0] + nx, base[1] + ny], [base[0] - nx, base[1] - ny]])
}

fn paint(rgba: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    p.anti_alias = true;
    p
}

fn none_highlight(n: usize) -> Vec<bool> {
    vec![false; n]
}

pub fn render_png(s: &Session, cam: &ViewCam, w: u32, h: u32, scale: f32) -> Option<Pixmap> {
    let mut pm = Pixmap::new(w, h)?;
    let bg = s.look.background();
    pm.fill(Color::from_rgba8(bg[0], bg[1], bg[2], 255));
    let size = V2::new(w as f32 / scale, h as f32 / scale);
    let none = none_highlight(s.g.n());
    let hi = Highlight { focus: &none, hood: &none, active: false };
    let mut dl = DrawList::default();
    build_draw_list(s, cam, size, &hi, &mut dl);
    let tf = Transform::from_scale(scale, scale);

    // bounding boxes (3D view), far to near, behind everything else
    for b in &dl.boxes {
        if b.fill > 0 {
            for poly in b.polygons() {
                let mut pb = PathBuilder::new();
                pb.move_to(poly[0][0], poly[0][1]);
                for p in &poly[1..] {
                    pb.line_to(p[0], p[1]);
                }
                pb.close();
                if let Some(path) = pb.finish() {
                    pm.fill_path(&path, &paint([b.rgb[0], b.rgb[1], b.rgb[2], b.fill]), FillRule::Winding, tf, None);
                }
            }
        }
        if b.line > 0 {
            for (p, q, alpha) in b.lines() {
                let mut pb = PathBuilder::new();
                pb.move_to(p[0], p[1]);
                pb.line_to(q[0], q[1]);
                if let Some(path) = pb.finish() {
                    pm.stroke_path(&path, &paint([b.rgb[0], b.rgb[1], b.rgb[2], alpha]), &Stroke { width: b.width, ..Default::default() }, tf, None);
                }
            }
        }
    }

    // edges, grouped by colour so uniform graphs need a single stroke call
    let stroke = Stroke { width: s.look.edge_width.max(0.2), ..Default::default() };
    let draw_edges = |pm: &mut Pixmap, edges: &[DrawEdge], wmul: f32| {
        let stroke = Stroke { width: stroke.width * wmul, ..Default::default() };
        let mut groups: Vec<([u8; 4], PathBuilder)> = Vec::new();
        let mut index: std::collections::HashMap<[u8; 4], usize> = std::collections::HashMap::new();
        for e in edges {
            let gi = *index.entry(e.rgba).or_insert_with(|| {
                groups.push((e.rgba, PathBuilder::new()));
                groups.len() - 1
            });
            let pb = &mut groups[gi].1;
            pb.move_to(e.a[0], e.a[1]);
            pb.line_to(e.b[0], e.b[1]);
        }
        for (rgba, pb) in groups {
            if let Some(path) = pb.finish() {
                pm.stroke_path(&path, &paint(rgba), &stroke, tf, None);
            }
        }
        if s.g.directed && s.look.arrows {
            for e in edges.iter().filter(|e| e.directed) {
                if let Some(t) = arrow_points(e.a, e.b, e.target_r) {
                    let mut pb = PathBuilder::new();
                    pb.move_to(t[0][0], t[0][1]);
                    pb.line_to(t[1][0], t[1][1]);
                    pb.line_to(t[2][0], t[2][1]);
                    pb.close();
                    if let Some(path) = pb.finish() {
                        pm.fill_path(&path, &paint(e.rgba), FillRule::Winding, tf, None);
                    }
                }
            }
        }
    };
    draw_edges(&mut pm, &dl.edges_back, 1.0);
    draw_edges(&mut pm, &dl.edges, 1.0);
    draw_edges(&mut pm, &dl.edges_hi, 2.4);

    let ring = {
        let t = s.look.text_color();
        paint([t[0], t[1], t[2], 255])
    };
    for n in &dl.nodes {
        if let Some(path) = PathBuilder::from_circle(n.x, n.y, n.r) {
            pm.fill_path(&path, &paint(n.rgba), FillRule::Winding, tf, None);
            if n.selected {
                if let Some(ring_path) = PathBuilder::from_circle(n.x, n.y, n.r + 2.0) {
                    pm.stroke_path(&ring_path, &ring, &Stroke { width: 2.0, ..Default::default() }, tf, None);
                }
            } else if n.ring {
                if let Some(ring_path) = PathBuilder::from_circle(n.x, n.y, n.r + 2.0) {
                    let c = CHANGE_COLOR;
                    pm.stroke_path(&ring_path, &paint([c[0], c[1], c[2], 255]), &Stroke { width: 2.0, ..Default::default() }, tf, None);
                }
            }
        }
    }
    Some(pm)
}

pub fn render_svg(s: &Session, cam: &ViewCam, w: f32, h: f32) -> String {
    use std::fmt::Write;
    let size = V2::new(w, h);
    let none = none_highlight(s.g.n());
    let hi = Highlight { focus: &none, hood: &none, active: false };
    let mut dl = DrawList::default();
    build_draw_list(s, cam, size, &hi, &mut dl);
    let bg = s.look.background();
    let mut out = String::with_capacity(dl.nodes.len() * 90 + (dl.edges.len() + dl.edges_back.len() + dl.edges_hi.len()) * 100 + 512);
    let _ = writeln!(out, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#);
    let _ = writeln!(out, r#"<rect width="100%" height="100%" fill="rgb({},{},{})"/>"#, bg[0], bg[1], bg[2]);
    for b in &dl.boxes {
        let rgb = format!("rgb({},{},{})", b.rgb[0], b.rgb[1], b.rgb[2]);
        if b.fill > 0 {
            for poly in b.polygons() {
                let pts: Vec<String> = poly.iter().map(|p| format!("{:.1},{:.1}", p[0], p[1])).collect();
                let _ = writeln!(out, r#"<polygon points="{}" fill="{rgb}" fill-opacity="{:.2}"/>"#, pts.join(" "), b.fill as f32 / 255.0);
            }
        }
        if b.line > 0 {
            for (p, q, alpha) in b.lines() {
                let _ = writeln!(
                    out,
                    r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{rgb}" stroke-opacity="{:.2}" stroke-width="{:.1}"/>"#,
                    p[0], p[1], q[0], q[1], alpha as f32 / 255.0, b.width
                );
            }
        }
    }
    let _ = writeln!(out, r#"<g fill="none" stroke-width="{}">"#, s.look.edge_width);
    for (li, list) in [&dl.edges_back, &dl.edges, &dl.edges_hi].into_iter().enumerate() {
        for e in list.iter() {
            let w = if li == 2 { format!(r#" stroke-width="{:.1}""#, s.look.edge_width * 2.4) } else { String::new() };
            let _ = writeln!(
                out,
                r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="rgb({},{},{})" stroke-opacity="{:.2}"{w}/>"#,
                e.a[0], e.a[1], e.b[0], e.b[1], e.rgba[0], e.rgba[1], e.rgba[2], e.rgba[3] as f32 / 255.0
            );
        }
    }
    out.push_str("</g>\n");
    let t = s.look.text_color();
    let label_all = dl.nodes.len() <= 300;
    for n in &dl.nodes {
        let _ = write!(
            out,
            r#"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="rgb({},{},{})" fill-opacity="{:.2}""#,
            n.x, n.y, n.r, n.rgba[0], n.rgba[1], n.rgba[2], n.rgba[3] as f32 / 255.0
        );
        if n.selected {
            let _ = write!(out, r#" stroke="rgb({},{},{})" stroke-width="2""#, t[0], t[1], t[2]);
        } else if n.ring {
            let c = CHANGE_COLOR;
            let _ = write!(out, r#" stroke="rgb({},{},{})" stroke-width="2""#, c[0], c[1], c[2]);
        }
        out.push_str("/>\n");
        if label_all || n.selected {
            let label = s.label_of(n.idx as usize).replace('&', "&amp;").replace('<', "&lt;");
            let _ = writeln!(
                out,
                r#"<text x="{:.1}" y="{:.1}" font-family="sans-serif" font-size="10" fill="rgb({},{},{})">{}</text>"#,
                n.x + n.r + 2.0, n.y + 3.0, t[0], t[1], t[2], label
            );
        }
    }
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::*;
    use crate::layout::LayoutKind;

    fn session() -> Session {
        let e = parse_csv_text("source,target\na,b\nb,c\nc,a\nc,d\n").unwrap();
        let mut s = Session::new();
        let mut m = auto_edge_mapping(&e);
        m.directed = true;
        s.set_edges("e".into(), e, m);
        s.layout.kind = LayoutKind::Circular;
        s.rebuild();
        s.apply_layout_now();
        s
    }

    #[test]
    fn png_has_nonbackground_pixels() {
        let s = session();
        let cam = s.fit_camera(V2::new(400.0, 300.0));
        let pm = render_png(&s, &cam, 400, 300, 1.0).unwrap();
        let bg = s.look.background();
        let painted = pm.pixels().iter().filter(|p| p.red() != bg[0] || p.green() != bg[1] || p.blue() != bg[2]).count();
        assert!(painted > 200, "only {painted} painted pixels");
        assert!(pm.encode_png().unwrap().len() > 100);
    }

    #[test]
    fn svg_is_well_formed() {
        let mut s = session();
        s.select_only(0);
        let cam = s.fit_camera(V2::new(400.0, 300.0));
        let svg = render_svg(&s, &cam, 400.0, 300.0);
        assert!(svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"));
        assert_eq!(svg.matches("<circle").count(), 4);
        assert_eq!(svg.matches("<line").count(), 4);
        assert!(svg.contains("<text"));
    }

    #[test]
    fn arrows_skip_short_edges() {
        assert!(arrow_points([0.0, 0.0], [5.0, 0.0], 3.0).is_none());
        let t = arrow_points([0.0, 0.0], [100.0, 0.0], 4.0).unwrap();
        assert!((t[0][0] - 95.0).abs() < 1e-3);
    }
}
