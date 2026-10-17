#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod app;
mod canvas;
mod changes;
mod config;
mod coords;
mod data;
mod dock;
mod expr;
mod graph;
mod layout;
mod model;
mod panels;
mod project;
mod render;
mod style;
mod subgraph;
mod table;
mod theme;

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

const HELP: &str = "\
Grapht — fast graph explorer for CSV and JSON files

USAGE
  grapht [FILES] [OPTIONS]

INPUT
  --edges FILE        edges CSV/TSV (columns like source,target[,weight,…])
  --nodes FILE        nodes CSV/TSV (an id column plus attributes)
  --json FILE         JSON graph ({nodes,links|edges}, Cytoscape elements, edge array)
  --directed          treat edges as directed
  FILE                a bare .csv/.json path is classified automatically;
                      a .grapht path opens a saved session

HEADLESS EXPORT (no window)
  --export-png OUT    render the graph to a PNG
  --export-svg OUT    render the graph to an SVG
  --size WxH          image size in pixels (default 1600x1000)
  --scale N           pixel density for PNG, e.g. 2 for sharper output (default 1)
  --layout NAME       force | circular | grid | concentric | hierarchical | group | scatter | random
  --ticks N           force-layout iterations before exporting (default 400)
  --color COL         colour nodes by this column
  --size-by COL       size nodes by this numeric column
  --group COL         attribute for the group layout / force group pull
  --rings COL         attribute for the concentric layout
  --x COL --y COL     attributes for the scatter layout
  --filter EXPR       filter expression, e.g. 'score > 3 and group == \"A\"'
";

#[derive(Default)]
struct Cli {
    edges: Option<PathBuf>,
    nodes: Option<PathBuf>,
    json: Option<PathBuf>,
    positional: Vec<PathBuf>,
    directed: bool,
    export_png: Option<PathBuf>,
    export_svg: Option<PathBuf>,
    size: (u32, u32),
    scale: f32,
    layout: Option<String>,
    ticks: usize,
    color: Option<String>,
    size_by: Option<String>,
    group: Option<String>,
    rings: Option<String>,
    x: Option<String>,
    y: Option<String>,
    filter: Option<String>,
    help: bool,
}

fn parse_cli() -> Result<Cli> {
    let mut c = Cli { size: (1600, 1000), scale: 1.0, ticks: 400, ..Default::default() };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().with_context(|| format!("{name} needs a value"));
        match a.as_str() {
            "-h" | "--help" => c.help = true,
            "--edges" => c.edges = Some(val("--edges")?.into()),
            "--nodes" => c.nodes = Some(val("--nodes")?.into()),
            "--json" => c.json = Some(val("--json")?.into()),
            "--directed" => c.directed = true,
            "--export-png" => c.export_png = Some(val("--export-png")?.into()),
            "--export-svg" => c.export_svg = Some(val("--export-svg")?.into()),
            "--size" => {
                let v = val("--size")?;
                let (w, h) = v.split_once(['x', 'X']).context("--size expects WxH, e.g. 1600x1000")?;
                c.size = (w.parse()?, h.parse()?);
            }
            "--scale" => c.scale = val("--scale")?.parse()?,
            "--layout" => c.layout = Some(val("--layout")?),
            "--ticks" => c.ticks = val("--ticks")?.parse()?,
            "--color" => c.color = Some(val("--color")?),
            "--size-by" => c.size_by = Some(val("--size-by")?),
            "--group" => c.group = Some(val("--group")?),
            "--rings" => c.rings = Some(val("--rings")?),
            "--x" => c.x = Some(val("--x")?),
            "--y" => c.y = Some(val("--y")?),
            "--filter" => c.filter = Some(val("--filter")?),
            other if !other.starts_with('-') => c.positional.push(other.into()),
            other => bail!("unknown option {other} (try --help)"),
        }
    }
    Ok(c)
}

fn say(msg: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stdout(), "{msg}");
}

fn headless(c: &Cli) -> Result<()> {
    use layout::LayoutKind;
    use model::*;
    let mut s = Session::new();
    let mut files: Vec<(PathBuf, Option<bool>)> = Vec::new(); // (path, is_edges hint)
    if let Some(j) = &c.json {
        s.load_json(j)?;
    }
    if let Some(n) = &c.nodes {
        files.push((n.clone(), Some(false)));
    }
    if let Some(e) = &c.edges {
        files.push((e.clone(), Some(true)));
    }
    for p in &c.positional {
        files.push((p.clone(), None));
    }
    for (p, is_edges) in files {
        if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
            s.load_json(&p)?;
            continue;
        }
        let t = data::read_csv(&p)?;
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if is_edges.unwrap_or_else(|| data::looks_like_edges(&t)) {
            let mut m = data::auto_edge_mapping(&t);
            m.directed = c.directed;
            s.set_edges(name, t, m);
        } else {
            let m = data::auto_node_mapping(&t);
            s.set_nodes(name, t, m);
        }
    }
    if c.directed {
        if let Some(src) = &mut s.edge_src {
            src.mapping.directed = true;
        }
    }
    if s.edge_src.is_none() && s.node_src.is_none() {
        bail!("nothing to draw: pass --edges, --nodes, --json or a file path");
    }
    s.rebuild();

    s.layout.kind = match c.layout.as_deref().unwrap_or("force") {
        "force" => LayoutKind::Force,
        "circular" | "circle" => LayoutKind::Circular,
        "grid" => LayoutKind::Grid,
        "concentric" | "rings" => LayoutKind::Concentric,
        "hierarchical" | "tree" => LayoutKind::Hierarchical,
        "group" | "groupby" => LayoutKind::GroupBy,
        "scatter" => LayoutKind::Scatter,
        "random" => LayoutKind::Random,
        other => bail!("unknown layout `{other}`"),
    };
    if let Some(g) = &c.group {
        s.layout.group_by = Some(g.clone());
        if s.layout.kind == LayoutKind::Force {
            s.layout.force.group_pull = 0.1;
        }
    }
    if let Some(r) = &c.rings {
        s.layout.ring_by = Some(r.clone());
    }
    s.layout.x_by = c.x.clone();
    s.layout.y_by = c.y.clone();
    if let Some(col) = &c.color {
        if s.g.node_col(col).is_none() {
            bail!("no node column named `{col}`");
        }
        s.look.color_by = Some(col.clone());
    }
    if let Some(col) = &c.size_by {
        if s.g.node_col(col).is_none() {
            bail!("no node column named `{col}`");
        }
        s.look.size_by = Some(col.clone());
    }
    if let Some(f) = &c.filter {
        s.filter.expr_text = f.clone();
    }
    s.recompute_visibility();
    if let Some(e) = &s.expr_error {
        bail!("filter expression: {e}");
    }
    s.recompute_style();
    s.apply_layout_now();
    let t = std::time::Instant::now();
    if s.layout.kind == LayoutKind::Force {
        s.settle(c.ticks);
    }
    say(&format!(
        "{} nodes, {} edges ({} shown), layout {:?} in {:.0} ms",
        s.g.n(),
        s.g.edges.len(),
        s.n_vis,
        s.layout.kind,
        t.elapsed().as_secs_f64() * 1000.0
    ));

    let (w, h) = c.size;
    let logical = egui::Vec2::new(w as f32 / c.scale, h as f32 / c.scale);
    let cam = s.fit_camera(logical);
    if let Some(out) = &c.export_png {
        let pm = render::render_png(&s, &cam, w, h, c.scale).context("could not allocate the image")?;
        pm.save_png(out).with_context(|| format!("writing {}", out.display()))?;
        say(&format!("wrote {}", out.display()));
    }
    if let Some(out) = &c.export_svg {
        std::fs::write(out, render::render_svg(&s, &cam, logical.x, logical.y)).with_context(|| format!("writing {}", out.display()))?;
        say(&format!("wrote {}", out.display()));
    }
    Ok(())
}

#[cfg(windows)]
fn attach_parent_console() {
    // The release build is a GUI-subsystem exe; reconnect to the terminal for --help and headless output.
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS);
    }
}

/// The Grapht app icon (256 px PNG baked into the binary), used for the window and taskbar.
fn window_icon() -> egui::IconData {
    const PNG: &[u8] = include_bytes!("../assets/icon/grapht-256.png");
    match tiny_skia::Pixmap::decode_png(PNG) {
        Ok(pm) => {
            // tiny-skia stores premultiplied alpha, IconData wants straight RGBA
            let rgba = pm.pixels().iter().flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            }).collect();
            egui::IconData { rgba, width: pm.width(), height: pm.height() }
        }
        Err(_) => egui::IconData::default(),
    }
}

fn main() -> Result<()> {
    let cli = parse_cli();
    let headless_mode = match &cli {
        Ok(c) => c.help || c.export_png.is_some() || c.export_svg.is_some(),
        Err(_) => true,
    };
    #[cfg(windows)]
    if headless_mode {
        attach_parent_console();
    }
    let cli = cli?;
    if cli.help {
        {
            use std::io::Write;
            let _ = write!(std::io::stdout(), "{HELP}");
        }
        return Ok(());
    }
    if headless_mode {
        return headless(&cli);
    }

    // classify bare paths for the GUI
    let mut args = app::Args { edges: cli.edges, nodes: cli.nodes, json: cli.json, directed: cli.directed, project: None };
    for p in cli.positional {
        if p.extension().is_some_and(|e| e.eq_ignore_ascii_case(project::EXTENSION)) {
            args.project = Some(p);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
            args.json = Some(p);
        } else if args.edges.is_none() {
            args.edges = Some(p);
        } else {
            args.nodes = Some(p);
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Grapht")
            .with_icon(window_icon())
            .with_inner_size([1480.0, 920.0])
            .with_min_inner_size([920.0, 580.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("Grapht", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, args))))).map_err(|e| anyhow::anyhow!("{e}"))
}

#[cfg(test)]
mod icon_tests {
    #[test]
    fn window_icon_decodes() {
        let i = super::window_icon();
        assert_eq!((i.width, i.height), (256, 256));
        assert_eq!(i.rgba.len(), 256 * 256 * 4);
        assert_eq!(i.rgba[3], 0, "corner is transparent");
        let mid = (128 * 256 + 20) * 4; // inside the blue tile, left of the glyph
        assert_eq!(&i.rgba[mid..mid + 4], &[10, 132, 255, 255]);
    }
}
