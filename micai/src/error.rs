use std::cell::RefCell;
use std::fmt;

/// import 로 합친 파일마다 줄 번호에 이만큼씩 더해 둡니다. 그래야 오류가 어느 파일의
/// 몇 번째 줄인지 알 수 있습니다 (예전에는 모두 main 파일의 줄로 보였습니다).
pub const FILE_LINES: usize = 1_000_000;

thread_local! {
    static WARNINGS: RefCell<Vec<SiskinError>> = RefCell::new(Vec::new());
    static FILES: RefCell<Vec<(String, String)>> = RefCell::new(vec![(String::new(), String::new())]);
}

/// 실행을 막지는 않지만 알려 줄 것 (코드가 W 로 시작). 같은 자리는 한 번만.
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

/// 다른 파일을 등록하고, 그 파일을 읽을 때 쓸 줄 번호 시작값을 돌려줍니다.
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

/// 등록한 파일 목록을 통째로 꺼냅니다(새 작업 스레드에 넘길 때).
pub fn files_snapshot() -> Vec<(String, String)> {
    FILES.with(|f| f.borrow().clone())
}

/// 다른 스레드에서 꺼낸 파일 목록을 이 스레드에 넣습니다.
pub fn files_restore(v: Vec<(String, String)>) {
    FILES.with(|f| *f.borrow_mut() = v);
}

/// 합친 파일의 줄 번호를 (파일 이름, 파일 글, 그 파일 안의 줄) 로 되돌립니다. main 파일이면 None.
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

/// 디버거의 `b util.skn:5`: 이름이 이렇게 끝나는 import 한 파일의 (줄 시작값, 경로).
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

/// Siskin 컴파일러/런타임 진단.
///
/// 설계 문서 §9.2 "기계가 읽는 진단"에 따라 모든 오류는
/// 안정된 코드(E0001 등), 위치, 사람이 읽는 메시지, 그리고
/// 가능하면 적용 가능한 수정안(fix)을 함께 가집니다.
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
        // 모듈 안의 이름(`a·greet`)은 사람에게 `a.greet` 로 보입니다.
        SiskinError { code, msg: crate::ns::shown(&msg.into()), line, col, fix: None }
    }

    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(crate::ns::shown(&fix.into()));
        self
    }

    /// 위치를 1열로만 아는 오류(타입 이름, 패턴, match 등)는 그 줄에서 더 나은 칸을 고릅니다:
    /// 메시지에 `이름` 으로 적힌 것이 그 줄에 있으면 거기, 아니면 들여쓰기 다음 첫 글자.
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

    /// `--json` 용: main 파일 안의 오류면 `best_col` 로 칸을 고친 사본.
    pub fn with_best_col(&self, src: &str) -> SiskinError {
        let mut e = self.clone();
        if self.line < FILE_LINES {
            if let Some(l) = src.lines().nth(self.line.saturating_sub(1)) {
                e.col = self.best_col(l);
            }
        }
        e
    }

    /// 사람이 읽는 형식.
    pub fn render(&self, file: &str, src: &str) -> String {
        // import 로 합친 다른 파일 안의 오류면 그 파일 이름과 줄로 보여 줍니다.
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

    /// 기계가 읽는 형식 (`siskin check --json`).
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
