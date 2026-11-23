//! Zero-copy lexer built on `winnow`. Tokens borrow from the source line and
//! carry byte spans, which feed both the parser and the syntax highlighter.
//! The lexer never fails: unknown characters become [`Tok::Unknown`].

use crate::error::Span;
use crate::value::Radix;
use winnow::Result as WResult;
use winnow::combinator::{alt, opt, preceded};
use winnow::prelude::*;
use winnow::token::{one_of, take_while};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Percent,
    Bang,
    Amp,
    Pipe,
    Shl,
    Shr,
    Tilde,
    Sup2,
    Sup3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kw {
    /// `in`, `to`, `as`, `в`, `->`, `→`
    In,
    /// `mod`
    Mod,
    /// `of`, `от`
    Of,
    /// `xor`
    Xor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok<'a> {
    Number { text: &'a str, radix: Radix },
    Ident(&'a str),
    Keyword(Kw),
    LineRef(usize),
    Op(Op),
    LParen,
    RParen,
    Comma,
    Assign,
    Comment,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub tok: Tok<'a>,
    pub span: (usize, usize),
}

impl Token<'_> {
    #[inline]
    pub fn span(&self) -> Span {
        self.span.0..self.span.1
    }
}

#[inline]
fn is_superscript(c: char) -> bool {
    matches!(c, '²' | '³' | '¹' | '⁰' | '⁴'..='⁹')
}

#[inline]
fn ident_start(c: char) -> bool {
    (c.is_alphabetic() && !is_superscript(c)) || matches!(c, '_' | '°' | 'µ' | 'Ω' | 'π' | 'τ' | 'φ')
}

#[inline]
fn ident_cont(c: char) -> bool {
    (c.is_alphanumeric() && !is_superscript(c)) || c == '_'
}

fn digits_us<'s>(i: &mut &'s str) -> WResult<&'s str> {
    (one_of(|c: char| c.is_ascii_digit()), take_while(0.., |c: char| c.is_ascii_digit() || c == '_'))
        .take()
        .parse_next(i)
}

fn exponent(i: &mut &str) -> WResult<()> {
    (one_of(['e', 'E']), opt(one_of(['+', '-'])), take_while(1.., |c: char| c.is_ascii_digit()))
        .void()
        .parse_next(i)
}

fn decimal<'s>(i: &mut &'s str) -> WResult<&'s str> {
    (
        alt((
            (digits_us, opt(('.', opt(digits_us)))).void(),
            ('.', digits_us).void(),
        )),
        opt(exponent),
    )
        .take()
        .parse_next(i)
}

fn radix_digits<'s>(prefix: [char; 2], pred: fn(char) -> bool) -> impl Parser<&'s str, &'s str, winnow::error::ContextError> {
    preceded(('0', one_of(prefix)), take_while(1.., move |c: char| pred(c) || c == '_'))
}

fn number<'s>(i: &mut &'s str) -> WResult<Tok<'s>> {
    alt((
        radix_digits(['x', 'X'], |c| c.is_ascii_hexdigit()).map(|text| Tok::Number { text, radix: Radix::Hex }),
        radix_digits(['b', 'B'], |c| matches!(c, '0' | '1')).map(|text| Tok::Number { text, radix: Radix::Bin }),
        radix_digits(['o', 'O'], |c| matches!(c, '0'..='7')).map(|text| Tok::Number { text, radix: Radix::Oct }),
        decimal.map(|text| Tok::Number { text, radix: Radix::Dec }),
    ))
    .parse_next(i)
}

fn ident<'s>(i: &mut &'s str) -> WResult<Tok<'s>> {
    (one_of(ident_start), take_while(0.., ident_cont))
        .take()
        .map(|s: &'s str| match s {
            "in" | "to" | "as" | "в" => Tok::Keyword(Kw::In),
            "mod" => Tok::Keyword(Kw::Mod),
            "of" | "от" => Tok::Keyword(Kw::Of),
            "xor" => Tok::Keyword(Kw::Xor),
            _ => Tok::Ident(s),
        })
        .parse_next(i)
}

fn line_ref<'s>(i: &mut &'s str) -> WResult<Tok<'s>> {
    preceded('#', take_while(1.., |c: char| c.is_ascii_digit()))
        .map(|d: &str| Tok::LineRef(d.parse().unwrap_or(usize::MAX)))
        .parse_next(i)
}

fn operator<'s>(i: &mut &'s str) -> WResult<Tok<'s>> {
    alt((
        alt((
            "<<".value(Tok::Op(Op::Shl)),
            ">>".value(Tok::Op(Op::Shr)),
            "**".value(Tok::Op(Op::Caret)),
            "->".value(Tok::Keyword(Kw::In)),
            '→'.value(Tok::Keyword(Kw::In)),
        )),
        alt((
            '+'.value(Tok::Op(Op::Plus)),
            one_of(['-', '−']).value(Tok::Op(Op::Minus)),
            one_of(['*', '×', '·', '⋅']).value(Tok::Op(Op::Star)),
            one_of(['/', '÷']).value(Tok::Op(Op::Slash)),
            '^'.value(Tok::Op(Op::Caret)),
            '%'.value(Tok::Op(Op::Percent)),
        )),
        alt((
            '!'.value(Tok::Op(Op::Bang)),
            '&'.value(Tok::Op(Op::Amp)),
            '|'.value(Tok::Op(Op::Pipe)),
            '~'.value(Tok::Op(Op::Tilde)),
            '²'.value(Tok::Op(Op::Sup2)),
            '³'.value(Tok::Op(Op::Sup3)),
        )),
        alt((
            '('.value(Tok::LParen),
            ')'.value(Tok::RParen),
            one_of([',', ';']).value(Tok::Comma),
            '='.value(Tok::Assign),
        )),
    ))
    .parse_next(i)
}

fn token<'s>(i: &mut &'s str) -> WResult<Tok<'s>> {
    alt((number, ident, line_ref, operator)).parse_next(i)
}

/// Returns `true` if `s` begins with a `#` comment/heading (and not a `#123` line reference).
#[inline]
pub fn is_hash_comment(s: &str) -> bool {
    s.starts_with('#') && !s[1..].starts_with(|c: char| c.is_ascii_digit())
}

/// Tokenizes one line. Whitespace is skipped; `//` and `# text` start a comment
/// that spans to the end of the line.
pub fn lex(src: &str) -> Vec<Token<'_>> {
    let mut out = Vec::with_capacity(src.len() / 2 + 1);
    let mut rest = src;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let start = src.len() - rest.len();
        let is_comment = rest.starts_with("//") || is_hash_comment(rest);
        if is_comment {
            out.push(Token { tok: Tok::Comment, span: (start, src.len()) });
            break;
        }
        let tok = match token.parse_next(&mut rest) {
            Ok(tok) => tok,
            Err(_) => {
                let skip = rest.chars().next().map_or(1, char::len_utf8);
                rest = &rest[skip..];
                Tok::Unknown
            }
        };
        out.push(Token { tok, span: (start, src.len() - rest.len()) });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok<'_>> {
        lex(s).into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn numbers() {
        assert_eq!(toks("1_000.5e-3"), vec![Tok::Number { text: "1_000.5e-3", radix: Radix::Dec }]);
        assert_eq!(toks(".5"), vec![Tok::Number { text: ".5", radix: Radix::Dec }]);
        assert_eq!(toks("0xFF"), vec![Tok::Number { text: "FF", radix: Radix::Hex }]);
        assert_eq!(toks("0b1010"), vec![Tok::Number { text: "1010", radix: Radix::Bin }]);
        assert_eq!(toks("0o17"), vec![Tok::Number { text: "17", radix: Radix::Oct }]);
        // `2e` is 2 * e, not a broken exponent
        assert_eq!(toks("2e"), vec![Tok::Number { text: "2", radix: Radix::Dec }, Tok::Ident("e")]);
    }

    #[test]
    fn units_and_keywords() {
        assert_eq!(
            toks("120 км/ч в м/с"),
            vec![
                Tok::Number { text: "120", radix: Radix::Dec },
                Tok::Ident("км"),
                Tok::Op(Op::Slash),
                Tok::Ident("ч"),
                Tok::Keyword(Kw::In),
                Tok::Ident("м"),
                Tok::Op(Op::Slash),
                Tok::Ident("с"),
            ]
        );
        assert_eq!(toks("20°C"), vec![Tok::Number { text: "20", radix: Radix::Dec }, Tok::Ident("°C")]);
        assert_eq!(toks("m²"), vec![Tok::Ident("m"), Tok::Op(Op::Sup2)]);
        assert_eq!(toks("a -> b"), vec![Tok::Ident("a"), Tok::Keyword(Kw::In), Tok::Ident("b")]);
    }

    #[test]
    fn spans_are_byte_offsets() {
        let t = lex("x = км");
        assert_eq!(t[2].span, (4, 8));
        assert_eq!(&"x = км"[t[2].span()], "км");
    }

    #[test]
    fn comments_and_refs() {
        assert_eq!(toks("#3 + 1 // note"), vec![
            Tok::LineRef(3),
            Tok::Op(Op::Plus),
            Tok::Number { text: "1", radix: Radix::Dec },
            Tok::Comment
        ]);
        assert_eq!(toks("# Header"), vec![Tok::Comment]);
    }

    #[test]
    fn unknown_never_panics() {
        assert_eq!(toks("1 @ 2"), vec![
            Tok::Number { text: "1", radix: Radix::Dec },
            Tok::Unknown,
            Tok::Number { text: "2", radix: Radix::Dec }
        ]);
        assert_eq!(toks("€"), vec![Tok::Unknown]);
        assert!(toks("").is_empty());
    }
}
