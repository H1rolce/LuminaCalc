use crate::i18n::{Lang, Strings, format_error, tr};
use crate::math_display::{self, MathNode, PaintFeedback};
use crate::state::SavedSciHistoryItem;
use crate::style::{self, Palette};
use egui::{
    Align2, Color32, Context, CornerRadius, CursorIcon, Event, FontId, Frame, Id, Key, Margin,
    Painter, Pos2, ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2,
};
use lumina_core::{
    AngleUnit, CalcError, Env, ErrorCategory, FormatOptions, Value, auto_close_parens,
    decimal_to_fraction, eval_str, format_engineering,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayMode {
    Fraction,
    MixedFraction,
    Decimal,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterCommit {
    Replace,
    ChainAns,
    Keep,
}

#[derive(Debug, Clone, Copy)]
enum KeyLabel {
    Text(&'static str),
    FractionIcon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyAction {
    Insert { token: &'static str, after_commit: AfterCommit },
    InsertHyp { token: &'static str, after_commit: AfterCommit },
    OpenParen,
    CloseParen,
    Fraction { mixed: bool },
    MemClear,
    MemStore,
    MemAdd(i8),
    Backspace,
    ClearAll,
    Evaluate,
    Random,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SciHotkey {
    Key(KeyAction),
    ToggleSd { shift: bool },
}

#[derive(Debug, Clone, Copy)]
struct KeySpec {
    main_label: KeyLabel,
    shift_label: &'static str,
    main: KeyAction,
    shift: KeyAction,
    tone: KeyTone,
    tooltip: &'static str,
}

impl KeySpec {
    const fn sci(
        main_label: &'static str,
        shift_label: &'static str,
        main: KeyAction,
        shift: KeyAction,
    ) -> Self {
        Self {
            main_label: KeyLabel::Text(main_label),
            shift_label,
            main,
            shift,
            tone: KeyTone::Sci,
            tooltip: "",
        }
    }

    const fn num(
        main_label: &'static str,
        shift_label: &'static str,
        main: KeyAction,
        shift: KeyAction,
        tone: KeyTone,
    ) -> Self {
        Self {
            main_label: KeyLabel::Text(main_label),
            shift_label,
            main,
            shift,
            tone,
            tooltip: "",
        }
    }

    const fn with_tip(mut self, tooltip: &'static str) -> Self {
        self.tooltip = tooltip;
        self
    }
}

const fn ins_new(token: &'static str) -> KeyAction {
    KeyAction::Insert { token, after_commit: AfterCommit::Replace }
}

const fn ins_chain(token: &'static str) -> KeyAction {
    KeyAction::Insert { token, after_commit: AfterCommit::ChainAns }
}

const fn ins_keep(token: &'static str) -> KeyAction {
    KeyAction::Insert { token, after_commit: AfterCommit::Keep }
}

const fn ins_hyp(token: &'static str) -> KeyAction {
    KeyAction::InsertHyp { token, after_commit: AfterCommit::Replace }
}

struct CachedMathTree {
    expr: String,
    tree: MathNode,
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
    pub cursor: usize,
    pub angle_unit: AngleUnit,
    display_mode: DisplayMode,
    shift: bool,
    hyp: bool,
    ans: Option<Value>,
    memory: Option<Value>,
    committed: bool,
    last_eval: Option<Result<Value, CalcError>>,
    math_tree_cache: Option<CachedMathTree>,
    history: Vec<HistoryItem>,
    history_cursor: Option<usize>,
    prng_state: u64,
    const_menu_open: bool,
    state_dirty: bool,
    undo_stack: Vec<(String, usize)>,
}

impl Default for ScientificView {
    fn default() -> Self {
        Self::new(&[], 0)
    }
}

impl ScientificView {
    pub fn new(saved_history: &[SavedSciHistoryItem], saved_angle_unit: u8) -> Self {
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
            angle_unit: AngleUnit::from_code(saved_angle_unit),
            display_mode: DisplayMode::Fraction,
            shift: false,
            hyp: false,
            ans: None,
            memory: None,
            committed: false,
            last_eval: None,
            math_tree_cache: None,
            history,
            history_cursor: None,
            prng_state: 0x9E37_79B9_7F4A_7C15,
            const_menu_open: false,
            state_dirty: false,
            undo_stack: Vec::new(),
        }
    }

    fn push_undo(&mut self) {
        if self
            .undo_stack
            .last()
            .is_some_and(|(e, c)| *e == self.expr && *c == self.cursor)
        {
            return;
        }
        if self.undo_stack.len() >= 64 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push((self.expr.clone(), self.cursor));
    }

    fn undo(&mut self) {
        if let Some((expr, cursor)) = self.undo_stack.pop() {
            self.expr = expr;
            self.cursor = cursor;
            self.clamp_cursor();
            self.committed = false;
            self.history_cursor = None;
            self.recompute();
        }
    }

    fn toggle_sd(&mut self, mixed_toggle: bool) {
        if mixed_toggle {
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

    fn paste_text(&mut self, raw: &str) {
        let cleaned: String = raw
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        if cleaned.is_empty() {
            return;
        }
        self.insert_token_mode(&cleaned, AfterCommit::Replace);
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
        (hist, self.angle_unit.to_code())
    }

    fn ensure_math_tree(&mut self) -> &MathNode {
        if self.math_tree_cache.as_ref().is_some_and(|c| c.expr != self.expr) {
            self.math_tree_cache = None;
        }
        let expr = &self.expr;
        &self
            .math_tree_cache
            .get_or_insert_with(|| CachedMathTree {
                expr: expr.clone(),
                tree: math_display::parse_math(expr),
            })
            .tree
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
        let eval_input = if trimmed.ends_with('(') {
            std::borrow::Cow::Borrowed(trimmed)
        } else {
            auto_close_parens(trimmed)
        };
        self.last_eval = Some(eval_str(&eval_input, &env));
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

    pub fn insert_token(&mut self, token: &str, replace_if_committed: bool) {
        let mode = if replace_if_committed {
            AfterCommit::Replace
        } else {
            AfterCommit::Keep
        };
        self.insert_token_mode(token, mode);
    }

    fn insert_token_mode(&mut self, token: &str, mode: AfterCommit) {
        self.push_undo();
        if self.committed {
            match mode {
                AfterCommit::Replace => {
                    self.expr.clear();
                    self.cursor = 0;
                }
                AfterCommit::ChainAns if self.ans.is_some() => {
                    self.expr.clear();
                    self.expr.push_str("Ans");
                    self.cursor = self.expr.len();
                }
                AfterCommit::ChainAns | AfterCommit::Keep => {}
            }
        }
        self.committed = false;
        self.history_cursor = None;
        self.clamp_cursor();

        let unclosed_parens = token
            .bytes()
            .filter(|&b| b == b'(')
            .count()
            .saturating_sub(token.bytes().filter(|&b| b == b')').count());

        if unclosed_parens > 0 && self.enclosing_fraction().is_some() {
            let mut balanced = String::with_capacity(token.len() + unclosed_parens);
            balanced.push_str(token);
            for _ in 0..unclosed_parens {
                balanced.push(')');
            }
            self.expr.insert_str(self.cursor, &balanced);
            self.cursor += token.len();
        } else {
            self.expr.insert_str(self.cursor, token);
            self.cursor += token.len();
        }
        self.recompute();
    }

    fn operand_start_before_cursor(&self) -> Option<usize> {
        let before = &self.expr[..self.cursor];
        let trimmed_end = before.trim_end().len();
        if trimmed_end == 0 {
            return None;
        }
        let slice = &before[..trimmed_end];
        let last = slice.chars().next_back()?;

        if last == ')' {
            let b = find_matching_open_paren(slice, slice.len() - 1)?;
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

    pub fn insert_fraction(&mut self, mixed: bool) {
        self.push_undo();
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
            } else {
                let tpl = " + ()/()";
                self.expr.insert_str(self.cursor, tpl);
                self.cursor += 4;
            }
            self.recompute();
            return;
        }

        if let Some(op_start) = self.operand_start_before_cursor() {
            let trimmed_end = self.expr[..self.cursor].trim_end().len();
            let operand = self.expr[op_start..trimmed_end].to_owned();
            let replacement = format!("({operand})/()");
            let den_cursor = op_start + replacement.len() - 1;
            self.expr.replace_range(op_start..self.cursor, &replacement);
            self.cursor = den_cursor;
        } else {
            self.expr.insert_str(self.cursor, "()/()");
            self.cursor += 1;
        }
        self.recompute();
    }

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

    pub fn insert_open_paren(&mut self) {
        self.push_undo();
        if self.committed {
            self.expr.clear();
            self.cursor = 0;
        }
        self.committed = false;
        self.history_cursor = None;
        self.clamp_cursor();

        if self.enclosing_fraction().is_some() {
            self.expr.insert_str(self.cursor, "()");
            self.cursor += 1;
        } else {
            self.expr.insert(self.cursor, '(');
            self.cursor += 1;
        }
        self.recompute();
    }

    pub fn insert_close_paren(&mut self) {
        self.clamp_cursor();
        self.committed = false;
        self.history_cursor = None;

        if self.expr[self.cursor..].starts_with(')')
            && let Some((open, close)) = self.enclosing_parens_at(self.cursor)
            && close == self.cursor
        {
            let is_frac_delim = self.expr[close..].starts_with(")/(")
                || (open >= 2 && self.expr[..open + 1].ends_with(")/("));
            if !is_frac_delim {
                self.cursor += 1;
                self.recompute();
                return;
            }
        }

        let mut depth = 0i32;
        for ch in self.expr[..self.cursor].chars() {
            match ch {
                '(' => depth += 1,
                ')' => depth = (depth - 1).max(0),
                _ => {}
            }
        }
        let min_depth = if self.enclosing_fraction().is_some() { 1 } else { 0 };
        if depth > min_depth {
            self.push_undo();
            self.expr.insert(self.cursor, ')');
            self.cursor += 1;
            self.recompute();
        }
    }

    fn enclosing_parens_at(&self, pos: usize) -> Option<(usize, usize)> {
        if pos > self.expr.len() || !self.expr.is_char_boundary(pos) {
            return None;
        }
        let mut depth = 0i32;
        let mut open_pos = None;
        for (b, c) in self.expr[..pos].char_indices().rev() {
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

    fn enclosing_fraction(&self) -> Option<(usize, usize)> {
        let mut probe = self.cursor;
        while let Some((open, close)) = self.enclosing_parens_at(probe) {
            let is_num = self.expr[close..].starts_with(")/(");
            let is_den = open >= 2 && self.expr[..open + 1].ends_with(")/(");
            if is_num || is_den {
                return Some((open, close));
            }
            probe = open;
        }
        None
    }

    pub fn jump_fraction_down(&mut self) -> bool {
        self.clamp_cursor();
        self.committed = false;

        if let Some((open, close)) = self.enclosing_fraction() {
            if self.expr[close..].starts_with(")/(") {
                let den_open = close + 2;
                let mut d = 0i32;
                for (rel, c) in self.expr[den_open..].char_indices() {
                    match c {
                        '(' => d += 1,
                        ')' => {
                            d -= 1;
                            if d == 0 {
                                self.cursor = den_open + rel;
                                return true;
                            }
                        }
                        _ => {}
                    }
                }
                self.cursor = (close + 3).min(self.expr.len());
                self.clamp_cursor();
                return true;
            }
            if open >= 2 && self.expr[..open + 1].ends_with(")/(") {
                self.cursor = (close + 1).min(self.expr.len());
                self.clamp_cursor();
                return true;
            }
        }

        if let Some(rel) = self.expr[self.cursor..].find("()") {
            self.cursor += rel + 1;
            self.clamp_cursor();
            return true;
        }
        false
    }

    pub fn jump_fraction_up(&mut self) -> bool {
        self.clamp_cursor();
        self.committed = false;

        if let Some((open, _close)) = self.enclosing_fraction()
            && open >= 2
            && self.expr[..open + 1].ends_with(")/(")
        {
            self.cursor = open - 2;
            self.clamp_cursor();
            return true;
        }
        false
    }

    pub fn backspace(&mut self) {
        self.clamp_cursor();
        self.committed = false;
        self.history_cursor = None;

        if self.cursor == 0 {
            return;
        }
        self.push_undo();

        if self.expr[..self.cursor].ends_with('(')
            && self.expr[self.cursor..].starts_with(")/()")
        {
            self.expr.replace_range(self.cursor - 1..self.cursor + 4, "");
            self.cursor -= 1;
            self.recompute();
            return;
        }

        if self.cursor >= 3
            && self.expr[..self.cursor].ends_with(")/(")
            && self.expr[self.cursor..].starts_with(')')
        {
            let num_close = self.cursor - 3;
            if let Some(open) = find_matching_open_paren(&self.expr, num_close) {
                let inner = self.expr[open + 1..num_close].to_owned();
                let new_cursor = open + inner.len();
                self.expr.replace_range(open..=self.cursor, &inner);
                self.cursor = new_cursor;
                self.recompute();
                return;
            }
        }

        if self.cursor >= 1
            && self.expr[..self.cursor].ends_with('(')
            && self.expr[self.cursor..].starts_with(')')
        {
            let del_len = smart_token_len_before(&self.expr[..self.cursor]).max(1);
            let start = self.cursor.saturating_sub(del_len);
            if self.expr.is_char_boundary(start) {
                self.expr.replace_range(start..self.cursor + 1, "");
                self.cursor = start;
                self.recompute();
                return;
            }
        }

        if self.expr[..self.cursor].ends_with(')') {
            let close = self.cursor - 1;
            if let Some(open) = find_matching_open_paren(&self.expr, close)
                && open >= 2
                && self.expr[..open + 1].ends_with(")/(")
            {
                self.cursor = close;
                if close == open + 1 {
                    self.backspace();
                } else if let Some((prev_b, _)) = self.expr[..self.cursor].char_indices().next_back() {
                    self.expr.replace_range(prev_b..self.cursor, "");
                    self.cursor = prev_b;
                    self.recompute();
                }
                return;
            }
        }

        let del_len = smart_token_len_before(&self.expr[..self.cursor]);
        if del_len > 0 && del_len <= self.cursor {
            let start = self.cursor - del_len;
            if self.expr.is_char_boundary(start) {
                self.expr.replace_range(start..self.cursor, "");
                self.cursor = start;
                self.recompute();
            }
        }
    }

    fn delete_forward(&mut self) {
        self.clamp_cursor();
        self.committed = false;
        if let Some(ch) = self.expr[self.cursor..].chars().next() {
            self.push_undo();
            self.expr.replace_range(self.cursor..self.cursor + ch.len_utf8(), "");
            self.recompute();
        }
    }

    pub fn clear_all(&mut self) {
        if !self.expr.is_empty() {
            self.push_undo();
        }
        self.expr.clear();
        self.cursor = 0;
        self.committed = false;
        self.shift = false;
        self.hyp = false;
        self.history_cursor = None;
        self.last_eval = None;
    }

    pub fn evaluate_commit(&mut self, precision: u32) {
        self.clamp_cursor();
        let trimmed = self.expr.trim();
        if !trimmed.is_empty() && !self.has_empty_slot() {
            let closed = auto_close_parens(trimmed);
            if matches!(closed, std::borrow::Cow::Owned(_)) {
                let env = self.build_env();
                if eval_str(&closed, &env).is_ok() {
                    self.expr = closed.into_owned();
                    self.cursor = self.expr.len();
                }
            }
        }
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

    fn execute_sci_hotkey(&mut self, hk: SciHotkey, precision: u32) {
        match hk {
            SciHotkey::Key(act) => self.execute_key_action(act, precision),
            SciHotkey::ToggleSd { shift } => self.toggle_sd(shift),
        }
    }

    fn handle_keyboard(&mut self, ui: &Ui, input_id: Id, input_focused: bool, precision: u32) {
        if input_focused {
            let mut hotkeys: Vec<SciHotkey> = Vec::new();
            ui.ctx().input_mut(|i| {
                let old_events = std::mem::take(&mut i.events);
                let mut kept = Vec::with_capacity(old_events.len());
                for ev in old_events {
                    match ev {
                        Event::Key { key, pressed: true, modifiers, .. }
                            if let Some(hk) = map_sci_hotkey(key, modifiers) =>
                        {
                            hotkeys.push(hk);
                        }
                        Event::Text(text) => {
                            let filtered: String = text
                                .chars()
                                .filter(|&c| is_allowed_sci_char(c))
                                .collect();
                            if !filtered.is_empty() {
                                kept.push(Event::Text(filtered));
                            }
                        }
                        other => kept.push(other),
                    }
                }
                i.events = kept;
            });

            if !hotkeys.is_empty() {
                for hk in hotkeys {
                    self.execute_sci_hotkey(hk, precision);
                }
                let char_pos = byte_to_char_idx(&self.expr, self.cursor);
                if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), input_id) {
                    let c = egui::text::CCursor::new(char_pos);
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(c)));
                    state.store(ui.ctx(), input_id);
                }
            }
            return;
        }

        enum DirectEvent {
            Hotkey(SciHotkey),
            Char(char),
            Paste(String),
            Copy,
            Cut,
            Undo,
            SelectAll,
            Home,
            End,
            Enter,
            Backspace,
            Delete,
            Left,
            Right,
            Up,
            Down,
            Tab,
        }

        let mut queue: Vec<DirectEvent> = Vec::new();
        ui.ctx().input(|i| {
            let ctrl_held = i.modifiers.ctrl || i.modifiers.command || i.modifiers.alt;
            for ev in &i.events {
                match ev {
                    Event::Key { key, pressed: true, modifiers, .. } => {
                        if let Some(hk) = map_sci_hotkey(*key, *modifiers) {
                            queue.push(DirectEvent::Hotkey(hk));
                            continue;
                        }
                        let ctrl = modifiers.ctrl || modifiers.command;
                        if ctrl && !modifiers.alt && !modifiers.shift {
                            match key {
                                Key::Z => queue.push(DirectEvent::Undo),
                                Key::A => queue.push(DirectEvent::SelectAll),
                                _ => {}
                            }
                            continue;
                        }
                        if !ctrl && !modifiers.alt {
                            match key {
                                Key::Enter => queue.push(DirectEvent::Enter),
                                Key::Backspace => queue.push(DirectEvent::Backspace),
                                Key::Delete => queue.push(DirectEvent::Delete),
                                Key::ArrowLeft => queue.push(DirectEvent::Left),
                                Key::ArrowRight => queue.push(DirectEvent::Right),
                                Key::ArrowUp => queue.push(DirectEvent::Up),
                                Key::ArrowDown => queue.push(DirectEvent::Down),
                                Key::Tab => queue.push(DirectEvent::Tab),
                                Key::Home => queue.push(DirectEvent::Home),
                                Key::End => queue.push(DirectEvent::End),
                                _ => {}
                            }
                        }
                    }
                    Event::Text(t) if !ctrl_held => {
                        for ch in t.chars() {
                            if is_allowed_sci_char(ch) {
                                queue.push(DirectEvent::Char(ch));
                            }
                        }
                    }
                    Event::Paste(s) => queue.push(DirectEvent::Paste(s.clone())),
                    Event::Copy => queue.push(DirectEvent::Copy),
                    Event::Cut => queue.push(DirectEvent::Cut),
                    _ => {}
                }
            }
        });

        for act in queue {
            match act {
                DirectEvent::Copy => {
                    if !self.expr.is_empty() {
                        ui.ctx().copy_text(self.expr.clone());
                    }
                }
                DirectEvent::Cut => {
                    if !self.expr.is_empty() {
                        ui.ctx().copy_text(self.expr.clone());
                        self.push_undo();
                        self.expr.clear();
                        self.cursor = 0;
                        self.committed = false;
                        self.recompute();
                    }
                }
                DirectEvent::SelectAll => {
                    ui.ctx().memory_mut(|m| m.request_focus(input_id));
                    let char_len = self.expr.chars().count();
                    if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), input_id) {
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(char_len),
                        )));
                        state.store(ui.ctx(), input_id);
                    }
                }
                DirectEvent::Undo => self.undo(),
                DirectEvent::Hotkey(hk) => self.execute_sci_hotkey(hk, precision),
                DirectEvent::Paste(s) => self.paste_text(&s),
                DirectEvent::Char(ch) => match ch {
                    '÷' => self.insert_token_mode(" ÷ ", AfterCommit::ChainAns),
                    '/' => {
                        if self.committed && self.ans.is_some() {
                            self.push_undo();
                            self.expr = "(Ans)/()".to_owned();
                            self.cursor = self.expr.len() - 1;
                            self.committed = false;
                            self.history_cursor = None;
                            self.recompute();
                        } else if self.enclosing_fraction().is_some() {
                            self.insert_token_mode(" ÷ ", AfterCommit::ChainAns);
                        } else {
                            self.insert_fraction(false);
                        }
                    }
                    '(' => self.insert_open_paren(),
                    ')' => self.insert_close_paren(),
                    _ => {
                        let mode = match ch {
                            '+' | '-' | '−' | '*' | '×' | '^' | '%' | '!' => AfterCommit::ChainAns,
                            ',' => AfterCommit::Keep,
                            _ => AfterCommit::Replace,
                        };
                        let mut buf = [0u8; 4];
                        self.insert_token_mode(ch.encode_utf8(&mut buf), mode);
                    }
                },
                DirectEvent::Home => {
                    self.cursor = 0;
                    self.committed = false;
                }
                DirectEvent::End => {
                    self.cursor = self.expr.len();
                    self.committed = false;
                }
                DirectEvent::Enter => self.evaluate_commit(precision),
                DirectEvent::Backspace => self.backspace(),
                DirectEvent::Delete => self.delete_forward(),
                DirectEvent::Left => self.move_left(),
                DirectEvent::Right => self.move_right(),
                DirectEvent::Down => {
                    if !self.jump_fraction_down() {
                        self.history_step(-1);
                    }
                }
                DirectEvent::Tab => {
                    let _ = self.jump_fraction_down();
                }
                DirectEvent::Up => {
                    if !self.jump_fraction_up() {
                        self.history_step(1);
                    }
                }
            }
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
            if self.const_menu_open {
                self.const_menu_open = false;
            } else {
                self.clear_all();
            }
        }
        if !self.const_menu_open {
            self.handle_keyboard(ui, input_id, input_focused, precision);
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

    fn execute_key_action(&mut self, act: KeyAction, precision: u32) {
        match act {
            KeyAction::Insert { token, after_commit } => {
                self.insert_token_mode(token, after_commit);
            }
            KeyAction::InsertHyp { token, after_commit } => {
                self.insert_token_mode(token, after_commit);
                self.hyp = false;
            }
            KeyAction::OpenParen => self.insert_open_paren(),
            KeyAction::CloseParen => self.insert_close_paren(),
            KeyAction::Fraction { mixed } => self.insert_fraction(mixed),
            KeyAction::MemClear => self.memory = None,
            KeyAction::MemStore => {
                self.recompute();
                if let Some(Ok(v)) = &self.last_eval {
                    self.memory = Some(v.clone());
                }
            }
            KeyAction::MemAdd(sign) => self.mem_add(sign),
            KeyAction::Backspace => self.backspace(),
            KeyAction::ClearAll => self.clear_all(),
            KeyAction::Evaluate => self.evaluate_commit(precision),
            KeyAction::Random => {
                let r = self.next_random();
                self.insert_token(&r, true);
            }
        }
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

                self.show_display_panel(
                    ui,
                    display_h,
                    scale,
                    input_id,
                    input_focused,
                    lang,
                    s,
                    precision,
                    palette,
                    action,
                );

                ui.add_space(gap_y + 2.0);

                let avail_w = ui.available_width();
                self.show_control_row(ui, avail_w, ctrl_h, gap_x, scale, lang, palette);

                ui.add_space(gap_y + 1.0);
                self.show_sci_keypad(ui, avail_w, sci_h, gap_x, gap_y, scale, precision, palette);

                ui.add_space(gap_y + 1.0);
                style::hairline_h(ui, palette.border_subtle);
                ui.add_space(gap_y + 1.0);

                self.show_num_keypad(ui, avail_w, num_h, gap_x, gap_y, scale, precision, palette);
            });
    }

    #[allow(clippy::too_many_arguments)]
    fn show_display_panel(
        &mut self,
        ui: &mut Ui,
        display_h: f32,
        scale: f32,
        input_id: Id,
        input_focused: bool,
        lang: Lang,
        s: &Strings,
        precision: u32,
        palette: &Palette,
        action: &mut SciAction,
    ) {
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
                    let cursor_byte = self.cursor;
                    let tree = self.ensure_math_tree();
                    let mut fs = (23.0 * scale).clamp(17.0, 34.0);
                    let max_w = (math_rect.width() - 10.0).max(80.0);
                    let max_h = (math_rect.height() - 4.0).max(32.0);

                    let mut m = math_display::measure(tree, ui.painter(), fs);
                    for _ in 0..4 {
                        let fit = (max_w / m.width.max(1.0)).min(max_h / m.height().max(1.0));
                        if fit >= 0.99 || fs <= 8.0 {
                            break;
                        }
                        fs = (fs * fit * 0.97).max(8.0);
                        m = math_display::measure(tree, ui.painter(), fs);
                    }

                    let top_y = math_rect.top() + (math_rect.height() - m.height()) * 0.5;
                    let mid_y = top_y + m.above;
                    let clipped = ui.painter().with_clip_rect(math_rect);
                    math_display::paint_interactive(
                        tree,
                        &clipped,
                        math_rect.left() + 6.0,
                        mid_y,
                        fs,
                        cursor_byte,
                        hover_pos,
                        palette,
                        &mut fb,
                    );

                    if !input_focused
                        && let Some(caret) = fb.caret
                    {
                        paint_blinking_caret(
                            ui.ctx(),
                            &clipped,
                            caret.x,
                            caret.top_y,
                            caret.bot_y,
                            palette,
                        );
                    }

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
                        paint_blinking_caret(
                            ui.ctx(),
                            ui.painter(),
                            caret_x,
                            math_rect.center().y - h * 0.5,
                            math_rect.center().y + h * 0.5,
                            palette,
                        );
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

                let err_ctx = self.last_eval.as_ref().and_then(|r| r.as_ref().err()).map(|e| {
                    let lead = self.expr.len().saturating_sub(self.expr.trim_start().len());
                    let start = e.span.start.saturating_add(lead).min(self.expr.len());
                    let end = e.span.end.saturating_add(lead).min(self.expr.len());
                    (self.expr.clone(), start..end)
                });
                let mut layouter = style::singleline_layouter(
                    false,
                    err_ctx.as_ref().map(|(src, span)| (src.as_str(), span)),
                    input_fs,
                    palette,
                );

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
                        let prefix = match (lang, err.kind.category()) {
                            (Lang::En, ErrorCategory::Syntax) => "Syntax ERROR",
                            (Lang::En, ErrorCategory::Math) => "Math ERROR",
                            (Lang::Ru, ErrorCategory::Syntax) => "Синтаксис",
                            (Lang::Ru, ErrorCategory::Math) => "Мат. ошибка",
                        };
                        let msg = format!("{prefix}: {}", format_error(lang, &err.kind));
                        ui.painter().text(
                            Pos2::new(res_rect.right() - 4.0, res_rect.center().y),
                            Align2::RIGHT_CENTER,
                            msg,
                            FontId::proportional((13.0 * scale).clamp(11.5, 17.5)),
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
    }

    #[allow(clippy::too_many_arguments)]
    fn show_control_row(
        &mut self,
        ui: &mut Ui,
        avail_w: f32,
        ctrl_h: f32,
        gap_x: f32,
        scale: f32,
        lang: Lang,
        palette: &Palette,
    ) {
        let ctrl_w = (avail_w - gap_x * 5.0) / 6.0;
        let ctrl_size = Vec2::new(ctrl_w, ctrl_h);
        let mut const_anchor: Option<egui::Rect> = None;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap_x;
            if paint_sci_key(ui, KeyLabel::Text("SHIFT"), "", ctrl_size, KeyTone::Shift, self.shift, false, scale, "", palette) {
                self.shift = !self.shift;
            }
            if paint_sci_key(ui, KeyLabel::Text("HYP"), "", ctrl_size, KeyTone::Sci, self.hyp, false, scale, "", palette) {
                self.hyp = !self.hyp;
            }
            if paint_sci_key(ui, KeyLabel::Text(self.angle_unit.label()), "", ctrl_size, KeyTone::Sci, false, false, scale, "", palette) {
                self.angle_unit = self.angle_unit.next();
                self.state_dirty = true;
                self.recompute();
            }
            if paint_sci_key(
                ui,
                KeyLabel::Text("S⇔D"),
                "a b/c",
                ctrl_size,
                KeyTone::Sci,
                matches!(self.display_mode, DisplayMode::Fraction | DisplayMode::MixedFraction),
                self.shift,
                scale,
                "D: S⇔D  •  Shift+D: a b/c",
                palette,
            ) {
                let shift_used = std::mem::take(&mut self.shift);
                self.toggle_sd(shift_used);
            }
            if paint_sci_key(
                ui,
                KeyLabel::Text("ENG"),
                "",
                ctrl_size,
                KeyTone::Sci,
                self.display_mode == DisplayMode::Engineering,
                false,
                scale,
                "",
                palette,
            ) {
                self.display_mode = if self.display_mode == DisplayMode::Engineering {
                    DisplayMode::Decimal
                } else {
                    DisplayMode::Engineering
                };
            }
            let const_start = ui.cursor().min;
            if paint_sci_key(
                ui,
                KeyLabel::Text("CONST"),
                "",
                ctrl_size,
                KeyTone::Sci,
                self.const_menu_open,
                false,
                scale,
                "",
                palette,
            ) {
                self.const_menu_open = !self.const_menu_open;
            }
            const_anchor = Some(egui::Rect::from_min_size(const_start, ctrl_size));
        });

        if self.const_menu_open
            && let Some(anchor) = const_anchor
        {
            self.show_const_popup(ui, anchor, lang, palette);
        }
    }

    fn show_const_popup(
        &mut self,
        ui: &Ui,
        anchor: egui::Rect,
        lang: Lang,
        palette: &Palette,
    ) {
        struct ConstEntry {
            symbol: &'static str,
            token: &'static str,
            ru: &'static str,
            en: &'static str,
            preview: &'static str,
        }
        const CONSTANTS: &[ConstEntry] = &[
            ConstEntry { symbol: "π", token: "pi", ru: "Число Пи", en: "Pi", preview: "3.14159265…" },
            ConstEntry { symbol: "e", token: "e", ru: "Число Эйлера", en: "Euler's e", preview: "2.71828182…" },
            ConstEntry { symbol: "τ", token: "tau", ru: "Тау (2π)", en: "Tau (2π)", preview: "6.28318530…" },
            ConstEntry { symbol: "φ", token: "phi", ru: "Золотое сечение", en: "Golden ratio", preview: "1.61803398…" },
            ConstEntry { symbol: "g", token: "9.80665", ru: "Ускорение своб. падения", en: "Standard gravity", preview: "9.80665" },
            ConstEntry { symbol: "c", token: "299792458", ru: "Скорость света", en: "Speed of light", preview: "299 792 458" },
            ConstEntry { symbol: "G", token: "6.6743e-11", ru: "Гравитационная пост.", en: "Gravitational const", preview: "6.6743e-11" },
            ConstEntry { symbol: "h", token: "6.62607015e-34", ru: "Постоянная Планка", en: "Planck constant", preview: "6.626e-34" },
            ConstEntry { symbol: "Nₐ", token: "6.02214076e23", ru: "Число Авогадро", en: "Avogadro const", preview: "6.022e23" },
            ConstEntry { symbol: "k", token: "1.380649e-23", ru: "Пост. Больцмана", en: "Boltzmann const", preview: "1.381e-23" },
            ConstEntry { symbol: "R", token: "8.314462618", ru: "Газовая постоянная", en: "Molar gas const", preview: "8.31446" },
            ConstEntry { symbol: "e₀", token: "1.602176634e-19", ru: "Элементарный заряд", en: "Elementary charge", preview: "1.602e-19" },
        ];

        let popup_w = 290.0f32;
        let viewport = ui.ctx().content_rect();
        let x = (anchor.right() - popup_w).clamp(viewport.left() + 8.0, (viewport.right() - popup_w - 8.0).max(viewport.left() + 8.0));
        let y = anchor.bottom() + 6.0;

        let mut picked: Option<&'static str> = None;
        let area_resp = egui::Area::new(Id::unique("lumina_sci_const_popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::new(x, y))
            .show(ui.ctx(), |ui| {
                Frame::new()
                    .fill(palette.bg_elevated)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(popup_w - 16.0);
                        ScrollArea::vertical()
                            .id_salt("sci_const_scroll")
                            .max_height(260.0)
                            .show(ui, |ui| {
                                for entry in CONSTANTS {
                                    let row_h = 28.0;
                                    let (rect, resp) = ui.allocate_exact_size(
                                        Vec2::new(ui.available_width(), row_h),
                                        Sense::click(),
                                    );
                                    if resp.hovered() {
                                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                        ui.painter().rect_filled(
                                            rect,
                                            CornerRadius::same(6),
                                            palette.bg_hover,
                                        );
                                    }
                                    ui.painter().text(
                                        Pos2::new(rect.left() + 8.0, rect.center().y),
                                        Align2::LEFT_CENTER,
                                        entry.symbol,
                                        FontId::monospace(13.5),
                                        palette.syn_constant,
                                    );
                                    let desc = if lang == Lang::Ru { entry.ru } else { entry.en };
                                    ui.painter().text(
                                        Pos2::new(rect.left() + 34.0, rect.center().y),
                                        Align2::LEFT_CENTER,
                                        desc,
                                        FontId::proportional(11.5),
                                        palette.text_primary,
                                    );
                                    ui.painter().text(
                                        Pos2::new(rect.right() - 8.0, rect.center().y),
                                        Align2::RIGHT_CENTER,
                                        entry.preview,
                                        FontId::monospace(11.0),
                                        palette.text_muted,
                                    );
                                    if resp.clicked() {
                                        picked = Some(entry.token);
                                    }
                                }
                            });
                    });
            });

        if let Some(tok) = picked {
            self.insert_token_mode(tok, AfterCommit::Replace);
            self.const_menu_open = false;
        } else if ui.ctx().input(|i| i.pointer.primary_pressed())
            && let Some(pos) = ui.ctx().input(|i| i.pointer.interact_pos())
            && !area_resp.response.rect.contains(pos)
            && !anchor.contains(pos)
        {
            self.const_menu_open = false;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn show_sci_keypad(
        &mut self,
        ui: &mut Ui,
        avail_w: f32,
        sci_h: f32,
        gap_x: f32,
        gap_y: f32,
        scale: f32,
        precision: u32,
        palette: &Palette,
    ) {
        let sci_size = Vec2::new((avail_w - gap_x * 5.0) / 6.0, sci_h);
        let hyp = self.hyp;
        let sci_rows: [[KeySpec; 6]; 3] = [
            [
                KeySpec {
                    main_label: KeyLabel::FractionIcon,
                    shift_label: "a b/c",
                    main: KeyAction::Fraction { mixed: false },
                    shift: KeyAction::Fraction { mixed: true },
                    tone: KeyTone::Sci,
                    tooltip: "/",
                },
                KeySpec::sci("x⁻¹", "x!", ins_chain("^(-1)"), ins_chain("!")),
                KeySpec::sci("√", "∛", ins_new("sqrt("), ins_new("cbrt(")).with_tip("R: √("),
                KeySpec::sci("x²", "x³", ins_chain("²"), ins_chain("³")).with_tip("Q: ²"),
                KeySpec::sci("xʸ", "ʸ√x", ins_chain("^"), ins_chain("^(1/")).with_tip("^"),
                KeySpec::sci("log", "10ˣ", ins_new("log("), ins_new("10^(")).with_tip("L: log("),
            ],
            [
                KeySpec::sci("ln", "eˣ", ins_new("ln("), ins_new("exp(")).with_tip("N: ln("),
                KeySpec::sci("(-)", "Abs", ins_new("-"), ins_new("abs(")),
                KeySpec::sci(
                    if hyp { "sinh" } else { "sin" },
                    if hyp { "sinh⁻¹" } else { "sin⁻¹" },
                    if hyp { ins_hyp("sinh(") } else { ins_new("sin(") },
                    if hyp { ins_hyp("asinh(") } else { ins_new("asin(") },
                )
                .with_tip("S: sin(  •  Shift+S: asin(  •  Ctrl+S: sinh("),
                KeySpec::sci(
                    if hyp { "cosh" } else { "cos" },
                    if hyp { "cosh⁻¹" } else { "cos⁻¹" },
                    if hyp { ins_hyp("cosh(") } else { ins_new("cos(") },
                    if hyp { ins_hyp("acosh(") } else { ins_new("acos(") },
                )
                .with_tip("O: cos(  •  Shift+O: acos(  •  Ctrl+O: cosh("),
                KeySpec::sci(
                    if hyp { "tanh" } else { "tan" },
                    if hyp { "tanh⁻¹" } else { "tan⁻¹" },
                    if hyp { ins_hyp("tanh(") } else { ins_new("tan(") },
                    if hyp { ins_hyp("atanh(") } else { ins_new("atan(") },
                )
                .with_tip("T: tan(  •  Shift+T: atan(  •  Ctrl+T: tanh("),
                KeySpec::sci("π", "e", ins_new("pi"), ins_new("e")).with_tip("P: π  •  E: e"),
            ],
            [
                KeySpec::sci("(", "%", KeyAction::OpenParen, ins_chain("%")).with_tip("("),
                KeySpec::sci(")", ",", KeyAction::CloseParen, ins_keep(", ")).with_tip(")"),
                KeySpec::sci("MC", "STO", KeyAction::MemClear, KeyAction::MemStore),
                KeySpec::sci("MR", "φ", ins_new("M"), ins_new("phi")),
                KeySpec::sci("M-", "τ", KeyAction::MemAdd(-1), ins_new("tau")),
                KeySpec::sci("M+", "log₂", KeyAction::MemAdd(1), ins_new("log2(")),
            ],
        ];

        for (s_idx, row) in sci_rows.into_iter().enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap_x;
                for spec in row {
                    if paint_sci_key(
                        ui,
                        spec.main_label,
                        spec.shift_label,
                        sci_size,
                        spec.tone,
                        false,
                        self.shift,
                        scale,
                        spec.tooltip,
                        palette,
                    ) {
                        let act = if std::mem::take(&mut self.shift) { spec.shift } else { spec.main };
                        self.execute_key_action(act, precision);
                    }
                }
            });
            if s_idx + 1 < 3 {
                ui.add_space(gap_y);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn show_num_keypad(
        &mut self,
        ui: &mut Ui,
        avail_w: f32,
        num_h: f32,
        gap_x: f32,
        gap_y: f32,
        scale: f32,
        precision: u32,
        palette: &Palette,
    ) {
        let num_size = Vec2::new((avail_w - gap_x * 4.0) / 5.0, num_h);
        const NUM_ROWS: [[KeySpec; 5]; 4] = [
            [
                KeySpec::num("7", "nPr", ins_new("7"), ins_new("nPr("), KeyTone::Digit),
                KeySpec::num("8", "nCr", ins_new("8"), ins_new("nCr("), KeyTone::Digit),
                KeySpec::num("9", "gcd", ins_new("9"), ins_new("gcd("), KeyTone::Digit),
                KeySpec::num("DEL", "", KeyAction::Backspace, KeyAction::Backspace, KeyTone::Danger)
                    .with_tip("Backspace"),
                KeySpec::num("AC", "", KeyAction::ClearAll, KeyAction::ClearAll, KeyTone::Danger)
                    .with_tip("Esc"),
            ],
            [
                KeySpec::num("4", "lcm", ins_new("4"), ins_new("lcm("), KeyTone::Digit),
                KeySpec::num("5", "hypot", ins_new("5"), ins_new("hypot("), KeyTone::Digit),
                KeySpec::num("6", "round", ins_new("6"), ins_new("round("), KeyTone::Digit),
                KeySpec::num("×", "xʸ", ins_chain(" * "), ins_chain("^"), KeyTone::Op),
                KeySpec::num("÷", "mod", ins_chain(" ÷ "), ins_chain(" mod "), KeyTone::Op),
            ],
            [
                KeySpec::num("1", "floor", ins_new("1"), ins_new("floor("), KeyTone::Digit),
                KeySpec::num("2", "ceil", ins_new("2"), ins_new("ceil("), KeyTone::Digit),
                KeySpec::num("3", "abs", ins_new("3"), ins_new("abs("), KeyTone::Digit),
                KeySpec::num("+", "min", ins_chain(" + "), ins_new("min("), KeyTone::Op),
                KeySpec::num("−", "max", ins_chain(" - "), ins_new("max("), KeyTone::Op),
            ],
            [
                KeySpec::num("0", "Rnd", ins_new("0"), ins_new("round("), KeyTone::Digit),
                KeySpec::num(".", "Ran#", ins_new("."), KeyAction::Random, KeyTone::Digit),
                KeySpec::num("×10ˣ", "π", ins_chain(" * 10^"), ins_new("pi"), KeyTone::Digit)
                    .with_tip("Shift+E: ×10ˣ  •  P: π"),
                KeySpec::num("Ans", "%", ins_new("Ans"), ins_chain("%"), KeyTone::Op),
                KeySpec::num("=", "", KeyAction::Evaluate, KeyAction::Evaluate, KeyTone::Accent)
                    .with_tip("Enter"),
            ],
        ];

        for (r_idx, row) in NUM_ROWS.into_iter().enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap_x;
                for spec in row {
                    if paint_sci_key(
                        ui,
                        spec.main_label,
                        spec.shift_label,
                        num_size,
                        spec.tone,
                        false,
                        self.shift,
                        scale,
                        spec.tooltip,
                        palette,
                    ) {
                        let act = if std::mem::take(&mut self.shift) { spec.shift } else { spec.main };
                        self.execute_key_action(act, precision);
                    }
                }
            });
            if r_idx + 1 < 4 {
                ui.add_space(gap_y);
            }
        }
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

fn map_sci_hotkey(key: Key, modifiers: egui::Modifiers) -> Option<SciHotkey> {
    if modifiers.alt {
        return None;
    }
    let ctrl = modifiers.ctrl || modifiers.command;
    let shift = modifiers.shift;
    match key {
        Key::S => match (ctrl, shift) {
            (false, false) => Some(SciHotkey::Key(ins_new("sin("))),
            (false, true) => Some(SciHotkey::Key(ins_new("asin("))),
            (true, false) => Some(SciHotkey::Key(ins_hyp("sinh("))),
            (true, true) => None,
        },
        Key::O => match (ctrl, shift) {
            (false, false) => Some(SciHotkey::Key(ins_new("cos("))),
            (false, true) => Some(SciHotkey::Key(ins_new("acos("))),
            (true, false) => Some(SciHotkey::Key(ins_hyp("cosh("))),
            (true, true) => None,
        },
        Key::T => match (ctrl, shift) {
            (false, false) => Some(SciHotkey::Key(ins_new("tan("))),
            (false, true) => Some(SciHotkey::Key(ins_new("atan("))),
            (true, false) => Some(SciHotkey::Key(ins_hyp("tanh("))),
            (true, true) => None,
        },
        Key::L if !ctrl && !shift => Some(SciHotkey::Key(ins_new("log("))),
        Key::N if !ctrl && !shift => Some(SciHotkey::Key(ins_new("ln("))),
        Key::R if !ctrl && !shift => Some(SciHotkey::Key(ins_new("sqrt("))),
        Key::Q if !ctrl && !shift => Some(SciHotkey::Key(ins_chain("²"))),
        Key::P if !ctrl && !shift => Some(SciHotkey::Key(ins_new("pi"))),
        Key::E if !ctrl && !shift => Some(SciHotkey::Key(ins_new("e"))),
        Key::E if !ctrl && shift => Some(SciHotkey::Key(ins_chain(" * 10^"))),
        Key::D if !ctrl => Some(SciHotkey::ToggleSd { shift }),
        _ => None,
    }
}

fn is_allowed_sci_char(ch: char) -> bool {
    ch.is_ascii_digit()
        || matches!(
            ch,
            '.' | ',' | '+' | '-' | '−' | '*' | '×' | '/' | '÷' | '(' | ')' | '^' | '%' | '!' | ' '
        )
}

fn byte_to_char_idx(s: &str, byte_idx: usize) -> usize {
    let clamped = byte_idx.min(s.len());
    let mut b = clamped;
    while b > 0 && !s.is_char_boundary(b) {
        b -= 1;
    }
    s[..b].chars().count()
}

fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map_or(s.len(), |(b, _)| b)
}

fn smart_token_len_before(before: &str) -> usize {
    const FIXED: &[&str] = &[
        " * 10^",
        "^(-1)",
        " mod ",
        "^(1/",
        "10^(",
        " + ",
        " - ",
        " * ",
        " / ",
        " ÷ ",
        ", ",
    ];
    for tok in FIXED {
        if before.ends_with(tok) {
            return tok.len();
        }
    }

    if let Some(stem) = before.strip_suffix('(') {
        let mut start = stem.len();
        for (b, c) in stem.char_indices().rev() {
            if c.is_alphanumeric() || c == '_' {
                start = b;
            } else {
                break;
            }
        }
        let lead_digits = stem[start..].bytes().take_while(|b| b.is_ascii_digit()).count();
        let name_start = start + lead_digits;
        let name = &stem[name_start..];
        if !name.is_empty() && lumina_core::eval::function(name).is_some() {
            return before.len() - name_start;
        }
    }

    const ATOMIC_IDENTS: &[&str] = &["Ans", "ans", "ANS", "tau", "phi", "pi"];
    for ident in ATOMIC_IDENTS {
        if let Some(prefix) = before.strip_suffix(ident) {
            let boundary_ok = prefix
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphabetic() && c != '_');
            if boundary_ok {
                return ident.len();
            }
        }
    }

    before.chars().next_back().map_or(0, char::len_utf8)
}

fn find_matching_open_paren(s: &str, close_byte: usize) -> Option<usize> {
    let prefix = s.get(..=close_byte)?;
    let mut depth = 0i32;
    for (b, c) in prefix.char_indices().rev() {
        match c {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    return Some(b);
                }
            }
            _ => {}
        }
    }
    None
}

fn paint_blinking_caret(
    ctx: &Context,
    painter: &Painter,
    x: f32,
    top_y: f32,
    bot_y: f32,
    palette: &Palette,
) {
    let t = ctx.input(|i| i.time);
    let blink = ((t * 5.2).sin() * 0.5 + 0.5) as f32;
    let alpha = (70.0 + blink * 185.0) as u8;
    painter.line_segment(
        [Pos2::new(x, top_y), Pos2::new(x, bot_y)],
        Stroke::new(2.0, Palette::alpha(palette.accent, alpha)),
    );
    ctx.request_repaint_after(std::time::Duration::from_millis(60));
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
    main_label: KeyLabel,
    shift_label: &str,
    size: Vec2,
    tone: KeyTone,
    active: bool,
    shift_active: bool,
    scale: f32,
    tooltip: &str,
    palette: &Palette,
) -> bool {
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if !tooltip.is_empty() {
        resp = resp.on_hover_text(tooltip);
    }
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
        match main_label {
            KeyLabel::FractionIcon => math_display::paint_fraction_key_icon(
                ui.painter(),
                draw_rect.center(),
                scale * 0.85,
                base_fg,
            ),
            KeyLabel::Text(txt) => {
                ui.painter().text(
                    draw_rect.center(),
                    Align2::CENTER_CENTER,
                    txt,
                    FontId::proportional((base_fs * scale).clamp(11.0, 26.0)),
                    base_fg,
                );
            }
        }
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

        match main_label {
            KeyLabel::FractionIcon => math_display::paint_fraction_key_icon(
                ui.painter(),
                Pos2::new(draw_rect.center().x, bot_y),
                scale * 0.85,
                main_col,
            ),
            KeyLabel::Text(txt) => {
                let main_base = match tone {
                    KeyTone::Digit => 16.5,
                    KeyTone::Op => 17.5,
                    _ => 13.5,
                };
                ui.painter().text(
                    Pos2::new(draw_rect.center().x, bot_y),
                    Align2::CENTER_CENTER,
                    txt,
                    FontId::proportional((main_base * scale).clamp(11.5, 25.0)),
                    main_col,
                );
            }
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
        assert!(v.jump_fraction_down());
        assert!(v.jump_fraction_down());
        v.insert_token("4", true);
        assert_eq!(v.expr, "((2)/(5))/(4)");
        let val = v.last_eval.as_ref().expect("eval").as_ref().expect("ok");
        assert_eq!(decimal_to_fraction(val.si, 10_000), Some((1, 10)));
    }

    #[test]
    fn shift_actions_respect_replace_on_commit() {
        let mut v = ScientificView::default();
        v.insert_token("20", true);
        v.evaluate_commit(8);
        assert!(v.committed);

        v.execute_key_action(ins_chain("%"), 8);
        assert_eq!(v.expr, "Ans%");
        let val = v.last_eval.as_ref().expect("eval").as_ref().expect("ok");
        assert_eq!(val.plain(8), "0.2");

        v.evaluate_commit(8);
        v.execute_key_action(ins_new("asin("), 8);
        assert_eq!(v.expr, "asin(");
    }

    #[test]
    fn casio_ans_chain_after_equals() {
        let mut v = ScientificView::default();
        v.insert_token("2 + 3", true);
        v.evaluate_commit(8);
        assert_eq!(v.ans.as_ref().expect("ans").plain(8), "5");

        v.execute_key_action(ins_chain(" * "), 8);
        assert_eq!(v.expr, "Ans * ");
        v.execute_key_action(ins_new("4"), 8);
        v.evaluate_commit(8);
        assert_eq!(v.ans.as_ref().expect("ans").plain(8), "20");

        v.execute_key_action(ins_chain("^"), 8);
        v.execute_key_action(ins_new("2"), 8);
        v.evaluate_commit(8);
        assert_eq!(v.ans.as_ref().expect("ans").plain(8), "400");

        v.execute_key_action(ins_new("7"), 8);
        assert_eq!(v.expr, "7");
    }

    #[test]
    fn casio_auto_closes_unclosed_parens_on_equals() {
        let mut v = ScientificView::default();
        assert_eq!(v.angle_unit, AngleUnit::Deg);
        v.insert_token("sin(30", true);
        assert_eq!(v.last_eval.as_ref().expect("eval").as_ref().expect("ok").plain(8), "0.5");

        v.evaluate_commit(8);
        assert_eq!(v.expr, "sin(30)");
        assert_eq!(v.ans.as_ref().expect("ans").plain(8), "0.5");

        v.insert_token("2*(3+(4+1", true);
        v.evaluate_commit(8);
        assert_eq!(v.expr, "2*(3+(4+1))");
        assert_eq!(v.ans.as_ref().expect("ans").plain(8), "16");
    }

    #[test]
    fn smart_del_removes_multichar_tokens_atomically() {
        let mut v = ScientificView::default();
        v.insert_token("2sin(", true);
        v.backspace();
        assert_eq!(v.expr, "2");

        v.insert_token("Ans", false);
        assert_eq!(v.expr, "2Ans");
        v.backspace();
        assert_eq!(v.expr, "2");

        v.insert_token(" * 10^", false);
        v.backspace();
        assert_eq!(v.expr, "2");

        v.insert_token(" + asinh(", false);
        v.backspace();
        assert_eq!(v.expr, "2 + ");
        v.backspace();
        assert_eq!(v.expr, "2");
    }

    #[test]
    fn reference_fraction_with_parentheses_equals_14_over_3() {
        let mut v = ScientificView::default();
        v.insert_fraction(false);
        assert_eq!(v.expr, "()/()");

        v.insert_open_paren();
        assert_eq!(v.expr, "(())/()");
        v.insert_token("2 + 5", false);
        v.insert_close_paren();
        v.insert_token(" * 2", false);
        assert_eq!(v.expr, "((2 + 5) * 2)/()");

        assert!(v.jump_fraction_down());
        v.insert_token("3", false);
        assert_eq!(v.expr, "((2 + 5) * 2)/(3)");

        let val = v.last_eval.as_ref().expect("eval").as_ref().expect("ok");
        assert_eq!(decimal_to_fraction(val.si, 10_000), Some((14, 3)));

        v.clear_all();
        v.insert_close_paren();
        assert_eq!(v.expr, "");

        v.insert_fraction(false);
        v.insert_open_paren();
        v.insert_token("2+5", false);
        assert!(v.jump_fraction_down());
        v.insert_token("3", false);
        assert_eq!(v.expr, "((2+5))/(3)");
        let val2 = v.last_eval.as_ref().expect("eval").as_ref().expect("ok");
        assert_eq!(decimal_to_fraction(val2.si, 10_000), Some((7, 3)));
    }

    #[test]
    fn step_by_step_incomplete_expressions_and_utf8_trig_never_panic() {
        let mut v = ScientificView::default();
        for seq in [
            vec!["sin("],
            vec!["sin(", "1", "/", "3"],
            vec!["cos(", "pi"],
            vec!["1", "/"],
            vec!["2", "^"],
            vec!["sqrt("],
            vec!["π", "+", "√", "+", "30°", "+", "2²", "+", "3³"],
            vec!["  ", "2²", "+"],
        ] {
            v.clear_all();
            for tok in seq {
                if tok == "/" {
                    v.insert_fraction(false);
                } else {
                    v.insert_token(tok, true);
                }
            }
            let _ = v.ensure_math_tree();
            v.evaluate_commit(8);
        }

        v.clear_all();
        v.insert_token("sin(1/3)", true);
        assert!(matches!(v.last_eval, Some(Ok(_))));
        v.clear_all();
        v.insert_token("sin(pi)", true);
        assert!(matches!(v.last_eval, Some(Ok(_))));
    }

    #[test]
    fn backspace_and_cursor_navigation_around_utf8_never_panic() {
        for sample in [
            "2²+12",
            "1+23²",
            "π+1234",
            "30°+1234",
            "√(4)+12",
            "абвгде",
            "((π)/(2²))/(3³)",
            ")/()",
        ] {
            for pos in 0..=sample.len() + 3 {
                let mut v = ScientificView {
                    expr: sample.to_owned(),
                    cursor: pos,
                    ..Default::default()
                };
                v.backspace();
                assert!(v.cursor <= v.expr.len());
                assert!(v.expr.is_char_boundary(v.cursor));

                v.expr = sample.to_owned();
                v.cursor = pos;
                v.delete_forward();
                v.move_left();
                v.move_right();
                let _ = v.jump_fraction_down();
                let _ = v.jump_fraction_up();
                assert!(v.cursor <= v.expr.len());
                assert!(v.expr.is_char_boundary(v.cursor));
            }
        }
    }

    #[test]
    fn multi_story_fractions_and_headless_ui_render_safely() {
        let ctx = egui::Context::default();
        let palette = style::apply_theme(&ctx, crate::style::ThemeId::Dark);
        let mut v = ScientificView::default();

        for _ in 0..5 {
            v.insert_fraction(false);
            v.insert_token("2²", false);
        }
        for _ in 0..8 {
            v.backspace();
        }

        let deep = format!("{}1{}", "(".repeat(150), ")".repeat(150));
        let cases = [
            "sin(",
            "1/",
            "()/()",
            "  2² + ",
            "π + √ + 30°",
            "sin(1/3)",
            "((2 + 5) * 2)/(3)",
            deep.as_str(),
        ];

        for expr in cases {
            v.expr = expr.to_owned();
            v.cursor = v.expr.len();
            v.recompute();

            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1040.0, 680.0))),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let _ = v.show(ui, Lang::Ru, 8, &palette);
                });
            });
            out.textures_delta.clear();
        }
    }

    #[test]
    fn sci_hotkey_mapping_covers_trig_functions_constants_and_sd() {
        let none = egui::Modifiers::NONE;
        let shift = egui::Modifiers { shift: true, ..Default::default() };
        let ctrl = egui::Modifiers { ctrl: true, ..Default::default() };
        let alt = egui::Modifiers { alt: true, ..Default::default() };

        assert_eq!(map_sci_hotkey(Key::S, none), Some(SciHotkey::Key(ins_new("sin("))));
        assert_eq!(map_sci_hotkey(Key::S, shift), Some(SciHotkey::Key(ins_new("asin("))));
        assert_eq!(map_sci_hotkey(Key::S, ctrl), Some(SciHotkey::Key(ins_hyp("sinh("))));

        assert_eq!(map_sci_hotkey(Key::O, none), Some(SciHotkey::Key(ins_new("cos("))));
        assert_eq!(map_sci_hotkey(Key::O, shift), Some(SciHotkey::Key(ins_new("acos("))));
        assert_eq!(map_sci_hotkey(Key::O, ctrl), Some(SciHotkey::Key(ins_hyp("cosh("))));

        assert_eq!(map_sci_hotkey(Key::T, none), Some(SciHotkey::Key(ins_new("tan("))));
        assert_eq!(map_sci_hotkey(Key::T, shift), Some(SciHotkey::Key(ins_new("atan("))));
        assert_eq!(map_sci_hotkey(Key::T, ctrl), Some(SciHotkey::Key(ins_hyp("tanh("))));

        assert_eq!(map_sci_hotkey(Key::L, none), Some(SciHotkey::Key(ins_new("log("))));
        assert_eq!(map_sci_hotkey(Key::N, none), Some(SciHotkey::Key(ins_new("ln("))));
        assert_eq!(map_sci_hotkey(Key::R, none), Some(SciHotkey::Key(ins_new("sqrt("))));
        assert_eq!(map_sci_hotkey(Key::Q, none), Some(SciHotkey::Key(ins_chain("²"))));
        assert_eq!(map_sci_hotkey(Key::P, none), Some(SciHotkey::Key(ins_new("pi"))));
        assert_eq!(map_sci_hotkey(Key::E, none), Some(SciHotkey::Key(ins_new("e"))));
        assert_eq!(map_sci_hotkey(Key::E, shift), Some(SciHotkey::Key(ins_chain(" * 10^"))));
        assert_eq!(map_sci_hotkey(Key::D, none), Some(SciHotkey::ToggleSd { shift: false }));
        assert_eq!(map_sci_hotkey(Key::D, shift), Some(SciHotkey::ToggleSd { shift: true }));

        assert_eq!(map_sci_hotkey(Key::C, ctrl), None);
        assert_eq!(map_sci_hotkey(Key::V, ctrl), None);
        assert_eq!(map_sci_hotkey(Key::X, ctrl), None);
        assert_eq!(map_sci_hotkey(Key::A, ctrl), None);
        assert_eq!(map_sci_hotkey(Key::Z, ctrl), None);
        assert_eq!(map_sci_hotkey(Key::S, alt), None);
        assert_eq!(map_sci_hotkey(Key::W, none), None);
    }

    #[test]
    fn sci_char_filter_blocks_letters_and_preserves_math_symbols() {
        for c in "0123456789.,+-*/^%!() ".chars() {
            assert!(is_allowed_sci_char(c), "char '{c}' must be allowed");
        }
        for c in "aAzZsStToOйЙыЫфФ@#?".chars() {
            assert!(!is_allowed_sci_char(c), "char '{c}' must be blocked");
        }
    }

    #[test]
    fn sci_hotkeys_insert_at_cursor_and_work_on_ru_layout_with_paste_and_undo() {
        let ctx = egui::Context::default();
        let palette = style::apply_theme(&ctx, crate::style::ThemeId::Dark);
        let mut v = ScientificView::default();

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1040.0, 680.0))),
            events: vec![
                Event::Key {
                    key: Key::S,
                    physical_key: Some(Key::S),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                Event::Text("ы".to_owned()),
                Event::Text("30".to_owned()),
                Event::Text(")".to_owned()),
                Event::Text("ж".to_owned()),
                Event::Text("+".to_owned()),
                Event::Key {
                    key: Key::P,
                    physical_key: Some(Key::P),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                Event::Text("з".to_owned()),
                Event::Key {
                    key: Key::Q,
                    physical_key: Some(Key::Q),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                Event::Text("й".to_owned()),
                Event::Key {
                    key: Key::D,
                    physical_key: Some(Key::D),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = v.show(ui, Lang::Ru, 8, &palette);
            });
        });
        out.textures_delta.clear();

        assert_eq!(v.expr, "sin(30)+pi²");
        assert_eq!(v.display_mode, DisplayMode::Decimal);

        let paste_and_undo = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1040.0, 680.0))),
            events: vec![
                Event::Key {
                    key: Key::Home,
                    physical_key: Some(Key::Home),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                Event::Paste("2*".to_owned()),
            ],
            ..Default::default()
        };
        let mut out = ctx.run_ui(paste_and_undo, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = v.show(ui, Lang::Ru, 8, &palette);
            });
        });
        out.textures_delta.clear();
        assert_eq!(v.expr, "2*sin(30)+pi²");
        assert_eq!(v.cursor, 2);

        v.undo();
        assert_eq!(v.expr, "sin(30)+pi²");
    }

    #[test]
    fn functions_insert_cleanly_inside_fraction_numerator_and_denominator() {
        for (tok, arg, expected_expr, expected_val) in [
            ("sin(", "30", "(sin(30))/(2)", "0.25"),
            ("cos(", "60", "(cos(60))/(2)", "0.25"),
            ("tan(", "45", "(tan(45))/(2)", "0.5"),
            ("sqrt(", "16", "(sqrt(16))/(2)", "2"),
            ("log(", "100", "(log(100))/(2)", "1"),
            ("ln(", "e", "(ln(e))/(2)", "0.5"),
        ] {
            let mut v = ScientificView::default();
            v.insert_fraction(false);
            assert_eq!(v.expr, "()/()");
            v.insert_token(tok, true);
            assert_eq!(v.expr, format!("({tok}))/()"));
            let tree = math_display::parse_math(&v.expr);
            assert!(
                matches!(tree, MathNode::Frac { .. }),
                "{tok} in empty numerator must stay inside Frac, got {tree:?}"
            );

            v.insert_token(arg, true);
            assert!(v.jump_fraction_down());
            v.insert_token("2", true);
            assert_eq!(v.expr, expected_expr);
            let val = v.last_eval.as_ref().expect("eval").as_ref().expect("ok");
            assert_eq!(val.plain(8), expected_val);
        }

        let mut v = ScientificView::default();
        v.insert_fraction(false);
        v.insert_token("1", true);
        assert!(v.jump_fraction_down());
        v.insert_token("cos(", true);
        assert_eq!(v.expr, "(1)/(cos())");
        v.insert_token("60", true);
        assert_eq!(v.expr, "(1)/(cos(60))");
        let val = v.last_eval.as_ref().expect("eval").as_ref().expect("ok");
        assert_eq!(val.plain(8), "2");

        let mut v = ScientificView::default();
        v.insert_fraction(false);
        v.insert_token("cos(", true);
        assert_eq!(v.expr, "(cos())/()");
        v.backspace();
        assert_eq!(v.expr, "()/()");
        assert_eq!(v.cursor, 1);
    }

    #[test]
    fn inline_division_inside_fraction_numerator_matches_casio_example() {
        let mut v = ScientificView::default();
        v.insert_fraction(false);
        v.insert_token("cos(", true);
        v.insert_token("60", true);
        v.insert_close_paren();
        v.execute_key_action(ins_chain(" ÷ "), 8);
        v.insert_token("20", true);
        assert!(v.jump_fraction_down());
        v.insert_token("2", true);
        assert_eq!(v.expr, "(cos(60) ÷ 20)/(2)");

        let val = v.last_eval.as_ref().expect("eval").as_ref().expect("ok");
        assert_eq!(val.plain(8), "0.0125");
        assert_eq!(decimal_to_fraction(val.si, 10_000), Some((1, 80)));

        let tree = math_display::parse_math(&v.expr);
        let MathNode::Frac { num, den, .. } = tree else {
            panic!("expected single outer fraction");
        };
        assert!(
            matches!(&*num, MathNode::Seq { items, .. } if items.iter().any(|n| matches!(n, MathNode::Op { text, .. } if text == "÷"))),
            "numerator must render inline ÷, got {num:?}"
        );
        assert!(matches!(&*den, MathNode::Text { text, .. } if text == "2"));

        let ctx = egui::Context::default();
        let palette = style::apply_theme(&ctx, crate::style::ThemeId::Dark);
        let mut kb = ScientificView::default();
        kb.insert_fraction(false);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1040.0, 680.0))),
            events: vec![
                Event::Key {
                    key: Key::O,
                    physical_key: Some(Key::O),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                Event::Text("60)/20".to_owned()),
                Event::Key {
                    key: Key::ArrowDown,
                    physical_key: Some(Key::ArrowDown),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                Event::Text("2".to_owned()),
            ],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = kb.show(ui, Lang::Ru, 8, &palette);
            });
        });
        out.textures_delta.clear();
        assert_eq!(kb.expr, "(cos(60) ÷ 20)/(2)");
        assert_eq!(kb.last_eval.as_ref().expect("eval").as_ref().expect("ok").plain(8), "0.0125");

        let mut del = ScientificView { expr: "(cos(60) ÷ )/(2)".to_owned(), cursor: 12, ..Default::default() };
        del.backspace();
        assert_eq!(del.expr, "(cos(60))/(2)");
    }
}
