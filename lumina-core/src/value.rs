//! Runtime value: an exact decimal magnitude in SI base units + dimension +
//! optional display unit and notation.

use crate::units::Dim;
use rust_decimal::prelude::*;

/// Preferred display unit (`km/h`, `°C`, …). `display = si / factor - offset`.
/// Stored as atoms with exponents so `m·m` renders as `m²`.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitHint {
    pub parts: Vec<(Box<str>, i8)>,
    pub factor: Decimal,
    pub offset: Decimal,
}

impl UnitHint {
    pub fn atom(name: &str, factor: Decimal, offset: Decimal) -> Self {
        Self { parts: vec![(name.into(), 1)], factor, offset }
    }

    /// `self^n` (offset dropped).
    pub fn powi(&self, n: i8, factor: Decimal) -> Self {
        let parts = self.parts.iter().map(|(a, e)| (a.clone(), e.saturating_mul(n))).filter(|p| p.1 != 0).collect();
        Self { parts, factor, offset: Decimal::ZERO }
    }

    /// `self · other^sign` (sign = 1 for product, -1 for quotient).
    pub fn merge(&self, other: &UnitHint, sign: i8, factor: Decimal) -> Self {
        let mut parts = self.parts.clone();
        for (name, e) in &other.parts {
            let e = e.saturating_mul(sign);
            match parts.iter_mut().find(|p| p.0 == *name) {
                Some(p) => p.1 = p.1.saturating_add(e),
                None => parts.push((name.clone(), e)),
            }
        }
        parts.retain(|p| p.1 != 0);
        Self { parts, factor, offset: Decimal::ZERO }
    }

    /// Renders `kg·m/s²`, `1/h`, `°C`.
    pub fn label(&self) -> String {
        use crate::units::superscript;
        let fmt = |filter: &dyn Fn(i8) -> bool, neg: bool| -> String {
            self.parts
                .iter()
                .filter(|p| filter(p.1))
                .map(|(a, e)| format!("{a}{}", superscript(if neg { -e } else { *e })))
                .collect::<Vec<_>>()
                .join("·")
        };
        let num = fmt(&|e| e > 0, false);
        let den = fmt(&|e| e < 0, true);
        match (num.is_empty(), den.is_empty()) {
            (_, true) => num,
            (true, false) => format!("1/{den}"),
            (false, false) => format!("{num}/{den}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Radix {
    #[default]
    Dec,
    Hex,
    Bin,
    Oct,
}

impl Radix {
    pub fn base(self) -> u32 {
        match self {
            Radix::Dec => 10,
            Radix::Hex => 16,
            Radix::Bin => 2,
            Radix::Oct => 8,
        }
    }
    pub fn prefix(self) -> &'static str {
        match self {
            Radix::Dec => "",
            Radix::Hex => "0x",
            Radix::Bin => "0b",
            Radix::Oct => "0o",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Notation {
    #[default]
    Auto,
    Sci,
    Radix(Radix),
}

impl Notation {
    /// Parses a conversion notation keyword (EN + RU aliases).
    pub fn parse_keyword(name: &str) -> Option<Self> {
        match name {
            "hex" | "шестн" => Some(Notation::Radix(Radix::Hex)),
            "bin" | "binary" | "двоич" => Some(Notation::Radix(Radix::Bin)),
            "oct" | "octal" | "восьм" => Some(Notation::Radix(Radix::Oct)),
            "dec" | "decimal" | "десят" => Some(Notation::Radix(Radix::Dec)),
            "sci" | "scientific" | "экспон" => Some(Notation::Sci),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    /// Magnitude in SI base units.
    pub si: Decimal,
    pub dim: Dim,
    pub unit: Option<Box<UnitHint>>,
    pub notation: Notation,
    /// `true` for a bare unit identifier (`km`) — enables offset units: `20 °C`.
    pub bare_unit: bool,
}

impl Value {
    #[inline]
    pub fn num(d: Decimal) -> Self {
        Self { si: d, dim: Dim::NONE, unit: None, notation: Notation::Auto, bare_unit: false }
    }

    #[inline]
    pub fn is_dimensionless(&self) -> bool {
        self.dim.is_none()
    }

    /// Magnitude expressed in the display unit (if any).
    pub fn display_magnitude(&self) -> Decimal {
        let res = match &self.unit {
            Some(h) if !h.factor.is_zero() => {
                let v = self.si.checked_div(h.factor).unwrap_or(self.si);
                let res = v - h.offset;
                // Collapse 28-digit repeating-fraction residue (e.g. `32 °F in °C` = 2e-26 -> 0).
                if !h.offset.is_zero() {
                    let eps = Decimal::new(1, 22) * h.offset.abs().max(Decimal::ONE);
                    if res.abs() < eps {
                        return Decimal::ZERO;
                    }
                }
                res
            }
            _ => self.si,
        };
        if res.is_zero() { Decimal::ZERO } else { res }
    }

    pub fn unit_label(&self) -> Option<String> {
        match &self.unit {
            Some(h) => Some(h.label()),
            None if !self.dim.is_none() => Some(self.dim.si_label()),
            None => None,
        }
    }

    /// Lossy conversion for plotting / transcendental functions.
    #[inline]
    pub fn to_f64(&self) -> f64 {
        self.si.to_f64().unwrap_or(f64::NAN)
    }

    /// Formatted for display (digit grouping, rounding, unit suffix).
    pub fn display(&self, opts: &FormatOptions) -> String {
        let mag = self.display_magnitude();
        let mut s = match self.notation {
            Notation::Radix(r) if r != Radix::Dec => {
                format_radix(mag, r, opts.grouping.is_some()).unwrap_or_else(|| format_decimal(mag, opts))
            }
            Notation::Sci => format_sci(mag, opts.precision),
            _ => format_decimal(mag, opts),
        };
        if let Some(label) = self.unit_label()
            && !label.is_empty()
        {
            if !label.starts_with('°') {
                s.push(' ');
            }
            s.push_str(&label);
        }
        s
    }

    /// Machine-friendly representation for the clipboard: no grouping, `.` decimal.
    pub fn plain(&self, precision: u32) -> String {
        self.display(&FormatOptions { precision, grouping: None })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormatOptions {
    /// Max fractional digits.
    pub precision: u32,
    /// Thousands separator for the integer part.
    pub grouping: Option<char>,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self { precision: 10, grouping: Some(' ') }
    }
}

const SCI_HIGH: Decimal = Decimal::from_parts(0xA4C6_8000, 0x0003_8D7E, 0, false, 0); // 1e15

/// Rounds to `precision` decimals, strips trailing zeros, groups thousands.
/// Switches to scientific notation for very large/small magnitudes.
pub fn format_decimal(d: Decimal, opts: &FormatOptions) -> String {
    let abs = d.abs();
    if !abs.is_zero() {
        let tiny = Decimal::new(1, opts.precision.min(28));
        if abs >= SCI_HIGH || abs < tiny {
            return format_sci(d, opts.precision.min(12));
        }
    }
    let r = d
        .round_dp_with_strategy(opts.precision, RoundingStrategy::MidpointAwayFromZero)
        .normalize();
    if r.is_zero() {
        return "0".to_owned();
    }
    let raw = r.to_string();
    let Some(sep) = opts.grouping else { return raw };
    let (sign, rest) = raw.strip_prefix('-').map_or(("", raw.as_str()), |r| ("-", r));
    let (int, frac) = rest.split_once('.').map_or((rest, None), |(i, f)| (i, Some(f)));
    let mut out = String::with_capacity(raw.len() + int.len() / 3 * sep.len_utf8());
    out.push_str(sign);
    let len = int.len();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 && len > 4 {
            out.push(sep);
        }
        out.push(ch);
    }
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    out
}

/// Scientific notation: `1.2345e20`.
pub fn format_sci(d: Decimal, sig: u32) -> String {
    let f = d.to_f64().unwrap_or(0.0);
    if f == 0.0 {
        return "0".to_owned();
    }
    let s = format!("{:.*e}", sig.min(15) as usize, f);
    match s.split_once('e') {
        Some((mant, exp)) => {
            let mant = if mant.contains('.') { mant.trim_end_matches('0').trim_end_matches('.') } else { mant };
            format!("{mant}e{exp}")
        }
        None => s,
    }
}

/// Integer in a given radix (`0xFF`, `0b1010 1010`). `None` if not an integer.
pub fn format_radix(d: Decimal, radix: Radix, group: bool) -> Option<String> {
    if !d.fract().is_zero() {
        return None;
    }
    let n = d.to_i128()?;
    let mag = n.unsigned_abs();
    let digits = match radix {
        Radix::Hex => format!("{mag:X}"),
        Radix::Bin => format!("{mag:b}"),
        Radix::Oct => format!("{mag:o}"),
        Radix::Dec => mag.to_string(),
    };
    let digits = if group && radix != Radix::Dec {
        let chunk = if radix == Radix::Oct { 3 } else { 4 };
        group_from_right(&digits, chunk, ' ')
    } else {
        digits
    };
    let sign = if n < 0 { "-" } else { "" };
    Some(format!("{sign}{}{digits}", radix.prefix()))
}

pub fn group_from_right(s: &str, chunk: usize, sep: char) -> String {
    let len = s.chars().count();
    let mut out = String::with_capacity(len + len / chunk.max(1));
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(chunk) {
            out.push(sep);
        }
        out.push(ch);
    }
    out
}

/// Approximates a non-integer `Decimal` as an irreducible rational `num / den`
/// with `1 < den <= max_den` using continued fractions. Returns `None` for
/// integers or irrationals that do not match within `1e-9`.
pub fn decimal_to_fraction(d: Decimal, max_den: i64) -> Option<(i64, i64)> {
    if d.fract().is_zero() || max_den < 2 {
        return None;
    }
    let val = d.to_f64()?;
    if !val.is_finite() || val.abs() > 1e12 {
        return None;
    }
    let sign = if val < 0.0 { -1i64 } else { 1i64 };
    let target = val.abs();

    let (mut p0, mut q0) = (0i64, 1i64);
    let (mut p1, mut q1) = (1i64, 0i64);
    let mut x = target;

    for _ in 0..32 {
        let a = x.floor();
        if !a.is_finite() || a > 1e12 {
            break;
        }
        let ai = a as i64;
        let p2 = ai.checked_mul(p1).and_then(|v| v.checked_add(p0))?;
        let q2 = ai.checked_mul(q1).and_then(|v| v.checked_add(q0))?;
        if q2 > max_den {
            break;
        }
        p0 = p1;
        q0 = q1;
        p1 = p2;
        q1 = q2;

        if q1 > 1 && ((p1 as f64 / q1 as f64) - target).abs() <= 1e-9 * target.max(1.0) {
            return Some((sign * p1, q1));
        }
        let frac = x - a;
        if frac < 1e-12 {
            break;
        }
        x = 1.0 / frac;
    }

    if q1 > 1 && ((p1 as f64 / q1 as f64) - target).abs() <= 1e-9 * target.max(1.0) {
        Some((sign * p1, q1))
    } else {
        None
    }
}

/// Engineering notation with exponent as a multiple of 3: `12.345×10³`, `4.5×10⁻³`.
pub fn format_engineering(d: Decimal, sig: u32) -> String {
    let f = d.to_f64().unwrap_or(0.0);
    if f == 0.0 || !f.is_finite() {
        return "0".to_owned();
    }
    let exp10 = f.abs().log10().floor() as i32;
    let eng_exp = exp10.div_euclid(3) * 3;
    let mant = f / 10f64.powi(eng_exp);
    let prec = sig.clamp(1, 12) as usize;
    let raw = format!("{mant:.prec$}");
    let clean = if raw.contains('.') {
        raw.trim_end_matches('0').trim_end_matches('.')
    } else {
        raw.as_str()
    };
    if eng_exp == 0 {
        clean.to_owned()
    } else {
        let sup = crate::units::superscript(eng_exp.clamp(-127, 127) as i8);
        format!("{clean}×10{sup}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).expect("decimal literal")
    }

    #[test]
    fn sci_threshold_constant() {
        assert_eq!(SCI_HIGH, d("1000000000000000"));
    }

    #[test]
    fn grouping_and_rounding() {
        let o = FormatOptions::default();
        assert_eq!(format_decimal(d("1234567.891"), &o), "1 234 567.891");
        assert_eq!(format_decimal(d("1234"), &o), "1234");
        assert_eq!(format_decimal(d("-12345.5"), &o), "-12 345.5");
        assert_eq!(format_decimal(d("0.30000000000000004"), &o), "0.3");
        assert_eq!(format_decimal(d("-0.00000000001"), &o), "-1e-11");
        assert_eq!(format_decimal(d("2.50"), &o), "2.5");
    }

    #[test]
    fn sci() {
        assert_eq!(format_sci(d("123450000000000000000"), 10), "1.2345e20");
        assert_eq!(format_sci(d("0.000012"), 10), "1.2e-5");
    }

    #[test]
    fn radix() {
        assert_eq!(format_radix(d("255"), Radix::Hex, false).as_deref(), Some("0xFF"));
        assert_eq!(format_radix(d("10"), Radix::Bin, false).as_deref(), Some("0b1010"));
        assert_eq!(format_radix(d("-8"), Radix::Oct, false).as_deref(), Some("-0o10"));
        assert_eq!(format_radix(d("170"), Radix::Bin, true).as_deref(), Some("0b1010 1010"));
        assert_eq!(format_radix(d("1.5"), Radix::Hex, false), None);
    }

    #[test]
    fn fraction_and_engineering() {
        // 1/2 + 2/3 + 3/4 = 23/12
        let v = d("1.9166666666666667");
        assert_eq!(decimal_to_fraction(v, 10_000), Some((23, 12)));
        assert_eq!(decimal_to_fraction(d("-0.75"), 10_000), Some((-3, 4)));
        assert_eq!(decimal_to_fraction(d("4"), 10_000), None);
        assert_eq!(decimal_to_fraction(Decimal::PI, 10_000), None);

        assert_eq!(format_engineering(d("12345"), 6), "12.345×10³");
        assert_eq!(format_engineering(d("0.0045"), 6), "4.5×10⁻³");
        assert_eq!(format_engineering(d("42"), 6), "42");
    }
}
