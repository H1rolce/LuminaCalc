use crate::i18n::{Lang, Strings, format_error, tr};
use crate::style::{self, Palette};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Event, FontId, Frame, Id, Key, Margin, Pos2, Rect,
    ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2,
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
    pub active_radix: Radix,
    dec_buf: String,
    hex_buf: String,
    oct_buf: String,
    bin_buf: String,
    expr_buf: String,
    expr_err: Option<CalcError>,
    bit_paint_state: Option<bool>,

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
            active_radix: Radix::Hex,
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

    fn radix_input_id(radix: Radix) -> Id {
        let idx: usize = match radix {
            Radix::Hex => 0,
            Radix::Dec => 1,
            Radix::Oct => 2,
            Radix::Bin => 3,
        };
        Id::unique(("prog_base", idx))
    }

    fn expr_input_id() -> Id {
        Id::unique("prog_expr_input")
    }

    fn conv_input_id() -> Id {
        Id::unique("prog_conv_input")
    }

    fn focus_expr_at_end(&self, ctx: &egui::Context) {
        let id = Self::expr_input_id();
        ctx.memory_mut(|m| m.request_focus(id));
        let char_len = self.expr_buf.chars().count();
        if let Some(mut state) = egui::text_edit::TextEditState::load(ctx, id) {
            let c = egui::text::CCursor::new(char_len);
            state.cursor.set_char_range(Some(egui::text::CCursorRange::one(c)));
            state.store(ctx, id);
        }
    }

    fn clear_all(&mut self) {
        self.reg.clear();
        self.expr_buf.clear();
        self.expr_err = None;
        self.sync_buffers(None);
    }

    fn seed_expr_with_operator(&mut self, ctx: &egui::Context, radix: Radix, op: char) {
        let val_str = self.reg.format_radix(radix, false);
        self.expr_buf = match op {
            '~' | '(' => op.to_string(),
            '<' | '>' => format!("{val_str} {op}{op} "),
            _ => format!("{val_str} {op} "),
        };
        self.expr_err = None;
        self.focus_expr_at_end(ctx);
    }

    fn handle_keyboard(&mut self, ui: &Ui) -> bool {
        let ctx = ui.ctx();
        let focused_radix = [Radix::Hex, Radix::Dec, Radix::Oct, Radix::Bin]
            .into_iter()
            .find(|&r| ctx.memory(|m| m.has_focus(Self::radix_input_id(r))));
        let expr_focused = ctx.memory(|m| m.has_focus(Self::expr_input_id()));
        let conv_focused = ctx.memory(|m| m.has_focus(Self::conv_input_id()));

        if let Some(r) = focused_radix {
            self.active_radix = r;
        }
        if conv_focused {
            return false;
        }

        let mut switched_radix: Option<Radix> = None;
        let mut esc_pressed = false;
        ctx.input(|i| {
            for ev in &i.events {
                if let Event::Key { key, pressed: true, modifiers, .. } = ev {
                    if let Some(r) = map_radix_switch_key(*key, *modifiers) {
                        switched_radix = Some(r);
                    } else if *key == Key::Escape
                        && !modifiers.ctrl
                        && !modifiers.command
                        && !modifiers.alt
                    {
                        esc_pressed = true;
                    }
                }
            }
        });

        let mut dirty = false;
        if let Some(new_radix) = switched_radix {
            self.active_radix = new_radix;
            if !expr_focused {
                ctx.memory_mut(|m| m.request_focus(Self::radix_input_id(new_radix)));
            }
        }
        if esc_pressed {
            self.clear_all();
            return true;
        }

        if focused_radix.is_some() || expr_focused {
            let effective_radix = focused_radix.unwrap_or(self.active_radix);
            let in_expr = expr_focused;
            let signed = self.reg.signed;
            let mut operator_handoff: Option<char> = None;

            ctx.input_mut(|i| {
                let old_events = std::mem::take(&mut i.events);
                let mut kept = Vec::with_capacity(old_events.len());
                let mut emitted_hex_from_key = false;

                for ev in old_events {
                    match ev {
                        Event::Key { key, physical_key, pressed: true, repeat, modifiers } => {
                            if map_radix_switch_key(key, modifiers).is_some() {
                                continue;
                            }
                            if effective_radix == Radix::Hex
                                && let Some(hex_ch) = map_hex_letter_key(key, modifiers)
                            {
                                kept.push(Event::Text(hex_ch.to_string()));
                                emitted_hex_from_key = true;
                                continue;
                            }
                            kept.push(Event::Key {
                                key,
                                physical_key,
                                pressed: true,
                                repeat,
                                modifiers,
                            });
                        }
                        Event::Text(text) => {
                            let mut out = String::new();
                            for ch in text.chars() {
                                if emitted_hex_from_key && ch.is_alphabetic() {
                                    continue;
                                }
                                if in_expr {
                                    if let Some(c) = filter_prog_char(ch, effective_radix, true, false) {
                                        out.push(c);
                                    }
                                } else if is_allowed_prog_operator(ch)
                                    && !(ch == '-' && effective_radix == Radix::Dec && signed)
                                {
                                    operator_handoff = Some(ch);
                                } else if let Some(c) = filter_prog_char(
                                    ch,
                                    effective_radix,
                                    false,
                                    effective_radix == Radix::Dec && signed,
                                ) {
                                    out.push(c);
                                }
                            }
                            if !out.is_empty() {
                                kept.push(Event::Text(out));
                            }
                        }
                        Event::Paste(raw) => {
                            let cleaned = if in_expr {
                                sanitize_prog_expr_paste(&raw, effective_radix)
                            } else {
                                sanitize_radix_paste(&raw, effective_radix, signed)
                            };
                            if !cleaned.is_empty() {
                                kept.push(Event::Paste(cleaned));
                            }
                        }
                        other => kept.push(other),
                    }
                }
                i.events = kept;
            });

            if let Some(op) = operator_handoff {
                self.seed_expr_with_operator(ctx, effective_radix, op);
            }
            return dirty;
        }

        let radix = self.active_radix;
        let signed = self.reg.signed;
        let mut typed = String::new();
        let mut pasted: Vec<String> = Vec::new();
        let mut back = false;
        let mut do_copy = false;
        let mut operator_handoff: Option<char> = None;

        ctx.input(|i| {
            let ctrl_held = i.modifiers.ctrl || i.modifiers.command || i.modifiers.alt;
            let mut emitted_hex_from_key = false;
            for ev in &i.events {
                match ev {
                    Event::Key { key, pressed: true, modifiers, .. } => {
                        if map_radix_switch_key(*key, *modifiers).is_some() {
                            continue;
                        }
                        if radix == Radix::Hex
                            && let Some(hex_ch) = map_hex_letter_key(*key, *modifiers)
                        {
                            typed.push(hex_ch);
                            emitted_hex_from_key = true;
                            continue;
                        }
                        if !modifiers.ctrl && !modifiers.command && !modifiers.alt && *key == Key::Backspace {
                            back = true;
                        }
                    }
                    Event::Text(text) if !ctrl_held => {
                        for ch in text.chars() {
                            if emitted_hex_from_key && ch.is_alphabetic() {
                                continue;
                            }
                            if is_allowed_prog_operator(ch)
                                && !(ch == '-' && radix == Radix::Dec && signed)
                            {
                                operator_handoff = Some(ch);
                            } else if let Some(c) =
                                filter_prog_char(ch, radix, false, radix == Radix::Dec && signed)
                                && c != ' '
                            {
                                typed.push(c);
                            }
                        }
                    }
                    Event::Paste(raw) => {
                        let cleaned = sanitize_radix_paste(raw, radix, signed);
                        if !cleaned.is_empty() {
                            pasted.push(cleaned);
                        }
                    }
                    Event::Copy => do_copy = true,
                    _ => {}
                }
            }
        });

        if do_copy {
            ctx.copy_text(self.format_copy_value(radix));
        }
        if let Some(op) = operator_handoff {
            self.seed_expr_with_operator(ctx, radix, op);
            return dirty;
        }
        for s in pasted {
            if self.reg.parse_radix(&s, radix) {
                self.sync_buffers(None);
                dirty = true;
            }
        }
        if !typed.is_empty() {
            let cur = self.reg.format_radix(radix, false);
            let combined = if cur == "0" && typed != "-" {
                typed
            } else {
                format!("{cur}{typed}")
            };
            if self.reg.parse_radix(&combined, radix) {
                self.sync_buffers(None);
                dirty = true;
            }
        }
        if back {
            let mut cur = self.reg.format_radix(radix, false);
            cur.pop();
            if cur.is_empty() || cur == "-" {
                self.reg.clear();
            } else {
                let _ = self.reg.parse_radix(&cur, radix);
            }
            self.sync_buffers(None);
            dirty = true;
        }
        dirty
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

        if !ui.ctx().input(|i| i.pointer.primary_down()) {
            self.bit_paint_state = None;
        }
        if self.handle_keyboard(ui) {
            dirty = true;
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
                    (Radix::Hex, "HEX", palette.syn_number, "F5 — HEX (0–9, A–F)"),
                    (Radix::Dec, "DEC", palette.accent, "F6 — DEC (0–9)"),
                    (Radix::Oct, "OCT", palette.syn_unit, "F7 — OCT (0–7)"),
                    (Radix::Bin, "BIN", palette.syn_function, "F8 — BIN (0–1)"),
                ];

                for (idx, (radix, tag, badge_col, tip)) in radices.into_iter().enumerate() {
                    let is_active = self.active_radix == radix;
                    ui.horizontal(|ui| {
                        let badge = style::paint_pill(
                            ui,
                            tag,
                            Palette::alpha(badge_col, if is_active { 62 } else { 28 }),
                            badge_col,
                            Stroke::new(
                                if is_active { 1.4 } else { 1.0 },
                                Palette::alpha(badge_col, if is_active { 210 } else { 95 }),
                            ),
                            11.5,
                        )
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text(tip);
                        if badge.clicked() {
                            self.active_radix = radix;
                            ui.ctx().memory_mut(|m| m.request_focus(Self::radix_input_id(radix)));
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
                                .id(Self::radix_input_id(radix))
                                .font(FontId::monospace(if radix == Radix::Bin { 13.5 } else { 16.0 }))
                                .frame(Frame::NONE)
                                .desired_width(input_w),
                        );
                        if resp.gained_focus() {
                            self.active_radix = radix;
                        }
                        if resp.changed() {
                            self.active_radix = radix;
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
            let ops: &[(&str, &str, BitOp)] = &[
                ("~ NOT", "~", BitRegister::bit_not),
                ("± NEG", "-", BitRegister::negate),
                ("<< 1", "<<", |r| r.shl(1)),
                (">> 1", ">>", |r| r.shr(1)),
                ("ROL", "", |r| r.rol(1)),
                ("ROR", "", |r| r.ror(1)),
                ("CLR", "Esc", BitRegister::clear),
            ];
            for &(lbl, tip, op) in ops {
                let mut btn = ui.button(egui::RichText::new(lbl).monospace().size(12.0));
                if !tip.is_empty() {
                    btn = btn.on_hover_text(tip);
                }
                if btn.clicked() {
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
                    let err_ctx = self
                        .expr_err
                        .as_ref()
                        .map(|e| (self.expr_buf.clone(), e.span.clone()));
                    let mut layouter = style::singleline_layouter(
                        false,
                        err_ctx.as_ref().map(|(src, span)| (src.as_str(), span)),
                        13.5,
                        palette,
                    );
                    let input_w = (ui.available_width() - 96.0).max(120.0);
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.expr_buf)
                            .id(Self::expr_input_id())
                            .frame(Frame::NONE)
                            .desired_width(input_w)
                            .hint_text(s.prog_expr_hint)
                            .layouter(&mut layouter),
                    );
                    let enter =
                        resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let clicked = ui
                        .button(egui::RichText::new(s.prog_apply).size(12.0))
                        .on_hover_text("Enter")
                        .clicked();
                    if resp.changed() {
                        self.expr_err = None;
                    }
                    if enter || clicked {
                        let normalized = normalize_prog_expr(&self.expr_buf, self.active_radix);
                        match self.reg.eval_and_apply(&normalized) {
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
                            .id(Self::conv_input_id())
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

fn map_radix_switch_key(key: Key, modifiers: egui::Modifiers) -> Option<Radix> {
    if modifiers.ctrl || modifiers.command || modifiers.alt {
        return None;
    }
    match key {
        Key::F5 => Some(Radix::Hex),
        Key::F6 => Some(Radix::Dec),
        Key::F7 => Some(Radix::Oct),
        Key::F8 => Some(Radix::Bin),
        _ => None,
    }
}

fn map_hex_letter_key(key: Key, modifiers: egui::Modifiers) -> Option<char> {
    if modifiers.ctrl || modifiers.command || modifiers.alt {
        return None;
    }
    match key {
        Key::A => Some('A'),
        Key::B => Some('B'),
        Key::C => Some('C'),
        Key::D => Some('D'),
        Key::E => Some('E'),
        Key::F => Some('F'),
        _ => None,
    }
}

fn is_valid_radix_digit(ch: char, radix: Radix) -> bool {
    match radix {
        Radix::Hex => ch.is_ascii_hexdigit(),
        Radix::Dec => ch.is_ascii_digit(),
        Radix::Oct => matches!(ch, '0'..='7'),
        Radix::Bin => matches!(ch, '0' | '1'),
    }
}

fn is_allowed_prog_operator(ch: char) -> bool {
    matches!(ch, '+' | '-' | '*' | '/' | '%' | '(' | ')' | '&' | '|' | '^' | '~' | '<' | '>')
}

fn filter_prog_char(
    ch: char,
    radix: Radix,
    allow_operators: bool,
    allow_neg_sign: bool,
) -> Option<char> {
    if is_valid_radix_digit(ch, radix) {
        Some(ch.to_ascii_uppercase())
    } else if allow_operators && is_allowed_prog_operator(ch) {
        Some(ch)
    } else if allow_neg_sign && ch == '-' {
        Some('-')
    } else if ch == ' ' {
        Some(' ')
    } else {
        None
    }
}

fn sanitize_radix_paste(raw: &str, radix: Radix, signed: bool) -> String {
    let trimmed = raw.trim();
    let (neg, rest) = if radix == Radix::Dec && signed {
        match trimmed.strip_prefix('-') {
            Some(r) => (true, r.trim_start()),
            None => (false, trimmed),
        }
    } else {
        (false, trimmed)
    };
    let rest = match radix {
        Radix::Hex => rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")).unwrap_or(rest),
        Radix::Bin => rest.strip_prefix("0b").or_else(|| rest.strip_prefix("0B")).unwrap_or(rest),
        Radix::Oct => rest.strip_prefix("0o").or_else(|| rest.strip_prefix("0O")).unwrap_or(rest),
        Radix::Dec => rest,
    };
    let mut out = String::with_capacity(rest.len() + usize::from(neg));
    if neg {
        out.push('-');
    }
    for ch in rest.chars() {
        if is_valid_radix_digit(ch, radix) {
            out.push(ch.to_ascii_uppercase());
        }
    }
    out
}

fn sanitize_prog_expr_paste(raw: &str, radix: Radix) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let rest = &raw[i..];
        let Some(ch) = rest.chars().next() else {
            break;
        };
        if ch == '0'
            && let Some(next) = rest.as_bytes().get(1).copied()
            && ((radix == Radix::Hex && matches!(next, b'x' | b'X'))
                || (radix == Radix::Bin && matches!(next, b'b' | b'B'))
                || (radix == Radix::Oct && matches!(next, b'o' | b'O')))
        {
            out.push('0');
            out.push((next as char).to_ascii_lowercase());
            i += 2;
            continue;
        }
        if let Some(c) = filter_prog_char(ch, radix, true, false) {
            out.push(c);
        }
        i += ch.len_utf8();
    }
    out
}

fn normalize_prog_expr(expr: &str, radix: Radix) -> String {
    let chars: Vec<char> = expr.chars().collect();
    let mut out = String::with_capacity(expr.len() + 16);
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() {
            out.push(' ');
            i += 1;
            continue;
        }
        if ch == '^' {
            out.push_str(" xor ");
            i += 1;
            continue;
        }
        if is_allowed_prog_operator(ch) {
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == '0'
            && i + 1 < chars.len()
            && matches!(chars[i + 1], 'x' | 'X' | 'b' | 'B' | 'o' | 'O')
        {
            let pfx_char = chars[i + 1].to_ascii_lowercase();
            let lit_radix = match pfx_char {
                'x' => Radix::Hex,
                'b' => Radix::Bin,
                _ => Radix::Oct,
            };
            out.push('0');
            out.push(pfx_char);
            i += 2;
            while i < chars.len()
                && (is_valid_radix_digit(chars[i], lit_radix) || chars[i] == '_')
            {
                if chars[i] != '_' {
                    out.push(chars[i].to_ascii_uppercase());
                }
                i += 1;
            }
            continue;
        }
        if is_valid_radix_digit(ch, radix) {
            let mut digits = String::new();
            let mut j = i;
            loop {
                while j < chars.len() && (is_valid_radix_digit(chars[j], radix) || chars[j] == '_') {
                    if chars[j] != '_' {
                        digits.push(chars[j].to_ascii_uppercase());
                    }
                    j += 1;
                }
                let mut k = j;
                while k < chars.len() && chars[k] == ' ' {
                    k += 1;
                }
                if k > j
                    && k < chars.len()
                    && is_valid_radix_digit(chars[k], radix)
                    && !(chars[k] == '0'
                        && k + 1 < chars.len()
                        && matches!(chars[k + 1], 'x' | 'X' | 'b' | 'B' | 'o' | 'O'))
                {
                    j = k;
                } else {
                    break;
                }
            }
            out.push_str(radix.prefix());
            out.push_str(&digits);
            i = j;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radix_digit_whitelists_enforce_strict_bases() {
        for c in "0123456789abcdefABCDEF".chars() {
            assert!(is_valid_radix_digit(c, Radix::Hex), "HEX must allow {c}");
        }
        for c in "gGzZйЙфФ@#.".chars() {
            assert!(!is_valid_radix_digit(c, Radix::Hex), "HEX must reject {c}");
        }

        for c in "0123456789".chars() {
            assert!(is_valid_radix_digit(c, Radix::Dec), "DEC must allow {c}");
        }
        for c in "aAfFgGфФ.".chars() {
            assert!(!is_valid_radix_digit(c, Radix::Dec), "DEC must reject {c}");
        }

        for c in "01234567".chars() {
            assert!(is_valid_radix_digit(c, Radix::Oct), "OCT must allow {c}");
        }
        for c in "89aAfF".chars() {
            assert!(!is_valid_radix_digit(c, Radix::Oct), "OCT must reject {c}");
        }

        for c in "01".chars() {
            assert!(is_valid_radix_digit(c, Radix::Bin), "BIN must allow {c}");
        }
        for c in "23456789aAfF".chars() {
            assert!(!is_valid_radix_digit(c, Radix::Bin), "BIN must reject {c}");
        }
    }

    #[test]
    fn operators_hex_keys_and_radix_switch_hotkeys() {
        for op in "+-*/%()&|^~<>".chars() {
            assert!(is_allowed_prog_operator(op), "operator {op} must be allowed");
            for radix in [Radix::Hex, Radix::Dec, Radix::Oct, Radix::Bin] {
                assert_eq!(filter_prog_char(op, radix, true, false), Some(op));
            }
        }
        assert_eq!(filter_prog_char('a', Radix::Hex, true, false), Some('A'));
        assert_eq!(filter_prog_char('f', Radix::Hex, false, false), Some('F'));
        assert_eq!(filter_prog_char('g', Radix::Hex, true, false), None);
        assert_eq!(filter_prog_char('ф', Radix::Hex, true, false), None);

        let none = egui::Modifiers::NONE;
        let ctrl = egui::Modifiers { ctrl: true, ..Default::default() };
        assert_eq!(map_radix_switch_key(Key::F5, none), Some(Radix::Hex));
        assert_eq!(map_radix_switch_key(Key::F6, none), Some(Radix::Dec));
        assert_eq!(map_radix_switch_key(Key::F7, none), Some(Radix::Oct));
        assert_eq!(map_radix_switch_key(Key::F8, none), Some(Radix::Bin));
        assert_eq!(map_radix_switch_key(Key::F5, ctrl), None);

        assert_eq!(map_hex_letter_key(Key::A, none), Some('A'));
        assert_eq!(map_hex_letter_key(Key::F, none), Some('F'));
        assert_eq!(map_hex_letter_key(Key::G, none), None);
        assert_eq!(map_hex_letter_key(Key::A, ctrl), None);
        assert_eq!(map_hex_letter_key(Key::C, ctrl), None);
    }

    #[test]
    fn paste_sanitization_and_expression_normalization() {
        assert_eq!(sanitize_radix_paste("DAD AFFF", Radix::Hex, false), "DADAFFF");
        assert_eq!(sanitize_radix_paste("0xde ad_be ef!!", Radix::Hex, false), "DEADBEEF");
        assert_eq!(sanitize_radix_paste("-12 345ab", Radix::Dec, true), "-12345");
        assert_eq!(sanitize_radix_paste("0o789 12", Radix::Oct, false), "712");
        assert_eq!(sanitize_radix_paste("0b1010 2911", Radix::Bin, false), "101011");

        assert_eq!(sanitize_prog_expr_paste("dad afff ^ 0f!!", Radix::Hex), "DAD AFFF ^ 0F");

        let mut reg = BitRegister::new(0, BitWidth::Bits64, false);
        let hex_norm = normalize_prog_expr("DAD AFFF", Radix::Hex);
        assert_eq!(reg.eval_and_apply(&hex_norm).expect("hex paste"), 0x0DAD_AFFF);

        let xor_norm = normalize_prog_expr("(FF << 4) ^ 0F", Radix::Hex);
        assert_eq!(reg.eval_and_apply(&xor_norm).expect("hex xor"), (0xFF << 4) ^ 0x0F);

        let bin_norm = normalize_prog_expr("1010 0101 & 1111 0000", Radix::Bin);
        assert_eq!(reg.eval_and_apply(&bin_norm).expect("bin and"), 0b1010_0000);

        let oct_norm = normalize_prog_expr("77 + 1", Radix::Oct);
        assert_eq!(reg.eval_and_apply(&oct_norm).expect("oct add"), 64);
    }

    #[test]
    fn headless_programmer_filters_ru_layout_and_switches_radices() {
        let ctx = egui::Context::default();
        let palette = style::apply_theme(&ctx, crate::style::ThemeId::Dark);
        let mut view = ProgrammerView::new(0, 64, false);

        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 720.0))),
            events: vec![
                Event::Key {
                    key: Key::A,
                    physical_key: Some(Key::A),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                Event::Text("ф".to_owned()),
                Event::Key {
                    key: Key::G,
                    physical_key: Some(Key::G),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                Event::Text("п".to_owned()),
                Event::Key {
                    key: Key::F,
                    physical_key: Some(Key::F),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                Event::Text("а".to_owned()),
            ],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = view.show(ui, Lang::Ru, &palette);
            });
        });
        out.textures_delta.clear();
        assert_eq!(view.reg.masked(), 0xAF);

        let switch_bin = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 720.0))),
            events: vec![
                Event::Key {
                    key: Key::F8,
                    physical_key: Some(Key::F8),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                Event::Key {
                    key: Key::Escape,
                    physical_key: Some(Key::Escape),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let mut out = ctx.run_ui(switch_bin, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = view.show(ui, Lang::En, &palette);
            });
        });
        out.textures_delta.clear();
        assert_eq!(view.active_radix, Radix::Bin);
        assert_eq!(view.reg.masked(), 0);
    }
}
