//! The dockable workspace: every panel is a tab that can be dragged, tabbed together, split or closed.

use crate::app::App;
use crate::theme;
use egui::{Id, Margin, Ui, WidgetText};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Tab {
    Canvas,
    Data,
    Layout,
    Appearance,
    Filter,
    Analysis,
    #[serde(alias = "Changes")]
    Subgraph,
    Table,
    Inspector,
    Legend,
}

impl Tab {
    pub const ALL: [Tab; 10] =
        [Tab::Canvas, Tab::Data, Tab::Layout, Tab::Appearance, Tab::Filter, Tab::Analysis, Tab::Subgraph, Tab::Table, Tab::Inspector, Tab::Legend];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Canvas => "Graph",
            Tab::Data => "Data",
            Tab::Layout => "Layout",
            Tab::Appearance => "Appearance",
            Tab::Filter => "Filter",
            Tab::Analysis => "Analysis",
            Tab::Subgraph => "Subgraph",
            Tab::Table => "Nodes",
            Tab::Inspector => "Inspector",
            Tab::Legend => "Legend",
        }
    }
}

/// The arrangement shown on first launch and after “Reset layout”:
/// controls on the left, node table at the bottom, inspector and legend on the right.
pub fn default_dock() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Canvas]);
    let tree = dock.main_surface_mut();
    let [center, left] = tree.split_left(NodeIndex::root(), 0.22, vec![Tab::Data, Tab::Layout, Tab::Appearance, Tab::Subgraph]);
    tree.split_below(left, 0.6, vec![Tab::Filter, Tab::Analysis]);
    let [center, _table] = tree.split_below(center, 0.74, vec![Tab::Table]);
    let [_, right] = tree.split_right(center, 0.79, vec![Tab::Inspector]);
    tree.split_below(right, 0.62, vec![Tab::Legend]);
    dock
}

/// A dock must contain the canvas exactly once; anything else (a hand-edited or damaged file) falls back to the default.
pub fn is_sane(dock: &DockState<Tab>) -> bool {
    dock.iter_all_tabs().filter(|(_, t)| **t == Tab::Canvas).count() == 1
}

pub fn is_open(dock: &DockState<Tab>, tab: Tab) -> bool {
    dock.find_tab(&tab).is_some()
}

/// Show a panel again: focus it if it is open, otherwise add it next to the focused one.
pub fn open_or_focus(dock: &mut DockState<Tab>, tab: Tab) {
    if let Some(path) = dock.find_tab(&tab) {
        let _ = dock.set_active_tab(path);
    } else {
        dock.push_to_focused_leaf(tab);
    }
}

pub fn close(dock: &mut DockState<Tab>, tab: Tab) {
    if tab == Tab::Canvas {
        return;
    }
    if let Some(path) = dock.find_tab(&tab) {
        dock.remove_tab(path);
    }
}

struct Viewer<'a> {
    app: &'a mut App,
    close: Option<Tab>,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> Id {
        #[allow(deprecated)]
        Id::new(("grapht-tab", *tab))
    }

    fn title(&mut self, tab: &mut Tab) -> WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match *tab {
            Tab::Canvas => self.app.canvas(ui),
            Tab::Table => self.app.table_panel(ui),
            other => self.app.panel_content(other, ui),
        }
    }

    fn context_menu(&mut self, ui: &mut Ui, tab: &mut Tab, _path: egui_dock::NodePath) {
        if *tab != Tab::Canvas {
            if ui.button("Close panel").clicked() {
                self.close = Some(*tab);
                ui.close();
            }
            ui.label(theme::secondary("Drag the tab to move it, or reopen it from View ▸ Panels."));
        } else {
            ui.label(theme::secondary("The graph view can be moved but not closed."));
        }
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        *tab != Tab::Canvas
    }

    fn scroll_bars(&self, tab: &Tab) -> [bool; 2] {
        match tab {
            Tab::Canvas | Tab::Table => [false, false],
            _ => [false, true],
        }
    }

    fn clear_background(&self, tab: &Tab) -> bool {
        *tab != Tab::Canvas
    }

    fn allowed_in_windows(&self, tab: &mut Tab) -> bool {
        *tab != Tab::Canvas
    }

    fn tab_style_override(&self, tab: &Tab, global: &egui_dock::TabStyle) -> Option<egui_dock::TabStyle> {
        let mut s = global.clone();
        match tab {
            Tab::Canvas => {
                s.tab_body.inner_margin = Margin::ZERO;
                s.tab_body.bg_fill = egui::Color32::TRANSPARENT;
            }
            Tab::Table => s.tab_body.inner_margin = Margin::symmetric(8, 4),
            _ => {}
        }
        Some(s)
    }
}

impl App {
    /// Draw the whole dock workspace into `ui`.
    pub fn show_dock(&mut self, ui: &mut Ui) {
        let mut dock = self.dock.take().unwrap_or_else(default_dock);
        let style = theme::dock_style(ui.ctx());
        let mut viewer = Viewer { app: self, close: None };
        DockArea::new(&mut dock)
            .style(style)
            .show_close_buttons(false)
            .show_add_buttons(false)
            .show_leaf_close_all_buttons(false)
            .show_leaf_collapse_buttons(false)
            .draggable_tabs(true)
            .show_inside(ui, &mut viewer);
        if let Some(t) = viewer.close {
            close(&mut dock, t);
        }
        self.dock = Some(dock);
    }
}
