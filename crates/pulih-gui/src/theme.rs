//! Design system: palettes, fonts, and the hand-drawn widgets and icons the
//! screens are built from. Clean light theme by default (white cards on a
//! soft grey canvas, one confident blue), with a matching dark theme.

use std::sync::Arc;

use eframe::egui::{
    self, epaint, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Pos2, Rect, Response, RichText, Sense, Shadow, Stroke, StrokeKind, TextStyle, Theme,
    ThemePreference, Ui, Vec2, Visuals,
};
use pulih_core::{Category, Condition};

use crate::i18n::T;

pub struct Pal {
    pub bg: Color32,
    pub card: Color32,
    pub sidebar: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub primary: Color32,
    pub primary_soft: Color32,
    pub success: Color32,
    pub success_soft: Color32,
    pub warning: Color32,
    pub warning_soft: Color32,
    pub danger: Color32,
    pub danger_soft: Color32,
    pub track: Color32,
    pub icon_bg: Color32,
    pub page: Color32,
    pub shadow: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub static LIGHT: Pal = Pal {
    bg: rgb(244, 246, 251),
    card: rgb(255, 255, 255),
    sidebar: rgb(255, 255, 255),
    border: rgb(226, 232, 240),
    text: rgb(15, 23, 42),
    muted: rgb(100, 116, 139),
    faint: rgb(248, 250, 252),
    primary: rgb(29, 106, 229),
    primary_soft: rgb(232, 240, 254),
    success: rgb(22, 163, 74),
    success_soft: rgb(220, 252, 231),
    warning: rgb(217, 119, 6),
    warning_soft: rgb(254, 243, 199),
    danger: rgb(220, 38, 38),
    danger_soft: rgb(254, 226, 226),
    track: rgb(226, 232, 240),
    icon_bg: rgb(232, 238, 248),
    page: rgb(255, 255, 255),
    shadow: Color32::from_rgba_premultiplied(15, 23, 42, 14),
};

pub static DARK: Pal = Pal {
    bg: rgb(11, 18, 32),
    card: rgb(17, 26, 46),
    sidebar: rgb(14, 22, 40),
    border: rgb(32, 44, 68),
    text: rgb(226, 232, 240),
    muted: rgb(148, 163, 184),
    faint: rgb(21, 31, 54),
    primary: rgb(59, 130, 246),
    primary_soft: rgb(23, 42, 82),
    success: rgb(34, 197, 94),
    success_soft: rgb(18, 52, 36),
    warning: rgb(245, 158, 11),
    warning_soft: rgb(60, 44, 14),
    danger: rgb(248, 113, 113),
    danger_soft: rgb(68, 24, 28),
    track: rgb(36, 50, 76),
    icon_bg: rgb(28, 42, 70),
    page: rgb(40, 52, 78),
    shadow: Color32::from_rgba_premultiplied(0, 0, 0, 60),
};

pub fn pal(ui: &Ui) -> &'static Pal {
    if ui.visuals().dark_mode {
        &DARK
    } else {
        &LIGHT
    }
}

pub fn bold(s: impl Into<String>) -> RichText {
    RichText::new(s).family(FontFamily::Name("bold".into()))
}

pub fn semi(s: impl Into<String>) -> RichText {
    RichText::new(s).family(FontFamily::Name("semibold".into()))
}

fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::default();
    let add = |f: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        f.font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        &mut f,
        "jakarta",
        include_bytes!("../assets/PlusJakartaSans-Regular.ttf"),
    );
    add(
        &mut f,
        "jakarta-semibold",
        include_bytes!("../assets/PlusJakartaSans-SemiBold.ttf"),
    );
    add(
        &mut f,
        "jakarta-bold",
        include_bytes!("../assets/PlusJakartaSans-Bold.ttf"),
    );
    // Keep egui's fonts behind ours for emoji / symbols.
    let fallback = f
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let family = |first: &str| {
        std::iter::once(first.to_string())
            .chain(fallback.iter().cloned())
            .collect::<Vec<_>>()
    };
    f.families
        .insert(FontFamily::Proportional, family("jakarta"));
    f.families.insert(
        FontFamily::Name("semibold".into()),
        family("jakarta-semibold"),
    );
    f.families
        .insert(FontFamily::Name("bold".into()), family("jakarta-bold"));
    f
}

fn visuals(p: &Pal, dark: bool) -> Visuals {
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let r = CornerRadius::same(8);
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.extreme_bg_color = p.card;
    v.faint_bg_color = p.faint;
    v.code_bg_color = p.faint;
    v.hyperlink_color = p.primary;
    v.selection.bg_fill = p.primary_soft;
    v.selection.stroke = Stroke::new(1.0, p.primary);
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: p.shadow,
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: p.shadow,
    };
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.noninteractive.bg_fill = p.card;
    let btn = if dark {
        rgb(30, 42, 66)
    } else {
        rgb(238, 242, 248)
    };
    let btn_hover = if dark {
        rgb(40, 55, 84)
    } else {
        rgb(226, 232, 242)
    };
    v.widgets.inactive.bg_fill = btn;
    v.widgets.inactive.weak_bg_fill = btn;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.hovered.bg_fill = btn_hover;
    v.widgets.hovered.weak_bg_fill = btn_hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.primary);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, p.text);
    v.widgets.active.bg_fill = p.primary_soft;
    v.widgets.active.weak_bg_fill = p.primary_soft;
    v.widgets.active.bg_stroke = Stroke::new(1.0, p.primary);
    v.widgets.active.fg_stroke = Stroke::new(1.5, p.primary);
    v.widgets.open = v.widgets.active;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
        w.expansion = 0.0;
    }
    v.striped = true;
    v
}

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_visuals_of(Theme::Light, visuals(&LIGHT, false));
    ctx.set_visuals_of(Theme::Dark, visuals(&DARK, true));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.interact_size.y = 28.0;
        s.spacing.menu_margin = egui::Margin::same(8);
        s.spacing.scroll.bar_width = 8.0;
        s.text_styles.insert(
            TextStyle::Heading,
            FontId::new(22.0, FontFamily::Name("bold".into())),
        );
        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        s.text_styles.insert(
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Name("semibold".into())),
        );
        s.text_styles
            .insert(TextStyle::Small, FontId::proportional(12.0));
        s.text_styles
            .insert(TextStyle::Monospace, FontId::monospace(12.5));
    });
    // A clean light look by default; the sidebar toggles it.
    ctx.set_theme(ThemePreference::Light);
}

// ------------------------------------------------------------------ widgets

pub fn card(ui: &Ui) -> egui::Frame {
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(12)
        .inner_margin(16)
        .shadow(Shadow {
            offset: [0, 1],
            blur: 6,
            spread: 0,
            color: p.shadow,
        })
}

pub fn primary_button(ui: &mut Ui, text: impl Into<String>, enabled: bool, big: bool) -> Response {
    let p = pal(ui);
    let size = if big { 16.0 } else { 14.0 };
    let fill = if enabled { p.primary } else { p.track };
    let fg = if enabled { Color32::WHITE } else { p.muted };
    let b = egui::Button::new(semi(text).size(size).color(fg))
        .fill(fill)
        .stroke(Stroke::NONE)
        .corner_radius(if big { 10 } else { 8 })
        .min_size(if big {
            Vec2::new(180.0, 44.0)
        } else {
            Vec2::new(0.0, 32.0)
        });
    ui.add_enabled(enabled, b)
}

pub fn soft_button(ui: &mut Ui, text: impl Into<String>) -> Response {
    let p = pal(ui);
    ui.add(
        egui::Button::new(semi(text).color(p.primary))
            .fill(p.primary_soft)
            .stroke(Stroke::NONE)
            .min_size(Vec2::new(0.0, 30.0)),
    )
}

/// Small rounded label.
pub fn pill(ui: &mut Ui, text: impl Into<String>, fg: Color32, bg: Color32) -> Response {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(20)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| ui.label(semi(text).size(11.5).color(fg)))
        .response
}

pub fn cond_colors(p: &Pal, c: Condition) -> (Color32, Color32) {
    match c {
        Condition::Good => (p.success, p.success_soft),
        Condition::Unknown => (p.muted, p.track),
        Condition::Partial => (p.warning, p.warning_soft),
        Condition::Overwritten => (p.danger, p.danger_soft),
    }
}

pub fn cond_pill(ui: &mut Ui, c: Condition, t: &T) -> Response {
    let p = pal(ui);
    let (fg, bg) = cond_colors(p, c);
    pill(ui, t.cond[c as usize], fg, bg).on_hover_text(t.cond_hint[c as usize])
}

/// Rounded progress bar with a soft sheen while `busy`.
pub fn progress(ui: &mut Ui, frac: f32, height: f32, busy: bool) -> Response {
    let p = pal(ui);
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    let r = (height / 2.0) as u8;
    painter.rect_filled(rect, r, p.track);
    let w = (rect.width() * frac.clamp(0.0, 1.0)).max(if frac > 0.0 { height } else { 0.0 });
    let fill = Rect::from_min_size(rect.min, Vec2::new(w, height));
    painter.rect_filled(fill, r, p.primary);
    if busy && w > height {
        let t = ui.input(|i| i.time) as f32;
        let x = fill.min.x + (t * 0.6).fract() * (fill.width() + 80.0) - 40.0;
        let sheen = Rect::from_center_size(Pos2::new(x, fill.center().y), Vec2::new(60.0, height));
        let sheen = sheen.intersect(fill);
        if sheen.width() > 0.0 {
            painter.rect_filled(sheen, r, Color32::from_white_alpha(46));
        }
        ui.ctx().request_repaint();
    }
    resp
}

/// Segmented control; returns the newly selected index.
pub fn segmented(ui: &mut Ui, items: &[&str], selected: usize) -> Option<usize> {
    let p = pal(ui);
    let mut out = None;
    egui::Frame::new()
        .fill(p.track)
        .corner_radius(10)
        .inner_margin(3)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, it) in items.iter().enumerate() {
                    let on = i == selected;
                    let txt = semi(*it).color(if on { p.text } else { p.muted });
                    let b = egui::Button::new(txt)
                        .fill(if on { p.card } else { Color32::TRANSPARENT })
                        .stroke(Stroke::NONE)
                        .corner_radius(8)
                        .min_size(Vec2::new(0.0, 28.0));
                    if ui.add(b).clicked() && !on {
                        out = Some(i);
                    }
                }
            });
        });
    out
}

/// Segmented control stretched over the available width, equal segments.
pub fn segmented_fill(ui: &mut Ui, items: &[&str], selected: usize) -> Option<usize> {
    let p = pal(ui);
    let (rect, base) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::hover());
    ui.painter().rect_filled(rect, 9, p.track);
    let inner = rect.shrink(3.0);
    let w = inner.width() / items.len().max(1) as f32;
    let mut out = None;
    for (i, it) in items.iter().enumerate() {
        let r = Rect::from_min_size(
            Pos2::new(inner.left() + w * i as f32, inner.top()),
            Vec2::new(w, inner.height()),
        );
        let resp = ui.interact(r, base.id.with(i), Sense::click());
        let on = i == selected;
        if on {
            ui.painter()
                .rect_filled(r.translate(Vec2::new(0.0, 1.0)), 7, p.shadow);
            ui.painter().rect_filled(r, 7, p.card);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 7, p.card.gamma_multiply(0.5));
        }
        let font = FontId::new(
            12.5,
            FontFamily::Name(if on { "bold" } else { "semibold" }.into()),
        );
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            *it,
            font,
            if on { p.text } else { p.muted },
        );
        if resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
            && !on
        {
            out = Some(i);
        }
    }
    out
}

/// Outlined dropdown button (blue when `active`); the popup stays open
/// while clicking inside it.
pub fn dropdown<R>(
    ui: &mut Ui,
    text: &str,
    active: bool,
    content: impl FnOnce(&mut Ui) -> R,
) -> Response {
    let p = pal(ui);
    let (fg, bg, st) = if active {
        (p.primary, p.primary_soft, Stroke::new(1.0, p.primary))
    } else {
        (p.text, p.card, Stroke::new(1.0, p.border))
    };
    let b = egui::Button::new(semi(format!("{text}     ")).size(13.0).color(fg))
        .fill(bg)
        .stroke(st)
        .corner_radius(8)
        .min_size(Vec2::new(0.0, 32.0));
    let cfg = egui::containers::menu::MenuConfig::new()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let (resp, inner) = egui::containers::menu::MenuButton::from_button(b)
        .config(cfg)
        .ui(ui, content);
    paint_chevron(
        ui.painter(),
        resp.rect.right_center() - Vec2::new(15.0, 0.0),
        4.0,
        fg,
        inner.is_some(),
    );
    resp
}

/// Small "×" close mark.
pub fn paint_x(painter: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let s = Stroke::new(1.6, color);
    painter.line_segment([c + Vec2::new(-r, -r), c + Vec2::new(r, r)], s);
    painter.line_segment([c + Vec2::new(-r, r), c + Vec2::new(r, -r)], s);
}

/// Small "v" (or "^" when `up`) chevron.
pub fn paint_chevron(painter: &egui::Painter, c: Pos2, r: f32, color: Color32, up: bool) {
    let dy = if up { -r * 0.5 } else { r * 0.5 };
    let s = Stroke::new(1.6, color);
    let tip = c + Vec2::new(0.0, dy);
    painter.line_segment([c + Vec2::new(-r, -dy), tip], s);
    painter.line_segment([c + Vec2::new(r, -dy), tip], s);
}

#[derive(Clone, Copy)]
pub enum NavIcon {
    Home,
    Results,
    Tips,
}

/// Line icons for the sidebar.
pub fn paint_nav_icon(painter: &egui::Painter, c: Pos2, kind: NavIcon, color: Color32) {
    let s = Stroke::new(1.7, color);
    match kind {
        NavIcon::Home => {
            let roof = [
                c + Vec2::new(-7.5, -0.5),
                c + Vec2::new(0.0, -7.5),
                c + Vec2::new(7.5, -0.5),
            ];
            painter.line_segment([roof[0], roof[1]], s);
            painter.line_segment([roof[1], roof[2]], s);
            let body = Rect::from_min_max(c + Vec2::new(-5.5, -2.0), c + Vec2::new(5.5, 7.0));
            painter.rect_stroke(body, 1, s, StrokeKind::Middle);
            painter.line_segment([c + Vec2::new(0.0, 7.0), c + Vec2::new(0.0, 3.0)], s);
        }
        NavIcon::Results => {
            let r = Rect::from_center_size(c, Vec2::new(14.0, 16.0));
            painter.rect_stroke(r, 2, s, StrokeKind::Middle);
            for (i, w) in [8.0, 6.0, 8.0].iter().enumerate() {
                let y = r.top() + 4.5 + i as f32 * 3.5;
                painter.line_segment(
                    [
                        Pos2::new(r.left() + 3.0, y),
                        Pos2::new(r.left() + 3.0 + w, y),
                    ],
                    s,
                );
            }
        }
        NavIcon::Tips => {
            painter.circle_stroke(c + Vec2::new(0.0, -2.5), 5.5, s);
            painter.line_segment([c + Vec2::new(-2.5, 5.0), c + Vec2::new(2.5, 5.0)], s);
            painter.line_segment([c + Vec2::new(-1.8, 7.5), c + Vec2::new(1.8, 7.5)], s);
            painter.line_segment([c + Vec2::new(-2.0, 2.5), c + Vec2::new(-2.0, 5.0)], s);
            painter.line_segment([c + Vec2::new(2.0, 2.5), c + Vec2::new(2.0, 5.0)], s);
        }
    }
}

/// One line of text, cut with "…" at `max_w`.
pub fn elided(
    painter: &egui::Painter,
    text: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
) -> Arc<epaint::Galley> {
    let mut job = epaint::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = epaint::text::TextWrapping::truncate_at_width(max_w.max(1.0));
    painter.layout_job(job)
}

pub fn font_bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn font_semi(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

/// Soft coloured notice box (warnings, errors).
pub fn callout(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text).size(13.0).color(fg)).wrap());
        });
}

/// A filled circle with a white check mark.
pub fn paint_check(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    painter.circle_filled(center, r, color);
    let s = Stroke::new((r / 5.0).max(1.5), Color32::WHITE);
    let a = center + Vec2::new(-r * 0.45, 0.0);
    let b = center + Vec2::new(-r * 0.1, r * 0.35);
    let c = center + Vec2::new(r * 0.5, -r * 0.35);
    painter.line_segment([a, b], s);
    painter.line_segment([b, c], s);
}

/// Heading + optional subtitle block.
pub fn title(ui: &mut Ui, text: &str, sub: Option<&str>) {
    let p = pal(ui);
    ui.label(bold(text).size(24.0).color(p.text));
    if let Some(s) = sub {
        ui.label(RichText::new(s).size(14.0).color(p.muted));
    }
}

// ------------------------------------------------------------------ icons

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DriveIcon {
    Disk,
    Partition,
    Removable,
    Image,
    Folder,
}

/// Vector drive/location icons (no image assets needed).
pub fn paint_drive(painter: &egui::Painter, rect: Rect, kind: DriveIcon, p: &Pal) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let stroke = Stroke::new((s / 28.0).max(1.2), p.muted);
    match kind {
        DriveIcon::Disk | DriveIcon::Partition => {
            let body = Rect::from_center_size(c, Vec2::new(s * 0.86, s * 0.56));
            painter.rect_filled(body, (s * 0.1) as u8, p.icon_bg);
            painter.rect_stroke(body, (s * 0.1) as u8, stroke, StrokeKind::Inside);
            let y = body.bottom() - body.height() * 0.32;
            painter.line_segment(
                [
                    Pos2::new(body.left() + s * 0.06, y),
                    Pos2::new(body.right() - s * 0.06, y),
                ],
                stroke,
            );
            let led = Pos2::new(
                body.right() - s * 0.14,
                body.bottom() - body.height() * 0.16,
            );
            let led_c = if kind == DriveIcon::Disk {
                p.primary
            } else {
                p.success
            };
            painter.circle_filled(led, s * 0.04, led_c);
            for i in 0..3 {
                let x = body.left() + s * (0.12 + i as f32 * 0.07);
                painter.line_segment(
                    [
                        Pos2::new(x, led.y - s * 0.03),
                        Pos2::new(x, led.y + s * 0.03),
                    ],
                    stroke,
                );
            }
        }
        DriveIcon::Removable => {
            let body =
                Rect::from_center_size(c + Vec2::new(0.0, s * 0.08), Vec2::new(s * 0.44, s * 0.62));
            let plug = Rect::from_center_size(
                Pos2::new(c.x, body.top() - s * 0.08),
                Vec2::new(s * 0.28, s * 0.18),
            );
            painter.rect_filled(plug, 2, p.track);
            painter.rect_stroke(plug, 2, stroke, StrokeKind::Inside);
            painter.rect_filled(body, (s * 0.08) as u8, p.icon_bg);
            painter.rect_stroke(body, (s * 0.08) as u8, stroke, StrokeKind::Inside);
            painter.circle_filled(
                Pos2::new(c.x, body.bottom() - s * 0.12),
                s * 0.04,
                p.success,
            );
        }
        DriveIcon::Image => {
            painter.circle_filled(c, s * 0.38, p.icon_bg);
            painter.circle_stroke(c, s * 0.38, stroke);
            painter.circle_stroke(c, s * 0.2, Stroke::new(stroke.width, p.primary));
            painter.circle_filled(c, s * 0.07, p.muted);
        }
        DriveIcon::Folder => {
            let body =
                Rect::from_center_size(c + Vec2::new(0.0, s * 0.05), Vec2::new(s * 0.84, s * 0.56));
            let tab = Rect::from_min_size(
                body.min - Vec2::new(0.0, s * 0.1),
                Vec2::new(s * 0.34, s * 0.16),
            );
            let col = rgb(245, 179, 52);
            painter.rect_filled(tab, 3, col.gamma_multiply(0.85));
            painter.rect_filled(body, (s * 0.07) as u8, col);
        }
    }
}

/// Category colour, refined per extension for the familiar office types.
pub fn type_color(ext: &str, cat: Category) -> Color32 {
    match ext {
        "pdf" => rgb(220, 38, 38),
        "doc" | "docx" | "odt" | "rtf" => rgb(37, 99, 235),
        "xls" | "xlsx" | "ods" | "csv" => rgb(22, 128, 61),
        "ppt" | "pptx" | "odp" => rgb(234, 88, 12),
        _ => match cat {
            Category::Image => rgb(13, 148, 136),
            Category::Video => rgb(124, 58, 237),
            Category::Audio => rgb(219, 39, 119),
            Category::Document => rgb(37, 99, 235),
            Category::Archive => rgb(180, 120, 20),
            Category::Other => rgb(71, 85, 105),
        },
    }
}

/// A file "page" with a folded corner and a coloured type label.
pub fn paint_file(painter: &egui::Painter, rect: Rect, ext: &str, cat: Category, p: &Pal) {
    let s = rect.height().min(rect.width() * 1.3);
    let w = s * 0.76;
    let page = Rect::from_center_size(rect.center(), Vec2::new(w, s));
    let fold = s * 0.24;
    let pts = vec![
        page.left_top(),
        Pos2::new(page.right() - fold, page.top()),
        Pos2::new(page.right(), page.top() + fold),
        page.right_bottom(),
        page.left_bottom(),
    ];
    painter.add(epaint::PathShape::convex_polygon(
        pts,
        p.page,
        Stroke::new(1.2, p.border.gamma_multiply(1.6)),
    ));
    painter.add(epaint::PathShape::convex_polygon(
        vec![
            Pos2::new(page.right() - fold, page.top()),
            Pos2::new(page.right() - fold, page.top() + fold),
            Pos2::new(page.right(), page.top() + fold),
        ],
        p.track,
        Stroke::NONE,
    ));
    let col = type_color(ext, cat);
    let band = Rect::from_min_max(
        Pos2::new(page.left() - w * 0.12, page.top() + s * 0.52),
        Pos2::new(page.right() - w * 0.1, page.top() + s * 0.8),
    );
    painter.rect_filled(band, (s * 0.05) as u8, col);
    let label = if ext.is_empty() {
        "?".to_string()
    } else {
        ext.to_uppercase()
    };
    let size = (band.height() * 0.62).min(band.width() / (label.len().max(3) as f32 * 0.62));
    painter.text(
        band.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::new(size, FontFamily::Name("bold".into())),
        Color32::WHITE,
    );
}

/// Window icon (64×64, drawn once at start-up): a blue tile with a
/// magnifier over a document.
pub fn icon() -> egui::IconData {
    const N: usize = 64;
    let mut rgba = vec![0u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let r = 14.0;
            let (cx, cy) = (fx.clamp(r, N as f32 - r), fy.clamp(r, N as f32 - r));
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let a = (r + 0.5 - d).clamp(0.0, 1.0);
            if a == 0.0 {
                continue;
            }
            let t = (fx + fy) / (2.0 * N as f32);
            let mut c = [30.0 + 20.0 * t, 120.0 - 40.0 * t, 235.0 - 20.0 * t];
            let in_doc = (17.0..41.0).contains(&fx) && (12.0..48.0).contains(&fy);
            if in_doc {
                c = [245.0, 248.0, 255.0];
                if (21.0..37.0).contains(&fx)
                    && [20.0, 26.0, 32.0].iter().any(|l| (fy - l).abs() < 1.2)
                {
                    c = [150.0, 175.0, 220.0];
                }
            }
            let (mx, my) = (38.0, 37.0);
            let md = ((fx - mx).powi(2) + (fy - my).powi(2)).sqrt();
            let ring = (md - 9.5).abs() < 2.6;
            let handle = {
                let (hx, hy) = (fx - mx, fy - my);
                let along = (hx + hy) / std::f32::consts::SQRT_2;
                let across = (hx - hy).abs() / std::f32::consts::SQRT_2;
                (11.0..21.0).contains(&along) && across < 3.0
            };
            if ring || handle {
                c = [255.0, 196.0, 61.0];
            } else if md < 7.0 && in_doc {
                c = [219, 234, 254].map(|v| v as f32);
            }
            let i = (y * N + x) * 4;
            rgba[i] = c[0] as u8;
            rgba[i + 1] = c[1] as u8;
            rgba[i + 2] = c[2] as u8;
            rgba[i + 3] = (a * 255.0) as u8;
        }
    }
    egui::IconData {
        rgba,
        width: N as u32,
        height: N as u32,
    }
}
