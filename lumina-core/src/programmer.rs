//! Programmer & bit-manipulation core (8 / 16 / 32 / 64-bit, signed / unsigned).

use crate::error::CalcResult;
use crate::eval::{Env, eval_str};
use crate::value::{Radix, group_from_right};
use rust_decimal::prelude::ToPrimitive;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BitWidth {
    Bits8 = 8,
    Bits16 = 16,
    Bits32 = 32,
    #[default]
    Bits64 = 64,
}

impl BitWidth {
    pub const ALL: [BitWidth; 4] = [
        BitWidth::Bits64,
        BitWidth::Bits32,
        BitWidth::Bits16,
        BitWidth::Bits8,
    ];

    #[inline]
    pub const fn from_bits(bits: u8) -> Self {
        match bits {
            8 => BitWidth::Bits8,
            16 => BitWidth::Bits16,
            32 => BitWidth::Bits32,
            _ => BitWidth::Bits64,
        }
    }

    #[inline]
    pub const fn bits(self) -> u32 {
        self as u32
    }

    #[inline]
    pub const fn mask(self) -> u64 {
        match self {
            BitWidth::Bits8 => 0xFF,
            BitWidth::Bits16 => 0xFFFF,
            BitWidth::Bits32 => 0xFFFF_FFFF,
            BitWidth::Bits64 => u64::MAX,
        }
    }

    #[inline]
    pub const fn label(self) -> &'static str {
        match self {
            BitWidth::Bits8 => "BYTE (8)",
            BitWidth::Bits16 => "WORD (16)",
            BitWidth::Bits32 => "DWORD (32)",
            BitWidth::Bits64 => "QWORD (64)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BitRegister {
    pub raw: u64,
    pub width: BitWidth,
    pub signed: bool,
}

impl BitRegister {
    #[inline]
    pub fn new(raw: u64, width: BitWidth, signed: bool) -> Self {
        Self { raw: raw & width.mask(), width, signed }
    }

    #[inline]
    pub fn masked(self) -> u64 {
        self.raw & self.width.mask()
    }

    #[inline]
    pub fn set_width(&mut self, width: BitWidth) {
        self.width = width;
        self.raw &= width.mask();
    }

    #[inline]
    pub fn set_raw(&mut self, raw: u64) {
        self.raw = raw & self.width.mask();
    }

    #[inline]
    pub fn get_bit(self, bit: u32) -> bool {
        if bit >= self.width.bits() {
            return false;
        }
        (self.masked() >> bit) & 1 == 1
    }

    #[inline]
    pub fn toggle_bit(&mut self, bit: u32) {
        if bit < self.width.bits() {
            self.raw = (self.raw ^ (1u64 << bit)) & self.width.mask();
        }
    }

    #[inline]
    pub fn bit_not(&mut self) {
        self.raw = (!self.raw) & self.width.mask();
    }

    #[inline]
    pub fn negate(&mut self) {
        self.raw = self.raw.wrapping_neg() & self.width.mask();
    }

    #[inline]
    pub fn shl(&mut self, n: u32) {
        if n >= self.width.bits() {
            self.raw = 0;
        } else {
            self.raw = (self.masked() << n) & self.width.mask();
        }
    }

    #[inline]
    pub fn shr(&mut self, n: u32) {
        let bits = self.width.bits();
        if self.signed {
            let s = self.as_signed();
            let shifted = if n >= bits {
                if s < 0 { -1 } else { 0 }
            } else {
                s >> n
            };
            self.raw = (shifted as u64) & self.width.mask();
        } else if n >= bits {
            self.raw = 0;
        } else {
            self.raw = self.masked() >> n;
        }
    }

    #[inline]
    pub fn rol(&mut self, n: u32) {
        let bits = self.width.bits();
        let k = n % bits;
        let v = self.masked();
        self.raw = ((v << k) | (v >> ((bits - k) % bits))) & self.width.mask();
    }

    #[inline]
    pub fn ror(&mut self, n: u32) {
        let bits = self.width.bits();
        let k = n % bits;
        let v = self.masked();
        self.raw = ((v >> k) | (v << ((bits - k) % bits))) & self.width.mask();
    }

    #[inline]
    pub fn clear(&mut self) {
        self.raw = 0;
    }

    #[inline]
    pub fn as_signed(self) -> i64 {
        let v = self.masked();
        match self.width {
            BitWidth::Bits8 => (v as u8) as i8 as i64,
            BitWidth::Bits16 => (v as u16) as i16 as i64,
            BitWidth::Bits32 => (v as u32) as i32 as i64,
            BitWidth::Bits64 => v as i64,
        }
    }

    pub fn format_dec(self, grouped: bool) -> String {
        let s = if self.signed {
            self.as_signed().to_string()
        } else {
            self.masked().to_string()
        };
        if !grouped {
            return s;
        }
        if let Some(rest) = s.strip_prefix('-') {
            format!("-{}", group_from_right(rest, 3, ' '))
        } else {
            group_from_right(&s, 3, ' ')
        }
    }

    pub fn format_hex(self, grouped: bool) -> String {
        let s = format!("{:X}", self.masked());
        if grouped { group_from_right(&s, 4, ' ') } else { s }
    }

    pub fn format_oct(self, grouped: bool) -> String {
        let s = format!("{:o}", self.masked());
        if grouped { group_from_right(&s, 3, ' ') } else { s }
    }

    pub fn format_bin(self, grouped: bool) -> String {
        let s = format!("{:b}", self.masked());
        if grouped { group_from_right(&s, 4, ' ') } else { s }
    }

    pub fn format_radix(self, radix: Radix, grouped: bool) -> String {
        match radix {
            Radix::Dec => self.format_dec(grouped),
            Radix::Hex => self.format_hex(grouped),
            Radix::Oct => self.format_oct(grouped),
            Radix::Bin => self.format_bin(grouped),
        }
    }

    /// Parses user input in a specific radix (tolerating spaces, `_`, and `0x`/`0b`/`0o` prefixes).
    pub fn parse_radix(&mut self, input: &str, radix: Radix) -> bool {
        let cleaned: String = input
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '_')
            .collect();
        if cleaned.is_empty() {
            self.raw = 0;
            return true;
        }
        let stripped = match radix {
            Radix::Hex => cleaned.strip_prefix("0x").or_else(|| cleaned.strip_prefix("0X")).unwrap_or(&cleaned),
            Radix::Bin => cleaned.strip_prefix("0b").or_else(|| cleaned.strip_prefix("0B")).unwrap_or(&cleaned),
            Radix::Oct => cleaned.strip_prefix("0o").or_else(|| cleaned.strip_prefix("0O")).unwrap_or(&cleaned),
            Radix::Dec => &cleaned,
        };
        if radix == Radix::Dec && stripped.starts_with('-') {
            if let Ok(v) = stripped.parse::<i64>() {
                self.raw = (v as u64) & self.width.mask();
                return true;
            }
            return false;
        }
        if let Ok(v) = u64::from_str_radix(stripped, radix.base()) {
            self.raw = v & self.width.mask();
            return true;
        }
        false
    }

    /// Evaluates a bitwise/arithmetic expression with `x` bound to the current register
    /// value and stores the truncated integer result.
    pub fn eval_and_apply(&mut self, expr: &str) -> CalcResult<u64> {
        let mut env = Env::default();
        let current = if self.signed {
            rust_decimal::Decimal::from(self.as_signed())
        } else {
            rust_decimal::Decimal::from(self.masked())
        };
        env.vars.insert("x".into(), crate::value::Value::num(current));
        env.prev = Some(crate::value::Value::num(current));
        let val = eval_str(expr, &env)?;
        let n = val.si.trunc().to_i128().unwrap_or(0);
        self.raw = (n as u64) & self.width.mask();
        Ok(self.raw)
    }

    /// Byte count of active bits / popcount for inspection bar.
    #[inline]
    pub fn popcount(self) -> u32 {
        self.masked().count_ones()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_and_format() {
        let mut r = BitRegister::new(0, BitWidth::Bits32, false);
        r.toggle_bit(0);
        r.toggle_bit(7);
        assert_eq!(r.masked(), 0x81);
        assert_eq!(r.format_hex(false), "81");
        assert_eq!(r.format_bin(true), "1000 0001");
        assert_eq!(r.format_oct(false), "201");
        assert_eq!(r.popcount(), 2);
    }

    #[test]
    fn signed_and_arithmetic_shift() {
        let mut r = BitRegister::new(0x80, BitWidth::Bits8, true);
        assert_eq!(r.as_signed(), -128);
        assert_eq!(r.format_dec(false), "-128");
        r.shr(1);
        assert_eq!(r.as_signed(), -64);
        assert_eq!(r.masked(), 0xC0);
    }

    #[test]
    fn rotates_and_not() {
        let mut r = BitRegister::new(0x81, BitWidth::Bits8, false);
        r.rol(1);
        assert_eq!(r.masked(), 0x03);
        r.ror(1);
        assert_eq!(r.masked(), 0x81);
        r.bit_not();
        assert_eq!(r.masked(), 0x7E);
    }

    #[test]
    fn parse_and_expr() {
        let mut r = BitRegister::new(0, BitWidth::Bits32, false);
        assert!(r.parse_radix("0xDE AD_BE EF", Radix::Hex));
        assert_eq!(r.masked(), 0xDEAD_BEEF);
        assert!(r.parse_radix("-1", Radix::Dec));
        assert_eq!(r.masked(), 0xFFFF_FFFF);
        r.eval_and_apply("(0x0F << 4) | 0x05").expect("eval");
        assert_eq!(r.masked(), 0xF5);
    }
}
