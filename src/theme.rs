//! Visual design: an Apple-flavoured dark theme (Inter typeface, soft greys, a single blue accent),
//! plus a few small widgets (switch, segmented control, section headings, cards).

use egui::{
    Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Response, RichText, Sense, Shadow,
    Stroke, TextStyle, Ui, Vec2, WidgetText, vec2,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ── palette (inspired by the macOS appearances) ──

/// Every colour of the interface for one appearance.
#[derive(Clone, Copy)]
pub struct Pal {
    pub bg: Color32,            // window / panel background
    pub bg_elevated: Color32,   // cards, panel bodies
    pub bg_control: Color32,    // buttons, inputs
    pub bg_control_hover: Color32,
    pub bg_control_active: Color32,
    pub hairline: Color32,
    pub text: Color32,
    pub text_strong: Color32,
    pub text_secondary: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub danger: Color32,
    pub window_fill: Color32,
    pub window_stroke: Color32,
    pub faint_bg: Color32,
    pub extreme_bg: Color32,
    pub card_fill: Color32,
    pub card_stroke: Color32,
    pub seg_track: Color32,
    pub seg_selected: Color32,
    pub shadow: u8,
}

pub const DARK: Pal = Pal {
    bg: Color32::from_rgb(28, 28, 31),
    bg_elevated: Color32::from_rgb(38, 38, 42),
    bg_control: Color32::from_rgb(56, 56, 60),
    bg_control_hover: Color32::from_rgb(70, 70, 75),
    bg_control_active: Color32::from_rgb(84, 84, 90),
    hairline: Color32::from_rgb(52, 52, 57),
    text: Color32::from_rgb(240, 240, 245),
    text_strong: Color32::WHITE,
    text_secondary: Color32::from_rgb(152, 152, 162),
    accent: Color32::from_rgb(10, 132, 255),
    accent_hover: Color32::from_rgb(64, 156, 255),
    danger: Color32::from_rgb(255, 105, 97),
    window_fill: Color32::from_rgb(44, 44, 48),
    window_stroke: Color32::from_rgb(64, 64, 70),
    faint_bg: Color32::from_rgb(34, 34, 38),
    extreme_bg: Color32::from_rgb(22, 22, 25),
    card_fill: Color32::from_rgb(46, 46, 51),
    card_stroke: Color32::from_rgb(60, 60, 66),
    seg_track: Color32::from_rgb(22, 22, 25),
    seg_selected: Color32::from_rgb(84, 84, 90),
    shadow: 110,
};

pub const LIGHT: Pal = Pal {
    bg: Color32::from_rgb(236, 236, 240),
    bg_elevated: Color32::from_rgb(250, 250, 252),
    bg_control: Color32::from_rgb(224, 224, 230),
    bg_control_hover: Color32::from_rgb(212, 212, 219),
    bg_control_active: Color32::from_rgb(198, 198, 207),
    hairline: Color32::from_rgb(210, 210, 217),
    text: Color32::from_rgb(28, 28, 32),
    text_strong: Color32::BLACK,
    text_secondary: Color32::from_rgb(104, 104, 116),
    accent: Color32::from_rgb(0, 122, 255),
    accent_hover: Color32::from_rgb(52, 140, 255),
    danger: Color32::from_rgb(214, 48, 40),
    window_fill: Color32::from_rgb(250, 250, 252),
    window_stroke: Color32::from_rgb(205, 205, 212),
    faint_bg: Color32::from_rgb(242, 242, 246),
    extreme_bg: Color32::from_rgb(255, 255, 255),
    card_fill: Color32::from_rgb(255, 255, 255),
    card_stroke: Color32::from_rgb(214, 214, 221),
    seg_track: Color32::from_rgb(218, 218, 225),
    seg_selected: Color32::from_rgb(255, 255, 255),
    shadow: 45,
};

pub fn pal(dark: bool) -> &'static Pal {
    if dark { &DARK } else { &LIGHT }
}

/// The palette of the appearance a `Ui` is drawn in.
fn pal_of(ui: &Ui) -> &'static Pal {
    pal(ui.visuals().dark_mode)
}

/// Palette of the appearance currently shown by the context (without translucency).
pub fn pal_ctx(ctx: &egui::Context) -> &'static Pal {
    pal(ctx.theme() == egui::Theme::Dark)
}

// ── translucency ──

/// The material Windows 11 draws behind the window (shows through wherever the interface is see-through).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Backdrop {
    None,
    Mica,
    MicaAlt,
    Acrylic,
}

/// How solid each kind of surface is: 1.0 is a plain opaque surface, lower lets the backdrop through.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Translucency {
    /// The menu bar, the panels and the space around them.
    pub panels: f32,
    /// The graph view.
    pub board: f32,
    /// Menus, popups and floating windows.
    pub menus: f32,
}

impl Translucency {
    pub const OPAQUE: Translucency = Translucency { panels: 1.0, board: 1.0, menus: 1.0 };

    /// Does the backdrop show through anywhere? (Then the window itself has to be cleared to transparent.)
    pub fn any(&self) -> bool {
        self.panels < 0.999 || self.board < 0.999
    }
}

#[allow(deprecated)]
fn tl_id() -> egui::Id {
    egui::Id::new("grapht-translucency")
}

/// The translucency the interface is drawn with right now.
pub fn translucency(ctx: &egui::Context) -> Translucency {
    ctx.data(|d| d.get_temp::<[f32; 3]>(tl_id())).map_or(Translucency::OPAQUE, |t| Translucency { panels: t[0], board: t[1], menus: t[2] })
}

/// The same colour, with its opacity multiplied by `k` (0 to 1).
pub fn faded(c: Color32, k: f32) -> Color32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (a as f32 * k.clamp(0.0, 1.0)).round() as u8)
}

/// The palette with the translucency applied to the surfaces it describes.
fn pal_faded(dark: bool, t: Translucency) -> Pal {
    let mut p = *pal(dark);
    p.bg = faded(p.bg, t.panels);
    // a panel's body sits over the space around it: lighter, so the two together are about as see-through as the setting
    p.bg_elevated = faded(p.bg_elevated, if t.panels >= 0.999 { 1.0 } else { t.panels * 0.6 });
    p.window_fill = faded(p.window_fill, t.menus);
    p
}

/// Palette of the current appearance, translucency included (what the panels, the menu bar and the tabs are filled with).
pub fn pal_tl(ctx: &egui::Context) -> Pal {
    pal_faded(ctx.theme() == egui::Theme::Dark, translucency(ctx))
}

/// Make the interface see-through by these amounts. Restyles both appearances; cheap enough to call when a slider moves.
pub fn set_translucency(ctx: &egui::Context, t: Translucency) {
    if translucency(ctx) == t {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(tl_id(), [t.panels, t.board, t.menus]));
    for (theme, dark) in [(egui::Theme::Dark, true), (egui::Theme::Light, false)] {
        ctx.style_mut_of(theme, |style| build_style(style, &pal_faded(dark, t), dark));
    }
    ctx.request_repaint();
}

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

/// Fonts, text sizes and the colours of both appearances. Which one is shown is decided by `set_mode`.
pub fn init(ctx: &egui::Context, mode: ThemeMode) {
    install_fonts(ctx);
    for (theme, dark) in [(egui::Theme::Dark, true), (egui::Theme::Light, false)] {
        ctx.style_mut_of(theme, |style| build_style(style, pal(dark), dark));
    }
    set_mode(ctx, mode);
}

/// Dark, light, or follow the operating system.
pub fn set_mode(ctx: &egui::Context, mode: ThemeMode) {
    ctx.set_theme(match mode {
        ThemeMode::Dark => egui::ThemePreference::Dark,
        ThemeMode::Light => egui::ThemePreference::Light,
        ThemeMode::System => egui::ThemePreference::System,
    });
}

/// How the interface is coloured. Saved with the user's settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ThemeMode {
    Dark,
    Light,
    System,
}

fn build_style(style: &mut egui::Style, p: &Pal, dark: bool) {
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
    v.dark_mode = dark;
    v.override_text_color = None;
    v.weak_text_color = Some(p.text_secondary);
    v.panel_fill = p.bg;
    v.window_fill = p.window_fill;
    v.window_stroke = Stroke::new(1.0, p.window_stroke);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(p.shadow) };
    v.popup_shadow = Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(p.shadow) };
    v.faint_bg_color = p.faint_bg;
    v.extreme_bg_color = p.extreme_bg;
    v.code_bg_color = p.window_fill;
    v.hyperlink_color = if dark { Color32::from_rgb(100, 170, 255) } else { Color32::from_rgb(0, 100, 220) };
    v.warn_fg_color = if dark { Color32::from_rgb(255, 204, 0) } else { Color32::from_rgb(176, 120, 0) };
    v.error_fg_color = p.danger;
    v.selection.bg_fill = p.accent.gamma_multiply(if dark { 0.55 } else { 0.35 });
    v.selection.stroke = Stroke::new(1.0, if dark { Color32::WHITE } else { p.accent });
    v.slider_trailing_fill = true;
    v.striped = false;
    v.indent_has_left_vline = false;
    v.handle_shape = egui::style::HandleShape::Circle;

    let r = CornerRadius::same(7);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.bg;
    w.noninteractive.weak_bg_fill = p.bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.hairline);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r;

    w.inactive.bg_fill = p.bg_control;
    w.inactive.weak_bg_fill = p.bg_control;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = r;

    w.hovered.bg_fill = p.bg_control_hover;
    w.hovered.weak_bg_fill = p.bg_control_hover;
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.fg_stroke = Stroke::new(1.0, p.text_strong);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.bg_control_active;
    w.active.weak_bg_fill = p.bg_control_active;
    w.active.bg_stroke = Stroke::NONE;
    w.active.fg_stroke = Stroke::new(1.0, p.text_strong);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;

    w.open.bg_fill = p.bg_control_hover;
    w.open.weak_bg_fill = p.bg_control_hover;
    w.open.bg_stroke = Stroke::NONE;
    w.open.fg_stroke = Stroke::new(1.0, p.text_strong);
    w.open.corner_radius = r;
}

/// Colour for secondary (de-emphasised) text.
pub fn secondary(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).weak()
}

// ── layout helpers ──

/// A small, quiet section title with some air above it.
pub fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).family(semibold_family()).size(11.5).weak());
    ui.add_space(-2.0);
}

/// A rounded container for a group of related controls inside a panel (e.g. one filter rule).
pub fn inner_card(ui: &Ui) -> egui::Frame {
    let p = pal_of(ui);
    egui::Frame::new()
        .fill(p.card_fill)
        .stroke(Stroke::new(1.0, p.card_stroke))
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
        let pl = pal_of(self);
        if self.is_rect_visible(rect) {
            let track_rect = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - track.y / 2.0), track);
            let off = if resp.hovered() { pl.bg_control_active } else { pl.bg_control_hover };
            let fill = off.lerp_to_gamma(if resp.hovered() { pl.accent_hover } else { pl.accent }, t);
            let p = self.painter();
            p.rect_filled(track_rect, CornerRadius::same(10), fill);
            let knob_x = egui::lerp((track_rect.left() + 9.5)..=(track_rect.right() - 9.5), t);
            p.circle_filled(egui::pos2(knob_x, track_rect.center().y + 0.5), 8.2, Color32::from_black_alpha(60));
            p.circle_filled(egui::pos2(knob_x, track_rect.center().y), 8.0, Color32::WHITE);
            if let Some(g) = galley {
                let pos = egui::pos2(track_rect.right() + gap, rect.center().y - g.size().y / 2.0);
                p.galley(pos, g, pl.text);
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
        let pl = pal_of(self);
        self.painter().rect_filled(rect, CornerRadius::same(8), pl.seg_track);
        let seg_w = (total - 4.0) / n;
        for (k, (v, text)) in options.iter().enumerate() {
            let r = egui::Rect::from_min_size(egui::pos2(rect.left() + 2.0 + seg_w * k as f32, rect.top() + 2.0), vec2(seg_w, h - 4.0));
            let id = self.next_auto_id().with(("seg", k, *text));
            let resp = self.interact(r, id, Sense::click());
            let selected = *value == *v;
            if selected {
                self.painter().rect_filled(r, CornerRadius::same(6), pl.seg_selected);
            } else if resp.hovered() {
                self.painter().rect_filled(r, CornerRadius::same(6), pl.bg_control.gamma_multiply(0.7));
            }
            let font = FontId::new(12.0, if selected { semibold_family() } else { medium_family() });
            self.painter().text(r.center(), Align2::CENTER_CENTER, *text, font, if selected { pl.text_strong } else { pl.text_secondary });
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
    let p = &pal_tl(ctx);
    let mut s = egui_dock::Style::from_egui(&ctx.global_style());
    s.dock_area_padding = Some(Margin::same(6));
    s.main_surface_border_stroke = Stroke::NONE;
    s.separator.width = 6.0;
    s.separator.extra_interact_width = 4.0;
    s.separator.color_idle = Color32::TRANSPARENT;
    s.separator.color_hovered = p.accent.gamma_multiply(0.6);
    s.separator.color_dragged = p.accent;
    s.tab_bar.bg_fill = Color32::TRANSPARENT;
    s.tab_bar.height = 30.0;
    s.tab_bar.hline_color = p.hairline;
    s.tab_bar.corner_radius = CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 };
    s.tab_bar.inner_margin = Margin { left: 4, right: 4, top: 3, bottom: 0 };
    s.tab.spacing = 2.0;
    s.tab.hline_below_active_tab_name = false;
    let rr = CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 };
    for (st, fill, text) in [
        (&mut s.tab.active, p.bg_elevated, p.text),
        (&mut s.tab.focused, p.bg_elevated, p.text_strong),
        (&mut s.tab.inactive, Color32::TRANSPARENT, p.text_secondary),
        (&mut s.tab.hovered, p.bg_control.gamma_multiply(0.6), p.text),
    ] {
        st.bg_fill = fill;
        st.text_color = text;
        st.outline_color = Color32::TRANSPARENT;
        st.corner_radius = rr;
    }
    s.tab.active_with_kb_focus = s.tab.active.clone();
    s.tab.inactive_with_kb_focus = s.tab.inactive.clone();
    s.tab.focused_with_kb_focus = s.tab.focused.clone();
    s.tab.tab_body.bg_fill = p.bg_elevated;
    s.tab.tab_body.stroke = Stroke::NONE;
    s.tab.tab_body.corner_radius = CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 };
    s.tab.tab_body.inner_margin = Margin::symmetric(12, 8);
    s.buttons.close_tab_color = p.text_secondary;
    s.buttons.close_tab_active_color = p.text_strong;
    s.buttons.close_tab_bg_fill = p.bg_control;
    s.overlay.selection_color = p.accent.gamma_multiply(0.35);
    s.overlay.button_color = p.text_secondary;
    s
}
