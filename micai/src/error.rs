use std::cell::RefCell;
use std::fmt;

/// Each file merged via import gets this much added to its line numbers, so an error
/// shows which file and line it is in (previously everything appeared as lines of the main file).
pub const FILE_LINES: usize = 1_000_000;

thread_local! {
    static WARNINGS: RefCell<Vec<SiskinError>> = RefCell::new(Vec::new());
    static FILES: RefCell<Vec<(String, String)>> = RefCell::new(vec![(String::new(), String::new())]);
}

/// Something worth reporting that does not stop execution (code starts with W). Once per location.
pub fn push_warning(w: SiskinError) {
    WARNINGS.with(|ws| {
        let mut ws = ws.borrow_mut();
        if !ws.iter().any(|x| x.code == w.code && x.line == w.line && x.col == w.col) {
            ws.push(w);
        }
    })
}

pub fn take_warnings() -> Vec<SiskinError> {
    WARNINGS.with(|ws| std::mem::take(&mut *ws.borrow_mut()))
}

/// Register another file and return the starting line number to use when reading it.
pub fn register_file(path: &str, src: &str) -> usize {
    FILES.with(|f| {
        let mut f = f.borrow_mut();
        if let Some(i) = f.iter().position(|(p, s)| p == path && s == src) {
            return i * FILE_LINES;
        }
        f.push((path.to_string(), src.to_string()));
        (f.len() - 1) * FILE_LINES
    })
}

/// Take the whole list of registered files (to hand to a new worker thread).
pub fn files_snapshot() -> Vec<(String, String)> {
    FILES.with(|f| f.borrow().clone())
}

/// Install a file list taken from another thread into this thread.
pub fn files_restore(v: Vec<(String, String)>) {
    FILES.with(|f| *f.borrow_mut() = v);
}

/// Map a merged line number back to (file name, file source, line within that file). None for the main file.
pub fn locate(line: usize) -> Option<(String, String, usize)> {
    if line < FILE_LINES {
        return None;
    }
    FILES.with(|f| {
        f.borrow()
            .get(line / FILE_LINES)
            .map(|(p, s)| (p.clone(), s.clone(), line % FILE_LINES))
    })
}

/// Debugger's `b util.skn:5`: (line offset, path) of the imported file whose name ends like this.
pub fn find_file(name: &str) -> Option<(usize, String)> {
    FILES.with(|f| {
        f.borrow().iter().enumerate().skip(1).find_map(|(i, (p, _))| {
            let hit = p == name || p.ends_with(&format!("/{}", name)) || p.ends_with(&format!("/{}.skn", name)) || p == &format!("{}.skn", name);
            if hit && !p.starts_with("<std.") {
                Some((i * FILE_LINES, p.clone()))
            } else {
                None
            }
        })
    })
}

/// Siskin compiler/runtime diagnostic.
///
/// Following design doc §9.2 "machine-readable diagnostics", every error carries
/// a stable code (E0001 etc.), a location, a human-readable message, and,
/// when possible, an applicable fix.
#[derive(Debug, Clone)]
pub struct SiskinError {
    pub code: &'static str,
    pub msg: String,
    pub line: usize,
    pub col: usize,
    pub fix: Option<String>,
}

impl SiskinError {
    pub fn new(code: &'static str, msg: impl Into<String>, line: usize, col: usize) -> Self {
        // Names inside a module (`a·greet`) are shown to people as `a.greet`.
        SiskinError { code, msg: crate::ns::shown(&msg.into()), line, col, fix: None }
    }

    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(crate::ns::shown(&fix.into()));
        self
    }

    /// For errors whose location is only known as column 1 (type names, patterns, match, etc.) pick a better column on that line:
    /// where the name quoted in backticks in the message appears on that line, else the first character after indentation.
    fn best_col(&self, line_src: &str) -> usize {
        if self.col > 1 {
            return self.col;
        }
        let chars: Vec<char> = line_src.chars().collect();
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        for (k, part) in self.msg.split('`').enumerate() {
            if k % 2 == 0 || part.is_empty() || part.contains(' ') {
                continue;
            }
            let pc: Vec<char> = part.chars().collect();
            let mut i = 0;
            while i + pc.len() <= chars.len() {
                if chars[i..i + pc.len()] == pc[..]
                    && (i == 0 || !is_word(chars[i - 1]) || !is_word(pc[0]))
                    && (i + pc.len() == chars.len() || !is_word(chars[i + pc.len()]) || !is_word(pc[pc.len() - 1]))
                {
                    return i + 1;
                }
                i += 1;
            }
        }
        chars.iter().take_while(|c| c.is_whitespace()).count() + 1
    }

    /// For `--json`: a copy with the column fixed via `best_col` if the error is in the main file.
    pub fn with_best_col(&self, src: &str) -> SiskinError {
        let mut e = self.clone();
        if self.line < FILE_LINES {
            if let Some(l) = src.lines().nth(self.line.saturating_sub(1)) {
                e.col = self.best_col(l);
            }
        }
        e
    }

    /// Human-readable format.
    pub fn render(&self, file: &str, src: &str) -> String {
        // Errors inside another file merged via import are shown with that file's name and line.
        if let Some((f, text, line)) = locate(self.line) {
            let mut e = self.clone();
            e.line = line;
            return e.render(&f, &text);
        }
        if let Some(line_src) = src.lines().nth(self.line.saturating_sub(1)) {
            let c = self.best_col(line_src);
            if c != self.col {
                let mut e = self.clone();
                e.col = c;
                return e.render(file, src);
            }
        }
        let mut s = String::new();
        let kind = match (self.code.starts_with('W'), crate::lang::ko()) {
            (true, true) => "경고",
            (true, false) => "warning",
            (false, true) => "오류",
            (false, false) => "error",
        };
        s.push_str(&format!("{}[{}]: {}\n", kind, self.code, self.msg));
        s.push_str(&format!("  --> {}:{}:{}\n", file, self.line, self.col));
        if let Some(line_src) = src.lines().nth(self.line.saturating_sub(1)) {
            let gutter = format!("{}", self.line);
            let pad = " ".repeat(gutter.len());
            s.push_str(&format!("{} |\n", pad));
            s.push_str(&format!("{} | {}\n", gutter, line_src));
            s.push_str(&format!("{} | {}^\n", pad, " ".repeat(self.col.saturating_sub(1))));
        }
        if let Some(fix) = &self.fix {
            s.push_str(&format!("  {}: {}\n", tr!("도움말", "help"), fix));
        }
        s
    }

    /// Machine-readable format (`siskin check --json`).
    pub fn to_json(&self, file: &str) -> String {
        if let Some((f, _, line)) = locate(self.line) {
            let mut e = self.clone();
            e.line = line;
            return e.to_json(&f);
        }
        let esc = |t: &str| t.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
        let fix = match &self.fix {
            Some(f) => format!("\"{}\"", esc(f)),
            None => "null".to_string(),
        };
        let severity = if self.code.starts_with('W') { "warning" } else { "error" };
        format!(
            "{{\"code\":\"{}\",\"severity\":\"{}\",\"file\":\"{}\",\"line\":{},\"col\":{},\"message\":\"{}\",\"fix\":{}}}",
            self.code, severity, esc(file), self.line, self.col, esc(&self.msg), fix
        )
    }
}

impl fmt::Display for SiskinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {} ({}:{})", self.code, self.msg, self.line, self.col)
    }
}
