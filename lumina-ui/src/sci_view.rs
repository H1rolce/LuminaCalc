//! Responsive Scientific / Engineering calculator inspired by Casio fx-991ES PLUS / SC-991ESP.
//!
//! Features:
//! - Full dynamic scaling: fills available width and height cleanly with comfortable margins
//! - Interactive 2D Natural-V.P.A.M. display: click directly into numerators, denominators,
//!   between digits, or outside fractions; live blinking caret rendered inside the 2D formula
//! - Real cursor-based fraction editing (`(num)/(den)`): typing `77` stays inside the numerator
//!   until you click the denominator or press `▼`/`▶`/`Tab`
//! - Stacked fraction results (`23/12`, mixed `1 11/12` via `SHIFT + S⇔D`) & `ENG` notation
//! - `SHIFT`, `HYP`, `DEG/RAD/GRAD`, memory (`MC/MR/M-/M+/STO`), `Ans`, and live history tape

use crate::i18n::{Lang, format_error, tr};
use crate::math_display::{self, PaintFeedback};
use crate::state::SavedSciHistoryItem;
use crate::style::{self, Palette};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Event, FontId, Frame, Id, Key, Margin, Pos2,
    ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2,
};
use lumina_core::{
    AngleUnit, CalcError, Env, FormatOptions, Value, decimal_to_fraction, eval_str,
    format_engineering,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayMode {
    /// Improper stacked fraction (`23/12`).
    Fraction,
    /// Mixed stacked fraction (`1 11/12`), toggled via `SHIFT + S⇔D`.
    MixedFraction,
    /// Standard grouped decimal.
    Decimal,
    /// Engineering notation (exponent multiple of 3).
    Engineering,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyTone {
    Digit,
    Sci,
    Op,
    Danger,
    Accent,
    Shift,
}

#[derive(Debug, Clone)]
struct HistoryItem {
    expr: String,
    result_display: String,
    result_plain: String,
}

pub enum SciAction {
    None,
    Copy { text: String, label: String },
    SendToNotebook(String),
}

pub struct ScientificView {
    pub expr: String,
    /// Byte offset of the interactive caret inside `self.expr` (always on a UTF-8 boundary).
    pub cursor: usize,
    pub angle_unit: AngleUnit,
    display_mode: DisplayMode,
    shift: bool,
    hyp: bool,
    ans: Option<Value>,
    memory: Option<Value>,
    committed: bool,
    last_eval: Option<Result<Value, CalcError>>,
    history: Vec<HistoryItem>,
    history_cursor: Option<usize>,
    prng_state: u64,
    state_dirty: bool,
}

impl Default for ScientificView {
    fn default() -> Self {
        Self::new(&[], 0)
    }
}

impl ScientificView {
    pub fn new(saved_history: &[SavedSciHistoryItem], saved_angle_unit: u8) -> Self {
        let angle_unit = match saved_angle_unit {
            1 => AngleUnit::Rad,
            2 => AngleUnit::Grad,
            _ => AngleUnit::Deg,
        };
        let history = saved_history
            .iter()
            .take(40)
            .map(|h| HistoryItem {
                expr: h.expr.clone(),
                result_display: h.result_display.clone(),
                result_plain: h.result_plain.clone(),
            })
            .collect();
        Self {
            expr: String::new(),
            cursor: 0,
            angle_unit,
            display_mode: DisplayMode::Fraction,
            shift: false,
            hyp: false,
            ans: None,
            memory: None,
            committed: false,
            last_eval: None,
            history,
            history_cursor: None,
            prng_state: 0x9E37_79B9_7F4A_7C15,
            state_dirty: false,
        }
    }

    pub fn to_saved(&self) -> (Vec<SavedSciHistoryItem>, u8) {
        let hist = self
            .history
            .iter()
            .map(|h| SavedSciHistoryItem {
                expr: h.expr.clone(),
                result_display: h.result_display.clone(),
                result_plain: h.result_plain.clone(),
            })
            .collect();
        let angle = match self.angle_unit {
            AngleUnit::Deg => 0,
            AngleUnit::Rad => 1,
            AngleUnit::Grad => 2,
        };
        (hist, angle)
    }

    fn clamp_cursor(&mut self) {
        if self.cursor > self.expr.len() {
            self.cursor = self.expr.len();
        }
        while self.cursor > 0 && !self.expr.is_char_boundary(self.cursor) {
            self.cursor -= 1;
        }
    }

    fn has_empty_slot(&self) -> bool {
        self.expr.contains("()") || self.expr.contains('□')
    }

    fn build_env(&self) -> Env {
        let mut env = Env {
            angle_unit: self.angle_unit,
            prev: self.ans.clone(),
            ..Env::default()
        };
        if let Some(m) = &self.memory {
            env.vars.insert("M".into(), m.clone());
        }
        env
    }

    fn recompute(&mut self) {
        self.clamp_cursor();
        let trimmed = self.expr.trim();
        if trimmed.is_empty() || self.has_empty_slot() {
            self.last_eval = None;
            return;
        }
        let env = self.build_env();
        self.last_eval = Some(eval_str(trimmed, &env));
    }

    fn format_value_text(&self, val: &Value, precision: u32) -> (String, Option<String>) {
        let opts = FormatOptions { precision, grouping: Some(' ') };
        let dec = val.display(&opts);
        if !val.is_dimensionless() {
            return (dec, None);
        }
        match self.display_mode {
            DisplayMode::Fraction => {
                if let Some((num, den)) = decimal_to_fraction(val.si, 10_000) {
                    (format!("{num}/{den}"), Some(format!("≈ {dec}")))
                } else {
                    (dec, None)
                }
            }
            DisplayMode::MixedFraction => {
                if let Some((num, den)) = decimal_to_fraction(val.si, 10_000) {
                    let sign = if num < 0 { "-" } else { "" };
                    let (an, ad) = (num.unsigned_abs(), den.unsigned_abs());
                    let txt = if an > ad {
                        format!("{sign}{} {}/{}", an / ad, an % ad, ad)
                    } else {
                        format!("{num}/{den}")
                    };
                    (txt, Some(format!("≈ {dec}")))
                } else {
                    (dec, None)
                }
            }
            DisplayMode::Decimal => {
                let alt = decimal_to_fraction(val.si, 10_000).map(|(n, d)| format!("= {n}/{d}"));
                (dec, alt)
            }
            DisplayMode::Engineering => {
                let eng = format_engineering(val.si, precision.min(8));
                (eng, Some(format!("= {dec}")))
            }
        }
    }

    /// Inserts `token` at `self.cursor`, keeping the caret right after the inserted text.
    /// Typing `7`, `7` inside `(|)/()` naturally produces `(77|)/()`.
    pub fn insert_token(&mut self, token: &str, replace_if_committed: bool) {
        if self.committed && replace_if_committed {
            self.expr.clear();
            self.cursor = 0;
        }
        self.committed = false;
        self.history_cursor = None;
        self.clamp_cursor();

        self.expr.insert_str(self.cursor, token);
        self.cursor += token.len();
        self.recompute();
    }

    /// Finds the start byte of the operand immediately preceding `self.cursor`
    /// (without crossing an unmatched `(`).
    fn operand_start_before_cursor(&self) -> Option<usize> {
        let before = &self.expr[..self.cursor];
        let trimmed_end = before.trim_end().len();
        if trimmed_end == 0 {
            return None;
        }
        let slice = &before[..trimmed_end];
        let last = slice.chars().next_back()?;

        if last == ')' {
            // Walk back to matching `(`.
            let mut depth = 0i32;
            for (b, c) in slice.char_indices().rev() {
                match c {
                    ')' => depth += 1,
                    '(' => {
                        depth -= 1;
                        if depth == 0 {
                            // Include function name before `(` if any (e.g. `sin(30)`).
                            let mut start = b;
                            for (fb, fc) in slice[..b].char_indices().rev() {
                                if fc.is_alphanumeric() || fc == '_' {
                                    start = fb;
                                } else {
                                    break;
                                }
                            }
                            return Some(start);
                        }
                    }
                    _ => {}
                }
            }
            return None;
        }

        if last.is_alphanumeric() || matches!(last, '.' | '_' | 'π' | 'τ' | 'φ' | '²' | '³' | '!') {
            let mut start = trimmed_end;
            for (b, c) in slice.char_indices().rev() {
                if c.is_alphanumeric() || matches!(c, '.' | '_' | 'π' | 'τ' | 'φ' | '²' | '³' | '!') {
                    start = b;
                } else {
                    break;
                }
            }
            return Some(start);
        }
        None
    }

    /// Inserts an interactive fraction template `(num)/(den)` at `self.cursor`.
    /// - If an operand immediately precedes the cursor (e.g. `77|`), wraps it into `(77)/(|)`
    ///   with the cursor inside the denominator.
    /// - Otherwise inserts `(|)/()` with the cursor inside the numerator so you can type `77`
    ///   freely without jumping down.
    pub fn insert_fraction(&mut self, mixed: bool) {
        if self.committed {
            self.expr.clear();
            self.cursor = 0;
            self.committed = false;
        }
        self.history_cursor = None;
        self.clamp_cursor();

        if mixed {
            if self.expr[..self.cursor].trim().is_empty() {
                self.expr.insert_str(self.cursor, " + ()/()");
                // Place caret before ` + ()/()` so user types the whole part first.
            } else {
                let tpl = " + ()/()";
                self.expr.insert_str(self.cursor, tpl);
                self.cursor += 4; // inside numerator `(|)/()`
            }
            self.recompute();
            return;
        }

        if let Some(op_start) = self.operand_start_before_cursor() {
            let trimmed_end = self.expr[..self.cursor].trim_end().len();
            let operand = self.expr[op_start..trimmed_end].to_owned();
            let replacement = format!("({operand})/()");
            let den_cursor = op_start + replacement.len() - 1; // inside `()`
            self.expr.replace_range(op_start..self.cursor, &replacement);
            self.cursor = den_cursor;
        } else {
            self.expr.insert_str(self.cursor, "()/()");
            self.cursor += 1; // inside numerator `(|)/()`
        }
        self.recompute();
    }

    /// Moves caret right, smoothly stepping across `)/(` from numerator into denominator.
    pub fn move_right(&mut self) {
        self.clamp_cursor();
        self.committed = false;
        if self.expr[self.cursor..].starts_with(")/(") {
            self.cursor += 3;
            return;
        }
        if let Some(ch) = self.expr[self.cursor..].chars().next() {
            self.cursor += ch.len_utf8();
        }
    }

    /// Moves caret left, smoothly stepping across `)/(` from denominator back into numerator.
    pub fn move_left(&mut self) {
        self.clamp_cursor();
        self.committed = false;
        if self.expr[..self.cursor].ends_with(")/(") {
            self.cursor -= 3;
            return;
        }
        if let Some((b, _)) = self.expr[..self.cursor].char_indices().next_back() {
            self.cursor = b;
        }
    }

    /// Finds the enclosing `(`..`)` pair containing `self.cursor` (innermost).
    fn enclosing_parens(&self) -> Option<(usize, usize)> {
        let mut depth = 0i32;
        let mut open_pos = None;
        for (b, c) in self.expr[..self.cursor].char_indices().rev() {
            match c {
                ')' => depth += 1,
                '(' => {
                    if depth == 0 {
                        open_pos = Some(b);
                        break;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        let open = open_pos?;
        let mut d = 0i32;
        for (rel, c) in self.expr[open..].char_indices() {
            match c {
                '(' => d += 1,
                ')' => {
                    d -= 1;
                    if d == 0 {
                        return Some((open, open + rel));
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Jumps from numerator to denominator, or exits denominator to after the fraction.
    pub fn jump_fraction_down(&mut self) -> bool {
        self.clamp_cursor();
        self.committed = false;

        if let Some((open, close)) = self.enclosing_parens() {
            // Case 1: currently inside numerator `(...|...)/(...)` -> jump to denominator!
            if self.expr[close..].starts_with(")/(") {
                let den_open = close + 2;
                let mut d = 0i32;
                for (rel, c) in self.expr[den_open..].char_indices() {
                    match c {
                        '(' => d += 1,
                        ')' => {
                            d -= 1;
                            if d == 0 {
                                self.cursor = den_open + rel; // end of denominator (or empty slot)
                                return true;
                            }
                        }
                        _ => {}
                    }
                }
                self.cursor = (close + 3).min(self.expr.len());
                return true;
            }
            // Case 2: currently inside denominator `(...)/(...|...)` -> exit after `)`!
            if open >= 2 && self.expr[..open + 1].ends_with(")/(") {
                self.cursor = (close + 1).min(self.expr.len());
                return true;
            }
        }

        // Fallback: jump into the next empty `()` slot if any exists ahead.
        if let Some(rel) = self.expr[self.cursor..].find("()") {
            self.cursor += rel + 1;
            return true;
        }
        false
    }

    /// Jumps from denominator back up into the numerator.
    pub fn jump_fraction_up(&mut self) -> bool {
        self.clamp_cursor();
        self.committed = false;

        if let Some((open, _close)) = self.enclosing_parens()
            && open >= 2
            && self.expr[..open + 1].ends_with(")/(")
        {
            // End of numerator is `open - 2` (right before `)/(`).
            self.cursor = open - 2;
            return true;
        }
        false
    }

    /// Smart backspace that cleanly removes empty fractions or unwraps `(num)/()` -> `num`.
    pub fn backspace(&mut self) {
        self.clamp_cursor();
        self.committed = false;
        self.history_cursor = None;

        if self.cursor == 0 {
            return;
        }

        // Case 1: inside empty numerator `(|)/()` -> remove entire `()/()`
        if self.cursor >= 1
            && self.cursor + 4 <= self.expr.len()
            && &self.expr[self.cursor - 1..self.cursor + 4] == "()/()"
        {
            self.expr.replace_range(self.cursor - 1..self.cursor + 4, "");
            self.cursor -= 1;
            self.recompute();
            return;
        }

        // Case 2: inside empty denominator `(num)/(|)` -> unwrap to `num|`
        if self.cursor >= 3
            && self.expr[..self.cursor].ends_with(")/(")
            && self.expr[self.cursor..].starts_with(')')
        {
            let num_close = self.cursor - 3;
            // Find matching `(` of numerator
            let mut d = 0i32;
            let mut num_open = None;
            for (b, c) in self.expr[..=num_close].char_indices().rev() {
                match c {
                    ')' => d += 1,
                    '(' => {
                        d -= 1;
                        if d == 0 {
                            num_open = Some(b);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(open) = num_open {
                let inner = self.expr[open + 1..num_close].to_owned();
                let new_cursor = open + inner.len();
                self.expr.replace_range(open..=self.cursor, &inner);
                self.cursor = new_cursor;
                self.recompute();
                return;
            }
        }

        // Case 3: right after a closed fraction `(num)/(den)|` -> step inside denominator and delete
        if self.expr[..self.cursor].ends_with(')') {
            let close = self.cursor - 1;
            let mut d = 0i32;
            let mut open_pos = None;
            for (b, c) in self.expr[..=close].char_indices().rev() {
                match c {
                    ')' => d += 1,
                    '(' => {
                        d -= 1;
                        if d == 0 {
                            open_pos = Some(b);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(open) = open_pos
                && open >= 2
                && self.expr[..open + 1].ends_with(")/(")
            {
                self.cursor = close;
                if close == open + 1 {
                    // Empty denominator: recurse once to unwrap cleanly.
                    self.backspace();
                } else if let Some((prev_b, _)) = self.expr[..self.cursor].char_indices().next_back() {
                    self.expr.replace_range(prev_b..self.cursor, "");
                    self.cursor = prev_b;
                    self.recompute();
                }
                return;
            }
        }

        // Default: delete one UTF-8 character before cursor.
        if let Some((prev_b, _)) = self.expr[..self.cursor].char_indices().next_back() {
            self.expr.replace_range(prev_b..self.cursor, "");
            self.cursor = prev_b;
            self.recompute();
        }
    }

    fn delete_forward(&mut self) {
        self.clamp_cursor();
        self.committed = false;
        if let Some(ch) = self.expr[self.cursor..].chars().next() {
            self.expr.replace_range(self.cursor..self.cursor + ch.len_utf8(), "");
            self.recompute();
        }
    }

    pub fn clear_all(&mut self) {
        self.expr.clear();
        self.cursor = 0;
        self.committed = false;
        self.shift = false;
        self.hyp = false;
        self.history_cursor = None;
        self.last_eval = None;
    }

    fn evaluate_commit(&mut self, precision: u32) {
        self.recompute();
        if let Some(Ok(val)) = &self.last_eval {
            let val = val.clone();
            let (shown, _) = self.format_value_text(&val, precision);
            let plain = val.plain(precision);
            let expr_clean = self.expr.trim().to_owned();
            if !expr_clean.is_empty() {
                let dup = self
                    .history
                    .first()
                    .is_some_and(|h| h.expr == expr_clean && h.result_plain == plain);
                if !dup {
                    self.history.insert(
                        0,
                        HistoryItem {
                            expr: expr_clean,
                            result_display: shown,
                            result_plain: plain,
                        },
                    );
                    self.history.truncate(40);
                    self.state_dirty = true;
                }
            }
            self.ans = Some(val);
            self.committed = true;
            self.cursor = self.expr.len();
            self.history_cursor = None;
        }
    }

    fn next_random(&mut self) -> String {
        let mut x = self.prng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.prng_state = x;
        let v = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / ((1u64 << 53) as f64);
        format!("{v:.4}")
    }

    fn mem_add(&mut self, sign: i8) {
        self.recompute();
        let Some(Ok(val)) = &self.last_eval else {
            return;
        };
        let mut v = val.clone();
        if sign < 0 {
            v.si = -v.si;
        }
        match &mut self.memory {
            Some(m) if m.dim == v.dim => {
                if let Some(sum) = m.si.checked_add(v.si) {
                    m.si = sum;
                }
            }
            _ => {
                self.memory = Some(v);
            }
        }
    }

    fn history_step(&mut self, delta: isize) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.history_cursor {
            None => {
                if delta > 0 {
                    0
                } else {
                    return;
                }
            }
            Some(cur) => {
                let cand = cur as isize + delta;
                if cand < 0 {
                    self.history_cursor = None;
                    return;
                }
                (cand as usize).min(self.history.len() - 1)
            }
        };
        self.history_cursor = Some(next);
        self.expr = self.history[next].expr.clone();
        self.cursor = self.expr.len();
        self.committed = false;
        self.recompute();
    }

    /// Handles physical keyboard input directly on the 2D display when the raw text bar is not focused.
    fn handle_direct_keyboard(&mut self, ui: &Ui, precision: u32) {
        let mut typed = String::new();
        let mut enter = false;
        let mut back = false;
        let mut del = false;
        let mut left = false;
        let mut right = false;
        let mut up = false;
        let mut down = false;
        let mut tab = false;

        ui.ctx().input(|i| {
            let ctrl = i.modifiers.ctrl || i.modifiers.command || i.modifiers.alt;
            if !ctrl {
                for ev in &i.events {
                    if let Event::Text(t) = ev {
                        for ch in t.chars() {
                            if ch.is_alphanumeric()
                                || matches!(
                                    ch,
                                    '.' | ',' | '+' | '-' | '*' | '/' | '(' | ')' | '^' | '%' | '!' | ' '
                                )
                            {
                                typed.push(ch);
                            }
                        }
                    }
                }
                if i.key_pressed(Key::Enter) {
                    enter = true;
                }
                if i.key_pressed(Key::Backspace) {
                    back = true;
                }
                if i.key_pressed(Key::Delete) {
                    del = true;
                }
                if i.key_pressed(Key::ArrowLeft) {
                    left = true;
                }
                if i.key_pressed(Key::ArrowRight) {
                    right = true;
                }
                if i.key_pressed(Key::ArrowUp) {
                    up = true;
                }
                if i.key_pressed(Key::ArrowDown) {
                    down = true;
                }
                if i.key_pressed(Key::Tab) {
                    tab = true;
                }
            }
        });

        for ch in typed.chars() {
            if ch == '/' {
                self.insert_fraction(false);
            } else {
                let replace = ch.is_ascii_digit() || ch == '(' || ch.is_alphabetic();
                let mut buf = [0u8; 4];
                self.insert_token(ch.encode_utf8(&mut buf), replace);
            }
        }
        if enter {
            self.evaluate_commit(precision);
        }
        if back {
            self.backspace();
        }
        if del {
            self.delete_forward();
        }
        if left {
            self.move_left();
        }
        if right {
            self.move_right();
        }
        if (tab || down) && !self.jump_fraction_down() && down {
            self.history_step(-1);
        }
        if up && !self.jump_fraction_up() {
            self.history_step(1);
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        precision: u32,
        palette: &Palette,
    ) -> (SciAction, bool) {
        let s = tr(lang);
        let mut action = SciAction::None;
        self.state_dirty = false;

        let input_id = Id::unique("lumina_sci_expr_input");
        let input_focused = ui.ctx().memory(|m| m.has_focus(input_id));
        if ui.ctx().input(|i| i.key_pressed(Key::Escape)) {
            self.clear_all();
        }
        if !input_focused {
            self.handle_direct_keyboard(ui, precision);
        }

        let avail = ui.available_rect_before_wrap();
        let pad_x = (avail.width() * 0.018).clamp(14.0, 28.0);
        let pad_top = (avail.height() * 0.020).clamp(14.0, 24.0);
        let pad_bottom = (avail.height() * 0.042).clamp(24.0, 48.0);
        let inner_w = (avail.width() - pad_x * 2.0).max(360.0);
        let inner_h = (avail.height() - pad_top - pad_bottom).max(440.0);

        let show_history_side = inner_w >= 780.0;
        let gap_cols = (inner_w * 0.014).clamp(12.0, 20.0);
        let side_w = if show_history_side {
            (inner_w * 0.28).clamp(240.0, 420.0)
        } else {
            0.0
        };
        let calc_w = if show_history_side {
            (inner_w - side_w - gap_cols).max(460.0)
        } else {
            inner_w
        };

        let needs_scroll = avail.height() < 510.0;
        let render_content = |ui: &mut Ui, this: &mut Self, action: &mut SciAction| {
            Frame::new()
                .inner_margin(Margin {
                    left: pad_x as i8,
                    right: pad_x as i8,
                    top: pad_top as i8,
                    bottom: pad_bottom as i8,
                })
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(calc_w, inner_h),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                this.show_calculator_body(
                                    ui,
                                    calc_w,
                                    inner_h,
                                    input_id,
                                    input_focused,
                                    lang,
                                    precision,
                                    palette,
                                    action,
                                );
                            },
                        );

                        if show_history_side {
                            ui.add_space(gap_cols);
                            ui.allocate_ui_with_layout(
                                Vec2::new(side_w, inner_h),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    this.show_history_panel(ui, side_w, inner_h, s, palette, action);
                                },
                            );
                        }
                    });
                });
        };

        if needs_scroll {
            ScrollArea::vertical()
                .id_salt("sci_view_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| render_content(ui, self, &mut action));
        } else {
            render_content(ui, self, &mut action);
        }

        (action, std::mem::take(&mut self.state_dirty))
    }

    #[allow(clippy::too_many_arguments)]
    fn show_calculator_body(
        &mut self,
        ui: &mut Ui,
        calc_w: f32,
        calc_h: f32,
        input_id: Id,
        input_focused: bool,
        lang: Lang,
        precision: u32,
        palette: &Palette,
        action: &mut SciAction,
    ) {
        let s = tr(lang);

        let outer_pad = (calc_h * 0.020).clamp(10.0, 18.0);
        let content_h = (calc_h - outer_pad * 2.0).max(410.0);
        let display_h = (content_h * 0.32).clamp(146.0, 300.0);
        let gap_x = (calc_w * 0.011).clamp(6.0, 12.0);
        let gap_y = (content_h * 0.010).clamp(5.0, 10.0);

        let fixed_spacers = gap_y * 9.0 + 6.0;
        let keys_budget = (content_h - display_h - fixed_spacers).max(240.0);
        let unit_h = keys_budget / 8.38;
        let ctrl_h = (unit_h * 0.78).clamp(26.0, 56.0);
        let sci_h = (unit_h * 0.96).clamp(32.0, 70.0);
        let num_h = (unit_h * 1.18).clamp(38.0, 86.0);

        let scale = (num_h / 48.0).min(calc_w / 540.0).clamp(0.86, 1.65);

        Frame::new()
            .fill(palette.bg_mantle)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::same(outer_pad as i8))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.set_min_size(Vec2::new(
                    (calc_w - outer_pad * 2.0).max(320.0),
                    content_h,
                ));

                // ── Interactive 2D Natural-V.P.A.M. Display ──
                let disp_pad_x = (14.0 * scale).clamp(10.0, 22.0);
                let disp_pad_y = (10.0 * scale).clamp(8.0, 14.0);
                Frame::new()
                    .fill(palette.bg_crust)
                    .stroke(Stroke::new(1.0, palette.border_subtle))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(disp_pad_x as i8, disp_pad_y as i8))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let inner_disp_h = (display_h - disp_pad_y * 2.0).max(124.0);
                        ui.set_min_height(inner_disp_h);

                        let tag_fs = (10.5 * scale).clamp(9.5, 13.5);
                        ui.horizontal(|ui| {
                            paint_status_tag(ui, "SHIFT", self.shift, palette.warning, tag_fs, palette);
                            paint_status_tag(ui, "HYP", self.hyp, palette.accent, tag_fs, palette);

                            let deg_resp = style::paint_pill(
                                ui,
                                self.angle_unit.label(),
                                Palette::alpha(palette.syn_unit, 32),
                                palette.syn_unit,
                                Stroke::new(1.0, Palette::alpha(palette.syn_unit, 110)),
                                tag_fs,
                            )
                            .on_hover_cursor(CursorIcon::PointingHand);
                            if deg_resp.clicked() {
                                self.angle_unit = self.angle_unit.next();
                                self.state_dirty = true;
                                self.recompute();
                            }

                            let mode_label = match self.display_mode {
                                DisplayMode::Fraction => "FRAC",
                                DisplayMode::MixedFraction => "a b/c",
                                DisplayMode::Decimal => "DEC",
                                DisplayMode::Engineering => "ENG",
                            };
                            let mode_resp = style::paint_pill(
                                ui,
                                mode_label,
                                Palette::alpha(palette.accent_alt, 28),
                                palette.accent_alt,
                                Stroke::new(1.0, Palette::alpha(palette.accent_alt, 100)),
                                tag_fs,
                            )
                            .on_hover_cursor(CursorIcon::PointingHand);
                            if mode_resp.clicked() {
                                self.display_mode = match self.display_mode {
                                    DisplayMode::Fraction => DisplayMode::MixedFraction,
                                    DisplayMode::MixedFraction => DisplayMode::Decimal,
                                    DisplayMode::Decimal => DisplayMode::Engineering,
                                    DisplayMode::Engineering => DisplayMode::Fraction,
                                };
                            }

                            if let Some(m) = &self.memory {
                                let m_str = m.plain(6);
                                paint_status_tag(ui, "M", true, palette.syn_number, tag_fs, palette)
                                    .on_hover_text(format!("M = {m_str}"));
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if !self.expr.trim().is_empty() && !self.has_empty_slot() {
                                    let send_btn = ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(s.prog_send_notebook)
                                                    .size((11.5 * scale).clamp(10.5, 14.0))
                                                    .color(palette.text_secondary),
                                            )
                                            .frame(false),
                                        )
                                        .on_hover_cursor(CursorIcon::PointingHand);
                                    if send_btn.clicked() {
                                        *action = SciAction::SendToNotebook(self.expr.trim().to_owned());
                                    }
                                }
                            });
                        });

                        // ── 2D Interactive Math Canvas (click numerator/denominator/chars) ──
                        ui.add_space(4.0);
                        let res_h = (inner_disp_h * 0.30).clamp(38.0, 72.0);
                        let input_fs = (13.0 * scale).clamp(11.5, 16.0);
                        let est_input_h = input_fs + 6.0;
                        let est_tags_h = tag_fs + 14.0;
                        let math_area_h =
                            (inner_disp_h - est_tags_h - est_input_h - res_h - 12.0).max(52.0);
                        let (math_rect, math_resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), math_area_h),
                            Sense::click(),
                        );

                        if math_resp.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::Text);
                        }
                        let hover_pos = math_resp.hover_pos();

                        let mut fb = PaintFeedback::default();
                        if !self.expr.is_empty() {
                            self.clamp_cursor();
                            let tree = math_display::parse_math(&self.expr);
                            let mut fs = (23.0 * scale).clamp(17.0, 34.0);
                            let max_w = (math_rect.width() - 10.0).max(80.0);
                            let max_h = (math_rect.height() - 4.0).max(32.0);

                            let mut m = math_display::measure(&tree, ui.painter(), fs);
                            for _ in 0..4 {
                                let fit = (max_w / m.width.max(1.0)).min(max_h / m.height().max(1.0));
                                if fit >= 0.99 || fs <= 8.0 {
                                    break;
                                }
                                fs = (fs * fit * 0.97).max(8.0);
                                m = math_display::measure(&tree, ui.painter(), fs);
                            }

                            let top_y = math_rect.top() + (math_rect.height() - m.height()) * 0.5;
                            let mid_y = top_y + m.above;
                            let clipped = ui.painter().with_clip_rect(math_rect);
                            math_display::paint_interactive(
                                &tree,
                                &clipped,
                                math_rect.left() + 6.0,
                                mid_y,
                                fs,
                                self.cursor,
                                hover_pos,
                                palette,
                                &mut fb,
                            );

                            // Paint smooth blinking caret inside the 2D formula
                            if !input_focused
                                && let Some(caret) = fb.caret
                            {
                                let t = ui.ctx().input(|i| i.time);
                                let blink = ((t * 5.2).sin() * 0.5 + 0.5) as f32;
                                let alpha = (70.0 + blink * 185.0) as u8;
                                clipped.line_segment(
                                    [Pos2::new(caret.x, caret.top_y), Pos2::new(caret.x, caret.bot_y)],
                                    Stroke::new(2.0, Palette::alpha(palette.accent, alpha)),
                                );
                                ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
                            }

                            // Click anywhere inside the 2D expression to position caret precisely!
                            if math_resp.clicked()
                                && let Some(click_pos) = math_resp.interact_pointer_pos()
                            {
                                self.cursor = fb.byte_at_pos(click_pos, self.expr.len());
                                self.clamp_cursor();
                                self.committed = false;
                                ui.ctx().memory_mut(|mem| mem.surrender_focus(input_id));
                            }
                        } else {
                            let caret_x = math_rect.left() + 6.0;
                            let h = (20.0 * scale).clamp(16.0, 28.0);
                            if !input_focused {
                                let t = ui.ctx().input(|i| i.time);
                                let blink = ((t * 5.2).sin() * 0.5 + 0.5) as f32;
                                let alpha = (70.0 + blink * 185.0) as u8;
                                ui.painter().line_segment(
                                    [
                                        Pos2::new(caret_x, math_rect.center().y - h * 0.5),
                                        Pos2::new(caret_x, math_rect.center().y + h * 0.5),
                                    ],
                                    Stroke::new(2.0, Palette::alpha(palette.accent, alpha)),
                                );
                                ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
                            }
                            ui.painter().text(
                                Pos2::new(caret_x + 6.0, math_rect.center().y),
                                Align2::LEFT_CENTER,
                                "2/3 + 1/6",
                                FontId::monospace((19.0 * scale).clamp(15.0, 26.0)),
                                Palette::alpha(palette.text_muted, 95),
                            );
                        }

                        ui.add_space(4.0);

                        // Compact synchronized raw text bar (clicking here edits raw text directly)
                        let err_span_opt = self
                            .last_eval
                            .as_ref()
                            .and_then(|r| r.as_ref().err())
                            .map(|e| e.span.clone());
                        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
                            let job = style::build_singleline_highlight(
                                buf.as_str(),
                                false,
                                err_span_opt.as_ref(),
                                input_fs,
                                wrap_width,
                                palette,
                            );
                            ui.painter().layout_job(job)
                        };

                        let expr_w = ui.available_width();
                        let out = TextEdit::singleline(&mut self.expr)
                            .id(input_id)
                            .frame(Frame::NONE)
                            .desired_width(expr_w)
                            .hint_text("...")
                            .layouter(&mut layouter)
                            .show(ui);
                        if out.response.changed() {
                            self.committed = false;
                            self.history_cursor = None;
                            if let Some(cr) = out.cursor_range {
                                let ci: usize = cr.primary.index.into();
                                self.cursor = char_to_byte_idx(&self.expr, ci);
                            } else {
                                self.cursor = self.expr.len();
                            }
                            self.recompute();
                        } else if out.response.has_focus()
                            && let Some(cr) = out.cursor_range
                        {
                            let ci: usize = cr.primary.index.into();
                            self.cursor = char_to_byte_idx(&self.expr, ci);
                        }
                        if out.response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            self.evaluate_commit(precision);
                        }

                        ui.add_space(4.0);

                        // ── Large Result Area (stacked fraction when in FRAC / a b/c mode) ──
                        let (res_rect, res_resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), res_h),
                            Sense::click(),
                        );

                        let res_fs = (26.0 * scale).clamp(21.0, 40.0);
                        match &self.last_eval {
                            Some(Ok(val)) => {
                                let (main_str, alt_str) = self.format_value_text(val, precision);
                                let plain = val.plain(precision);
                                let res_col = if self.committed {
                                    palette.success
                                } else {
                                    Palette::alpha(palette.text_primary, 210)
                                };

                                if res_resp.hovered() {
                                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                    ui.painter().rect_filled(
                                        res_rect,
                                        CornerRadius::same(6),
                                        Palette::alpha(palette.bg_elevated, 120),
                                    );
                                }

                                if let Some(alt) = &alt_str {
                                    ui.painter().text(
                                        Pos2::new(res_rect.left() + 4.0, res_rect.center().y),
                                        Align2::LEFT_CENTER,
                                        alt,
                                        FontId::monospace((13.0 * scale).clamp(11.5, 17.5)),
                                        palette.text_muted,
                                    );
                                }

                                let frac_pair = if val.is_dimensionless()
                                    && matches!(
                                        self.display_mode,
                                        DisplayMode::Fraction | DisplayMode::MixedFraction
                                    ) {
                                    decimal_to_fraction(val.si, 10_000)
                                } else {
                                    None
                                };

                                if let Some((num, den)) = frac_pair {
                                    math_display::paint_stacked_fraction_result(
                                        ui.painter(),
                                        res_rect,
                                        num,
                                        den,
                                        self.display_mode == DisplayMode::MixedFraction,
                                        res_fs,
                                        res_col,
                                    );
                                } else {
                                    ui.painter().text(
                                        Pos2::new(res_rect.right() - 4.0, res_rect.center().y),
                                        Align2::RIGHT_CENTER,
                                        &main_str,
                                        FontId::monospace(res_fs),
                                        res_col,
                                    );
                                }

                                if res_resp.clicked() {
                                    *action = SciAction::Copy {
                                        text: plain,
                                        label: main_str,
                                    };
                                }
                            }
                            Some(Err(err)) => {
                                let msg = format_error(lang, &err.kind);
                                ui.painter().text(
                                    Pos2::new(res_rect.right() - 4.0, res_rect.center().y),
                                    Align2::RIGHT_CENTER,
                                    msg,
                                    FontId::proportional((13.5 * scale).clamp(12.0, 18.0)),
                                    palette.error,
                                );
                            }
                            None => {
                                let placeholder = if self.has_empty_slot() { "…" } else { "0" };
                                ui.painter().text(
                                    Pos2::new(res_rect.right() - 4.0, res_rect.center().y),
                                    Align2::RIGHT_CENTER,
                                    placeholder,
                                    FontId::monospace(res_fs),
                                    palette.text_muted,
                                );
                            }
                        }
                    });

                ui.add_space(gap_y + 2.0);

                // ── Top control row: SHIFT, HYP, DEG/RAD, ◀, ▶, ▲/▼, S⇔D, ENG ──
                let avail_w = ui.available_width();
                let ctrl_w = (avail_w - gap_x * 7.0) / 8.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap_x;
                    if paint_sci_key(
                        ui,
                        "SHIFT",
                        "",
                        Vec2::new(ctrl_w, ctrl_h),
                        KeyTone::Shift,
                        self.shift,
                        false,
                        scale,
                        palette,
                    ) {
                        self.shift = !self.shift;
                    }
                    if paint_sci_key(
                        ui,
                        "HYP",
                        "",
                        Vec2::new(ctrl_w, ctrl_h),
                        KeyTone::Sci,
                        self.hyp,
                        false,
                        scale,
                        palette,
                    ) {
                        self.hyp = !self.hyp;
                    }
                    if paint_sci_key(
                        ui,
                        self.angle_unit.label(),
                        "",
                        Vec2::new(ctrl_w, ctrl_h),
                        KeyTone::Sci,
                        false,
                        false,
                        scale,
                        palette,
                    ) {
                        self.angle_unit = self.angle_unit.next();
                        self.state_dirty = true;
                        self.recompute();
                    }
                    if paint_sci_key(
                        ui,
                        "◀",
                        "",
                        Vec2::new(ctrl_w, ctrl_h),
                        KeyTone::Sci,
                        false,
                        false,
                        scale,
                        palette,
                    ) {
                        self.move_left();
                    }
                    if paint_sci_key(
                        ui,
                        "▶",
                        "",
                        Vec2::new(ctrl_w, ctrl_h),
                        KeyTone::Sci,
                        false,
                        false,
                        scale,
                        palette,
                    ) {
                        self.move_right();
                    }
                    if paint_sci_key(
                        ui,
                        "▼",
                        "▲",
                        Vec2::new(ctrl_w, ctrl_h),
                        KeyTone::Sci,
                        false,
                        self.shift,
                        scale,
                        palette,
                    ) {
                        if std::mem::take(&mut self.shift) {
                            if !self.jump_fraction_up() {
                                self.history_step(1);
                            }
                        } else if !self.jump_fraction_down() {
                            self.history_step(-1);
                        }
                    }
                    if paint_sci_key(
                        ui,
                        "S⇔D",
                        "a b/c",
                        Vec2::new(ctrl_w, ctrl_h),
                        KeyTone::Sci,
                        matches!(self.display_mode, DisplayMode::Fraction | DisplayMode::MixedFraction),
                        self.shift,
                        scale,
                        palette,
                    ) {
                        if std::mem::take(&mut self.shift) {
                            self.display_mode = if self.display_mode == DisplayMode::MixedFraction {
                                DisplayMode::Fraction
                            } else {
                                DisplayMode::MixedFraction
                            };
                        } else {
                            self.display_mode = match self.display_mode {
                                DisplayMode::Fraction | DisplayMode::MixedFraction => DisplayMode::Decimal,
                                DisplayMode::Decimal | DisplayMode::Engineering => DisplayMode::Fraction,
                            };
                        }
                    }
                    if paint_sci_key(
                        ui,
                        "ENG",
                        "",
                        Vec2::new(ctrl_w, ctrl_h),
                        KeyTone::Sci,
                        self.display_mode == DisplayMode::Engineering,
                        false,
                        scale,
                        palette,
                    ) {
                        self.display_mode = if self.display_mode == DisplayMode::Engineering {
                            DisplayMode::Decimal
                        } else {
                            DisplayMode::Engineering
                        };
                    }
                });

                ui.add_space(gap_y + 1.0);

                // ── Scientific function rows (6 columns × 3 rows) ──
                let sci_w = (avail_w - gap_x * 5.0) / 6.0;
                let sci_size = Vec2::new(sci_w, sci_h);

                let sin_lbl = if self.hyp { "sinh" } else { "sin" };
                let sin_sh = if self.hyp { "sinh⁻¹" } else { "sin⁻¹" };
                let cos_lbl = if self.hyp { "cosh" } else { "cos" };
                let cos_sh = if self.hyp { "cosh⁻¹" } else { "cos⁻¹" };
                let tan_lbl = if self.hyp { "tanh" } else { "tan" };
                let tan_sh = if self.hyp { "tanh⁻¹" } else { "tan⁻¹" };

                type SciKeySpec<'a> = (&'a str, &'a str, &'a str, &'a str, bool);
                let sci_rows: [[SciKeySpec<'_>; 6]; 3] = [
                    [
                        ("__FRAC_ICON__", "a b/c", "__FRAC__", "__MFRAC__", false),
                        ("x⁻¹", "x!", "^(-1)", "!", false),
                        ("√", "∛", "sqrt(", "cbrt(", true),
                        ("x²", "x³", "²", "³", false),
                        ("xʸ", "ʸ√x", "^", "^(1/", false),
                        ("log", "10ˣ", "log(", "10^(", true),
                    ],
                    [
                        ("ln", "eˣ", "ln(", "exp(", true),
                        ("(-)", "Abs", "-", "abs(", true),
                        (
                            sin_lbl,
                            sin_sh,
                            if self.hyp { "sinh(" } else { "sin(" },
                            if self.hyp { "asinh(" } else { "asin(" },
                            true,
                        ),
                        (
                            cos_lbl,
                            cos_sh,
                            if self.hyp { "cosh(" } else { "cos(" },
                            if self.hyp { "acosh(" } else { "acos(" },
                            true,
                        ),
                        (
                            tan_lbl,
                            tan_sh,
                            if self.hyp { "tanh(" } else { "tan(" },
                            if self.hyp { "atanh(" } else { "atan(" },
                            true,
                        ),
                        ("π", "e", "pi", "e", true),
                    ],
                    [
                        ("(", "%", "(", "%", true),
                        (")", ",", ")", ", ", false),
                        ("MC", "STO", "__MC__", "__STO__", false),
                        ("MR", "φ", "M", "phi", true),
                        ("M-", "τ", "__M-__", "tau", false),
                        ("M+", "log₂", "__M+__", "log2(", false),
                    ],
                ];

                for (s_idx, row) in sci_rows.into_iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap_x;
                        for (main_lbl, shift_lbl, main_ins, shift_ins, replace_on_commit) in row {
                            if paint_sci_key(
                                ui,
                                main_lbl,
                                shift_lbl,
                                sci_size,
                                KeyTone::Sci,
                                false,
                                self.shift,
                                scale,
                                palette,
                            ) {
                                let shifted = std::mem::take(&mut self.shift);
                                let tok = if shifted { shift_ins } else { main_ins };
                                match tok {
                                    "__FRAC__" => self.insert_fraction(false),
                                    "__MFRAC__" => self.insert_fraction(true),
                                    "__MC__" => self.memory = None,
                                    "__STO__" => {
                                        self.recompute();
                                        if let Some(Ok(v)) = &self.last_eval {
                                            self.memory = Some(v.clone());
                                        }
                                    }
                                    "__M+__" => self.mem_add(1),
                                    "__M-__" => self.mem_add(-1),
                                    _ => {
                                        self.insert_token(tok, replace_on_commit && !shifted);
                                        if matches!(
                                            tok,
                                            "sinh(" | "cosh(" | "tanh(" | "asinh(" | "acosh(" | "atanh("
                                        ) {
                                            self.hyp = false;
                                        }
                                    }
                                }
                            }
                        }
                    });
                    if s_idx + 1 < 3 {
                        ui.add_space(gap_y);
                    }
                }

                ui.add_space(gap_y + 1.0);
                style::hairline_h(ui, palette.border_subtle);
                ui.add_space(gap_y + 1.0);

                // ── Main numeric & operator keypad (5 columns × 4 rows) ──
                let num_w = (avail_w - gap_x * 4.0) / 5.0;
                let num_size = Vec2::new(num_w, num_h);

                type NumKeySpec<'a> = (&'a str, &'a str, &'a str, &'a str, KeyTone, bool);
                let num_rows: [[NumKeySpec<'_>; 5]; 4] = [
                    [
                        ("7", "nPr", "7", "nPr(", KeyTone::Digit, true),
                        ("8", "nCr", "8", "nCr(", KeyTone::Digit, true),
                        ("9", "gcd", "9", "gcd(", KeyTone::Digit, true),
                        ("DEL", "", "__DEL__", "__DEL__", KeyTone::Danger, false),
                        ("AC", "", "__AC__", "__AC__", KeyTone::Danger, false),
                    ],
                    [
                        ("4", "lcm", "4", "lcm(", KeyTone::Digit, true),
                        ("5", "hypot", "5", "hypot(", KeyTone::Digit, true),
                        ("6", "round", "6", "round(", KeyTone::Digit, true),
                        ("×", "xʸ", " * ", "^", KeyTone::Op, false),
                        ("÷", "mod", " / ", " mod ", KeyTone::Op, false),
                    ],
                    [
                        ("1", "floor", "1", "floor(", KeyTone::Digit, true),
                        ("2", "ceil", "2", "ceil(", KeyTone::Digit, true),
                        ("3", "abs", "3", "abs(", KeyTone::Digit, true),
                        ("+", "min", " + ", "min(", KeyTone::Op, false),
                        ("−", "max", " - ", "max(", KeyTone::Op, false),
                    ],
                    [
                        ("0", "Rnd", "0", "round(", KeyTone::Digit, true),
                        (".", "Ran#", ".", "__RAN__", KeyTone::Digit, false),
                        ("×10ˣ", "π", " * 10^", "pi", KeyTone::Digit, false),
                        ("Ans", "%", "ans", "%", KeyTone::Op, true),
                        ("=", "", "__EQ__", "__EQ__", KeyTone::Accent, false),
                    ],
                ];

                for (r_idx, row) in num_rows.into_iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap_x;
                        for (main_lbl, shift_lbl, main_ins, shift_ins, tone, replace_on_commit) in row {
                            if paint_sci_key(
                                ui,
                                main_lbl,
                                shift_lbl,
                                num_size,
                                tone,
                                false,
                                self.shift,
                                scale,
                                palette,
                            ) {
                                let shifted = std::mem::take(&mut self.shift);
                                let tok = if shifted && !shift_ins.is_empty() {
                                    shift_ins
                                } else {
                                    main_ins
                                };
                                match tok {
                                    "__DEL__" => self.backspace(),
                                    "__AC__" => self.clear_all(),
                                    "__EQ__" => self.evaluate_commit(precision),
                                    "__RAN__" => {
                                        let r = self.next_random();
                                        self.insert_token(&r, true);
                                    }
                                    _ => self.insert_token(tok, replace_on_commit),
                                }
                            }
                        }
                    });
                    if r_idx + 1 < 4 {
                        ui.add_space(gap_y);
                    }
                }
            });
    }

    fn show_history_panel(
        &mut self,
        ui: &mut Ui,
        side_w: f32,
        side_h: f32,
        s: &crate::i18n::Strings,
        palette: &Palette,
        action: &mut SciAction,
    ) {
        Frame::new()
            .fill(palette.bg_mantle)
            .stroke(Stroke::new(1.0, palette.border_subtle))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_min_size(Vec2::new((side_w - 28.0).max(160.0), (side_h - 28.0).max(200.0)));

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(s.sci_history)
                            .size(13.5)
                            .strong()
                            .color(palette.text_secondary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !self.history.is_empty()
                            && ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(s.nb_clear)
                                            .size(12.0)
                                            .color(palette.text_muted),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                        {
                            self.history.clear();
                            self.history_cursor = None;
                            self.state_dirty = true;
                        }
                    });
                });

                ui.add_space(8.0);
                style::hairline_h(ui, palette.border_subtle);
                ui.add_space(8.0);

                if self.history.is_empty() {
                    ui.label(
                        egui::RichText::new(s.sci_history_empty)
                            .size(12.5)
                            .color(palette.text_muted),
                    );
                    return;
                }

                let list_h = (side_h - 80.0).max(120.0);
                let mut pick_expr: Option<String> = None;

                ScrollArea::vertical()
                    .id_salt("sci_history_scroll")
                    .max_height(list_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for item in &self.history {
                            Frame::new()
                                .fill(palette.bg_surface)
                                .stroke(Stroke::new(1.0, palette.border_subtle))
                                .corner_radius(CornerRadius::same(8))
                                .inner_margin(Margin::symmetric(10, 8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    let expr_resp = ui
                                        .add(
                                            egui::Label::new(
                                                egui::RichText::new(&item.expr)
                                                    .monospace()
                                                    .size(13.0)
                                                    .color(palette.text_secondary),
                                            )
                                            .sense(Sense::click()),
                                        )
                                        .on_hover_cursor(CursorIcon::PointingHand);
                                    if expr_resp.clicked() {
                                        pick_expr = Some(item.expr.clone());
                                    }

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let res_resp = ui
                                            .add(
                                                egui::Label::new(
                                                    egui::RichText::new(format!("= {}", item.result_display))
                                                        .monospace()
                                                        .size(15.0)
                                                        .strong()
                                                        .color(palette.success),
                                                )
                                                .sense(Sense::click()),
                                            )
                                            .on_hover_cursor(CursorIcon::PointingHand);
                                        if res_resp.clicked() {
                                            *action = SciAction::Copy {
                                                text: item.result_plain.clone(),
                                                label: item.result_display.clone(),
                                            };
                                        }
                                    });
                                });
                            ui.add_space(6.0);
                        }
                    });

                if let Some(e) = pick_expr {
                    self.expr = e;
                    self.cursor = self.expr.len();
                    self.committed = false;
                    self.recompute();
                }
            });
    }
}

fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map_or(s.len(), |(b, _)| b)
}

fn paint_status_tag(
    ui: &mut Ui,
    label: &str,
    active: bool,
    color: Color32,
    font_size: f32,
    palette: &Palette,
) -> egui::Response {
    let bg = if active {
        Palette::alpha(color, 44)
    } else {
        Palette::alpha(palette.bg_surface, 140)
    };
    let fg = if active {
        color
    } else {
        Palette::alpha(palette.text_muted, 110)
    };
    let stroke = if active {
        Stroke::new(1.0, Palette::alpha(color, 150))
    } else {
        Stroke::new(1.0, palette.border_subtle)
    };
    style::paint_pill(ui, label, bg, fg, stroke, font_size)
}

#[allow(clippy::too_many_arguments)]
fn paint_sci_key(
    ui: &mut Ui,
    main_label: &str,
    shift_label: &str,
    size: Vec2,
    tone: KeyTone,
    active: bool,
    shift_active: bool,
    scale: f32,
    palette: &Palette,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let hover_t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.09);
    let press_t = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("press"), resp.is_pointer_button_down_on(), 0.06);

    let (base_bg, base_fg, base_border) = match tone {
        KeyTone::Digit => (palette.bg_elevated, palette.text_primary, palette.border_subtle),
        KeyTone::Sci => (palette.bg_surface, palette.text_primary, palette.border_subtle),
        KeyTone::Op => (
            Palette::alpha(palette.accent_alt, if palette.dark { 36 } else { 30 }),
            palette.accent_alt,
            Palette::alpha(palette.accent_alt, 95),
        ),
        KeyTone::Danger => (
            Palette::alpha(palette.error, if palette.dark { 195 } else { 215 }),
            Color32::WHITE,
            palette.error,
        ),
        KeyTone::Accent => (palette.accent, palette.accent_fg, palette.accent),
        KeyTone::Shift => (
            if active {
                palette.warning
            } else {
                Palette::alpha(palette.warning, 34)
            },
            if active {
                Color32::from_rgb(0x11, 0x11, 0x1B)
            } else {
                palette.warning
            },
            Palette::alpha(palette.warning, if active { 230 } else { 120 }),
        ),
    };

    let bg = if active && tone != KeyTone::Shift {
        Palette::alpha(palette.accent, 52)
    } else {
        base_bg
    };
    let bg = Palette::lerp(bg, Color32::WHITE, hover_t * 0.08);
    let bg = Palette::lerp(bg, Color32::BLACK, press_t * 0.14);
    let border = if active {
        Stroke::new(1.2, palette.accent)
    } else if resp.hovered() {
        Stroke::new(1.0, Palette::lerp(base_border, palette.accent, 0.55))
    } else {
        Stroke::new(1.0, base_border)
    };

    let draw_rect = rect.shrink(press_t * 0.6);
    let radius = (8.0 * scale).clamp(6.0, 12.0) as u8;
    ui.painter().rect(
        draw_rect,
        CornerRadius::same(radius),
        bg,
        border,
        egui::StrokeKind::Inside,
    );

    if shift_label.is_empty() {
        let base_fs = match tone {
            KeyTone::Digit | KeyTone::Accent => 17.0,
            KeyTone::Op => 18.0,
            KeyTone::Danger => 14.5,
            _ => 12.5,
        };
        ui.painter().text(
            draw_rect.center(),
            Align2::CENTER_CENTER,
            main_label,
            FontId::proportional((base_fs * scale).clamp(11.0, 26.0)),
            base_fg,
        );
    } else {
        let shift_col = if shift_active {
            palette.warning
        } else {
            Palette::alpha(palette.warning, if palette.dark { 170 } else { 205 })
        };
        let shift_fs = ((if shift_active { 10.5 } else { 9.5 }) * scale).clamp(9.0, 15.0);
        let top_y = draw_rect.top() + draw_rect.height() * 0.24;
        ui.painter().text(
            Pos2::new(draw_rect.center().x, top_y),
            Align2::CENTER_CENTER,
            shift_label,
            FontId::proportional(shift_fs),
            shift_col,
        );

        let main_col = if shift_active {
            Palette::alpha(base_fg, 130)
        } else {
            base_fg
        };
        let bot_y = draw_rect.top() + draw_rect.height() * 0.68;

        if main_label == "__FRAC_ICON__" {
            math_display::paint_fraction_key_icon(
                ui.painter(),
                Pos2::new(draw_rect.center().x, bot_y),
                scale * 0.85,
                main_col,
            );
        } else {
            let main_base = match tone {
                KeyTone::Digit => 16.5,
                KeyTone::Op => 17.5,
                _ => 13.5,
            };
            ui.painter().text(
                Pos2::new(draw_rect.center().x, bot_y),
                Align2::CENTER_CENTER,
                main_label,
                FontId::proportional((main_base * scale).clamp(11.5, 25.0)),
                main_col,
            );
        }
    }

    resp.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_77_stays_in_numerator_until_moved() {
        let mut v = ScientificView::default();
        v.insert_fraction(false);
        assert_eq!(v.expr, "()/()");
        assert_eq!(v.cursor, 1);

        v.insert_token("7", true);
        v.insert_token("7", true);
        assert_eq!(v.expr, "(77)/()");
        assert_eq!(v.cursor, 3);

        assert!(v.jump_fraction_down());
        v.insert_token("1", true);
        v.insert_token("2", true);
        assert_eq!(v.expr, "(77)/(12)");

        // Exit denominator and add another fraction
        assert!(v.jump_fraction_down());
        assert_eq!(v.cursor, v.expr.len());
    }

    #[test]
    fn fraction_after_digits_wraps_numerator() {
        let mut v = ScientificView::default();
        v.insert_token("7", true);
        v.insert_token("7", true);
        v.insert_fraction(false);
        assert_eq!(v.expr, "(77)/()");
        v.insert_token("5", true);
        assert_eq!(v.expr, "(77)/(5)");
    }

    #[test]
    fn smart_backspace_unwraps_and_deletes_empty_fractions() {
        let mut v = ScientificView::default();
        v.insert_fraction(false);
        assert_eq!(v.expr, "()/()");
        v.backspace();
        assert_eq!(v.expr, "");

        v.insert_token("77", true);
        v.insert_fraction(false);
        assert_eq!(v.expr, "(77)/()");
        v.backspace();
        assert_eq!(v.expr, "77");
        assert_eq!(v.cursor, 2);
    }

    #[test]
    fn nested_fraction_navigation_and_eval() {
        let mut v = ScientificView::default();
        v.insert_fraction(false);
        v.insert_token("2", true);
        v.insert_fraction(false);
        v.insert_token("5", true);
        assert_eq!(v.expr, "((2)/(5))/()");
        assert!(v.jump_fraction_down()); // exit inner denominator -> end of outer numerator
        assert!(v.jump_fraction_down()); // jump into outer denominator
        v.insert_token("4", true);
        assert_eq!(v.expr, "((2)/(5))/(4)");
        let val = v.last_eval.as_ref().expect("eval").as_ref().expect("ok");
        assert_eq!(decimal_to_fraction(val.si, 10_000), Some((1, 10)));
    }
}
