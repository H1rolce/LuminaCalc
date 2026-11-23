//! Cohesive design system for LuminaCalc.
//!
//! Replaces default egui visuals with three tailored themes (Catppuccin Mocha,
//! Layan Dark, Catppuccin Latte), semantic syntax-highlighting palettes,
//! crisp system fonts with Cyrillic + math glyph coverage, and smooth UI helpers.

use crate::i18n::Lang;
use egui::text::LayoutJob;
use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect,
    Stroke, TextFormat, Visuals, epaint::Shadow, style::Selection,
};
use lumina_core::{HighlightKind, HighlightSpan, LineEntry, Span};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ThemeId {
    #[default]
    #[serde(alias = "CatppuccinMocha", alias = "LayanDark", alias = "WindowsAccent")]
    Dark,
    #[serde(alias = "CatppuccinLatte")]
    Light,
}

impl ThemeId {
    pub fn next(self) -> Self {
        match self {
            ThemeId::Dark => ThemeId::Light,
            ThemeId::Light => ThemeId::Dark,
        }
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (ThemeId::Dark, Lang::Ru) => "Тёмная",
            (ThemeId::Light, Lang::Ru) => "Светлая",
            (ThemeId::Dark, Lang::En) => "Dark",
            (ThemeId::Light, Lang::En) => "Light",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg_crust: Color32,
    pub bg_mantle: Color32,
    pub bg_base: Color32,
    pub bg_surface: Color32,
    pub bg_elevated: Color32,
    pub bg_hover: Color32,
    pub border: Color32,
    pub border_subtle: Color32,

    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,

    pub accent: Color32,
    pub accent_alt: Color32,
    pub accent_fg: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,

    // Syntax colors
    pub syn_number: Color32,
    pub syn_variable: Color32,
    pub syn_assign: Color32,
    pub syn_constant: Color32,
    pub syn_function: Color32,
    pub syn_unit: Color32,
    pub syn_keyword: Color32,
    pub syn_operator: Color32,
    pub syn_punct: Color32,
    pub syn_lineref: Color32,
    pub syn_comment: Color32,
    pub syn_heading: Color32,

    pub plot_colors: [Color32; 6],
}

impl Palette {
    pub fn for_theme(theme: ThemeId) -> Self {
        let mut sys = crate::win_theme::read_system_theme();
        sys.dark = matches!(theme, ThemeId::Dark);
        Self::from_system_theme(sys)
    }

    /// Generates a cohesive Windows 11 Mica-inspired palette from the OS accent color & dark/light mode.
    pub fn from_system_theme(sys: crate::win_theme::SystemTheme) -> Self {
        let accent = crate::win_theme::tune_accent_for_mode(sys.accent, sys.dark);
        let accent_fg = if crate::win_theme::luminance(accent) > 0.52 {
            Color32::from_rgb(0x12, 0x12, 0x16)
        } else {
            Color32::WHITE
        };
        let accent_alt = if sys.dark {
            Self::lerp(accent, Color32::from_rgb(0x8A, 0xE2, 0xFF), 0.35)
        } else {
            Self::lerp(accent, Color32::from_rgb(0x1E, 0x66, 0xF5), 0.35)
        };

        if sys.dark {
            // Subtle Mica tint (4-6% of Windows accent mixed into neutral dark graphite surfaces)
            let tint = |base: Color32, k: f32| Self::lerp(base, sys.accent, k);
            Self {
                dark: true,
                bg_crust: tint(Color32::from_rgb(0x14, 0x14, 0x18), 0.04),
                bg_mantle: tint(Color32::from_rgb(0x1B, 0x1C, 0x22), 0.05),
                bg_base: tint(Color32::from_rgb(0x20, 0x21, 0x28), 0.05),
                bg_surface: tint(Color32::from_rgb(0x29, 0x2A, 0x33), 0.06),
                bg_elevated: tint(Color32::from_rgb(0x33, 0x35, 0x40), 0.07),
                bg_hover: tint(Color32::from_rgb(0x3E, 0x41, 0x4F), 0.09),
                border: tint(Color32::from_rgb(0x47, 0x4A, 0x58), 0.14),
                border_subtle: tint(Color32::from_rgb(0x30, 0x32, 0x3D), 0.09),

                text_primary: Color32::from_rgb(0xF3, 0xF4, 0xF8),
                text_secondary: Color32::from_rgb(0xB4, 0xB8, 0xC6),
                text_muted: Color32::from_rgb(0x78, 0x7D, 0x8F),

                accent,
                accent_alt,
                accent_fg,
                success: Color32::from_rgb(0x6C, 0xCB, 0x5F),
                warning: Color32::from_rgb(0xFC, 0xE1, 0x00),
                error: Color32::from_rgb(0xFF, 0x7A, 0x8A),

                syn_number: Color32::from_rgb(0xFF, 0xB9, 0x78),
                syn_variable: Color32::from_rgb(0xF3, 0xF4, 0xF8),
                syn_assign: accent_alt,
                syn_constant: Color32::from_rgb(0xF4, 0x9F, 0xDC),
                syn_function: accent,
                syn_unit: Color32::from_rgb(0x6E, 0xE0, 0xC8),
                syn_keyword: Self::lerp(accent, Color32::WHITE, 0.22),
                syn_operator: accent_alt,
                syn_punct: Color32::from_rgb(0x9A, 0xA0, 0xB2),
                syn_lineref: Color32::from_rgb(0xF9, 0xE2, 0xAF),
                syn_comment: Color32::from_rgb(0x75, 0x7A, 0x8C),
                syn_heading: accent,

                plot_colors: [
                    accent,
                    Color32::from_rgb(0x6C, 0xCB, 0x5F),
                    Color32::from_rgb(0xFF, 0xB9, 0x78),
                    Color32::from_rgb(0xFF, 0x7A, 0x8A),
                    Color32::from_rgb(0x6E, 0xE0, 0xC8),
                    Color32::from_rgb(0xF4, 0x9F, 0xDC),
                ],
            }
        } else {
            let tint = |base: Color32, k: f32| Self::lerp(base, sys.accent, k);
            Self {
                dark: false,
                bg_crust: tint(Color32::from_rgb(0xE3, 0xE6, 0xEC), 0.04),
                bg_mantle: tint(Color32::from_rgb(0xEC, 0xEE, 0xF3), 0.04),
                bg_base: tint(Color32::from_rgb(0xF5, 0xF6, 0xF9), 0.03),
                bg_surface: tint(Color32::from_rgb(0xEB, 0xED, 0xF2), 0.05),
                bg_elevated: tint(Color32::from_rgb(0xFF, 0xFF, 0xFF), 0.03),
                bg_hover: tint(Color32::from_rgb(0xDF, 0xE3, 0xEC), 0.07),
                border: tint(Color32::from_rgb(0xC2, 0xC7, 0xD2), 0.14),
                border_subtle: tint(Color32::from_rgb(0xD8, 0xDC, 0xE5), 0.08),

                text_primary: Color32::from_rgb(0x1B, 0x1D, 0x24),
                text_secondary: Color32::from_rgb(0x52, 0x56, 0x66),
                text_muted: Color32::from_rgb(0x80, 0x85, 0x96),

                accent,
                accent_alt,
                accent_fg,
                success: Color32::from_rgb(0x0F, 0x7B, 0x0F),
                warning: Color32::from_rgb(0x9D, 0x5D, 0x00),
                error: Color32::from_rgb(0xC4, 0x2B, 0x1C),

                syn_number: Color32::from_rgb(0xD1, 0x57, 0x00),
                syn_variable: Color32::from_rgb(0x1B, 0x1D, 0x24),
                syn_assign: accent,
                syn_constant: Color32::from_rgb(0xB4, 0x20, 0x8A),
                syn_function: accent_alt,
                syn_unit: Color32::from_rgb(0x0B, 0x7A, 0x75),
                syn_keyword: accent,
                syn_operator: Color32::from_rgb(0x1A, 0x6B, 0x8A),
                syn_punct: Color32::from_rgb(0x66, 0x6B, 0x7C),
                syn_lineref: Color32::from_rgb(0x9D, 0x5D, 0x00),
                syn_comment: Color32::from_rgb(0x85, 0x8A, 0x9B),
                syn_heading: accent,

                plot_colors: [
                    accent,
                    Color32::from_rgb(0x0F, 0x7B, 0x0F),
                    Color32::from_rgb(0xD1, 0x57, 0x00),
                    Color32::from_rgb(0xC4, 0x2B, 0x1C),
                    Color32::from_rgb(0x0B, 0x7A, 0x75),
                    Color32::from_rgb(0xB4, 0x20, 0x8A),
                ],
            }
        }
    }

    #[inline]
    pub fn syntax_color(&self, kind: HighlightKind, is_heading: bool) -> Color32 {
        match kind {
            HighlightKind::Number => self.syn_number,
            HighlightKind::Variable => self.syn_variable,
            HighlightKind::AssignTarget => self.syn_assign,
            HighlightKind::Constant => self.syn_constant,
            HighlightKind::Function => self.syn_function,
            HighlightKind::Unit => self.syn_unit,
            HighlightKind::Keyword => self.syn_keyword,
            HighlightKind::Operator => self.syn_operator,
            HighlightKind::Punct => self.syn_punct,
            HighlightKind::LineRef => self.syn_lineref,
            HighlightKind::Comment => {
                if is_heading {
                    self.syn_heading
                } else {
                    self.syn_comment
                }
            }
            HighlightKind::Error => self.error,
        }
    }

    #[inline]
    pub fn alpha(color: Color32, a: u8) -> Color32 {
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a)
    }

    #[inline]
    pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
        let t = t.clamp(0.0, 1.0);
        let mix = |x: u8, y: u8| -> u8 { (x as f32 + (y as f32 - x as f32) * t).round() as u8 };
        Color32::from_rgba_unmultiplied(
            mix(a.r(), b.r()),
            mix(a.g(), b.g()),
            mix(a.b(), b.b()),
            mix(a.a(), b.a()),
        )
    }
}

/// Applies the LuminaCalc design system to `egui::Context`.
pub fn apply_theme(ctx: &Context, theme: ThemeId) -> Palette {
    let p = Palette::for_theme(theme);

    let mut visuals = if p.dark { Visuals::dark() } else { Visuals::light() };
    visuals.dark_mode = p.dark;
    visuals.override_text_color = Some(p.text_primary);
    visuals.panel_fill = p.bg_base;
    visuals.window_fill = p.bg_mantle;
    visuals.faint_bg_color = p.bg_surface;
    visuals.extreme_bg_color = p.bg_crust;
    visuals.code_bg_color = p.bg_surface;
    visuals.hyperlink_color = p.accent_alt;
    visuals.warn_fg_color = p.warning;
    visuals.error_fg_color = p.error;

    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.window_stroke = Stroke::new(1.0, p.border);
    visuals.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 110 } else { 38 }),
    };
    visuals.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 95 } else { 30 }),
    };

    visuals.selection = Selection {
        bg_fill: Palette::alpha(p.accent, if p.dark { 64 } else { 48 }),
        stroke: Stroke::new(1.2, p.accent),
    };

    let r_sm = CornerRadius::same(6);

    visuals.widgets.noninteractive.bg_fill = p.bg_surface;
    visuals.widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border_subtle);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text_secondary);
    visuals.widgets.noninteractive.corner_radius = r_sm;

    visuals.widgets.inactive.bg_fill = p.bg_surface;
    visuals.widgets.inactive.weak_bg_fill = p.bg_surface;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border_subtle);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text_primary);
    visuals.widgets.inactive.corner_radius = r_sm;

    visuals.widgets.hovered.bg_fill = p.bg_elevated;
    visuals.widgets.hovered.weak_bg_fill = p.bg_hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, p.accent);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.2, p.text_primary);
    visuals.widgets.hovered.corner_radius = r_sm;
    visuals.widgets.hovered.expansion = 0.0;

    visuals.widgets.active.bg_fill = p.bg_hover;
    visuals.widgets.active.weak_bg_fill = p.bg_hover;
    visuals.widgets.active.bg_stroke = Stroke::new(1.5, p.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, p.text_primary);
    visuals.widgets.active.corner_radius = r_sm;
    visuals.widgets.active.expansion = 0.0;

    visuals.widgets.open.bg_fill = p.bg_elevated;
    visuals.widgets.open.weak_bg_fill = p.bg_elevated;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, p.accent);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, p.text_primary);
    visuals.widgets.open.corner_radius = r_sm;

    let mut style = (*ctx.global_style()).clone();
    style.visuals = visuals;
    style.animation_time = 0.14;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.scroll = egui::style::ScrollStyle::floating();
    ctx.set_global_style(style);

    p
}

/// Loads crisp system fonts with full Cyrillic + mathematical symbol support,
/// keeping egui's bundled fonts as graceful fallbacks.
pub fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();

    let mut prop_candidates: Vec<PathBuf> = Vec::new();
    let mut mono_candidates: Vec<PathBuf> = Vec::new();
    let mut sym_candidates: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let user_fonts = PathBuf::from(local).join("Microsoft/Windows/Fonts");
            mono_candidates.push(user_fonts.join("JetBrainsMono-Regular.ttf"));
            mono_candidates.push(user_fonts.join("CascadiaMono.ttf"));
            prop_candidates.push(user_fonts.join("Inter-Regular.ttf"));
        }
        let sys = PathBuf::from(r"C:\Windows\Fonts");
        prop_candidates.push(sys.join("segoeui.ttf"));
        prop_candidates.push(sys.join("arial.ttf"));
        mono_candidates.push(sys.join("CascadiaMono.ttf"));
        mono_candidates.push(sys.join("consola.ttf"));
        sym_candidates.push(sys.join("seguisym.ttf"));
        sym_candidates.push(sys.join("cambria.ttc"));
    }

    #[cfg(not(target_os = "windows"))]
    {
        prop_candidates.push(PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"));
        mono_candidates.push(PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"));
    }

    if let Some(bytes) = load_first(&prop_candidates) {
        fonts.font_data.insert("lumina_sans".into(), Arc::new(FontData::from_owned(bytes)));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "lumina_sans".into());
    }
    if let Some(bytes) = load_first(&mono_candidates) {
        fonts.font_data.insert("lumina_mono".into(), Arc::new(FontData::from_owned(bytes)));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "lumina_mono".into());
    }
    if let Some(bytes) = load_first(&sym_candidates) {
        fonts.font_data.insert("lumina_sym".into(), Arc::new(FontData::from_owned(bytes)));
        for fam in [FontFamily::Proportional, FontFamily::Monospace] {
            let list = fonts.families.entry(fam).or_default();
            let pos = list.len().min(1);
            list.insert(pos, "lumina_sym".into());
        }
    }

    ctx.set_fonts(fonts);
}

fn load_first(paths: &[PathBuf]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

/// Appends one highlighted line into `job`, applying inline error underlining
/// to `err_span` without popups.
pub fn append_highlighted_line(
    job: &mut LayoutJob,
    src: &str,
    spans: &[HighlightSpan],
    err_span: Option<&Span>,
    is_heading: bool,
    font_id: &FontId,
    palette: &Palette,
) {
    let base_fmt = |color: Color32, underline: Stroke, bg: Color32| TextFormat {
        font_id: font_id.clone(),
        color,
        underline,
        background: bg,
        italics: is_heading,
        ..Default::default()
    };

    let err_stroke = Stroke::new(1.6, palette.error);
    let err_bg = Palette::alpha(palette.error, if palette.dark { 28 } else { 22 });
    let err_range = err_span.map(|s| {
        if s.start == s.end {
            // Point-at-end error: underline the last char if any.
            let end = s.end.min(src.len());
            let start = src[..end].char_indices().next_back().map_or(end, |(i, _)| i);
            start..end
        } else {
            s.start.min(src.len())..s.end.min(src.len())
        }
    });

    let push_slice = |job: &mut LayoutJob, range: Span, color: Color32| {
        if range.start >= range.end || range.end > src.len() {
            return;
        }
        let Some(er) = &err_range else {
            job.append(&src[range], 0.0, base_fmt(color, Stroke::NONE, Color32::TRANSPARENT));
            return;
        };
        if er.end <= range.start || er.start >= range.end {
            job.append(&src[range], 0.0, base_fmt(color, Stroke::NONE, Color32::TRANSPARENT));
            return;
        }
        // Split around the error overlap so only the offending bytes are underlined.
        let ov_start = er.start.max(range.start);
        let ov_end = er.end.min(range.end);
        if range.start < ov_start {
            job.append(
                &src[range.start..ov_start],
                0.0,
                base_fmt(color, Stroke::NONE, Color32::TRANSPARENT),
            );
        }
        if ov_start < ov_end {
            job.append(&src[ov_start..ov_end], 0.0, base_fmt(palette.error, err_stroke, err_bg));
        }
        if ov_end < range.end {
            job.append(
                &src[ov_end..range.end],
                0.0,
                base_fmt(color, Stroke::NONE, Color32::TRANSPARENT),
            );
        }
    };

    let mut cursor = 0usize;
    for sp in spans {
        let start = sp.range.start.min(src.len());
        let end = sp.range.end.min(src.len());
        if start > cursor {
            push_slice(job, cursor..start, palette.text_primary);
        }
        if end > start {
            let color = palette.syntax_color(sp.kind, is_heading);
            push_slice(job, start..end, color);
            cursor = end;
        }
    }
    if cursor < src.len() {
        push_slice(job, cursor..src.len(), palette.text_primary);
    }
}

/// Builds a syntax-highlighted `LayoutJob` for the entire notebook document.
pub fn build_notebook_layout_job(
    full_text: &str,
    lines: &[LineEntry],
    font_size: f32,
    wrap_width: f32,
    palette: &Palette,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let min_row_h = (font_size * 1.35).max(18.0);
    job.first_row_min_height = min_row_h;
    let font_id = FontId::monospace(font_size);

    if full_text.is_empty() {
        let mut empty = LayoutJob::simple(String::new(), font_id, palette.text_primary, wrap_width);
        empty.first_row_min_height = min_row_h;
        return empty;
    }

    for (idx, line) in lines.iter().enumerate() {
        let err_span = line.output.error().map(|e| &e.span);
        let line_slice_end = (line.byte_start + line.raw_len).min(full_text.len());
        let clean = &line.source;

        append_highlighted_line(
            &mut job,
            clean,
            &line.highlights,
            err_span,
            line.is_heading,
            &font_id,
            palette,
        );

        // Preserve any trailing `\r` if pasted from CRLF source so LayoutJob length matches `full_text`.
        let extra_cr = line_slice_end.saturating_sub(line.byte_start + clean.len());
        if extra_cr > 0 {
            let cr_start = line.byte_start + clean.len();
            job.append(
                &full_text[cr_start..line_slice_end],
                0.0,
                TextFormat::simple(font_id.clone(), palette.text_primary),
            );
        }

        if idx + 1 < lines.len() {
            job.append("\n", 0.0, TextFormat::simple(font_id.clone(), palette.text_primary));
        }
    }

    // Safety fallback if `lines` was momentarily out of sync with `full_text`.
    if job.text.len() != full_text.len() {
        return LayoutJob::simple(full_text.to_owned(), font_id, palette.text_primary, wrap_width);
    }
    job
}

/// Builds a single-line highlighted `LayoutJob` (used by Plotter and Programmer inputs).
pub fn build_singleline_highlight(
    text: &str,
    plot_mode: bool,
    err_span: Option<&Span>,
    font_size: f32,
    wrap_width: f32,
    palette: &Palette,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let font_id = FontId::monospace(font_size);
    let tokens = lumina_core::lexer::lex(text);
    let empty = std::collections::HashSet::new();
    let spans = lumina_core::highlight::classify(&tokens, &empty, plot_mode);
    append_highlighted_line(&mut job, text, &spans, err_span, false, &font_id, palette);
    job
}

/// Paints a smooth pill badge (returns the painted rect).
pub fn paint_pill(
    ui: &mut egui::Ui,
    text: &str,
    bg: Color32,
    fg: Color32,
    stroke: Stroke,
    font_size: f32,
) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::proportional(font_size),
        fg,
    );
    let pad = egui::vec2(10.0, 4.0);
    let desired = galley.size() + pad * 2.0;
    let (rect, resp) = ui.allocate_exact_size(desired, egui::Sense::click());
    let hover_t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    let fill = Palette::lerp(bg, Color32::WHITE, hover_t * 0.07);
    ui.painter().rect(rect, CornerRadius::same(12), fill, stroke, egui::StrokeKind::Inside);
    let text_pos = Pos2::new(rect.left() + pad.x, rect.center().y - galley.size().y * 0.5);
    ui.painter().galley(text_pos, galley, fg);
    resp
}

/// Draws a subtle separator hairline.
pub fn hairline_h(ui: &mut egui::Ui, color: Color32) {
    let rect = ui.available_rect_before_wrap();
    let y = rect.top();
    ui.painter().line_segment(
        [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
        Stroke::new(1.0, color),
    );
    ui.add_space(1.0);
}

/// Draws a crisp vertical hairline inside `rect`.
pub fn hairline_v(painter: &egui::Painter, rect: Rect, x: f32, color: Color32) {
    painter.line_segment(
        [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
        Stroke::new(1.0, color),
    );
}
