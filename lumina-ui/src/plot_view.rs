use crate::i18n::{Lang, Strings, format_error, tr};
use crate::state::{SavedPlotCurve, SavedPlotParam};
use crate::style::{self, Palette};
use egui::{
    Align2, Color32, CornerRadius, FontId, Frame, Id, Margin, Pos2, Rect, ScrollArea, Sense,
    Stroke, TextEdit, Ui, Vec2,
};
use egui_plot::{HLine, Line, MarkerShape, Plot, PlotPoint, PlotPoints, Points, VLine};
use lumina_core::{
    CalcError, CompiledPlot, SnapKind, SnapPoint, compile_plot, eval_f64, find_snap_points,
    sample_curve,
};
use std::collections::HashMap;

pub struct PlotCurve {
    pub id: usize,
    pub expr_text: String,
    pub visible: bool,
    pub color_idx: usize,
    pub compiled: Result<CompiledPlot, CalcError>,
}

pub struct ParamSlider {
    pub name: Box<str>,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub animating: bool,
    pub dir: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    version: u64,
    x_min: u64,
    x_max: u64,
    y_min: u64,
    y_max: u64,
}

struct SampledCurve {
    color_idx: usize,
    name: String,
    segments: Vec<Vec<PlotPoint>>,
}

struct PlotCache {
    key: CacheKey,
    curves: Vec<SampledCurve>,
    snaps: Vec<SnapPoint>,
    snap_points_root: Vec<PlotPoint>,
    snap_points_inter: Vec<PlotPoint>,
    snap_points_other: Vec<PlotPoint>,
}

pub enum PlotAction {
    None,
    Copy(String),
}

pub struct PlotView {
    pub curves: Vec<PlotCurve>,
    pub params: Vec<ParamSlider>,
    pub snap_enabled: bool,
    param_memory: HashMap<String, (f64, f64, f64)>,
    next_curve_id: usize,
    version: u64,
    cache: Option<PlotCache>,
    reset_requested: bool,
    last_bounds: [f64; 4],
}

impl PlotView {
    pub fn new(
        saved_curves: &[SavedPlotCurve],
        saved_params: &[SavedPlotParam],
        snap_enabled: bool,
    ) -> Self {
        let mut param_memory = HashMap::new();
        for p in saved_params {
            param_memory.insert(p.name.clone(), (p.value, p.min, p.max));
        }

        let mut curves = Vec::new();
        let mut next_id = 1usize;
        for c in saved_curves {
            let compiled = compile_plot(&c.expr);
            curves.push(PlotCurve {
                id: next_id,
                expr_text: c.expr.clone(),
                visible: c.visible,
                color_idx: c.color_idx,
                compiled,
            });
            next_id += 1;
        }
        if curves.is_empty() {
            let expr = "y = a * sin(b * x)".to_owned();
            let compiled = compile_plot(&expr);
            curves.push(PlotCurve {
                id: next_id,
                expr_text: expr,
                visible: true,
                color_idx: 0,
                compiled,
            });
            next_id += 1;
        }

        let mut view = Self {
            curves,
            params: Vec::new(),
            snap_enabled,
            param_memory,
            next_curve_id: next_id,
            version: 1,
            cache: None,
            reset_requested: false,
            last_bounds: [-8.0, 8.0, -5.0, 5.0],
        };
        view.sync_params();
        view
    }

    fn next_free_color_idx(&self) -> usize {
        const PALETTE_LEN: usize = 6;
        (0..PALETTE_LEN)
            .find(|&idx| self.curves.iter().all(|c| c.color_idx % PALETTE_LEN != idx))
            .unwrap_or_else(|| self.next_curve_id.saturating_sub(1) % PALETTE_LEN)
    }

    pub fn add_expression(&mut self, expr: String) {
        let color_idx = self.next_free_color_idx();
        let compiled = compile_plot(&expr);
        self.curves.push(PlotCurve {
            id: self.next_curve_id,
            expr_text: expr,
            visible: true,
            color_idx,
            compiled,
        });
        self.next_curve_id += 1;
        self.sync_params();
        self.bump_version();
    }

    pub fn to_saved(&self) -> (Vec<SavedPlotCurve>, Vec<SavedPlotParam>, bool) {
        let curves = self
            .curves
            .iter()
            .map(|c| SavedPlotCurve {
                expr: c.expr_text.clone(),
                visible: c.visible,
                color_idx: c.color_idx,
            })
            .collect();
        let params = self
            .params
            .iter()
            .map(|p| SavedPlotParam {
                name: p.name.to_string(),
                value: p.value,
                min: p.min,
                max: p.max,
            })
            .collect();
        (curves, params, self.snap_enabled)
    }

    fn bump_version(&mut self) {
        self.version = self.version.wrapping_add(1).max(1);
    }

    fn sync_params(&mut self) {
        for p in &self.params {
            self.param_memory.insert(p.name.to_string(), (p.value, p.min, p.max));
        }

        let mut needed: Vec<Box<str>> = Vec::new();
        for c in &self.curves {
            if let Ok(compiled) = &c.compiled {
                for p in &compiled.params {
                    if !needed.iter().any(|k| k == p) {
                        needed.push(p.clone());
                    }
                }
            }
        }

        let prev_anim: HashMap<Box<str>, (bool, f64)> = self
            .params
            .iter()
            .map(|p| (p.name.clone(), (p.animating, p.dir)))
            .collect();

        self.params = needed
            .into_iter()
            .map(|name| {
                let (value, min, max) = self
                    .param_memory
                    .get(name.as_ref())
                    .copied()
                    .unwrap_or((1.0, -5.0, 5.0));
                let (animating, dir) = prev_anim.get(&name).copied().unwrap_or((false, 1.0));
                ParamSlider { name, value, min, max, animating, dir }
            })
            .collect();
    }

    fn param_pairs(&self) -> Vec<(Box<str>, f64)> {
        self.params.iter().map(|p| (p.name.clone(), p.value)).collect()
    }

    fn ensure_cache(&mut self, x_min: f64, x_max: f64, y_min: f64, y_max: f64) {
        let key = CacheKey {
            version: self.version,
            x_min: x_min.to_bits(),
            x_max: x_max.to_bits(),
            y_min: y_min.to_bits(),
            y_max: y_max.to_bits(),
        };
        if self.cache.as_ref().is_some_and(|c| c.key == key) {
            return;
        }

        let params = self.param_pairs();
        let y_span = (y_max - y_min).abs().max(1.0);

        let mut sampled = Vec::new();
        let mut active_compiled: Vec<&CompiledPlot> = Vec::new();
        let mut active_indices: Vec<usize> = Vec::new();

        for (idx, c) in self.curves.iter().enumerate() {
            if !c.visible {
                continue;
            }
            if let Ok(compiled) = &c.compiled {
                let raw_segs = sample_curve(compiled, x_min, x_max, y_span, 640, &params);
                let segments: Vec<Vec<PlotPoint>> = raw_segs
                    .into_iter()
                    .map(|seg| seg.into_iter().map(|[x, y]| PlotPoint::new(x, y)).collect())
                    .collect();
                sampled.push(SampledCurve {
                    color_idx: c.color_idx,
                    name: c.expr_text.clone(),
                    segments,
                });
                active_compiled.push(compiled);
                active_indices.push(idx);
            }
        }

        let mut snaps = find_snap_points(&active_compiled, x_min, x_max, y_min, y_max, 360, &params);
        for sp in &mut snaps {
            if let Some(&orig) = active_indices.get(sp.curve_a) {
                sp.curve_a = orig;
            }
            if let Some(b) = sp.curve_b {
                sp.curve_b = active_indices.get(b).copied();
            }
        }

        let mut snap_points_root = Vec::new();
        let mut snap_points_inter = Vec::new();
        let mut snap_points_other = Vec::new();
        for sp in &snaps {
            let pt = PlotPoint::new(sp.x, sp.y);
            match sp.kind {
                SnapKind::Root => snap_points_root.push(pt),
                SnapKind::Intersection => snap_points_inter.push(pt),
                SnapKind::YIntercept | SnapKind::Extremum => snap_points_other.push(pt),
            }
        }

        self.cache = Some(PlotCache {
            key,
            curves: sampled,
            snaps,
            snap_points_root,
            snap_points_inter,
            snap_points_other,
        });
    }

    fn advance_animations(&mut self, ctx: &egui::Context) {
        let dt = ctx.input(|i| i.stable_dt).min(0.05) as f64;
        let mut any_animating = false;
        for p in &mut self.params {
            if p.animating {
                any_animating = true;
                let span = (p.max - p.min).abs().max(0.01);
                p.value += p.dir * span * 0.22 * dt;
                if p.value >= p.max {
                    p.value = p.max;
                    p.dir = -1.0;
                } else if p.value <= p.min {
                    p.value = p.min;
                    p.dir = 1.0;
                }
            }
        }
        if any_animating {
            self.bump_version();
            ctx.request_repaint();
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        palette: &Palette,
    ) -> (PlotAction, bool) {
        let s = tr(lang);
        self.advance_animations(ui.ctx());

        let avail = ui.available_rect_before_wrap();
        let side_w = (avail.width() * 0.31).clamp(245.0, 340.0);
        let side_rect = Rect::from_min_size(avail.min, Vec2::new(side_w, avail.height()));
        let canvas_rect = Rect::from_min_max(
            Pos2::new(side_rect.right(), avail.top()),
            avail.max,
        );

        let dirty_state = self.show_sidebar(ui, side_rect, lang, s, palette);
        let action = self.show_canvas(ui, canvas_rect, s, palette);

        (action, dirty_state)
    }

    fn show_sidebar(
        &mut self,
        ui: &mut Ui,
        side_rect: Rect,
        lang: Lang,
        s: &Strings,
        palette: &Palette,
    ) -> bool {
        let mut dirty_state = false;

        ui.painter().rect_filled(side_rect, CornerRadius::ZERO, palette.bg_mantle);
        style::hairline_v(ui.painter(), side_rect, side_rect.right(), palette.border_subtle);

        ui.scope_builder(egui::UiBuilder::new().max_rect(side_rect.shrink(10.0)), |ui| {
            ScrollArea::vertical()
                .id_salt("plot_sidebar_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.show_curves_section(ui, lang, s, palette) {
                        dirty_state = true;
                    }

                    ui.add_space(10.0);
                    style::hairline_h(ui, palette.border_subtle);
                    ui.add_space(8.0);

                    if self.show_params_section(ui, s, palette) {
                        dirty_state = true;
                    }

                    ui.add_space(10.0);
                    style::hairline_h(ui, palette.border_subtle);
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        let snap_resp = ui.checkbox(&mut self.snap_enabled, s.plot_snap_toggle);
                        if snap_resp.changed() {
                            dirty_state = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(egui::RichText::new(s.plot_reset_view).size(12.0)).clicked() {
                                self.reset_requested = true;
                            }
                        });
                    });
                });
        });

        dirty_state
    }

    fn show_curves_section(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        s: &Strings,
        palette: &Palette,
    ) -> bool {
        let mut dirty_state = false;

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(s.plot_functions)
                    .size(13.0)
                    .strong()
                    .color(palette.text_secondary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let add_btn = style::paint_pill(
                    ui,
                    s.plot_add_fn,
                    Palette::alpha(palette.accent, 36),
                    palette.accent,
                    Stroke::new(1.0, Palette::alpha(palette.accent, 110)),
                    12.0,
                );
                if add_btn.clicked() {
                    let idx = self.curves.len();
                    let default_eq = match idx % 3 {
                        0 => "y = cos(x)",
                        1 => "y = 0.5 * x",
                        _ => "y = sin(x) / x",
                    };
                    self.add_expression(default_eq.to_owned());
                    dirty_state = true;
                }
            });
        });

        ui.add_space(6.0);

        let mut remove_idx: Option<usize> = None;
        let mut recomp_needed = false;

        for i in 0..self.curves.len() {
            let color = palette.plot_colors[self.curves[i].color_idx % palette.plot_colors.len()];
            let err_info = self.curves[i].compiled.as_ref().err().cloned();

            Frame::new()
                .fill(palette.bg_surface)
                .stroke(Stroke::new(
                    1.0,
                    if err_info.is_some() {
                        Palette::alpha(palette.error, 140)
                    } else {
                        palette.border_subtle
                    },
                ))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(8, 7))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (dot_r, dot_resp) =
                            ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                        if self.curves[i].visible {
                            ui.painter().circle_filled(dot_r.center(), 6.0, color);
                        } else {
                            ui.painter().circle_stroke(
                                dot_r.center(),
                                5.5,
                                Stroke::new(1.5, palette.text_muted),
                            );
                        }
                        if dot_resp.clicked() {
                            self.curves[i].visible = !self.curves[i].visible;
                            recomp_needed = true;
                            dirty_state = true;
                        }

                        let err_ctx = err_info
                            .as_ref()
                            .map(|e| (self.curves[i].expr_text.clone(), e.span.clone()));
                        let mut layouter = style::singleline_layouter(
                            true,
                            err_ctx.as_ref().map(|(src, span)| (src.as_str(), span)),
                            14.0,
                            palette,
                        );

                        let input_w = (ui.available_width() - 26.0).max(80.0);
                        let curve_id = self.curves[i].id;
                        let resp = ui.add(
                            TextEdit::singleline(&mut self.curves[i].expr_text)
                                .id(Id::unique(("plot_eq", curve_id)))
                                .frame(Frame::NONE)
                                .desired_width(input_w)
                                .hint_text(s.plot_expr_hint)
                                .layouter(&mut layouter),
                        );
                        if resp.changed() {
                            self.curves[i].compiled = compile_plot(&self.curves[i].expr_text);
                            recomp_needed = true;
                            dirty_state = true;
                        }

                        if self.curves.len() > 1 {
                            let (del_r, del_resp) =
                                ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                            let col = if del_resp.hovered() {
                                palette.error
                            } else {
                                palette.text_muted
                            };
                            ui.painter().text(
                                del_r.center(),
                                Align2::CENTER_CENTER,
                                "×",
                                FontId::proportional(15.0),
                                col,
                            );
                            if del_resp.clicked() {
                                remove_idx = Some(i);
                            }
                        }
                    });

                    if let Some(err) = &err_info {
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(format_error(lang, &err.kind))
                                .size(11.0)
                                .color(palette.error),
                        );
                    }
                });
            ui.add_space(5.0);
        }

        if let Some(idx) = remove_idx {
            self.curves.remove(idx);
            recomp_needed = true;
            dirty_state = true;
        }
        if recomp_needed {
            self.sync_params();
            self.bump_version();
        }

        dirty_state
    }

    fn show_params_section(&mut self, ui: &mut Ui, s: &Strings, palette: &Palette) -> bool {
        ui.label(
            egui::RichText::new(s.plot_params)
                .size(13.0)
                .strong()
                .color(palette.text_secondary),
        );
        ui.add_space(6.0);

        if self.params.is_empty() {
            ui.label(
                egui::RichText::new(s.plot_no_params)
                    .size(12.0)
                    .color(palette.text_muted),
            );
            return false;
        }

        let mut param_changed = false;
        for p in &mut self.params {
            Frame::new()
                .fill(palette.bg_surface)
                .stroke(Stroke::new(1.0, palette.border_subtle))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(8, 7))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let play_lbl = if p.animating { "❚❚" } else { "▶" };
                        let play_btn = style::paint_pill(
                            ui,
                            play_lbl,
                            if p.animating {
                                Palette::alpha(palette.accent, 45)
                            } else {
                                palette.bg_elevated
                            },
                            if p.animating { palette.accent } else { palette.text_secondary },
                            Stroke::new(1.0, palette.border_subtle),
                            10.5,
                        );
                        if play_btn.clicked() {
                            p.animating = !p.animating;
                        }

                        ui.label(
                            egui::RichText::new(format!("{} =", p.name))
                                .monospace()
                                .size(13.0)
                                .color(palette.syn_assign),
                        );
                        let dv = ui.add(
                            egui::DragValue::new(&mut p.value)
                                .speed(0.05)
                                .fixed_decimals(2),
                        );
                        if dv.changed() {
                            if p.value < p.min {
                                p.min = p.value;
                            }
                            if p.value > p.max {
                                p.max = p.value;
                            }
                            param_changed = true;
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(egui::RichText::new("↺").size(12.0))
                                        .frame(false),
                                )
                                .clicked()
                            {
                                p.value = 1.0;
                                p.min = -5.0;
                                p.max = 5.0;
                                p.animating = false;
                                param_changed = true;
                            }
                        });
                    });

                    ui.add_space(3.0);
                    let min = p.min;
                    let max = p.max.max(min + 0.01);
                    ui.spacing_mut().slider_width = (ui.available_width() - 8.0).max(80.0);
                    if ui
                        .add(egui::Slider::new(&mut p.value, min..=max).show_value(false))
                        .changed()
                    {
                        param_changed = true;
                    }

                    ui.horizontal(|ui| {
                        let dmin = ui.add(
                            egui::DragValue::new(&mut p.min)
                                .speed(0.2)
                                .max_decimals(2),
                        );
                        if dmin.changed() {
                            if p.min >= p.max {
                                p.max = p.min + 1.0;
                            }
                            p.value = p.value.clamp(p.min, p.max);
                            param_changed = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let dmax = ui.add(
                                egui::DragValue::new(&mut p.max)
                                    .speed(0.2)
                                    .max_decimals(2),
                            );
                            if dmax.changed() {
                                if p.max <= p.min {
                                    p.min = p.max - 1.0;
                                }
                                p.value = p.value.clamp(p.min, p.max);
                                param_changed = true;
                            }
                        });
                    });
                });
            ui.add_space(5.0);
        }
        if param_changed {
            self.bump_version();
        }
        param_changed
    }

    fn show_canvas(
        &mut self,
        ui: &mut Ui,
        canvas_rect: Rect,
        s: &Strings,
        palette: &Palette,
    ) -> PlotAction {
        let mut action = PlotAction::None;

        let [bx0, bx1, by0, by1] = self.last_bounds;
        self.ensure_cache(bx0, bx1, by0, by1);

        let params_snapshot = self.param_pairs();
        let snap_enabled = self.snap_enabled;

        let mut new_bounds: Option<[f64; 4]> = None;
        let mut hovered_snap: Option<(SnapPoint, Pos2)> = None;
        let mut hovered_curve_pt: Option<(f64, f64, Pos2, Color32)> = None;
        let mut plot_clicked = false;

        ui.scope_builder(egui::UiBuilder::new().max_rect(canvas_rect), |ui| {
            let mut plot = Plot::new("lumina_main_plot")
                .default_x_bounds(-8.0, 8.0)
                .default_y_bounds(-5.0, 5.0)
                .show_grid(true)
                .show_axes([true, true])
                .show_background(true)
                .allow_drag(true)
                .allow_zoom(true)
                .allow_scroll(true)
                .allow_boxed_zoom(true)
                .show_x(false)
                .show_y(false);

            if self.reset_requested {
                plot = plot.reset();
                self.reset_requested = false;
                self.last_bounds = [-8.0, 8.0, -5.0, 5.0];
            }

            let resp = plot.show(ui, |plot_ui| {
                let b = plot_ui.plot_bounds();
                let b_arr = [b.min()[0], b.max()[0], b.min()[1], b.max()[1]];
                if b_arr != self.last_bounds {
                    new_bounds = Some(b_arr);
                }

                let axis_col = Palette::alpha(palette.text_muted, 115);
                plot_ui.hline(HLine::new("x-axis", 0.0).color(axis_col).width(1.2));
                plot_ui.vline(VLine::new("y-axis", 0.0).color(axis_col).width(1.2));

                if let Some(cache) = &self.cache {
                    for sc in &cache.curves {
                        let col = palette.plot_colors[sc.color_idx % palette.plot_colors.len()];
                        for (seg_idx, seg) in sc.segments.iter().enumerate() {
                            let label = if seg_idx == 0 {
                                sc.name.as_str()
                            } else {
                                ""
                            };
                            plot_ui.line(
                                Line::new(label, PlotPoints::Borrowed(seg))
                                    .color(col)
                                    .width(2.2),
                            );
                        }
                    }

                    if snap_enabled {
                        if !cache.snap_points_root.is_empty() {
                            plot_ui.points(
                                Points::new("roots", PlotPoints::Borrowed(&cache.snap_points_root))
                                    .color(palette.success)
                                    .radius(4.2)
                                    .shape(MarkerShape::Circle),
                            );
                        }
                        if !cache.snap_points_inter.is_empty() {
                            plot_ui.points(
                                Points::new(
                                    "intersections",
                                    PlotPoints::Borrowed(&cache.snap_points_inter),
                                )
                                .color(palette.warning)
                                .radius(4.8)
                                .shape(MarkerShape::Diamond),
                            );
                        }
                        if !cache.snap_points_other.is_empty() {
                            plot_ui.points(
                                Points::new("extrema", PlotPoints::Borrowed(&cache.snap_points_other))
                                    .color(Palette::alpha(palette.accent_alt, 200))
                                    .radius(3.8)
                                    .shape(MarkerShape::Circle),
                            );
                        }
                    }

                    if let Some(ptr_plot) = plot_ui.pointer_coordinate() {
                        let transform = plot_ui.transform();
                        let ptr_screen = transform.position_from_point(&ptr_plot);

                        if snap_enabled {
                            let mut best_dist = 20.0f32;
                            for sp in &cache.snaps {
                                let sp_screen = transform.position_from_point(&PlotPoint::new(sp.x, sp.y));
                                let dist = sp_screen.distance(ptr_screen);
                                if dist < best_dist {
                                    best_dist = dist;
                                    hovered_snap = Some((*sp, sp_screen));
                                }
                            }
                        }

                        if hovered_snap.is_none() {
                            let mut best_dy = 22.0f32;
                            for c in &self.curves {
                                if !c.visible {
                                    continue;
                                }
                                if let Ok(compiled) = &c.compiled {
                                    let y = eval_f64(&compiled.expr, &compiled.indep, ptr_plot.x, &params_snapshot);
                                    if y.is_finite() {
                                        let pt_screen =
                                            transform.position_from_point(&PlotPoint::new(ptr_plot.x, y));
                                        let dy = (pt_screen.y - ptr_screen.y).abs();
                                        if dy < best_dy {
                                            best_dy = dy;
                                            let col = palette.plot_colors[c.color_idx % palette.plot_colors.len()];
                                            hovered_curve_pt = Some((ptr_plot.x, y, pt_screen, col));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            });

            plot_clicked = resp.response.clicked();
        });

        if let Some(b) = new_bounds {
            self.last_bounds = b;
            ui.ctx().request_repaint();
        }

        let overlay = ui.painter().with_clip_rect(canvas_rect);
        if let Some((sp, screen_pos)) = hovered_snap {
            let accent = match sp.kind {
                SnapKind::Root => palette.success,
                SnapKind::Intersection => palette.warning,
                SnapKind::YIntercept | SnapKind::Extremum => palette.accent_alt,
            };
            let kind_label = match sp.kind {
                SnapKind::Root => s.plot_snap_root,
                SnapKind::Intersection => s.plot_snap_intersection,
                SnapKind::YIntercept => s.plot_snap_yintercept,
                SnapKind::Extremum => s.plot_snap_extremum,
            };

            overlay.circle_filled(screen_pos, 11.0, Palette::alpha(accent, 55));
            overlay.circle_filled(screen_pos, 5.5, accent);
            overlay.circle_stroke(screen_pos, 5.5, Stroke::new(1.6, palette.bg_base));

            let coords = format!("({}, {})", fmt_coord(sp.x), fmt_coord(sp.y));
            let title = format!("{kind_label}  {coords}");
            paint_coord_badge(&overlay, screen_pos, canvas_rect, &title, Some(s.plot_click_copy_coords), accent, palette);

            if plot_clicked {
                action = PlotAction::Copy(coords);
            }
        } else if let Some((x, y, screen_pos, col)) = hovered_curve_pt {
            overlay.circle_filled(screen_pos, 8.0, Palette::alpha(col, 50));
            overlay.circle_filled(screen_pos, 4.2, col);
            let coords = format!("({}, {})", fmt_coord(x), fmt_coord(y));
            paint_coord_badge(&overlay, screen_pos, canvas_rect, &coords, None, col, palette);

            if plot_clicked {
                action = PlotAction::Copy(coords);
            }
        }

        action
    }
}

fn fmt_coord(v: f64) -> String {
    if v.abs() < 1e-9 {
        return "0".to_owned();
    }
    let s = format!("{v:.4}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    }
}

fn paint_coord_badge(
    painter: &egui::Painter,
    anchor: Pos2,
    bounds: Rect,
    main: &str,
    sub: Option<&str>,
    accent: Color32,
    palette: &Palette,
) {
    let g_main = painter.layout_no_wrap(main.to_owned(), FontId::monospace(12.5), palette.text_primary);
    let g_sub = sub.map(|t| painter.layout_no_wrap(t.to_owned(), FontId::proportional(10.5), palette.text_muted));

    let pad = Vec2::new(10.0, 6.0);
    let w = g_main.size().x.max(g_sub.as_ref().map_or(0.0, |g| g.size().x)) + pad.x * 2.0;
    let h = g_main.size().y + g_sub.as_ref().map_or(0.0, |g| g.size().y + 2.0) + pad.y * 2.0;

    let mut pos = Pos2::new(anchor.x + 14.0, anchor.y - h - 10.0);
    if pos.x + w > bounds.right() - 8.0 {
        pos.x = anchor.x - w - 14.0;
    }
    if pos.y < bounds.top() + 8.0 {
        pos.y = anchor.y + 14.0;
    }

    let rect = Rect::from_min_size(pos, Vec2::new(w, h));
    painter.rect(
        rect,
        CornerRadius::same(8),
        Palette::alpha(palette.bg_elevated, 240),
        Stroke::new(1.0, Palette::alpha(accent, 175)),
        egui::StrokeKind::Inside,
    );
    painter.galley(Pos2::new(rect.left() + pad.x, rect.top() + pad.y), g_main.clone(), palette.text_primary);
    if let Some(gs) = g_sub {
        painter.galley(
            Pos2::new(rect.left() + pad.x, rect.top() + pad.y + g_main.size().y + 2.0),
            gs,
            palette.text_muted,
        );
    }
}
