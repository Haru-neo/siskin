use crate::error::SiskinError;
use std::fmt;

/// An f-string piece: either literal text or the source of an expression inside braces.
#[derive(Debug, Clone, PartialEq)]
pub enum FPart {
    Lit(String),
    /// Expression source and format spec (the `.2f` in `{x:.2f}`). Empty string if there is no spec.
    Expr(String, String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Float(f64),
    Str(String),
    FStr(Vec<FPart>),
    Ident(String),
    Kw(String),

    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Dot,
    Arrow,
    Question,
    Bang,

    Newline,
    Indent,
    Dedent,
    Eof,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Tok::Int(n) => return write!(f, "{}", tr!(format!("정수 {}", n), format!("integer {}", n))),
            Tok::Float(n) => return write!(f, "{}", tr!(format!("실수 {}", n), format!("float {}", n))),
            Tok::Str(_) => tr!("문자열", "string"),
            Tok::FStr(_) => tr!("f-문자열", "f-string"),
            Tok::Ident(n) => return write!(f, "{}", tr!(format!("이름 `{}`", n), format!("name `{}`", n))),
            Tok::Kw(k) => return write!(f, "{}", tr!(format!("키워드 `{}`", k), format!("keyword `{}`", k))),
            Tok::Plus => "`+`",
            Tok::Minus => "`-`",
            Tok::Star => "`*`",
            Tok::Slash => "`/`",
            Tok::Percent => "`%`",
            Tok::Assign => "`=`",
            Tok::EqEq => "`==`",
            Tok::Ne => "`!=`",
            Tok::Lt => "`<`",
            Tok::Le => "`<=`",
            Tok::Gt => "`>`",
            Tok::Ge => "`>=`",
            Tok::PlusEq => "`+=`",
            Tok::MinusEq => "`-=`",
            Tok::StarEq => "`*=`",
            Tok::SlashEq => "`/=`",
            Tok::PercentEq => "`%=`",
            Tok::LParen => "`(`",
            Tok::RParen => "`)`",
            Tok::LBracket => "`[`",
            Tok::RBracket => "`]`",
            Tok::LBrace => "`{`",
            Tok::RBrace => "`}`",
            Tok::Comma => "`,`",
            Tok::Colon => "`:`",
            Tok::Dot => "`.`",
            Tok::Arrow => "`->`",
            Tok::Question => "`?`",
            Tok::Bang => "`!`",
            Tok::Newline => tr!("줄바꿈", "newline"),
            Tok::Indent => tr!("들여쓰기", "indent"),
            Tok::Dedent => tr!("내어쓰기", "dedent"),
            Tok::Eof => tr!("파일 끝", "end of file"),
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
    pub col: usize,
}

/// Design doc §4.7. `requires`/`ensures`/`self`/`arena` etc. are
/// contextual keywords, so they are not in this list. The shorter the list,
/// the less there is for people to memorize and for AI to confuse.
pub const KEYWORDS: &[&str] = &[
    "fn", "let", "var", "if", "elif", "else", "for", "while", "in", "break", "continue", "return",
    "struct", "enum", "interface", "match", "case", "import", "from", "as", "pub", "try", "catch",
    "none", "true", "false", "and", "or", "not", "owned", "inout", "unsafe", "with", "comptime",
    "extern",
];

pub fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

const INDENT_WIDTH: usize = 4;

struct Lexer {
    src: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    indents: Vec<usize>,
    depth: usize,
    at_line_start: bool,
    out: Vec<Token>,
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, SiskinError> {
    tokenize_at(src, 0)
}

/// Counts line numbers from `base + 1` (for files merged in via import).
pub fn tokenize_at(src: &str, base: usize) -> Result<Vec<Token>, SiskinError> {
    let mut lx = Lexer {
        src: src.chars().collect(),
        pos: 0,
        line: base + 1,
        col: 1,
        indents: vec![0],
        depth: 0,
        at_line_start: true,
        out: Vec::new(),
    };
    lx.run()?;
    Ok(lx.out)
}

impl Lexer {
    fn eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn peek(&self) -> char {
        if self.eof() {
            '\0'
        } else {
            self.src[self.pos]
        }
    }

    fn peek_at(&self, n: usize) -> char {
        if self.pos + n >= self.src.len() {
            '\0'
        } else {
            self.src[self.pos + n]
        }
    }

    fn bump(&mut self) -> char {
        let c = self.peek();
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        c
    }

    fn emit(&mut self, tok: Tok, line: usize, col: usize) {
        self.out.push(Token { tok, line, col });
    }

    fn last_is_newline(&self) -> bool {
        matches!(self.out.last().map(|t| &t.tok), Some(Tok::Newline) | None)
    }

    fn err(&self, code: &'static str, msg: impl Into<String>) -> SiskinError {
        SiskinError::new(code, msg, self.line, self.col)
    }

    fn run(&mut self) -> Result<(), SiskinError> {
        loop {
            if self.at_line_start && self.depth == 0 {
                if !self.line_start()? {
                    break;
                }
                continue;
            }

            while self.peek() == ' ' || self.peek() == '\r' {
                self.bump();
            }

            if self.eof() {
                break;
            }

            let c = self.peek();

            if c == '#' {
                while !self.eof() && self.peek() != '\n' {
                    self.bump();
                }
                continue;
            }

            if c == '\n' {
                self.bump();
                if self.depth == 0 {
                    if !self.last_is_newline() {
                        let (l, cl) = (self.line - 1, self.col);
                        self.emit(Tok::Newline, l, cl);
                    }
                    self.at_line_start = true;
                }
                continue;
            }

            self.lex_token()?;
        }

        if !self.last_is_newline() {
            let (l, c) = (self.line, self.col);
            self.emit(Tok::Newline, l, c);
        }
        while self.indents.len() > 1 {
            self.indents.pop();
            let (l, c) = (self.line, self.col);
            self.emit(Tok::Dedent, l, c);
        }
        let (l, c) = (self.line, self.col);
        self.emit(Tok::Eof, l, c);
        Ok(())
    }

    /// Handles indentation at the start of a line.
    /// Returns false when the end of the file has been reached.
    fn line_start(&mut self) -> Result<bool, SiskinError> {
        self.at_line_start = false;
        let line = self.line;
        let mut n = 0usize;

        loop {
            match self.peek() {
                ' ' => {
                    n += 1;
                    self.bump();
                }
                '\t' => {
                    return Err(self
                        .err("E0001", tr!("탭 문자로는 들여쓸 수 없습니다", "tabs cannot be used for indentation"))
                        .with_fix(tr!("공백 4칸을 사용하세요. `siskin fmt`가 자동으로 고쳐줍니다", "use 4 spaces; `siskin fmt` fixes this automatically")));
                }
                _ => break,
            }
        }

        if self.eof() {
            return Ok(false);
        }

        // Blank lines and comment-only lines are excluded from indentation tracking.
        if self.peek() == '\n' || self.peek() == '\r' || self.peek() == '#' {
            while !self.eof() && self.peek() != '\n' {
                self.bump();
            }
            if self.eof() {
                return Ok(false);
            }
            self.bump();
            self.at_line_start = true;
            return Ok(true);
        }

        if n % INDENT_WIDTH != 0 {
            let want = (n / INDENT_WIDTH) * INDENT_WIDTH;
            return Err(SiskinError::new(
                "E0002",
                tr!(format!("들여쓰기가 공백 {}칸입니다. 4의 배수여야 합니다", n), format!("indentation is {} spaces; it must be a multiple of 4", n)),
                line,
                1,
            )
            .with_fix(tr!(format!("공백 {}칸 또는 {}칸으로 맞추세요", want, want + INDENT_WIDTH), format!("indent with {} or {} spaces", want, want + INDENT_WIDTH))));
        }

        let cur = *self.indents.last().unwrap();
        if n > cur {
            self.indents.push(n);
            self.emit(Tok::Indent, line, 1);
        } else if n < cur {
            while *self.indents.last().unwrap() > n {
                self.indents.pop();
                self.emit(Tok::Dedent, line, 1);
            }
            if *self.indents.last().unwrap() != n {
                return Err(SiskinError::new(
                    "E0003",
                    tr!(format!("들여쓰기 {}칸이 바깥쪽 어느 블록과도 맞지 않습니다", n), format!("indentation of {} spaces does not match any enclosing block", n)),
                    line,
                    1,
                )
                .with_fix(tr!(
                    format!("열려 있는 들여쓰기 단계는 {:?}입니다. 그중 하나로 맞추세요", self.indents),
                    format!("open indentation levels are {:?}; use one of them", self.indents)
                )));
            }
        }
        Ok(true)
    }

    fn lex_token(&mut self) -> Result<(), SiskinError> {
        let line = self.line;
        let col = self.col;
        let c = self.peek();

        if c.is_ascii_digit() {
            return self.lex_number(line, col);
        }

        // r"..." / r'...' — read verbatim. Handy for regular expressions.
        // Wrapping in single quotes lets you put double quotes (") inside as-is.
        if c == 'r' && (self.peek_at(1) == '"' || self.peek_at(1) == '\'') {
            let line = self.line;
            let col = self.col;
            self.bump();
            let quote = self.bump();
            let mut s = String::new();
            loop {
                if self.eof() || self.peek() == '\n' {
                    return Err(SiskinError::new("E0007", tr!("원시 문자열이 닫히지 않았습니다", "unterminated raw string"), line, col)
                        .with_fix(tr!("닫는 따옴표를 추가하세요", "add the closing quote")));
                }
                let ch = self.bump();
                if ch == quote {
                    break;
                }
                s.push(ch);
            }
            self.emit(Tok::Str(s), line, col);
            return Ok(());
        }

        if c == 'f' && (self.peek_at(1) == '"' || self.peek_at(1) == '\'') {
            self.bump();
            let quote = self.bump();
            let parts = self.lex_fstring(line, col, quote)?;
            self.emit(Tok::FStr(parts), line, col);
            return Ok(());
        }

        if c.is_alphabetic() || c == '_' {
            let mut s = String::new();
            while self.peek().is_alphanumeric() || self.peek() == '_' {
                s.push(self.bump());
            }
            if is_keyword(&s) {
                self.emit(Tok::Kw(s), line, col);
            } else {
                self.emit(Tok::Ident(s), line, col);
            }
            return Ok(());
        }

        if c == '"' || c == '\'' {
            let triple = self.peek_at(1) == c && self.peek_at(2) == c;
            let s = self.lex_string(triple, c)?;
            self.emit(Tok::Str(s), line, col);
            return Ok(());
        }

        // Operators and punctuation
        self.bump();
        let tok = match c {
            '+' => {
                if self.peek() == '=' {
                    self.bump();
                    Tok::PlusEq
                } else {
                    Tok::Plus
                }
            }
            '-' => {
                if self.peek() == '>' {
                    self.bump();
                    Tok::Arrow
                } else if self.peek() == '=' {
                    self.bump();
                    Tok::MinusEq
                } else {
                    Tok::Minus
                }
            }
            '*' => {
                if self.peek() == '=' {
                    self.bump();
                    Tok::StarEq
                } else {
                    Tok::Star
                }
            }
            '/' => {
                if self.peek() == '=' {
                    self.bump();
                    Tok::SlashEq
                } else {
                    Tok::Slash
                }
            }
            '%' => {
                if self.peek() == '=' {
                    self.bump();
                    Tok::PercentEq
                } else {
                    Tok::Percent
                }
            }
            '=' => {
                if self.peek() == '=' {
                    self.bump();
                    Tok::EqEq
                } else {
                    Tok::Assign
                }
            }
            '!' => {
                if self.peek() == '=' {
                    self.bump();
                    Tok::Ne
                } else {
                    Tok::Bang
                }
            }
            '<' => {
                if self.peek() == '=' {
                    self.bump();
                    Tok::Le
                } else {
                    Tok::Lt
                }
            }
            '>' => {
                if self.peek() == '=' {
                    self.bump();
                    Tok::Ge
                } else {
                    Tok::Gt
                }
            }
            '(' => {
                self.depth += 1;
                Tok::LParen
            }
            ')' => {
                self.depth = self.depth.saturating_sub(1);
                Tok::RParen
            }
            '[' => {
                self.depth += 1;
                Tok::LBracket
            }
            ']' => {
                self.depth = self.depth.saturating_sub(1);
                Tok::RBracket
            }
            '{' => {
                self.depth += 1;
                Tok::LBrace
            }
            '}' => {
                self.depth = self.depth.saturating_sub(1);
                Tok::RBrace
            }
            ',' => Tok::Comma,
            ':' => Tok::Colon,
            '.' => Tok::Dot,
            '?' => Tok::Question,
            _ => {
                return Err(SiskinError::new(
                    "E0004",
                    tr!(format!("알 수 없는 문자 `{}`", c), format!("unknown character `{}`", c)),
                    line,
                    col,
                ))
            }
        };
        self.emit(tok, line, col);
        Ok(())
    }

    fn lex_number(&mut self, line: usize, col: usize) -> Result<(), SiskinError> {
        let mut s = String::new();
        while self.peek().is_ascii_digit() || self.peek() == '_' {
            let c = self.bump();
            if c != '_' {
                s.push(c);
            }
        }
        let mut is_float = false;
        // The `1.0` in `p.1.0` is not a float but two tuple indices.
        let after_dot = matches!(self.out.last().map(|t| &t.tok), Some(Tok::Dot));
        if !after_dot && self.peek() == '.' && self.peek_at(1).is_ascii_digit() {
            is_float = true;
            s.push(self.bump());
            while self.peek().is_ascii_digit() || self.peek() == '_' {
                let c = self.bump();
                if c != '_' {
                    s.push(c);
                }
            }
        }
        if self.peek() == 'e' || self.peek() == 'E' {
            is_float = true;
            s.push(self.bump());
            if self.peek() == '+' || self.peek() == '-' {
                s.push(self.bump());
            }
            while self.peek().is_ascii_digit() {
                s.push(self.bump());
            }
        }
        if is_float {
            let v: f64 = s
                .parse()
                .map_err(|_| SiskinError::new("E0005", tr!(format!("실수 `{}`를 읽을 수 없습니다", s), format!("cannot parse float `{}`", s)), line, col))?;
            self.emit(Tok::Float(v), line, col);
        } else {
            let v: i64 = s.parse().map_err(|_| {
                SiskinError::new("E0006", tr!(format!("정수 `{}`가 Int 범위를 벗어납니다", s), format!("integer `{}` is out of range for Int", s)), line, col)
            })?;
            self.emit(Tok::Int(v), line, col);
        }
        Ok(())
    }

    fn lex_string(&mut self, triple: bool, quote: char) -> Result<String, SiskinError> {
        let line = self.line;
        let col = self.col;
        if triple {
            self.bump();
            self.bump();
            self.bump();
        } else {
            self.bump();
        }
        let mut s = String::new();
        loop {
            if self.eof() {
                return Err(SiskinError::new("E0007", tr!("문자열이 닫히지 않았습니다", "unterminated string"), line, col)
                    .with_fix(tr!("닫는 따옴표를 추가하세요", "add the closing quote")));
            }
            if triple {
                if self.peek() == quote && self.peek_at(1) == quote && self.peek_at(2) == quote {
                    self.bump();
                    self.bump();
                    self.bump();
                    break;
                }
            } else if self.peek() == quote {
                self.bump();
                break;
            } else if self.peek() == '\n' {
                return Err(SiskinError::new("E0007", tr!("문자열이 줄 끝에서 닫히지 않았습니다", "string is not closed before the end of the line"), line, col)
                    .with_fix(tr!("닫는 따옴표를 추가하거나 여러 줄 문자열 `\"\"\"`을 쓰세요", "add the closing quote, or use a multi-line string `\"\"\"`")));
            }
            let c = self.bump();
            if c == '\\' {
                let eline = self.line;
                let ecol = self.col;
                let e = self.bump();
                s.push(match e {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    '0' => '\0',
                    '\\' => '\\',
                    '"' => '"',
                    '\'' => '\'',
                    '{' => '{',
                    '}' => '}',
                    other => {
                        return Err(SiskinError::new(
                            "E0008",
                            tr!(format!("`\\{}` 는 모르는 표기입니다", other), format!("unknown escape `\\{}`", other)),
                            eline,
                            ecol.saturating_sub(1),
                        )
                        .with_fix(tr!(
                            format!("역슬래시를 그대로 쓰려면 `\\\\{}` 또는 원시 문자열 `r\"...\"` 을 쓰세요", other),
                            format!("for a literal backslash write `\\\\{}`, or use a raw string `r\"...\"`", other)
                        )))
                    }
                });
            } else {
                s.push(c);
            }
        }
        Ok(s)
    }

    /// Reads an f-string body and splits it into literal/expression pieces.
    /// The opening `f"` has already been consumed on entry.
    fn lex_fstring(&mut self, line: usize, col: usize, quote: char) -> Result<Vec<FPart>, SiskinError> {
        let mut parts = Vec::new();
        let mut lit = String::new();
        loop {
            if self.eof() || self.peek() == '\n' {
                return Err(SiskinError::new("E0007", tr!("f-문자열이 닫히지 않았습니다", "unterminated f-string"), line, col)
                    .with_fix(tr!("닫는 따옴표를 추가하세요", "add the closing quote")));
            }
            let c = self.peek();
            if c == quote {
                self.bump();
                break;
            }
            if c == '\\' {
                // Accepts the same escapes as regular strings. (Previously `\r` just became `r`.)
                let eline = self.line;
                let ecol = self.col;
                self.bump();
                let e = self.bump();
                lit.push(match e {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    '0' => '\0',
                    '\\' => '\\',
                    '"' => '"',
                    '\'' => '\'',
                    '{' => '{',
                    '}' => '}',
                    other => {
                        return Err(SiskinError::new(
                            "E0008",
                            tr!(format!("`\\{}` 는 모르는 표기입니다", other), format!("unknown escape `\\{}`", other)),
                            eline,
                            ecol,
                        )
                        .with_fix(tr!(format!("역슬래시를 그대로 쓰려면 `\\\\{}` 로 쓰세요", other), format!("for a literal backslash write `\\\\{}`", other))))
                    }
                });
                continue;
            }
            if c == '{' {
                if self.peek_at(1) == '{' {
                    self.bump();
                    self.bump();
                    lit.push('{');
                    continue;
                }
                self.bump();
                if !lit.is_empty() {
                    parts.push(FPart::Lit(std::mem::take(&mut lit)));
                }
                let mut expr = String::new();
                let mut spec = String::new();
                let mut depth = 0i32; // () [] {} nesting
                let mut in_spec = false;
                let mut in_str: Option<char> = None;
                loop {
                    if self.eof() || self.peek() == '\n' {
                        return Err(SiskinError::new(
                            "E0008",
                            tr!("f-문자열 안의 `{`가 닫히지 않았습니다", "unclosed `{` in f-string"),
                            line,
                            col,
                        )
                        .with_fix(tr!("`}`를 추가하세요", "add `}`")));
                    }
                    let ec = self.peek();
                    // String literals inside the expression are skipped whole.
                    if let Some(q) = in_str {
                        if ec == q {
                            in_str = None;
                        }
                        let ch = self.bump();
                        if in_spec { spec.push(ch) } else { expr.push(ch) }
                        continue;
                    }
                    match ec {
                        '"' | '\'' => in_str = Some(ec),
                        '(' | '[' | '{' => depth += 1,
                        ')' | ']' => depth -= 1,
                        '}' => {
                            if depth == 0 {
                                self.bump();
                                break;
                            }
                            depth -= 1;
                        }
                        ':' if depth == 0 && !in_spec => {
                            // Everything after a top-level `:` is the format spec.
                            in_spec = true;
                            self.bump();
                            continue;
                        }
                        _ => {}
                    }
                    let ch = self.bump();
                    if in_spec { spec.push(ch) } else { expr.push(ch) }
                }
                parts.push(FPart::Expr(expr, spec));
                continue;
            }
            if c == '}' && self.peek_at(1) == '}' {
                self.bump();
                self.bump();
                lit.push('}');
                continue;
            }
            lit.push(self.bump());
        }
        if !lit.is_empty() {
            parts.push(FPart::Lit(lit));
        }
        Ok(parts)
    }
}
