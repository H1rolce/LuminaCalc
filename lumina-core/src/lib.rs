//! LuminaCalc core calculation engine.
//!
//! Zero GUI dependencies: exact decimal arithmetic, unit dimensional analysis,
//! reactive multi-line notebook with AST caching, semantic syntax highlighting,
//! 2D function plotting helpers, and bit-level programmer register.

pub mod error;
pub mod eval;
pub mod highlight;
pub mod lexer;
pub mod notebook;
pub mod parser;
pub mod plot;
pub mod programmer;
pub mod units;
pub mod value;

pub use error::{CalcError, CalcResult, ErrorCategory, ErrorKind, Span};
pub use eval::{AngleUnit, Env, eval, eval_str};
pub use highlight::{HighlightKind, HighlightSpan};
pub use notebook::{LineEntry, LineOutput, Notebook};
pub use parser::auto_close_parens;
pub use plot::{CompiledPlot, SnapKind, SnapPoint, compile_plot, eval_f64, find_snap_points, sample_curve};
pub use programmer::{BitRegister, BitWidth};
pub use units::Dim;
pub use value::{FormatOptions, Notation, Radix, Value, decimal_to_fraction, format_engineering};
