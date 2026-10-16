# Graphy

A fast native desktop app (Rust, egui + wgpu) for exploring graphs stored in **CSV** and **JSON** files.
Load an edges file and, optionally, a nodes file with attributes; colour, size, filter and sort the graph by
those attributes; switch between layouts; export PNG / SVG.

Designed for graphs of roughly 1 000 – 10 000 nodes. Measured on a 2-core cloud VM with a 10 000-node /
50 000-edge graph: load + metrics 88 ms, force layout converges in ~3 s, one UI frame costs ~6 ms of CPU.

## Build and run

Windows:

1. Install Rust from <https://rustup.rs> (accept the default MSVC toolchain; it will ask you to install the
   Visual Studio C++ Build Tools if they are missing).
2. In this folder: `cargo build --release`
3. Run `target\release\graphy.exe` (or `cargo run --release`).

macOS / Linux: the same `cargo build --release`. (Linux needs the usual X11/Wayland + Vulkan or OpenGL libraries.)

Optional: `cargo run --release -- --edges sample/edges.csv --nodes sample/nodes.csv` opens the sample straight away.
The first build downloads and compiles ~450 crates (several minutes); later builds take seconds.

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

```
age > 30 and (group == "A" or group in ["B", "C"])
not name contains "test"        score >= 2.5        remote == true
`first name` startswith "An"    degree > 10         score == null
```
Operators: `== != < <= > >=`, `contains`, `startswith` (case-insensitive), `in [..]`, `and`/`or`/`not`, parentheses.
Computed columns work without the leading underscore (`degree`, `pagerank`, `community`).

**Sorting** — the node table under the canvas sorts by any column (click the header: ascending, descending, off),
follows the current filter, shares the selection with the canvas, and can be exported to CSV.

**Layouts** — force-directed (live, Barnes–Hut, parallel; sliders for repulsion/link distance/gravity/…; optional
pull toward groups of an attribute; drag nodes to pin), circular, grid, concentric rings by attribute, hierarchical
(top-down, left-right, radial), group-by-attribute (packed discs, hubs in the middle) and scatter (X/Y by attribute,
optional log). Circular/grid/concentric can order nodes by any attribute. Static layouts glide into place.

**Canvas** — drag to pan, wheel/pinch to zoom at the cursor, click to select (Ctrl/Shift adds), Shift-drag to box
select, drag a node to move it (the whole selection moves together), double-click a node to select its neighbours,
double-click empty space or press `F` to fit. `Esc` clears, `Del` hides the selection, `Ctrl+A` selects all visible,
`Space` pauses/resumes the force layout. Hovering or selecting fades everything but the neighbourhood.

## Headless export / scripting

```
graphy --edges e.csv --nodes n.csv --layout group --group team --color team --size-by _degree \
       --filter 'score > 3' --size 3840x2160 --export-png out.png --export-svg out.svg
graphy --help
```

## Code map

| file | role |
|---|---|
| `data.rs` | CSV/JSON readers, typed columns, column auto-detection |
| `graph.rs` | graph construction, adjacency, metrics (PageRank, k-core, components, Louvain) |
| `expr.rs` | filter expression parser/evaluator |
| `layout.rs` | static layouts and the Barnes–Hut force simulation |
| `model.rs` | session state: appearance, filters, selection, camera, per-frame draw list |
| `render.rs` | PNG (tiny-skia) and SVG export |
| `app.rs`, `panels.rs`, `canvas.rs`, `table.rs` | egui UI; the canvas draws nodes as textured quads and edges as meshes |

`cargo test --release` runs 50 unit tests, including headless UI tests that feed pointer, wheel and keyboard events
through the real egui code and validate the generated meshes. `cargo test --release bench -- --ignored --nocapture`
prints the large-graph timings.

## Known limitations

* Developed and tested on Linux (including the real window under a virtual display); not yet run on Windows or macOS.
* Labels are not included in PNG exports (SVG has them); curved/bundled edges are not implemented.
* Louvain communities are deterministic but not tuned (resolution is fixed at 1.0).
* The whole table is loaded into memory — fine for files of a few MB, not meant for multi-GB inputs.
* Session save/restore is not implemented.
