//! Programmer (interactive 64/32/16/8-bit grid + DEC/HEX/OCT/BIN)
//! and Engineering unit converter view.

use crate::i18n::{Lang, Strings, format_error, tr};
use crate::style::{self, Palette};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Margin, Pos2, Rect, ScrollArea,
    Sense, Stroke, TextEdit, Ui, Vec2,
};
use lumina_core::{BitRegister, BitWidth, CalcError, Env, FormatOptions, Radix, eval_str};

pub enum ProgAction {
    None,
    Copy(String),
    SendToNotebook(String),
}

struct UnitCategory {
    ru: &'static str,
    en: &'static str,
    units: &'static [&'static str],
    default_from: usize,
    default_to: usize,
    default_val: &'static str,
}

const CATEGORIES: &[UnitCategory] = &[
    UnitCategory {
        ru: "Скорость",
        en: "Speed",
        units: &["km/h", "m/s", "mph", "kn"],
        default_from: 0,
        default_to: 1,
        default_val: "120",
    },
    UnitCategory {
        ru: "Длина",
        en: "Length",
        units: &["m", "km", "cm", "mm", "mi", "ft", "inch", "yd", "nmi"],
        default_from: 1,
        default_to: 4,
        default_val: "10",
    },
    UnitCategory {
        ru: "Масса",
        en: "Mass",
        units: &["kg", "g", "mg", "tonne", "lb", "oz"],
        default_from: 0,
        default_to: 4,
        default_val: "75",
    },
    UnitCategory {
        ru: "Температура",
        en: "Temp",
        units: &["°C", "°F", "K"],
        default_from: 0,
        default_to: 1,
        default_val: "22",
    },
    UnitCategory {
        ru: "Данные",
        en: "Data",
        units: &["bit", "B", "KB", "MB", "GB", "TB", "KiB", "MiB", "GiB", "TiB"],
        default_from: 8,
        default_to: 3,
        default_val: "1",
    },
    UnitCategory {
        ru: "Энергия",
        en: "Energy",
        units: &["J", "kJ", "Wh", "kWh", "cal", "kcal", "eV"],
        default_from: 3,
        default_to: 0,
        default_val: "1",
    },
    UnitCategory {
        ru: "Давление",
        en: "Pressure",
        units: &["Pa", "kPa", "MPa", "bar", "atm", "psi", "mmHg"],
        default_from: 4,
        default_to: 3,
        default_val: "1",
    },
    UnitCategory {
        ru: "Время",
        en: "Time",
        units: &["s", "ms", "µs", "min", "h", "d", "wk", "yr"],
        default_from: 4,
        default_to: 0,
        default_val: "2.5",
    },
];

struct ConvCache {
    cat: usize,
    from: usize,
    to: usize,
    val: String,
    query: String,
    result: Result<(String, String), CalcError>,
}

pub struct ProgrammerView {
    pub reg: BitRegister,
    dec_buf: String,
    hex_buf: String,
    oct_buf: String,
    bin_buf: String,
    expr_buf: String,
    expr_err: Option<CalcError>,
    /// Drag-paint target state when dragging pointer across bit tiles.
    bit_paint_state: Option<bool>,

    // Engineering unit converter state
    conv_cat: usize,
    conv_val: String,
    conv_from: usize,
    conv_to: usize,
    conv_cache: Option<ConvCache>,
}

impl ProgrammerView {
    pub fn new(raw: u64, width_bits: u8, signed: bool) -> Self {
        let reg = BitRegister::new(raw, BitWidth::from_bits(width_bits), signed);
        let mut view = Self {
            reg,
            dec_buf: String::new(),
            hex_buf: String::new(),
            oct_buf: String::new(),
            bin_buf: String::new(),
            expr_buf: "(0xFF << 4) | 0x0A".into(),
            expr_err: None,
            bit_paint_state: None,
            conv_cat: 0,
            conv_val: CATEGORIES[0].default_val.to_owned(),
            conv_from: CATEGORIES[0].default_from,
            conv_to: CATEGORIES[0].default_to,
            conv_cache: None,
        };
        view.sync_buffers(None);
        view
    }

    fn format_copy_value(&self, radix: Radix) -> String {
        format!("{}{}", radix.prefix(), self.reg.format_radix(radix, false))
    }

    fn ensure_conv_cache(&mut self, from_unit: &str, to_unit: &str) {
        let trimmed = self.conv_val.trim();
        if trimmed.is_empty() {
            self.conv_cache = None;
            return;
        }
        let hit = self.conv_cache.as_ref().is_some_and(|c| {
            c.cat == self.conv_cat
                && c.from == self.conv_from
                && c.to == self.conv_to
                && c.val == trimmed
        });
        if hit {
            return;
        }
        let query = format!("{trimmed} {from_unit} in {to_unit}");
        let result = eval_str(&query, &Env::default()).map(|val| {
            let shown = val.display(&FormatOptions { precision: 8, grouping: Some(' ') });
            let plain = val.plain(8);
            (shown, plain)
        });
        self.conv_cache = Some(ConvCache {
            cat: self.conv_cat,
            from: self.conv_from,
            to: self.conv_to,
            val: trimmed.to_owned(),
            query,
            result,
        });
    }

    fn sync_buffers(&mut self, skip: Option<Radix>) {
        if skip != Some(Radix::Dec) {
            self.dec_buf = self.reg.format_dec(true);
        }
        if skip != Some(Radix::Hex) {
            self.hex_buf = self.reg.format_hex(true);
        }
        if skip != Some(Radix::Oct) {
            self.oct_buf = self.reg.format_oct(true);
        }
        if skip != Some(Radix::Bin) {
            self.bin_buf = self.reg.format_bin(true);
        }
    }

    pub fn show(&mut self, ui: &mut Ui, lang: Lang, palette: &Palette) -> (ProgAction, bool) {
        let s = tr(lang);
        let mut action = ProgAction::None;
        let mut dirty = false;

        // Release drag-paint when pointer button is released anywhere.
        if !ui.ctx().input(|i| i.pointer.primary_down()) {
            self.bit_paint_state = None;
        }

        ScrollArea::vertical()
            .id_salt("prog_view_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                Frame::new()
                    .inner_margin(Margin::symmetric(16, 14))
                    .show(ui, |ui| {
                        if self.show_radix_cards(ui, palette, &mut action) {
                            dirty = true;
                        }
                        ui.add_space(10.0);
                        if self.show_toolbar_and_expr(ui, lang, s, palette) {
                            dirty = true;
                        }
                        ui.add_space(12.0);
                        if self.show_bit_grid(ui, s, palette) {
                            dirty = true;
                        }
                        ui.add_space(12.0);
                        self.show_unit_converter(ui, lang, s, palette, &mut action);
                    });
            });

        (action, dirty)
    }

    fn show_radix_cards(
        &mut self,
        ui: &mut Ui,
        palette: &Palette,
        action: &mut ProgAction,
    ) -> bool {
        let mut dirty = false;
        Frame::new()
            .fill(palette.bg_mantle)
            .stroke(Stroke::new(1.0, palette.border_subtle))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                let radices = [
                    (Radix::Hex, "HEX", palette.syn_number),
                    (Radix::Dec, "DEC", palette.accent),
                    (Radix::Oct, "OCT", palette.syn_unit),
                    (Radix::Bin, "BIN", palette.syn_function),
                ];

                for (idx, (radix, tag, badge_col)) in radices.into_iter().enumerate() {
                    ui.horizontal(|ui| {
                        let badge = style::paint_pill(
                            ui,
                            tag,
                            Palette::alpha(badge_col, 32),
                            badge_col,
                            Stroke::new(1.0, Palette::alpha(badge_col, 110)),
                            11.5,
                        )
                        .on_hover_cursor(CursorIcon::PointingHand);
                        if badge.clicked() {
                            *action = ProgAction::Copy(self.format_copy_value(radix));
                        }

                        ui.add_space(6.0);
                        let buf = match radix {
                            Radix::Dec => &mut self.dec_buf,
                            Radix::Hex => &mut self.hex_buf,
                            Radix::Oct => &mut self.oct_buf,
                            Radix::Bin => &mut self.bin_buf,
                        };
                        let input_w = (ui.available_width() - 40.0).max(120.0);
                        let resp = ui.add(
                            TextEdit::singleline(buf)
                                .id(Id::unique(("prog_base", idx)))
                                .font(FontId::monospace(if radix == Radix::Bin { 13.5 } else { 16.0 }))
                                .frame(Frame::NONE)
                                .desired_width(input_w),
                        );
                        if resp.changed() {
                            let snapshot = buf.clone();
                            if self.reg.parse_radix(&snapshot, radix) {
                                self.sync_buffers(Some(radix));
                                dirty = true;
                            }
                        }
                        if resp.lost_focus() {
                            self.sync_buffers(None);
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let copy_btn = ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("⧉")
                                            .size(13.0)
                                            .color(palette.text_secondary),
                                    )
                                    .frame(false),
                                )
                                .on_hover_cursor(CursorIcon::PointingHand);
                            if copy_btn.clicked() {
                                *action = ProgAction::Copy(self.format_copy_value(radix));
                            }
                        });
                    });
                    if idx + 1 < 4 {
                        ui.add_space(2.0);
                        style::hairline_h(ui, palette.border_subtle);
                        ui.add_space(2.0);
                    }
                }
            });
        dirty
    }

    fn show_toolbar_and_expr(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        s: &Strings,
        palette: &Palette,
    ) -> bool {
        let mut dirty = false;

        ui.horizontal_wrapped(|ui| {
            for w in BitWidth::ALL {
                let active = self.reg.width == w;
                let resp = style::paint_pill(
                    ui,
                    w.label(),
                    if active { Palette::alpha(palette.accent, 42) } else { palette.bg_surface },
                    if active { palette.accent } else { palette.text_secondary },
                    Stroke::new(1.0, if active { palette.accent } else { palette.border_subtle }),
                    11.5,
                );
                if resp.clicked() && !active {
                    self.reg.set_width(w);
                    self.sync_buffers(None);
                    dirty = true;
                }
            }

            ui.add_space(6.0);
            let sign_label = if self.reg.signed { s.prog_signed } else { s.prog_unsigned };
            let sign_resp = style::paint_pill(
                ui,
                sign_label,
                palette.bg_surface,
                palette.accent_alt,
                Stroke::new(1.0, palette.border_subtle),
                11.5,
            );
            if sign_resp.clicked() {
                self.reg.signed = !self.reg.signed;
                self.sync_buffers(None);
                dirty = true;
            }

            ui.add_space(8.0);
            type BitOp = fn(&mut BitRegister);
            let ops: &[(&str, BitOp)] = &[
                ("~ NOT", BitRegister::bit_not),
                ("± NEG", BitRegister::negate),
                ("<< 1", |r| r.shl(1)),
                (">> 1", |r| r.shr(1)),
                ("ROL", |r| r.rol(1)),
                ("ROR", |r| r.ror(1)),
                ("CLR", BitRegister::clear),
            ];
            for &(lbl, op) in ops {
                if ui.button(egui::RichText::new(lbl).monospace().size(12.0)).clicked() {
                    op(&mut self.reg);
                    self.sync_buffers(None);
                    dirty = true;
                }
            }
        });

        ui.add_space(8.0);

        Frame::new()
            .fill(palette.bg_surface)
            .stroke(Stroke::new(
                1.0,
                if self.expr_err.is_some() {
                    Palette::alpha(palette.error, 150)
                } else {
                    palette.border_subtle
                },
            ))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(s.prog_expr_label)
                            .size(12.5)
                            .color(palette.text_secondary),
                    );
                    let err_span_opt = self.expr_err.as_ref().map(|e| e.span.clone());
                    let mut layouter =
                        style::singleline_layouter(false, err_span_opt.as_ref(), 13.5, palette);
                    let input_w = (ui.available_width() - 96.0).max(120.0);
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.expr_buf)
                            .id(Id::unique("prog_expr_input"))
                            .frame(Frame::NONE)
                            .desired_width(input_w)
                            .hint_text(s.prog_expr_hint)
                            .layouter(&mut layouter),
                    );
                    let enter =
                        resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let clicked = ui
                        .button(egui::RichText::new(s.prog_apply).size(12.0))
                        .clicked();
                    if resp.changed() {
                        self.expr_err = None;
                    }
                    if enter || clicked {
                        match self.reg.eval_and_apply(&self.expr_buf) {
                            Ok(_) => {
                                self.expr_err = None;
                                self.sync_buffers(None);
                                dirty = true;
                            }
                            Err(e) => {
                                self.expr_err = Some(e);
                            }
                        }
                    }
                });
                if let Some(err) = &self.expr_err {
                    ui.label(
                        egui::RichText::new(format_error(lang, &err.kind))
                            .size(11.5)
                            .color(palette.error),
                    );
                }
            });

        dirty
    }

    fn show_bit_grid(&mut self, ui: &mut Ui, s: &Strings, palette: &Palette) -> bool {
        let mut dirty = false;

        Frame::new()
            .fill(palette.bg_mantle)
            .stroke(Stroke::new(1.0, palette.border_subtle))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(s.prog_bits_grid)
                            .size(13.0)
                            .strong()
                            .color(palette.text_secondary),
                    );
                    ui.label(
                        egui::RichText::new(format!("— {}", s.prog_bits_hint))
                            .size(11.5)
                            .color(palette.text_muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let bytes = self.reg.masked().to_be_bytes();
                        let byte_count = (self.reg.width.bits() / 8) as usize;
                        let slice = &bytes[8 - byte_count..];
                        let hex_bytes: String = slice
                            .iter()
                            .map(|b| format!("{b:02X}"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        ui.label(
                            egui::RichText::new(format!(
                                "{} {}   •   {} {}",
                                s.prog_set_bits,
                                self.reg.popcount(),
                                s.prog_bytes,
                                hex_bytes
                            ))
                            .monospace()
                            .size(11.5)
                            .color(palette.text_muted),
                        );
                    });
                });

                ui.add_space(10.0);

                // Render 2 rows of 32 bits (8 nibbles per row) for 64-bit,
                // or 1 row of 32 bits for 32/16/8-bit.
                let row_tops: &[u32] = if self.reg.width == BitWidth::Bits64 {
                    &[63, 31]
                } else {
                    &[31]
                };

                let avail_w = ui.available_width();
                let nibble_gap = 10.0f32;
                let bit_gap = 3.0f32;
                let nibbles = 8usize;
                let nibble_w =
                    ((avail_w - nibble_gap * (nibbles as f32 - 1.0)) / nibbles as f32).max(56.0);
                let bit_w = ((nibble_w - bit_gap * 3.0) / 4.0).clamp(12.0, 26.0);
                let bit_h = 28.0f32;

                for &hi in row_tops {
                    let row_h = bit_h + 16.0;
                    let (row_rect, _) =
                        ui.allocate_exact_size(Vec2::new(avail_w, row_h), Sense::hover());
                    let total_row_w = (bit_w * 4.0 + bit_gap * 3.0) * 8.0 + nibble_gap * 7.0;
                    let start_x = row_rect.left() + ((avail_w - total_row_w) * 0.5).max(0.0);

                    for nib in 0..8u32 {
                        let nib_hi = hi - nib * 4;
                        let nib_lo = nib_hi - 3;
                        let nib_x = start_x
                            + nib as f32 * (bit_w * 4.0 + bit_gap * 3.0 + nibble_gap);

                        let nib_active = nib_lo < self.reg.width.bits();
                        let idx_col = if nib_active {
                            palette.text_muted
                        } else {
                            Palette::alpha(palette.text_muted, 80)
                        };
                        ui.painter().text(
                            Pos2::new(nib_x, row_rect.top() + 5.0),
                            Align2::LEFT_CENTER,
                            nib_hi.to_string(),
                            FontId::monospace(10.0),
                            idx_col,
                        );
                        ui.painter().text(
                            Pos2::new(nib_x + bit_w * 4.0 + bit_gap * 3.0, row_rect.top() + 5.0),
                            Align2::RIGHT_CENTER,
                            nib_lo.to_string(),
                            FontId::monospace(10.0),
                            idx_col,
                        );

                        for b_in_nib in 0..4u32 {
                            let bit = nib_hi - b_in_nib;
                            let enabled = bit < self.reg.width.bits();
                            let bx = nib_x + b_in_nib as f32 * (bit_w + bit_gap);
                            let rect = Rect::from_min_size(
                                Pos2::new(bx, row_rect.top() + 14.0),
                                Vec2::new(bit_w, bit_h),
                            );

                            let resp = ui.interact(
                                rect,
                                Id::unique(("bit_tile", bit)),
                                if enabled { Sense::click_and_drag() } else { Sense::hover() },
                            );

                            if enabled {
                                if resp.is_pointer_button_down_on()
                                    && ui.ctx().input(|i| i.pointer.primary_pressed())
                                {
                                    let new_state = !self.reg.get_bit(bit);
                                    self.bit_paint_state = Some(new_state);
                                    self.reg.toggle_bit(bit);
                                    self.sync_buffers(None);
                                    dirty = true;
                                } else if let Some(target) = self.bit_paint_state
                                    && resp.hovered()
                                    && self.reg.get_bit(bit) != target
                                {
                                    self.reg.toggle_bit(bit);
                                    self.sync_buffers(None);
                                    dirty = true;
                                }
                            }

                            let is_one = enabled && self.reg.get_bit(bit);
                            let on_t = ui.ctx().animate_bool_with_time(
                                Id::unique(("bit_anim", bit)),
                                is_one,
                                0.11,
                            );
                            let hov_t = if enabled {
                                ui.ctx().animate_bool_with_time(
                                    Id::unique(("bit_hov", bit)),
                                    resp.hovered(),
                                    0.09,
                                )
                            } else {
                                0.0
                            };

                            let base_fill = if enabled {
                                Palette::lerp(palette.bg_surface, palette.accent, on_t)
                            } else {
                                Palette::alpha(palette.bg_crust, 120)
                            };
                            let fill = Palette::lerp(base_fill, Color32::WHITE, hov_t * 0.08);
                            let stroke = if enabled {
                                Stroke::new(
                                    1.0,
                                    Palette::lerp(
                                        palette.border_subtle,
                                        palette.accent,
                                        on_t.max(hov_t),
                                    ),
                                )
                            } else {
                                Stroke::new(1.0, Palette::alpha(palette.border_subtle, 60))
                            };

                            ui.painter().rect(
                                rect,
                                CornerRadius::same(5),
                                fill,
                                stroke,
                                egui::StrokeKind::Inside,
                            );

                            let fg = if !enabled {
                                Palette::alpha(palette.text_muted, 70)
                            } else {
                                Palette::lerp(palette.text_muted, palette.accent_fg, on_t)
                            };
                            ui.painter().text(
                                rect.center(),
                                Align2::CENTER_CENTER,
                                if is_one { "1" } else { "0" },
                                FontId::monospace(13.0),
                                fg,
                            );

                            if enabled && resp.hovered() {
                                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                            }
                        }
                    }
                    ui.add_space(6.0);
                }
            });

        dirty
    }

    fn show_unit_converter(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        s: &Strings,
        palette: &Palette,
        action: &mut ProgAction,
    ) {
        Frame::new()
            .fill(palette.bg_mantle)
            .stroke(Stroke::new(1.0, palette.border_subtle))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(s.prog_converter_title)
                        .size(13.0)
                        .strong()
                        .color(palette.text_secondary),
                );
                ui.add_space(8.0);

                ui.horizontal_wrapped(|ui| {
                    for (idx, cat) in CATEGORIES.iter().enumerate() {
                        let active = self.conv_cat == idx;
                        let label = if lang == Lang::Ru { cat.ru } else { cat.en };
                        let resp = style::paint_pill(
                            ui,
                            label,
                            if active {
                                Palette::alpha(palette.syn_unit, 38)
                            } else {
                                palette.bg_surface
                            },
                            if active { palette.syn_unit } else { palette.text_secondary },
                            Stroke::new(
                                1.0,
                                if active { palette.syn_unit } else { palette.border_subtle },
                            ),
                            11.5,
                        );
                        if resp.clicked() && !active {
                            self.conv_cat = idx;
                            self.conv_from = cat.default_from;
                            self.conv_to = cat.default_to;
                            self.conv_val = cat.default_val.to_owned();
                        }
                    }
                });

                ui.add_space(10.0);

                let cat_idx = self.conv_cat.min(CATEGORIES.len() - 1);
                let cat = &CATEGORIES[cat_idx];
                let from_unit = cat.units[self.conv_from.min(cat.units.len() - 1)];
                let to_unit = cat.units[self.conv_to.min(cat.units.len() - 1)];

                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(s.prog_conv_value)
                            .size(12.0)
                            .color(palette.text_muted),
                    );
                    ui.add(
                        TextEdit::singleline(&mut self.conv_val)
                            .font(FontId::monospace(14.0))
                            .desired_width(100.0),
                    );

                    egui::ComboBox::from_id_salt("conv_from_combo")
                        .selected_text(from_unit)
                        .width(82.0)
                        .show_ui(ui, |ui| {
                            for (i, u) in cat.units.iter().enumerate() {
                                ui.selectable_value(&mut self.conv_from, i, *u);
                            }
                        });

                    if ui.button(egui::RichText::new("⇄").size(14.0)).clicked() {
                        std::mem::swap(&mut self.conv_from, &mut self.conv_to);
                    }

                    egui::ComboBox::from_id_salt("conv_to_combo")
                        .selected_text(to_unit)
                        .width(82.0)
                        .show_ui(ui, |ui| {
                            for (i, u) in cat.units.iter().enumerate() {
                                ui.selectable_value(&mut self.conv_to, i, *u);
                            }
                        });

                    let from_u = cat.units[self.conv_from.min(cat.units.len() - 1)];
                    let to_u = cat.units[self.conv_to.min(cat.units.len() - 1)];
                    self.ensure_conv_cache(from_u, to_u);

                    if let Some(cache) = &self.conv_cache {
                        ui.add_space(8.0);
                        match &cache.result {
                            Ok((shown, plain)) => {
                                let pill = style::paint_pill(
                                    ui,
                                    &format!("= {shown}"),
                                    Palette::alpha(palette.success, 28),
                                    palette.success,
                                    Stroke::new(1.0, Palette::alpha(palette.success, 120)),
                                    13.5,
                                )
                                .on_hover_cursor(CursorIcon::PointingHand);
                                if pill.clicked() {
                                    *action = ProgAction::Copy(plain.clone());
                                }

                                if ui
                                    .button(egui::RichText::new(s.prog_send_notebook).size(12.0))
                                    .clicked()
                                {
                                    *action = ProgAction::SendToNotebook(cache.query.clone());
                                }
                            }
                            Err(err) => {
                                ui.label(
                                    egui::RichText::new(format_error(lang, &err.kind))
                                        .size(12.0)
                                        .color(palette.error),
                                );
                            }
                        }
                    }
                });
            });
    }
}
