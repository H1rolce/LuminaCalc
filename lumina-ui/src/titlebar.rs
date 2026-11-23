//! Custom frameless window titlebar, segmented animated tab bar,
//! and 8-direction border resize handling for `with_decorations(false)`.

use crate::i18n::{Lang, tr};
use crate::state::ActiveTab;
use crate::style::{Palette, ThemeId};
use egui::{
    Color32, Context, CornerRadius, CursorIcon, FontId, Id, Pos2, Rect, ResizeDirection, Sense,
    Stroke, Ui, Vec2, ViewportCommand,
};

pub struct TitlebarAction {
    pub theme_cycled: bool,
    pub lang_toggled: bool,
    pub pin_toggled: bool,
    pub tab_changed: Option<ActiveTab>,
}

/// Handles 8-direction window resizing around the edges of a frameless window.
pub fn handle_frameless_resize(ctx: &Context, palette: &Palette) {
    let is_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let rect = ctx.content_rect();

    // Subtle 1px outer border when windowed so dark themes stand out cleanly.
    if !is_maximized {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            Id::unique("lumina_window_frame"),
        ));
        painter.rect_stroke(
            rect.shrink(0.5),
            CornerRadius::same(10),
            Stroke::new(1.0, palette.border),
            egui::StrokeKind::Inside,
        );
    }

    if is_maximized {
        return;
    }

    let edge = 6.0f32;
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };

    let left = pos.x <= rect.left() + edge;
    let right = pos.x >= rect.right() - edge;
    let top = pos.y <= rect.top() + edge;
    let bottom = pos.y >= rect.bottom() - edge;

    let dir = match (left, right, top, bottom) {
        (true, _, true, _) => Some((ResizeDirection::NorthWest, CursorIcon::ResizeNwSe)),
        (_, true, true, _) => Some((ResizeDirection::NorthEast, CursorIcon::ResizeNeSw)),
        (true, _, _, true) => Some((ResizeDirection::SouthWest, CursorIcon::ResizeNeSw)),
        (_, true, _, true) => Some((ResizeDirection::SouthEast, CursorIcon::ResizeNwSe)),
        (true, _, _, _) => Some((ResizeDirection::West, CursorIcon::ResizeHorizontal)),
        (_, true, _, _) => Some((ResizeDirection::East, CursorIcon::ResizeHorizontal)),
        (_, _, true, _) => Some((ResizeDirection::North, CursorIcon::ResizeVertical)),
        (_, _, _, true) => Some((ResizeDirection::South, CursorIcon::ResizeVertical)),
        _ => None,
    };

    if let Some((resize_dir, icon)) = dir {
        ctx.set_cursor_icon(icon);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(resize_dir));
        }
    }
}

/// Renders the custom titlebar inside a top panel.
pub fn show_titlebar(
    ui: &mut Ui,
    tab: ActiveTab,
    theme: ThemeId,
    lang: Lang,
    always_on_top: bool,
    palette: &Palette,
) -> TitlebarAction {
    let s = tr(lang);
    let mut action = TitlebarAction {
        theme_cycled: false,
        lang_toggled: false,
        pin_toggled: false,
        tab_changed: None,
    };

    let bar_h = 44.0;
    let (bar_rect, bar_resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), bar_h),
        Sense::click_and_drag(),
    );

    let is_maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));

    // Background + bottom hairline
    ui.painter().rect_filled(bar_rect, CornerRadius::ZERO, palette.bg_mantle);
    ui.painter().line_segment(
        [
            Pos2::new(bar_rect.left(), bar_rect.bottom()),
            Pos2::new(bar_rect.right(), bar_rect.bottom()),
        ],
        Stroke::new(1.0, palette.border_subtle),
    );

    // ── Left: vector Lumina spark logo + app title (compact icon-only on narrow windows) ──
    let show_brand_text = bar_rect.width() >= 780.0;
    let brand_w = if show_brand_text { 118.0 } else { 30.0 };
    let brand_rect = Rect::from_min_size(
        Pos2::new(bar_rect.left() + 10.0, bar_rect.top() + 6.0),
        Vec2::new(brand_w, bar_h - 12.0),
    );
    ui.interact(brand_rect, Id::unique("tb_brand"), Sense::hover())
        .on_hover_text(s.hotkey_hint);
    let logo_center = Pos2::new(bar_rect.left() + 24.0, bar_rect.center().y);
    paint_lumina_logo(ui.painter(), logo_center, 8.5, palette);
    if show_brand_text {
        ui.painter().text(
            Pos2::new(bar_rect.left() + 40.0, bar_rect.center().y),
            egui::Align2::LEFT_CENTER,
            "LuminaCalc",
            FontId::proportional(14.0),
            palette.text_primary,
        );
    }
    let left_limit = brand_rect.right() + 6.0;

    // ── Right: Theme, Language, Pin, Window Controls ──
    let mut right_x = bar_rect.right() - 8.0;
    let btn_y = bar_rect.center().y;

    // Close button
    let close_rect = Rect::from_center_size(Pos2::new(right_x - 16.0, btn_y), Vec2::new(30.0, 26.0));
    right_x -= 34.0;
    if paint_win_button(ui, close_rect, Id::unique("win_close"), palette, true, |p, c, col| {
        let d = 4.5;
        p.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], Stroke::new(1.5, col));
        p.line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], Stroke::new(1.5, col));
    }) {
        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
    }

    // Maximize / Restore button
    let max_rect = Rect::from_center_size(Pos2::new(right_x - 16.0, btn_y), Vec2::new(30.0, 26.0));
    right_x -= 34.0;
    if paint_win_button(ui, max_rect, Id::unique("win_max"), palette, false, |p, c, col| {
        let sq = Rect::from_center_size(c, Vec2::splat(9.0));
        p.rect_stroke(sq, CornerRadius::same(1), Stroke::new(1.3, col), egui::StrokeKind::Inside);
    }) {
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!is_maximized));
    }

    // Minimize button
    let min_rect = Rect::from_center_size(Pos2::new(right_x - 16.0, btn_y), Vec2::new(30.0, 26.0));
    right_x -= 40.0;
    if paint_win_button(ui, min_rect, Id::unique("win_min"), palette, false, |p, c, col| {
        p.line_segment(
            [Pos2::new(c.x - 5.0, c.y + 1.0), Pos2::new(c.x + 5.0, c.y + 1.0)],
            Stroke::new(1.4, col),
        );
    }) {
        ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
    }

    // Subtle divider between utility buttons and window controls
    ui.painter().line_segment(
        [Pos2::new(right_x, bar_rect.top() + 12.0), Pos2::new(right_x, bar_rect.bottom() - 12.0)],
        Stroke::new(1.0, palette.border_subtle),
    );
    right_x -= 10.0;

    // Pin (Always on top)
    let pin_rect = Rect::from_center_size(Pos2::new(right_x - 14.0, btn_y), Vec2::new(28.0, 26.0));
    right_x -= 34.0;
    let pin_resp = ui.interact(pin_rect, Id::unique("tb_pin"), Sense::click()).on_hover_text(s.pin_tooltip);
    let pin_hover = ui.ctx().animate_bool_with_time(pin_resp.id, pin_resp.hovered(), 0.10);
    let pin_bg = if always_on_top {
        Palette::alpha(palette.accent, 42)
    } else {
        Palette::alpha(palette.bg_elevated, (pin_hover * 210.0) as u8)
    };
    let pin_fg = if always_on_top { palette.accent } else { palette.text_secondary };
    ui.painter().rect(
        pin_rect,
        CornerRadius::same(6),
        pin_bg,
        if always_on_top { Stroke::new(1.0, Palette::alpha(palette.accent, 130)) } else { Stroke::NONE },
        egui::StrokeKind::Inside,
    );
    // Vector pin glyph
    let pc = pin_rect.center();
    ui.painter().circle_stroke(Pos2::new(pc.x, pc.y - 2.5), 3.2, Stroke::new(1.4, pin_fg));
    ui.painter().line_segment(
        [Pos2::new(pc.x, pc.y + 1.0), Pos2::new(pc.x, pc.y + 6.0)],
        Stroke::new(1.4, pin_fg),
    );
    if pin_resp.clicked() {
        action.pin_toggled = true;
    }

    // Language toggle pill (RU / EN)
    let lang_rect = Rect::from_center_size(Pos2::new(right_x - 18.0, btn_y), Vec2::new(36.0, 26.0));
    right_x -= 42.0;
    let lang_resp = ui.interact(lang_rect, Id::unique("tb_lang"), Sense::click()).on_hover_text(s.lang_tooltip);
    let lang_hover = ui.ctx().animate_bool_with_time(lang_resp.id, lang_resp.hovered(), 0.10);
    ui.painter().rect(
        lang_rect,
        CornerRadius::same(6),
        Palette::lerp(palette.bg_surface, palette.bg_elevated, lang_hover),
        Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        lang_rect.center(),
        egui::Align2::CENTER_CENTER,
        lang.code(),
        FontId::monospace(11.5),
        palette.text_primary,
    );
    if lang_resp.clicked() {
        action.lang_toggled = true;
    }

    // Theme selector pill (color dot + name)
    let theme_w = 82.0;
    let theme_rect = Rect::from_center_size(Pos2::new(right_x - theme_w * 0.5, btn_y), Vec2::new(theme_w, 26.0));
    let theme_resp = ui.interact(theme_rect, Id::unique("tb_theme"), Sense::click()).on_hover_text(s.theme_tooltip);
    let theme_hover = ui.ctx().animate_bool_with_time(theme_resp.id, theme_resp.hovered(), 0.10);
    ui.painter().rect(
        theme_rect,
        CornerRadius::same(6),
        Palette::lerp(palette.bg_surface, palette.bg_elevated, theme_hover),
        Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    ui.painter().circle_filled(Pos2::new(theme_rect.left() + 12.0, btn_y), 4.0, palette.accent);
    ui.painter().text(
        Pos2::new(theme_rect.left() + 22.0, btn_y),
        egui::Align2::LEFT_CENTER,
        theme.label(lang),
        FontId::proportional(11.5),
        palette.text_primary,
    );
    if theme_resp.clicked() {
        action.theme_cycled = true;
    }
    let right_limit = theme_rect.left() - 8.0;

    // ── Center: segmented tab control (clamped cleanly between brand and right controls) ──
    let tabs = [
        (ActiveTab::Notebook, s.tab_notebook),
        (ActiveTab::Scientific, s.tab_scientific),
        (ActiveTab::Plotter, s.tab_plot),
        (ActiveTab::Programmer, s.tab_programmer),
    ];
    let tab_h = 28.0f32;
    let pad = 3.0f32;
    let avail_tabs_w = (right_limit - left_limit).max(160.0);
    let tab_w = ((avail_tabs_w - pad * 2.0) / tabs.len() as f32).clamp(56.0, 96.0);
    let tab_fs = if tab_w < 84.0 { 11.5 } else { 13.0 };
    let group_w = tab_w * tabs.len() as f32 + pad * 2.0;
    let group_h = tab_h + pad * 2.0;
    let ideal_left = bar_rect.center().x - group_w * 0.5;
    let clamped_left = ideal_left.clamp(left_limit, (right_limit - group_w).max(left_limit));
    let group_rect = Rect::from_min_size(
        Pos2::new(clamped_left, bar_rect.center().y - group_h * 0.5),
        Vec2::new(group_w, group_h),
    );

    ui.painter().rect(
        group_rect,
        CornerRadius::same(9),
        palette.bg_crust,
        Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );

    let active_idx = tabs.iter().position(|(t, _)| *t == tab).unwrap_or(0) as f32;
    let anim_idx = ui.ctx().animate_value_with_time(Id::unique("lumina_tab_slide"), active_idx, 0.16);

    let indicator_rect = Rect::from_min_size(
        Pos2::new(group_rect.left() + pad + anim_idx * tab_w, group_rect.top() + pad),
        Vec2::new(tab_w, tab_h),
    );
    ui.painter().rect(
        indicator_rect,
        CornerRadius::same(7),
        palette.bg_elevated,
        Stroke::new(1.0, Palette::alpha(palette.accent, 110)),
        egui::StrokeKind::Inside,
    );

    for (i, (t, label)) in tabs.iter().enumerate() {
        let r = Rect::from_min_size(
            Pos2::new(group_rect.left() + pad + i as f32 * tab_w, group_rect.top() + pad),
            Vec2::new(tab_w, tab_h),
        );
        let resp = ui.interact(r, Id::unique(("lumina_tab", i)), Sense::click());
        let is_active = *t == tab;
        let hover_t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.10);
        let text_col = if is_active {
            palette.text_primary
        } else {
            Palette::lerp(palette.text_secondary, palette.text_primary, hover_t)
        };
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            FontId::proportional(tab_fs),
            text_col,
        );
        if resp.clicked() && !is_active {
            action.tab_changed = Some(*t);
        }
    }

    // Drag or double-click empty titlebar space to move / maximize window.
    if bar_resp.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if bar_resp.double_clicked() {
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!is_maximized));
    }

    action
}

fn paint_win_button(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    palette: &Palette,
    danger: bool,
    draw_icon: impl FnOnce(&egui::Painter, Pos2, Color32),
) -> bool {
    let resp = ui.interact(rect, id, Sense::click());
    let hover = ui.ctx().animate_bool_with_time(id, resp.hovered(), 0.10);
    let hover_bg = if danger { palette.error } else { palette.bg_elevated };
    let bg = Palette::alpha(hover_bg, (hover * 230.0) as u8);
    let fg = if danger && hover > 0.35 {
        Color32::WHITE
    } else {
        Palette::lerp(palette.text_secondary, palette.text_primary, hover)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), bg);
    draw_icon(ui.painter(), rect.center(), fg);
    resp.clicked()
}

/// Crisp vector 4-point Lumina prism icon.
fn paint_lumina_logo(painter: &egui::Painter, c: Pos2, r: f32, palette: &Palette) {
    let inner = r * 0.34;
    let pts = vec![
        Pos2::new(c.x, c.y - r),
        Pos2::new(c.x + inner, c.y - inner),
        Pos2::new(c.x + r, c.y),
        Pos2::new(c.x + inner, c.y + inner),
        Pos2::new(c.x, c.y + r),
        Pos2::new(c.x - inner, c.y + inner),
        Pos2::new(c.x - r, c.y),
        Pos2::new(c.x - inner, c.y - inner),
    ];
    painter.add(egui::Shape::convex_polygon(pts, palette.accent, Stroke::NONE));
    painter.circle_filled(c, r * 0.22, palette.accent_alt);
}
