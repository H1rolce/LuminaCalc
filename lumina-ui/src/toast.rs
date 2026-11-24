use crate::style::Palette;
use egui::{Color32, Context, CornerRadius, FontId, Id, LayerId, Order, Pos2, Rect, Stroke, Vec2};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
}

#[derive(Debug, Clone)]
struct ToastItem {
    text: String,
    kind: ToastKind,
    born: Instant,
    duration: f32,
}

#[derive(Debug, Default)]
pub struct ToastManager {
    active: Option<ToastItem>,
}

impl ToastManager {
    pub fn show(&mut self, text: impl Into<String>, kind: ToastKind) {
        self.active = Some(ToastItem {
            text: text.into(),
            kind,
            born: Instant::now(),
            duration: 1.9,
        });
    }

    pub fn success(&mut self, text: impl Into<String>) {
        self.show(text, ToastKind::Success);
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.show(text, ToastKind::Info);
    }

    pub fn paint(&mut self, ctx: &Context, palette: &Palette) {
        let Some(item) = &self.active else {
            return;
        };

        let elapsed = item.born.elapsed().as_secs_f32();
        let fade_in = 0.16;
        let fade_out = 0.28;
        let total = item.duration;

        if elapsed >= total {
            self.active = None;
            return;
        }

        if elapsed < fade_in || elapsed > total - fade_out {
            ctx.request_repaint();
        } else {
            let until_fade_out = (total - fade_out - elapsed).max(0.01);
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(until_fade_out));
        }

        let alpha = if elapsed < fade_in {
            let t = (elapsed / fade_in).clamp(0.0, 1.0);
            1.0 - (1.0 - t).powi(3)
        } else if elapsed > total - fade_out {
            ((total - elapsed) / fade_out).clamp(0.0, 1.0).powi(2)
        } else {
            1.0
        };

        let slide = if elapsed < fade_in {
            let t = 1.0 - (elapsed / fade_in).clamp(0.0, 1.0);
            t * t * 14.0
        } else {
            0.0
        };

        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("lumina_toast")));
        let viewport = ctx.content_rect();

        let accent = match item.kind {
            ToastKind::Success => palette.success,
            ToastKind::Info => palette.accent,
        };

        let text_color = Palette::alpha(palette.text_primary, (255.0 * alpha) as u8);
        let galley = painter.layout_no_wrap(
            item.text.clone(),
            FontId::proportional(13.0),
            text_color,
        );

        let dot_w = 14.0;
        let pad = Vec2::new(14.0, 9.0);
        let size = Vec2::new(galley.size().x + pad.x * 2.0 + dot_w, galley.size().y + pad.y * 2.0);
        let center = Pos2::new(viewport.center().x, viewport.bottom() - 44.0 + slide);
        let rect = Rect::from_center_size(center, size);

        let shadow_rect = rect.translate(Vec2::new(0.0, 4.0)).expand(2.0);
        painter.rect_filled(
            shadow_rect,
            CornerRadius::same(12),
            Color32::from_black_alpha((70.0 * alpha) as u8),
        );

        let bg = Palette::alpha(palette.bg_elevated, (242.0 * alpha) as u8);
        let stroke = Stroke::new(1.0, Palette::alpha(accent, (165.0 * alpha) as u8));
        painter.rect(rect, CornerRadius::same(10), bg, stroke, egui::StrokeKind::Inside);

        let dot_pos = Pos2::new(rect.left() + pad.x + 3.0, rect.center().y);
        painter.circle_filled(dot_pos, 3.8, Palette::alpha(accent, (255.0 * alpha) as u8));

        let text_pos = Pos2::new(
            rect.left() + pad.x + dot_w,
            rect.center().y - galley.size().y * 0.5,
        );
        painter.galley(text_pos, galley, text_color);
    }
}
