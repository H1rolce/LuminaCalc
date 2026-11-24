use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Dim(pub [i8; 8]);

const BASE_SYMBOLS: [&str; 8] = ["m", "kg", "s", "A", "K", "mol", "cd", "bit"];

impl Dim {
    pub const NONE: Dim = Dim([0; 8]);
    pub const LENGTH: Dim = Dim([1, 0, 0, 0, 0, 0, 0, 0]);
    pub const MASS: Dim = Dim([0, 1, 0, 0, 0, 0, 0, 0]);
    pub const TIME: Dim = Dim([0, 0, 1, 0, 0, 0, 0, 0]);
    pub const CURRENT: Dim = Dim([0, 0, 0, 1, 0, 0, 0, 0]);
    pub const TEMP: Dim = Dim([0, 0, 0, 0, 1, 0, 0, 0]);
    pub const AMOUNT: Dim = Dim([0, 0, 0, 0, 0, 1, 0, 0]);
    pub const LUMINOUS: Dim = Dim([0, 0, 0, 0, 0, 0, 1, 0]);
    pub const INFO: Dim = Dim([0, 0, 0, 0, 0, 0, 0, 1]);

    #[inline]
    pub fn is_none(self) -> bool {
        self.0 == [0; 8]
    }

    #[inline]
    pub fn powi(self, n: i8) -> Dim {
        let mut r = [0i8; 8];
        for (i, v) in r.iter_mut().enumerate() {
            *v = self.0[i].saturating_mul(n);
        }
        Dim(r)
    }

    pub fn root(self, n: i8) -> Option<Dim> {
        if n == 0 {
            return None;
        }
        let mut r = [0i8; 8];
        for (i, v) in r.iter_mut().enumerate() {
            if self.0[i] % n != 0 {
                return None;
            }
            *v = self.0[i] / n;
        }
        Some(Dim(r))
    }

    pub fn si_label(self) -> String {
        if let Some(name) = DERIVED.iter().find(|(d, _)| *d == self).map(|(_, n)| *n) {
            return name.to_owned();
        }
        let mut num = Vec::new();
        let mut den = Vec::new();
        for (i, &e) in self.0.iter().enumerate() {
            match e.cmp(&0) {
                std::cmp::Ordering::Greater => num.push((BASE_SYMBOLS[i], e)),
                std::cmp::Ordering::Less => den.push((BASE_SYMBOLS[i], -e)),
                std::cmp::Ordering::Equal => {}
            }
        }
        let join = |parts: &[(&str, i8)]| -> String {
            parts
                .iter()
                .map(|(s, e)| format!("{s}{}", superscript(*e)))
                .collect::<Vec<_>>()
                .join("·")
        };
        match (num.is_empty(), den.is_empty()) {
            (true, true) => String::new(),
            (false, true) => join(&num),
            (true, false) => format!("1/{}", join(&den)),
            (false, false) => format!("{}/{}", join(&num), join(&den)),
        }
    }
}

impl std::ops::Mul for Dim {
    type Output = Dim;

    #[inline]
    fn mul(self, o: Dim) -> Dim {
        let mut r = [0i8; 8];
        for (i, v) in r.iter_mut().enumerate() {
            *v = self.0[i].saturating_add(o.0[i]);
        }
        Dim(r)
    }
}

impl std::ops::Div for Dim {
    type Output = Dim;

    #[inline]
    fn div(self, o: Dim) -> Dim {
        let mut r = [0i8; 8];
        for (i, v) in r.iter_mut().enumerate() {
            *v = self.0[i].saturating_sub(o.0[i]);
        }
        Dim(r)
    }
}

const DERIVED: &[(Dim, &str)] = &[
    (Dim([1, 1, -2, 0, 0, 0, 0, 0]), "N"),
    (Dim([2, 1, -2, 0, 0, 0, 0, 0]), "J"),
    (Dim([2, 1, -3, 0, 0, 0, 0, 0]), "W"),
    (Dim([-1, 1, -2, 0, 0, 0, 0, 0]), "Pa"),
    (Dim([2, 1, -3, -1, 0, 0, 0, 0]), "V"),
    (Dim([2, 1, -3, -2, 0, 0, 0, 0]), "Ω"),
    (Dim([0, 0, 1, 1, 0, 0, 0, 0]), "C"),
];

pub fn superscript(e: i8) -> String {
    if e == 1 {
        return String::new();
    }
    const DIGITS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    let mut out = String::new();
    if e < 0 {
        out.push('⁻');
    }
    for c in e.unsigned_abs().to_string().chars() {
        if let Some(d) = c.to_digit(10) {
            out.push(DIGITS[d as usize]);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnitDef {
    pub canonical: &'static str,
    pub dim: Dim,
    pub factor: Decimal,
    pub offset: Decimal,
}

type Row = (&'static [&'static str], Dim, &'static str, &'static str, &'static str);

const L: Dim = Dim::LENGTH;
const M: Dim = Dim::MASS;
const T: Dim = Dim::TIME;
const AREA: Dim = Dim([2, 0, 0, 0, 0, 0, 0, 0]);
const VOLUME: Dim = Dim([3, 0, 0, 0, 0, 0, 0, 0]);
const SPEED: Dim = Dim([1, 0, -1, 0, 0, 0, 0, 0]);
const FORCE: Dim = Dim([1, 1, -2, 0, 0, 0, 0, 0]);
const ENERGY: Dim = Dim([2, 1, -2, 0, 0, 0, 0, 0]);
const POWER: Dim = Dim([2, 1, -3, 0, 0, 0, 0, 0]);
const PRESSURE: Dim = Dim([-1, 1, -2, 0, 0, 0, 0, 0]);
const VOLT: Dim = Dim([2, 1, -3, -1, 0, 0, 0, 0]);
const OHM: Dim = Dim([2, 1, -3, -2, 0, 0, 0, 0]);
const CHARGE: Dim = Dim([0, 0, 1, 1, 0, 0, 0, 0]);
const FREQ: Dim = Dim([0, 0, -1, 0, 0, 0, 0, 0]);
const NONE: Dim = Dim::NONE;

#[rustfmt::skip]
const TABLE: &[Row] = &[
    (&["m", "meter", "meters", "metre", "metres", "м", "метр", "метра", "метров"], L, "1", "1", "0"),
    (&["km", "kilometer", "kilometers", "км", "километр", "километра", "километров"], L, "1000", "1", "0"),
    (&["dm", "дм"], L, "0.1", "1", "0"),
    (&["cm", "centimeter", "centimeters", "см", "сантиметр", "сантиметров"], L, "0.01", "1", "0"),
    (&["mm", "millimeter", "millimeters", "мм", "миллиметр", "миллиметров"], L, "0.001", "1", "0"),
    (&["µm", "um", "micrometer", "мкм"], L, "0.000001", "1", "0"),
    (&["nm", "nanometer", "нм"], L, "0.000000001", "1", "0"),
    (&["mi", "mile", "miles", "миля", "мили", "миль"], L, "1609.344", "1", "0"),
    (&["yd", "yard", "yards", "ярд", "ярдов"], L, "0.9144", "1", "0"),
    (&["ft", "foot", "feet", "фут", "фута", "футов"], L, "0.3048", "1", "0"),
    (&["inch", "inches", "дюйм", "дюйма", "дюймов"], L, "0.0254", "1", "0"),
    (&["nmi", "мормиля"], L, "1852", "1", "0"),
    (&["au", "ае"], L, "149597870700", "1", "0"),
    (&["ly", "lightyear", "lightyears", "свгод"], L, "9460730472580800", "1", "0"),
    (&["kg", "kilogram", "kilograms", "кг", "килограмм", "килограмма", "килограммов"], M, "1", "1", "0"),
    (&["g", "gram", "grams", "г", "грамм", "грамма", "граммов"], M, "0.001", "1", "0"),
    (&["mg", "milligram", "мг"], M, "0.000001", "1", "0"),
    (&["µg", "ug", "мкг"], M, "0.000000001", "1", "0"),
    (&["tonne", "tonnes", "т", "тонна", "тонны", "тонн"], M, "1000", "1", "0"),
    (&["lb", "lbs", "pound", "pounds", "фунт", "фунта", "фунтов"], M, "0.45359237", "1", "0"),
    (&["oz", "ounce", "ounces", "унция", "унций"], M, "0.028349523125", "1", "0"),
    (&["s", "sec", "second", "seconds", "с", "сек", "секунда", "секунды", "секунд"], T, "1", "1", "0"),
    (&["ms", "millisecond", "milliseconds", "мс"], T, "0.001", "1", "0"),
    (&["µs", "us", "microsecond", "мкс"], T, "0.000001", "1", "0"),
    (&["ns", "nanosecond", "нс"], T, "0.000000001", "1", "0"),
    (&["min", "minute", "minutes", "мин", "минута", "минуты", "минут"], T, "60", "1", "0"),
    (&["h", "hr", "hour", "hours", "ч", "час", "часа", "часов"], T, "3600", "1", "0"),
    (&["d", "day", "days", "сут", "день", "дня", "дней"], T, "86400", "1", "0"),
    (&["wk", "week", "weeks", "нед", "неделя", "недели", "недель"], T, "604800", "1", "0"),
    (&["yr", "year", "years", "год", "года", "лет"], T, "31557600", "1", "0"),
    (&["kn", "knot", "knots", "узел", "узла", "узлов"], SPEED, "1852", "3600", "0"),
    (&["mph"], SPEED, "1609.344", "3600", "0"),
    (&["kph", "kmh"], SPEED, "1000", "3600", "0"),
    (&["ha", "hectare", "hectares", "га", "гектар", "гектаров"], AREA, "10000", "1", "0"),
    (&["acre", "acres", "акр", "акров"], AREA, "4046.8564224", "1", "0"),
    (&["L", "l", "liter", "liters", "litre", "litres", "л", "литр", "литра", "литров"], VOLUME, "0.001", "1", "0"),
    (&["mL", "ml", "milliliter", "мл"], VOLUME, "0.000001", "1", "0"),
    (&["gal", "gallon", "gallons", "галлон", "галлонов"], VOLUME, "0.003785411784", "1", "0"),
    (&["N", "newton", "newtons", "Н", "ньютон"], FORCE, "1", "1", "0"),
    (&["kN", "кН"], FORCE, "1000", "1", "0"),
    (&["J", "joule", "joules", "Дж", "джоуль"], ENERGY, "1", "1", "0"),
    (&["kJ", "кДж"], ENERGY, "1000", "1", "0"),
    (&["cal", "calorie", "calories", "кал"], ENERGY, "4.184", "1", "0"),
    (&["kcal", "ккал"], ENERGY, "4184", "1", "0"),
    (&["Wh", "Втч"], ENERGY, "3600", "1", "0"),
    (&["kWh", "кВтч"], ENERGY, "3600000", "1", "0"),
    (&["eV", "эВ"], ENERGY, "0.0000000000000000001602176634", "1", "0"),
    (&["W", "watt", "watts", "Вт", "ватт"], POWER, "1", "1", "0"),
    (&["kW", "кВт"], POWER, "1000", "1", "0"),
    (&["MW", "МВт"], POWER, "1000000", "1", "0"),
    (&["hp", "лс"], POWER, "745.69987158227022", "1", "0"),
    (&["Pa", "pascal", "Па"], PRESSURE, "1", "1", "0"),
    (&["kPa", "кПа"], PRESSURE, "1000", "1", "0"),
    (&["MPa", "МПа"], PRESSURE, "1000000", "1", "0"),
    (&["bar", "бар"], PRESSURE, "100000", "1", "0"),
    (&["atm", "атм"], PRESSURE, "101325", "1", "0"),
    (&["psi"], PRESSURE, "6894.757293168361", "1", "0"),
    (&["mmHg", "ммртст"], PRESSURE, "133.322387415", "1", "0"),
    (&["K", "kelvin", "К", "кельвин"], Dim::TEMP, "1", "1", "0"),
    (&["°C", "degC", "celsius", "цельсий"], Dim::TEMP, "1", "1", "273.15"),
    (&["°F", "degF", "fahrenheit", "фаренгейт"], Dim::TEMP, "5", "9", "459.67"),
    (&["A", "amp", "ampere", "amperes", "А", "ампер"], Dim::CURRENT, "1", "1", "0"),
    (&["mA", "мА"], Dim::CURRENT, "0.001", "1", "0"),
    (&["V", "volt", "volts", "В", "вольт"], VOLT, "1", "1", "0"),
    (&["mV", "мВ"], VOLT, "0.001", "1", "0"),
    (&["kV", "кВ"], VOLT, "1000", "1", "0"),
    (&["Ω", "ohm", "ohms", "Ом"], OHM, "1", "1", "0"),
    (&["kΩ", "kohm", "кОм"], OHM, "1000", "1", "0"),
    (&["C", "coulomb", "Кл"], CHARGE, "1", "1", "0"),
    (&["mAh", "мАч"], CHARGE, "3.6", "1", "0"),
    (&["Hz", "hertz", "Гц"], FREQ, "1", "1", "0"),
    (&["kHz", "кГц"], FREQ, "1000", "1", "0"),
    (&["MHz", "МГц"], FREQ, "1000000", "1", "0"),
    (&["GHz", "ГГц"], FREQ, "1000000000", "1", "0"),
    (&["rpm", "обмин"], FREQ, "1", "60", "0"),
    (&["mol", "моль"], Dim::AMOUNT, "1", "1", "0"),
    (&["cd", "candela", "кд"], Dim::LUMINOUS, "1", "1", "0"),
    (&["rad", "radian", "radians", "рад"], NONE, "1", "1", "0"),
    (&["deg", "degree", "degrees", "°", "град", "градус", "градуса", "градусов"], NONE, "pi", "180", "0"),
    (&["bit", "bits", "бит", "бита", "битов"], Dim::INFO, "1", "1", "0"),
    (&["B", "byte", "bytes", "Б", "байт", "байта", "байтов"], Dim::INFO, "8", "1", "0"),
    (&["kB", "KB", "кБ", "КБ"], Dim::INFO, "8000", "1", "0"),
    (&["MB", "МБ"], Dim::INFO, "8000000", "1", "0"),
    (&["GB", "ГБ"], Dim::INFO, "8000000000", "1", "0"),
    (&["TB", "ТБ"], Dim::INFO, "8000000000000", "1", "0"),
    (&["KiB", "КиБ"], Dim::INFO, "8192", "1", "0"),
    (&["MiB", "МиБ"], Dim::INFO, "8388608", "1", "0"),
    (&["GiB", "ГиБ"], Dim::INFO, "8589934592", "1", "0"),
    (&["TiB", "ТиБ"], Dim::INFO, "8796093022208", "1", "0"),
];

fn parse_factor(s: &str) -> Option<Decimal> {
    if s == "pi" {
        Some(Decimal::PI)
    } else {
        Decimal::from_str(s).ok()
    }
}

fn registry() -> &'static HashMap<&'static str, UnitDef> {
    static REG: OnceLock<HashMap<&'static str, UnitDef>> = OnceLock::new();
    REG.get_or_init(|| {
        let mut map = HashMap::with_capacity(TABLE.len() * 6);
        for &(names, dim, num, den, off) in TABLE {
            let (Some(n), Some(d), Some(o)) = (parse_factor(num), parse_factor(den), parse_factor(off))
            else {
                debug_assert!(false, "bad unit row {names:?}");
                continue;
            };
            let Some(factor) = n.checked_div(d) else { continue };
            let def = UnitDef { canonical: names[0], dim, factor, offset: o };
            for &alias in names {
                map.insert(alias, def.clone());
            }
        }
        map
    })
}

#[inline]
pub fn lookup(name: &str) -> Option<&'static UnitDef> {
    registry().get(name)
}

#[inline]
pub fn is_unit(name: &str) -> bool {
    registry().contains_key(name)
}

pub fn all_aliases() -> impl Iterator<Item = &'static str> {
    TABLE.iter().flat_map(|r| r.0.iter().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_registered() {
        for row in TABLE {
            for alias in row.0 {
                assert!(lookup(alias).is_some(), "alias {alias} missing");
            }
        }
    }

    #[test]
    fn no_duplicate_aliases() {
        let mut seen = std::collections::HashSet::new();
        for a in all_aliases() {
            assert!(seen.insert(a), "duplicate alias {a}");
        }
    }

    #[test]
    fn dim_algebra() {
        let speed = Dim::LENGTH / Dim::TIME;
        assert_eq!(speed, SPEED);
        assert_eq!(speed * Dim::TIME, Dim::LENGTH);
        assert_eq!(Dim::LENGTH.powi(2), AREA);
        assert_eq!(AREA.root(2), Some(Dim::LENGTH));
        assert_eq!(Dim::LENGTH.root(2), None);
    }

    #[test]
    fn labels() {
        assert_eq!(FORCE.si_label(), "N");
        assert_eq!(Dim([1, 0, -2, 0, 0, 0, 0, 0]).si_label(), "m/s²");
        assert_eq!(FREQ.si_label(), "1/s");
        assert_eq!(AREA.si_label(), "m²");
        assert_eq!(superscript(-2), "⁻²");
    }

    #[test]
    fn fahrenheit_exact() {
        let f = lookup("°F").expect("°F");
        let si = (Decimal::from(32) + f.offset) * f.factor;
        assert_eq!(si.round_dp(20), Decimal::from_str("273.15").expect("lit"));
    }
}
