//! Two-column reactive math notebook (Soulver / Numi style).
//!
//! - Low-overhead render loop when idle: caches both the syntax-highlighted
//!   `LayoutJob` and formatted result strings using a dirty-flag key.
//! - Pixel-aligned gutter line numbers (left) and clickable results (right)
//!   derived directly from `output.galley.rows`, staying aligned even with wrapping.
//! - One-click result copy with toast feedback, right-click actions, and inline
//!   error underlining + hover tooltips without intrusive popups.

use crate::i18n::{Lang, Strings, format_error, tr};
use crate::style::{self, Palette, ThemeId};
use egui::text::LayoutJob;
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Galley, Id, Margin, Pos2, Rect,
    ScrollArea, Sense, Shape, Stroke, TextEdit, Ui, Vec2,
};
use lumina_core::{FormatOptions, LineOutput, Notebook};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LayoutKey {
    doc_version: u64,
    theme: ThemeId,
    accent: Color32,
    font_size_bits: u32,
    wrap_width_bits: u32,
}

/// Caches the syntax-highlighted [`LayoutJob`] (NOT `Arc<Galley>`).
/// Caching `Galley` across frames can serve stale glyph-atlas UVs whenever
/// another tab adds glyphs and triggers an `epaint` font-atlas repack.
struct CachedLayout {
    key: LayoutKey,
    job: LayoutJob,
}

struct CachedResults {
    doc_version: u64,
    precision: u32,
    formatted: Vec<Option<String>>,
    plain: Vec<Option<String>>,
    plain_num_only: Vec<Option<String>>,
}

pub enum NotebookAction {
    None,
    Copy { text: String, label: String },
    SendToPlot(String),
}

pub struct NotebookView {
    pub text: String,
    pub notebook: Notebook,
    layout_cache: Option<CachedLayout>,
    results_cache: Option<CachedResults>,
    active_line: usize,
    pending_insert: Option<String>,
}

impl NotebookView {
    pub fn new(initial_text: String) -> Self {
        let mut notebook = Notebook::new();
        notebook.sync(&initial_text);
        Self {
            text: initial_text,
            notebook,
            layout_cache: None,
            results_cache: None,
            active_line: 0,
            pending_insert: None,
        }
    }

    pub fn append_line(&mut self, line: &str) {
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push('\n');
        }
        self.text.push_str(line);
        self.notebook.sync(&self.text);
    }

    fn ensure_results_cache(&mut self, precision: u32) {
        let ver = self.notebook.version();
        if self
            .results_cache
            .as_ref()
            .is_some_and(|c| c.doc_version == ver && c.precision == precision)
        {
            return;
        }
        let opts = FormatOptions { precision, grouping: Some(' ') };
        let opts_plain = FormatOptions { precision, grouping: None };
        let mut formatted = Vec::with_capacity(self.notebook.lines().len());
        let mut plain = Vec::with_capacity(self.notebook.lines().len());
        let mut plain_num_only = Vec::with_capacity(self.notebook.lines().len());

        for line in self.notebook.lines() {
            match &line.output {
                LineOutput::Value(v) => {
                    formatted.push(Some(v.display(&opts)));
                    plain.push(Some(v.plain(precision)));
                    let num_only = lumina_core::value::format_decimal(
                        v.display_magnitude(),
                        &opts_plain,
                    );
                    plain_num_only.push(Some(num_only));
                }
                _ => {
                    formatted.push(None);
                    plain.push(None);
                    plain_num_only.push(None);
                }
            }
        }
        self.results_cache = Some(CachedResults {
            doc_version: ver,
            precision,
            formatted,
            plain,
            plain_num_only,
        });
    }

    /// Renders the two-column notebook and bottom variable/status bar.
    /// Returns `(action, text_changed)`.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        theme: ThemeId,
        lang: Lang,
        font_size: f32,
        precision: &mut u32,
        palette: &Palette,
    ) -> (NotebookAction, bool) {
        let s = tr(lang);
        let mut action = NotebookAction::None;
        let mut text_changed = false;

        if let Some(insert) = self.pending_insert.take() {
            if !self.text.is_empty() && !self.text.ends_with(['\n', ' ']) {
                self.text.push(' ');
            }
            self.text.push_str(&insert);
            self.notebook.sync(&self.text);
            text_changed = true;
        }

        // Bottom bar height reserved first so the scroll area takes the rest cleanly.
        let bottom_h = 48.0;
        let avail = ui.available_rect_before_wrap();
        let editor_area_h = (avail.height() - bottom_h - 1.0).max(120.0);

        // Two-column geometry
        let total_w = avail.width();
        let gutter_w = 42.0f32;
        let result_w = (total_w * 0.34).clamp(180.0, 340.0);
        let editor_w = (total_w - gutter_w - result_w).max(180.0);
        let divider_x = avail.left() + gutter_w + editor_w;

        // Paint subtle tinted background for the right results column across the full height.
        let right_col_bg = Rect::from_min_max(
            Pos2::new(divider_x, avail.top()),
            Pos2::new(avail.right(), avail.top() + editor_area_h),
        );
        ui.painter().rect_filled(right_col_bg, CornerRadius::ZERO, palette.bg_mantle);
        style::hairline_v(ui.painter(), right_col_bg, divider_x, palette.border_subtle);

        // Gutter background subtle tint
        let gutter_bg = Rect::from_min_max(
            Pos2::new(avail.left(), avail.top()),
            Pos2::new(avail.left() + gutter_w, avail.top() + editor_area_h),
        );
        ui.painter().rect_filled(
            gutter_bg,
            CornerRadius::ZERO,
            Palette::alpha(palette.bg_crust, 90),
        );

        let editor_id = Id::unique("lumina_notebook_editor");

        ScrollArea::vertical()
            .id_salt("lumina_notebook_scroll")
            .max_height(editor_area_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Allocate one unified canvas inside the scroll area so editor, gutter, and results scroll in lockstep.
                let scroll_top_left = ui.cursor().min;

                // Reserve background highlight slot painted *under* the text editor.
                let active_row_shape = ui.painter().add(Shape::Noop);

                // Sync notebook before layout in case text changed externally.
                self.notebook.sync(&self.text);

                // Borrow fields separately for the layouter closure.
                let notebook = &mut self.notebook;
                let layout_cache = &mut self.layout_cache;
                let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| -> Arc<Galley> {
                    let src = buf.as_str();
                    notebook.sync(src);
                    let key = LayoutKey {
                        doc_version: notebook.version(),
                        theme,
                        accent: palette.accent,
                        font_size_bits: font_size.to_bits(),
                        wrap_width_bits: wrap_width.to_bits(),
                    };
                    let job = if let Some(cached) = layout_cache.as_ref()
                        && cached.key == key
                    {
                        cached.job.clone()
                    } else {
                        let built = style::build_notebook_layout_job(
                            src,
                            notebook.lines(),
                            font_size,
                            wrap_width,
                            palette,
                        );
                        *layout_cache = Some(CachedLayout { key, job: built.clone() });
                        built
                    };
                    // `epaint`'s internal GalleyCache handles atlas invalidation automatically.
                    ui.painter().layout_job(job)
                };

                let text_edit_w = (editor_w - 18.0).max(120.0);

                // Position the TextEdit cleanly after the gutter so ScrollArea tracks true height.
                ui.add_space(10.0);
                let output = ui
                    .horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.add_space(gutter_w + 10.0);
                        TextEdit::multiline(&mut self.text)
                            .id(editor_id)
                            .frame(Frame::NONE)
                            .margin(Margin::ZERO)
                            .font(FontId::monospace(font_size))
                            .desired_width(text_edit_w)
                            .desired_rows(18)
                            .hint_text(s.nb_placeholder)
                            .lock_focus(true)
                            .layouter(&mut layouter)
                            .show(ui)
                    })
                    .inner;

                if output.response.changed() {
                    text_changed = true;
                    self.notebook.sync(&self.text);
                }

                // Determine active logical line from cursor char index.
                if let Some(range) = output.cursor_range {
                    let idx: usize = range.primary.index.into();
                    self.active_line = self
                        .text
                        .chars()
                        .take(idx)
                        .filter(|&c| c == '\n')
                        .count();
                }

                // Ensure formatted results cache matches current document version and precision.
                self.ensure_results_cache(*precision);

                // Extract exact Y-bands for every logical line from `output.galley.rows`.
                let min_h = (font_size * 1.35).max(18.0);
                let mut line_bands: Vec<(f32, f32)> = Vec::with_capacity(self.notebook.lines().len());
                let mut cur_top: Option<f32> = None;
                let mut cur_bot: f32 = output.galley_pos.y;
                for placed in &output.galley.rows {
                    let r = placed.rect();
                    let top = output.galley_pos.y + r.top();
                    let bot = output.galley_pos.y + r.bottom();
                    if cur_top.is_none() {
                        cur_top = Some(top);
                    }
                    cur_bot = bot;
                    if placed.ends_with_newline
                        && let Some(t) = cur_top.take()
                    {
                        line_bands.push((t, cur_bot.max(t + min_h)));
                    }
                }
                if let Some(t) = cur_top {
                    line_bands.push((t, cur_bot.max(t + min_h)));
                }

                // Paint active-line subtle highlight across both columns under the text.
                if let Some(&(y0, y1)) = line_bands.get(self.active_line) {
                    let highlight_rect = Rect::from_min_max(
                        Pos2::new(scroll_top_left.x, y0 - 2.0),
                        Pos2::new(scroll_top_left.x + total_w, y1 + 2.0),
                    );
                    ui.painter().set(
                        active_row_shape,
                        Shape::rect_filled(
                            highlight_rect,
                            CornerRadius::ZERO,
                            Palette::alpha(palette.accent, if palette.dark { 18 } else { 22 }),
                        ),
                    );
                }

                // Paint line numbers in the left gutter & interactive results in the right column.
                let line_num_font = FontId::monospace((font_size - 2.5).max(10.0));
                let result_font = FontId::monospace(font_size);
                let err_font = FontId::proportional((font_size - 2.5).max(11.0));
                let clip_rect = ui.clip_rect();

                let mut pending_insert_local: Option<String> = None;

                for (i, &(y0, y1)) in line_bands.iter().enumerate() {
                    if y1 < clip_rect.top() - 24.0 || y0 > clip_rect.bottom() + 24.0 {
                        continue;
                    }
                    let y_center = 0.5 * (y0 + y1);
                    let is_active = i == self.active_line;

                    // Gutter line number
                    let num_color = if is_active { palette.accent } else { palette.text_muted };
                    ui.painter().text(
                        Pos2::new(scroll_top_left.x + gutter_w - 10.0, y_center),
                        Align2::RIGHT_CENTER,
                        (i + 1).to_string(),
                        line_num_font.clone(),
                        num_color,
                    );

                    // Active-line left accent bar in gutter
                    if is_active {
                        let bar = Rect::from_min_max(
                            Pos2::new(scroll_top_left.x, y0),
                            Pos2::new(scroll_top_left.x + 2.5, y1),
                        );
                        ui.painter().rect_filled(bar, CornerRadius::same(1), palette.accent);
                    }

                    let Some(line) = self.notebook.lines().get(i) else {
                        continue;
                    };

                    let res_rect = Rect::from_min_max(
                        Pos2::new(divider_x + 8.0, y0 - 1.0),
                        Pos2::new(scroll_top_left.x + total_w - 10.0, y1 + 1.0),
                    );

                    match &line.output {
                        LineOutput::Empty => {}
                        LineOutput::Value(_) => {
                            let formatted = self
                                .results_cache
                                .as_ref()
                                .and_then(|c| c.formatted.get(i))
                                .and_then(|o| o.as_deref())
                                .unwrap_or("");
                            let plain = self
                                .results_cache
                                .as_ref()
                                .and_then(|c| c.plain.get(i))
                                .and_then(|o| o.as_deref())
                                .unwrap_or("");
                            let plain_num = self
                                .results_cache
                                .as_ref()
                                .and_then(|c| c.plain_num_only.get(i))
                                .and_then(|o| o.as_deref())
                                .unwrap_or("");

                            let resp = ui
                                .interact(res_rect, Id::unique(("nb_res", i)), Sense::click())
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .on_hover_text(s.nb_click_to_copy);

                            let hover_t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.10);
                            if hover_t > 0.01 {
                                ui.painter().rect(
                                    res_rect,
                                    CornerRadius::same(6),
                                    Palette::alpha(palette.bg_elevated, (hover_t * 210.0) as u8),
                                    Stroke::new(
                                        1.0,
                                        Palette::alpha(palette.accent, (hover_t * 120.0) as u8),
                                    ),
                                    egui::StrokeKind::Inside,
                                );
                            }

                            let text_color = if is_active || resp.hovered() {
                                palette.accent
                            } else {
                                palette.success
                            };

                            // Right-aligned clipped result string
                            let clipped = ui.painter().with_clip_rect(res_rect.shrink2(Vec2::new(6.0, 0.0)));
                            clipped.text(
                                Pos2::new(res_rect.right() - 8.0, y_center),
                                Align2::RIGHT_CENTER,
                                formatted,
                                result_font.clone(),
                                text_color,
                            );

                            if resp.clicked() {
                                action = NotebookAction::Copy {
                                    text: plain.to_owned(),
                                    label: formatted.to_owned(),
                                };
                            }

                            let line_src = line.source.as_str();
                            resp.context_menu(|ui| {
                                if ui.button(s.nb_copy_full).clicked() {
                                    action = NotebookAction::Copy {
                                        text: plain.to_owned(),
                                        label: formatted.to_owned(),
                                    };
                                    ui.close();
                                }
                                if ui.button(s.nb_copy_val).clicked() {
                                    action = NotebookAction::Copy {
                                        text: plain_num.to_owned(),
                                        label: plain_num.to_owned(),
                                    };
                                    ui.close();
                                }
                                if ui.button(format!("{} #{}", s.nb_insert_ref, i + 1)).clicked() {
                                    pending_insert_local = Some(format!("#{}", i + 1));
                                    ui.close();
                                }
                                if ui.button(s.nb_send_to_plot).clicked() {
                                    action = NotebookAction::SendToPlot(line_src.to_owned());
                                    ui.close();
                                }
                            });
                        }
                        LineOutput::Error(err) => {
                            let msg = format_error(lang, &err.kind);
                            let resp = ui
                                .interact(res_rect, Id::unique(("nb_err", i)), Sense::hover())
                                .on_hover_text(&msg);

                            let hover_t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.10);
                            if hover_t > 0.01 {
                                ui.painter().rect_filled(
                                    res_rect,
                                    CornerRadius::same(6),
                                    Palette::alpha(palette.error, (hover_t * 36.0) as u8),
                                );
                            }

                            let clipped = ui.painter().with_clip_rect(res_rect.shrink2(Vec2::new(6.0, 0.0)));
                            clipped.text(
                                Pos2::new(res_rect.right() - 8.0, y_center),
                                Align2::RIGHT_CENTER,
                                format!("! {msg}"),
                                err_font.clone(),
                                Palette::alpha(palette.error, 200),
                            );
                        }
                    }
                }

                if let Some(ins) = pending_insert_local {
                    self.pending_insert = Some(ins);
                }

                // Bottom breathing room
                ui.add_space(28.0);
            });

        // ── Bottom status / variables bar ──
        if self.show_bottom_bar(ui, s, precision, palette) {
            text_changed = true;
        }

        (action, text_changed)
    }

    fn show_bottom_bar(
        &mut self,
        ui: &mut Ui,
        s: &Strings,
        precision: &mut u32,
        palette: &Palette,
    ) -> bool {
        let mut text_changed = false;
        style::hairline_h(ui, palette.border_subtle);
        Frame::new()
            .fill(palette.bg_mantle)
            .inner_margin(Margin {
                left: 16,
                right: 20,
                top: 10,
                bottom: 14,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Left: defined variables as clickable pills
                    ui.label(
                        egui::RichText::new(s.nb_vars_label)
                            .size(12.0)
                            .color(palette.text_muted),
                    );

                    let mut vars: Vec<(&str, String)> = self
                        .notebook
                        .env()
                        .vars
                        .iter()
                        .map(|(k, v)| (k.as_ref(), v.plain(4)))
                        .collect();
                    vars.sort_by(|a, b| a.0.cmp(b.0));

                    if vars.is_empty() {
                        ui.label(
                            egui::RichText::new(s.nb_no_vars)
                                .size(12.0)
                                .color(palette.text_muted),
                        );
                    } else {
                        for (name, val) in vars.iter().take(6) {
                            let label = format!("{name} = {val}");
                            let resp = style::paint_pill(
                                ui,
                                &label,
                                palette.bg_surface,
                                palette.syn_assign,
                                Stroke::new(1.0, palette.border_subtle),
                                11.5,
                            );
                            if resp.clicked() {
                                self.pending_insert = Some((*name).to_owned());
                            }
                        }
                    }

                    // Right: precision + Examples + Clear
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(egui::RichText::new(s.nb_clear).size(12.0)).clicked() {
                            self.text.clear();
                            self.notebook.sync("");
                            text_changed = true;
                        }
                        if ui.button(egui::RichText::new(s.nb_examples).size(12.0)).clicked() {
                            self.text = s.nb_default_doc.to_owned();
                            self.notebook.sync(&self.text);
                            text_changed = true;
                        }
                        ui.add_space(6.0);
                        ui.add(
                            egui::DragValue::new(precision)
                                .range(1..=15)
                                .speed(0.1),
                        );
                        ui.label(
                            egui::RichText::new(s.nb_precision)
                                .size(12.0)
                                .color(palette.text_secondary),
                        );
                    });
                });
            });
        text_changed
    }
}
