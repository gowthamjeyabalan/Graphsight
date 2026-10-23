//! Sortable node table (virtualised: only the visible rows are laid out).

use crate::app::*;
use crate::data::natural_cmp;
use egui::{Align, Color32, Layout, RichText, Sense, Ui};
use egui_extras::{Column as TCol, TableBuilder};

impl App {
    /// Take an attribute column out of the table (it can be brought back from the Columns menu).
    pub fn hide_column(&mut self, name: &str) {
        if name != "id" && !self.table.hidden.iter().any(|h| h == name) {
            self.table.hidden.push(name.to_string());
        }
        self.table.picked.retain(|p| p != name);
    }

    /// “Columns” button of the table: tick the attribute columns to show, hide the picked ones, show everything again.
    fn columns_menu(&mut self, ui: &mut Ui) {
        let n_hidden = self.table.hidden.len();
        let label = if n_hidden > 0 { format!("Columns ({n_hidden} hidden) ▾") } else { "Columns ▾".to_string() };
        let names: Vec<String> = self.s.g.node_cols.iter().map(|c| c.name.clone()).collect();
        ui.menu_button(label, |ui| {
            ui.set_min_width(190.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(n_hidden > 0, egui::Button::new("Show all")).clicked() {
                    self.table.hidden.clear();
                }
                let picked = self.table.picked.len();
                if ui.add_enabled(picked > 0, egui::Button::new(format!("Hide picked ({picked})"))).clicked() {
                    for p in self.table.picked.clone() {
                        self.hide_column(&p);
                    }
                    self.table.picked.clear();
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for name in &names {
                    let mut on = !self.table.hidden.contains(name);
                    if ui.checkbox(&mut on, name).changed() {
                        if on {
                            self.table.hidden.retain(|h| h != name);
                        } else {
                            self.hide_column(name);
                        }
                    }
                }
            });
            ui.separator();
            ui.weak("Ctrl+click column headers to pick several, right-click a header to hide it.");
        });
    }

    fn rebuild_table_order(&mut self) {
        let g = &self.s.g;
        let mut order: Vec<u32> = (0..g.n() as u32).filter(|&i| self.s.vis[i as usize]).collect();
        if let Some((name, asc)) = &self.table.sort {
            if name == "id" {
                order.sort_by(|&a, &b| natural_cmp(&g.ids[a as usize], &g.ids[b as usize]));
                if !*asc {
                    order.reverse();
                }
            } else if let Some(c) = g.node_col(name) {
                order.sort_by(|&a, &b| {
                    // missing values stay last in both directions
                    let (ma, mb) = (c.num(a as usize).is_none() && c.code(a as usize).is_none(), c.num(b as usize).is_none() && c.code(b as usize).is_none());
                    match (ma, mb) {
                        (true, true) => std::cmp::Ordering::Equal,
                        (true, false) => std::cmp::Ordering::Greater,
                        (false, true) => std::cmp::Ordering::Less,
                        _ => {
                            let o = c.cmp_rows(a as usize, b as usize);
                            if *asc { o } else { o.reverse() }
                        }
                    }
                });
            }
        }
        self.table.order = order;
        self.table.dirty = false;
    }

    pub fn table_panel(&mut self, ui: &mut Ui) {
        if self.table.dirty || self.table.order.len() > self.s.g.n() {
            self.rebuild_table_order();
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Nodes ({})", self.table.order.len())).strong());
            match &self.table.sort {
                Some((c, asc)) => {
                    ui.weak(format!("sorted by {} {}", c, if *asc { "▲" } else { "▼" }));
                    if ui.small_button("clear sort").clicked() {
                        self.table.sort = None;
                        self.table.dirty = true;
                    }
                }
                None => {
                    ui.weak("click a column header to sort");
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.columns_menu(ui);
                if ui.small_button("Select all listed").clicked() {
                    self.s.clear_selection();
                    for &i in &self.table.order {
                        self.s.set_selected(i as usize, true);
                    }
                }
            });
        });
        if self.s.g.n() == 0 {
            return;
        }

        // forget hidden / picked columns that no longer exist (another file was loaded)
        {
            let have: Vec<&str> = self.s.g.node_cols.iter().map(|c| c.name.as_str()).collect();
            self.table.hidden.retain(|h| have.contains(&h.as_str()));
            self.table.picked.retain(|h| have.contains(&h.as_str()));
        }
        // the columns that are shown: indices into the graph's attribute columns
        let shown_cols: Vec<usize> = (0..self.s.g.node_cols.len()).filter(|&k| !self.table.hidden.contains(&self.s.g.node_cols[k].name)).collect();
        let n_cols = 1 + shown_cols.len();
        let mut clicked_header: Option<String> = None;
        let mut pick_header: Option<String> = None;
        let mut hide_now: Vec<String> = vec![];
        let mut clicked_row: Option<(usize, bool)> = None;
        let mut dbl_row: Option<usize> = None;
        let order = std::mem::take(&mut self.table.order);
        let sort = self.table.sort.clone();
        let picked = self.table.picked.clone();

        egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
            let mut tb = TableBuilder::new(ui).striped(true).resizable(true).sense(Sense::click()).cell_layout(Layout::left_to_right(Align::Center)).vscroll(true);
            tb = tb.column(TCol::initial(130.0).at_least(60.0).clip(true));
            for _ in 1..n_cols {
                tb = tb.column(TCol::initial(95.0).at_least(40.0).clip(true));
            }
            tb.header(20.0, |mut header| {
                let mut names = vec!["id".to_string()];
                names.extend(shown_cols.iter().map(|&k| self.s.g.node_cols[k].name.clone()));
                for name in names {
                    header.col(|ui| {
                        let arrow = match &sort {
                            Some((c, asc)) if *c == name => {
                                if *asc { " ▲" } else { " ▼" }
                            }
                            _ => "",
                        };
                        let is_picked = picked.contains(&name);
                        let mut text = RichText::new(format!("{name}{arrow}")).strong();
                        if is_picked {
                            text = text.color(ui.visuals().selection.stroke.color).underline();
                        }
                        let resp = ui.add(egui::Label::new(text).sense(Sense::click()));
                        if resp.clicked() {
                            if name != "id" && ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.shift) {
                                pick_header = Some(name.clone());
                            } else {
                                clicked_header = Some(name.clone());
                            }
                        }
                        if name != "id" {
                            resp.context_menu(|ui| {
                                if ui.button(format!("Hide “{name}”")).clicked() {
                                    hide_now.push(name.clone());
                                    ui.close();
                                }
                                if is_picked && picked.len() > 1 && ui.button(format!("Hide the {} picked columns", picked.len())).clicked() {
                                    hide_now.extend(picked.iter().cloned());
                                    ui.close();
                                }
                                ui.weak("Ctrl+click headers to pick several columns.");
                            });
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(18.0, order.len(), |mut row| {
                    let i = order[row.index()] as usize;
                    row.set_selected(self.s.sel[i]);
                    row.col(|ui| {
                        let c = self.s.style.node_rgb[i];
                        let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.5, Color32::from_rgb(c[0], c[1], c[2]));
                        ui.label(&self.s.g.ids[i]);
                    });
                    for &k in &shown_cols {
                        let c = &self.s.g.node_cols[k];
                        row.col(|ui| match c.label_at(i) {
                            Some(l) => {
                                ui.label(l);
                            }
                            None => {
                                ui.weak("—");
                            }
                        });
                    }
                    let resp = row.response();
                    if resp.double_clicked() {
                        dbl_row = Some(i);
                    } else if resp.clicked() {
                        let m = ui_modifiers(&resp);
                        clicked_row = Some((i, m));
                    }
                });
            });
        });
        self.table.order = order;

        if let Some(name) = pick_header {
            match self.table.picked.iter().position(|p| *p == name) {
                Some(k) => {
                    self.table.picked.remove(k);
                }
                None => self.table.picked.push(name),
            }
        }
        for name in hide_now {
            self.hide_column(&name);
        }
        if let Some(name) = clicked_header {
            self.table.sort = match &self.table.sort {
                Some((c, true)) if *c == name => Some((name, false)),
                Some((c, false)) if *c == name => None,
                _ => Some((name, true)),
            };
            self.table.dirty = true;
        }
        if let Some((i, additive)) = clicked_row {
            if additive {
                self.s.toggle_selected(i);
            } else {
                self.s.select_only(i);
            }
        }
        if let Some(i) = dbl_row {
            self.select_and_focus(i);
        }
    }
}

fn ui_modifiers(resp: &egui::Response) -> bool {
    resp.ctx.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.shift)
}
