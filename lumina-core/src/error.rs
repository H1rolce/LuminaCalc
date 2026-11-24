//! Error types. Every error carries a byte span inside the source line so the UI
//! can underline the exact location. Messages are English via `thiserror`;
//! the UI layer localizes them by matching on [`ErrorKind`].

use std::ops::Range;

pub type Span = Range<usize>;

/// High-level error classification matching scientific calculator conventions
/// (`Syntax ERROR` vs `Math ERROR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    Syntax,
    Math,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ErrorKind {
    #[error("unexpected character")]
    UnexpectedChar,
    #[error("unexpected end of expression")]
    UnexpectedEnd,
    #[error("unexpected token")]
    UnexpectedToken,
    #[error("missing closing parenthesis")]
    UnclosedParen,
    #[error("invalid number literal")]
    InvalidNumber,
    #[error("unknown identifier `{0}`")]
    UnknownIdent(String),
    #[error("unknown function `{0}`")]
    UnknownFunction(String),
    #[error("`{name}` expects {expected} argument(s), got {got}")]
    Arity {
        name: String,
        expected: &'static str,
        got: usize,
    },
    #[error("division by zero")]
    DivisionByZero,
    #[error("numeric overflow")]
    Overflow,
    #[error("result is not a real number")]
    Domain,
    #[error("incompatible units: {0} vs {1}")]
    DimensionMismatch(String, String),
    #[error("operation requires a dimensionless value")]
    ExpectedDimensionless,
    #[error("operation requires an integer")]
    ExpectedInteger,
    #[error("line #{0} has no value")]
    BadLineRef(usize),
    #[error("no previous result")]
    NoPrevious,
    #[error("cannot assign to `{0}`")]
    BadAssignment(String),
    #[error("invalid conversion target")]
    BadConversionTarget,
}

impl ErrorKind {
    /// Classifies the error into [`ErrorCategory::Syntax`] or [`ErrorCategory::Math`].
    #[inline]
    pub const fn category(&self) -> ErrorCategory {
        match self {
            ErrorKind::UnexpectedChar
            | ErrorKind::UnexpectedEnd
            | ErrorKind::UnexpectedToken
            | ErrorKind::UnclosedParen
            | ErrorKind::InvalidNumber
            | ErrorKind::UnknownIdent(_)
            | ErrorKind::UnknownFunction(_)
            | ErrorKind::Arity { .. }
            | ErrorKind::BadAssignment(_)
            | ErrorKind::BadConversionTarget => ErrorCategory::Syntax,

            ErrorKind::DivisionByZero
            | ErrorKind::Overflow
            | ErrorKind::Domain
            | ErrorKind::DimensionMismatch(_, _)
            | ErrorKind::ExpectedDimensionless
            | ErrorKind::ExpectedInteger
            | ErrorKind::BadLineRef(_)
            | ErrorKind::NoPrevious => ErrorCategory::Math,
        }
    }

    #[inline]
    pub const fn is_syntax(&self) -> bool {
        matches!(self.category(), ErrorCategory::Syntax)
    }

    #[inline]
    pub const fn is_math(&self) -> bool {
        matches!(self.category(), ErrorCategory::Math)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind} at {span:?}")]
pub struct CalcError {
    pub kind: ErrorKind,
    pub span: Span,
}

impl CalcError {
    #[inline]
    pub fn new(kind: ErrorKind, span: Span) -> Self {
        Self { kind, span }
    }
}

pub type CalcResult<T> = Result<T, CalcError>;
