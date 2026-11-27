use crate::i18n::{Lang, Strings, format_error, tr};
use crate::style::{self, Palette, ThemeId};
use egui::text::LayoutJob;
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Galley, Id, Margin, Pos2, Rect,
    ScrollArea, Sense, Shape, Stroke, TextEdit, Ui,
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

        let bottom_h = 48.0;
        let avail = ui.available_rect_before_wrap();
        let editor_area_h = (avail.height() - bottom_h - 1.0).max(120.0);

        let total_w = avail.width();
        let gutter_w = 42.0f32;
        let result_w = (total_w * 0.34).clamp(180.0, 340.0);
        let editor_w = (total_w - gutter_w - result_w).max(180.0);
        let divider_x = avail.left() + gutter_w + editor_w;

        let right_col_bg = Rect::from_min_max(
            Pos2::new(divider_x, avail.top()),
            Pos2::new(avail.right(), avail.top() + editor_area_h),
        );
        ui.painter().rect_filled(right_col_bg, CornerRadius::ZERO, palette.bg_mantle);
        style::hairline_v(ui.painter(), right_col_bg, divider_x, palette.border_subtle);

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
                let scroll_top_left = ui.cursor().min;

                let active_row_shape = ui.painter().add(Shape::Noop);

                self.notebook.sync(&self.text);

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
                    ui.painter().layout_job(job)
                };

                let text_edit_w = (editor_w - 18.0).max(120.0);

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

                if let Some(range) = output.cursor_range {
                    let idx: usize = range.primary.index.into();
                    self.active_line = self
                        .text
                        .chars()
                        .take(idx)
                        .filter(|&c| c == '\n')
                        .count();
                }

                self.ensure_results_cache(*precision);

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
                for idx in 0..line_bands.len().saturating_sub(1) {
                    let next_top = line_bands[idx + 1].0;
                    if line_bands[idx].1 > next_top {
                        line_bands[idx].1 = next_top.max(line_bands[idx].0);
                    }
                }

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

                    let num_color = if is_active { palette.accent } else { palette.text_muted };
                    ui.painter().text(
                        Pos2::new(scroll_top_left.x + gutter_w - 10.0, y_center),
                        Align2::RIGHT_CENTER,
                        (i + 1).to_string(),
                        line_num_font.clone(),
                        num_color,
                    );

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

                    let res_left = divider_x + 8.0;
                    let res_right = (scroll_top_left.x + total_w - 10.0).max(res_left);
                    let pad_y = if y1 - y0 >= 4.0 { 0.5 } else { 0.0 };
                    let res_rect = Rect::from_min_max(
                        Pos2::new(res_left, y0 + pad_y),
                        Pos2::new(res_right, (y1 - pad_y).max(y0 + pad_y)),
                    );
                    let text_clip = Rect::from_min_max(
                        Pos2::new((res_rect.left() + 6.0).min(res_rect.right()), res_rect.top()),
                        Pos2::new((res_rect.right() - 6.0).max(res_rect.left()), res_rect.bottom()),
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

                            let clipped = ui.painter().with_clip_rect(text_clip);
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
                                .interact(res_rect, Id::unique(("nb_err", i)), Sense::click())
                                .on_hover_text(&msg);

                            let hover_t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.10);
                            if hover_t > 0.01 {
                                ui.painter().rect_filled(
                                    res_rect,
                                    CornerRadius::same(6),
                                    Palette::alpha(palette.error, (hover_t * 36.0) as u8),
                                );
                            }

                            let clipped = ui.painter().with_clip_rect(text_clip);
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

                ui.add_space(28.0);
            });

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

#[cfg(test)]
mod tests {
    use super::*;

    fn filled_doc() -> String {
        let base = "\
# Physics & Unit Conversions
dist = 420 km
speed = 120 km/h
speed in m/s
time = dist / speed in min

# Exact Decimal & Percentages
0.1 + 0.2
price = 2490
price - 15%
20% of price

# Geometry & Constants
r = 5 cm
area = pi * r^2 in cm^2
sin(30°) + cos(60°)

# Block Aggregates & Line Refs
120
340
540
sum
avg
#22 + #23

# Programmer & Data
(0xFF << 4) | 0x0A in hex
1 GiB in MB
22 °C in °F
# Кириллица и ошибки
ошибка = 1 +
1 / 0
#999 + 5
1/3 K in °C";
        let mut out = String::from(base);
        for i in 0..45 {
            out.push_str(&format!("\nстрока_{i} = ({i} + 1) * 2.5 + sin(30°)"));
        }
        out.push_str("\nsum\navg\n");
        out
    }

    #[test]
    fn filled_notebook_renders_scrolls_and_resizes_without_panic() {
        let ctx = egui::Context::default();
        style::install_fonts(&ctx);
        let palette = style::apply_theme(&ctx, ThemeId::Dark);
        let mut view = NotebookView::new(filled_doc());
        let mut precision = 8u32;

        for (w, h) in [(1040.0, 680.0), (680.0, 440.0), (320.0, 260.0), (1040.0, 680.0)] {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(w, h))),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let _ = view.show(ui, ThemeId::Dark, Lang::Ru, 15.0, &mut precision, &palette);
                });
            });
            out.textures_delta.clear();
        }

        view.append_line("22 °C in °F");
        view.append_line("sin(30°");
        let click_input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(900.0, 600.0))),
            events: vec![
                egui::Event::PointerMoved(Pos2::new(160.0, 140.0)),
                egui::Event::PointerButton {
                    pos: Pos2::new(160.0, 140.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: Pos2::new(160.0, 140.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let mut out = ctx.run_ui(click_input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = view.show(ui, ThemeId::Light, Lang::En, 15.0, &mut precision, &palette);
            });
        });
        out.textures_delta.clear();

        let type_input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(900.0, 600.0))),
            events: vec![
                egui::Event::Text(" + 2² + π".to_owned()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let mut out = ctx.run_ui(type_input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = view.show(ui, ThemeId::Dark, Lang::Ru, 15.0, &mut precision, &palette);
            });
        });
        out.textures_delta.clear();
    }

    #[test]
    fn highlight_layout_handles_unaligned_utf8_and_stale_spans() {
        let palette = Palette::for_theme(ThemeId::Dark);
        let src = "sin(30°) + 2² + π + кириллица";
        for start in 0..=src.len() + 5 {
            for end in [start, start + 1, start + 3, src.len() + 10] {
                let job = style::build_singleline_highlight(
                    src,
                    false,
                    Some(&(start..end)),
                    14.0,
                    400.0,
                    &palette,
                );
                assert_eq!(job.text, src);
            }
        }
        let nb = Notebook::from_text("22 °C in °F\r\nошибка = 1 +\r\n");
        let job = style::build_notebook_layout_job(nb.text(), nb.lines(), 15.0, 500.0, &palette);
        assert_eq!(job.text, nb.text());
    }

    #[test]
    fn default_notebook_docs_evaluate_without_errors() {
        for lang in [Lang::En, Lang::Ru] {
            let doc = tr(lang).nb_default_doc;
            let nb = Notebook::from_text(doc);
            assert_eq!(nb.lines().len(), 29, "lang={lang:?}");
            for (idx, line) in nb.lines().iter().enumerate() {
                assert!(
                    !matches!(line.output, LineOutput::Error(_)),
                    "lang={lang:?} line {} ('{}') failed: {:?}",
                    idx + 1,
                    line.source,
                    line.output
                );
            }
        }
    }

    #[test]
    fn adjacent_value_and_error_rows_never_hover_simultaneously() {
        let ctx = egui::Context::default();
        style::install_fonts(&ctx);
        let palette = style::apply_theme(&ctx, ThemeId::Dark);
        let mut view = NotebookView::new("100 + 200\n#13 + #14".to_owned());
        let mut precision = 8u32;

        let mut hovered_pairs = 0usize;
        for step in 0..80 {
            let y = 10.0 + step as f32 * 0.5;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(900.0, 400.0))),
                events: vec![egui::Event::PointerMoved(Pos2::new(760.0, y))],
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let _ = view.show(ui, ThemeId::Dark, Lang::Ru, 15.0, &mut precision, &palette);
                });
            });
            out.textures_delta.clear();

            let val_hovered = ctx.read_response(Id::unique(("nb_res", 0usize))).is_some_and(|r| r.hovered());
            let err_hovered = ctx.read_response(Id::unique(("nb_err", 1usize))).is_some_and(|r| r.hovered());
            if val_hovered && err_hovered {
                hovered_pairs += 1;
            }
        }
        assert_eq!(hovered_pairs, 0, "adjacent Value and Error rows must never hover at the same time");
    }
}
