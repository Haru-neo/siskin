//! `siskin debug` — a debugger that steps line by line and shows values.
//!
//! Two modes share the same commands (n s o c b d p v l w q):
//! - Ordinary programs: runs on the interpreter. Right before each statement executes it
//!   asks `Debugger::should_stop`, and when stopped reads commands from the terminal.
//! - Programs using C libraries or std.net: compiled natively with stop points, then
//!   run as a child process (`rt_dbg.c`). When stopped, the child sends the line, call stack and
//!   variable values over a pipe, and we send commands back. `spawn` tasks (real threads) are followed too.
//! The debugger writes to stderr, so it does not mix with the program's output (stdout).
//! It also stops inside imported files (`b util.skn:5`). It never stops inside standard library pieces.

use crate::ast::Stmt;
use crate::interp::Interp;
use std::collections::BTreeSet;
use std::io::{BufRead, Write};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Stop at the next statement (even when entering a function)
    Step,
    /// Stop at the next statement at this depth or shallower (steps over calls)
    Next(usize),
    /// Stop once shallower than this depth (step out of the current function)
    Out(usize),
    /// Stop only at breakpoints
    Continue,
}

/// Queries to a stopped program. The interpreter and native modes each answer them.
pub trait Target {
    /// `p expr`
    fn eval(&mut self, src: &str) -> Result<String, String>;
    /// Local variables of the current function (name, value)
    fn locals(&self) -> Vec<(String, String)>;
    /// Call stack (function name, line), outermost first
    fn stack(&self) -> Vec<(String, usize)>;
}

impl Target for Interp {
    fn eval(&mut self, src: &str) -> Result<String, String> {
        self.debug_eval(src)
    }
    fn locals(&self) -> Vec<(String, String)> {
        self.debug_locals()
    }
    fn stack(&self) -> Vec<(String, usize)> {
        self.debug_stack()
    }
}

pub struct Debugger {
    /// Breakpoints (global line numbers: each imported file has a large offset added)
    breaks: BTreeSet<usize>,
    mode: Mode,
    src: Vec<String>,
    last_cmd: String,
    path: String,
    /// Got `q`. Terminate the program.
    pub quit: bool,
}

/// Whether a line is inside a standard library piece (never stop there).
fn in_std(line: usize) -> bool {
    matches!(crate::error::locate(line), Some((f, _, _)) if f.starts_with("<std."))
}

/// `12` or `util.skn:12` to a global line number.
pub fn parse_break(s: &str) -> Result<usize, String> {
    match s.rsplit_once(':') {
        Some((f, n)) => {
            let n: usize = n.trim().parse().map_err(|_| tr!("줄 번호가 숫자가 아닙니다", "the line number is not a number").to_string())?;
            let main_hit = f.trim().is_empty();
            if main_hit {
                return Ok(n);
            }
            match crate::error::find_file(f.trim()) {
                Some((base, _)) => Ok(base + n),
                None => Err(tr!(
                    format!("`{}` 은(는) 이 프로그램이 import 한 파일이 아닙니다", f.trim()),
                    format!("`{}` is not a file this program imports", f.trim())
                )),
            }
        }
        None => s.trim().parse().map_err(|_| tr!("줄 번호를 적어 주세요. 예: b 12  또는  b util.skn:5", "give a line number, e.g. b 12  or  b util.skn:5").to_string()),
    }
}

/// Line where a statement starts. Declarations (fn, struct ...) are not stop points.
pub fn stmt_line(s: &Stmt) -> Option<usize> {
    let l = match s {
        Stmt::Let { line, .. } | Stmt::Assign { line, .. } | Stmt::LetTuple { line, .. } => *line,
        Stmt::Expr(e, _) => e.pos().0,
        Stmt::If { arms, .. } => arms.first().map(|(c, _)| c.pos().0).unwrap_or(0),
        Stmt::While { cond, .. } => cond.pos().0,
        Stmt::For { line, .. } | Stmt::Match { line, .. } => *line,
        Stmt::Return(_, l, _) | Stmt::Break(l, _) | Stmt::Continue(l, _) => *l,
        Stmt::Arena { line, .. } | Stmt::Unsafe { line, .. } => *line,
        _ => 0,
    };
    if l > 0 {
        Some(l)
    } else {
        None
    }
}

fn say(msg: &str) {
    let mut e = std::io::stderr();
    let _ = e.write_all(msg.as_bytes());
    let _ = e.flush();
}

const HELP_EN: &str = "\
  n  (next)       go to the next line; a function called on this line runs in one go
  s  (step)       go to the next line, stepping into any function it calls
  o  (out)        run until the current function returns
  c  (continue)   keep running until a breakpoint (b)
  b 12            set a breakpoint at line 12.  `b` alone lists them
  d 12            delete the breakpoint at line 12
  p expr          show a value.  e.g. p xs   p xs.len()   p a + b
  v               show all variables in the current function
  l               show the code around the current line
  w               show the chain of calls that led here
  q               quit
  (a bare Enter repeats the last command)
";

const HELP_KO: &str = "\
  n  (다음)       다음 줄로. 함수를 부르는 줄이면 그 함수는 한 번에 실행합니다
  s  (안으로)     다음 줄로. 함수를 부르면 그 함수 안으로 들어갑니다
  o  (밖으로)     지금 함수가 끝날 때까지 실행합니다
  c  (계속)       멈출 곳(b)까지 계속 실행합니다
  b 12            12번째 줄에서 멈추게 합니다.  b 만 치면 목록
  d 12            12번째 줄의 멈출 곳을 지웁니다
  p 식            값을 봅니다.  예: p xs   p xs.len()   p a + b
  v               지금 함수의 변수를 전부 봅니다
  l               지금 줄 둘레의 코드를 봅니다
  w               어느 함수에서 어느 함수를 불러 여기까지 왔는지 봅니다
  q               끝냅니다
  (그냥 Enter 는 방금 한 명령을 한 번 더)
";

impl Debugger {
    pub fn new(path: &str, src: &str, breaks: BTreeSet<usize>) -> Self {
        Debugger {
            breaks,
            mode: Mode::Step,
            src: src.split('\n').map(|s| s.to_string()).collect(),
            last_cmd: "n".into(),
            path: path.to_string(),
            quit: false,
        }
    }

    pub fn should_stop(&self, line: usize, depth: usize) -> bool {
        if line == 0 || in_std(line) {
            return false;
        }
        if self.breaks.contains(&line) {
            return true;
        }
        match self.mode {
            Mode::Step => true,
            Mode::Next(d) => depth <= d,
            Mode::Out(d) => depth < d,
            Mode::Continue => false,
        }
    }

    /// Global line number → (display file path, that file's lines, line number within the file)
    fn file_of(&self, line: usize) -> (String, Vec<String>, usize) {
        match crate::error::locate(line) {
            Some((f, text, l)) => (f, text.split('\n').map(|s| s.to_string()).collect(), l),
            None => (self.path.clone(), self.src.clone(), line),
        }
    }

    /// Human-readable line name: `12` for the main file, `util.skn:5` for other files.
    fn line_name(&self, line: usize) -> String {
        match crate::error::locate(line) {
            Some((f, _, l)) => {
                let short = std::path::Path::new(&f).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(f);
                format!("{}:{}", short, l)
            }
            None => line.to_string(),
        }
    }

    fn text_at(&self, line: usize) -> String {
        let (_, lines, l) = self.file_of(line);
        lines.get(l.saturating_sub(1)).map(|s| s.trim().to_string()).unwrap_or_default()
    }

    fn show_line(&self, line: usize, func: &str) {
        let (path, _, l) = self.file_of(line);
        say(&format!("→ {}:{} ({})   {}\n", path, l, crate::ns::shown(func), self.text_at(line)));
    }

    fn list(&self, line: usize) {
        let (_, lines, l) = self.file_of(line);
        let base = line - l;
        let a = l.saturating_sub(4).max(1);
        let b = (l + 4).min(lines.len());
        for i in a..=b {
            let mark = if i == l { "→" } else if self.breaks.contains(&(base + i)) { "●" } else { " " };
            say(&format!("{} {:4} | {}\n", mark, i, lines[i - 1]));
        }
    }

    /// Command for the native side: all breakpoints, plus how to continue.
    fn wire(&self) -> String {
        let bs: Vec<String> = self.breaks.iter().map(|b| b.to_string()).collect();
        let (m, d) = match self.mode {
            Mode::Step => ('s', 0),
            Mode::Next(d) => ('n', d),
            Mode::Out(d) => ('o', d),
            Mode::Continue => ('c', 0),
        };
        format!("B {}\nG {} {}\n", bs.join(" "), m, d)
    }

    /// Stop and read commands. Returns on a command that resumes execution (n s o c).
    pub fn stop_at(&mut self, line: usize, func: &str, depth: usize, it: &mut dyn Target) {
        let _ = std::io::stdout().flush();
        if self.breaks.contains(&line) && self.mode == Mode::Continue {
            let n = self.line_name(line);
            say(&tr!(format!("● 멈출 곳 {}번째 줄\n", n), format!("● breakpoint at line {}\n", n)));
        }
        self.show_line(line, func);
        let stdin = std::io::stdin();
        loop {
            say("(siskin) ");
            let mut input = String::new();
            match stdin.lock().read_line(&mut input) {
                Ok(0) | Err(_) => {
                    say(tr!("\n입력이 끝나서 디버거를 마칩니다\n", "\nend of input; leaving the debugger\n"));
                    self.quit = true;
                    return;
                }
                Ok(_) => {}
            }
            let mut cmd = input.trim().to_string();
            if cmd.is_empty() {
                cmd = self.last_cmd.clone();
            }
            let (head, rest) = match cmd.split_once(char::is_whitespace) {
                Some((h, r)) => (h.to_string(), r.trim().to_string()),
                None => (cmd.clone(), String::new()),
            };
            match head.as_str() {
                "n" | "next" | "다음" => {
                    self.last_cmd = cmd;
                    self.mode = Mode::Next(depth);
                    return;
                }
                "s" | "step" | "안으로" => {
                    self.last_cmd = cmd;
                    self.mode = Mode::Step;
                    return;
                }
                "o" | "out" | "finish" | "밖으로" => {
                    self.last_cmd = cmd;
                    self.mode = Mode::Out(depth);
                    return;
                }
                "c" | "continue" | "계속" => {
                    self.last_cmd = cmd;
                    self.mode = Mode::Continue;
                    return;
                }
                "b" | "break" => {
                    if rest.is_empty() {
                        if self.breaks.is_empty() {
                            say(tr!(
                                "멈출 곳이 없습니다. `b 12` 처럼 줄 번호를 적으세요\n",
                                "no breakpoints; set one with a line number, like `b 12`\n"
                            ));
                        }
                        for b in &self.breaks {
                            say(&format!("● {:>4} | {}\n", self.line_name(*b), self.text_at(*b)));
                        }
                    } else {
                        match parse_break(&rest) {
                            Ok(n) if n % crate::error::FILE_LINES >= 1 && n % crate::error::FILE_LINES <= self.file_of(n).1.len() => {
                                self.breaks.insert(n);
                                let (nm, t) = (self.line_name(n), self.text_at(n));
                                say(&tr!(format!("{}번째 줄에서 멈춥니다: {}\n", nm, t), format!("breakpoint set at line {}: {}\n", nm, t)));
                            }
                            Ok(_) => {
                                let len = self.file_of(parse_break(&rest).unwrap_or(0)).1.len();
                                say(&tr!(
                                    format!("줄 번호는 1 부터 {} 사이로 적어 주세요\n", len),
                                    format!("line number must be between 1 and {}\n", len)
                                ))
                            }
                            Err(m) => say(&format!("{}\n", m)),
                        }
                    }
                }
                "d" | "delete" => match parse_break(&rest) {
                    Ok(n) if self.breaks.remove(&n) => {
                        let nm = self.line_name(n);
                        say(&tr!(format!("{}번째 줄의 멈출 곳을 지웠습니다\n", nm), format!("deleted the breakpoint at line {}\n", nm)))
                    }
                    _ => say(tr!(
                        "지울 멈출 곳의 줄 번호를 적어 주세요. 목록은 `b`\n",
                        "give the line number of the breakpoint to delete; list them with `b`\n"
                    )),
                },
                "p" | "print" => {
                    if rest.is_empty() {
                        say(tr!("볼 식을 적어 주세요. 예: p xs\n", "give an expression to show, e.g. p xs\n"));
                        continue;
                    }
                    match it.eval(&rest) {
                        Ok(v) => say(&format!("{}\n", v)),
                        Err(e) => say(&tr!(format!("계산할 수 없습니다: {}\n", e), format!("cannot evaluate: {}\n", e))),
                    }
                }
                "v" | "vars" => {
                    let vs = it.locals();
                    if vs.is_empty() {
                        say(tr!("(변수가 아직 없습니다)\n", "(no variables yet)\n"));
                    }
                    for (k, v) in vs {
                        say(&format!("  {} = {}\n", k, v));
                    }
                }
                "l" | "list" => self.list(line),
                "w" | "where" | "bt" => {
                    for (i, (f, l)) in it.stack().iter().filter(|(_, l)| *l > 0).enumerate() {
                        let t = if in_std(*l) { tr!("(표준 라이브러리)", "(standard library)").to_string() } else { self.text_at(*l) };
                        let ind = "  ".repeat(i);
                        let nm = self.line_name(*l);
                        say(&tr!(
                            format!("  {}{} {}번째 줄   {}\n", ind, crate::ns::shown(f), nm, t),
                            format!("  {}{} line {}   {}\n", ind, crate::ns::shown(f), nm, t)
                        ));
                    }
                }
                "q" | "quit" | "끝" => {
                    say(tr!("디버거를 마칩니다\n", "leaving the debugger\n"));
                    self.quit = true;
                    return;
                }
                "h" | "help" | "?" | "도움" => say(tr!(HELP_KO, HELP_EN)),
                other => say(&tr!(
                    format!("`{}` 은(는) 모르는 명령입니다. `h` 로 목록을 보세요\n", other),
                    format!("unknown command `{}`; type `h` for a list\n", other)
                )),
            }
        }
    }
}

// ------------------------------------------------------------ native mode

/// A stopped native program: the variable values and call stack it sent.
struct NativeStop<'a> {
    vars: Vec<(String, String)>,
    frames: Vec<(String, usize)>,
    /// Interpreter used to evaluate expressions like `p a + b` (only the program's declarations are loaded)
    calc: &'a mut Interp,
}

impl Target for NativeStop<'_> {
    fn eval(&mut self, src: &str) -> Result<String, String> {
        let t = src.trim();
        if let Some((_, v)) = self.vars.iter().rev().find(|(n, _)| n == t) {
            return Ok(v.clone());
        }
        self.calc.debug_eval_with(&self.vars, src)
    }
    fn locals(&self) -> Vec<(String, String)> {
        let mut m: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
        for (n, v) in &self.vars {
            m.insert(n.clone(), v.clone());
        }
        m.into_iter().collect()
    }
    fn stack(&self) -> Vec<(String, usize)> {
        self.frames.clone()
    }
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(unix)]
mod fds {
    extern "C" {
        pub fn pipe(fds: *mut i32) -> i32;
        pub fn close(fd: i32) -> i32;
    }
}

/// Launch the program as a child and connect two pipes to it.
/// (child, write end to child, read end from child). On failure, an exit code.
#[cfg(unix)]
fn spawn_piped(exe: &std::path::Path, args: &[String]) -> Result<(std::process::Child, std::fs::File, std::fs::File), i32> {
    use std::os::unix::io::FromRawFd;
    use std::os::unix::process::CommandExt;
    let mut to_child = [0i32; 2];
    let mut from_child = [0i32; 2];
    // Safety: receives both pipe ends into an array of size 2.
    if unsafe { fds::pipe(to_child.as_mut_ptr()) } != 0 || unsafe { fds::pipe(from_child.as_mut_ptr()) } != 0 {
        say(tr!("디버거 파이프를 만들 수 없습니다\n", "cannot create the debugger pipes\n"));
        return Err(2);
    }
    let (child_in, parent_out) = (to_child[0], to_child[1]);
    let (parent_in, child_out) = (from_child[0], from_child[1]);
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args).env("SISKIN_DBG_FDS", format!("{},{}", child_in, child_out));
    // Safety: after fork and before exec, only close the parent's ends (calls only close, which is async-signal-safe).
    unsafe {
        cmd.pre_exec(move || {
            fds::close(parent_out);
            fds::close(parent_in);
            Ok(())
        });
    }
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            say(&format!("{}: {}\n", tr!("컴파일한 프로그램을 실행할 수 없습니다", "cannot run the compiled program"), e));
            return Err(2);
        }
    };
    unsafe {
        fds::close(child_in);
        fds::close(child_out);
    }
    // Safety: wrap each freshly created pipe end in a File (each exactly once).
    let out = unsafe { std::fs::File::from_raw_fd(parent_out) };
    let inp = unsafe { std::fs::File::from_raw_fd(parent_in) };
    Ok((child, out, inp))
}

#[cfg(windows)]
mod win {
    #[repr(C)]
    pub struct SecurityAttributes {
        pub len: u32,
        pub desc: *mut std::ffi::c_void,
        pub inherit: i32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreatePipe(r: *mut isize, w: *mut isize, sa: *mut SecurityAttributes, size: u32) -> i32;
        pub fn SetHandleInformation(h: isize, mask: u32, flags: u32) -> i32;
        pub fn CloseHandle(h: isize) -> i32;
    }
}

/// Windows: create inheritable pipes and tell the child its handle values via SISKIN_DBG_FDS.
#[cfg(windows)]
fn spawn_piped(exe: &std::path::Path, args: &[String]) -> Result<(std::process::Child, std::fs::File, std::fs::File), i32> {
    use std::os::windows::io::FromRawHandle;
    let mut sa = win::SecurityAttributes { len: std::mem::size_of::<win::SecurityAttributes>() as u32, desc: std::ptr::null_mut(), inherit: 1 };
    let (mut child_in, mut parent_out, mut parent_in, mut child_out) = (0isize, 0isize, 0isize, 0isize);
    // Safety: passes slots for four handles. Clears the inherit flag on the parent's ends.
    let ok = unsafe {
        win::CreatePipe(&mut child_in, &mut parent_out, &mut sa, 0) != 0
            && win::CreatePipe(&mut parent_in, &mut child_out, &mut sa, 0) != 0
            && win::SetHandleInformation(parent_out, 1, 0) != 0
            && win::SetHandleInformation(parent_in, 1, 0) != 0
    };
    if !ok {
        say(tr!("디버거 파이프를 만들 수 없습니다\n", "cannot create the debugger pipes\n"));
        return Err(2);
    }
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args).env("SISKIN_DBG_FDS", format!("{},{}", child_in, child_out));
    let child = cmd.spawn();
    // Safety: handed to the child, so the parent closes the child's ends.
    unsafe {
        win::CloseHandle(child_in);
        win::CloseHandle(child_out);
    }
    let child = match child {
        Ok(c) => c,
        Err(e) => {
            say(&format!("{}: {}\n", tr!("컴파일한 프로그램을 실행할 수 없습니다", "cannot run the compiled program"), e));
            return Err(2);
        }
    };
    // Safety: wrap each freshly created pipe end in a File (each exactly once).
    let out = unsafe { std::fs::File::from_raw_handle(parent_out as *mut std::ffi::c_void) };
    let inp = unsafe { std::fs::File::from_raw_handle(parent_in as *mut std::ffi::c_void) };
    Ok((child, out, inp))
}

/// Run the program compiled with stop points (`exe`) as a child and follow it.
/// Returns the program's exit code.
pub fn run_native(exe: &std::path::Path, args: &[String], dbg: &mut Debugger, prog: &crate::ast::Program) -> i32 {
    let (mut child, mut out, inp) = match spawn_piped(exe, args) {
        Ok(x) => x,
        Err(code) => return code,
    };
    let inp = std::io::BufReader::new(inp);
    let mut calc = Interp::new();
    calc.debug_prepare(prog);
    let mut lines = inp.lines();
    while let Some(Ok(l)) = lines.next() {
        let Some(rest) = l.strip_prefix("STOP ") else { continue };
        let nums: Vec<usize> = rest.split(' ').filter_map(|x| x.parse().ok()).collect();
        let (line, depth, task) = (nums.first().copied().unwrap_or(0), nums.get(1).copied().unwrap_or(0), nums.get(2).copied().unwrap_or(0));
        let mut frames = Vec::new();
        let mut vars = Vec::new();
        for l in lines.by_ref() {
            let Ok(l) = l else { break };
            if l == "." {
                break;
            }
            if let Some(f) = l.strip_prefix("F ") {
                let (n, ln) = f.split_once('\t').unwrap_or((f, "0"));
                frames.push((n.to_string(), ln.parse().unwrap_or(0)));
            } else if let Some(v) = l.strip_prefix("V ") {
                let (n, val) = v.split_once('\t').unwrap_or((v, ""));
                vars.push((n.to_string(), unescape(val)));
            }
        }
        let func = frames.last().map(|f| f.0.clone()).unwrap_or_default();
        if task > 0 {
            say(&tr!(format!("(작업 {})\n", task), format!("(task {})\n", task)));
        }
        let mut stop = NativeStop { vars, frames, calc: &mut calc };
        dbg.stop_at(line, &func, depth, &mut stop);
        if dbg.quit {
            let _ = out.write_all(b"Q\n").and_then(|_| out.flush());
            let _ = child.wait();
            return 0;
        }
        if out.write_all(dbg.wire().as_bytes()).and_then(|_| out.flush()).is_err() {
            break;
        }
    }
    match child.wait() {
        Ok(st) => st.code().unwrap_or(1),
        Err(_) => 1,
    }
}
