//! Visual design: an Apple-flavoured dark theme (Inter typeface, soft greys, a single blue accent),
//! plus a few small widgets (switch, segmented control, section headings, cards).

use egui::{
    Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Response, RichText, Sense, Shadow,
    Stroke, TextStyle, Ui, Vec2, WidgetText, vec2,
};
use std::sync::Arc;

// ── palette (inspired by the macOS dark appearance) ──
pub const BG: Color32 = Color32::from_rgb(28, 28, 31); // window / panel background
pub const BG_ELEVATED: Color32 = Color32::from_rgb(38, 38, 42); // cards
pub const BG_CONTROL: Color32 = Color32::from_rgb(56, 56, 60); // buttons, inputs
pub const BG_CONTROL_HOVER: Color32 = Color32::from_rgb(70, 70, 75);
pub const BG_CONTROL_ACTIVE: Color32 = Color32::from_rgb(84, 84, 90);
pub const HAIRLINE: Color32 = Color32::from_rgb(52, 52, 57);
pub const TEXT: Color32 = Color32::from_rgb(240, 240, 245);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(152, 152, 162);
pub const ACCENT: Color32 = Color32::from_rgb(10, 132, 255);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(64, 156, 255);
pub const DANGER: Color32 = Color32::from_rgb(255, 105, 97);

pub fn semibold_family() -> FontFamily {
    FontFamily::Name("semibold".into())
}
pub fn medium_family() -> FontFamily {
    FontFamily::Name("medium".into())
}

/// Text in the semibold weight.
pub fn semibold(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).family(semibold_family())
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let faces: [(&str, &[u8]); 3] = [
        ("Inter-Regular", include_bytes!("../assets/fonts/Inter-Regular.ttf")),
        ("Inter-Medium", include_bytes!("../assets/fonts/Inter-Medium.ttf")),
        ("Inter-SemiBold", include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
    ];
    for (name, bytes) in faces {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    // Inter first; egui's built-in fonts stay behind it as a fallback for symbols and arrows.
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "Inter-Regular".to_owned());
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (family, face) in [(medium_family(), "Inter-Medium"), (semibold_family(), "Inter-SemiBold")] {
        let mut list = vec![face.to_owned()];
        list.extend(fallback.iter().filter(|f| !f.starts_with("Inter-")).cloned());
        fonts.families.insert(family, list);
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.global_style_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.0, medium_family())),
            (TextStyle::Heading, FontId::new(16.0, semibold_family())),
            (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
        ]
        .into();

        let sp = &mut style.spacing;
        sp.item_spacing = vec2(8.0, 7.0);
        sp.button_padding = vec2(11.0, 4.0);
        sp.interact_size = vec2(34.0, 24.0);
        sp.window_margin = Margin::same(14);
        sp.menu_margin = Margin::same(6);
        sp.indent = 14.0;
        sp.combo_height = 320.0;
        sp.slider_rail_height = 4.0;
        sp.icon_width = 16.0;
        sp.icon_width_inner = 8.0;
        sp.icon_spacing = 7.0;

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.override_text_color = None;
        v.panel_fill = BG;
        v.window_fill = Color32::from_rgb(44, 44, 48);
        v.window_stroke = Stroke::new(1.0, Color32::from_rgb(64, 64, 70));
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(10);
        v.window_shadow = Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(110) };
        v.popup_shadow = Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(100) };
        v.faint_bg_color = Color32::from_rgb(34, 34, 38);
        v.extreme_bg_color = Color32::from_rgb(22, 22, 25);
        v.code_bg_color = Color32::from_rgb(44, 44, 48);
        v.hyperlink_color = Color32::from_rgb(100, 170, 255);
        v.warn_fg_color = Color32::from_rgb(255, 204, 0);
        v.error_fg_color = DANGER;
        v.selection.bg_fill = ACCENT.gamma_multiply(0.55);
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.slider_trailing_fill = true;
        v.striped = false;
        v.indent_has_left_vline = false;
        v.handle_shape = egui::style::HandleShape::Circle;

        let r = CornerRadius::same(7);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = BG;
        w.noninteractive.weak_bg_fill = BG;
        w.noninteractive.bg_stroke = Stroke::new(1.0, HAIRLINE);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = r;

        w.inactive.bg_fill = BG_CONTROL;
        w.inactive.weak_bg_fill = BG_CONTROL;
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.corner_radius = r;

        w.hovered.bg_fill = BG_CONTROL_HOVER;
        w.hovered.weak_bg_fill = BG_CONTROL_HOVER;
        w.hovered.bg_stroke = Stroke::NONE;
        w.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        w.hovered.corner_radius = r;
        w.hovered.expansion = 0.0;

        w.active.bg_fill = BG_CONTROL_ACTIVE;
        w.active.weak_bg_fill = BG_CONTROL_ACTIVE;
        w.active.bg_stroke = Stroke::NONE;
        w.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        w.active.corner_radius = r;
        w.active.expansion = 0.0;

        w.open.bg_fill = BG_CONTROL_HOVER;
        w.open.weak_bg_fill = BG_CONTROL_HOVER;
        w.open.bg_stroke = Stroke::NONE;
        w.open.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        w.open.corner_radius = r;
    });
}

/// Colour for secondary (de-emphasised) text.
pub fn secondary(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(TEXT_SECONDARY)
}

// ── layout helpers ──

/// A small, quiet section title with some air above it.
pub fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).family(semibold_family()).size(11.5).color(TEXT_SECONDARY));
    ui.add_space(-2.0);
}

/// A rounded container for a group of related controls inside a panel (e.g. one filter rule).
pub fn inner_card() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgb(46, 46, 51))
        .stroke(Stroke::new(1.0, Color32::from_rgb(60, 60, 66)))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(10, 8))
}

/// Panel body: gives sliders and combo boxes a sensible width for the current panel size.
pub fn panel_body<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let w = ui.available_width();
    ui.spacing_mut().slider_width = (w * 0.5).clamp(70.0, 260.0);
    ui.spacing_mut().combo_width = (w * 0.62).clamp(110.0, 320.0);
    add(ui)
}

pub trait UiExt {
    /// iOS-style on/off switch with a label to its right.
    fn switch(&mut self, on: &mut bool, label: impl Into<WidgetText>) -> Response;
    /// Segmented control: one option is always selected. Returns true if the value changed.
    fn segmented<T: PartialEq + Copy>(&mut self, value: &mut T, options: &[(T, &str)]) -> bool;
}

impl UiExt for Ui {
    fn switch(&mut self, on: &mut bool, label: impl Into<WidgetText>) -> Response {
        let label: WidgetText = label.into();
        let galley = if label.is_empty() { None } else { Some(label.into_galley(self, Some(egui::TextWrapMode::Wrap), (self.available_width() - 48.0).max(40.0), TextStyle::Body)) };
        let track = vec2(32.0, 19.0);
        let gap = if galley.is_some() { 8.0 } else { 0.0 };
        let text_size = galley.as_ref().map(|g| g.size()).unwrap_or(Vec2::ZERO);
        let size = vec2(track.x + gap + text_size.x, track.y.max(text_size.y + 2.0));
        let (rect, mut resp) = self.allocate_exact_size(size, Sense::click());
        if resp.clicked() {
            *on = !*on;
            resp.mark_changed();
        }
        let t = self.ctx().animate_bool_responsive(resp.id, *on);
        if self.is_rect_visible(rect) {
            let track_rect = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - track.y / 2.0), track);
            let off = if resp.hovered() { BG_CONTROL_ACTIVE } else { BG_CONTROL_HOVER };
            let fill = off.lerp_to_gamma(if resp.hovered() { ACCENT_HOVER } else { ACCENT }, t);
            let p = self.painter();
            p.rect_filled(track_rect, CornerRadius::same(10), fill);
            let knob_x = egui::lerp((track_rect.left() + 9.5)..=(track_rect.right() - 9.5), t);
            p.circle_filled(egui::pos2(knob_x, track_rect.center().y + 0.5), 8.2, Color32::from_black_alpha(60));
            p.circle_filled(egui::pos2(knob_x, track_rect.center().y), 8.0, Color32::WHITE);
            if let Some(g) = galley {
                let pos = egui::pos2(track_rect.right() + gap, rect.center().y - g.size().y / 2.0);
                p.galley(pos, g, TEXT);
            }
        }
        resp
    }

    fn segmented<T: PartialEq + Copy>(&mut self, value: &mut T, options: &[(T, &str)]) -> bool {
        let mut changed = false;
        let n = options.len().max(1) as f32;
        let total = self.available_width().min(420.0);
        let h = 26.0;
        let (rect, _) = self.allocate_exact_size(vec2(total, h), Sense::hover());
        self.painter().rect_filled(rect, CornerRadius::same(8), Color32::from_rgb(22, 22, 25));
        let seg_w = (total - 4.0) / n;
        for (k, (v, text)) in options.iter().enumerate() {
            let r = egui::Rect::from_min_size(egui::pos2(rect.left() + 2.0 + seg_w * k as f32, rect.top() + 2.0), vec2(seg_w, h - 4.0));
            let id = self.next_auto_id().with(("seg", k, *text));
            let resp = self.interact(r, id, Sense::click());
            let selected = *value == *v;
            if selected {
                self.painter().rect_filled(r, CornerRadius::same(6), BG_CONTROL_ACTIVE);
            } else if resp.hovered() {
                self.painter().rect_filled(r, CornerRadius::same(6), BG_CONTROL.gamma_multiply(0.7));
            }
            let font = FontId::new(12.0, if selected { semibold_family() } else { medium_family() });
            self.painter().text(r.center(), Align2::CENTER_CENTER, *text, font, if selected { Color32::WHITE } else { TEXT_SECONDARY });
            if resp.clicked() && !selected {
                *value = *v;
                changed = true;
            }
        }
        changed
    }
}

/// Style for the docking area (tab bars and separators), matching the rest of the theme.
pub fn dock_style(ctx: &egui::Context) -> egui_dock::Style {
    let mut s = egui_dock::Style::from_egui(&ctx.global_style());
    s.dock_area_padding = Some(Margin::same(6));
    s.main_surface_border_stroke = Stroke::NONE;
    s.separator.width = 6.0;
    s.separator.extra_interact_width = 4.0;
    s.separator.color_idle = Color32::TRANSPARENT;
    s.separator.color_hovered = ACCENT.gamma_multiply(0.6);
    s.separator.color_dragged = ACCENT;
    s.tab_bar.bg_fill = Color32::TRANSPARENT;
    s.tab_bar.height = 30.0;
    s.tab_bar.hline_color = HAIRLINE;
    s.tab_bar.corner_radius = CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 };
    s.tab_bar.inner_margin = Margin { left: 4, right: 4, top: 3, bottom: 0 };
    s.tab.spacing = 2.0;
    s.tab.hline_below_active_tab_name = false;
    let rr = CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 };
    for (st, fill, text) in [
        (&mut s.tab.active, BG_ELEVATED, TEXT),
        (&mut s.tab.focused, BG_ELEVATED, Color32::WHITE),
        (&mut s.tab.inactive, Color32::TRANSPARENT, TEXT_SECONDARY),
        (&mut s.tab.hovered, BG_CONTROL.gamma_multiply(0.6), TEXT),
    ] {
        st.bg_fill = fill;
        st.text_color = text;
        st.outline_color = Color32::TRANSPARENT;
        st.corner_radius = rr;
    }
    s.tab.active_with_kb_focus = s.tab.active.clone();
    s.tab.inactive_with_kb_focus = s.tab.inactive.clone();
    s.tab.focused_with_kb_focus = s.tab.focused.clone();
    s.tab.tab_body.bg_fill = BG_ELEVATED;
    s.tab.tab_body.stroke = Stroke::NONE;
    s.tab.tab_body.corner_radius = CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 };
    s.tab.tab_body.inner_margin = Margin::symmetric(12, 8);
    s.buttons.close_tab_color = TEXT_SECONDARY;
    s.buttons.close_tab_active_color = Color32::WHITE;
    s.buttons.close_tab_bg_fill = BG_CONTROL;
    s.overlay.selection_color = ACCENT.gamma_multiply(0.35);
    s.overlay.button_color = TEXT_SECONDARY;
    s
}
