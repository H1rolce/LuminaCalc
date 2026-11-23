//! Exact evaluator. Arithmetic is done in `rust_decimal` (no `0.1 + 0.2`
//! artifacts); transcendental functions fall back to `f64` and are rounded to
//! 15 significant digits to hide binary noise.

use crate::error::{CalcError, CalcResult, ErrorKind, Span};
use crate::parser::{BinOp, Expr, ExprKind, PostOp, Target, UnOp};
use crate::units::{self, Dim};
use crate::value::{Notation, Radix, UnitHint, Value};
use rust_decimal::MathematicalOps;
use rust_decimal::prelude::*;
use std::collections::HashMap;

/// Angle mode used by trigonometric functions when no explicit unit (`deg`/`rad`) is attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AngleUnit {
    #[default]
    Rad,
    Deg,
    Grad,
}

impl AngleUnit {
    #[inline]
    pub fn to_rad(self, x: f64) -> f64 {
        match self {
            AngleUnit::Rad => x,
            AngleUnit::Deg => x.to_radians(),
            AngleUnit::Grad => x * (std::f64::consts::PI / 200.0),
        }
    }

    #[inline]
    pub fn from_rad(self, x: f64) -> f64 {
        match self {
            AngleUnit::Rad => x,
            AngleUnit::Deg => x.to_degrees(),
            AngleUnit::Grad => x * (200.0 / std::f64::consts::PI),
        }
    }

    #[inline]
    pub fn next(self) -> Self {
        match self {
            AngleUnit::Deg => AngleUnit::Rad,
            AngleUnit::Rad => AngleUnit::Grad,
            AngleUnit::Grad => AngleUnit::Deg,
        }
    }

    #[inline]
    pub fn label(self) -> &'static str {
        match self {
            AngleUnit::Rad => "RAD",
            AngleUnit::Deg => "DEG",
            AngleUnit::Grad => "GRAD",
        }
    }
}

/// Variables and notebook context visible to an expression.
#[derive(Debug, Default, Clone)]
pub struct Env {
    pub vars: HashMap<Box<str>, Value>,
    /// Values of previous document lines, index = line number - 1.
    pub lines: Vec<Option<Value>>,
    /// Values of the current block (since the last empty line) for `sum`/`avg`.
    pub block: Vec<Value>,
    /// Last successful result for `ans`.
    pub prev: Option<Value>,
    /// Angle unit for bare trigonometric arguments (defaults to radians).
    pub angle_unit: AngleUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Special {
    Prev,
    Sum,
    Avg,
}

pub fn special(name: &str) -> Option<Special> {
    match name {
        "ans" | "prev" | "отв" | "пред" => Some(Special::Prev),
        "sum" | "total" | "сумма" | "итого" => Some(Special::Sum),
        "avg" | "average" | "среднее" => Some(Special::Avg),
        _ => None,
    }
}

pub fn constant(name: &str) -> Option<Decimal> {
    match name {
        "pi" | "π" | "пи" => Some(Decimal::PI),
        "e" => Some(Decimal::E),
        "tau" | "τ" => Some(Decimal::TWO_PI),
        "phi" | "φ" => Decimal::from_str("1.6180339887498948482045868344").ok(),
        _ => None,
    }
}

/// Canonical function name for any alias (EN + RU).
pub fn function(name: &str) -> Option<&'static str> {
    Some(match name {
        "sqrt" | "корень" => "sqrt",
        "cbrt" => "cbrt",
        "abs" | "абс" | "модуль" => "abs",
        "round" | "округл" => "round",
        "floor" | "пол" => "floor",
        "ceil" | "потолок" => "ceil",
        "trunc" => "trunc",
        "sign" | "sgn" | "знак" => "sign",
        "min" | "мин" => "min",
        "max" | "макс" => "max",
        "sin" | "син" => "sin",
        "cos" | "кос" => "cos",
        "tan" | "tg" | "тан" | "тг" => "tan",
        "cot" | "ctg" | "ктг" => "cot",
        "asin" | "arcsin" => "asin",
        "acos" | "arccos" => "acos",
        "atan" | "arctan" | "arctg" => "atan",
        "atan2" => "atan2",
        "sinh" | "sh" => "sinh",
        "cosh" | "ch" => "cosh",
        "tanh" | "th" => "tanh",
        "asinh" | "arsinh" => "asinh",
        "acosh" | "arcosh" => "acosh",
        "atanh" | "artanh" => "atanh",
        "ln" => "ln",
        "log" | "lg" | "лог" => "log",
        "log2" => "log2",
        "log10" => "log10",
        "exp" | "эксп" => "exp",
        "gcd" | "нод" => "gcd",
        "lcm" | "нок" => "lcm",
        "npr" | "nPr" => "npr",
        "ncr" | "nCr" => "ncr",
        "hypot" => "hypot",
        _ => return None,
    })
}

#[inline]
fn err<T>(kind: ErrorKind, span: &Span) -> CalcResult<T> {
    Err(CalcError::new(kind, span.clone()))
}

fn mismatch(a: &Value, b: &Value, span: &Span) -> CalcError {
    let label = |v: &Value| v.unit_label().filter(|s| !s.is_empty()).unwrap_or_else(|| "1".to_owned());
    CalcError::new(ErrorKind::DimensionMismatch(label(a), label(b)), span.clone())
}

/// Converts an `f64` result back to `Decimal`, rounding away binary noise.
fn from_f64(x: f64, span: &Span) -> CalcResult<Decimal> {
    if x.is_nan() {
        return err(ErrorKind::Domain, span);
    }
    if x.is_infinite() {
        return err(ErrorKind::Overflow, span);
    }
    if x.abs() < 1e-15 {
        return Ok(Decimal::ZERO);
    }
    let d = Decimal::from_f64(x).ok_or_else(|| CalcError::new(ErrorKind::Overflow, span.clone()))?;
    Ok(d.round_sf(15).unwrap_or(d).normalize())
}

fn to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(f64::NAN)
}

fn to_int(v: &Value, span: &Span) -> CalcResult<i128> {
    if !v.is_dimensionless() {
        return err(ErrorKind::ExpectedDimensionless, span);
    }
    if !v.si.fract().is_zero() {
        return err(ErrorKind::ExpectedInteger, span);
    }
    v.si.to_i128().ok_or_else(|| CalcError::new(ErrorKind::Overflow, span.clone()))
}

fn from_int(n: i128, span: &Span) -> CalcResult<Decimal> {
    Decimal::from_i128(n).ok_or_else(|| CalcError::new(ErrorKind::Overflow, span.clone()))
}

fn merge_notation(a: Notation, b: Notation) -> Notation {
    if a != Notation::Auto { a } else { b }
}

/// Integer power by squaring with overflow checks.
fn powi_exact(base: Decimal, exp: i64, span: &Span) -> CalcResult<Decimal> {
    let overflow = || CalcError::new(ErrorKind::Overflow, span.clone());
    let mut result = Decimal::ONE;
    let mut b = base;
    let mut e = exp.unsigned_abs();
    while e > 0 {
        if e & 1 == 1 {
            result = result.checked_mul(b).ok_or_else(overflow)?;
        }
        e >>= 1;
        if e > 0 {
            b = b.checked_mul(b).ok_or_else(overflow)?;
        }
    }
    if exp < 0 {
        if result.is_zero() {
            return err(ErrorKind::DivisionByZero, span);
        }
        result = Decimal::ONE.checked_div(result).ok_or_else(overflow)?;
    }
    Ok(result)
}

pub fn eval(e: &Expr, env: &Env) -> CalcResult<Value> {
    let span = &e.span;
    match &e.kind {
        ExprKind::Number(d, radix) => {
            let mut v = Value::num(*d);
            if *radix != Radix::Dec {
                v.notation = Notation::Radix(*radix);
            }
            Ok(v)
        }
        ExprKind::Ident(name) => ident(name, span, env),
        ExprKind::LineRef(n) => n
            .checked_sub(1)
            .and_then(|i| env.lines.get(i))
            .and_then(|v| v.clone())
            .ok_or_else(|| CalcError::new(ErrorKind::BadLineRef(*n), span.clone())),
        ExprKind::Unary(op, a) => {
            let mut v = eval(a, env)?;
            v.bare_unit = false;
            match op {
                UnOp::Pos => Ok(v),
                UnOp::Neg => {
                    v.si = -v.si;
                    Ok(v)
                }
                UnOp::Not => {
                    let n = to_int(&v, &a.span)?;
                    v.si = from_int(!n, span)?;
                    Ok(v)
                }
            }
        }
        ExprKind::Postfix(op, a) => {
            let mut v = eval(a, env)?;
            v.bare_unit = false;
            match op {
                PostOp::Percent => {
                    if !v.is_dimensionless() {
                        return err(ErrorKind::ExpectedDimensionless, &a.span);
                    }
                    v.si /= Decimal::ONE_HUNDRED;
                    Ok(v)
                }
                PostOp::Factorial => {
                    let n = to_int(&v, &a.span)?;
                    if n < 0 {
                        return err(ErrorKind::Domain, span);
                    }
                    let mut acc = Decimal::ONE;
                    for k in 2..=n {
                        acc = acc
                            .checked_mul(Decimal::from_i128(k).unwrap_or(Decimal::MAX))
                            .ok_or_else(|| CalcError::new(ErrorKind::Overflow, span.clone()))?;
                    }
                    v.si = acc;
                    Ok(v)
                }
            }
        }
        ExprKind::Binary(op, a, b) => binary(*op, a, b, span, env),
        ExprKind::Call { name, name_span, args } => call(name, name_span, args, span, env),
        ExprKind::Convert(a, target) => {
            let mut v = eval(a, env)?;
            v.bare_unit = false;
            match target {
                Target::Notation(n) => {
                    if let Notation::Radix(r) = n
                        && *r != Radix::Dec
                    {
                        to_int(&v, span)?;
                    }
                    v.notation = if *n == Notation::Radix(Radix::Dec) { Notation::Auto } else { *n };
                    Ok(v)
                }
                Target::Unit(u) => {
                    let t = eval(u, env)?;
                    let Some(mut hint) = t.unit.clone() else {
                        return err(ErrorKind::BadConversionTarget, &u.span);
                    };
                    if t.dim != v.dim {
                        return Err(mismatch(&v, &t, span));
                    }
                    if !t.bare_unit {
                        hint.offset = Decimal::ZERO;
                    }
                    v.unit = Some(hint);
                    Ok(v)
                }
            }
        }
    }
}

fn ident(name: &str, span: &Span, env: &Env) -> CalcResult<Value> {
    if let Some(v) = env.vars.get(name) {
        let mut v = v.clone();
        v.bare_unit = false;
        return Ok(v);
    }
    if let Some(c) = constant(name) {
        return Ok(Value::num(c));
    }
    if let Some(s) = special(name) {
        return match s {
            Special::Prev => env.prev.clone().ok_or_else(|| CalcError::new(ErrorKind::NoPrevious, span.clone())),
            Special::Sum | Special::Avg => {
                let Some(first) = env.block.first() else {
                    return Ok(Value::num(Decimal::ZERO));
                };
                let mut acc = first.clone();
                acc.bare_unit = false;
                for v in &env.block[1..] {
                    if v.dim != acc.dim {
                        return Err(mismatch(&acc, v, span));
                    }
                    acc.si = acc
                        .si
                        .checked_add(v.si)
                        .ok_or_else(|| CalcError::new(ErrorKind::Overflow, span.clone()))?;
                }
                if s == Special::Avg {
                    acc.si /= Decimal::from(env.block.len());
                }
                Ok(acc)
            }
        };
    }
    if let Some(u) = units::lookup(name) {
        return Ok(Value {
            si: u.factor,
            dim: u.dim,
            unit: Some(Box::new(UnitHint::atom(name, u.factor, u.offset))),
            notation: Notation::Auto,
            bare_unit: true,
        });
    }
    if function(name).is_some() {
        return err(ErrorKind::Arity { name: name.to_owned(), expected: "≥1", got: 0 }, span);
    }
    err(ErrorKind::UnknownIdent(name.to_owned()), span)
}

fn binary(op: BinOp, a: &Expr, b: &Expr, span: &Span, env: &Env) -> CalcResult<Value> {
    let lhs = eval(a, env)?;
    // `x + 10%` → x · 1.1
    if matches!(op, BinOp::Add | BinOp::Sub)
        && let ExprKind::Postfix(PostOp::Percent, inner) = &b.kind
    {
        let p = eval(inner, env)?;
        if !p.is_dimensionless() {
            return err(ErrorKind::ExpectedDimensionless, &inner.span);
        }
        let ratio = p.si / Decimal::ONE_HUNDRED;
        let k = if op == BinOp::Add { Decimal::ONE + ratio } else { Decimal::ONE - ratio };
        let mut out = lhs;
        out.bare_unit = false;
        out.si = out.si.checked_mul(k).ok_or_else(|| CalcError::new(ErrorKind::Overflow, span.clone()))?;
        return Ok(out);
    }
    let rhs = eval(b, env)?;
    apply_binary(op, lhs, rhs, span, &b.span)
}

pub fn apply_binary(op: BinOp, lhs: Value, rhs: Value, span: &Span, rhs_span: &Span) -> CalcResult<Value> {
    let overflow = || CalcError::new(ErrorKind::Overflow, span.clone());
    let notation = merge_notation(lhs.notation, rhs.notation);
    match op {
        BinOp::Add | BinOp::Sub => {
            if lhs.dim != rhs.dim {
                return Err(mismatch(&lhs, &rhs, span));
            }
            let si = if op == BinOp::Add { lhs.si.checked_add(rhs.si) } else { lhs.si.checked_sub(rhs.si) }
                .ok_or_else(overflow)?;
            let unit = lhs.unit.or(rhs.unit).map(|mut h| {
                h.offset = Decimal::ZERO;
                h
            });
            Ok(Value { si, dim: lhs.dim, unit, notation, bare_unit: false })
        }
        BinOp::Mul | BinOp::ImplicitMul => {
            // `20 °C`: a plain number times a bare offset unit applies the offset.
            if rhs.bare_unit
                && lhs.unit.is_none()
                && lhs.is_dimensionless()
                && let Some(h) = &rhs.unit
                && !h.offset.is_zero()
            {
                let si = (lhs.si + h.offset).checked_mul(h.factor).ok_or_else(overflow)?;
                return Ok(Value { si, dim: rhs.dim, unit: rhs.unit, notation, bare_unit: false });
            }
            let si = lhs.si.checked_mul(rhs.si).ok_or_else(overflow)?;
            let dim = lhs.dim * rhs.dim;
            let unit = combine_units(&lhs, &rhs, dim, '·');
            Ok(Value { si, dim, unit, notation, bare_unit: false })
        }
        BinOp::Div => {
            if rhs.si.is_zero() {
                return err(ErrorKind::DivisionByZero, rhs_span);
            }
            let si = lhs.si.checked_div(rhs.si).ok_or_else(overflow)?;
            let dim = lhs.dim / rhs.dim;
            let unit = combine_units(&lhs, &rhs, dim, '/');
            Ok(Value { si, dim, unit, notation, bare_unit: false })
        }
        BinOp::Mod => {
            if lhs.dim != rhs.dim {
                return Err(mismatch(&lhs, &rhs, span));
            }
            if rhs.si.is_zero() {
                return err(ErrorKind::DivisionByZero, rhs_span);
            }
            let si = lhs.si.checked_rem(rhs.si).ok_or_else(overflow)?;
            Ok(Value { si, dim: lhs.dim, unit: lhs.unit, notation, bare_unit: false })
        }
        BinOp::Pow => pow(lhs, rhs, span, rhs_span),
        BinOp::And | BinOp::Or | BinOp::Xor | BinOp::Shl | BinOp::Shr => {
            let x = to_int(&lhs, span)?;
            let y = to_int(&rhs, rhs_span)?;
            let r = match op {
                BinOp::And => x & y,
                BinOp::Or => x | y,
                BinOp::Xor => x ^ y,
                BinOp::Shl | BinOp::Shr => {
                    let sh = u32::try_from(y).ok().filter(|s| *s < 127).ok_or_else(overflow)?;
                    if op == BinOp::Shl {
                        let r = x.checked_shl(sh).ok_or_else(overflow)?;
                        if r >> sh != x {
                            return Err(overflow());
                        }
                        r
                    } else {
                        x >> sh
                    }
                }
                _ => unreachable!("guarded by outer match"),
            };
            let mut v = Value::num(from_int(r, span)?);
            v.notation = notation;
            Ok(v)
        }
    }
}

/// Derives a display unit for products/quotients.
fn combine_units(lhs: &Value, rhs: &Value, dim: Dim, sep: char) -> Option<Box<UnitHint>> {
    let sign: i8 = if sep == '/' { -1 } else { 1 };
    match (&lhs.unit, &rhs.unit) {
        // dimensionless scalar keeps the other side's unit: `2 * 5 km`, `30 deg`
        (Some(h), None) if rhs.is_dimensionless() => Some(Box::new(h.powi(1, h.factor))),
        (None, Some(h)) if lhs.is_dimensionless() && sign == 1 => Some(Box::new(h.powi(1, h.factor))),
        (None, Some(h)) if lhs.is_dimensionless() => {
            let factor = Decimal::ONE.checked_div(h.factor)?;
            Some(Box::new(h.powi(-1, factor)))
        }
        (Some(a), Some(b)) if !dim.is_none() => {
            let factor = if sign == -1 { a.factor.checked_div(b.factor)? } else { a.factor.checked_mul(b.factor)? };
            Some(Box::new(a.merge(b, sign, factor)))
        }
        _ => None,
    }
}

fn pow(base: Value, exp: Value, span: &Span, exp_span: &Span) -> CalcResult<Value> {
    if !exp.is_dimensionless() {
        return err(ErrorKind::ExpectedDimensionless, exp_span);
    }
    let int_exp = if exp.si.fract().is_zero() { exp.si.to_i64() } else { None };
    if !base.is_dimensionless() {
        let Some(n) = int_exp.and_then(|n| i8::try_from(n).ok()) else {
            return err(ErrorKind::ExpectedInteger, exp_span);
        };
        let si = powi_exact(base.si, i64::from(n), span)?;
        let unit = base.unit.as_ref().and_then(|h| {
            let factor = powi_exact(h.factor, i64::from(n), span).ok()?;
            Some(Box::new(h.powi(n, factor)))
        });
        return Ok(Value { si, dim: base.dim.powi(n), unit, notation: base.notation, bare_unit: false });
    }
    let si = match int_exp {
        Some(n) if n.unsigned_abs() <= 4096 => powi_exact(base.si, n, span)?,
        _ => {
            let (b, x) = (to_f64(base.si), to_f64(exp.si));
            if b < 0.0 {
                return err(ErrorKind::Domain, span);
            }
            from_f64(b.powf(x), span)?
        }
    };
    let mut v = Value::num(si);
    v.unit = base.unit;
    v.notation = base.notation;
    Ok(v)
}

fn arity(name: &str, expected: &'static str, got: usize, span: &Span) -> CalcError {
    CalcError::new(ErrorKind::Arity { name: name.to_owned(), expected, got }, span.clone())
}

fn call(name: &str, name_span: &Span, args: &[Expr], span: &Span, env: &Env) -> CalcResult<Value> {
    let Some(canon) = function(name).filter(|_| !env.vars.contains_key(name)) else {
        // `a(b + 1)` with a variable/unit `a` → implicit multiplication
        if args.len() == 1 && (env.vars.contains_key(name) || units::is_unit(name) || constant(name).is_some()) {
            let lhs = ident(name, name_span, env)?;
            let rhs = eval(&args[0], env)?;
            return apply_binary(BinOp::Mul, lhs, rhs, span, &args[0].span);
        }
        return err(ErrorKind::UnknownFunction(name.to_owned()), name_span);
    };
    let vals: Vec<Value> = args.iter().map(|a| eval(a, env)).collect::<CalcResult<_>>()?;
    let arg_span = |i: usize| args.get(i).map_or(span.clone(), |a| a.span.clone());
    let one = || -> CalcResult<&Value> {
        if vals.len() == 1 { Ok(&vals[0]) } else { Err(arity(name, "1", vals.len(), name_span)) }
    };
    // Unit-preserving magnitude operations act on the displayed magnitude.
    let map_mag = |v: &Value, f: &dyn Fn(Decimal) -> Decimal| -> Value {
        let mut out = v.clone();
        out.bare_unit = false;
        let shown = v.display_magnitude();
        let r = f(shown);
        out.si = match &v.unit {
            Some(h) => (r + h.offset) * h.factor,
            None => r,
        };
        out
    };
    let unary_f64 = |f: fn(f64) -> f64| -> CalcResult<Value> {
        let v = one()?;
        if !v.is_dimensionless() {
            return err(ErrorKind::ExpectedDimensionless, &arg_span(0));
        }
        Ok(Value::num(from_f64(f(to_f64(v.si)), span)?))
    };
    match canon {
        "sqrt" | "cbrt" => {
            let v = one()?;
            let n: i8 = if canon == "sqrt" { 2 } else { 3 };
            let dim = v.dim.root(n).ok_or_else(|| CalcError::new(ErrorKind::ExpectedDimensionless, arg_span(0)))?;
            let si = if n == 2 {
                if v.si.is_sign_negative() && !v.si.is_zero() {
                    return err(ErrorKind::Domain, &arg_span(0));
                }
                v.si.sqrt().ok_or_else(|| CalcError::new(ErrorKind::Domain, arg_span(0)))?
            } else {
                from_f64(to_f64(v.si).cbrt(), span)?
            };
            Ok(Value { si, dim, unit: None, notation: Notation::Auto, bare_unit: false })
        }
        "abs" => Ok(map_mag(one()?, &|d| d.abs())),
        "floor" => Ok(map_mag(one()?, &|d| d.floor())),
        "ceil" => Ok(map_mag(one()?, &|d| d.ceil())),
        "trunc" => Ok(map_mag(one()?, &|d| d.trunc())),
        "sign" => Ok(Value::num(one()?.si.signum())),
        "round" => match vals.len() {
            1 => Ok(map_mag(&vals[0], &|d| d.round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero))),
            2 => {
                let dp = to_int(&vals[1], &arg_span(1))?;
                let dp = u32::try_from(dp).map_err(|_| CalcError::new(ErrorKind::Domain, arg_span(1)))?.min(28);
                Ok(map_mag(&vals[0], &|d| d.round_dp_with_strategy(dp, RoundingStrategy::MidpointAwayFromZero)))
            }
            n => Err(arity(name, "1–2", n, name_span)),
        },
        "min" | "max" => {
            let Some(first) = vals.first() else { return Err(arity(name, "≥1", 0, name_span)) };
            let mut best = first.clone();
            for (i, v) in vals.iter().enumerate().skip(1) {
                if v.dim != best.dim {
                    return Err(mismatch(&best, v, &arg_span(i)));
                }
                if (canon == "min" && v.si < best.si) || (canon == "max" && v.si > best.si) {
                    best = v.clone();
                }
            }
            best.bare_unit = false;
            Ok(best)
        }
        "hypot" => {
            if vals.len() != 2 {
                return Err(arity(name, "2", vals.len(), name_span));
            }
            if vals[0].dim != vals[1].dim {
                return Err(mismatch(&vals[0], &vals[1], span));
            }
            let sq = vals[0].si.checked_mul(vals[0].si).and_then(|a| Some(a + vals[1].si.checked_mul(vals[1].si)?));
            let si = sq.and_then(|s| s.sqrt()).ok_or_else(|| CalcError::new(ErrorKind::Overflow, span.clone()))?;
            Ok(Value { si, dim: vals[0].dim, unit: vals[0].unit.clone(), notation: Notation::Auto, bare_unit: false })
        }
        "gcd" | "lcm" => {
            if vals.len() != 2 {
                return Err(arity(name, "2", vals.len(), name_span));
            }
            let (a, b) = (to_int(&vals[0], &arg_span(0))?.abs(), to_int(&vals[1], &arg_span(1))?.abs());
            let g = gcd(a, b);
            let r = if canon == "gcd" { g } else if g == 0 { 0 } else { a / g * b };
            Ok(Value::num(from_int(r, span)?))
        }
        "npr" | "ncr" => {
            if vals.len() != 2 {
                return Err(arity(name, "2", vals.len(), name_span));
            }
            let n = to_int(&vals[0], &arg_span(0))?;
            let r = to_int(&vals[1], &arg_span(1))?;
            if n < 0 || r < 0 || r > n {
                return err(ErrorKind::Domain, span);
            }
            let overflow = || CalcError::new(ErrorKind::Overflow, span.clone());
            let res = if canon == "npr" {
                let mut acc = Decimal::ONE;
                for k in (n - r + 1)..=n {
                    let dk = Decimal::from_i128(k).ok_or_else(overflow)?;
                    acc = acc.checked_mul(dk).ok_or_else(overflow)?;
                }
                acc
            } else {
                let k_lim = r.min(n - r);
                let mut acc = Decimal::ONE;
                for i in 1..=k_lim {
                    let num = Decimal::from_i128(n - k_lim + i).ok_or_else(overflow)?;
                    let den = Decimal::from_i128(i).ok_or_else(overflow)?;
                    acc = acc.checked_mul(num).ok_or_else(overflow)? / den;
                }
                acc.round_dp(0)
            };
            Ok(Value::num(res))
        }
        "sin" | "cos" | "tan" | "cot" => {
            let v = one()?;
            if !v.is_dimensionless() {
                return err(ErrorKind::ExpectedDimensionless, &arg_span(0));
            }
            let raw = to_f64(v.si);
            let rad = if v.unit.is_some() { raw } else { env.angle_unit.to_rad(raw) };
            let out = match canon {
                "sin" => rad.sin(),
                "cos" => rad.cos(),
                "tan" => rad.tan(),
                _ => 1.0 / rad.tan(),
            };
            Ok(Value::num(from_f64(out, span)?))
        }
        "asin" | "acos" | "atan" => {
            let v = one()?;
            if !v.is_dimensionless() {
                return err(ErrorKind::ExpectedDimensionless, &arg_span(0));
            }
            let raw = to_f64(v.si);
            let rad = match canon {
                "asin" => raw.asin(),
                "acos" => raw.acos(),
                _ => raw.atan(),
            };
            Ok(Value::num(from_f64(env.angle_unit.from_rad(rad), span)?))
        }
        "sinh" => unary_f64(f64::sinh),
        "cosh" => unary_f64(f64::cosh),
        "tanh" => unary_f64(f64::tanh),
        "asinh" => unary_f64(f64::asinh),
        "acosh" => unary_f64(f64::acosh),
        "atanh" => unary_f64(f64::atanh),
        "exp" => unary_f64(f64::exp),
        "ln" => unary_f64(|x| if x > 0.0 { x.ln() } else { f64::NAN }),
        "log2" => unary_f64(|x| if x > 0.0 { x.log2() } else { f64::NAN }),
        "log10" => unary_f64(|x| if x > 0.0 { x.log10() } else { f64::NAN }),
        "log" => match vals.len() {
            1 => unary_f64(|x| if x > 0.0 { x.log10() } else { f64::NAN }),
            2 => {
                if !vals[0].is_dimensionless() || !vals[1].is_dimensionless() {
                    return err(ErrorKind::ExpectedDimensionless, span);
                }
                let (x, b) = (to_f64(vals[0].si), to_f64(vals[1].si));
                let r = if x > 0.0 && b > 0.0 && b != 1.0 { x.ln() / b.ln() } else { f64::NAN };
                Ok(Value::num(from_f64(r, span)?))
            }
            n => Err(arity(name, "1–2", n, name_span)),
        },
        "atan2" => {
            if vals.len() != 2 {
                return Err(arity(name, "2", vals.len(), name_span));
            }
            if vals[0].dim != vals[1].dim {
                return Err(mismatch(&vals[0], &vals[1], span));
            }
            Ok(Value::num(from_f64(
                env.angle_unit.from_rad(to_f64(vals[0].si).atan2(to_f64(vals[1].si))),
                span,
            )?))
        }
        _ => err(ErrorKind::UnknownFunction(name.to_owned()), name_span),
    }
}

fn gcd(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Convenience: lex + parse + evaluate a standalone expression.
pub fn eval_str(src: &str, env: &Env) -> CalcResult<Value> {
    let toks = crate::lexer::lex(src);
    let expr = crate::parser::parse_expr(&toks, src.len())?;
    eval(&expr, env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::FormatOptions;

    fn show(src: &str) -> String {
        match eval_str(src, &Env::default()) {
            Ok(v) => v.plain(10),
            Err(e) => format!("ERR {:?}", e.kind),
        }
    }

    fn kind(src: &str) -> ErrorKind {
        eval_str(src, &Env::default()).expect_err("expected error").kind
    }

    #[test]
    fn exact_decimal_arithmetic() {
        assert_eq!(show("0.1 + 0.2"), "0.3");
        assert_eq!(show("1 / 3 * 3"), "1");
        assert_eq!(show("2^10"), "1024");
        assert_eq!(show("2^-2"), "0.25");
        assert_eq!(show("-2^2"), "-4");
        assert_eq!(show("(-2)^2"), "4");
        assert_eq!(show("10 mod 3"), "1");
        assert_eq!(show("7 % 4"), "3");
        assert_eq!(show("5!"), "120");
        assert_eq!(show("0!"), "1");
        assert_eq!(show("1_000 * 1.5"), "1500");
        assert_eq!(show("3²+4²"), "25");
    }

    #[test]
    fn percentages() {
        assert_eq!(show("50 + 10%"), "55");
        assert_eq!(show("200 - 25%"), "150");
        assert_eq!(show("20% of 50"), "10");
        assert_eq!(show("15% от 200"), "30");
        assert_eq!(show("5%"), "0.05");
    }

    #[test]
    fn functions_and_constants() {
        assert_eq!(show("sqrt(16)"), "4");
        assert_eq!(show("корень(2)^2"), "2");
        assert_eq!(show("sin(pi)"), "0");
        assert_eq!(show("sin(30 deg)"), "0.5");
        assert_eq!(show("sin(30°)"), "0.5");
        assert_eq!(show("cos(0)"), "1");
        assert_eq!(show("ln(e)"), "1");
        assert_eq!(show("log(1000)"), "3");
        assert_eq!(show("log(8, 2)"), "3");
        assert_eq!(show("max(1, 7, 3)"), "7");
        assert_eq!(show("min(2 km, 300 m)"), "300 m");
        assert_eq!(show("round(2.567, 2)"), "2.57");
        assert_eq!(show("round(2.5)"), "3");
        assert_eq!(show("abs(-3)"), "3");
        assert_eq!(show("gcd(12, 18)"), "6");
        assert_eq!(show("lcm(4, 6)"), "12");
        assert_eq!(show("2pi"), "6.2831853072");
        assert_eq!(show("2^0.5"), "1.4142135624");
    }

    #[test]
    fn units() {
        assert_eq!(show("120 km/h in m/s"), "33.3333333333 m/s");
        assert_eq!(show("120 км/ч в м/с"), "33.3333333333 м/с");
        assert_eq!(show("1 mi in km"), "1.609344 km");
        assert_eq!(show("5 km + 300 m"), "5.3 km");
        assert_eq!(show("2 * 5 km"), "10 km");
        assert_eq!(show("10 m / 2 s"), "5 m/s");
        assert_eq!(show("(10 m) / (2 s)"), "5 m/s");
        assert_eq!(show("3 m * 4 m"), "12 m²");
        assert_eq!(show("9.81 m/s/s"), "9.81 m/s²");
        assert_eq!(show("2 kg * 9.81 m/s²"), "19.62 kg·m/s²");
        assert_eq!(show("1 / 4 s"), "0.25 1/s");
        assert_eq!(show("3 m * 4 m in m²"), "12 m²");
        assert_eq!(show("1 ha in m^2"), "10000 m²");
        assert_eq!(show("1 GiB in MB"), "1073.741824 MB");
        assert_eq!(show("1 kWh in J"), "3600000 J");
        assert_eq!(show("sqrt(9 m²)"), "3 m");
        assert_eq!(show("1 h in min"), "60 min");
        assert_eq!(show("1 km / 1 m"), "1000");
        assert_eq!(show("100 N * 2 m"), "200 N·m");
        assert_eq!(show("100 N * 2 m in J"), "200 J");
        assert_eq!(show("90 deg in rad"), "1.5707963268 rad");
    }

    #[test]
    fn temperatures() {
        assert_eq!(show("20 °C in K"), "293.15 K");
        assert_eq!(show("100 °C in °F"), "212°F");
        assert_eq!(show("32 °F in °C"), "0°C");
        assert_eq!(show("-40 °C in °F"), "-40°F");
        assert_eq!(show("0 K in °C"), "-273.15°C");
        assert_eq!(show("20 °C"), "20°C");
    }

    #[test]
    fn programmer_notation() {
        assert_eq!(show("255 in hex"), "0xFF");
        assert_eq!(show("0xFF + 1"), "0x100");
        assert_eq!(show("0b1010 in dec"), "10");
        assert_eq!(show("0xF0 | 0x0F"), "0xFF");
        assert_eq!(show("0xFF & 0x0F"), "0xF");
        assert_eq!(show("6 xor 3"), "5");
        assert_eq!(show("1 << 10"), "1024");
        assert_eq!(show("1024 >> 3"), "128");
        assert_eq!(show("~0"), "-1");
        assert_eq!(show("8 in oct"), "0o10");
        assert_eq!(show("123456789 in sci"), "1.23456789e8");
    }

    #[test]
    fn errors() {
        assert_eq!(kind("1 / 0"), ErrorKind::DivisionByZero);
        assert_eq!(kind("1 m + 1 s"), ErrorKind::DimensionMismatch("m".into(), "s".into()));
        assert_eq!(kind("5 km in kg"), ErrorKind::DimensionMismatch("km".into(), "kg".into()));
        assert_eq!(kind("foo + 1"), ErrorKind::UnknownIdent("foo".into()));
        assert_eq!(kind("foo(1, 2)"), ErrorKind::UnknownFunction("foo".into()));
        assert_eq!(kind("sqrt(-1)"), ErrorKind::Domain);
        assert_eq!(kind("ln(0)"), ErrorKind::Domain);
        assert_eq!(kind("1.5 in hex"), ErrorKind::ExpectedInteger);
        assert_eq!(kind("1.5 & 1"), ErrorKind::ExpectedInteger);
        assert_eq!(kind("100!"), ErrorKind::Overflow);
        assert_eq!(kind("10^100"), ErrorKind::Overflow);
        assert_eq!(kind("(-1)^0.5"), ErrorKind::Domain);
        assert_eq!(kind("2^(1 m)"), ErrorKind::ExpectedDimensionless);
        assert_eq!(kind("sin(1 m)"), ErrorKind::ExpectedDimensionless);
        assert_eq!(kind("5 in 3"), ErrorKind::BadConversionTarget);
        assert_eq!(kind("#4"), ErrorKind::BadLineRef(4));
        assert_eq!(kind("ans"), ErrorKind::NoPrevious);
        assert!(matches!(kind("sqrt(1, 2)"), ErrorKind::Arity { .. }));
        assert!(matches!(kind("sqrt"), ErrorKind::Arity { .. }));
    }

    #[test]
    fn error_span_points_to_divisor() {
        let e = eval_str("10 / (5 - 5)", &Env::default()).expect_err("div0");
        assert_eq!(e.span, 5..12);
    }

    #[test]
    fn variables_shadow_units() {
        let mut env = Env::default();
        env.vars.insert("m".into(), Value::num(Decimal::from(7)));
        assert_eq!(eval_str("2m", &env).map(|v| v.plain(10)).ok().as_deref(), Some("14"));
        assert_eq!(eval_str("m(2 + 1)", &env).map(|v| v.plain(10)).ok().as_deref(), Some("21"));
    }

    #[test]
    fn display_grouping() {
        let v = eval_str("1234567.5", &Env::default()).expect("ok");
        assert_eq!(v.display(&FormatOptions::default()), "1 234 567.5");
    }

    #[test]
    fn angle_modes_and_combinatorics() {
        let mut env = Env { angle_unit: AngleUnit::Deg, ..Env::default() };
        assert_eq!(eval_str("sin(30)", &env).expect("ok").plain(10), "0.5");
        assert_eq!(eval_str("asin(0.5)", &env).expect("ok").plain(10), "30");
        assert_eq!(eval_str("cos(60)", &env).expect("ok").plain(10), "0.5");
        assert_eq!(eval_str("tan(45)", &env).expect("ok").plain(10), "1");
        env.angle_unit = AngleUnit::Grad;
        assert_eq!(eval_str("sin(100)", &env).expect("ok").plain(10), "1");

        assert_eq!(show("nPr(5, 3)"), "60");
        assert_eq!(show("nCr(10, 3)"), "120");
        assert_eq!(show("nCr(5, 0)"), "1");
        assert_eq!(kind("nCr(3, 5)"), ErrorKind::Domain);
    }
}
