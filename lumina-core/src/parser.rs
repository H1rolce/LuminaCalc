//! AST and a precedence-climbing parser over lexer tokens.
//!
//! Precedence (low → high):
//! `in`-conversion < `|` < `xor` < `&` < `<<`,`>>` < `+`,`-` < `*`,`/`,`mod`,`of`,`%`
//! < implicit multiplication (`2x`, `120 km`) < unary `-`,`+`,`~` < `^` (right-assoc)
//! < postfix `!`, `%`, `²`, `³` < primary.
//!
//! Implicit multiplication binds tighter than `/`, so `120 km/h` is `(120·km)/h`
//! and units need no special grammar.

use crate::error::{CalcError, CalcResult, ErrorKind, Span};
use crate::lexer::{Kw, Op, Tok, Token};
use crate::value::{Notation, Radix};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Pos,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    /// Juxtaposition: `2x`, `120 km`. Same math as `Mul`, kept for offset units.
    ImplicitMul,
    Div,
    Mod,
    Pow,
    And,
    Or,
    Xor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostOp {
    Factorial,
    Percent,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Unit(Box<Expr>),
    Notation(Notation),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Number(Decimal, Radix),
    Ident(Box<str>),
    LineRef(usize),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Postfix(PostOp, Box<Expr>),
    Call { name: Box<str>, name_span: Span, args: Vec<Expr> },
    Convert(Box<Expr>, Target),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Empty,
    Expr(Expr),
    Assign { name: Box<str>, name_span: Span, value: Expr },
}

/// Parses a statement from a token stream (comments must already be removed or
/// are ignored here).
pub fn parse_stmt(tokens: &[Token<'_>], src_len: usize) -> CalcResult<Stmt> {
    let toks: Vec<Token<'_>> = tokens.iter().copied().filter(|t| t.tok != Tok::Comment).collect();
    if toks.is_empty() {
        return Ok(Stmt::Empty);
    }
    let mut p = Parser { toks: &toks, pos: 0, end: src_len };
    if let (Some(Tok::Ident(name)), Some(Tok::Assign)) = (p.peek_at(0), p.peek_at(1)) {
        let name_span = toks[0].span();
        p.pos = 2;
        let value = p.expr()?;
        p.expect_end()?;
        return Ok(Stmt::Assign { name: name.into(), name_span, value });
    }
    let e = p.expr()?;
    p.expect_end()?;
    Ok(Stmt::Expr(e))
}

/// Parses a single expression (used by the plotter / programmer input).
pub fn parse_expr(tokens: &[Token<'_>], src_len: usize) -> CalcResult<Expr> {
    match parse_stmt(tokens, src_len)? {
        Stmt::Expr(e) => Ok(e),
        Stmt::Empty => Err(CalcError::new(ErrorKind::UnexpectedEnd, 0..src_len)),
        Stmt::Assign { name_span, .. } => Err(CalcError::new(ErrorKind::UnexpectedToken, name_span)),
    }
}

struct Parser<'t, 'a> {
    toks: &'t [Token<'a>],
    pos: usize,
    end: usize,
}

fn mk(kind: ExprKind, span: Span) -> Expr {
    Expr { kind, span }
}

fn bin(op: BinOp, l: Expr, r: Expr) -> Expr {
    let span = l.span.start..r.span.end;
    mk(ExprKind::Binary(op, Box::new(l), Box::new(r)), span)
}

impl<'a> Parser<'_, 'a> {
    #[inline]
    fn peek(&self) -> Option<Tok<'a>> {
        self.toks.get(self.pos).map(|t| t.tok)
    }
    #[inline]
    fn peek_at(&self, k: usize) -> Option<Tok<'a>> {
        self.toks.get(self.pos + k).map(|t| t.tok)
    }
    #[inline]
    fn span_here(&self) -> Span {
        self.toks.get(self.pos).map_or(self.end..self.end, Token::span)
    }
    #[inline]
    fn bump(&mut self) -> Option<Token<'a>> {
        let t = self.toks.get(self.pos).copied();
        self.pos += 1;
        t
    }

    fn err_here(&self) -> CalcError {
        match self.toks.get(self.pos) {
            Some(t) if t.tok == Tok::Unknown => CalcError::new(ErrorKind::UnexpectedChar, t.span()),
            Some(t) => CalcError::new(ErrorKind::UnexpectedToken, t.span()),
            None => CalcError::new(ErrorKind::UnexpectedEnd, self.end..self.end),
        }
    }

    fn expect_end(&self) -> CalcResult<()> {
        if self.pos < self.toks.len() { Err(self.err_here()) } else { Ok(()) }
    }

    fn expr(&mut self) -> CalcResult<Expr> {
        let lhs = self.bitor()?;
        if self.peek() == Some(Tok::Keyword(Kw::In)) {
            self.bump();
            let target_start = self.span_here().start;
            if let Some(Tok::Ident(name)) = self.peek()
                && let Some(n) = Notation::parse_keyword(name)
            {
                let end = self.toks[self.pos].span.1;
                self.bump();
                let span = lhs.span.start..end;
                return Ok(mk(ExprKind::Convert(Box::new(lhs), Target::Notation(n)), span));
            }
            if self.peek().is_none() {
                return Err(CalcError::new(ErrorKind::UnexpectedEnd, target_start..target_start));
            }
            let unit = self.bitor()?;
            let span = lhs.span.start..unit.span.end;
            return Ok(mk(ExprKind::Convert(Box::new(lhs), Target::Unit(Box::new(unit))), span));
        }
        Ok(lhs)
    }

    fn bitor(&mut self) -> CalcResult<Expr> {
        let mut l = self.bitxor()?;
        while self.peek() == Some(Tok::Op(Op::Pipe)) {
            self.bump();
            let r = self.bitxor()?;
            l = bin(BinOp::Or, l, r);
        }
        Ok(l)
    }

    fn bitxor(&mut self) -> CalcResult<Expr> {
        let mut l = self.bitand()?;
        while self.peek() == Some(Tok::Keyword(Kw::Xor)) {
            self.bump();
            let r = self.bitand()?;
            l = bin(BinOp::Xor, l, r);
        }
        Ok(l)
    }

    fn bitand(&mut self) -> CalcResult<Expr> {
        let mut l = self.shift()?;
        while self.peek() == Some(Tok::Op(Op::Amp)) {
            self.bump();
            let r = self.shift()?;
            l = bin(BinOp::And, l, r);
        }
        Ok(l)
    }

    fn shift(&mut self) -> CalcResult<Expr> {
        let mut l = self.additive()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op(Op::Shl)) => BinOp::Shl,
                Some(Tok::Op(Op::Shr)) => BinOp::Shr,
                _ => break,
            };
            self.bump();
            let r = self.additive()?;
            l = bin(op, l, r);
        }
        Ok(l)
    }

    fn additive(&mut self) -> CalcResult<Expr> {
        let mut l = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op(Op::Plus)) => BinOp::Add,
                Some(Tok::Op(Op::Minus)) => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let r = self.multiplicative()?;
            l = bin(op, l, r);
        }
        Ok(l)
    }

    fn multiplicative(&mut self) -> CalcResult<Expr> {
        let mut l = self.implicit()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op(Op::Star)) | Some(Tok::Keyword(Kw::Of)) => BinOp::Mul,
                Some(Tok::Op(Op::Slash)) => BinOp::Div,
                Some(Tok::Op(Op::Percent)) | Some(Tok::Keyword(Kw::Mod)) => BinOp::Mod,
                _ => break,
            };
            self.bump();
            let r = self.implicit()?;
            l = bin(op, l, r);
        }
        Ok(l)
    }

    /// Juxtaposition: an operand directly followed by an identifier or `(`.
    fn implicit(&mut self) -> CalcResult<Expr> {
        let mut l = self.unary()?;
        while matches!(self.peek(), Some(Tok::Ident(_)) | Some(Tok::LParen) | Some(Tok::LineRef(_))) {
            let r = self.unary()?;
            l = bin(BinOp::ImplicitMul, l, r);
        }
        Ok(l)
    }

    fn unary(&mut self) -> CalcResult<Expr> {
        let op = match self.peek() {
            Some(Tok::Op(Op::Minus)) => UnOp::Neg,
            Some(Tok::Op(Op::Plus)) => UnOp::Pos,
            Some(Tok::Op(Op::Tilde)) => UnOp::Not,
            _ => return self.power(),
        };
        let start = self.span_here().start;
        self.bump();
        let inner = self.unary()?;
        let span = start..inner.span.end;
        Ok(mk(ExprKind::Unary(op, Box::new(inner)), span))
    }

    fn power(&mut self) -> CalcResult<Expr> {
        let base = self.postfix()?;
        if self.peek() == Some(Tok::Op(Op::Caret)) {
            self.bump();
            let exp = self.unary()?; // right-assoc, allows 2^-1
            return Ok(bin(BinOp::Pow, base, exp));
        }
        Ok(base)
    }

    /// `%` is postfix percent unless an operand follows (then it is modulo).
    fn percent_is_postfix(&self) -> bool {
        !matches!(
            self.peek_at(1),
            Some(Tok::Number { .. }) | Some(Tok::Ident(_)) | Some(Tok::LParen) | Some(Tok::LineRef(_))
        )
    }

    fn postfix(&mut self) -> CalcResult<Expr> {
        let mut e = self.primary()?;
        while let Some(&tok) = self.toks.get(self.pos) {
            let end = tok.span.1;
            let span = e.span.start..end;
            e = match tok.tok {
                Tok::Op(Op::Bang) => mk(ExprKind::Postfix(PostOp::Factorial, Box::new(e)), span),
                Tok::Op(Op::Percent) if self.percent_is_postfix() => {
                    mk(ExprKind::Postfix(PostOp::Percent, Box::new(e)), span)
                }
                Tok::Op(Op::Sup2) | Tok::Op(Op::Sup3) => {
                    let n = if tok.tok == Tok::Op(Op::Sup2) { 2 } else { 3 };
                    let exp = mk(ExprKind::Number(Decimal::from(n), Radix::Dec), tok.span());
                    bin(BinOp::Pow, e, exp)
                }
                _ => break,
            };
            self.bump();
        }
        Ok(e)
    }

    fn primary(&mut self) -> CalcResult<Expr> {
        let Some(t) = self.toks.get(self.pos).copied() else {
            return Err(self.err_here());
        };
        match t.tok {
            Tok::Number { text, radix } => {
                self.bump();
                let d = parse_number(text, radix).ok_or_else(|| CalcError::new(ErrorKind::InvalidNumber, t.span()))?;
                Ok(mk(ExprKind::Number(d, radix), t.span()))
            }
            Tok::LineRef(n) => {
                self.bump();
                Ok(mk(ExprKind::LineRef(n), t.span()))
            }
            Tok::Ident(name) => {
                self.bump();
                if self.peek() == Some(Tok::LParen) {
                    let open = self.span_here();
                    self.bump();
                    let mut args = Vec::new();
                    if self.peek() != Some(Tok::RParen) {
                        loop {
                            args.push(self.expr()?);
                            if self.peek() == Some(Tok::Comma) {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                    }
                    let Some(close) = self.toks.get(self.pos).filter(|t| t.tok == Tok::RParen).copied() else {
                        return Err(if self.peek().is_none() {
                            CalcError::new(ErrorKind::UnclosedParen, open)
                        } else {
                            self.err_here()
                        });
                    };
                    self.bump();
                    return Ok(mk(
                        ExprKind::Call { name: name.into(), name_span: t.span(), args },
                        t.span.0..close.span.1,
                    ));
                }
                Ok(mk(ExprKind::Ident(name.into()), t.span()))
            }
            Tok::LParen => {
                self.bump();
                let inner = self.expr()?;
                match self.peek() {
                    Some(Tok::RParen) => {
                        let end = self.toks[self.pos].span.1;
                        self.bump();
                        Ok(Expr { kind: inner.kind, span: t.span.0..end })
                    }
                    None => Err(CalcError::new(ErrorKind::UnclosedParen, t.span())),
                    _ => Err(self.err_here()),
                }
            }
            _ => Err(self.err_here()),
        }
    }
}

/// Parses a numeric literal body (prefix already stripped for radix literals).
pub fn parse_number(text: &str, radix: Radix) -> Option<Decimal> {
    let clean: std::borrow::Cow<'_, str> =
        if text.contains('_') { text.replace('_', "").into() } else { text.into() };
    match radix {
        Radix::Dec => {
            if clean.contains(['e', 'E']) {
                Decimal::from_scientific(&clean).ok()
            } else {
                Decimal::from_str(&clean).ok()
            }
        }
        r => u128::from_str_radix(&clean, r.base()).ok().and_then(Decimal::from_u128),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn p(src: &str) -> CalcResult<Stmt> {
        parse_stmt(&lex(src), src.len())
    }

    /// Renders the AST as an S-expression for compact assertions.
    fn sx(e: &Expr) -> String {
        match &e.kind {
            ExprKind::Number(d, _) => d.to_string(),
            ExprKind::Ident(n) => n.to_string(),
            ExprKind::LineRef(n) => format!("#{n}"),
            ExprKind::Unary(op, a) => format!("({op:?} {})", sx(a)),
            ExprKind::Binary(op, a, b) => format!("({op:?} {} {})", sx(a), sx(b)),
            ExprKind::Postfix(op, a) => format!("({op:?} {})", sx(a)),
            ExprKind::Call { name, args, .. } => {
                format!("{name}({})", args.iter().map(sx).collect::<Vec<_>>().join(","))
            }
            ExprKind::Convert(a, Target::Unit(u)) => format!("(in {} {})", sx(a), sx(u)),
            ExprKind::Convert(a, Target::Notation(n)) => format!("(in {} {n:?})", sx(a)),
        }
    }

    fn e(src: &str) -> String {
        match p(src) {
            Ok(Stmt::Expr(e)) => sx(&e),
            Ok(Stmt::Assign { name, value, .. }) => format!("{name}={}", sx(&value)),
            Ok(Stmt::Empty) => "<empty>".into(),
            Err(err) => format!("ERR {:?}", err.kind),
        }
    }

    #[test]
    fn precedence() {
        assert_eq!(e("1 + 2 * 3"), "(Add 1 (Mul 2 3))");
        assert_eq!(e("2 ^ 3 ^ 2"), "(Pow 2 (Pow 3 2))");
        assert_eq!(e("-2^2"), "(Neg (Pow 2 2))");
        assert_eq!(e("2^-1"), "(Pow 2 (Neg 1))");
        assert_eq!(e("(1 + 2) * 3"), "(Mul (Add 1 2) 3)");
        assert_eq!(e("1 | 2 & 3"), "(Or 1 (And 2 3))");
        assert_eq!(e("1 << 2 + 1"), "(Shl 1 (Add 2 1))");
    }

    #[test]
    fn implicit_multiplication() {
        assert_eq!(e("2x"), "(ImplicitMul 2 x)");
        assert_eq!(e("120 km/h"), "(Div (ImplicitMul 120 km) h)");
        assert_eq!(e("2 sin(x)^2"), "(ImplicitMul 2 (Pow sin(x) 2))");
        assert_eq!(e("-40 °C"), "(ImplicitMul (Neg 40) °C)");
        assert_eq!(e("(1+2)(3+4)"), "(ImplicitMul (Add 1 2) (Add 3 4))");
        assert_eq!(e("1/2x"), "(Div 1 (ImplicitMul 2 x))");
    }

    #[test]
    fn percent_vs_modulo() {
        assert_eq!(e("50 + 10%"), "(Add 50 (Percent 10))");
        assert_eq!(e("10 % 3"), "(Mod 10 3)");
        assert_eq!(e("20% of 50"), "(Mul (Percent 20) 50)");
        assert_eq!(e("10 mod 3"), "(Mod 10 3)");
    }

    #[test]
    fn conversions() {
        assert_eq!(e("120 km/h in m/s"), "(in (Div (ImplicitMul 120 km) h) (Div m s))");
        assert_eq!(e("255 in hex"), "(in 255 Radix(Hex))");
        assert_eq!(e("1 в sci"), "(in 1 Sci)");
        assert_eq!(e("5 in"), "ERR UnexpectedEnd");
    }

    #[test]
    fn postfix_ops() {
        assert_eq!(e("5!"), "(Factorial 5)");
        assert_eq!(e("3²"), "(Pow 3 2)");
        assert_eq!(e("m³"), "(Pow m 3)");
    }

    #[test]
    fn statements() {
        assert_eq!(e("x = 2 + 3"), "x=(Add 2 3)");
        assert_eq!(e("   "), "<empty>");
        assert_eq!(e("// only comment"), "<empty>");
        assert_eq!(e("max(1, 2, 3)"), "max(1,2,3)");
        assert_eq!(e("rand()"), "rand()");
    }

    #[test]
    fn errors_have_spans() {
        let err = p("1 + ").expect_err("must fail");
        assert_eq!(err.kind, ErrorKind::UnexpectedEnd);
        assert_eq!(err.span, 4..4);
        let err = p("(1 + 2").expect_err("must fail");
        assert_eq!(err.kind, ErrorKind::UnclosedParen);
        assert_eq!(err.span, 0..1);
        let err = p("1 @ 2").expect_err("must fail");
        assert_eq!(err.kind, ErrorKind::UnexpectedChar);
        assert_eq!(err.span, 2..3);
        let err = p("1 2").expect_err("must fail");
        assert_eq!(err.kind, ErrorKind::UnexpectedToken);
        assert_eq!(e("sqrt(4"), "ERR UnclosedParen");
    }

    #[test]
    fn number_literals() {
        assert_eq!(parse_number("1_000", Radix::Dec), Some(Decimal::from(1000)));
        assert_eq!(parse_number("1.5e3", Radix::Dec), Some(Decimal::from(1500)));
        assert_eq!(parse_number("FF", Radix::Hex), Some(Decimal::from(255)));
        assert_eq!(parse_number("1e999", Radix::Dec), None);
    }
}
