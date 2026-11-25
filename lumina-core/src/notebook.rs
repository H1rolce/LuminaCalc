use crate::error::{CalcError, CalcResult, ErrorKind};
use crate::eval::{self, Env, Special};
use crate::highlight::{self, HighlightSpan};
use crate::lexer::{is_hash_comment, lex};
use crate::parser::{ExprKind, Stmt, parse_stmt};
use crate::value::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub enum LineOutput {
    Empty,
    Value(Value),
    Error(CalcError),
}

impl LineOutput {
    #[inline]
    pub fn value(&self) -> Option<&Value> {
        match self {
            LineOutput::Value(v) => Some(v),
            _ => None,
        }
    }

    #[inline]
    pub fn error(&self) -> Option<&CalcError> {
        match self {
            LineOutput::Error(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LineEntry {
    pub source: String,
    pub byte_start: usize,
    pub raw_len: usize,
    parsed: CalcResult<Stmt>,
    pub highlights: Vec<HighlightSpan>,
    pub output: LineOutput,
    pub assigned_var: Option<Box<str>>,
    pub is_heading: bool,
}

#[derive(Debug, Clone)]
pub struct Notebook {
    raw_text: String,
    lines: Vec<LineEntry>,
    env: Env,
    version: u64,
}

impl Default for Notebook {
    fn default() -> Self {
        Self::new()
    }
}

impl Notebook {
    pub fn new() -> Self {
        Self {
            raw_text: String::new(),
            lines: Vec::new(),
            env: Env::default(),
            version: 0,
        }
    }

    pub fn from_text(text: &str) -> Self {
        let mut nb = Self::new();
        nb.sync(text);
        nb
    }

    #[inline]
    pub fn version(&self) -> u64 {
        self.version
    }

    #[inline]
    pub fn lines(&self) -> &[LineEntry] {
        &self.lines
    }

    #[inline]
    pub fn env(&self) -> &Env {
        &self.env
    }

    #[inline]
    pub fn text(&self) -> &str {
        &self.raw_text
    }

    pub fn sync(&mut self, text: &str) -> bool {
        if text == self.raw_text && (!self.lines.is_empty() || text.is_empty() && self.version > 0) {
            return false;
        }

        let old_lines = std::mem::take(&mut self.lines);
        let mut old_by_text: std::collections::HashMap<String, CalcResult<Stmt>> =
            std::collections::HashMap::with_capacity(old_lines.len());
        for entry in old_lines {
            old_by_text.entry(entry.source).or_insert(entry.parsed);
        }

        let mut new_lines = Vec::new();
        let mut env = Env::default();
        let mut defined_vars: HashSet<Box<str>> = HashSet::new();

        let mut offset = 0usize;
        for raw_segment in text.split('\n') {
            let raw_len = raw_segment.len();
            let clean = raw_segment.strip_suffix('\r').unwrap_or(raw_segment);
            let trimmed = clean.trim_start();
            let is_heading = is_hash_comment(trimmed);

            let tokens = lex(clean);
            let highlights = highlight::classify(&tokens, &defined_vars, false);

            let parsed = match old_by_text.get(clean) {
                Some(cached) => cached.clone(),
                None => parse_stmt(&tokens, clean.len()),
            };

            let mut assigned_var = None;
            let output = match &parsed {
                Err(err) => {
                    env.lines.push(None);
                    LineOutput::Error(err.clone())
                }
                Ok(Stmt::Empty) => {
                    if clean.trim().is_empty() || is_heading {
                        env.block.clear();
                    }
                    env.lines.push(None);
                    LineOutput::Empty
                }
                Ok(Stmt::Expr(expr)) => {
                    let is_agg = matches!(
                        &expr.kind,
                        ExprKind::Ident(n)
                            if matches!(eval::special(n), Some(Special::Sum | Special::Avg))
                    );
                    match eval::eval(expr, &env) {
                        Ok(val) => {
                            env.lines.push(Some(val.clone()));
                            env.prev = Some(val.clone());
                            if !is_agg {
                                env.block.push(val.clone());
                            }
                            LineOutput::Value(val)
                        }
                        Err(err) => {
                            env.lines.push(None);
                            LineOutput::Error(err)
                        }
                    }
                }
                Ok(Stmt::Assign { name, name_span, value }) => {
                    if eval::constant(name).is_some() || eval::special(name).is_some() {
                        env.lines.push(None);
                        LineOutput::Error(CalcError::new(
                            ErrorKind::BadAssignment(name.to_string()),
                            name_span.clone(),
                        ))
                    } else {
                        match eval::eval(value, &env) {
                            Ok(val) => {
                                env.vars.insert(name.clone(), val.clone());
                                defined_vars.insert(name.clone());
                                env.lines.push(Some(val.clone()));
                                env.prev = Some(val.clone());
                                env.block.push(val.clone());
                                assigned_var = Some(name.clone());
                                LineOutput::Value(val)
                            }
                            Err(err) => {
                                env.lines.push(None);
                                LineOutput::Error(err)
                            }
                        }
                    }
                }
            };

            new_lines.push(LineEntry {
                source: clean.to_owned(),
                byte_start: offset,
                raw_len,
                parsed,
                highlights,
                output,
                assigned_var,
                is_heading,
            });
            offset += raw_len + 1;
        }

        self.raw_text.clear();
        self.raw_text.push_str(text);
        self.lines = new_lines;
        self.env = env;
        self.version = self.version.wrapping_add(1).max(1);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn results(doc: &str) -> Vec<String> {
        let nb = Notebook::from_text(doc);
        nb.lines()
            .iter()
            .map(|l| match &l.output {
                LineOutput::Empty => String::new(),
                LineOutput::Value(v) => v.plain(6),
                LineOutput::Error(e) => format!("ERR:{:?}", e.kind),
            })
            .collect()
    }

    #[test]
    fn reactive_variables_and_line_refs() {
        let doc = "\
# Расчёт поездки
dist = 420 km
speed = 105 km/h
time = dist / speed in h
#4 in min
ans + 15 min";
        assert_eq!(
            results(doc),
            vec!["", "420 km", "105 km/h", "4 h", "240 min", "255 min"]
        );
    }

    #[test]
    fn block_sum_and_avg() {
        let doc = "\
10
20
30
sum
avg

100
200
сумма";
        assert_eq!(
            results(doc),
            vec!["10", "20", "30", "60", "20", "", "100", "200", "300"]
        );
    }

    #[test]
    fn forbidden_constant_assignment() {
        let nb = Notebook::from_text("pi = 3\n2 * pi");
        assert!(matches!(
            nb.lines()[0].output,
            LineOutput::Error(CalcError { kind: ErrorKind::BadAssignment(_), .. })
        ));
        assert!(nb.lines()[1].output.value().is_some());
    }

    #[test]
    fn dirty_sync_skips_unchanged_text() {
        let mut nb = Notebook::from_text("1 + 2\n3 * 4");
        let v1 = nb.version();
        assert!(!nb.sync("1 + 2\n3 * 4"));
        assert_eq!(nb.version(), v1);
        assert!(nb.sync("1 + 2\n3 * 5"));
        assert_eq!(nb.version(), v1 + 1);
        assert_eq!(nb.lines()[1].output.value().map(|v| v.plain(4)).as_deref(), Some("15"));
    }

    #[test]
    fn line_byte_offsets_handle_utf8_and_crlf() {
        let doc = "а = 5\r\nа * 2";
        let nb = Notebook::from_text(doc);
        assert_eq!(nb.lines().len(), 2);
        assert_eq!(nb.lines()[0].byte_start, 0);
        assert_eq!(nb.lines()[1].byte_start, "а = 5\r\n".len());
        assert_eq!(nb.lines()[1].output.value().map(|v| v.plain(4)).as_deref(), Some("10"));
    }

    #[test]
    fn filled_notebook_many_lines_refs_utf8_and_errors() {
        let mut lines = vec![
            "# Physics & Unit Conversions".to_owned(),
            "dist = 420 km".to_owned(),
            "speed = 120 km/h".to_owned(),
            "speed in m/s".to_owned(),
            "time = dist / speed in min".to_owned(),
            String::new(),
            "# Точная арифметика и кириллица".to_owned(),
            "цена = 2490".to_owned(),
            "цена - 15%".to_owned(),
            "20% от цена".to_owned(),
            "r = 5 см".to_owned(),
            "area = pi * r² в см²".to_owned(),
            "sin(30°) + cos(60°)".to_owned(),
            String::new(),
            "120".to_owned(),
            "340".to_owned(),
            "540".to_owned(),
            "sum".to_owned(),
            "avg".to_owned(),
            "#18 + #19".to_owned(),
            "#999 + 1".to_owned(),
            "sin(".to_owned(),
            "1 / 0".to_owned(),
            "22 °C in °F".to_owned(),
            "1/3 K in °C".to_owned(),
            "(0xFF << 4) | 0x0A in hex".to_owned(),
            "1 GiB in MB".to_owned(),
            String::new(),
        ];
        for i in 1..=50 {
            lines.push(format!("v{i} = ({i} * 1.5 + 1/3) * 2"));
        }
        lines.push("sum".to_owned());
        lines.push("avg".to_owned());
        lines.push(format!("#{} + #{}", lines.len() - 1, lines.len()));

        let doc = lines.join("\n");
        let mut nb = Notebook::from_text(&doc);
        assert_eq!(nb.lines().len(), lines.len());
        assert!(nb.lines()[12].output.value().is_some());
        assert!(nb.lines()[19].output.value().is_some());
        assert!(nb.lines()[20].output.error().is_some());
        assert!(nb.lines()[21].output.error().is_some());
        assert!(nb.lines()[22].output.error().is_some());
        assert!(nb.lines()[23].output.value().is_some());
        assert!(nb.lines().last().and_then(|l| l.output.value()).is_some());

        let crlf_doc = lines.join("\r\n");
        assert!(nb.sync(&crlf_doc));
        assert_eq!(nb.lines().len(), lines.len());
    }
}
