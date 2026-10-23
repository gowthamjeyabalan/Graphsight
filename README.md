<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/readme/grapht-logo-dark.svg">
    <img src="assets/readme/grapht-logo-light.svg" alt="Grapht" width="340">
  </picture>
</p>

A fast native desktop app (Rust, egui + OpenGL) for exploring graphs stored in **CSV** and **JSON** files.
Load an edges file and, optionally, a nodes file with attributes; colour, size, filter and sort the graph by
those attributes; switch between layouts; export PNG / SVG.

Designed for graphs of roughly 1 000 – 10 000 nodes. Measured on a 2-core cloud VM with a 10 000-node /
50 000-edge graph: load + metrics 88 ms, force layout converges in ~3 s, one UI frame costs ~6 ms of CPU.

## Download

Ready-made Windows builds are on the repository's **Releases** page: download `grapht-windows-x64.zip`, unzip it
and run `grapht.exe` (no install needed). Windows may show a SmartScreen warning for an unsigned app; choose
*More info ▸ Run anyway*.

## Build and run

Windows:

1. Install Rust from <https://rustup.rs> (accept the default MSVC toolchain; it will ask you to install the
   Visual Studio C++ Build Tools if they are missing).
2. In this folder: `cargo build --release`
3. Run `target\release\grapht.exe` (or `cargo run --release`).

macOS / Linux: the same `cargo build --release`. (Linux needs the usual X11/Wayland + Vulkan or OpenGL libraries.)

Optional: `cargo run --release -- --edges sample/edges.csv --nodes sample/nodes.csv` opens the sample straight away.
The first build downloads and compiles ~450 crates (several minutes); later builds take seconds.

## Workspace, panels and sessions

The window is a docking workspace in an Apple-style dark theme (Inter typeface). The default arrangement:
Data / Layout / Appearance / Subgraph tabs and Filter / Analysis / Query tabs on the left, the graph in the middle, the node table
along the bottom, and the inspector and legend on the right.

* **Move panels:** drag a tab by its title onto another panel to tab them together, or onto an edge to split; drag the
  dividers to resize. Right-click a tab for *Close panel*; bring panels back with **View ▸ Panels**.
  **View ▸ Reset panel layout** restores the default (use it, or View ▸ Panels ▸ Query, if you saved a layout before the Query panel existed). The arrangement is saved automatically (in
  `%APPDATA%\Grapht\layout.json`) and restored at the next start.
* **Sessions:** **File ▸ Save session** (Ctrl+S) writes a `.grapht` file with the loaded files and column mappings,
  all appearance / layout / filter settings, node positions and pins, the selection, hand-hidden nodes, the table
  sort and the camera. **Open session** (Ctrl+O), **Open recent**, or start `grapht my.grapht`. File paths are stored
  both absolutely and relative to the session file, so a project folder can be moved. The data is re-read from the
  original files, so edits to the CSVs show up when you reopen the session.

## Using it

**Loading data** — use File ▸ Open, or drop files on the window. Edge files are recognised by columns such as
`source,target` (also `from/to`, `src/dst`, …); node files by an `id` / `name` / `node` column. When the headers are
not recognisable a column-mapping dialog opens (source, target, optional weight, directed flag, node id). Comma,
semicolon, tab and pipe delimiters are detected automatically. Every other column becomes an attribute; numeric
columns are detected automatically, everything else is categorical. Nodes that appear only in the edge file are
still drawn (with empty attributes).

**JSON** — accepted shapes: `{nodes:[…], links|edges:[…]}` (D3 / NetworkX), Cytoscape `elements` (node positions
are kept), a flat array of edge objects (`source/target`, `from/to`, …), arrays of `[source, target, weight]`, and
adjacency maps `{"a": ["b","c"]}`. Nested properties are flattened (`a.b`).

**Computed columns** (usable everywhere an attribute is): `_degree`, `_in_degree`/`_out_degree` (directed),
`_pagerank`, `_kcore`, `_component` (weakly connected, 0 = largest) and `_community` (Louvain modularity, 0 = largest).

**Colour / size** — colour by any column (categorical palette, or a gradient with optional log scale / 2–98 %
clipping / invert; numeric columns can also be treated as categories). Size by any numeric column. Edges can be one
colour, take the source node's colour, blend both ends, or be coloured by an edge attribute.

**Filtering** — combine any of: search box (id and text columns); per-attribute rules (numeric range, category
checklist); a free-form expression; hand-hiding (select ▸ Hide / Isolate). Filtered-out nodes are hidden or dimmed.

**Subgraph** — the *Subgraph* tab has two tools. *Show a subgraph* keeps only the nodes that match an attribute
(a numeric range or ticked categories), optionally with their neighbours 1–3 steps out, lays them out again and
hides the rest; switching it off brings the full graph and its layout back. If the matches fall apart into pieces,
*Join separate pieces* walks the full graph along shortest paths and adds the nodes needed to connect them (drawn
dimmer). *Highlight where a value changes* marks the connections whose two ends differ in any attribute (e.g. where
`Level` goes 1 → 2), rings the nodes on them, lists every kind of change with its count (click a row to see just
that one), and can show only the changes, also joined into one graph. Both respect the filter.

**Node coordinates and 3D** — in the *Layout* tab, *Place nodes by coordinates* puts every node at the X / Y values
stored in two numeric attributes (columns named x, y, z, lon, lat … are picked automatically), instead of running a
layout. Add a Z column and a *2D / 3D orbit* switch appears. The 3D view is a perspective camera: drag to orbit,
middle- or right-drag (or Space + drag) to pan, scroll to zoom towards the cursor, Top / Front / Side / Iso views, perspective or orthographic projection, double-click empty space or `F` to fit,
an axes gizmo in the corner, and nearer nodes drawn on top. Colour, size, filters, subgraphs and change highlighting
all work in 3D; clicking and Shift-drag selection work on what you see. Layouts and dragging nodes are off while
coordinates are used; switching them off brings the previous layout back.

**View toolbar** — floating buttons at the bottom of the graph: *zoom to fit* (`F`), *zoom to selection* (`S`, greyed
out until visible nodes are selected; one node is centred at a readable zoom, a group fills the view with padding),
*zoom to region* (`R`, then drag a rectangle; `Esc` cancels; it switches itself off after one use), *zoom in / out*,
*reset view* (`Home`) and *previous / next view* (`Alt+←` / `Alt+→`, a history that remembers each place the view came
to rest). In 3D a second row adds the drag mode (orbit / pan / zoom), Top / Front / Side / Iso views and a
perspective / orthographic switch. Camera moves glide (about 0.3 s; **View ▸ Animate view changes** turns that off) and
stop as soon as you scroll or drag. The view history is not saved in sessions; the camera and projection are.

**Selecting edges and orbiting around the selection** — clicking an edge selects it (Ctrl/Shift+click adds to the
selection, so nodes and edges can be selected together; `Esc` clears). In the 3D view, dragging to orbit then turns the
camera around the middle of whatever is selected (a selected edge counts with both of its ends) and keeps the selection
where it is on screen; with nothing selected it turns around the centre of the view as before.

**Full-screen graph** — the button in the top-right corner of the graph (or **View ▸ Maximise graph**, `F11`) makes the
graph fill the whole screen together with its view controls. The *Leave full screen* button, `F11` or `Esc` (after a
selection has been cleared) bring the panels back.

**Nodes table columns** — right-click a column header to hide it, Ctrl+click headers to pick several, or use the
*Columns* button above the table to tick columns on and off, hide the picked ones and *Show all* again.

**Appearance** — *Settings* opens the settings window directly. Mode (System / Dark / Light), interface size, and a
*Translucency* section: a Windows 11 window backdrop (None, Mica, Mica Alt, Acrylic) and how solid the panels, the graph
and the menus / floating windows are. The backdrop needs Windows 11; with *None* the interface stays solid.

**Connection query** — the *Query* tab answers “how many X are connected to Y?”. Pick the first group (an attribute
and *is / is not / < / > / contains / starts with / is empty …*, or any node, or the node id), pick the second group,
and choose whether a connection has to run *either way*, *first → second* or *second → first* (undirected edges count
both ways). It says, for example, “2 of 4 Room nodes connect to Sensor nodes.” and “2 have no connection to Sensor
nodes.”; click a sentence, or the buttons under it, to select those nodes. *Only visible nodes* respects the filter and
subgraph. *Cross-tab* mode counts the connections between every value of one attribute and every value of another
(up to 40 values each) as a shaded grid; click a cell to select the nodes at both ends. The query is saved in the
session.

**Bounding boxes** — while the 3D view is on, a *Bounding boxes* block appears under the coordinates. Choose the six
numeric attributes that hold each node's box (minimum and maximum of X, Y and Z, in the same units as the
coordinates; columns named like `bbox_min_x` or `xmin` are found for you) and switch *Show bounding boxes* on. Pick an
attribute and tick its values to draw boxes only for those nodes, for example every `room` but no `hall`. Boxes can be
one colour or take their node's colour (so *Colour by* in Appearance colours them too), with sliders for fill opacity,
outline opacity and outline width, and a switch to hide the nodes and edges so only the boxes show. Boxes follow the
filters and the selected node's box is highlighted. Boxes that lie far outside the nodes (placeholder values such as
-9999, a site outline) are counted in the panel and ignored when the camera frames the view; a box much bigger than the
view is drawn as an outline only. They are also drawn in PNG and SVG exports, and the settings are
saved in the session.

**Directed and undirected edges** — if the edges file has a column saying `directed` / `undirected` for each edge
(found automatically, or chosen in the import dialog), every edge keeps its own direction: arrows only on directed
edges, in/out degree, PageRank and the hierarchical layout all follow it.

## Headless export / scripting

```
grapht --edges e.csv --nodes n.csv --layout group --group team --color team --size-by _degree \
       --filter 'score > 3' --size 3840x2160 --export-png out.png --export-svg out.svg
grapht --help
```

## Code map

| file | role |
|---|---|
| `data.rs` | CSV/JSON readers, typed columns, column auto-detection |
| `graph.rs` | graph construction, adjacency, metrics (PageRank, k-core, components, Louvain) |
| `expr.rs` | filter expression parser/evaluator |
| `changes.rs` | finds the connections where an attribute changes, with per-value counts |
| `coords.rs` | node coordinates → positions, the 3D orbit camera (perspective / orthographic) and its projection |
| `query.rs` | the connection query: group conditions, direction, counts, cross-tab |
| `view.rs` | view tools: zoom to fit / selection / region, animated camera moves, view history |
| `toolbar.rs` | the floating view toolbar and its painter-drawn icons |
| `backdrop.rs` | the Windows 11 window material (Mica / Acrylic) behind the see-through window |
| `boxes.rs` | bounding boxes: the six min/max columns, which nodes get a box, box geometry |
| `subgraph.rs` | attribute sub-graphs, N-hop neighbours, joining separate pieces by shortest paths |
| `layout.rs` | static layouts and the Barnes–Hut force simulation |
| `model.rs` | session state: appearance, filters, selection, camera, per-frame draw list |
| `render.rs` | PNG (tiny-skia) and SVG export |
| `app.rs`, `panels.rs`, `canvas.rs`, `table.rs` | egui UI; the canvas draws nodes as textured quads and edges as meshes |
| `theme.rs` | fonts, colours, spacing and small widgets (switch, segmented control) |
| `dock.rs` | the dockable workspace: which panels exist, the default layout |
| `project.rs`, `config.rs` | `.grapht` session files; per-user settings, recent files and saved panel layout |
| `assets/readme` | the logo files shown at the top of this README |
| `.github/workflows/release.yml` | tests, then builds the Windows exe and publishes a GitHub release when a `v*` tag is pushed |
| `vendor/egui_dock` | the `egui_dock` docking crate (MIT), ported to egui 0.37 (one-line change) |

`cargo test --release` runs the unit tests (130+), including headless UI tests that feed pointer, wheel and keyboard events
through the real egui code and validate the generated meshes. `cargo test --release bench -- --ignored --nocapture`
prints the large-graph timings.

## Known limitations

* Developed on Linux (including the real window under a virtual display) and used on Windows; not yet run on macOS.
* Labels are not included in PNG exports (SVG has them); curved/bundled edges are not implemented.
* Louvain communities are deterministic but not tuned (resolution is fixed at 1.0).
* The whole table is loaded into memory — fine for files of a few MB, not meant for multi-GB inputs.
* Sessions store file paths, not the data itself: moving or deleting the CSVs breaks a session (it opens with a warning).
