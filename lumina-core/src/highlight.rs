//! Semantic syntax-highlighting classification over lexer tokens.
//! Pure data (byte ranges + token categories) so `lumina-core` stays UI-agnostic.

use crate::error::Span;
use crate::eval;
use crate::lexer::{Kw, Tok, Token};
use crate::units;
use crate::value::Notation;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HighlightKind {
    Number,
    Variable,
    AssignTarget,
    Constant,
    Function,
    Unit,
    Keyword,
    Operator,
    Punct,
    LineRef,
    Comment,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    pub range: Span,
    pub kind: HighlightKind,
}

/// Classifies every token on a line using the variables visible before this line.
pub fn classify(
    tokens: &[Token<'_>],
    defined_vars: &HashSet<Box<str>>,
    plot_mode: bool,
) -> Vec<HighlightSpan> {
    let mut out = Vec::with_capacity(tokens.len());
    for (idx, t) in tokens.iter().enumerate() {
        let prev = idx.checked_sub(1).and_then(|i| tokens.get(i)).map(|t| t.tok);
        let next = tokens.get(idx + 1).map(|t| t.tok);

        let kind = match t.tok {
            Tok::Number { .. } => HighlightKind::Number,
            Tok::Keyword(_) => HighlightKind::Keyword,
            Tok::LineRef(_) => HighlightKind::LineRef,
            Tok::Op(_) => HighlightKind::Operator,
            Tok::LParen | Tok::RParen | Tok::Comma => HighlightKind::Punct,
            Tok::Assign => HighlightKind::Operator,
            Tok::Comment => HighlightKind::Comment,
            Tok::Unknown => HighlightKind::Error,
            Tok::Ident(name) => {
                if idx == 0 && next == Some(Tok::Assign) {
                    HighlightKind::AssignTarget
                } else if (prev == Some(Tok::Keyword(Kw::In)) && Notation::parse_keyword(name).is_some())
                    || (plot_mode && matches!(name, "x" | "y" | "t"))
                {
                    HighlightKind::Keyword
                } else if defined_vars.contains(name) {
                    HighlightKind::Variable
                } else if next == Some(Tok::LParen) && eval::function(name).is_some() {
                    HighlightKind::Function
                } else if eval::constant(name).is_some() {
                    HighlightKind::Constant
                } else if eval::special(name).is_some() {
                    HighlightKind::Keyword
                } else if !plot_mode && units::is_unit(name) {
                    HighlightKind::Unit
                } else if eval::function(name).is_some() {
                    HighlightKind::Function
                } else {
                    HighlightKind::Variable
                }
            }
        };
        out.push(HighlightSpan { range: t.span(), kind });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    #[test]
    fn classifies_notebook_line() {
        let mut vars = HashSet::new();
        vars.insert(Box::<str>::from("speed"));
        let toks = lex("speed + 120 km/h in m/s // fast");
        let spans = classify(&toks, &vars, false);
        let kinds: Vec<_> = spans.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            vec![
                HighlightKind::Variable,
                HighlightKind::Operator,
                HighlightKind::Number,
                HighlightKind::Unit,
                HighlightKind::Operator,
                HighlightKind::Unit,
                HighlightKind::Keyword,
                HighlightKind::Unit,
                HighlightKind::Operator,
                HighlightKind::Unit,
                HighlightKind::Comment,
            ]
        );
    }

    #[test]
    fn classifies_assignment_and_function() {
        let vars = HashSet::new();
        let toks = lex("y = sin(pi / 2) in hex");
        let spans = classify(&toks, &vars, false);
        assert_eq!(spans[0].kind, HighlightKind::AssignTarget);
        assert_eq!(spans[2].kind, HighlightKind::Function);
        assert_eq!(spans[4].kind, HighlightKind::Constant);
        assert_eq!(spans[9].kind, HighlightKind::Keyword);
    }
}
