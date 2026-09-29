//! A tiny JSON reader/writer.
//!
//! Uses no external libraries. The C side follows the same rules, so
//! `siskin run` and `siskin build` give the same answer.

use std::cell::RefCell;
use std::rc::Rc;

pub type JRef = Rc<RefCell<JsonVal>>;

#[derive(Debug, Clone)]
pub enum JsonVal {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<JRef>),
    /// Preserves insertion order.
    Dict(Vec<(String, JRef)>),
}

pub fn wrap(v: JsonVal) -> JRef {
    Rc::new(RefCell::new(v))
}

/// A fully fresh copy (for passing between tasks).
pub fn detach(j: &JRef) -> JRef {
    let v = match &*j.borrow() {
        JsonVal::List(xs) => JsonVal::List(xs.iter().map(detach).collect()),
        JsonVal::Dict(ps) => JsonVal::Dict(ps.iter().map(|(k, v)| (k.clone(), detach(v))).collect()),
        other => other.clone(),
    };
    wrap(v)
}

pub fn kind(v: &JsonVal) -> &'static str {
    match v {
        JsonVal::Null => "null",
        JsonVal::Bool(_) => "bool",
        JsonVal::Int(_) => "int",
        JsonVal::Float(_) => "float",
        JsonVal::Str(_) => "str",
        JsonVal::List(_) => "list",
        JsonVal::Dict(_) => "dict",
    }
}

struct P {
    s: Vec<char>,
    i: usize,
}

pub fn parse(text: &str) -> Result<JRef, String> {
    let mut p = P { s: text.chars().collect(), i: 0 };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i < p.s.len() {
        return Err(tr!(format!("{}번째 글자 뒤에 남는 것이 있습니다", p.i), format!("unexpected trailing characters after position {}", p.i)));
    }
    Ok(v)
}

impl P {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], ' ' | '\t' | '\n' | '\r') {
            self.i += 1;
        }
    }

    fn at(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn lit(&mut self, word: &str) -> bool {
        let w: Vec<char> = word.chars().collect();
        if self.i + w.len() <= self.s.len() && self.s[self.i..self.i + w.len()] == w[..] {
            self.i += w.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<JRef, String> {
        self.ws();
        let c = match self.at() {
            Some(c) => c,
            None => return Err(tr!("값이 오기 전에 끝났습니다", "unexpected end of input").into()),
        };
        match c {
            '{' => self.object(),
            '[' => self.array(),
            '"' => {
                let s = self.string()?;
                Ok(wrap(JsonVal::Str(s)))
            }
            't' => {
                if self.lit("true") {
                    Ok(wrap(JsonVal::Bool(true)))
                } else {
                    Err(tr!(format!("{}번째: `true` 가 아닙니다", self.i), format!("at {}: expected `true`", self.i)))
                }
            }
            'f' => {
                if self.lit("false") {
                    Ok(wrap(JsonVal::Bool(false)))
                } else {
                    Err(tr!(format!("{}번째: `false` 가 아닙니다", self.i), format!("at {}: expected `false`", self.i)))
                }
            }
            'n' => {
                if self.lit("null") {
                    Ok(wrap(JsonVal::Null))
                } else {
                    Err(tr!(format!("{}번째: `null` 이 아닙니다", self.i), format!("at {}: expected `null`", self.i)))
                }
            }
            _ => self.number(),
        }
    }

    fn object(&mut self) -> Result<JRef, String> {
        self.i += 1; // {
        let mut out: Vec<(String, JRef)> = Vec::new();
        self.ws();
        if self.at() == Some('}') {
            self.i += 1;
            return Ok(wrap(JsonVal::Dict(out)));
        }
        loop {
            self.ws();
            if self.at() != Some('"') {
                return Err(tr!(format!("{}번째: 이름은 따옴표로 감싸야 합니다", self.i), format!("at {}: object keys must be quoted", self.i)));
            }
            let k = self.string()?;
            self.ws();
            if self.at() != Some(':') {
                return Err(tr!(format!("{}번째: `:` 가 필요합니다", self.i), format!("at {}: expected `:`", self.i)));
            }
            self.i += 1;
            let v = self.value()?;
            // If the same key appears again, the later one wins.
            match out.iter_mut().find(|(ek, _)| *ek == k) {
                Some(slot) => slot.1 = v,
                None => out.push((k, v)),
            }
            self.ws();
            match self.at() {
                Some(',') => {
                    self.i += 1;
                }
                Some('}') => {
                    self.i += 1;
                    break;
                }
                _ => return Err(tr!(format!("{}번째: `,` 나 `}}` 가 필요합니다", self.i), format!("at {}: expected `,` or `}}`", self.i))),
            }
        }
        Ok(wrap(JsonVal::Dict(out)))
    }

    fn array(&mut self) -> Result<JRef, String> {
        self.i += 1; // [
        let mut out: Vec<JRef> = Vec::new();
        self.ws();
        if self.at() == Some(']') {
            self.i += 1;
            return Ok(wrap(JsonVal::List(out)));
        }
        loop {
            let v = self.value()?;
            out.push(v);
            self.ws();
            match self.at() {
                Some(',') => {
                    self.i += 1;
                }
                Some(']') => {
                    self.i += 1;
                    break;
                }
                _ => return Err(tr!(format!("{}번째: `,` 나 `]` 가 필요합니다", self.i), format!("at {}: expected `,` or `]`", self.i))),
            }
        }
        Ok(wrap(JsonVal::List(out)))
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // "
        let mut out = String::new();
        loop {
            let c = match self.at() {
                Some(c) => c,
                None => return Err(tr!("문자열이 닫히지 않았습니다", "unterminated string").into()),
            };
            self.i += 1;
            if c == '"' {
                break;
            }
            if c != '\\' {
                out.push(c);
                continue;
            }
            let e = match self.at() {
                Some(e) => e,
                None => return Err(tr!("문자열이 닫히지 않았습니다", "unterminated string").into()),
            };
            self.i += 1;
            match e {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                '/' => out.push('/'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'u' => {
                    let mut v: u32 = 0;
                    for _ in 0..4 {
                        let h = match self.at() {
                            Some(h) => h,
                            None => return Err(tr!("`\\u` 뒤가 모자랍니다", "incomplete `\\u` escape").into()),
                        };
                        self.i += 1;
                        let d = h.to_digit(16).ok_or(tr!("`\\u` 뒤는 16진수 네 자리입니다", "`\\u` must be followed by four hex digits"))?;
                        v = v * 16 + d;
                    }
                    out.push(char::from_u32(v).unwrap_or('\u{FFFD}'));
                }
                other => return Err(tr!(format!("`\\{}` 는 JSON에 없는 표기입니다", other), format!("`\\{}` is not a valid JSON escape", other))),
            }
        }
        Ok(out)
    }

    fn number(&mut self) -> Result<JRef, String> {
        let start = self.i;
        if self.at() == Some('-') {
            self.i += 1;
        }
        let mut isfloat = false;
        while let Some(c) = self.at() {
            if c.is_ascii_digit() {
                self.i += 1;
            } else if c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-' {
                isfloat = isfloat || c == '.' || c == 'e' || c == 'E';
                self.i += 1;
            } else {
                break;
            }
        }
        let text: String = self.s[start..self.i].iter().collect();
        if text.is_empty() || text == "-" {
            return Err(tr!(format!("{}번째: 숫자가 아닙니다", start), format!("at {}: expected a number", start)));
        }
        if isfloat {
            text.parse::<f64>()
                .map(|f| wrap(JsonVal::Float(f)))
                .map_err(|_| tr!(format!("`{}` 는 숫자로 읽을 수 없습니다", text), format!("cannot parse `{}` as a number", text)))
        } else {
            match text.parse::<i64>() {
                Ok(n) => Ok(wrap(JsonVal::Int(n))),
                // Integers that are too large are read as floats.
                Err(_) => text
                    .parse::<f64>()
                    .map(|f| wrap(JsonVal::Float(f)))
                    .map_err(|_| tr!(format!("`{}` 는 숫자로 읽을 수 없습니다", text), format!("cannot parse `{}` as a number", text))),
            }
        }
    }
}

pub fn escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn write(v: &JRef, out: &mut String) {
    match &*v.borrow() {
        JsonVal::Null => out.push_str("null"),
        JsonVal::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JsonVal::Int(n) => out.push_str(&n.to_string()),
        JsonVal::Float(f) => out.push_str(&crate::value::float_repr(*f)),
        JsonVal::Str(s) => escape(s, out),
        JsonVal::List(items) => {
            out.push('[');
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(it, out);
            }
            out.push(']');
        }
        JsonVal::Dict(pairs) => {
            out.push('{');
            for (i, (k, val)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                escape(k, out);
                out.push(':');
                write(val, out);
            }
            out.push('}');
        }
    }
}

pub fn stringify(v: &JRef) -> String {
    let mut out = String::new();
    write(v, &mut out);
    out
}
