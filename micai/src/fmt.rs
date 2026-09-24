//! `siskin fmt` — 코드 모양을 한 가지로 맞춥니다.
//!
//! 하는 일: 들여쓰기를 공백 4칸 단위로(탭도 고칩니다), 연산자·쉼표·콜론 둘레의
//! 띄어쓰기, 줄 끝 공백 지우기, 빈 줄은 최대 2줄, 파일 끝 줄바꿈 하나.
//! 하지 않는 일: 줄을 나누거나 합치기, 문자열·주석 내용 바꾸기, 이름 바꾸기.
//!
//! 안전장치: 정리한 결과를 다시 토큰으로 읽어 원래와 한 토큰도 다르지 않은지
//! 확인합니다(`check_same`). 다르면 쓰지 않습니다. 모양만 바뀌고 뜻은 그대로입니다.

use crate::lexer::{is_keyword, tokenize, Tok};

#[derive(Debug, Clone, PartialEq)]
enum T {
    Word(String),
    Num(String),
    Str(String),
    Op(String),
    Open(char),
    Close(char),
    Comma,
    Colon,
    Dot,
    /// 괄호 안에서 줄이 바뀐 자리. 다음 줄의 원래 들여쓰기 폭을 기억합니다.
    Nl(usize),
    /// 줄 끝 주석. (원래 열 위치, 주석 글)
    Comment(usize, String),
}

enum Line {
    Blank,
    /// 주석만 있는 줄: (원래 들여쓰기 폭, 주석 글)
    Comment(usize, String),
    /// 코드 줄: (원래 들여쓰기 폭, 토큰들)
    Code(usize, Vec<T>),
}

struct Scan {
    s: Vec<char>,
    i: usize,
}

impl Scan {
    fn peek(&self, n: usize) -> char {
        *self.s.get(self.i + n).unwrap_or(&'\0')
    }
    fn eof(&self) -> bool {
        self.i >= self.s.len()
    }

    /// 줄 머리의 공백 폭. 탭은 4칸으로 셉니다.
    fn indent(&mut self) -> usize {
        let mut w = 0;
        loop {
            match self.peek(0) {
                ' ' => w += 1,
                '\t' => w += 4,
                _ => return w,
            }
            self.i += 1;
        }
    }

    fn rest_of_line(&mut self) -> String {
        let mut out = String::new();
        while !self.eof() && self.peek(0) != '\n' {
            out.push(self.peek(0));
            self.i += 1;
        }
        out.trim_end().to_string()
    }

    /// 문자열 하나를 원래 글자 그대로 읽습니다. 접두사(r, f)는 이미 넣었습니다.
    fn string(&mut self, out: &mut String, raw: bool) {
        let q = self.peek(0);
        let triple = self.peek(1) == q && self.peek(2) == q;
        let n = if triple { 3 } else { 1 };
        for _ in 0..n {
            out.push(q);
            self.i += 1;
        }
        while !self.eof() {
            let c = self.peek(0);
            if !triple && c == '\n' {
                return; // 닫히지 않은 문자열: 컴파일러가 알려 줄 일이라 그대로 둡니다.
            }
            if c == '\\' && !raw {
                out.push(c);
                self.i += 1;
                if !self.eof() {
                    out.push(self.peek(0));
                    self.i += 1;
                }
                continue;
            }
            if c == q && (!triple || (self.peek(1) == q && self.peek(2) == q)) {
                for _ in 0..n {
                    out.push(q);
                    self.i += 1;
                }
                return;
            }
            out.push(c);
            self.i += 1;
        }
    }

    /// f-문자열. `{...}` 안에는 같은 따옴표의 문자열이 또 들어갈 수 있습니다.
    fn fstring(&mut self, out: &mut String) {
        let q = self.peek(0);
        out.push(q);
        self.i += 1;
        while !self.eof() {
            let c = self.peek(0);
            if c == '\n' {
                return;
            }
            if c == '\\' {
                out.push(c);
                self.i += 1;
                if !self.eof() {
                    out.push(self.peek(0));
                    self.i += 1;
                }
                continue;
            }
            if c == q {
                out.push(c);
                self.i += 1;
                return;
            }
            if c == '{' && self.peek(1) == '{' {
                out.push_str("{{");
                self.i += 2;
                continue;
            }
            if c == '{' {
                // 식 부분: 괄호 깊이와 안쪽 문자열을 따라가며 닫는 `}` 까지.
                let mut depth = 0i32;
                out.push(c);
                self.i += 1;
                while !self.eof() {
                    let d = self.peek(0);
                    if d == '\n' {
                        return;
                    }
                    if d == '"' || d == '\'' {
                        let mut inner = String::new();
                        self.string(&mut inner, false);
                        out.push_str(&inner);
                        continue;
                    }
                    out.push(d);
                    self.i += 1;
                    match d {
                        '(' | '[' | '{' => depth += 1,
                        ')' | ']' => depth -= 1,
                        '}' => {
                            if depth == 0 {
                                break;
                            }
                            depth -= 1;
                        }
                        _ => {}
                    }
                }
                continue;
            }
            out.push(c);
            self.i += 1;
        }
    }

    /// 코드 한 줄(괄호가 열려 있으면 여러 줄)을 토큰으로 읽습니다.
    fn code_line(&mut self, line_start: usize) -> Vec<T> {
        let mut toks = Vec::new();
        let mut depth = 0i32;
        let mut line_start = line_start;
        loop {
            // 띄어쓰기는 버립니다.
            while matches!(self.peek(0), ' ' | '\t' | '\r') {
                self.i += 1;
            }
            if self.eof() {
                return toks;
            }
            let c = self.peek(0);
            if c == '\n' {
                self.i += 1;
                if depth > 0 {
                    line_start = self.i;
                    // 괄호 안의 빈 줄과 주석 줄은 건너뛰지 않고 그대로 둡니다.
                    let w = self.indent();
                    toks.push(T::Nl(w));
                    continue;
                }
                return toks;
            }
            if c == '#' {
                let at = self.col_of(line_start);
                let text = self.rest_of_line();
                toks.push(T::Comment(at, text));
                continue;
            }
            if (c == 'r' || c == 'f') && (self.peek(1) == '"' || self.peek(1) == '\'') {
                let mut s = String::new();
                s.push(c);
                self.i += 1;
                if c == 'f' {
                    self.fstring(&mut s);
                } else {
                    self.string(&mut s, true);
                }
                toks.push(T::Str(s));
                continue;
            }
            if c == '"' || c == '\'' {
                let mut s = String::new();
                self.string(&mut s, false);
                toks.push(T::Str(s));
                continue;
            }
            if c.is_ascii_digit() {
                let mut s = String::new();
                while self.peek(0).is_alphanumeric() || self.peek(0) == '_' || (self.peek(0) == '.' && self.peek(1).is_ascii_digit()) {
                    s.push(self.peek(0));
                    self.i += 1;
                }
                toks.push(T::Num(s));
                continue;
            }
            if c.is_alphabetic() || c == '_' {
                let mut s = String::new();
                while self.peek(0).is_alphanumeric() || self.peek(0) == '_' {
                    s.push(self.peek(0));
                    self.i += 1;
                }
                toks.push(T::Word(s));
                continue;
            }
            self.i += 1;
            let two: String = [c, self.peek(0)].iter().collect();
            match two.as_str() {
                "==" | "!=" | "<=" | ">=" | "+=" | "-=" | "*=" | "/=" | "%=" | "->" => {
                    self.i += 1;
                    toks.push(T::Op(two));
                    continue;
                }
                _ => {}
            }
            toks.push(match c {
                '(' | '[' | '{' => {
                    depth += 1;
                    T::Open(c)
                }
                ')' | ']' | '}' => {
                    depth -= 1;
                    T::Close(c)
                }
                ',' => T::Comma,
                ':' => T::Colon,
                '.' => T::Dot,
                other => T::Op(other.to_string()),
            });
        }
    }

    fn col_of(&self, line_start: usize) -> usize {
        self.i.saturating_sub(line_start)
    }
}

fn split_lines(src: &str) -> Vec<Line> {
    let src = src.replace("\r\n", "\n");
    let mut sc = Scan { s: src.chars().collect(), i: 0 };
    let mut out = Vec::new();
    while !sc.eof() {
        let ls = sc.i;
        let w = sc.indent();
        match sc.peek(0) {
            '\n' | '\r' => {
                sc.rest_of_line();
                sc.i += 1;
                out.push(Line::Blank);
            }
            '\0' if sc.eof() => {
                if w > 0 {
                    out.push(Line::Blank);
                }
            }
            '#' => {
                let t = sc.rest_of_line();
                sc.i += 1;
                out.push(Line::Comment(w, t));
            }
            _ => {
                let toks = sc.code_line(ls);
                out.push(Line::Code(w, toks));
            }
        }
    }
    out
}

fn is_kw_word(t: &T) -> bool {
    match t {
        T::Word(w) => is_keyword(w) && !matches!(w.as_str(), "true" | "false" | "none"),
        _ => false,
    }
}

/// 이 연산자가 앞에 붙는 것(`-x`, `!Int`, `?Str`, `*p`)인가.
fn is_prefix(op: &str, prev: Option<&T>, prev_prefix: bool) -> bool {
    match op {
        "!" | "?" => true,
        "-" | "*" => match prev {
            None => true,
            Some(T::Open(_)) | Some(T::Comma) | Some(T::Colon) | Some(T::Nl(_)) => true,
            Some(T::Op(_)) => true,
            Some(p) if is_kw_word(p) => true,
            _ => {
                let _ = prev_prefix;
                false
            }
        },
        _ => false,
    }
}

fn render_toks(toks: &[T], level: usize, orig_indent: usize) -> String {
    let pad = level * 4;
    let mut out = " ".repeat(pad);
    let mut line_begin = 0usize; // out 안에서 지금 줄이 시작한 곳
    let mut prev: Option<&T> = None;
    let mut prev_prefix = false;
    let mut depth = 0i32;
    for (idx, t) in toks.iter().enumerate() {
        match t {
            T::Open(_) => depth += 1,
            T::Close(_) => depth -= 1,
            _ => {}
        }
        match t {
            T::Nl(_) => {
                // 괄호 안 줄바꿈: 열린 괄호 하나마다 4칸 더 들여씁니다.
                // 닫는 괄호로 시작하는 줄은 한 단계 덜 들여씁니다.
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push('\n');
                line_begin = out.len();
                let mut d = depth;
                if let Some(T::Close(_)) = toks.get(idx + 1) {
                    d -= 1;
                }
                out.push_str(&" ".repeat(pad + 4 * d.max(0) as usize));
                prev = Some(t);
                prev_prefix = false;
                continue;
            }
            T::Comment(at, text) => {
                let code_len = out[line_begin..].chars().count();
                if code_len == 0 || out[line_begin..].trim().is_empty() {
                    out.push_str(text);
                } else {
                    // 원래 주석이 줄 맞춰 있었으면 그 열을 지킵니다.
                    let want = (*at + pad).saturating_sub(orig_indent);
                    let gap = if want >= code_len + 2 { want - code_len } else { 2 };
                    out.push_str(&" ".repeat(gap));
                    out.push_str(text);
                }
                prev = Some(t);
                continue;
            }
            _ => {}
        }
        let cur_prefix = matches!(t, T::Op(o) if is_prefix(o, prev, prev_prefix));
        let space = match (prev, t) {
            (None, _) | (Some(T::Nl(_)), _) => false,
            (Some(T::Comment(..)), _) => false,
            (_, T::Close(_)) | (_, T::Comma) | (_, T::Colon) | (_, T::Dot) => false,
            (Some(T::Dot), _) => false,
            (Some(T::Open(_)), _) => false,
            (Some(T::Comma), _) | (Some(T::Colon), _) => true,
            // `BankError!Int` — 오류 타입과 `!` 는 붙여 씁니다.
            (Some(p @ T::Word(_)), T::Op(o)) if o == "!" && !is_kw_word(p) => false,
            (Some(T::Op(_)), _) if prev_prefix => false,
            (Some(T::Op(_)), _) => true,
            (Some(p), T::Open(c)) if *c == '(' || *c == '[' => match p {
                T::Word(w) if w == "fn" => false,
                p if is_kw_word(p) => true,
                T::Word(_) | T::Close(_) | T::Str(_) => false,
                _ => true,
            },
            (Some(_), T::Op(_)) if cur_prefix => true,
            _ => true,
        };
        if space && !out.ends_with(' ') {
            out.push(' ');
        }
        match t {
            T::Word(s) | T::Num(s) | T::Str(s) | T::Op(s) => out.push_str(s),
            T::Open(c) | T::Close(c) => out.push(*c),
            T::Comma => out.push(','),
            T::Colon => out.push(':'),
            T::Dot => out.push('.'),
            T::Nl(_) | T::Comment(..) => {}
        }
        prev = Some(t);
        prev_prefix = cur_prefix;
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// 코드 모양을 정리한 새 글을 돌려줍니다.
pub fn format(src: &str) -> String {
    let lines = split_lines(src);
    let mut stack: Vec<usize> = vec![0];
    let mut out: Vec<String> = Vec::new();
    let mut blanks = 0usize;
    // 주석 줄은 다음 코드 줄을 봐야 들여쓰기를 정할 수 있어 잠깐 모아 둡니다.
    let mut pending: Vec<(usize, String, usize)> = Vec::new(); // (폭, 글, 앞의 빈 줄 수)

    let level_for = |stack: &mut Vec<usize>, w: usize| -> usize {
        if w > *stack.last().unwrap() {
            stack.push(w);
        } else {
            while stack.len() > 1 && *stack.last().unwrap() > w {
                stack.pop();
            }
            if *stack.last().unwrap() < w {
                stack.push(w);
            }
        }
        stack.len() - 1
    };

    let flush_blanks = |out: &mut Vec<String>, n: usize| {
        if !out.is_empty() {
            for _ in 0..n.min(2) {
                out.push(String::new());
            }
        }
    };

    for line in &lines {
        match line {
            Line::Blank => blanks += 1,
            Line::Comment(w, t) => {
                pending.push((*w, t.clone(), blanks));
                blanks = 0;
            }
            Line::Code(w, toks) => {
                // 이 줄의 단계를 먼저 정해 두고, 모아 둔 주석의 단계를 거기에 맞춥니다.
                let mut probe = stack.clone();
                let lv = level_for(&mut probe, *w);
                // 맨 바깥의 fn / struct / enum 앞에는 빈 줄을 하나 이상 둡니다.
                let is_def = lv == 0
                    && matches!(toks.first(), Some(T::Word(k)) if matches!(k.as_str(), "fn" | "struct" | "enum" | "interface" | "pub" | "extern"));
                let prev_is_def_head = matches!(out.last(), Some(l) if l.ends_with(':') && !l.starts_with(' '));
                let mut sep_needed = is_def && !out.is_empty() && !prev_is_def_head;
                if sep_needed {
                    if let Some(first) = pending.first_mut() {
                        if first.2 == 0 {
                            first.2 = 1;
                        }
                        sep_needed = false;
                    }
                }
                for (cw, ct, cb) in pending.drain(..) {
                    // 주석이 이 줄보다 깊게 적혀 있었으면 (앞 블록의 끝 주석) 그 깊이를 지킵니다.
                    let clv = if cw > *w {
                        let mut s2 = stack.clone();
                        level_for(&mut s2, cw).max(lv)
                    } else {
                        lv
                    };
                    flush_blanks(&mut out, cb);
                    out.push(format!("{}{}", " ".repeat(clv * 4), ct));
                }
                stack = probe;
                if sep_needed && blanks == 0 {
                    blanks = 1;
                }
                flush_blanks(&mut out, blanks);
                blanks = 0;
                out.push(render_toks(toks, lv, *w));
            }
        }
    }
    for (cw, ct, cb) in pending.drain(..) {
        let mut s2 = stack.clone();
        let clv = if cw == 0 { 0 } else { level_for(&mut s2, cw) };
        flush_blanks(&mut out, cb);
        out.push(format!("{}{}", " ".repeat(clv * 4), ct));
    }
    let mut s = out.join("\n");
    while s.ends_with('\n') {
        s.pop();
    }
    s.push('\n');
    if s.trim().is_empty() {
        return String::new();
    }
    s
}

/// 정리 전후의 토큰이 같은지(뜻이 그대로인지) 봅니다.
/// 원래 코드가 토큰으로 안 읽히면(탭 들여쓰기 등) 결과가 읽히기만 하면 됩니다.
pub fn check_same(before: &str, after: &str) -> Result<(), String> {
    let b = tokenize(before);
    let a = tokenize(after).map_err(|e| tr!(format!("정리한 결과를 읽을 수 없습니다 ({}): {}", e.code, e.msg), format!("cannot read the formatted result ({}): {}", e.code, e.msg)))?;
    if let Ok(b) = b {
        let bt: Vec<&Tok> = b.iter().map(|t| &t.tok).collect();
        let at: Vec<&Tok> = a.iter().map(|t| &t.tok).collect();
        if bt != at {
            let k = bt.iter().zip(at.iter()).position(|(x, y)| x != y).unwrap_or(bt.len().min(at.len()));
            let line = a.get(k).map(|t| t.line).unwrap_or(0);
            return Err(tr!(
                format!("정리하면 뜻이 바뀔 수 있어 멈췄습니다 ({}번째 줄 근처). siskin 의 버그이니 알려 주세요", line),
                format!("stopped because formatting could change the meaning (near line {}); this is a bug in siskin, please report it", line)
            ));
        }
    }
    Ok(())
}
