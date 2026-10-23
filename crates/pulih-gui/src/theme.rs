//! Look & feel: accent colours, spacing, and the procedurally drawn app icon.

use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, Theme, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(20, 160, 150);
pub const GOOD: Color32 = Color32::from_rgb(46, 170, 90);
pub const WARN: Color32 = Color32::from_rgb(220, 160, 30);
pub const BAD: Color32 = Color32::from_rgb(215, 75, 65);
pub const MUTED: Color32 = Color32::from_rgb(130, 135, 145);

fn tune(v: &mut Visuals, dark: bool) {
    let r = CornerRadius::same(6);
    v.selection.bg_fill = if dark {
        Color32::from_rgb(18, 110, 104)
    } else {
        Color32::from_rgb(170, 225, 220)
    };
    v.selection.stroke = Stroke::new(
        1.0,
        if dark {
            Color32::WHITE
        } else {
            Color32::from_rgb(10, 70, 66)
        },
    );
    v.hyperlink_color = ACCENT;
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    if dark {
        v.panel_fill = Color32::from_rgb(24, 26, 30);
        v.window_fill = Color32::from_rgb(30, 33, 38);
        v.extreme_bg_color = Color32::from_rgb(17, 18, 21);
        v.faint_bg_color = Color32::from_rgb(32, 35, 41);
    } else {
        v.panel_fill = Color32::from_rgb(246, 247, 249);
        v.faint_bg_color = Color32::from_rgb(236, 239, 243);
    }
}

pub fn apply(ctx: &egui::Context) {
    let mut dark = Visuals::dark();
    tune(&mut dark, true);
    let mut light = Visuals::light();
    tune(&mut light, false);
    ctx.set_visuals_of(Theme::Dark, dark);
    ctx.set_visuals_of(Theme::Light, light);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 24.0;
        s.text_styles
            .insert(TextStyle::Heading, FontId::proportional(20.0));
        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        s.text_styles
            .insert(TextStyle::Button, FontId::proportional(14.0));
        s.text_styles
            .insert(TextStyle::Small, FontId::proportional(11.5));
        s.text_styles
            .insert(TextStyle::Monospace, FontId::monospace(12.5));
    });
}

/// 64×64 icon: teal rounded tile with a magnifier over a file.
pub fn icon() -> egui::IconData {
    const N: usize = 64;
    let mut rgba = vec![0u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // rounded square mask
            let r = 14.0;
            let (cx, cy) = (fx.clamp(r, N as f32 - r), fy.clamp(r, N as f32 - r));
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let a = (r + 0.5 - d).clamp(0.0, 1.0);
            if a == 0.0 {
                continue;
            }
            let t = (fx + fy) / (2.0 * N as f32);
            let mut c = [20.0 + 30.0 * t, 175.0 - 60.0 * t, 160.0 + 40.0 * t];
            // document sheet
            let in_doc = (17.0..41.0).contains(&fx) && (12.0..48.0).contains(&fy);
            if in_doc {
                c = [235.0, 248.0, 246.0];
                if (21.0..37.0).contains(&fx)
                    && [20.0, 26.0, 32.0].iter().any(|l| (fy - l).abs() < 1.2)
                {
                    c = [120.0, 170.0, 165.0];
                }
            }
            // magnifier ring + handle
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
                c = [255.0, 200.0, 60.0];
            } else if md < 7.0 && in_doc {
                c = [205.0, 240.0, 236.0];
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
