//! Interactive 2D Natural-V.P.A.M. mathematical renderer & hit-tester.
//!
//! Parses live (including incomplete) expressions into a spanned layout tree,
//! renders stacked fractions, radicals, superscripts, empty slots `()` / `□`,
//! and a live caret at `cursor_byte`, while emitting clickable [`HitZone`]s so
//! users can click directly into numerators, denominators, or between characters.

use crate::style::Palette;
use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, Vec2};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathNode {
    Seq { items: Vec<MathNode>, span: Range<usize> },
    Text { text: String, span: Range<usize> },
    Op { text: String, span: Range<usize> },
    /// Empty editable slot inside `()` or `□`, with exact byte insertion offset.
    Slot { byte_pos: usize, span: Range<usize> },
    Frac { num: Box<MathNode>, den: Box<MathNode>, span: Range<usize> },
    Sup { base: Box<MathNode>, exp: Box<MathNode>, span: Range<usize> },
    Sqrt { inner: Box<MathNode>, span: Range<usize> },
    Paren { inner: Box<MathNode>, closed: bool, span: Range<usize> },
}

impl MathNode {
    pub fn span(&self) -> Range<usize> {
        match self {
            MathNode::Seq { span, .. }
            | MathNode::Text { span, .. }
            | MathNode::Op { span, .. }
            | MathNode::Slot { span, .. }
            | MathNode::Frac { span, .. }
            | MathNode::Sup { span, .. }
            | MathNode::Sqrt { span, .. }
            | MathNode::Paren { span, .. } => span.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    pub width: f32,
    /// Height above the mathematical middle axis (where `+`, `=`, fraction bars sit).
    pub above: f32,
    /// Height below the mathematical middle axis.
    pub below: f32,
}

impl Metrics {
    #[inline]
    pub fn height(self) -> f32 {
        self.above + self.below
    }
}

/// Interactive hit zone collected during 2D painting for mouse click placement.
#[derive(Debug, Clone, Copy)]
pub struct HitZone {
    pub rect: Rect,
    pub byte_pos: usize,
    /// Higher priority zones (like empty slots or character halves) win on overlap.
    pub priority: u8,
}

/// Visual caret geometry recorded during painting if `cursor_byte` matched.
#[derive(Debug, Clone, Copy)]
pub struct CaretPos {
    pub x: f32,
    pub top_y: f32,
    pub bot_y: f32,
}

#[derive(Default)]
pub struct PaintFeedback {
    pub hits: Vec<HitZone>,
    pub caret: Option<CaretPos>,
}

impl PaintFeedback {
    /// Resolves a mouse click position inside the 2D display to the closest byte offset.
    pub fn byte_at_pos(&self, pos: Pos2, fallback_end: usize) -> usize {
        // 1. Prefer direct hit with highest priority (smallest enclosing rect on tie).
        let mut best_direct: Option<(u8, f32, usize)> = None;
        for h in &self.hits {
            if h.rect.contains(pos) {
                let area = h.rect.width().max(1.0) * h.rect.height().max(1.0);
                let better = match best_direct {
                    None => true,
                    Some((bp, ba, _)) => h.priority > bp || (h.priority == bp && area < ba),
                };
                if better {
                    best_direct = Some((h.priority, area, h.byte_pos));
                }
            }
        }
        if let Some((_, _, b)) = best_direct {
            return b;
        }

        // 2. Otherwise pick nearest hit zone center (weighted vertically so numerator/denominator stay distinct).
        let mut best_dist = f32::INFINITY;
        let mut best_byte = fallback_end;
        for h in &self.hits {
            let dx = pos.x - h.rect.center().x;
            let dy = (pos.y - h.rect.center().y) * 1.35;
            let d = dx * dx + dy * dy;
            if d < best_dist {
                best_dist = d;
                best_byte = h.byte_pos;
            }
        }
        best_byte
    }
}

/// Parses an expression string into a spanned [`MathNode`] tree. Never fails.
pub fn parse_math(src: &str) -> MathNode {
    let tokens = tokenize_math(src);
    let mut p = MathParser { toks: &tokens, pos: 0, src_len: src.len() };
    p.parse_seq(false, 0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MTokKind {
    Atom(String),
    Op(String),
    Slash,
    Caret,
    Sup2,
    Sup3,
    LParen,
    RParen,
    Slot,
    Comma,
}

#[derive(Debug, Clone)]
struct MTok {
    kind: MTokKind,
    span: Range<usize>,
}

fn tokenize_math(src: &str) -> Vec<MTok> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (byte_start, c) = chars[i];
        let byte_end = byte_start + c.len_utf8();
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '□' => {
                out.push(MTok { kind: MTokKind::Slot, span: byte_start..byte_end });
                i += 1;
            }
            '/' | '÷' => {
                out.push(MTok { kind: MTokKind::Slash, span: byte_start..byte_end });
                i += 1;
            }
            '^' => {
                out.push(MTok { kind: MTokKind::Caret, span: byte_start..byte_end });
                i += 1;
            }
            '²' => {
                out.push(MTok { kind: MTokKind::Sup2, span: byte_start..byte_end });
                i += 1;
            }
            '³' => {
                out.push(MTok { kind: MTokKind::Sup3, span: byte_start..byte_end });
                i += 1;
            }
            '(' => {
                out.push(MTok { kind: MTokKind::LParen, span: byte_start..byte_end });
                i += 1;
            }
            ')' => {
                out.push(MTok { kind: MTokKind::RParen, span: byte_start..byte_end });
                i += 1;
            }
            ',' => {
                out.push(MTok { kind: MTokKind::Comma, span: byte_start..byte_end });
                i += 1;
            }
            '+' => {
                out.push(MTok { kind: MTokKind::Op("+".into()), span: byte_start..byte_end });
                i += 1;
            }
            '-' | '−' => {
                out.push(MTok { kind: MTokKind::Op("−".into()), span: byte_start..byte_end });
                i += 1;
            }
            '*' | '×' | '·' => {
                out.push(MTok { kind: MTokKind::Op("×".into()), span: byte_start..byte_end });
                i += 1;
            }
            '%' | '!' => {
                out.push(MTok { kind: MTokKind::Atom(c.to_string()), span: byte_start..byte_end });
                i += 1;
            }
            _ if c.is_ascii_digit() || c == '.' => {
                let mut j = i + 1;
                while j < chars.len()
                    && (chars[j].1.is_ascii_digit() || chars[j].1 == '.' || chars[j].1 == '_')
                {
                    j += 1;
                }
                let end = if j < chars.len() { chars[j].0 } else { src.len() };
                out.push(MTok {
                    kind: MTokKind::Atom(src[byte_start..end].to_owned()),
                    span: byte_start..end,
                });
                i = j;
            }
            _ if c.is_alphabetic() || matches!(c, 'π' | 'τ' | 'φ' | '°' | 'µ' | '_') => {
                let mut j = i + 1;
                while j < chars.len() && (chars[j].1.is_alphanumeric() || chars[j].1 == '_') {
                    j += 1;
                }
                let end = if j < chars.len() { chars[j].0 } else { src.len() };
                let word = &src[byte_start..end];
                if word == "mod" {
                    out.push(MTok { kind: MTokKind::Op("mod".into()), span: byte_start..end });
                } else {
                    let pretty = match word {
                        "pi" => "π",
                        "tau" => "τ",
                        "phi" => "φ",
                        other => other,
                    };
                    out.push(MTok {
                        kind: MTokKind::Atom(pretty.to_owned()),
                        span: byte_start..end,
                    });
                }
                i = j;
            }
            _ => {
                out.push(MTok { kind: MTokKind::Atom(c.to_string()), span: byte_start..byte_end });
                i += 1;
            }
        }
    }
    out
}

struct MathParser<'a> {
    toks: &'a [MTok],
    pos: usize,
    src_len: usize,
}

impl MathParser<'_> {
    fn peek(&self) -> Option<&MTok> {
        self.toks.get(self.pos)
    }

    fn bump(&mut self) -> Option<MTok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_seq(&mut self, stop_at_rparen: bool, empty_byte: usize) -> MathNode {
        let mut items = Vec::new();
        while let Some(tok) = self.peek() {
            if stop_at_rparen && tok.kind == MTokKind::RParen {
                break;
            }
            match &tok.kind {
                MTokKind::Op(op) => {
                    let op = op.clone();
                    let span = tok.span.clone();
                    self.bump();
                    items.push(MathNode::Op { text: op, span });
                }
                MTokKind::Comma => {
                    let span = tok.span.clone();
                    self.bump();
                    items.push(MathNode::Op { text: ",".into(), span });
                }
                _ => {
                    items.push(self.parse_frac());
                }
            }
        }
        match items.len() {
            0 => MathNode::Slot { byte_pos: empty_byte, span: empty_byte..empty_byte },
            1 => items
                .pop()
                .unwrap_or(MathNode::Slot { byte_pos: empty_byte, span: empty_byte..empty_byte }),
            _ => {
                let start = items.first().map_or(empty_byte, |n| n.span().start);
                let end = items.last().map_or(empty_byte, |n| n.span().end);
                MathNode::Seq { items, span: start..end }
            }
        }
    }

    fn parse_frac(&mut self) -> MathNode {
        let mut lhs = self.parse_pow();
        while let Some(tok) = self.peek() {
            if tok.kind != MTokKind::Slash {
                break;
            }
            let slash_end = tok.span.end;
            self.bump();
            let rhs = if matches!(
                self.peek().map(|t| &t.kind),
                None | Some(MTokKind::RParen | MTokKind::Op(_) | MTokKind::Comma)
            ) {
                MathNode::Slot { byte_pos: slash_end, span: slash_end..slash_end }
            } else {
                self.parse_pow()
            };
            let span = lhs.span().start..rhs.span().end;
            let num = unwrap_paren(lhs);
            let den = unwrap_paren(rhs);
            lhs = MathNode::Frac {
                num: Box::new(num),
                den: Box::new(den),
                span,
            };
        }
        lhs
    }

    fn parse_pow(&mut self) -> MathNode {
        let mut base = self.parse_primary();
        while let Some(tok) = self.peek() {
            match tok.kind {
                MTokKind::Sup2 => {
                    let tspan = tok.span.clone();
                    self.bump();
                    let span = base.span().start..tspan.end;
                    base = MathNode::Sup {
                        base: Box::new(base),
                        exp: Box::new(MathNode::Text { text: "2".into(), span: tspan }),
                        span,
                    };
                }
                MTokKind::Sup3 => {
                    let tspan = tok.span.clone();
                    self.bump();
                    let span = base.span().start..tspan.end;
                    base = MathNode::Sup {
                        base: Box::new(base),
                        exp: Box::new(MathNode::Text { text: "3".into(), span: tspan }),
                        span,
                    };
                }
                MTokKind::Caret => {
                    let caret_end = tok.span.end;
                    self.bump();
                    let exp = if matches!(
                        self.peek().map(|t| &t.kind),
                        None | Some(MTokKind::RParen | MTokKind::Op(_) | MTokKind::Comma)
                    ) {
                        MathNode::Slot { byte_pos: caret_end, span: caret_end..caret_end }
                    } else {
                        unwrap_paren(self.parse_primary())
                    };
                    let span = base.span().start..exp.span().end;
                    base = MathNode::Sup {
                        base: Box::new(base),
                        exp: Box::new(exp),
                        span,
                    };
                }
                _ => break,
            }
        }
        base
    }

    fn parse_primary(&mut self) -> MathNode {
        let Some(tok) = self.bump() else {
            return MathNode::Text { text: String::new(), span: self.src_len..self.src_len };
        };
        match tok.kind {
            MTokKind::Slot => MathNode::Slot { byte_pos: tok.span.start, span: tok.span },
            MTokKind::LParen => {
                let inner_start = tok.span.end;
                let inner = self.parse_seq(true, inner_start);
                let (end, closed) = if self.peek().is_some_and(|t| t.kind == MTokKind::RParen) {
                    (self.bump().map_or(inner.span().end, |t| t.span.end), true)
                } else {
                    (inner.span().end, false)
                };
                MathNode::Paren { inner: Box::new(inner), closed, span: tok.span.start..end }
            }
            MTokKind::Atom(word) => {
                if word == "sqrt" && self.peek().is_some_and(|t| t.kind == MTokKind::LParen) {
                    let lp = self.bump();
                    let inner_start = lp.map_or(tok.span.end, |t| t.span.end);
                    let inner = self.parse_seq(true, inner_start);
                    let end = if self.peek().is_some_and(|t| t.kind == MTokKind::RParen) {
                        self.bump().map_or(inner.span().end, |t| t.span.end)
                    } else {
                        inner.span().end
                    };
                    MathNode::Sqrt { inner: Box::new(inner), span: tok.span.start..end }
                } else {
                    MathNode::Text { text: word, span: tok.span }
                }
            }
            MTokKind::Op(op) => MathNode::Op { text: op, span: tok.span },
            MTokKind::Slash => MathNode::Op { text: "/".into(), span: tok.span },
            MTokKind::Caret => MathNode::Op { text: "^".into(), span: tok.span },
            MTokKind::Sup2 => MathNode::Text { text: "²".into(), span: tok.span },
            MTokKind::Sup3 => MathNode::Text { text: "³".into(), span: tok.span },
            MTokKind::RParen => MathNode::Text { text: ")".into(), span: tok.span },
            MTokKind::Comma => MathNode::Op { text: ",".into(), span: tok.span },
        }
    }
}

fn unwrap_paren(node: MathNode) -> MathNode {
    match node {
        MathNode::Paren { inner, .. } => *inner,
        other => other,
    }
}

/// Measures a [`MathNode`] subtree at the given base `font_size`.
pub fn measure(node: &MathNode, painter: &Painter, font_size: f32) -> Metrics {
    match node {
        MathNode::Seq { items, .. } => {
            let mut w = 0.0f32;
            let mut above = font_size * 0.55;
            let mut below = font_size * 0.55;
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    w += font_size * 0.14;
                }
                let m = measure(item, painter, font_size);
                w += m.width;
                above = above.max(m.above);
                below = below.max(m.below);
            }
            Metrics { width: w, above, below }
        }
        MathNode::Text { text, .. } => {
            if text.is_empty() {
                return Metrics { width: 0.0, above: font_size * 0.55, below: font_size * 0.55 };
            }
            let g = painter.layout_no_wrap(text.clone(), FontId::monospace(font_size), Color32::WHITE);
            let h = g.size().y * 0.5;
            Metrics { width: g.size().x, above: h, below: h }
        }
        MathNode::Op { text, .. } => {
            let pad = if text == "," { font_size * 0.12 } else { font_size * 0.24 };
            let g = painter.layout_no_wrap(text.clone(), FontId::monospace(font_size), Color32::WHITE);
            let h = g.size().y * 0.5;
            Metrics { width: g.size().x + pad * 2.0, above: h, below: h }
        }
        MathNode::Slot { .. } => {
            let s = (font_size * 0.85).max(11.0);
            Metrics { width: s + 6.0, above: s * 0.55, below: s * 0.55 }
        }
        MathNode::Frac { num, den, .. } => {
            let sub_fs = (font_size * 0.78).max(7.5);
            let mn = measure(num, painter, sub_fs);
            let md = measure(den, painter, sub_fs);
            let pad_x = (font_size * 0.28).max(3.0);
            let gap = (font_size * 0.14).max(1.8);
            Metrics {
                width: mn.width.max(md.width) + pad_x * 2.0,
                above: mn.height() + gap,
                below: md.height() + gap,
            }
        }
        MathNode::Sup { base, exp, .. } => {
            let mb = measure(base, painter, font_size);
            let exp_fs = (font_size * 0.65).max(7.5);
            let me = measure(exp, painter, exp_fs);
            let lift = mb.above * 0.55;
            Metrics {
                width: mb.width + me.width + 1.5,
                above: mb.above.max(lift + me.above),
                below: mb.below,
            }
        }
        MathNode::Sqrt { inner, .. } => {
            let mi = measure(inner, painter, font_size);
            let hook_w = font_size * 0.65;
            let top_pad = (font_size * 0.14).max(2.0);
            Metrics {
                width: hook_w + mi.width + 4.0,
                above: mi.above + top_pad,
                below: mi.below,
            }
        }
        MathNode::Paren { inner, .. } => {
            let mi = measure(inner, painter, font_size);
            let pw = (font_size * 0.42).max(7.0);
            Metrics {
                width: mi.width + pw * 2.0,
                above: mi.above + 1.5,
                below: mi.below + 1.5,
            }
        }
    }
}

/// Paints a [`MathNode`] subtree with its mathematical middle axis at `mid_y`,
/// recording clickable [`HitZone`]s and computing the exact [`CaretPos`] for `cursor_byte`.
#[allow(clippy::too_many_arguments)]
pub fn paint_interactive(
    node: &MathNode,
    painter: &Painter,
    left_x: f32,
    mid_y: f32,
    font_size: f32,
    cursor_byte: usize,
    hover_pos: Option<Pos2>,
    palette: &Palette,
    fb: &mut PaintFeedback,
) -> Metrics {
    match node {
        MathNode::Seq { items, .. } => {
            let mut x = left_x;
            let mut above = font_size * 0.55;
            let mut below = font_size * 0.55;
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    x += font_size * 0.14;
                }
                let m = paint_interactive(
                    item,
                    painter,
                    x,
                    mid_y,
                    font_size,
                    cursor_byte,
                    hover_pos,
                    palette,
                    fb,
                );
                x += m.width;
                above = above.max(m.above);
                below = below.max(m.below);
            }
            Metrics { width: x - left_x, above, below }
        }
        MathNode::Text { text, span } => {
            if text.is_empty() {
                return Metrics { width: 0.0, above: font_size * 0.55, below: font_size * 0.55 };
            }
            let color = if text.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '_') {
                palette.syn_number
            } else if matches!(text.as_str(), "π" | "e" | "τ" | "φ") {
                palette.syn_constant
            } else if matches!(text.as_str(), "ans" | "Ans" | "ANS" | "M") {
                palette.syn_keyword
            } else {
                palette.syn_function
            };
            let g = painter.layout_no_wrap(text.clone(), FontId::monospace(font_size), color);
            let sz = g.size();
            let top_y = mid_y - sz.y * 0.5;
            let bot_y = mid_y + sz.y * 0.5;
            painter.galley(Pos2::new(left_x, top_y), g, color);

            // Emit per-character hit zones for accurate mouse clicking inside numbers (e.g. `7|7`).
            let char_count = text.chars().count().max(1);
            let cw = sz.x / char_count as f32;
            let byte_span_len = span.end.saturating_sub(span.start);
            let per_char_match = text.len() == byte_span_len;

            if per_char_match && char_count > 1 {
                let mut b = span.start;
                for (ci, ch) in text.chars().enumerate() {
                    let x0 = left_x + ci as f32 * cw;
                    let x_mid = x0 + cw * 0.5;
                    let x1 = x0 + cw;
                    let next_b = b + ch.len_utf8();
                    fb.hits.push(HitZone {
                        rect: Rect::from_min_max(Pos2::new(x0, top_y), Pos2::new(x_mid, bot_y)),
                        byte_pos: b,
                        priority: 3,
                    });
                    fb.hits.push(HitZone {
                        rect: Rect::from_min_max(Pos2::new(x_mid, top_y), Pos2::new(x1, bot_y)),
                        byte_pos: next_b,
                        priority: 3,
                    });
                    if cursor_byte == b {
                        fb.caret = Some(CaretPos { x: x0, top_y, bot_y });
                    }
                    b = next_b;
                }
                if cursor_byte == span.end {
                    fb.caret = Some(CaretPos { x: left_x + sz.x, top_y, bot_y });
                }
            } else {
                let x_mid = left_x + sz.x * 0.5;
                fb.hits.push(HitZone {
                    rect: Rect::from_min_max(Pos2::new(left_x, top_y), Pos2::new(x_mid, bot_y)),
                    byte_pos: span.start,
                    priority: 3,
                });
                fb.hits.push(HitZone {
                    rect: Rect::from_min_max(Pos2::new(x_mid, top_y), Pos2::new(left_x + sz.x, bot_y)),
                    byte_pos: span.end,
                    priority: 3,
                });
                if cursor_byte == span.start {
                    fb.caret = Some(CaretPos { x: left_x, top_y, bot_y });
                } else if cursor_byte > span.start && cursor_byte <= span.end {
                    fb.caret = Some(CaretPos { x: left_x + sz.x, top_y, bot_y });
                }
            }

            Metrics { width: sz.x, above: sz.y * 0.5, below: sz.y * 0.5 }
        }
        MathNode::Op { text, span } => {
            let pad = if text == "," { font_size * 0.12 } else { font_size * 0.24 };
            let g = painter.layout_no_wrap(text.clone(), FontId::monospace(font_size), palette.syn_operator);
            let sz = g.size();
            let top_y = mid_y - sz.y * 0.5;
            let bot_y = mid_y + sz.y * 0.5;
            let total_w = sz.x + pad * 2.0;
            painter.galley(Pos2::new(left_x + pad, top_y), g, palette.syn_operator);

            let x_mid = left_x + total_w * 0.5;
            fb.hits.push(HitZone {
                rect: Rect::from_min_max(Pos2::new(left_x, top_y), Pos2::new(x_mid, bot_y)),
                byte_pos: span.start,
                priority: 2,
            });
            fb.hits.push(HitZone {
                rect: Rect::from_min_max(Pos2::new(x_mid, top_y), Pos2::new(left_x + total_w, bot_y)),
                byte_pos: span.end,
                priority: 2,
            });
            if cursor_byte == span.start {
                fb.caret = Some(CaretPos { x: left_x + pad * 0.4, top_y, bot_y });
            } else if cursor_byte == span.end {
                fb.caret = Some(CaretPos { x: left_x + total_w - pad * 0.4, top_y, bot_y });
            }

            Metrics { width: total_w, above: sz.y * 0.5, below: sz.y * 0.5 }
        }
        MathNode::Slot { byte_pos, span } => {
            let s = (font_size * 0.85).max(11.0);
            let r = Rect::from_center_size(Pos2::new(left_x + s * 0.5 + 3.0, mid_y), Vec2::splat(s));
            let active = cursor_byte >= span.start && cursor_byte <= span.end || cursor_byte == *byte_pos;
            let hovered = hover_pos.is_some_and(|p| r.expand(3.0).contains(p));

            let fill = if active {
                Palette::alpha(palette.accent, 52)
            } else if hovered {
                Palette::alpha(palette.accent, 28)
            } else {
                Palette::alpha(palette.bg_elevated, 145)
            };
            let stroke = if active {
                Stroke::new(1.7, palette.accent)
            } else if hovered {
                Stroke::new(1.4, Palette::alpha(palette.accent, 180))
            } else {
                Stroke::new(1.2, palette.text_muted)
            };
            painter.rect(r, CornerRadius::same(4), fill, stroke, egui::StrokeKind::Inside);

            fb.hits.push(HitZone {
                rect: r.expand(4.0),
                byte_pos: *byte_pos,
                priority: 5,
            });
            if active {
                fb.caret = Some(CaretPos {
                    x: r.center().x,
                    top_y: r.top() + 2.5,
                    bot_y: r.bottom() - 2.5,
                });
            }

            Metrics { width: s + 6.0, above: s * 0.55, below: s * 0.55 }
        }
        MathNode::Frac { num, den, span } => {
            let sub_fs = (font_size * 0.78).max(7.5);
            let mn = measure(num, painter, sub_fs);
            let md = measure(den, painter, sub_fs);
            let pad_x = (font_size * 0.28).max(3.0);
            let gap = (font_size * 0.14).max(1.8);
            let total_w = mn.width.max(md.width) + pad_x * 2.0;

            let num_rect = Rect::from_min_max(
                Pos2::new(left_x, mid_y - gap - mn.height()),
                Pos2::new(left_x + total_w, mid_y - gap * 0.5),
            );
            let den_rect = Rect::from_min_max(
                Pos2::new(left_x, mid_y + gap * 0.5),
                Pos2::new(left_x + total_w, mid_y + gap + md.height()),
            );

            // Subtle hover highlight on numerator / denominator area to invite mouse clicking
            let num_contains_cursor = cursor_byte >= num.span().start && cursor_byte <= num.span().end;
            let den_contains_cursor = cursor_byte >= den.span().start && cursor_byte <= den.span().end;
            if let Some(hp) = hover_pos {
                if num_rect.contains(hp) && !matches!(**num, MathNode::Slot { .. }) {
                    painter.rect_filled(
                        num_rect.expand2(Vec2::new(1.0, 1.0)),
                        CornerRadius::same(4),
                        Palette::alpha(palette.accent, 22),
                    );
                } else if den_rect.contains(hp) && !matches!(**den, MathNode::Slot { .. }) {
                    painter.rect_filled(
                        den_rect.expand2(Vec2::new(1.0, 1.0)),
                        CornerRadius::same(4),
                        Palette::alpha(palette.accent, 22),
                    );
                }
            }

            // Fallback hit zones for numerator / denominator margins (priority 1, below inner chars)
            fb.hits.push(HitZone { rect: num_rect, byte_pos: num.span().end, priority: 1 });
            fb.hits.push(HitZone { rect: den_rect, byte_pos: den.span().end, priority: 1 });

            // Edge hit zones to place cursor cleanly BEFORE or AFTER the entire fraction
            let edge_w = (pad_x * 0.65).max(4.0);
            fb.hits.push(HitZone {
                rect: Rect::from_min_max(
                    Pos2::new(left_x - edge_w * 0.5, mid_y - mn.height()),
                    Pos2::new(left_x + edge_w * 0.5, mid_y + md.height()),
                ),
                byte_pos: span.start,
                priority: 2,
            });
            fb.hits.push(HitZone {
                rect: Rect::from_min_max(
                    Pos2::new(left_x + total_w - edge_w * 0.5, mid_y - mn.height()),
                    Pos2::new(left_x + total_w + edge_w * 0.5, mid_y + md.height()),
                ),
                byte_pos: span.end,
                priority: 2,
            });

            // Paint numerator centered above bar
            let num_x = left_x + (total_w - mn.width) * 0.5;
            let num_mid_y = mid_y - gap - mn.below;
            paint_interactive(num, painter, num_x, num_mid_y, sub_fs, cursor_byte, hover_pos, palette, fb);

            // Crisp horizontal vinculum (accented lightly when cursor is inside the fraction)
            let bar_thick = (font_size * 0.085).clamp(1.2, 2.5);
            let bar_col = if num_contains_cursor || den_contains_cursor {
                Palette::lerp(palette.text_primary, palette.accent, 0.45)
            } else {
                palette.text_primary
            };
            painter.line_segment(
                [
                    Pos2::new(left_x + pad_x * 0.25, mid_y),
                    Pos2::new(left_x + total_w - pad_x * 0.25, mid_y),
                ],
                Stroke::new(bar_thick, bar_col),
            );

            // Paint denominator centered below bar
            let den_x = left_x + (total_w - md.width) * 0.5;
            let den_mid_y = mid_y + gap + md.above;
            paint_interactive(den, painter, den_x, den_mid_y, sub_fs, cursor_byte, hover_pos, palette, fb);

            // Caret at exact start/end of the outer fraction (outside its parens)
            if cursor_byte == span.start && fb.caret.is_none() {
                fb.caret = Some(CaretPos {
                    x: left_x,
                    top_y: mid_y - font_size * 0.55,
                    bot_y: mid_y + font_size * 0.55,
                });
            } else if cursor_byte == span.end && fb.caret.is_none() {
                fb.caret = Some(CaretPos {
                    x: left_x + total_w + 1.0,
                    top_y: mid_y - font_size * 0.55,
                    bot_y: mid_y + font_size * 0.55,
                });
            }

            Metrics {
                width: total_w,
                above: mn.height() + gap,
                below: md.height() + gap,
            }
        }
        MathNode::Sup { base, exp, span } => {
            let mb = paint_interactive(base, painter, left_x, mid_y, font_size, cursor_byte, hover_pos, palette, fb);
            let exp_fs = (font_size * 0.65).max(7.5);
            let me = measure(exp, painter, exp_fs);
            let lift = mb.above * 0.55;
            paint_interactive(
                exp,
                painter,
                left_x + mb.width + 1.5,
                mid_y - lift,
                exp_fs,
                cursor_byte,
                hover_pos,
                palette,
                fb,
            );
            if cursor_byte == span.end && fb.caret.is_none() {
                fb.caret = Some(CaretPos {
                    x: left_x + mb.width + me.width + 2.5,
                    top_y: mid_y - font_size * 0.55,
                    bot_y: mid_y + font_size * 0.55,
                });
            }
            Metrics {
                width: mb.width + me.width + 1.5,
                above: mb.above.max(lift + me.above),
                below: mb.below,
            }
        }
        MathNode::Sqrt { inner, span } => {
            let mi = measure(inner, painter, font_size);
            let hook_w = font_size * 0.65;
            let top_pad = (font_size * 0.14).max(2.0);
            let top_y = mid_y - mi.above - top_pad;
            let bot_y = mid_y + mi.below * 0.85;
            let stroke = Stroke::new((font_size * 0.08).clamp(1.2, 2.2), palette.syn_function);

            let pts = vec![
                Pos2::new(left_x, mid_y + mi.below * 0.1),
                Pos2::new(left_x + hook_w * 0.32, bot_y),
                Pos2::new(left_x + hook_w * 0.82, top_y),
                Pos2::new(left_x + hook_w + mi.width + 2.0, top_y),
            ];
            painter.add(egui::Shape::line(pts, stroke));

            paint_interactive(inner, painter, left_x + hook_w, mid_y, font_size, cursor_byte, hover_pos, palette, fb);
            if cursor_byte == span.end && fb.caret.is_none() {
                fb.caret = Some(CaretPos {
                    x: left_x + hook_w + mi.width + 3.5,
                    top_y: mid_y - mi.above,
                    bot_y: mid_y + mi.below,
                });
            }
            Metrics {
                width: hook_w + mi.width + 4.0,
                above: mi.above + top_pad,
                below: mi.below,
            }
        }
        MathNode::Paren { inner, closed, span } => {
            let mi = measure(inner, painter, font_size);
            let pw = (font_size * 0.42).max(7.0);
            let top_y = mid_y - mi.above - 1.0;
            let bot_y = mid_y + mi.below + 1.0;
            let thick = (font_size * 0.075).clamp(1.3, 2.0);
            let open_stroke = Stroke::new(thick, palette.syn_punct);
            let close_stroke = if *closed {
                open_stroke
            } else {
                Stroke::new(thick, Palette::alpha(palette.text_muted, 85))
            };

            paint_paren_curve(painter, left_x + pw * 0.72, top_y, bot_y, -pw * 0.45, open_stroke);
            paint_interactive(inner, painter, left_x + pw, mid_y, font_size, cursor_byte, hover_pos, palette, fb);
            paint_paren_curve(painter, left_x + pw + mi.width + pw * 0.28, top_y, bot_y, pw * 0.45, close_stroke);

            fb.hits.push(HitZone {
                rect: Rect::from_min_max(Pos2::new(left_x, top_y), Pos2::new(left_x + pw, bot_y)),
                byte_pos: span.start,
                priority: 2,
            });
            fb.hits.push(HitZone {
                rect: Rect::from_min_max(
                    Pos2::new(left_x + pw + mi.width, top_y),
                    Pos2::new(left_x + pw * 2.0 + mi.width, bot_y),
                ),
                byte_pos: span.end,
                priority: 2,
            });

            if cursor_byte == span.start && fb.caret.is_none() {
                fb.caret = Some(CaretPos { x: left_x, top_y, bot_y });
            } else if cursor_byte == span.end && fb.caret.is_none() {
                fb.caret = Some(CaretPos { x: left_x + pw * 2.0 + mi.width, top_y, bot_y });
            }

            Metrics {
                width: mi.width + pw * 2.0,
                above: mi.above + 1.5,
                below: mi.below + 1.5,
            }
        }
    }
}

fn paint_paren_curve(painter: &Painter, x: f32, top_y: f32, bot_y: f32, bulge: f32, stroke: Stroke) {
    let steps = 8;
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let y = top_y + (bot_y - top_y) * t;
        let dx = 4.0 * t * (1.0 - t) * bulge;
        pts.push(Pos2::new(x + dx, y));
    }
    painter.add(egui::Shape::line(pts, stroke));
}

/// Paints a stacked rational result right-aligned inside `rect`, e.g. `23/12` or mixed `1 11/12`.
pub fn paint_stacked_fraction_result(
    painter: &Painter,
    rect: Rect,
    num: i64,
    den: i64,
    mixed: bool,
    font_size: f32,
    color: Color32,
) {
    let sign = if num < 0 { "−" } else { "" };
    let abs_n = num.unsigned_abs();
    let abs_d = den.unsigned_abs();

    let (whole_opt, rem_n) = if mixed && abs_n > abs_d {
        (Some(abs_n / abs_d), abs_n % abs_d)
    } else {
        (None, abs_n)
    };

    let sub_fs = (font_size * 0.72).max(12.0);
    let gn = painter.layout_no_wrap(rem_n.to_string(), FontId::monospace(sub_fs), color);
    let gd = painter.layout_no_wrap(abs_d.to_string(), FontId::monospace(sub_fs), color);
    let frac_w = gn.size().x.max(gd.size().x) + 12.0;
    let mid_y = rect.center().y;
    let gap = 2.5f32;

    let frac_right = rect.right() - 4.0;
    let frac_left = frac_right - frac_w;

    painter.galley(
        Pos2::new(frac_left + (frac_w - gn.size().x) * 0.5, mid_y - gap - gn.size().y),
        gn,
        color,
    );
    painter.line_segment(
        [Pos2::new(frac_left + 2.0, mid_y), Pos2::new(frac_right - 2.0, mid_y)],
        Stroke::new(2.0, color),
    );
    painter.galley(
        Pos2::new(frac_left + (frac_w - gd.size().x) * 0.5, mid_y + gap),
        gd,
        color,
    );

    let prefix = match whole_opt {
        Some(w) => format!("{sign}{w} "),
        None => sign.to_owned(),
    };
    if !prefix.is_empty() {
        painter.text(
            Pos2::new(frac_left - 2.0, mid_y),
            Align2::RIGHT_CENTER,
            prefix,
            FontId::monospace(font_size),
            color,
        );
    }
}

/// Paints the classic Casio `□ / □` fraction button icon centered at `center`.
pub fn paint_fraction_key_icon(painter: &Painter, center: Pos2, scale: f32, color: Color32) {
    let box_sz = (6.5 * scale).clamp(5.5, 10.0);
    let bar_w = (14.0 * scale).clamp(12.0, 22.0);
    let gap = (2.2 * scale).clamp(1.8, 3.5);

    let top_rect = Rect::from_center_size(
        Pos2::new(center.x, center.y - gap - box_sz * 0.5),
        Vec2::splat(box_sz),
    );
    let bot_rect = Rect::from_center_size(
        Pos2::new(center.x, center.y + gap + box_sz * 0.5),
        Vec2::splat(box_sz),
    );

    painter.rect_filled(top_rect, CornerRadius::same(1), color);
    painter.line_segment(
        [Pos2::new(center.x - bar_w * 0.5, center.y), Pos2::new(center.x + bar_w * 0.5, center.y)],
        Stroke::new((1.6 * scale).clamp(1.3, 2.2), color),
    );
    painter.rect_stroke(
        bot_rect,
        CornerRadius::same(1),
        Stroke::new((1.2 * scale).clamp(1.0, 1.8), color),
        egui::StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stacked_fractions_with_exact_spans() {
        let tree = parse_math("2/3 + 1/6");
        let MathNode::Seq { items, span } = tree else {
            panic!("expected seq");
        };
        assert_eq!(span, 0..9);
        assert_eq!(items.len(), 3);
        assert!(matches!(items[0], MathNode::Frac { .. }));
        assert!(matches!(items[2], MathNode::Frac { .. }));
    }

    #[test]
    fn parses_empty_fraction_template_as_two_clickable_slots() {
        let tree = parse_math("()/()");
        let MathNode::Frac { num, den, span } = tree else {
            panic!("expected frac");
        };
        assert_eq!(span, 0..5);
        assert_eq!(*num, MathNode::Slot { byte_pos: 1, span: 1..1 });
        assert_eq!(*den, MathNode::Slot { byte_pos: 4, span: 4..4 });
    }

    #[test]
    fn parses_multi_story_fractions() {
        let three = parse_math("((2)/(5))/(4)");
        assert!(matches!(
            three,
            MathNode::Frac { num, .. } if matches!(*num, MathNode::Frac { .. })
        ));
        let four = parse_math("(1/2)/(3/4)");
        assert!(matches!(
            four,
            MathNode::Frac { num, den, .. }
                if matches!(*num, MathNode::Frac { .. }) && matches!(*den, MathNode::Frac { .. })
        ));
    }
}
