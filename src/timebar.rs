//! The time bar: a rounded bar along the top of the graph, like a touch bar, for a date attribute. Its scale shows
//! days, months or years depending on how far it is zoomed (the scroll wheel over the bar zooms, Shift + scroll
//! pans); a vertical line marks the moment shown. Nodes dated after it — or, with "only the period", outside the
//! day, month or year under it — are taken out like filtered-out nodes (dimmed or hidden). ▶ plays time forward.

use crate::app::App;
use crate::theme::UiExt;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeSettings {
    /// The node attribute holding the dates.
    pub col: Option<String>,
    pub show: bool,
    /// The moment shown, and the part of time the bar spans (days since 1970-01-01).
    pub cursor: Option<f64>,
    pub view: Option<[f64; 2]>,
    /// Keep only what is dated in the period (day, month or year) under the line, instead of everything up to it.
    pub only_period: bool,
    pub period: Unit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Unit {
    Day,
    #[default]
    Month,
    Year,
}

impl Unit {
    /// The unit a bar spanning this many days shows.
    pub fn for_span(days: f64) -> Unit {
        if days <= 92.0 {
            Unit::Day
        } else if days <= 365.0 * 6.0 {
            Unit::Month
        } else {
            Unit::Year
        }
    }
}

// ── calendar arithmetic (proleptic Gregorian, days since 1970-01-01) ──

pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

fn month_of(word: &str) -> Option<u32> {
    let w = word.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase();
    if w.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| w.starts_with(m)).map(|i| i as u32 + 1)
}

fn valid(y: i64, m: u32, d: u32) -> Option<f64> {
    ((1..=12).contains(&m) && (1..=31).contains(&d) && (1000..=9999).contains(&y)).then(|| days_from_civil(y, m, d) as f64)
}

/// A date (with an optional time of day) as days since 1970-01-01: 2024-03-12, 2024/03/12, 12/03/2024, 12.03.2024,
/// 2024-03, 2024, 12 Mar 2024, Mar 2024, March 12, 2024, an ISO time like 2024-03-12T14:30, or an Excel date number.
pub fn parse_date(text: &str) -> Option<f64> {
    let s = text.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(v) = s.parse::<f64>() {
        return from_number(v);
    }
    // the time of day, if any
    let (date, time) = match s.find(|c: char| c == 'T' || c == ' ').filter(|&i| s[i + 1..].contains(':')) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let frac = time.and_then(|t| {
        let mut p = t.trim_end_matches('Z').split(':').map(|x| x.trim().parse::<f64>().ok());
        let (h, m) = (p.next()??, p.next().flatten().unwrap_or(0.0));
        let sec = p.next().flatten().unwrap_or(0.0);
        Some((h * 3600.0 + m * 60.0 + sec) / 86400.0)
    });
    let base = if date.chars().any(|c| c.is_alphabetic()) {
        // with a month name: "12 Mar 2024", "Mar 2024", "March 12, 2024"
        let words: Vec<&str> = date.split(|c: char| c == ' ' || c == ',' || c == '-' || c == '/' || c == '.').filter(|w| !w.is_empty()).collect();
        let m = words.iter().find_map(|w| month_of(w))?;
        let nums: Vec<i64> = words.iter().filter_map(|w| w.parse::<i64>().ok()).collect();
        let y = *nums.iter().find(|&&n| n >= 1000)?;
        let d = nums.iter().find(|&&n| (1..=31).contains(&n)).copied().unwrap_or(1) as u32;
        valid(y, m, d)
    } else {
        let parts: Vec<i64> = date.split(|c| c == '-' || c == '/' || c == '.').map(|p| p.trim().parse::<i64>().ok()).collect::<Option<_>>()?;
        match parts.as_slice() {
            [y] if *y >= 1000 => valid(*y, 1, 1),
            [y, m] if *y >= 1000 => valid(*y, *m as u32, 1),
            [y, m, d] if *y >= 1000 => valid(*y, *m as u32, *d as u32),
            // day first, as written in most of the world
            [d, m, y] if *y >= 1000 => valid(*y, *m as u32, *d as u32),
            _ => None,
        }
    }?;
    Some(base + frac.unwrap_or(0.0))
}

/// A number as a date: a year (1000 – 3000) or an Excel date (days since 1899-12-30).
pub fn from_number(v: f64) -> Option<f64> {
    if !v.is_finite() {
        None
    } else if (1000.0..=3000.0).contains(&v) && v.fract() == 0.0 {
        Some(days_from_civil(v as i64, 1, 1) as f64)
    } else if (10000.0..=100000.0).contains(&v) {
        Some(v - 25569.0)
    } else {
        None
    }
}

/// The start of the day / month / year a moment falls in.
pub fn bucket_start(t: f64, unit: Unit) -> f64 {
    let (y, m, _) = civil_from_days(t.floor() as i64);
    match unit {
        Unit::Day => t.floor(),
        Unit::Month => days_from_civil(y, m, 1) as f64,
        Unit::Year => days_from_civil(y, 1, 1) as f64,
    }
}

/// The start of the next day / month / year.
pub fn bucket_next(start: f64, unit: Unit) -> f64 {
    let (y, m, _) = civil_from_days(start.floor() as i64);
    match unit {
        Unit::Day => start.floor() + 1.0,
        Unit::Month => (if m == 12 { days_from_civil(y + 1, 1, 1) } else { days_from_civil(y, m + 1, 1) }) as f64,
        Unit::Year => days_from_civil(y + 1, 1, 1) as f64,
    }
}

/// "12 Mar 2024", "Mar 2024" or "2024".
pub fn label(t: f64, unit: Unit) -> String {
    let (y, m, d) = civil_from_days(t.floor() as i64);
    let mon = MONTHS[(m - 1) as usize];
    let mon = format!("{}{}", mon[..1].to_uppercase(), &mon[1..]);
    match unit {
        Unit::Day => format!("{d} {mon} {y}"),
        Unit::Month => format!("{mon} {y}"),
        Unit::Year => y.to_string(),
    }
}

/// Every node's date in a column (NaN where there is none).
pub fn node_days(col: &crate::data::Column) -> Vec<f64> {
    (0..col.len())
        .map(|i| match col.num(i) {
            Some(v) => from_number(v),
            None => col.cat_str(i).and_then(parse_date),
        }.unwrap_or(f64::NAN))
        .collect()
}

/// Does a node dated `d` pass the time bar set to `cursor`?
pub fn passes(d: f64, cursor: f64, only_period: bool, unit: Unit) -> bool {
    if !d.is_finite() {
        return true; // undated nodes are always there
    }
    if only_period {
        bucket_start(d, unit) == bucket_start(cursor, unit)
    } else {
        d <= cursor
    }
}

// ───────────────────────── the bar ─────────────────────────

impl App {
    /// The date attribute was chosen: show the whole range, with the line at its end (everything shown).
    pub fn time_reset(&mut self) {
        let days = self.s.time_days();
        let (lo, hi) = days.iter().filter(|d| d.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &d| (a.min(d), b.max(d)));
        if lo.is_finite() {
            let pad = ((hi - lo) * 0.04).max(1.0);
            self.s.time.view = Some([lo - pad, hi + pad]);
            self.s.time.cursor = Some(hi);
        }
        self.time_playing = false;
        self.refilter();
    }

    /// Filter panel: the date attribute and the bar.
    pub fn time_section(&mut self, ui: &mut Ui, cols: &[crate::panels::ColInfo]) {
        ui.add_space(4.0);
        crate::theme::heading(ui, "Time");
        let before = self.s.time.col.clone();
        ui.horizontal(|ui| {
            ui.label("Dates in");
            egui::ComboBox::from_id_salt("time_col").selected_text(self.s.time.col.clone().unwrap_or_else(|| "(none)".into())).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.s.time.col, None, "(none)");
                for c in cols.iter().filter(|c| !c.metric) {
                    ui.selectable_value(&mut self.s.time.col, Some(c.name.clone()), &c.name);
                }
            });
        });
        if self.s.time.col != before {
            self.s.time.show = self.s.time.col.is_some();
            self.time_reset();
        }
        if self.s.time.col.is_some() {
            let dated = self.s.time_days().iter().filter(|d| d.is_finite()).count();
            if dated == 0 {
                ui.colored_label(Color32::from_rgb(235, 100, 90), "No dates could be read in this column.");
            } else {
                ui.weak(format!("{dated} nodes have a date"));
            }
            let mut changed = ui.switch(&mut self.s.time.show, "Show the time bar").changed();
            ui.horizontal(|ui| {
                changed |= ui.switch(&mut self.s.time.only_period, "Only the").on_hover_text("Off: everything up to the line").changed();
                ui.add_enabled_ui(self.s.time.only_period, |ui| {
                    changed |= ui.segmented(&mut self.s.time.period, &[(Unit::Day, "day"), (Unit::Month, "month"), (Unit::Year, "year")]);
                });
                ui.label("under the line");
            });
            if changed {
                self.refilter();
            }
        } else {
            ui.weak("Choose a date attribute to step through time with a bar along the top of the graph.");
        }
    }

    /// Draw the bar along the top of the graph and handle it. Returns the area it covers.
    pub fn time_bar(&mut self, ui: &mut Ui, canvas: Rect) -> Option<Rect> {
        let t = &self.s.time;
        if !t.show || t.col.is_none() || canvas.width() < 360.0 {
            return None;
        }
        let [v0, v1] = t.view?;
        let cursor = t.cursor?;
        let only = t.only_period;
        let period = t.period;
        let days = self.s.time_days();
        let span = (v1 - v0).max(1e-3);
        let unit = Unit::for_span(span);

        // under the line of counts at the top left, clear of the full-screen button
        let w = (canvas.width() - 150.0).clamp(320.0, 980.0);
        let bar = Rect::from_min_size(Pos2::new(canvas.center().x - w * 0.5, canvas.top() + 34.0), Vec2::new(w, 78.0));
        let id = ui.make_persistent_id("time_bar");
        let resp = ui.interact(bar, id, Sense::click_and_drag());
        let v = ui.visuals().clone();
        let accent = Color32::from_rgb(10, 132, 255);
        let ink = v.text_color();
        let weak = v.weak_text_color();
        let painter = ui.painter_at(bar.expand(2.0));
        painter.rect_filled(bar, 18.0, v.window_fill().gamma_multiply(0.94));
        painter.rect_stroke(bar, 18.0, Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Inside);

        // play button, at the left
        let play = Rect::from_center_size(Pos2::new(bar.left() + 26.0, bar.center().y + 6.0), Vec2::splat(30.0));
        let play_resp = ui.interact(play, id.with("play"), Sense::click()).on_hover_text("Play time forward");
        painter.circle_filled(play.center(), 15.0, if play_resp.hovered() { v.widgets.hovered.bg_fill } else { v.widgets.inactive.bg_fill });
        if self.time_playing {
            for dx in [-4.0, 4.0] {
                painter.rect_filled(Rect::from_center_size(play.center() + Vec2::new(dx, 0.0), Vec2::new(3.5, 12.0)), 1.0, ink);
            }
        } else {
            let c = play.center() + Vec2::new(1.5, 0.0);
            painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(-4.5, -7.0), c + Vec2::new(6.5, 0.0), c + Vec2::new(-4.5, 7.0)], ink, Stroke::NONE));
        }

        // the unit chips along the top: click one to zoom to that scale
        let track = Rect::from_min_max(Pos2::new(bar.left() + 52.0, bar.top() + 28.0), Pos2::new(bar.right() - 16.0, bar.bottom() - 10.0));
        let chips = [("Days", Unit::Day, 30.0), ("Months", Unit::Month, 730.0), ("Years", Unit::Year, f64::NAN)];
        let mut x = track.left();
        let mut zoom_to: Option<f64> = None;
        for (name, u, days_span) in chips {
            let galley = painter.layout_no_wrap(name.to_string(), FontId::proportional(11.5), if u == unit { Color32::WHITE } else { weak });
            let r = Rect::from_min_size(Pos2::new(x, bar.top() + 7.0), Vec2::new(galley.size().x + 16.0, 17.0));
            let cr = ui.interact(r, id.with(name), Sense::click()).on_hover_text(format!("Show the bar in {}", name.to_lowercase()));
            if u == unit {
                painter.rect_filled(r, 8.5, accent);
            } else if cr.hovered() {
                painter.rect_filled(r, 8.5, v.widgets.hovered.bg_fill);
            }
            painter.galley(Pos2::new(r.left() + 8.0, r.center().y - galley.size().y * 0.5), galley, ink);
            if cr.clicked() {
                zoom_to = Some(days_span);
            }
            x = r.right() + 4.0;
        }
        let close = Rect::from_center_size(Pos2::new(bar.right() - 18.0, bar.top() + 15.5), Vec2::splat(16.0));
        let close_resp = ui.interact(close, id.with("close"), Sense::click()).on_hover_text("Hide the time bar (Filter ▸ Time)");
        painter.text(close.center(), Align2::CENTER_CENTER, "×", FontId::proportional(15.0), if close_resp.hovered() { ink } else { weak });

        let to_x = |d: f64| track.left() + ((d - v0) / span) as f32 * track.width();
        let to_t = |x: f32| v0 + ((x - track.left()) / track.width()) as f64 * span;

        // periods: how many nodes fall in each (a small bar chart), with ticks and labels
        let mut buckets: Vec<(f64, f64, usize)> = Vec::new();
        let mut b = bucket_start(v0, unit);
        while b < v1 && buckets.len() < 400 {
            let n = bucket_next(b, unit);
            buckets.push((b, n, 0));
            b = n;
        }
        if !buckets.is_empty() {
            let first = buckets[0].0;
            for &d in days.iter().filter(|d| d.is_finite() && **d >= first && **d < v1) {
                let k = buckets.partition_point(|x| x.1 <= d);
                if let Some(x) = buckets.get_mut(k) {
                    x.2 += 1;
                }
            }
        }
        let most = buckets.iter().map(|x| x.2).max().unwrap_or(1).max(1);
        let chart = Rect::from_min_max(Pos2::new(track.left(), track.top() + 14.0), track.max);
        let label_every = ((buckets.len() as f32 * 46.0 / track.width()).ceil() as usize).max(1);
        for (k, &(s, e, n)) in buckets.iter().enumerate() {
            let (x0, x1) = (to_x(s).max(track.left()), to_x(e).min(track.right()));
            if n > 0 && x1 > x0 {
                let h = chart.height() * (n as f32 / most as f32);
                let (p0, p1) = (bucket_start(cursor, period), bucket_next(bucket_start(cursor, period), period));
                let fill = if only && s >= p0 && s < p1 { accent.gamma_multiply(0.75) } else { accent.gamma_multiply(0.28) };
                painter.rect_filled(Rect::from_min_max(Pos2::new(x0 + 0.5, chart.bottom() - h), Pos2::new((x1 - 0.5).max(x0 + 1.0), chart.bottom())), 1.5, fill);
            }
            if to_x(s) >= track.left() {
                painter.line_segment([Pos2::new(to_x(s), track.top() + 10.0), Pos2::new(to_x(s), track.top() + 14.0)], Stroke::new(1.0, weak));
                let new_year = unit == Unit::Month && civil_from_days(s as i64).1 == 1;
                // a month just before a new year gives its place to the year's label
                let year_next = unit == Unit::Month && civil_from_days(e as i64).1 == 1 && label_every > 1;
                if (k % label_every == 0 && !year_next) || new_year {
                    let text = match unit {
                        Unit::Day => civil_from_days(s as i64).2.to_string(),
                        // January carries the year, so the months can be told apart
                        Unit::Month if civil_from_days(s as i64).1 == 1 => label(s, Unit::Year),
                        Unit::Month => label(s, Unit::Month).split(' ').next().unwrap_or("").to_string(),
                        Unit::Year => label(s, Unit::Year),
                    };
                    painter.text(Pos2::new(to_x(s) + 2.0, track.top() + 1.0), Align2::LEFT_TOP, text, FontId::proportional(10.0), weak);
                }
            }
        }
        // what lies after the line (when everything up to it is shown) is shaded
        let cx = to_x(cursor).clamp(track.left(), track.right());
        if !only && cx < track.right() {
            painter.rect_filled(Rect::from_min_max(Pos2::new(cx, track.top() + 12.0), track.max), 4.0, v.window_fill().gamma_multiply(0.55));
        }
        // with only one period kept: that period is marked as a band under the line
        if only {
            let p0 = bucket_start(cursor, period);
            let (x0, x1) = (to_x(p0).max(track.left()), to_x(bucket_next(p0, period)).min(track.right()));
            if x1 > x0 {
                painter.rect_filled(Rect::from_min_max(Pos2::new(x0, track.top() + 12.0), Pos2::new(x1.max(x0 + 2.0), track.bottom())), 3.0, accent.gamma_multiply(0.14));
            }
        }
        // the line, with a tag carrying the date
        painter.line_segment([Pos2::new(cx, track.top() + 8.0), Pos2::new(cx, track.bottom() + 2.0)], Stroke::new(2.0, accent));
        let tag_text = label(cursor, if unit == Unit::Year { Unit::Month } else { Unit::Day });
        let galley = painter.layout_no_wrap(tag_text, FontId::proportional(11.0), Color32::WHITE);
        let tw = galley.size().x + 12.0;
        let tag = Rect::from_min_size(Pos2::new((cx - tw * 0.5).clamp(track.left(), track.right() - tw), track.top() - 1.0), Vec2::new(tw, 15.0));
        painter.rect_filled(tag, 7.5, accent);
        painter.galley(Pos2::new(tag.left() + 6.0, tag.center().y - galley.size().y * 0.5), galley, Color32::WHITE);

        // ── input ──
        let mut new_cursor = None;
        let mut new_view = None;
        if close_resp.clicked() {
            self.s.time.show = false;
            self.time_playing = false;
            self.refilter();
            return Some(bar);
        }
        if play_resp.clicked() {
            self.time_playing = !self.time_playing;
            if self.time_playing && cursor >= v1 - span * 0.01 {
                new_cursor = Some(v0);
            }
        }
        if (resp.dragged() || resp.clicked()) && !play_resp.hovered() {
            if let Some(p) = resp.interact_pointer_pos() {
                if track.expand2(Vec2::new(4.0, 0.0)).contains(Pos2::new(p.x, track.center().y)) && p.y > track.top() - 4.0 {
                    new_cursor = Some(to_t(p.x.clamp(track.left(), track.right())));
                    self.time_playing = false;
                }
            }
        }
        if resp.double_clicked() {
            let (lo, hi) = days.iter().filter(|d| d.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &d| (a.min(d), b.max(d)));
            if lo.is_finite() {
                let pad = ((hi - lo) * 0.04).max(1.0);
                new_view = Some([lo - pad, hi + pad]);
            }
        }
        if resp.hovered() {
            let (scroll, shift) = ui.input(|i| (i.smooth_scroll_delta(), i.modifiers.shift));
            let pan = if shift { scroll.y } else { scroll.x };
            if pan.abs() > 0.0 {
                let d = -(pan / track.width()) as f64 * span;
                new_view = Some([v0 + d, v1 + d]);
            } else if scroll.y.abs() > 0.0 {
                let at = ui.input(|i| i.pointer.hover_pos()).map_or(cursor, |p| to_t(p.x));
                let f = (-(scroll.y as f64) * 0.003).exp();
                let ns = (span * f).clamp(3.0, 365.25 * 400.0);
                let k = (at - v0) / span;
                new_view = Some([at - k * ns, at - k * ns + ns]);
            }
        }
        if let Some(days_span) = zoom_to {
            let ns = if days_span.is_nan() {
                let (lo, hi) = days.iter().filter(|d| d.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &d| (a.min(d), b.max(d)));
                (hi - lo).max(365.25 * 7.0) * 1.08
            } else {
                days_span
            };
            new_view = Some([cursor - ns * 0.5, cursor + ns * 0.5]);
        }
        // playing: about twelve seconds across the bar
        if self.time_playing {
            let dt = ui.input(|i| i.stable_dt).min(0.1) as f64;
            let next = new_cursor.unwrap_or(cursor) + span / 12.0 * dt;
            if next >= v1 {
                self.time_playing = false;
                new_cursor = Some(v1);
            } else {
                new_cursor = Some(next);
            }
            ui.ctx().request_repaint();
        }
        if let Some(vw) = new_view {
            self.s.time.view = Some(vw);
        }
        if let Some(c) = new_cursor {
            self.s.time.cursor = Some(c);
            self.refilter();
        }
        Some(bar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_in_many_spellings() {
        let d = |s: &str| parse_date(s).map(|v| civil_from_days(v.floor() as i64));
        assert_eq!(d("2024-03-12"), Some((2024, 3, 12)));
        assert_eq!(d("2024/03/12"), Some((2024, 3, 12)));
        assert_eq!(d("12/03/2024"), Some((2024, 3, 12)), "day first");
        assert_eq!(d("12.03.2024"), Some((2024, 3, 12)));
        assert_eq!(d("2024-03"), Some((2024, 3, 1)));
        assert_eq!(d("2024"), Some((2024, 1, 1)));
        assert_eq!(d("12 Mar 2024"), Some((2024, 3, 12)));
        assert_eq!(d("March 12, 2024"), Some((2024, 3, 12)));
        assert_eq!(d("Mar 2024"), Some((2024, 3, 1)));
        assert_eq!(d("45363"), Some((2024, 3, 12)), "an Excel date number");
        let t = parse_date("2024-03-12T18:00").unwrap();
        assert!((t.fract() - 0.75).abs() < 1e-9, "six in the evening is three quarters of the day");
        assert_eq!(parse_date("hello"), None);
        assert_eq!(parse_date("2024-13-01"), None);
        assert_eq!(civil_from_days(days_from_civil(1999, 12, 31)), (1999, 12, 31));
    }

    #[test]
    fn periods_and_the_line() {
        let m = days_from_civil(2024, 3, 12) as f64;
        assert_eq!(bucket_start(m, Unit::Month), days_from_civil(2024, 3, 1) as f64);
        assert_eq!(bucket_next(days_from_civil(2024, 12, 1) as f64, Unit::Month), days_from_civil(2025, 1, 1) as f64);
        assert_eq!(label(m, Unit::Day), "12 Mar 2024");
        assert_eq!(Unit::for_span(30.0), Unit::Day);
        assert_eq!(Unit::for_span(700.0), Unit::Month);
        assert_eq!(Unit::for_span(5000.0), Unit::Year);
        let later = days_from_civil(2024, 3, 20) as f64;
        assert!(passes(m, later, false, Unit::Day) && !passes(later, m, false, Unit::Day));
        assert!(passes(later, m, true, Unit::Month), "same month");
        assert!(!passes(later, m, true, Unit::Day), "not the same day");
        assert!(passes(f64::NAN, m, true, Unit::Day), "undated nodes stay");
    }
}
