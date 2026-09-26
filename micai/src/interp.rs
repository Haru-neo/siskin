use crate::ast::*;
use crate::error::SiskinError;
use crate::value::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Abnormal control flow during execution.
pub enum Flow {
    Return(Value),
    Break,
    Continue,
    /// An unrecoverable runtime error. Distinct from a `!T` error (Value::Error).
    Fail(SiskinError),
}

type R<T> = Result<T, Flow>;

fn fail<T>(code: &'static str, msg: impl Into<String>, line: usize, col: usize) -> R<T> {
    Err(Flow::Fail(SiskinError::new(code, msg, line, col)))
}

fn fail_fix<T>(
    code: &'static str,
    msg: impl Into<String>,
    line: usize,
    col: usize,
    fix: impl Into<String>,
) -> R<T> {
    Err(Flow::Fail(SiskinError::new(code, msg, line, col).with_fix(fix)))
}

/// The initial value `alloc[T](n)` fills in. Starts at 0, like C's calloc.
fn zero_of(t: Option<&TypeExpr>) -> Value {
    match t {
        Some(TypeExpr::Named(n, _)) => match n.as_str() {
            "Float" => Value::Float(0.0),
            "Bool" => Value::Bool(false),
            "Str" => Value::Str(Rc::new(String::new())),
            _ => Value::Int(0),
        },
        _ => Value::Int(0),
    }
}

/// Checks whether a pointer access is safe. Catches use-after-free and out-of-bounds access here.
fn check_raw(b: &RawBuf, off: usize, n: i64, line: usize, col: usize) -> R<()> {
    if !b.alive {
        return fail_fix(
            "E0236",
            tr!("이미 해제된 메모리에 접근했습니다", "access to freed memory"),
            line,
            col,
            tr!("`free` 뒤에는, 또 `with arena` 블록을 벗어난 뒤에는 그 포인터를 쓸 수 없습니다", "a pointer cannot be used after `free` or after leaving its `with arena` block"),
        );
    }
    let at = off as i64 + n;
    if at < 0 || at >= b.data.len() as i64 {
        return fail_fix(
            "E0237",
            tr!(format!("포인터 접근 {}이(가) 범위를 벗어납니다 (크기 {})", at, b.data.len()), format!("pointer access {} is out of bounds (size {})", at, b.data.len())),
            line,
            col,
            tr!("`alloc`에 넘긴 개수보다 작은 위치만 읽고 쓸 수 있습니다", "only offsets below the count passed to `alloc` can be read or written"),
        );
    }
    Ok(())
}

struct Frame {
    scopes: Vec<HashMap<String, Value>>,
    /// This frame's function name and the line currently executing. Used by the debugger.
    func: String,
    cur_line: usize,
}

pub struct Interp {
    fns: HashMap<String, crate::ast::Shared<FnDecl>>,
    structs: HashMap<String, crate::ast::Shared<StructDecl>>,
    enums: HashMap<String, crate::ast::Shared<EnumDecl>>,
    /// variant name -> enum name
    variant_of: HashMap<String, String>,
    /// Imported names. Visible from every function, across frames.
    globals: HashMap<String, Value>,
    frames: Vec<Frame>,
    /// Whether contracts (requires/ensures) are checked. On in debug builds.
    pub contracts: bool,
    depth: usize,
    /// State of `std.random`.
    rng: u64,
    /// Command-line arguments passed to the program. Returned by `args()`.
    pub prog_args: Vec<String>,
    /// Final values of `inout` arguments from the call that just ended (positional index excluding self, name, value).
    /// Written back to the caller.
    inout_out: Vec<(usize, String, Value)>,
    /// Captured values of a closure that the next `call_fn_with_self` lays into the new frame first.
    pending_env: Option<Rc<Closure>>,
    /// Stdout and stderr of the program just run with `__run`.
    run_out: String,
    run_err: String,
    /// Present only when running under `siskin debug`.
    pub dbg: Option<Box<crate::debug::Debugger>>,
}

const PRELUDE: &[&str] = &[
    "print", "eprint", "len", "range", "str", "int", "float", "error", "assert", "abs", "min", "max", "sum",
    "input", "args", "exit", "channel",
    // Small built-ins used by the Siskin parts of the standard library (std/*.skn)
    "__time_parts", "__time_make", "__run", "__run_out", "__run_err", "__ko",
    "__net_open", "__net_send", "__net_recv", "__net_recv_line", "__net_close", "__net_listen",
    "__net_accept", "__net_peer", "__net_port", "__net_error", "__net_timeout", "__http",
    "__http_headers", "__http_body", "__url_encode", "__net_listen_tls", "__net_recv_n", "__net_url_decode", "__net_byte_len",
];

/// Safely clamps the range of `slice(a, b)`. Out-of-range bounds are clipped.
fn slice_bounds(args: &[Value], n: i64) -> (i64, i64) {
    let get = |i: usize, d: i64| match args.get(i) {
        Some(Value::Int(v)) => *v,
        _ => d,
    };
    let a = get(0, 0).max(0).min(n);
    let b = get(1, n).max(a).min(n);
    (a, b)
}

/// Display width. Hangul, CJK, full-width and emoji count as two columns. Same rule as native mi_char_width.
fn char_width(c: char) -> i64 {
    let u = c as u32;
    if (0x1100..=0x115F).contains(&u)
        || (0x2E80..=0xA4CF).contains(&u)
        || (0xAC00..=0xD7A3).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0xFE30..=0xFE4F).contains(&u)
        || (0xFF00..=0xFF60).contains(&u)
        || (0xFFE0..=0xFFE6).contains(&u)
        || (0x1F300..=0x1FAFF).contains(&u)
        || (0x20000..=0x3FFFD).contains(&u)
    {
        2
    } else {
        1
    }
}
fn disp_width(s: &str) -> i64 {
    s.chars().map(char_width).sum()
}

/// Rounding. 0.5 rounds away from zero (same as C's round).
fn mi_round(f: f64) -> i64 {
    if f >= 0.0 {
        (f + 0.5).floor() as i64
    } else {
        (f - 0.5).ceil() as i64
    }
}

/// Seconds elapsed since the program started. Used for timing.
fn mi_clock() -> f64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    let s = START.get_or_init(std::time::Instant::now);
    s.elapsed().as_secs_f64()
}

/// The interpreter and native code must produce the same random numbers,
/// so both sides use the same formula (xorshift64*).
fn mi_next_rand(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545F4914F6CDD1D)
}

pub(crate) const MODULES: &[(&str, &[&str])] = &[
    (
        "math",
        &["sqrt", "floor", "ceil", "pow", "sin", "cos", "tan", "log", "log10", "exp", "round", "pi", "e"],
    ),
    ("fs", &["read_text", "write_text", "append_text", "exists", "remove", "list_dir", "make_dir", "is_dir"]),
    ("io", &["print"]),
    ("time", &["now", "clock", "sleep"]),
    ("process", &["env", "set_env", "cwd", "set_cwd", "pid"]),
    ("net", &[]),
    ("random", &["seed", "rand", "rand_int"]),
    ("re", &["test", "find", "find_all", "groups", "replace", "split_re"]),
    (
        "json",
        &["parse", "stringify", "jnull", "jbool", "jint", "jfloat", "jstr", "jlist", "jdict"],
    ),
];

impl Interp {
    pub fn new() -> Self {
        Interp {
            fns: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            variant_of: HashMap::new(),
            globals: HashMap::new(),
            frames: vec![Frame { scopes: vec![HashMap::new()], func: String::new(), cur_line: 0 }],
            dbg: None,
            contracts: true,
            depth: 0,
            rng: 0x853C49E6748FEA9B,
            prog_args: Vec::new(),
            inout_out: Vec::new(),
            pending_env: None,
            run_out: String::new(),
            run_err: String::new(),
        }
    }

    // ------------------------------------------------------------- scope management

    fn frame(&mut self) -> &mut Frame {
        self.frames.last_mut().unwrap()
    }

    fn push_scope(&mut self) {
        self.frame().scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.frame().scopes.pop();
    }

    fn declare(&mut self, name: &str, v: Value) {
        self.frame().scopes.last_mut().unwrap().insert(name.to_string(), v);
    }

    fn lookup(&self, name: &str) -> Option<Value> {
        for s in self.frames.last().unwrap().scopes.iter().rev() {
            if let Some(v) = s.get(name) {
                return Some(v.clone());
            }
        }
        // Imported names are visible across function boundaries.
        self.globals.get(name).cloned()
    }

    /// Looks up only local names in the current function (excluding imports and top-level functions).
    fn lookup_local(&self, name: &str) -> Option<Value> {
        for s in self.frames.last().unwrap().scopes.iter().rev() {
            if let Some(v) = s.get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    /// Creates a closure. Copies the outer local variables used by the body at their current values.
    fn make_closure(&self, f: &crate::ast::Shared<FnDecl>) -> Value {
        let mut env = Vec::new();
        for n in free_vars(f) {
            if let Some(v) = self.lookup_local(&n) {
                env.push((n, v.deep_clone()));
            }
        }
        Value::Closure(Rc::new(Closure { decl: f.clone(), env }))
    }

    fn call_closure(
        &mut self,
        c: &Rc<Closure>,
        pos: Vec<Value>,
        named: Vec<(String, Value)>,
        line: usize,
        col: usize,
    ) -> R<Value> {
        self.pending_env = Some(c.clone());
        let r = self.call_fn_with_self(&c.decl, None, pos, named, line, col);
        self.pending_env = None;
        r
    }

    fn assign_existing(&mut self, name: &str, v: Value) -> bool {
        for s in self.frames.last_mut().unwrap().scopes.iter_mut().rev() {
            if s.contains_key(name) {
                s.insert(name.to_string(), v);
                return true;
            }
        }
        false
    }

    /// Writes a value to an lvalue (`x`, `x.f`, `x[i]`). `rhs` must already be deep_clone'd.
    fn store(&mut self, target: &Expr, rhs: Value, line: usize, col: usize) -> R<()> {
        match target {
            Expr::Ident(name, l, c) => {
                if !self.assign_existing(name, rhs) {
                    return fail_fix(
                        "E0212",
                        tr!(format!("`{}`은(는) 선언되지 않았습니다", name), format!("`{}` is not declared", name)),
                        *l,
                        *c,
                        tr!(format!("`var {} = ...` 로 먼저 선언하세요", name), format!("declare it first with `var {} = ...`", name)),
                    );
                }
                Ok(())
            }
            Expr::Field(obj, fname, l, c) => {
                let o = self.eval(obj)?;
                match o {
                    Value::Struct(s) => {
                        let mut s = s.borrow_mut();
                        let sname = s.name.clone();
                        match s.fields.iter_mut().find(|(n, _)| n == fname) {
                            Some(slot) => {
                                slot.1 = rhs;
                                Ok(())
                            }
                            None => fail(
                                "E0213",
                                tr!(format!("`{}`에 `{}` 필드가 없습니다", sname, fname), format!("`{}` has no field `{}`", sname, fname)),
                                *l,
                                *c,
                            ),
                        }
                    }
                    other => fail(
                        "E0214",
                        tr!(format!("{} 값에는 필드를 대입할 수 없습니다", other.type_name()), format!("cannot assign to a field of a {} value", other.type_name())),
                        *l,
                        *c,
                    ),
                }
            }
            Expr::Index(obj, idx, l, c) => {
                let o = self.eval(obj)?;
                let i = self.eval(idx)?;
                if let (Value::Raw(b, off), Value::Int(n)) = (&o, &i) {
                    let mut bb = b.borrow_mut();
                    check_raw(&bb, *off, *n, *l, *c)?;
                    let at = (*off as i64 + *n) as usize;
                    bb.data[at] = rhs;
                    return Ok(());
                }
                match (&o, &i) {
                    (Value::List(items), Value::Int(n)) => {
                        let mut items = items.borrow_mut();
                        let len = items.len() as i64;
                        if *n < 0 || *n >= len {
                            return fail_fix(
                                "E0204",
                                tr!(format!("인덱스 {}이(가) 범위를 벗어납니다 (길이 {})", n, len), format!("index {} out of range (length {})", n, len)),
                                *l,
                                *c,
                                tr!("인덱스는 0부터 길이-1까지입니다", "valid indices run from 0 to length-1"),
                            );
                        }
                        items[*n as usize] = rhs;
                        Ok(())
                    }
                    (Value::Dict(pairs), key) => {
                        let mut pairs = pairs.borrow_mut();
                        for (k, v) in pairs.iter_mut() {
                            if k.eq_value(key) {
                                *v = rhs;
                                return Ok(());
                            }
                        }
                        pairs.push((key.clone(), rhs));
                        Ok(())
                    }
                    _ => fail(
                        "E0215",
                        tr!(format!("{}[{}] 에는 대입할 수 없습니다", o.type_name(), i.type_name()), format!("cannot assign to {}[{}]", o.type_name(), i.type_name())),
                        *l,
                        *c,
                    ),
                }
            }
            _ => fail("E0106", tr!("대입할 수 없는 대상입니다", "invalid assignment target"), line, col),
        }
    }

    /// Writes the final `inout` scalar values of the call that just ended back to the caller's lvalues.
    /// Used by both function and method calls (for methods, only the arguments excluding self are passed).
    fn writeback_inout(&mut self, args: &[Arg]) -> R<()> {
        if self.inout_out.is_empty() {
            return Ok(());
        }
        let outs = std::mem::take(&mut self.inout_out);
        let is_lvalue = |e: &Expr| matches!(e, Expr::Ident(..) | Expr::Field(..) | Expr::Index(..));
        let mut todo: Vec<(Expr, Value)> = Vec::new();
        let mut pos_i = 0usize;
        for a in args {
            match &a.name {
                Some(n) => {
                    if let Some((_, _, v)) = outs.iter().find(|(_, pn, _)| pn == n) {
                        if is_lvalue(&a.value) {
                            todo.push((a.value.clone(), v.clone()));
                        }
                    }
                }
                None => {
                    let idx = pos_i;
                    pos_i += 1;
                    if let Some((_, _, v)) = outs.iter().find(|(i, _, _)| *i == idx) {
                        if is_lvalue(&a.value) {
                            todo.push((a.value.clone(), v.clone()));
                        }
                    }
                }
            }
        }
        for (e, v) in todo {
            self.store(&e, v, 0, 0)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------ execution

    /// Registers declarations first, then runs top-level statements, and calls `main` if present.
    /// Debugger: if execution should stop at this line, stop and take commands.
    fn debug_hook(&mut self, line: usize) {
        let mut d = match self.dbg.take() {
            Some(d) => d,
            None => return,
        };
        let func = self.frames.last().map(|f| f.func.clone()).unwrap_or_default();
        let depth = self.frames.len();
        if d.should_stop(line, depth) {
            d.stop_at(line, &func, depth, self);
            if d.quit {
                std::process::exit(0);
            }
        }
        self.dbg = Some(d);
    }

    /// Local variables of the current function for the debugger to show (sorted by name).
    pub fn debug_locals(&self) -> Vec<(String, String)> {
        let mut m: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
        if let Some(f) = self.frames.last() {
            for sc in &f.scopes {
                for (k, v) in sc {
                    m.insert(k.clone(), v.repr());
                }
            }
        }
        m.into_iter().collect()
    }

    /// The debugger's call path: (function name, line), outermost first.
    pub fn debug_stack(&self) -> Vec<(String, usize)> {
        self.frames.iter().filter(|f| !f.func.is_empty()).map(|f| (f.func.clone(), f.cur_line)).collect()
    }

    /// For the native debugger: loads only the program's declarations (functions, structs …) and imports.
    pub fn debug_prepare(&mut self, prog: &Program) {
        self.collect(&prog.stmts);
        for s in &prog.stmts {
            if matches!(s, Stmt::Import { .. }) {
                let _ = self.exec(s);
            }
        }
    }

    /// The native debugger's `p expr`: parses the variable values (as text) sent by the stopped program
    /// back into values, puts them in a fresh scope and evaluates the expression. Variables that can't be parsed as values (`<task>` etc.) are skipped.
    pub fn debug_eval_with(&mut self, vars: &[(String, String)], src: &str) -> Result<String, String> {
        self.frames.push(Frame { scopes: vec![HashMap::new()], func: String::new(), cur_line: 0 });
        for (n, text) in vars {
            let v = match crate::parser::parse_expr_str(text) {
                Ok(e) => self.eval(&e),
                Err(_) => continue,
            };
            if let Ok(v) = v {
                if let Some(f) = self.frames.last_mut() {
                    f.scopes[0].insert(n.clone(), v);
                }
            }
        }
        let r = self.debug_eval(src);
        self.frames.pop();
        r
    }

    /// The debugger's `p expr`. Evaluates an expression at the current location.
    pub fn debug_eval(&mut self, src: &str) -> Result<String, String> {
        let e = crate::parser::parse_expr_str(src).map_err(|e| e.msg)?;
        match self.eval(&e) {
            Ok(v) => Ok(v.repr()),
            Err(Flow::Fail(e)) => Err(e.msg),
            Err(_) => Err(tr!("계산할 수 없습니다", "cannot evaluate").into()),
        }
    }

    pub fn run_program(&mut self, prog: &Program) -> Result<(), SiskinError> {
        crate::conc::enter();
        let r = self.run_program_inner(prog);
        // When main ends, wait for all still-running tasks (same as native).
        if r.is_ok() && crate::conc::wait_all().is_err() {
            crate::conc::report_plain("E0260", crate::conc::deadlock_msg());
        }
        crate::conc::shutdown();
        r
    }

    fn run_program_inner(&mut self, prog: &Program) -> Result<(), SiskinError> {
        self.collect(&prog.stmts);
        for s in &prog.stmts {
            if matches!(
                s,
                Stmt::Fn(_) | Stmt::Struct(_) | Stmt::Enum(_) | Stmt::Interface(_)
            ) {
                continue;
            }
            let r = self.exec(s);
            // A top-level `let` is a constant visible from every function.
            if let (Ok(()), Stmt::Let { name, .. }) = (&r, s) {
                let v = self.frames.first_mut().and_then(|f| f.scopes.last_mut()).and_then(|sc| sc.remove(name));
                if let Some(v) = v {
                    self.globals.insert(name.clone(), v);
                }
            }
            match r {
                Ok(()) => {}
                Err(Flow::Fail(e)) => return Err(e),
                Err(Flow::Return(_)) => {
                    return Err(SiskinError::new("E0207", tr!("최상위에서는 return할 수 없습니다", "cannot `return` at the top level"), 0, 0))
                }
                Err(_) => {
                    return Err(SiskinError::new("E0208", tr!("반복문 밖에서 break/continue를 썼습니다", "`break`/`continue` used outside of a loop"), 0, 0))
                }
            }
        }
        if let Some(main) = self.fns.get("main").cloned() {
            match self.call_fn(&main, Vec::new(), Vec::new(), main.line, 1) {
                // If `fn main() -> !Unit` ends with an error, report it and exit with a failure code (same as native).
                Ok(Value::Error(m)) => {
                    use std::io::Write;
                    crate::conc::shutdown();
                    let _ = std::io::stdout().flush();
                    eprintln!("{}", tr!(format!("오류: {}", m), format!("error: {}", m)));
                    std::process::exit(1);
                }
                Ok(Value::ErrorOf(v)) => {
                    use std::io::Write;
                    crate::conc::shutdown();
                    let _ = std::io::stdout().flush();
                    eprintln!("{}", tr!(format!("오류: {}", v.repr()), format!("error: {}", v.repr())));
                    std::process::exit(1);
                }
                Ok(_) => {}
                Err(Flow::Fail(e)) => return Err(e),
                Err(_) => {}
            }
        }
        Ok(())
    }

    fn collect(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            match s {
                Stmt::Fn(f) => {
                    self.fns.insert(f.name.clone(), f.clone());
                }
                Stmt::Struct(sd) => {
                    self.structs.insert(sd.name.clone(), sd.clone());
                }
                Stmt::Enum(ed) => {
                    for v in &ed.variants {
                        self.variant_of.insert(v.name.clone(), ed.name.clone());
                    }
                    self.enums.insert(ed.name.clone(), ed.clone());
                }
                _ => {}
            }
        }
    }

    fn exec_block(&mut self, stmts: &[Stmt]) -> R<()> {
        self.push_scope();
        let mut result = Ok(());
        for s in stmts {
            if let Err(f) = self.exec(s) {
                result = Err(f);
                break;
            }
        }
        self.pop_scope();
        result
    }

    /// Handles `expr catch e:`. On error, runs the catch block and returns None.
    /// With `want_value` (`let x = f() catch e:`), the catch block's last expression is the value used in place of x.
    fn eval_with_catch(&mut self, e: &Expr, catch: &Option<CatchClause>, want_value: bool) -> R<Option<Value>> {
        let v = self.eval(e)?;
        if matches!(v, Value::Error(_) | Value::ErrorOf(_)) {
            if let Some(c) = catch {
                self.push_scope();
                let ev = match &v {
                    Value::ErrorOf(x) => x.as_ref().clone(),
                    Value::Error(msg) => str_value(msg.as_ref().clone()),
                    _ => unreachable!(),
                };
                self.declare(&c.name, ev);
                let fallback = if want_value { crate::ast::catch_fallback(c) } else { None };
                let n = c.body.len() - if fallback.is_some() { 1 } else { 0 };
                let mut out = Ok(None);
                for s in &c.body[..n] {
                    if let Err(f) = self.exec(s) {
                        out = Err(f);
                        break;
                    }
                }
                if out.is_ok() {
                    if let Some(fe) = fallback {
                        // The type checker has already verified this value has x's type.
                        out = self.eval(fe).map(Some);
                    }
                }
                self.pop_scope();
                return out;
            }
        }
        Ok(Some(v))
    }

    fn exec(&mut self, stmt: &Stmt) -> R<()> {
        if self.dbg.is_some() {
            if let Some(l) = crate::debug::stmt_line(stmt) {
                if let Some(f) = self.frames.last_mut() {
                    f.cur_line = l;
                }
                self.debug_hook(l);
            }
        }
        match stmt {
            Stmt::Link(_, _) | Stmt::CHeader { .. } => Ok(()),

            Stmt::Arena { name, body, .. } => {
                let chunks: Rc<RefCell<Vec<Rc<RefCell<RawBuf>>>>> =
                    Rc::new(RefCell::new(Vec::new()));
                self.push_scope();
                self.declare(name, Value::Arena(Rc::clone(&chunks)));
                let r = self.exec_block(body);
                self.pop_scope();
                // However the block is exited, the arena is freed in one go.
                for ch in chunks.borrow().iter() {
                    let mut c = ch.borrow_mut();
                    c.alive = false;
                    c.data.clear();
                }
                r
            }

            Stmt::Unsafe { body, .. } => {
                self.push_scope();
                let r = self.exec_block(body);
                self.pop_scope();
                r
            }

            Stmt::Fn(f) => {
                if self.frames.len() > 1 {
                    // A `fn` inside a function is a local closure that captures outer values.
                    let c = self.make_closure(f);
                    self.declare(&f.name, c);
                } else {
                    self.fns.insert(f.name.clone(), f.clone());
                }
                Ok(())
            }
            Stmt::Struct(sd) => {
                self.structs.insert(sd.name.clone(), sd.clone());
                Ok(())
            }
            Stmt::Enum(ed) => {
                for v in &ed.variants {
                    self.variant_of.insert(v.name.clone(), ed.name.clone());
                }
                self.enums.insert(ed.name.clone(), ed.clone());
                Ok(())
            }
            Stmt::Interface(_) => Ok(()),

            Stmt::Import { path, names, line, col, .. } => {
                let module: &'static str = match path.last().map(|s| s.as_str()) {
                    Some("math") => "math",
                    Some("fs") => "fs",
                    Some("io") => "io",
                    Some("time") => "time",
                    Some("random") => "random",
                    Some("re") => "re",
                    Some("json") => "json",
                    Some("process") => "process",
                    Some("net") => "net",
                    Some("prelude") => "prelude",
                    Some(other) => {
                        return fail_fix(
                            "E0209",
                            tr!(format!("모듈 `{}`를 찾을 수 없습니다", other), format!("module `{}` not found", other)),
                            *line,
                            *col,
                            tr!("쓸 수 있는 모듈: std.math, std.fs, std.io, std.time, std.random, std.re, std.json, std.process, std.net", "available modules: std.math, std.fs, std.io, std.time, std.random, std.re, std.json, std.process, std.net"),
                        )
                    }
                    None => return fail("E0209", tr!("모듈 경로가 비어 있습니다", "empty module path"), *line, *col),
                };
                if names.is_empty() {
                    self.globals.insert(module.to_string(), Value::Module(module));
                    return Ok(());
                }
                let avail = MODULES.iter().find(|(m, _)| *m == module).map(|(_, f)| *f).unwrap_or(&[]);
                for n in names {
                    // The parts of the standard library written in Siskin (DateTime, run …) are already merged in.
                    if self.fns.contains_key(n) || self.structs.contains_key(n) {
                        continue;
                    }
                    match avail.iter().find(|f| **f == n.as_str()) {
                        Some(f) => {
                            self.globals.insert(n.clone(), Value::Builtin(*f));
                        }
                        None => {
                            return fail_fix(
                                "E0210",
                                tr!(format!("`std.{}`에 `{}`이(가) 없습니다", module, n), format!("`std.{}` has no `{}`", module, n)),
                                *line,
                                *col,
                                tr!(format!(
                                    "이 모듈에 있는 것: {}",
                                    crate::types::Types::std_members(module)
                                        .map(|v| v.join(", "))
                                        .unwrap_or_else(|| avail.join(", "))
                                ), format!(
                                    "this module provides: {}",
                                    crate::types::Types::std_members(module)
                                        .map(|v| v.join(", "))
                                        .unwrap_or_else(|| avail.join(", "))
                                )),
                            )
                        }
                    }
                }
                Ok(())
            }

            Stmt::Let { name, value, mutable, catch, line, col, .. } => {
                let v = match self.eval_with_catch(value, catch, true)? {
                    Some(v) => v,
                    None => return Ok(()),
                };
                if self.frames.last().unwrap().scopes.last().unwrap().contains_key(name) {
                    return fail_fix(
                        "E0211",
                        tr!(format!("`{}`은(는) 이 블록에서 이미 선언되었습니다", name), format!("`{}` is already declared in this block", name)),
                        *line,
                        *col,
                        tr!("다른 이름을 쓰거나, 값을 바꾸려면 `var`로 선언한 뒤 대입하세요", "use a different name, or declare it with `var` and assign to change its value"),
                    );
                }
                let _ = mutable;
                self.declare(name, v.deep_clone());
                Ok(())
            }

            Stmt::LetTuple { names, value, line, col } => {
                let v = self.eval(value)?;
                let items = match &v {
                    Value::Tuple(items) => items.clone(),
                    other => {
                        return fail_fix(
                            "E0212",
                            tr!(format!("튜플로 풀어 받으려면 오른쪽이 튜플이어야 하는데 {}입니다", other.type_name()), format!("tuple destructuring needs a tuple on the right, found {}", other.type_name())),
                            *line,
                            *col,
                            tr!("`let (a, b) = ...`는 `(값1, 값2)` 형태의 튜플에만 씁니다", "`let (a, b) = ...` only works with tuples like `(value1, value2)`"),
                        )
                    }
                };
                if items.len() != names.len() {
                    return fail_fix(
                        "E0213",
                        tr!(format!("튜플에는 값이 {}개인데 이름을 {}개 적었습니다", items.len(), names.len()), format!("the tuple has {} values but {} names were given", items.len(), names.len())),
                        *line,
                        *col,
                        tr!("이름 개수를 튜플 값 개수와 맞추세요", "use as many names as the tuple has values"),
                    );
                }
                for (name, item) in names.iter().zip(items.iter()) {
                    if self.frames.last().unwrap().scopes.last().unwrap().contains_key(name) {
                        return fail_fix(
                            "E0211",
                            tr!(format!("`{}`은(는) 이 블록에서 이미 선언되었습니다", name), format!("`{}` is already declared in this block", name)),
                            *line,
                            *col,
                            tr!("다른 이름을 쓰세요", "use a different name"),
                        );
                    }
                    self.declare(name, item.deep_clone());
                }
                Ok(())
            }

            Stmt::Assign { target, op, value, catch, line, col } => {
                let rhs = match self.eval_with_catch(value, catch, true)? {
                    Some(v) => v,
                    None => return Ok(()),
                };
                let rhs = match op {
                    None => rhs,
                    Some(o) => {
                        let cur = self.eval(target)?;
                        self.binary(*o, cur, rhs, *line, *col)?
                    }
                };
                let rhs = rhs.deep_clone();
                self.store(target, rhs, *line, *col)
            }

            Stmt::Expr(e, catch) => {
                self.eval_with_catch(e, catch, false)?;
                Ok(())
            }

            Stmt::If { arms, els } => {
                for (cond, body) in arms {
                    let (l, c) = cond.pos();
                    let v = self.eval(cond)?;
                    match v {
                        Value::Bool(true) => return self.exec_block(body),
                        Value::Bool(false) => continue,
                        other => {
                            return fail_fix(
                                "E0202",
                                tr!(format!("조건은 Bool이어야 하는데 {}입니다", other.type_name()), format!("condition must be Bool, found {}", other.type_name())),
                                l,
                                c,
                                tr!("Siskin에는 암묵적 참/거짓 변환이 없습니다. `x != 0` 처럼 명시하세요", "Siskin has no implicit truthiness; write the comparison explicitly, like `x != 0`"),
                            )
                        }
                    }
                }
                if let Some(b) = els {
                    return self.exec_block(b);
                }
                Ok(())
            }

            Stmt::While { cond, body } => loop {
                let (l, c) = cond.pos();
                match self.eval(cond)? {
                    Value::Bool(true) => {}
                    Value::Bool(false) => return Ok(()),
                    other => {
                        return fail_fix(
                            "E0202",
                            tr!(format!("조건은 Bool이어야 하는데 {}입니다", other.type_name()), format!("condition must be Bool, found {}", other.type_name())),
                            l,
                            c,
                            tr!("Siskin에는 암묵적 참/거짓 변환이 없습니다", "Siskin has no implicit truthiness"),
                        )
                    }
                }
                match self.exec_block(body) {
                    Ok(()) => {}
                    Err(Flow::Break) => return Ok(()),
                    Err(Flow::Continue) => {}
                    Err(other) => return Err(other),
                }
            },

            Stmt::For { var, var2, iter, body, line } => {
                // `for i in range(...)` just counts without building a list (same result, saves memory).
                if let (None, Expr::Call { callee, targs, args, line: cl, col: cc }) = (var2, iter) {
                    if matches!(&**callee, Expr::Ident(n, ..) if n == "range")
                        && targs.is_empty()
                        && args.iter().all(|a| a.name.is_none())
                        && !self.fns.contains_key("range")
                        && self.lookup_local("range").is_none()
                        && matches!(self.globals.get("range"), None | Some(Value::Builtin(_)))
                    {
                        let mut vals = Vec::new();
                        for a in args {
                            vals.push(self.eval(&a.value)?);
                        }
                        let bounds = match vals.as_slice() {
                            [Value::Int(n)] => Some((0, *n)),
                            [Value::Int(a), Value::Int(b)] => Some((*a, *b)),
                            _ => None,
                        };
                        let (from, to) = match bounds {
                            Some(b) => b,
                            // For invalid usage, the regular range produces the same error.
                            None => {
                                self.call_builtin("range", vals, Vec::new(), *cl, *cc)?;
                                return Ok(());
                            }
                        };
                        for i in from..to {
                            self.push_scope();
                            self.declare(var, Value::Int(i));
                            let mut out = Ok(());
                            for s in body {
                                if let Err(f) = self.exec(s) {
                                    out = Err(f);
                                    break;
                                }
                            }
                            self.pop_scope();
                            match out {
                                Ok(()) => {}
                                Err(Flow::Break) => break,
                                Err(Flow::Continue) => continue,
                                Err(other) => return Err(other),
                            }
                        }
                        return Ok(());
                    }
                }
                let it = self.eval(iter)?;
                // `for x in ch:` — receives one at a time until the channel is closed and empty.
                if let (Value::Chan(ch), None) = (&it, var2) {
                    let ch = ch.clone();
                    loop {
                        let item = match self.chan_take(&ch, *line, 1)? {
                            Some(v) => v,
                            None => break,
                        };
                        self.push_scope();
                        self.declare(var, item);
                        let mut out = Ok(());
                        for s in body {
                            if let Err(f) = self.exec(s) {
                                out = Err(f);
                                break;
                            }
                        }
                        self.pop_scope();
                        match out {
                            Ok(()) => {}
                            Err(Flow::Break) => break,
                            Err(Flow::Continue) => continue,
                            Err(other) => return Err(other),
                        }
                    }
                    return Ok(());
                }
                // `for k, v in d:` — binds the two values (key, value) on each iteration.
                let items: Vec<(Value, Option<Value>)> = if var2.is_some() {
                    match &it {
                        Value::Dict(pairs) => pairs
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), Some(v.clone())))
                            .collect(),
                        other => {
                            return fail_fix(
                                "E0216",
                                tr!(format!("`for 키, 값 in ...`은 사전만 되는데 {}입니다", other.type_name()), format!("`for key, value in ...` only works on dictionaries, found {}", other.type_name())),
                                *line,
                                1,
                                tr!("`for k, v in d:` 는 사전에만 씁니다", "`for k, v in d:` is only for dictionaries"),
                            )
                        }
                    }
                } else {
                    match &it {
                        Value::List(v) => v.borrow().iter().map(|x| (x.clone(), None)).collect(),
                        Value::Str(s) => {
                            s.chars().map(|c| (str_value(c.to_string()), None)).collect()
                        }
                        Value::Json(j) => match &*j.borrow() {
                            crate::json::JsonVal::List(xs) => xs.iter().map(|x| (Value::Json(x.clone()), None)).collect(),
                            _ => Vec::new(),
                        },
                        other => {
                            return fail_fix(
                                "E0216",
                                tr!(format!("{} 값은 반복할 수 없습니다", other.type_name()), format!("cannot iterate over a {} value", other.type_name())),
                                *line,
                                1,
                                tr!("리스트나 문자열, `range(n)`, 또는 사전은 `for k, v in d:`", "iterate over a list, a string, `range(n)`, or a dictionary with `for k, v in d:`"),
                            )
                        }
                    }
                };
                for (item, second) in items {
                    self.push_scope();
                    self.declare(var, item);
                    if let (Some(v2), Some(sv)) = (var2, second) {
                        self.declare(v2, sv);
                    }
                    let mut out = Ok(());
                    for s in body {
                        if let Err(f) = self.exec(s) {
                            out = Err(f);
                            break;
                        }
                    }
                    self.pop_scope();
                    match out {
                        Ok(()) => {}
                        Err(Flow::Break) => break,
                        Err(Flow::Continue) => continue,
                        Err(other) => return Err(other),
                    }
                }
                Ok(())
            }

            Stmt::Match { subject, cases, line } => {
                let v = self.eval(subject)?;
                for case in cases {
                    if let Some(binds) = self.match_pattern(&case.pattern, &v, case.line)? {
                        self.push_scope();
                        for (n, bv) in binds {
                            self.declare(&n, bv);
                        }
                        let mut out = Ok(());
                        for s in &case.body {
                            if let Err(f) = self.exec(s) {
                                out = Err(f);
                                break;
                            }
                        }
                        self.pop_scope();
                        return out;
                    }
                }
                // An error that will be promoted to a compile-time exhaustiveness check in P2.
                let hint = match &v {
                    Value::Enum(e) => {
                        let name = e.borrow().enum_name.clone();
                        match self.enums.get(&name) {
                            Some(ed) => {
                                let all: Vec<String> =
                                    ed.variants.iter().map(|x| x.name.clone()).collect();
                                tr!(format!("`{}`의 변형: {}", name, all.join(", ")), format!("variants of `{}`: {}", name, all.join(", ")))
                            }
                            None => tr!("모든 경우를 덮거나 `case _:`를 추가하세요", "cover every case or add `case _:`").to_string(),
                        }
                    }
                    _ => tr!("모든 경우를 덮거나 `case _:`를 추가하세요", "cover every case or add `case _:`").to_string(),
                };
                fail_fix(
                    "E0206",
                    tr!(format!("match가 {}을(를) 처리하지 못했습니다", v.repr()), format!("match did not handle {}", v.repr())),
                    *line,
                    1,
                    hint,
                )
            }

            Stmt::Return(v, _, _) => {
                let val = match v {
                    Some(e) => self.eval(e)?,
                    None => Value::None,
                };
                Err(Flow::Return(val))
            }
            Stmt::Break(_, _) => Err(Flow::Break),
            Stmt::Continue(_, _) => Err(Flow::Continue),
        }
    }

    fn match_pattern(
        &mut self,
        pat: &Pattern,
        v: &Value,
        line: usize,
    ) -> R<Option<Vec<(String, Value)>>> {
        match pat {
            Pattern::Wildcard => Ok(Some(Vec::new())),
            Pattern::Bind(n) => Ok(Some(vec![(n.clone(), v.clone())])),
            Pattern::Literal(e) => {
                let lit = self.eval(e)?;
                Ok(if lit.eq_value(v) { Some(Vec::new()) } else { None })
            }
            Pattern::Variant(name, binds) => match v {
                Value::Enum(ev) => {
                    let ev = ev.borrow();
                    if &ev.variant != name {
                        return Ok(None);
                    }
                    if !binds.is_empty() && binds.len() != ev.fields.len() {
                        return fail_fix(
                            "E0217",
                            tr!(format!(
                                "`{}`은(는) 필드가 {}개인데 {}개를 받으려 합니다",
                                name,
                                ev.fields.len(),
                                binds.len()
                            ), format!(
                                "`{}` has {} fields but the pattern binds {}",
                                name,
                                ev.fields.len(),
                                binds.len()
                            )),
                            line,
                            1,
                            tr!(format!(
                                "`case {}({})` 형태로 쓰세요",
                                name,
                                ev.fields
                                    .iter()
                                    .map(|(n, _)| n.clone())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ), format!(
                                "write it as `case {}({})`",
                                name,
                                ev.fields
                                    .iter()
                                    .map(|(n, _)| n.clone())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )),
                        );
                    }
                    let mut out = Vec::new();
                    for (i, b) in binds.iter().enumerate() {
                        out.push((b.clone(), ev.fields[i].1.clone()));
                    }
                    Ok(Some(out))
                }
                _ => Ok(None),
            },
        }
    }

    // ---------------------------------------------------------------- expressions

    fn eval(&mut self, e: &Expr) -> R<Value> {
        match e {
            Expr::Lambda(f, _, _) => Ok(self.make_closure(f)),
            Expr::Spawn(f, l, c) => Ok(self.spawn_task(f, *l, *c)),
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Float(f) => Ok(Value::Float(*f)),
            Expr::Str(s) => Ok(Value::Str(Rc::new(s.as_ref().clone()))),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::NoneLit => Ok(Value::None),

            Expr::Ident(name, l, c) => {
                if let Some(v) = self.lookup(name) {
                    return Ok(v);
                }
                if PRELUDE.contains(&name.as_str()) {
                    let s: &'static str = PRELUDE.iter().find(|p| **p == name.as_str()).unwrap();
                    return Ok(Value::Builtin(s));
                }
                // Using a top-level function name as a value yields a function value.
                if let Some(f) = self.fns.get(name).cloned() {
                    return Ok(Value::Func(f));
                }
                // Enum variants that carry no value can be constructed by writing them without parentheses.
                if let Some(ename) = self.variant_of.get(name).cloned() {
                    let ed = self.enums.get(&ename).cloned().unwrap();
                    let vd = ed.variants.iter().find(|v| &v.name == name).unwrap().clone();
                    if vd.fields.is_empty() {
                        return self.construct_variant(&ename, &vd, vec![], vec![], *l, *c);
                    }
                    return fail_fix(
                        "E0219",
                        tr!(format!("`{}`은(는) 값을 담는 변형이라 그냥 쓸 수 없습니다", name), format!("`{}` is a variant that carries values and cannot be used bare", name)),
                        *l,
                        *c,
                        tr!(format!("`{}(...)` 처럼 값을 채워서 만드세요", name), format!("construct it with values, like `{}(...)`", name)),
                    );
                }
                if self.structs.contains_key(name) || self.enums.contains_key(name) {
                    return Ok(str_value(name.clone()));
                }
                let near = self.suggest(name);
                fail_fix(
                    "E0218",
                    tr!(format!("`{}`을(를) 찾을 수 없습니다", name), format!("cannot find `{}`", name)),
                    *l,
                    *c,
                    near.unwrap_or_else(|| tr!("선언했는지, 임포트했는지 확인하세요", "check that it is declared or imported").into()),
                )
            }

            Expr::FString(parts) => {
                let mut s = String::new();
                for p in parts {
                    match p {
                        FStrPart::Lit(t) => s.push_str(t),
                        FStrPart::Expr(e, spec) => {
                            let v = self.eval(e)?;
                            if spec.is_empty() {
                                s.push_str(&v.display());
                            } else {
                                s.push_str(&crate::value::format_value(&v, spec));
                            }
                        }
                    }
                }
                Ok(str_value(s))
            }

            Expr::List(items) => {
                let mut out = Vec::new();
                for i in items {
                    out.push(self.eval(i)?);
                }
                Ok(list_value(out))
            }

            Expr::Tuple(items) => {
                let mut out = Vec::new();
                for i in items {
                    out.push(self.eval(i)?);
                }
                Ok(Value::Tuple(Rc::new(out)))
            }

            Expr::Dict(pairs) => {
                let mut out = Vec::new();
                for (k, v) in pairs {
                    out.push((self.eval(k)?, self.eval(v)?));
                }
                Ok(Value::Dict(Rc::new(RefCell::new(out))))
            }

            Expr::Unary(op, inner, l, c) => {
                let v = self.eval(inner)?;
                match (op, v) {
                    (UnOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
                    (UnOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
                    (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                    (UnOp::Not, other) => fail_fix(
                        "E0202",
                        tr!(format!("`not`은 Bool에만 쓸 수 있는데 {}입니다", other.type_name()), format!("`not` only works on Bool, found {}", other.type_name())),
                        *l,
                        *c,
                        tr!("Siskin에는 암묵적 참/거짓 변환이 없습니다", "Siskin has no implicit truthiness"),
                    ),
                    (UnOp::Neg, other) => fail(
                        "E0219",
                        tr!(format!("{} 값에는 단항 `-`를 쓸 수 없습니다", other.type_name()), format!("cannot apply unary `-` to a {} value", other.type_name())),
                        *l,
                        *c,
                    ),
                }
            }

            Expr::Binary(op, a, b, l, c) => {
                // and/or short-circuit.
                if matches!(op, BinOp::And | BinOp::Or) {
                    let left = self.eval(a)?;
                    let lb = match left {
                        Value::Bool(b) => b,
                        other => {
                            return fail_fix(
                                "E0202",
                                tr!(format!("`{}`의 왼쪽은 Bool이어야 하는데 {}입니다", op.symbol(), other.type_name()), format!("left side of `{}` must be Bool, found {}", op.symbol(), other.type_name())),
                                *l,
                                *c,
                                tr!("Siskin에는 암묵적 참/거짓 변환이 없습니다", "Siskin has no implicit truthiness"),
                            )
                        }
                    };
                    if (*op == BinOp::And && !lb) || (*op == BinOp::Or && lb) {
                        return Ok(Value::Bool(lb));
                    }
                    let right = self.eval(b)?;
                    return match right {
                        Value::Bool(rb) => Ok(Value::Bool(rb)),
                        other => fail_fix(
                            "E0202",
                            tr!(format!("`{}`의 오른쪽은 Bool이어야 하는데 {}입니다", op.symbol(), other.type_name()), format!("right side of `{}` must be Bool, found {}", op.symbol(), other.type_name())),
                            *l,
                            *c,
                            tr!("Siskin에는 암묵적 참/거짓 변환이 없습니다", "Siskin has no implicit truthiness"),
                        ),
                    };
                }
                let av = self.eval(a)?;
                let bv = self.eval(b)?;
                self.binary(*op, av, bv, *l, *c)
            }

            Expr::IfExpr { cond, then, els } => {
                let (l, c) = cond.pos();
                match self.eval(cond)? {
                    Value::Bool(true) => self.eval(then),
                    Value::Bool(false) => self.eval(els),
                    other => fail_fix(
                        "E0202",
                        tr!(format!("조건은 Bool이어야 하는데 {}입니다", other.type_name()), format!("condition must be Bool, found {}", other.type_name())),
                        l,
                        c,
                        tr!("Siskin에는 암묵적 참/거짓 변환이 없습니다", "Siskin has no implicit truthiness"),
                    ),
                }
            }

            Expr::Try(inner, _l, _c) => {
                let v = self.eval(inner)?;
                if let Value::Error(_) | Value::ErrorOf(_) = v {
                    // Propagate the error out of the current function.
                    return Err(Flow::Return(v));
                }
                Ok(v)
            }

            // `a else default` — yields default if a is none.
            Expr::OrElse(a, b, _, _) => {
                let v = self.eval(a)?;
                if let Value::None = v {
                    self.eval(b)
                } else {
                    Ok(v)
                }
            }

            Expr::Index(obj, idx, l, c) => {
                let o = self.eval(obj)?;
                let i = self.eval(idx)?;
                if let (Value::Raw(b, off), Value::Int(n)) = (&o, &i) {
                    let bb = b.borrow();
                    check_raw(&bb, *off, *n, *l, *c)?;
                    return Ok(bb.data[(*off as i64 + *n) as usize].clone());
                }
                match (&o, &i) {
                    (Value::List(items), Value::Int(n)) => {
                        let items = items.borrow();
                        let len = items.len() as i64;
                        if *n < 0 || *n >= len {
                            return fail_fix(
                                "E0204",
                                tr!(format!("인덱스 {}이(가) 범위를 벗어납니다 (길이 {})", n, len), format!("index {} out of range (length {})", n, len)),
                                *l,
                                *c,
                                tr!("인덱스는 0부터 길이-1까지입니다", "valid indices run from 0 to length-1"),
                            );
                        }
                        Ok(items[*n as usize].clone())
                    }
                    (Value::Str(s), Value::Int(n)) => {
                        let chars: Vec<char> = s.chars().collect();
                        let len = chars.len() as i64;
                        if *n < 0 || *n >= len {
                            return fail_fix(
                                "E0204",
                                tr!(format!("인덱스 {}이(가) 범위를 벗어납니다 (길이 {})", n, len), format!("index {} out of range (length {})", n, len)),
                                *l,
                                *c,
                                tr!("인덱스는 0부터 길이-1까지입니다", "valid indices run from 0 to length-1"),
                            );
                        }
                        Ok(str_value(chars[*n as usize].to_string()))
                    }
                    (Value::Dict(pairs), key) => {
                        for (k, v) in pairs.borrow().iter() {
                            if k.eq_value(key) {
                                return Ok(v.clone());
                            }
                        }
                        Ok(Value::None)
                    }
                    _ => fail(
                        "E0220",
                        tr!(format!("{}에는 {}로 인덱싱할 수 없습니다", o.type_name(), i.type_name()), format!("cannot index {} with {}", o.type_name(), i.type_name())),
                        *l,
                        *c,
                    ),
                }
            }

            Expr::Field(obj, name, l, c) => {
                let o = self.eval(obj)?;
                match &o {
                    Value::Struct(s) => {
                        let sv = s.borrow();
                        if let Some((_, v)) = sv.fields.iter().find(|(n, _)| n == name) {
                            return Ok(v.clone());
                        }
                        let all: Vec<String> = sv.fields.iter().map(|(n, _)| n.clone()).collect();
                        fail_fix(
                            "E0213",
                            tr!(format!("`{}`에 `{}` 필드가 없습니다", sv.name, name), format!("`{}` has no field `{}`", sv.name, name)),
                            *l,
                            *c,
                            tr!(format!("있는 필드: {}", all.join(", ")), format!("available fields: {}", all.join(", "))),
                        )
                    }
                    Value::Tuple(items) if name.chars().all(|ch| ch.is_ascii_digit()) => {
                        let i: usize = name.parse().unwrap_or(usize::MAX);
                        match items.get(i) {
                            Some(v) => Ok(v.clone()),
                            None => fail("E0213", tr!(format!("튜플에 `.{}` 가 없습니다", name), format!("tuple has no `.{}`", name)), *l, *c),
                        }
                    }
                    Value::Enum(e) => {
                        let ev = e.borrow();
                        if let Some((_, v)) = ev.fields.iter().find(|(n, _)| n == name) {
                            return Ok(v.clone());
                        }
                        fail("E0213", tr!(format!("`{}`에 `{}` 필드가 없습니다", ev.variant, name), format!("`{}` has no field `{}`", ev.variant, name)), *l, *c)
                    }
                    _ => fail_fix(
                        "E0221",
                        tr!(format!("{} 값에서 `{}`을(를) 꺼낼 수 없습니다", o.type_name(), name), format!("cannot access `{1}` on a {0} value", o.type_name(), name)),
                        *l,
                        *c,
                        tr!("메서드로 호출하려 했다면 괄호를 붙이세요", "if you meant to call a method, add parentheses"),
                    ),
                }
            }

            Expr::Call { callee, targs, args, line, col } => {
                self.eval_call(callee, targs, args, *line, *col)
            }
        }
    }

    fn suggest(&self, name: &str) -> Option<String> {
        let mut candidates: Vec<String> = Vec::new();
        candidates.extend(self.fns.keys().cloned());
        candidates.extend(self.structs.keys().cloned());
        candidates.extend(PRELUDE.iter().map(|s| s.to_string()));
        candidates.extend(self.globals.keys().cloned());
        for s in self.frames.last().unwrap().scopes.iter() {
            candidates.extend(s.keys().cloned());
        }
        let lower = name.to_lowercase();
        let hit = candidates.into_iter().find(|c| {
            c.to_lowercase() == lower
                || (c.len() >= 3 && name.len() >= 3 && c.to_lowercase().starts_with(&lower[..3.min(lower.len())]))
        })?;
        Some(tr!(format!("`{}` 말씀이신가요?", hit), format!("did you mean `{}`?", hit)))
    }

    // -------------------------------------------------------------- function calls

    fn eval_call(&mut self, callee: &Expr, targs: &[TypeExpr], args: &[Arg], line: usize, col: usize) -> R<Value> {
        // Method call: obj.method(...)
        if let Expr::Field(obj, mname, fl, fc) = callee {
            // Module function: math.sqrt(...)
            if let Expr::Ident(modname, _, _) = obj.as_ref() {
                if let Some(Value::Module(m)) = self.lookup(modname) {
                    // Calling a standard function written in Siskin via its module name, like `time.today()`
                    if let Some(f) = self.fns.get(mname.as_str()).cloned() {
                        let (pos, named) = self.eval_args(args)?;
                        return self.call_fn(&f, pos, named, *fl, *fc);
                    }
                    let full = MODULES
                        .iter()
                        .find(|(name, _)| *name == m)
                        .and_then(|(_, fns)| fns.iter().find(|f| **f == mname.as_str()))
                        .copied();
                    return match full {
                        Some(f) => {
                            let a = self.eval_args(args)?;
                            self.call_builtin(f, a.0, a.1, line, col)
                        }
                        None => fail(
                            "E0210",
                            tr!(format!("`std.{}`에 `{}`이(가) 없습니다", m, mname), format!("`std.{}` has no `{}`", m, mname)),
                            *fl,
                            *fc,
                        ),
                    };
                }
            }
            let recv = self.eval(obj)?;
            let (pos, named) = self.eval_args(args)?;
            // If there is no method of that name, call a function-typed field: `self.on_click(x)`
            if let Value::Struct(sv) = &recv {
                let sname = sv.borrow().name.clone();
                let has_method = self
                    .structs
                    .get(&sname)
                    .map_or(false, |sd| sd.methods.iter().any(|m| &m.name == mname));
                if !has_method {
                    let fv = sv.borrow().fields.iter().find(|(n, _)| n == mname).map(|(_, v)| v.clone());
                    match fv {
                        Some(Value::Func(f)) => return self.call_fn(&f, pos, named, *fl, *fc),
                        Some(Value::Closure(cl)) => return self.call_closure(&cl, pos, named, *fl, *fc),
                        _ => {}
                    }
                }
            }
            if let Value::Arena(chunks) = &recv {
                let elem = targs.first();
                return match mname.as_str() {
                    // A list created by an arena. Same semantics as a regular list; only the time of freeing differs.
                    "list" => Ok(Value::List(Rc::new(RefCell::new(Vec::new())))),
                    "alloc" => {
                        let n = match pos.first() {
                            Some(Value::Int(n)) if *n >= 0 => *n as usize,
                            _ => {
                                return fail(
                                    "E0230",
                                    tr!("`a.alloc[T](개수)` 에는 0 이상의 Int가 필요합니다", "`a.alloc[T](count)` needs a non-negative Int"),
                                    *fl,
                                    *fc,
                                )
                            }
                        };
                        let zero = zero_of(elem);
                        let buf = Rc::new(RefCell::new(RawBuf {
                            data: vec![zero; n],
                            alive: true,
                            in_arena: true,
                        }));
                        chunks.borrow_mut().push(Rc::clone(&buf));
                        Ok(Value::Raw(buf, 0))
                    }
                    other => fail_fix(
                        "E0231",
                        tr!(format!("아레나에 `{}` 메서드가 없습니다", other), format!("arena has no method `{}`", other)),
                        *fl,
                        *fc,
                        tr!("쓸 수 있는 것: `a.list[T]()`, `a.alloc[T](개수)`", "available: `a.list[T]()`, `a.alloc[T](count)`"),
                    ),
                };
            }
            let r = self.call_method(recv, mname, pos, named, *fl, *fc)?;
            self.writeback_inout(args)?;
            return Ok(r);
        }

        // Call by name
        if let Expr::Ident(name, l, c) = callee {
            // Memory Level 2 built-ins
            match name.as_str() {
                "cstr" | "ptr_get" => {
                    return fail_fix(
                        "E0231",
                        tr!("C 라이브러리에서 받은 주소를 읽는 기능입니다", "this reads an address received from a C library"),
                        *l,
                        *c,
                        tr!("`import c \"헤더.h\"` 로 가져온 함수와 함께 쓰세요", "use it with functions imported via `import c \"header.h\"`"),
                    );
                }
                "alloc" => {
                    let (pos, _) = self.eval_args(args)?;
                    let n = match pos.first() {
                        Some(Value::Int(n)) if *n >= 0 => *n as usize,
                        _ => return fail("E0230", tr!("`alloc[T](개수)` 에는 0 이상의 Int가 필요합니다", "`alloc[T](count)` needs a non-negative Int"), *l, *c),
                    };
                    let zero = zero_of(targs.first());
                    return Ok(Value::Raw(
                        Rc::new(RefCell::new(RawBuf { data: vec![zero; n], alive: true, in_arena: false })),
                        0,
                    ));
                }
                "free" => {
                    let (pos, _) = self.eval_args(args)?;
                    return match pos.first() {
                        Some(Value::Raw(b, off)) => {
                            if *off != 0 {
                                return fail_fix(
                                    "E0232",
                                    tr!("포인터 중간을 해제할 수 없습니다", "cannot free a pointer into the middle of an allocation"),
                                    *l,
                                    *c,
                                    tr!("`alloc`이 돌려준 처음 포인터를 그대로 `free`에 넘기세요", "pass the exact pointer returned by `alloc` to `free`"),
                                );
                            }
                            let mut bm = b.borrow_mut();
                            if bm.in_arena {
                                return fail_fix(
                                    "E0233",
                                    tr!("아레나 메모리는 따로 해제하지 않습니다", "arena memory is not freed individually"),
                                    *l,
                                    *c,
                                    tr!("`with arena` 블록이 끝날 때 한 번에 해제됩니다", "it is all released when the `with arena` block ends"),
                                );
                            }
                            if !bm.alive {
                                return fail_fix(
                                    "E0234",
                                    tr!("이미 해제한 메모리를 또 해제했습니다", "double free: this memory was already freed"),
                                    *l,
                                    *c,
                                    tr!("`free`는 한 번만 부릅니다", "call `free` only once"),
                                );
                            }
                            bm.alive = false;
                            Ok(Value::None)
                        }
                        _ => fail("E0235", tr!("`free`는 원시 포인터를 받습니다", "`free` takes a raw pointer"), *l, *c),
                    };
                }
                _ => {}
            }
            // Calling a function value (a function received as a local variable or argument)
            if let Some(Value::Func(f)) = self.lookup(name) {
                let (pos, named) = self.eval_args(args)?;
                let r = self.call_fn(&f, pos, named, *l, *c)?;
                self.writeback_inout(args)?;
                return Ok(r);
            }
            if let Some(Value::Closure(cl)) = self.lookup(name) {
                let (pos, named) = self.eval_args(args)?;
                return self.call_closure(&cl, pos, named, *l, *c);
            }
            // User function
            if let Some(f) = self.fns.get(name).cloned() {
                let (pos, named) = self.eval_args(args)?;
                let r = self.call_fn(&f, pos, named, *l, *c)?;
                self.writeback_inout(args)?;
                return Ok(r);
            }
            // Struct construction
            if let Some(sd) = self.structs.get(name).cloned() {
                let (pos, named) = self.eval_args(args)?;
                return self.construct_struct(&sd, pos, named, *l, *c);
            }
            // Enum variant construction
            if let Some(ename) = self.variant_of.get(name).cloned() {
                let ed = self.enums.get(&ename).cloned().unwrap();
                let vd = ed.variants.iter().find(|v| &v.name == name).unwrap().clone();
                let (pos, named) = self.eval_args(args)?;
                return self.construct_variant(&ename, &vd, pos, named, *l, *c);
            }
            // Built-in bound in local scope (imported)
            if let Some(Value::Builtin(b)) = self.lookup(name) {
                let (pos, named) = self.eval_args(args)?;
                return self.call_builtin(b, pos, named, *l, *c);
            }
            // Prelude
            if let Some(b) = PRELUDE.iter().find(|p| **p == name.as_str()) {
                let (pos, named) = self.eval_args(args)?;
                return self.call_builtin(b, pos, named, *l, *c);
            }
            let near = self.suggest(name);
            return fail_fix(
                "E0222",
                tr!(format!("함수 `{}`을(를) 찾을 수 없습니다", name), format!("cannot find function `{}`", name)),
                *l,
                *c,
                near.unwrap_or_else(|| tr!("선언했는지, 임포트했는지 확인하세요", "check that it is declared or imported").into()),
            );
        }

        // Calling an expression that returns a function directly: `get()(x)`, `table[0](x)`, etc.
        let cv = self.eval(callee)?;
        match cv {
            Value::Func(f) => {
                let (pos, named) = self.eval_args(args)?;
                let r = self.call_fn(&f, pos, named, line, col)?;
                self.writeback_inout(args)?;
                return Ok(r);
            }
            Value::Closure(cl) => {
                let (pos, named) = self.eval_args(args)?;
                return self.call_closure(&cl, pos, named, line, col);
            }
            Value::Builtin(b) => {
                let (pos, named) = self.eval_args(args)?;
                return self.call_builtin(b, pos, named, line, col);
            }
            _ => {}
        }
        fail("E0223", tr!("호출할 수 없는 대상입니다", "this value is not callable"), line, col)
    }

    fn eval_args(&mut self, args: &[Arg]) -> R<(Vec<Value>, Vec<(String, Value)>)> {
        let mut pos = Vec::new();
        let mut named = Vec::new();
        for a in args {
            let v = self.eval(&a.value)?;
            match &a.name {
                Some(n) => named.push((n.clone(), v)),
                None => pos.push(v),
            }
        }
        Ok((pos, named))
    }

    fn call_fn(
        &mut self,
        f: &crate::ast::Shared<FnDecl>,
        pos: Vec<Value>,
        named: Vec<(String, Value)>,
        line: usize,
        col: usize,
    ) -> R<Value> {
        if f.is_extern {
            return fail_fix(
                "E0240",
                tr!(format!("`{}`은(는) C 라이브러리 함수라 `siskin run`에서는 부를 수 없습니다", f.name), format!("`{}` is a C library function and cannot be called under `siskin run`", f.name)),
                line,
                col,
                tr!("`siskin build`로 컴파일한 뒤 실행하세요. C 함수는 실제로 링크되어야 동작합니다", "compile with `siskin build` and run the result; C functions only work when actually linked"),
            );
        }
        self.call_fn_with_self(f, None, pos, named, line, col)
    }

    fn call_fn_with_self(
        &mut self,
        f: &crate::ast::Shared<FnDecl>,
        recv: Option<Value>,
        pos: Vec<Value>,
        named: Vec<(String, Value)>,
        line: usize,
        col: usize,
    ) -> R<Value> {
        self.depth += 1;
        if self.depth > 2000 {
            self.depth -= 1;
            return fail_fix(
                "E0224",
                tr!("재귀가 너무 깊습니다", "recursion too deep"),
                f.line,
                1,
                tr!("종료 조건이 있는지 확인하세요 (P1 인터프리터의 한계는 2000단계입니다)", "check that the recursion has a base case (the P1 interpreter's limit is 2000 levels)"),
            );
        }

        let mut scope: HashMap<String, Value> = HashMap::new();
        let params: Vec<&Param> = f.params.iter().collect();
        let mut idx = 0usize;

        for p in &params {
            if p.is_self {
                match &recv {
                    Some(r) => {
                        scope.insert("self".into(), r.clone());
                    }
                    None => {
                        self.depth -= 1;
                        return fail("E0225", tr!(format!("`{}`은(는) 메서드입니다", f.name), format!("`{}` is a method", f.name)), line, col);
                    }
                }
                continue;
            }
            if let Some((_, v)) = named.iter().find(|(n, _)| n == &p.name) {
                let v = if p.conv == Convention::Owned { v.deep_clone() } else { v.clone() };
                scope.insert(p.name.clone(), v);
                continue;
            }
            if idx < pos.len() {
                let v = &pos[idx];
                let v = if p.conv == Convention::Owned { v.deep_clone() } else { v.clone() };
                scope.insert(p.name.clone(), v);
                idx += 1;
                continue;
            }
            self.depth -= 1;
            let expected: Vec<String> =
                params.iter().filter(|q| !q.is_self).map(|q| q.name.clone()).collect();
            return fail_fix(
                "E0226",
                tr!(format!("`{}`의 인자 `{}`이(가) 빠졌습니다", f.name, p.name), format!("missing argument `{1}` for `{0}`", f.name, p.name)),
                line,
                col,
                tr!(format!("필요한 인자: {}", expected.join(", ")), format!("required arguments: {}", expected.join(", "))),
            );
        }

        let want = params.iter().filter(|p| !p.is_self).count();
        if pos.len() > want {
            self.depth -= 1;
            return fail(
                "E0227",
                tr!(format!("`{}`은(는) 인자 {}개를 받는데 {}개를 받았습니다", f.name, want, pos.len()), format!("`{}` takes {} arguments but {} were given", f.name, want, pos.len())),
                line,
                col,
            );
        }
        for (n, _) in &named {
            if !params.iter().any(|p| &p.name == n) {
                self.depth -= 1;
                let expected: Vec<String> =
                    params.iter().filter(|q| !q.is_self).map(|q| q.name.clone()).collect();
                return fail_fix(
                    "E0228",
                    tr!(format!("`{}`에 `{}`이라는 인자가 없습니다", f.name, n), format!("`{}` has no parameter named `{}`", f.name, n)),
                    line,
                    col,
                    tr!(format!("받는 인자: {}", expected.join(", ")), format!("parameters: {}", expected.join(", "))),
                );
            }
        }

        // For a closure, lay the captured values and its own name (for recursion) in a scope outside the arguments.
        let mut scopes = Vec::new();
        if let Some(cl) = self.pending_env.take() {
            let mut env: HashMap<String, Value> = HashMap::new();
            for (n, v) in &cl.env {
                env.insert(n.clone(), v.clone());
            }
            if !cl.decl.is_lambda() {
                env.insert(cl.decl.name.clone(), Value::Closure(cl.clone()));
            }
            scopes.push(env);
        }
        scopes.push(scope);
        self.frames.push(Frame { scopes, func: f.name.clone(), cur_line: f.line });

        // Preconditions (design doc §9.5)
        if self.contracts {
            for r in &f.requires {
                match self.eval(r) {
                    Ok(Value::Bool(true)) => {}
                    Ok(Value::Bool(false)) => {
                        self.frames.pop();
                        self.depth -= 1;
                        return fail_fix(
                            "E0229",
                            tr!(format!("`{}`의 사전 조건이 깨졌습니다: requires {}", f.name, render_expr(r)), format!("precondition of `{}` violated: requires {}", f.name, render_expr(r))),
                            f.line,
                            1,
                            tr!("호출하는 쪽에서 이 조건을 먼저 확인하세요", "check this condition at the call site first"),
                        );
                    }
                    Ok(other) => {
                        self.frames.pop();
                        self.depth -= 1;
                        return fail(
                            "E0230",
                            tr!(format!("requires는 Bool이어야 하는데 {}입니다", other.type_name()), format!("`requires` must be Bool, found {}", other.type_name())),
                            f.line,
                            1,
                        );
                    }
                    Err(e) => {
                        self.frames.pop();
                        self.depth -= 1;
                        return Err(e);
                    }
                }
            }
        }

        let mut result = Value::None;
        let mut flow_err = None;
        for s in &f.body {
            match self.exec(s) {
                Ok(()) => {}
                Err(Flow::Return(v)) => {
                    result = v;
                    break;
                }
                Err(other) => {
                    flow_err = Some(other);
                    break;
                }
            }
        }

        if let Some(e) = flow_err {
            self.frames.pop();
            self.depth -= 1;
            return Err(e);
        }
        // When an enum error reaches a string-error function (`!T`) via `try`, convert it to a string (same as native).
        if let (Value::ErrorOf(v), Some(TypeExpr::Fallible(_, None))) = (&result, &f.ret) {
            result = Value::Error(Rc::new(v.repr()));
        }

        // Postconditions. `result` refers to the return value.
        if self.contracts && !f.ensures.is_empty() {
            self.push_scope();
            self.declare("result", result.clone());
            for en in &f.ensures {
                match self.eval(en) {
                    Ok(Value::Bool(true)) => {}
                    Ok(Value::Bool(false)) => {
                        self.pop_scope();
                        self.frames.pop();
                        self.depth -= 1;
                        return fail_fix(
                            "E0231",
                            tr!(format!("`{}`의 사후 조건이 깨졌습니다: ensures {}", f.name, render_expr(en)), format!("postcondition of `{}` violated: ensures {}", f.name, render_expr(en))),
                            f.line,
                            1,
                            tr!("함수 본문이 약속한 결과를 내지 못했습니다", "the function body did not produce the promised result"),
                        );
                    }
                    Ok(other) => {
                        self.pop_scope();
                        self.frames.pop();
                        self.depth -= 1;
                        return fail(
                            "E0230",
                            tr!(format!("ensures는 Bool이어야 하는데 {}입니다", other.type_name()), format!("`ensures` must be Bool, found {}", other.type_name())),
                            f.line,
                            1,
                        );
                    }
                    Err(e) => {
                        self.pop_scope();
                        self.frames.pop();
                        self.depth -= 1;
                        return Err(e);
                    }
                }
            }
            self.pop_scope();
        }

        // Collect the final values of `inout` arguments. Lists, structs, etc. are already shared, but
        // scalars (Int/Float/Bool/Str) are passed by value and must be written back to the caller.
        self.inout_out.clear();
        if let Some(frame) = self.frames.last() {
            let mut ni = 0usize;
            for p in &f.params {
                if p.is_self {
                    continue;
                }
                let this = ni;
                ni += 1;
                if p.conv != Convention::Inout {
                    continue;
                }
                if let Some(v) = frame.scopes.iter().rev().find_map(|s| s.get(&p.name)) {
                    self.inout_out.push((this, p.name.clone(), v.clone()));
                }
            }
        }

        self.frames.pop();
        self.depth -= 1;
        Ok(result)
    }

    fn construct_struct(
        &mut self,
        sd: &crate::ast::Shared<StructDecl>,
        pos: Vec<Value>,
        named: Vec<(String, Value)>,
        line: usize,
        col: usize,
    ) -> R<Value> {
        let mut fields: Vec<(String, Value)> = Vec::new();
        for (i, f) in sd.fields.iter().enumerate() {
            if let Some((_, v)) = named.iter().find(|(n, _)| n == &f.name) {
                fields.push((f.name.clone(), v.deep_clone()));
                continue;
            }
            if i < pos.len() {
                fields.push((f.name.clone(), pos[i].deep_clone()));
                continue;
            }
            if let Some(d) = &f.default {
                let v = self.eval(d)?;
                fields.push((f.name.clone(), v));
                continue;
            }
            let all: Vec<String> = sd.fields.iter().map(|x| x.name.clone()).collect();
            return fail_fix(
                "E0232",
                tr!(format!("`{}`의 필드 `{}`이(가) 빠졌습니다", sd.name, f.name), format!("missing field `{1}` for `{0}`", sd.name, f.name)),
                line,
                col,
                tr!(format!("필요한 필드: {}", all.join(", ")), format!("required fields: {}", all.join(", "))),
            );
        }
        for (n, _) in &named {
            if !sd.fields.iter().any(|f| &f.name == n) {
                let all: Vec<String> = sd.fields.iter().map(|x| x.name.clone()).collect();
                return fail_fix(
                    "E0233",
                    tr!(format!("`{}`에 `{}` 필드가 없습니다", sd.name, n), format!("`{}` has no field `{}`", sd.name, n)),
                    line,
                    col,
                    tr!(format!("있는 필드: {}", all.join(", ")), format!("available fields: {}", all.join(", "))),
                );
            }
        }
        Ok(Value::Struct(Rc::new(RefCell::new(StructVal {
            name: sd.name.clone(),
            fields,
        }))))
    }

    fn construct_variant(
        &mut self,
        ename: &str,
        vd: &VariantDecl,
        pos: Vec<Value>,
        named: Vec<(String, Value)>,
        line: usize,
        col: usize,
    ) -> R<Value> {
        let mut fields: Vec<(String, Value)> = Vec::new();
        for (i, f) in vd.fields.iter().enumerate() {
            if let Some((_, v)) = named.iter().find(|(n, _)| n == &f.name) {
                fields.push((f.name.clone(), v.deep_clone()));
                continue;
            }
            if i < pos.len() {
                fields.push((f.name.clone(), pos[i].deep_clone()));
                continue;
            }
            let all: Vec<String> = vd.fields.iter().map(|x| x.name.clone()).collect();
            return fail_fix(
                "E0234",
                tr!(format!("`{}`의 필드 `{}`이(가) 빠졌습니다", vd.name, f.name), format!("missing field `{1}` for `{0}`", vd.name, f.name)),
                line,
                col,
                tr!(format!("필요한 필드: {}", all.join(", ")), format!("required fields: {}", all.join(", "))),
            );
        }
        Ok(Value::Enum(Rc::new(RefCell::new(EnumVal {
            enum_name: ename.to_string(),
            variant: vd.name.clone(),
            fields,
        }))))
    }

    fn call_method(
        &mut self,
        recv: Value,
        name: &str,
        pos: Vec<Value>,
        named: Vec<(String, Value)>,
        line: usize,
        col: usize,
    ) -> R<Value> {
        // Built-in methods need no inout write-back. Clear it first so a value from a previous call
        // isn't wrongly written back. (It is refilled for user methods.)
        self.inout_out.clear();
        // User-defined methods take precedence.
        let decl = match &recv {
            Value::Struct(s) => {
                let sname = s.borrow().name.clone();
                self.structs
                    .get(&sname)
                    .and_then(|sd| sd.methods.iter().find(|m| m.name == name).cloned())
            }
            Value::Enum(e) => {
                let ename = e.borrow().enum_name.clone();
                self.enums
                    .get(&ename)
                    .and_then(|ed| ed.methods.iter().find(|m| m.name == name).cloned())
            }
            _ => None,
        };
        if let Some(f) = decl {
            return self.call_fn_with_self(&f, Some(recv), pos, named, line, col);
        }
        if let Value::List(items) = &recv {
            if matches!(name, "map" | "filter" | "any" | "all" | "sort_by") {
                let f = pos.first().cloned().unwrap_or(Value::None);
                let xs: Vec<Value> = items.borrow().clone();
                return match name {
                    "map" => {
                        let mut out = Vec::new();
                        for x in xs {
                            out.push(self.call_value(&f, vec![x], line, col)?.deep_clone());
                        }
                        Ok(list_value(out))
                    }
                    "filter" => {
                        let mut out = Vec::new();
                        for x in xs {
                            if let Value::Bool(true) = self.call_value(&f, vec![x.clone()], line, col)? {
                                out.push(x.deep_clone());
                            }
                        }
                        Ok(list_value(out))
                    }
                    "any" | "all" => {
                        let want = name == "any";
                        for x in xs {
                            if let Value::Bool(b) = self.call_value(&f, vec![x], line, col)? {
                                if b == want {
                                    return Ok(Value::Bool(want));
                                }
                            }
                        }
                        Ok(Value::Bool(!want))
                    }
                    _ => {
                        // Compute all keys first, then sort stably (equal keys keep their original order).
                        let mut keyed = Vec::new();
                        for x in xs {
                            let k = self.call_value(&f, vec![x.clone()], line, col)?;
                            keyed.push((k, x));
                        }
                        keyed.sort_by(|(a, _), (b, _)| match (a, b) {
                            (Value::Int(x), Value::Int(y)) => x.cmp(y),
                            (Value::Float(x), Value::Float(y)) => {
                                x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
                            }
                            (Value::Str(x), Value::Str(y)) => x.as_bytes().cmp(y.as_bytes()),
                            _ => std::cmp::Ordering::Equal,
                        });
                        *items.borrow_mut() = keyed.into_iter().map(|(_, x)| x).collect();
                        Ok(Value::None)
                    }
                };
            }
        }
        self.builtin_method(recv, name, pos, line, col)
    }

    /// Calls a function value (a named function, closure or built-in).
    /// `spawn f(x)`: runs a closure with copied captured values in a new task.
    /// The new task gets its own interpreter (sharing the function table), and its random seed comes from
    /// a value drawn once from the parent (same rule as native).
    fn spawn_task(&mut self, f: &crate::ast::Shared<FnDecl>, line: usize, col: usize) -> Value {
        // Tasks truly run concurrently. Every value handed to the new thread is rebuilt from scratch (detach)
        // so it shares no `Rc` with this thread. Declarations (fns …) are `Arc`, so they are read shared.
        let clo = crate::value::Moved::new(&self.make_closure(f));
        let seed = mi_next_rand(&mut self.rng);
        let mut child = Interp::new();
        child.fns = self.fns.clone();
        child.structs = self.structs.clone();
        child.enums = self.enums.clone();
        child.variant_of = self.variant_of.clone();
        child.globals = self.globals.iter().map(|(k, v)| (k.clone(), v.detach())).collect();
        child.contracts = self.contracts;
        child.prog_args = self.prog_args.clone();
        child.rng = if seed == 0 { 0x853C49E6748FEA9B } else { seed };
        let cell = std::sync::Arc::new(crate::value::TaskCell {
            done: std::sync::atomic::AtomicBool::new(false),
            result: std::sync::Mutex::new(None),
        });
        let out = cell.clone();
        let files = crate::error::files_snapshot();
        crate::conc::start(Box::new(move || {
            crate::error::files_restore(files);
            let mut child = child;
            let clo = clo.take();
            let r = child.call_value(&clo, Vec::new(), line, col);
            let res = match r {
                Ok(v) => Some(crate::value::Moved::new(&v)),
                Err(Flow::Fail(e)) => crate::conc::report_error(&e),
                Err(_) => None,
            };
            // Signal completion only after releasing all of this task's values.
            drop(clo);
            drop(child);
            *out.result.lock().unwrap_or_else(|e| e.into_inner()) = res;
            crate::conc::change(|| out.done.store(true, std::sync::atomic::Ordering::SeqCst));
        }));
        Value::Task(cell)
    }

    /// Receives one item from a channel. None if it is closed and empty.
    fn chan_take(&mut self, ch: &crate::value::ChanCell, line: usize, col: usize) -> R<Option<Value>> {
        let r = crate::conc::wait_then(
            |_| {
                let st = ch.st();
                !st.q.is_empty() || st.closed
            },
            || ch.st().q.pop_front(),
        );
        match r {
            Ok(v) => Ok(v.map(|m| m.take())),
            Err(()) => self.deadlock(line, col),
        }
    }

    fn deadlock<T>(&mut self, line: usize, col: usize) -> R<T> {
        fail_fix("E0260", crate::conc::deadlock_msg(), line, col, crate::conc::deadlock_help())
    }

    fn call_value(&mut self, f: &Value, args: Vec<Value>, line: usize, col: usize) -> R<Value> {
        match f {
            Value::Func(fd) => self.call_fn(fd, args, Vec::new(), line, col),
            Value::Closure(c) => self.call_closure(c, args, Vec::new(), line, col),
            Value::Builtin(b) => self.call_builtin(b, args, Vec::new(), line, col),
            _ => fail("E0223", tr!("호출할 수 없는 대상입니다", "this value is not callable"), line, col),
        }
    }

    fn builtin_method(
        &mut self,
        recv: Value,
        name: &str,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> R<Value> {
        match (&recv, name) {
            (Value::Task(t), "wait") => {
                use std::sync::atomic::Ordering;
                let t = t.clone();
                if crate::conc::wait_until(|_| t.done.load(Ordering::SeqCst)).is_err() {
                    return self.deadlock(line, col);
                }
                // Build a fresh result each time (different values even when waited on multiple times or by multiple tasks).
                let r = t.result.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|m| m.copy());
                Ok(r.unwrap_or(Value::None))
            }
            (Value::Task(t), "done") => Ok(Value::Bool(t.done.load(std::sync::atomic::Ordering::SeqCst))),
            (Value::Chan(ch), "send") => {
                let ch = ch.clone();
                let v = crate::value::Moved::new(args.first().unwrap_or(&Value::None));
                        let r = crate::conc::wait_then(
                    |_| {
                        let st = ch.st();
                        st.closed || ch.cap == 0 || st.q.len() < ch.cap
                    },
                    || {
                        let mut st = ch.st();
                        if st.closed {
                            false
                        } else {
                            st.q.push_back(v);
                            true
                        }
                    },
                );
                let sent = match r {
                    Ok(b) => b,
                    Err(()) => return self.deadlock(line, col),
                };
                if !sent {
                    return fail_fix(
                        "E0261",
                        tr!("닫힌 통로에 보냈습니다", "send on a closed channel"),
                        line,
                        col,
                        tr!("`close()` 는 보내는 쪽이 다 보낸 뒤에 한 번 부릅니다", "call `close()` once, after the sender has sent everything"),
                    );
                }
                Ok(Value::None)
            }
            (Value::Chan(ch), "recv") => {
                let ch = ch.clone();
                Ok(self.chan_take(&ch, line, col)?.unwrap_or(Value::None))
            }
            (Value::Chan(ch), "close") => {
                crate::conc::change(|| ch.st().closed = true);
                Ok(Value::None)
            }
            (Value::List(items), "len") => Ok(Value::Int(items.borrow().len() as i64)),
            (Value::List(items), "push") => {
                let v = args.first().cloned().unwrap_or(Value::None);
                items.borrow_mut().push(v.deep_clone());
                Ok(Value::None)
            }
            (Value::List(items), "pop") => {
                let mut b = items.borrow_mut();
                match b.pop() {
                    Some(v) => Ok(v),
                    None => Ok(Value::None),
                }
            }
            (Value::List(items), "reverse") => {
                items.borrow_mut().reverse();
                Ok(Value::None)
            }
            (Value::List(items), "contains") => {
                let target = args.first().cloned().unwrap_or(Value::None);
                Ok(Value::Bool(items.borrow().iter().any(|v| v.eq_value(&target))))
            }
            (Value::List(items), "join") => {
                let sep = args.first().map(|v| v.display()).unwrap_or_default();
                let parts: Vec<String> = items.borrow().iter().map(|v| v.display()).collect();
                Ok(str_value(parts.join(&sep)))
            }

            (Value::Str(s), "len") => Ok(Value::Int(s.chars().count() as i64)),
            (Value::Str(s), "split") => {
                let sep = args.first().map(|v| v.display()).unwrap_or_else(|| " ".into());
                let parts: Vec<Value> = s.split(sep.as_str()).map(str_value).collect();
                Ok(list_value(parts))
            }
            (Value::Str(s), "upper") => Ok(str_value(s.to_uppercase())),
            (Value::Str(s), "lower") => Ok(str_value(s.to_lowercase())),
            (Value::Str(s), "strip") => Ok(str_value(s.trim().to_string())),
            (Value::Str(s), "contains") => {
                let t = args.first().map(|v| v.display()).unwrap_or_default();
                Ok(Value::Bool(s.contains(t.as_str())))
            }
            (Value::Str(s), "starts_with") => {
                let t = args.first().map(|v| v.display()).unwrap_or_default();
                Ok(Value::Bool(s.starts_with(t.as_str())))
            }
            (Value::Str(s), "ends_with") => {
                let t = args.first().map(|v| v.display()).unwrap_or_default();
                Ok(Value::Bool(s.ends_with(t.as_str())))
            }
            (Value::Str(s), "replace") => {
                let a = args.first().map(|v| v.display()).unwrap_or_default();
                let b = args.get(1).map(|v| v.display()).unwrap_or_default();
                Ok(str_value(s.replace(a.as_str(), b.as_str())))
            }

            (Value::List(items), "sort") => {
                let mut v = items.borrow_mut();
                let mut err: Option<String> = None;
                v.sort_by(|a, b| match (a, b) {
                    (Value::Int(x), Value::Int(y)) => x.cmp(y),
                    (Value::Float(x), Value::Float(y)) => {
                        x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    (Value::Str(x), Value::Str(y)) => x.cmp(y),
                    (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
                    (x, _) => {
                        err = Some(x.type_name());
                        std::cmp::Ordering::Equal
                    }
                });
                match err {
                    Some(t) => fail_fix(
                        "E0245",
                        tr!(format!("{} 리스트는 정렬할 수 없습니다", t), format!("cannot sort a list of {}", t)),
                        line,
                        col,
                        tr!("Int, Float, Str, Bool 리스트만 정렬됩니다", "only lists of Int, Float, Str, or Bool can be sorted"),
                    ),
                    None => Ok(Value::None),
                }
            }
            (Value::List(items), "index_of") => {
                let target = match args.first() {
                    Some(v) => v,
                    None => return fail("E0241", tr!("index_of()는 찾을 값을 받습니다", "index_of() takes the value to search for"), line, col),
                };
                let v = items.borrow();
                Ok(Value::Int(
                    v.iter().position(|x| x.eq_value(target)).map(|i| i as i64).unwrap_or(-1),
                ))
            }
            (Value::List(items), "slice") => {
                let v = items.borrow();
                let n = v.len() as i64;
                let (a, b) = slice_bounds(&args, n);
                Ok(Value::List(Rc::new(RefCell::new(
                    v[a as usize..b as usize].iter().map(|x| x.deep_clone()).collect(),
                ))))
            }
            (Value::List(items), "clear") => {
                items.borrow_mut().clear();
                Ok(Value::None)
            }
            (Value::Str(st), "find") => {
                let needle = args.first().map(|v| v.display()).unwrap_or_default();
                // Counts characters, not bytes.
                Ok(Value::Int(match st.find(&needle) {
                    Some(byte_at) => st[..byte_at].chars().count() as i64,
                    None => -1,
                }))
            }
            (Value::Str(st), "repeat") => match args.first() {
                Some(Value::Int(n)) if *n >= 0 => Ok(str_value(st.repeat(*n as usize))),
                _ => fail("E0241", tr!("repeat()는 0 이상의 Int를 받습니다", "repeat() takes a non-negative Int"), line, col),
            },
            (Value::Str(st), "slice") => {
                let chars: Vec<char> = st.chars().collect();
                let n = chars.len() as i64;
                let (a, b) = slice_bounds(&args, n);
                Ok(str_value(chars[a as usize..b as usize].iter().collect::<String>()))
            }
            // Display width. Hangul, CJK, full-width and emoji take two columns.
            (Value::Str(st), "width") => Ok(Value::Int(disp_width(st))),
            // pad_right: left-align (spaces on the right). pad_left: right-align (spaces on the left). Based on display width.
            (Value::Str(st), "pad_right") => match args.first() {
                Some(Value::Int(w)) => {
                    let pad = (*w - disp_width(st)).max(0) as usize;
                    Ok(str_value(format!("{}{}", st, " ".repeat(pad))))
                }
                _ => fail("E0241", tr!("pad_right()는 Int(폭)를 받습니다", "pad_right() takes an Int width"), line, col),
            },
            (Value::Str(st), "pad_left") => match args.first() {
                Some(Value::Int(w)) => {
                    let pad = (*w - disp_width(st)).max(0) as usize;
                    Ok(str_value(format!("{}{}", " ".repeat(pad), st)))
                }
                _ => fail("E0241", tr!("pad_left()는 Int(폭)를 받습니다", "pad_left() takes an Int width"), line, col),
            },
            (Value::Json(j), _) => {
                use crate::json::{JsonVal, wrap};
                let take_json = |v: &Value| match v {
                    Value::Json(x) => Some(x.clone()),
                    _ => None,
                };
                match name {
                    "kind" => Ok(str_value(crate::json::kind(&j.borrow()).to_string())),
                    "as_int" => Ok(match &*j.borrow() {
                        JsonVal::Int(n) => Value::Int(*n),
                        _ => Value::None,
                    }),
                    "as_float" => Ok(match &*j.borrow() {
                        JsonVal::Float(f) => Value::Float(*f),
                        JsonVal::Int(n) => Value::Float(*n as f64),
                        _ => Value::None,
                    }),
                    "as_str" => Ok(match &*j.borrow() {
                        JsonVal::Str(s) => str_value(s.clone()),
                        _ => Value::None,
                    }),
                    "as_bool" => Ok(match &*j.borrow() {
                        JsonVal::Bool(b) => Value::Bool(*b),
                        _ => Value::None,
                    }),
                    "len" => Ok(Value::Int(match &*j.borrow() {
                        JsonVal::List(v) => v.len() as i64,
                        JsonVal::Dict(v) => v.len() as i64,
                        _ => 0,
                    })),
                    "keys" => Ok(match &*j.borrow() {
                        JsonVal::Dict(v) => {
                            list_value(v.iter().map(|(k, _)| str_value(k.clone())).collect())
                        }
                        _ => list_value(Vec::new()),
                    }),
                    "get" => {
                        let key = match args.first() {
                            Some(v) => v.display(),
                            None => return fail("E0241", tr!("get()은 이름을 받습니다", "get() takes a key"), line, col),
                        };
                        Ok(match &*j.borrow() {
                            JsonVal::Dict(v) => match v.iter().find(|(k, _)| *k == key) {
                                Some((_, x)) => Value::Json(x.clone()),
                                None => Value::None,
                            },
                            _ => Value::None,
                        })
                    }
                    "at" => {
                        let i = match args.first() {
                            Some(Value::Int(n)) => *n,
                            _ => return fail("E0241", tr!("at()은 Int를 받습니다", "at() takes an Int"), line, col),
                        };
                        Ok(match &*j.borrow() {
                            JsonVal::List(v) => {
                                if i < 0 || i as usize >= v.len() {
                                    Value::None
                                } else {
                                    Value::Json(v[i as usize].clone())
                                }
                            }
                            _ => Value::None,
                        })
                    }
                    "set" => {
                        let key = match args.first() {
                            Some(v) => v.display(),
                            None => return fail("E0241", tr!("set()은 이름과 값을 받습니다", "set() takes a key and a value"), line, col),
                        };
                        let val = match args.get(1).and_then(take_json) {
                            Some(v) => v,
                            None => return fail("E0241", tr!("set()의 값은 Json이어야 합니다", "the value passed to set() must be Json"), line, col),
                        };
                        let mut b = j.borrow_mut();
                        if let JsonVal::Dict(v) = &mut *b {
                            match v.iter_mut().find(|(k, _)| *k == key) {
                                Some(slot) => slot.1 = val,
                                None => v.push((key, val)),
                            }
                            Ok(Value::None)
                        } else {
                            fail_fix("E0247", tr!("set()은 JSON 객체에만 씁니다", "set() only works on JSON objects"), line, col, tr!("`jdict()` 로 만든 값에 쓰세요", "use it on a value created with `jdict()`"))
                        }
                    }
                    "push" => {
                        let val = match args.first().and_then(take_json) {
                            Some(v) => v,
                            None => return fail("E0241", tr!("push()의 값은 Json이어야 합니다", "the value passed to push() must be Json"), line, col),
                        };
                        let mut b = j.borrow_mut();
                        if let JsonVal::List(v) = &mut *b {
                            v.push(val);
                            Ok(Value::None)
                        } else {
                            fail_fix("E0247", tr!("push()는 JSON 배열에만 씁니다", "push() only works on JSON arrays"), line, col, tr!("`jlist()` 로 만든 값에 쓰세요", "use it on a value created with `jlist()`"))
                        }
                    }
                    other => {
                        let _ = wrap;
                        fail_fix(
                            "E0235",
                            tr!(format!("Json에 `{}` 메서드가 없습니다", other), format!("Json has no method `{}`", other)),
                            line,
                            col,
                            tr!("쓸 수 있는 것: kind, as_int, as_float, as_str, as_bool, get, at, len, keys, set, push", "available: kind, as_int, as_float, as_str, as_bool, get, at, len, keys, set, push"),
                        )
                    }
                }
            }
            (Value::Dict(pairs), "len") => Ok(Value::Int(pairs.borrow().len() as i64)),
            (Value::Dict(pairs), "set") => {
                let k = args.first().cloned().unwrap_or(Value::None);
                let v = args.get(1).cloned().unwrap_or(Value::None);
                let mut b = pairs.borrow_mut();
                if let Some(slot) = b.iter_mut().find(|(ek, _)| ek.eq_value(&k)) {
                    slot.1 = v;
                } else {
                    b.push((k, v));
                }
                Ok(Value::None)
            }
            (Value::Dict(pairs), "has" | "contains") => {
                let k = args.first().cloned().unwrap_or(Value::None);
                Ok(Value::Bool(pairs.borrow().iter().any(|(ek, _)| ek.eq_value(&k))))
            }
            (Value::Dict(pairs), "keys") => {
                let ks: Vec<Value> = pairs.borrow().iter().map(|(k, _)| k.clone()).collect();
                Ok(list_value(ks))
            }
            // `d.get(k, default)` — yields the value if the key exists, otherwise the default.
            // Returns V directly rather than `?V`, so no none check is needed.
            (Value::Dict(pairs), "get") => {
                let k = args.first().cloned().unwrap_or(Value::None);
                let def = args.get(1).cloned().unwrap_or(Value::None);
                let found = pairs.borrow().iter().find(|(ek, _)| ek.eq_value(&k)).map(|(_, v)| v.clone());
                Ok(found.unwrap_or(def))
            }

            _ => fail_fix(
                "E0235",
                tr!(format!("{}에 `{}` 메서드가 없습니다", recv.type_name(), name), format!("{} has no method `{}`", recv.type_name(), name)),
                line,
                col,
                tr!("`siskin check`로 쓸 수 있는 메서드를 확인하세요", "run `siskin check` to see which methods are available"),
            ),
        }
    }

    fn call_builtin(
        &mut self,
        name: &str,
        pos: Vec<Value>,
        named: Vec<(String, Value)>,
        line: usize,
        col: usize,
    ) -> R<Value> {
        if !named.is_empty() && name != "print" {
            return fail(
                "E0236",
                tr!(format!("`{}`은(는) 이름 붙은 인자를 받지 않습니다", name), format!("`{}` does not take named arguments", name)),
                line,
                col,
            );
        }
        match name {
            // `channel[T]()` / `channel[T](size)` — the type argument is only seen by the type checker.
            "channel" => {
                let cap = match pos.first() {
                    None => 0,
                    Some(Value::Int(n)) if *n >= 1 => *n as usize,
                    Some(Value::Int(n)) => {
                        return fail_fix(
                            "E0262",
                            tr!(format!("통로 크기는 1 이상이어야 하는데 {}입니다", n), format!("channel size must be at least 1, got {}", n)),
                            line,
                            col,
                            tr!("크기 없이 `channel[T]()` 로 만들면 끝없이 쌓입니다", "create it without a size, `channel[T]()`, for an unbounded channel"),
                        )
                    }
                    Some(_) => 0,
                };
                Ok(Value::Chan(std::sync::Arc::new(crate::value::ChanCell {
                    state: std::sync::Mutex::new(crate::value::ChanState::default()),
                    cap,
                })))
            }
            // print writes exactly what it is given. Newlines must be added explicitly with `\n`.
            // Error output (stderr). Flushes stdout first so the two don't interleave.
            "eprint" => {
                use std::io::Write;
                let _ = std::io::stdout().flush();
                let mut out = String::new();
                for v in &pos {
                    out.push_str(&v.display());
                }
                eprint!("{}", out);
                Ok(Value::None)
            }
            "print" => {
                if let Some((n, _)) = named.first() {
                    return fail_fix(
                        "E0236",
                        tr!(format!("`print`에 `{}`이라는 인자가 없습니다", n), format!("`print` has no parameter named `{}`", n)),
                        line,
                        col,
                        tr!("print는 받은 값을 그대로 이어 붙여 출력합니다. 줄바꿈은 `\\n`을 쓰세요", "print outputs its arguments joined together as is; use `\\n` for a newline"),
                    );
                }
                let mut out = String::new();
                for v in &pos {
                    out.push_str(&v.display());
                }
                print!("{}", out);
                use std::io::Write;
                let _ = std::io::stdout().flush();
                Ok(Value::None)
            }
            "len" => match pos.first() {
                Some(Value::List(l)) => Ok(Value::Int(l.borrow().len() as i64)),
                Some(Value::Str(s)) => Ok(Value::Int(s.chars().count() as i64)),
                Some(Value::Dict(d)) => Ok(Value::Int(d.borrow().len() as i64)),
                Some(other) => fail(
                    "E0237",
                    tr!(format!("len()은 {} 값에 쓸 수 없습니다", other.type_name()), format!("len() cannot be used on a {} value", other.type_name())),
                    line,
                    col,
                ),
                None => fail("E0238", tr!("len()에 인자가 필요합니다", "len() needs an argument"), line, col),
            },
            "range" => {
                let (from, to) = match pos.len() {
                    1 => match &pos[0] {
                        Value::Int(n) => (0, *n),
                        other => {
                            return fail(
                                "E0239",
                                tr!(format!("range()는 Int를 받는데 {}입니다", other.type_name()), format!("range() takes Int, found {}", other.type_name())),
                                line,
                                col,
                            )
                        }
                    },
                    2 => match (&pos[0], &pos[1]) {
                        (Value::Int(a), Value::Int(b)) => (*a, *b),
                        _ => return fail("E0239", tr!("range()는 Int를 받습니다", "range() takes Int"), line, col),
                    },
                    _ => {
                        return fail_fix(
                            "E0240",
                            tr!("range()는 인자 1개 또는 2개를 받습니다", "range() takes 1 or 2 arguments"),
                            line,
                            col,
                            tr!("`range(n)` 또는 `range(시작, 끝)`", "`range(n)` or `range(start, end)`"),
                        )
                    }
                };
                let items: Vec<Value> = (from..to).map(Value::Int).collect();
                Ok(list_value(items))
            }
            "input" => {
                use std::io::Write;
                // If a prompt is given, print it first (without a newline).
                if let Some(v) = pos.first() {
                    print!("{}", v.display());
                    let _ = std::io::stdout().flush();
                }
                let mut line_in = String::new();
                match crate::conc::without_gil(|| std::io::stdin().read_line(&mut line_in)) {
                    Ok(0) => Ok(Value::None), // end of input (EOF)
                    Ok(_) => {
                        while line_in.ends_with('\n') || line_in.ends_with('\r') {
                            line_in.pop();
                        }
                        Ok(str_value(line_in))
                    }
                    Err(_) => Ok(Value::None),
                }
            }
            "args" => Ok(list_value(
                self.prog_args.iter().map(|a| str_value(a.clone())).collect(),
            )),
            "exit" => {
                use std::io::Write;
                crate::conc::shutdown();
                let _ = std::io::stdout().flush();
                let code = match pos.first() {
                    Some(Value::Int(n)) => *n as i32,
                    _ => 0,
                };
                std::process::exit(code);
            }
            "str" => Ok(str_value(pos.first().map(|v| v.display()).unwrap_or_default())),
            "int" => match pos.first() {
                Some(Value::Str(s)) => match s.trim().parse::<i64>() {
                    Ok(n) => Ok(Value::Int(n)),
                    Err(_) => Ok(Value::Error(Rc::new(tr!(format!("`{}`을(를) Int로 읽을 수 없습니다", s), format!("cannot parse `{}` as Int", s))))),
                },
                Some(Value::Float(f)) => Ok(Value::Int(*f as i64)),
                Some(Value::Int(n)) => Ok(Value::Int(*n)),
                Some(other) => fail(
                    "E0241",
                    tr!(format!("int()는 {} 값을 받을 수 없습니다", other.type_name()), format!("int() cannot take a {} value", other.type_name())),
                    line,
                    col,
                ),
                None => fail("E0238", tr!("int()에 인자가 필요합니다", "int() needs an argument"), line, col),
            },
            "float" => match pos.first() {
                Some(Value::Int(n)) => Ok(Value::Float(*n as f64)),
                Some(Value::Float(f)) => Ok(Value::Float(*f)),
                Some(Value::Str(s)) => match s.trim().parse::<f64>() {
                    Ok(f) => Ok(Value::Float(f)),
                    Err(_) => Ok(Value::Error(Rc::new(tr!(format!("`{}`을(를) Float로 읽을 수 없습니다", s), format!("cannot parse `{}` as Float", s))))),
                },
                Some(other) => fail(
                    "E0241",
                    tr!(format!("float()는 {} 값을 받을 수 없습니다", other.type_name()), format!("float() cannot take a {} value", other.type_name())),
                    line,
                    col,
                ),
                None => fail("E0238", tr!("float()에 인자가 필요합니다", "float() needs an argument"), line, col),
            },
            "error" => match pos.first() {
                Some(v @ Value::Enum(_)) => Ok(Value::ErrorOf(Rc::new(v.deep_clone()))),
                other => Ok(Value::Error(Rc::new(other.map(|v| v.display()).unwrap_or_default()))),
            },
            "assert" => match pos.first() {
                Some(Value::Bool(true)) => Ok(Value::None),
                Some(Value::Bool(false)) => {
                    let msg = pos.get(1).map(|v| v.display()).unwrap_or_else(|| tr!("단언 실패", "assertion failed").into());
                    fail("E0242", msg, line, col)
                }
                Some(other) => fail(
                    "E0202",
                    tr!(format!("assert()는 Bool을 받는데 {}입니다", other.type_name()), format!("assert() takes Bool, found {}", other.type_name())),
                    line,
                    col,
                ),
                None => fail("E0238", tr!("assert()에 인자가 필요합니다", "assert() needs an argument"), line, col),
            },
            "abs" => match pos.first() {
                Some(Value::Int(n)) => Ok(Value::Int(n.abs())),
                Some(Value::Float(f)) => Ok(Value::Float(f.abs())),
                _ => fail("E0241", tr!("abs()는 Int 또는 Float를 받습니다", "abs() takes Int or Float"), line, col),
            },
            "min" | "max" => {
                if pos.len() != 2 {
                    return fail("E0240", tr!(format!("{}()는 인자 2개를 받습니다", name), format!("{}() takes 2 arguments", name)), line, col);
                }
                let is_min = name == "min";
                match (&pos[0], &pos[1]) {
                    (Value::Int(a), Value::Int(b)) => {
                        Ok(Value::Int(if (a < b) == is_min { *a } else { *b }))
                    }
                    (Value::Float(a), Value::Float(b)) => {
                        Ok(Value::Float(if (a < b) == is_min { *a } else { *b }))
                    }
                    (a, b) => fail_fix(
                        "E0201",
                        tr!(format!("{}()에 {}와(과) {}을(를) 섞을 수 없습니다", name, a.type_name(), b.type_name()), format!("{}() cannot mix {} and {}", name, a.type_name(), b.type_name())),
                        line,
                        col,
                        tr!("Siskin에는 암묵적 형변환이 없습니다. `float(x)`로 맞추세요", "Siskin has no implicit conversions; convert with `float(x)`"),
                    ),
                }
            }
            "sqrt" => match pos.first() {
                Some(Value::Float(f)) => Ok(Value::Float(f.sqrt())),
                Some(Value::Int(_)) => fail_fix(
                    "E0201",
                    tr!("sqrt()는 Float를 받습니다", "sqrt() takes Float"),
                    line,
                    col,
                    tr!("`sqrt(float(n))` 으로 쓰세요. Siskin에는 암묵적 형변환이 없습니다", "write `sqrt(float(n))`; Siskin has no implicit conversions"),
                ),
                _ => fail("E0241", tr!("sqrt()는 Float를 받습니다", "sqrt() takes Float"), line, col),
            },
            "floor" => match pos.first() {
                Some(Value::Float(f)) => Ok(Value::Int(f.floor() as i64)),
                _ => fail("E0241", tr!("floor()는 Float를 받습니다", "floor() takes Float"), line, col),
            },
            "ceil" => match pos.first() {
                Some(Value::Float(f)) => Ok(Value::Int(f.ceil() as i64)),
                _ => fail("E0241", tr!("ceil()는 Float를 받습니다", "ceil() takes Float"), line, col),
            },
            "pow" => match (pos.first(), pos.get(1)) {
                (Some(Value::Float(a)), Some(Value::Float(b))) => Ok(Value::Float(a.powf(*b))),
                (Some(Value::Int(a)), Some(Value::Int(b))) => {
                    Ok(Value::Int(a.pow((*b).max(0) as u32)))
                }
                _ => fail("E0241", tr!("pow()는 같은 타입 두 개를 받습니다", "pow() takes two values of the same type"), line, col),
            },
            "read_text" => match pos.first() {
                Some(Value::Str(p)) => match std::fs::read(p.as_str()) {
                    Ok(t) => Ok(str_value(String::from_utf8_lossy(&t).to_string())),
                    Err(e) => Ok(Value::Error(Rc::new(crate::sys::errmsg(p, &e)))),
                },
                _ => fail("E0241", tr!("read_text()는 Str 경로를 받습니다", "read_text() takes a Str path"), line, col),
            },
            "write_text" => match (pos.first(), pos.get(1)) {
                (Some(Value::Str(p)), Some(v)) => match std::fs::write(p.as_str(), v.display()) {
                    Ok(()) => Ok(Value::None),
                    Err(e) => Ok(Value::Error(Rc::new(crate::sys::errmsg(p, &e)))),
                },
                _ => fail("E0241", tr!("write_text()는 경로와 내용을 받습니다", "write_text() takes a path and contents"), line, col),
            },
            // ---- std.math additions ----
            "sin" | "cos" | "tan" | "log" | "log10" | "exp" => match pos.first() {
                Some(Value::Float(f)) => Ok(Value::Float(match name {
                    "sin" => f.sin(),
                    "cos" => f.cos(),
                    "tan" => f.tan(),
                    "log" => f.ln(),
                    "log10" => f.log10(),
                    _ => f.exp(),
                })),
                _ => fail_fix(
                    "E0241",
                    tr!(format!("{}()는 Float를 받습니다", name), format!("{}() takes Float", name)),
                    line,
                    col,
                    tr!(format!("`{}(float(n))` 으로 쓰세요. Siskin에는 암묵적 형변환이 없습니다", name), format!("write `{}(float(n))`; Siskin has no implicit conversions", name)),
                ),
            },
            "round" => match pos.first() {
                Some(Value::Float(f)) => Ok(Value::Int(mi_round(*f))),
                _ => fail("E0241", tr!("round()는 Float를 받습니다", "round() takes Float"), line, col),
            },
            "pi" => Ok(Value::Float(std::f64::consts::PI)),
            "e" => Ok(Value::Float(std::f64::consts::E)),

            // ---- std.fs additions ----
            "append_text" => match (pos.first(), pos.get(1)) {
                (Some(Value::Str(p)), Some(v)) => {
                    use std::io::Write;
                    let r = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(p.as_str())
                        .and_then(|mut f| f.write_all(v.display().as_bytes()));
                    match r {
                        Ok(()) => Ok(Value::None),
                        Err(e) => Ok(Value::Error(Rc::new(crate::sys::errmsg(p, &e)))),
                    }
                }
                _ => fail("E0241", tr!("append_text()는 경로와 내용을 받습니다", "append_text() takes a path and contents"), line, col),
            },
            "exists" => match pos.first() {
                Some(Value::Str(p)) => Ok(Value::Bool(std::path::Path::new(p.as_str()).exists())),
                _ => fail("E0241", tr!("exists()는 Str 경로를 받습니다", "exists() takes a Str path"), line, col),
            },
            "remove" => match pos.first() {
                Some(Value::Str(p)) => match std::fs::remove_file(p.as_str()) {
                    Ok(()) => Ok(Value::None),
                    Err(e) => Ok(Value::Error(Rc::new(crate::sys::errmsg(p, &e)))),
                },
                _ => fail("E0241", tr!("remove()는 Str 경로를 받습니다", "remove() takes a Str path"), line, col),
            },
            "list_dir" => match pos.first() {
                Some(Value::Str(p)) => match crate::sys::list_dir(p) {
                    Ok(v) => Ok(list_value(v.into_iter().map(str_value).collect())),
                    Err(m) => Ok(Value::Error(Rc::new(m))),
                },
                _ => fail("E0241", tr!("list_dir()는 Str 경로를 받습니다", "list_dir() takes a Str path"), line, col),
            },
            "make_dir" => match pos.first() {
                Some(Value::Str(p)) => match crate::sys::make_dir(p) {
                    Ok(()) => Ok(Value::None),
                    Err(m) => Ok(Value::Error(Rc::new(m))),
                },
                _ => fail("E0241", tr!("make_dir()는 Str 경로를 받습니다", "make_dir() takes a Str path"), line, col),
            },
            "is_dir" => match pos.first() {
                Some(Value::Str(p)) => Ok(Value::Bool(crate::sys::is_dir(p))),
                _ => fail("E0241", tr!("is_dir()는 Str 경로를 받습니다", "is_dir() takes a Str path"), line, col),
            },

            // ---- std.process ----
            "env" => match pos.first() {
                Some(Value::Str(n)) => Ok(match std::env::var_os(n.as_str()) {
                    Some(v) => str_value(v.to_string_lossy().to_string()),
                    None => Value::None,
                }),
                _ => fail("E0241", tr!("env()는 Str 이름을 받습니다", "env() takes a Str name"), line, col),
            },
            "set_env" => match (pos.first(), pos.get(1)) {
                (Some(Value::Str(n)), Some(Value::Str(v))) => {
                    std::env::set_var(n.as_str(), v.as_str());
                    Ok(Value::None)
                }
                _ => fail("E0241", tr!("set_env()는 이름과 값(Str)을 받습니다", "set_env() takes a name and a value (Str)"), line, col),
            },
            "cwd" => Ok(str_value(
                std::env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
            )),
            "set_cwd" => match pos.first() {
                Some(Value::Str(p)) => match std::env::set_current_dir(p.as_str()) {
                    Ok(()) => Ok(Value::None),
                    Err(e) => Ok(Value::Error(Rc::new(crate::sys::errmsg(p, &e)))),
                },
                _ => fail("E0241", tr!("set_cwd()는 Str 경로를 받습니다", "set_cwd() takes a Str path"), line, col),
            },
            "pid" => Ok(Value::Int(std::process::id() as i64)),
            "__run" => {
                let prog = pos.first().map(|v| v.display()).unwrap_or_default();
                let argv: Vec<String> = match pos.get(1) {
                    Some(Value::List(xs)) => xs.borrow().iter().map(|v| v.display()).collect(),
                    _ => Vec::new(),
                };
                let shell = matches!(pos.get(2), Some(Value::Bool(true)));
                let (code, out, err) = crate::sys::run(&prog, &argv, shell);
                self.run_out = out;
                self.run_err = err;
                Ok(Value::Int(code))
            }
            "__run_out" => Ok(str_value(self.run_out.clone())),
            // Used by the standard library (the parts written in Siskin) to pick the language of error text.
            "__ko" => Ok(Value::Bool(crate::lang::ko())),
            "__run_err" => Ok(str_value(self.run_err.clone())),
            // std.net only runs natively. `siskin run` switches to native automatically,
            // but calls made directly by the interpreter (e.g. `siskin test` examples) end up here.
            n if n.starts_with("__net_") || n.starts_with("__http") || n == "__url_encode" => fail_fix(
                "E0250",
                tr!("std.net 은 이 자리(인터프리터)에서는 쓸 수 없습니다", "std.net cannot be used here (in the interpreter)"),
                line,
                col,
                tr!("`siskin run` 이나 `siskin build` 로 돌리세요. 둘 다 네이티브로 컴파일해서 실행합니다", "run it with `siskin run` or `siskin build`; both compile it natively"),
            ),

            // ---- std.time additions ----
            "sleep" => match pos.first() {
                Some(Value::Float(s)) => {
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    let secs = *s;
                    crate::conc::without_gil(|| crate::sys::sleep(secs));
                    Ok(Value::None)
                }
                _ => fail("E0241", tr!("sleep()는 Float(초)를 받습니다", "sleep() takes Float seconds"), line, col),
            },
            "__time_parts" => match (pos.first(), pos.get(1)) {
                (Some(Value::Float(t)), Some(Value::Bool(u))) => Ok(list_value(
                    crate::sys::time_parts(*t, *u).into_iter().map(Value::Int).collect(),
                )),
                _ => fail("E0241", "__time_parts(Float, Bool)", line, col),
            },
            "__time_make" => {
                let nums: Vec<i64> = pos
                    .iter()
                    .take(6)
                    .map(|v| if let Value::Int(n) = v { *n } else { 0 })
                    .collect();
                let utc = matches!(pos.get(6), Some(Value::Bool(true)));
                Ok(Value::Float(crate::sys::time_make(&nums, utc)))
            }

            // ---- std.time ----
            "now" => {
                let d = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                Ok(Value::Float(d))
            }
            "clock" => Ok(Value::Float(mi_clock())),

            // ---- std.random ----
            "seed" => match pos.first() {
                Some(Value::Int(n)) => {
                    // Avoid 0, which would make it get stuck.
                    self.rng = (*n as u64) ^ 0x9E3779B97F4A7C15;
                    if self.rng == 0 {
                        self.rng = 0x853C49E6748FEA9B;
                    }
                    Ok(Value::None)
                }
                _ => fail("E0241", tr!("seed()는 Int를 받습니다", "seed() takes Int"), line, col),
            },
            "rand" => {
                let r = mi_next_rand(&mut self.rng);
                // Use only the top 53 bits to produce a value in [0.0, 1.0).
                Ok(Value::Float((r >> 11) as f64 / 9007199254740992.0))
            }
            "rand_int" => match (pos.first(), pos.get(1)) {
                (Some(Value::Int(lo)), Some(Value::Int(hi))) => {
                    if hi <= lo {
                        return fail_fix(
                            "E0244",
                            tr!(format!("rand_int({}, {}): 뒤 숫자가 더 커야 합니다", lo, hi), format!("rand_int({}, {}): the upper bound must be greater than the lower bound", lo, hi)),
                            line,
                            col,
                            tr!("`rand_int(1, 7)` 은 1 이상 7 미만입니다", "`rand_int(1, 7)` returns a number from 1 up to but not including 7"),
                        );
                    }
                    let span = (hi - lo) as u64;
                    let r = mi_next_rand(&mut self.rng);
                    Ok(Value::Int(lo + (r % span) as i64))
                }
                _ => fail("E0241", tr!("rand_int()는 Int 두 개를 받습니다", "rand_int() takes two Ints"), line, col),
            },

            // ---- std.json ----
            "parse" => match pos.first() {
                Some(Value::Str(t)) => match crate::json::parse(t.as_str()) {
                    Ok(j) => Ok(Value::Json(j)),
                    Err(e) => Ok(Value::Error(Rc::new(format!("JSON: {}", e)))),
                },
                _ => fail("E0241", tr!("parse()는 Str을 받습니다", "parse() takes Str"), line, col),
            },
            "stringify" => match pos.first() {
                Some(Value::Json(j)) => Ok(str_value(crate::json::stringify(j))),
                _ => fail("E0241", tr!("stringify()는 Json을 받습니다", "stringify() takes Json"), line, col),
            },
            "jnull" => Ok(Value::Json(crate::json::wrap(crate::json::JsonVal::Null))),
            "jlist" => Ok(Value::Json(crate::json::wrap(crate::json::JsonVal::List(Vec::new())))),
            "jdict" => Ok(Value::Json(crate::json::wrap(crate::json::JsonVal::Dict(Vec::new())))),
            "jbool" => match pos.first() {
                Some(Value::Bool(b)) => Ok(Value::Json(crate::json::wrap(crate::json::JsonVal::Bool(*b)))),
                _ => fail("E0241", tr!("jbool()은 Bool을 받습니다", "jbool() takes Bool"), line, col),
            },
            "jint" => match pos.first() {
                Some(Value::Int(n)) => Ok(Value::Json(crate::json::wrap(crate::json::JsonVal::Int(*n)))),
                _ => fail("E0241", tr!("jint()는 Int를 받습니다", "jint() takes Int"), line, col),
            },
            "jfloat" => match pos.first() {
                Some(Value::Float(f)) => Ok(Value::Json(crate::json::wrap(crate::json::JsonVal::Float(*f)))),
                _ => fail("E0241", tr!("jfloat()는 Float를 받습니다", "jfloat() takes Float"), line, col),
            },
            "jstr" => match pos.first() {
                Some(Value::Str(t)) => {
                    Ok(Value::Json(crate::json::wrap(crate::json::JsonVal::Str(t.as_str().to_string()))))
                }
                _ => fail("E0241", tr!("jstr()는 Str을 받습니다", "jstr() takes Str"), line, col),
            },

            // ---- std.re (regular expressions) ----
            "test" | "find" | "find_all" | "groups" | "replace" | "split_re" => {
                let pat = match pos.first() {
                    Some(Value::Str(p)) => p.as_str().to_string(),
                    _ => return fail("E0241", tr!(format!("{}()의 첫 인자는 정규식(Str)입니다", name), format!("the first argument of {}() is the regex (Str)", name)), line, col),
                };
                let text = match pos.get(1) {
                    Some(Value::Str(t)) => t.as_str().to_string(),
                    _ => return fail("E0241", tr!(format!("{}()의 둘째 인자는 대상 문자열입니다", name), format!("the second argument of {}() is the text to search", name)), line, col),
                };
                let prog = match crate::regex::compile(&pat) {
                    Ok(p) => p,
                    Err(e) => {
                        return fail_fix("E0246", e, line, col, tr!("정규식을 다시 확인하세요", "check the regular expression"))
                    }
                };
                let chars: Vec<char> = text.chars().collect();
                let take = |s: &[isize], i: usize| -> Option<String> {
                    let a = s[i * 2];
                    let b = s[i * 2 + 1];
                    if a < 0 || b < 0 {
                        None
                    } else {
                        Some(chars[a as usize..b as usize].iter().collect())
                    }
                };
                match name {
                    "test" => Ok(Value::Bool(crate::regex::search(&prog, &chars, 0).is_some())),
                    "find" => Ok(match crate::regex::search(&prog, &chars, 0) {
                        Some(s) => match take(&s, 0) {
                            Some(t) => str_value(t),
                            None => Value::None,
                        },
                        None => Value::None,
                    }),
                    "groups" => Ok(match crate::regex::search(&prog, &chars, 0) {
                        Some(s) => {
                            let mut out = Vec::new();
                            for g in 0..=prog.ngroups {
                                out.push(str_value(take(&s, g).unwrap_or_default()));
                            }
                            list_value(out)
                        }
                        None => list_value(Vec::new()),
                    }),
                    "find_all" => {
                        let mut out = Vec::new();
                        let mut at = 0usize;
                        while at <= chars.len() {
                            match crate::regex::search(&prog, &chars, at) {
                                Some(s) => {
                                    let (a, b) = (s[0] as usize, s[1] as usize);
                                    out.push(str_value(chars[a..b].iter().collect::<String>()));
                                    at = if b > a { b } else { b + 1 };
                                }
                                None => break,
                            }
                        }
                        Ok(list_value(out))
                    }
                    "split_re" => {
                        let mut out = Vec::new();
                        let mut at = 0usize;
                        let mut last = 0usize;
                        while at <= chars.len() {
                            match crate::regex::search(&prog, &chars, at) {
                                Some(s) => {
                                    let (a, b) = (s[0] as usize, s[1] as usize);
                                    if b == a {
                                        at = a + 1;
                                        continue;
                                    }
                                    out.push(str_value(chars[last..a].iter().collect::<String>()));
                                    last = b;
                                    at = b;
                                }
                                None => break,
                            }
                        }
                        out.push(str_value(chars[last..].iter().collect::<String>()));
                        Ok(list_value(out))
                    }
                    _ => {
                        // replace(pattern, target, replacement)
                        let repl = match pos.get(2) {
                            Some(Value::Str(r)) => r.as_str().to_string(),
                            _ => {
                                return fail(
                                    "E0241",
                                    tr!("replace()는 `replace(정규식, 대상, 바꿀문자열)` 입니다", "usage: `replace(regex, text, replacement)`"),
                                    line,
                                    col,
                                )
                            }
                        };
                        let mut out = String::new();
                        let mut at = 0usize;
                        let mut last = 0usize;
                        while at <= chars.len() {
                            match crate::regex::search(&prog, &chars, at) {
                                Some(s) => {
                                    let (a, b) = (s[0] as usize, s[1] as usize);
                                    out.extend(chars[last..a].iter());
                                    out.push_str(&repl);
                                    last = b;
                                    at = if b > a { b } else { b + 1 };
                                }
                                None => break,
                            }
                        }
                        out.extend(chars[last.min(chars.len())..].iter());
                        Ok(str_value(out))
                    }
                }
            }

            // ---- prelude additions ----
            "sum" => match pos.first() {
                Some(Value::List(items)) => {
                    let items = items.borrow();
                    if items.iter().any(|v| matches!(v, Value::Float(_))) {
                        let mut t = 0.0;
                        for v in items.iter() {
                            match v {
                                Value::Float(f) => t += f,
                                Value::Int(i) => t += *i as f64,
                                other => {
                                    return fail(
                                        "E0241",
                                        tr!(format!("sum()에 {} 값이 들어 있습니다", other.type_name()), format!("sum() got a list containing a {} value", other.type_name())),
                                        line,
                                        col,
                                    )
                                }
                            }
                        }
                        Ok(Value::Float(t))
                    } else {
                        let mut t: i64 = 0;
                        for v in items.iter() {
                            match v {
                                Value::Int(i) => t = t.wrapping_add(*i),
                                other => {
                                    return fail(
                                        "E0241",
                                        tr!(format!("sum()에 {} 값이 들어 있습니다", other.type_name()), format!("sum() got a list containing a {} value", other.type_name())),
                                        line,
                                        col,
                                    )
                                }
                            }
                        }
                        Ok(Value::Int(t))
                    }
                }
                _ => fail_fix("E0241", tr!("sum()은 숫자 리스트를 받습니다", "sum() takes a list of numbers"), line, col, tr!("`sum(xs)` 처럼 씁니다", "use it like `sum(xs)`")),
            },

            other => fail("E0243", tr!(format!("내장 함수 `{}`을(를) 모릅니다", other), format!("unknown builtin function `{}`", other)), line, col),
        }
    }

    // ------------------------------------------------------------- binary operations

    fn binary(&mut self, op: BinOp, a: Value, b: Value, line: usize, col: usize) -> R<Value> {
        use BinOp::*;
        // Equality comparison is allowed across different types (result is false).
        if op == Eq {
            return Ok(Value::Bool(a.eq_value(&b)));
        }
        if op == Ne {
            return Ok(Value::Bool(!a.eq_value(&b)));
        }

        // Pointer arithmetic: `p + 1` points to the next element.
        if let (Value::Raw(buf, off), Value::Int(n)) = (&a, &b) {
            let shifted = match op {
                Add => *off as i64 + *n,
                Sub => *off as i64 - *n,
                _ => {
                    return fail("E0238", tr!("포인터에는 더하기와 빼기만 쓸 수 있습니다", "only addition and subtraction work on pointers"), line, col);
                }
            };
            if shifted < 0 {
                return fail("E0237", tr!("포인터가 메모리 앞쪽으로 벗어났습니다", "pointer moved before the start of its memory"), line, col);
            }
            return Ok(Value::Raw(Rc::clone(buf), shifted as usize));
        }

        match (&a, &b) {
            (Value::Int(x), Value::Int(y)) => {
                let (x, y) = (*x, *y);
                match op {
                    Add => Ok(Value::Int(x.wrapping_add(y))),
                    Sub => Ok(Value::Int(x.wrapping_sub(y))),
                    Mul => Ok(Value::Int(x.wrapping_mul(y))),
                    Div => {
                        if y == 0 {
                            fail_fix("E0203", tr!("0으로 나눌 수 없습니다", "division by zero"), line, col, tr!("나누기 전에 `b != 0`을 확인하세요", "check `b != 0` before dividing"))
                        } else {
                            Ok(Value::Int(x / y))
                        }
                    }
                    Mod => {
                        if y == 0 {
                            fail("E0203", tr!("0으로 나눌 수 없습니다", "division by zero"), line, col)
                        } else {
                            Ok(Value::Int(x % y))
                        }
                    }
                    Lt => Ok(Value::Bool(x < y)),
                    Le => Ok(Value::Bool(x <= y)),
                    Gt => Ok(Value::Bool(x > y)),
                    Ge => Ok(Value::Bool(x >= y)),
                    _ => unreachable!(),
                }
            }
            (Value::Float(x), Value::Float(y)) => {
                let (x, y) = (*x, *y);
                match op {
                    Add => Ok(Value::Float(x + y)),
                    Sub => Ok(Value::Float(x - y)),
                    Mul => Ok(Value::Float(x * y)),
                    Div => {
                        if y == 0.0 {
                            fail("E0203", tr!("0으로 나눌 수 없습니다", "division by zero"), line, col)
                        } else {
                            Ok(Value::Float(x / y))
                        }
                    }
                    Mod => Ok(Value::Float(x % y)),
                    Lt => Ok(Value::Bool(x < y)),
                    Le => Ok(Value::Bool(x <= y)),
                    Gt => Ok(Value::Bool(x > y)),
                    Ge => Ok(Value::Bool(x >= y)),
                    _ => unreachable!(),
                }
            }
            (Value::Str(x), Value::Str(y)) => match op {
                Add => Ok(str_value(format!("{}{}", x, y))),
                Lt => Ok(Value::Bool(x < y)),
                Le => Ok(Value::Bool(x <= y)),
                Gt => Ok(Value::Bool(x > y)),
                Ge => Ok(Value::Bool(x >= y)),
                _ => fail(
                    "E0244",
                    tr!(format!("문자열에는 `{}`을(를) 쓸 수 없습니다", op.symbol()), format!("cannot use `{}` on strings", op.symbol())),
                    line,
                    col,
                ),
            },
            (Value::List(x), Value::List(y)) if op == Add => {
                let mut out = x.borrow().clone();
                out.extend(y.borrow().iter().cloned());
                Ok(list_value(out))
            }
            (Value::Int(_), Value::Float(_)) | (Value::Float(_), Value::Int(_)) => fail_fix(
                "E0201",
                tr!(format!("Int와 Float에 `{}`을(를) 바로 쓸 수 없습니다", op.symbol()), format!("cannot use `{}` on Int and Float directly", op.symbol())),
                line,
                col,
                tr!("Siskin에는 암묵적 형변환이 없습니다. `float(x)` 또는 `int(x)`로 맞추세요", "Siskin has no implicit conversions; convert with `float(x)` or `int(x)`"),
            ),
            _ => fail(
                "E0245",
                tr!(format!("{}와(과) {}에 `{}`을(를) 쓸 수 없습니다", a.type_name(), b.type_name(), op.symbol()), format!("cannot use `{2}` on {0} and {1}", a.type_name(), b.type_name(), op.symbol())),
                line,
                col,
            ),
        }
    }

    // ---------------------------------------------------------------- doctest

    /// Design doc §9.6. Actually runs the `>>>` examples in docstrings.
    pub fn run_doctests(&mut self, prog: &Program) -> (usize, usize, Vec<String>) {
        crate::conc::enter();
        self.collect(&prog.stmts);
        // Lines like `from std.re import find_all` at the top of the file must run first
        // so those names work inside doctests (main is not called).
        for s in &prog.stmts {
            if matches!(s, Stmt::Fn(_) | Stmt::Struct(_) | Stmt::Enum(_) | Stmt::Interface(_)) {
                continue;
            }
            let _ = self.exec(s);
        }
        let mut pass = 0;
        let mut fail_n = 0;
        let mut msgs = Vec::new();

        let mut docs: Vec<(String, String)> = Vec::new();
        for s in &prog.stmts {
            match s {
                Stmt::Fn(f) => {
                    if let Some(d) = &f.doc {
                        docs.push((f.name.clone(), d.clone()));
                    }
                }
                Stmt::Struct(sd) => {
                    for m in &sd.methods {
                        if let Some(d) = &m.doc {
                            docs.push((format!("{}.{}", sd.name, m.name), d.clone()));
                        }
                    }
                }
                _ => {}
            }
        }

        for (owner, doc) in docs {
            let lines: Vec<&str> = doc.lines().map(|l| l.trim()).collect();
            let mut i = 0;
            while i < lines.len() {
                if let Some(src) = lines[i].strip_prefix(">>>") {
                    let src = src.trim().to_string();
                    let expected = lines.get(i + 1).map(|s| s.trim().to_string()).unwrap_or_default();
                    i += 2;
                    if expected.is_empty() || expected.starts_with(">>>") {
                        continue;
                    }
                    match crate::parser::parse_expr_str(&src) {
                        Ok(e) => {
                            self.frames.push(Frame { scopes: vec![HashMap::new()], func: String::new(), cur_line: 0 });
                            let got = self.eval(&e);
                            self.frames.pop();
                            match got {
                                Ok(v) => {
                                    let actual = v.repr();
                                    if actual == expected {
                                        pass += 1;
                                    } else {
                                        fail_n += 1;
                                        msgs.push(tr!(format!(
                                            "  {}: `{}`\n    기대: {}\n    실제: {}",
                                            owner, src, expected, actual
                                        ), format!(
                                            "  {}: `{}`\n    expected: {}\n    got: {}",
                                            owner, src, expected, actual
                                        )));
                                    }
                                }
                                Err(Flow::Fail(err)) => {
                                    fail_n += 1;
                                    msgs.push(tr!(format!("  {}: `{}`\n    실행 오류: {}", owner, src, err), format!("  {}: `{}`\n    runtime error: {}", owner, src, err)));
                                }
                                Err(_) => {
                                    fail_n += 1;
                                    msgs.push(tr!(format!("  {}: `{}`\n    예상치 못한 제어 흐름", owner, src), format!("  {}: `{}`\n    unexpected control flow", owner, src)));
                                }
                            }
                        }
                        Err(err) => {
                            fail_n += 1;
                            msgs.push(tr!(format!("  {}: `{}`\n    문법 오류: {}", owner, src, err), format!("  {}: `{}`\n    syntax error: {}", owner, src, err)));
                        }
                    }
                    continue;
                }
                i += 1;
            }
        }
        (pass, fail_n, msgs)
    }
}

/// Reconstructs the condition expression in human-readable form for contract failure messages.
pub fn render_expr(e: &Expr) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Float(f) => format!("{}", f),
        Expr::Str(s) => format!("\"{}\"", s),
        Expr::Bool(b) => if *b { "true".into() } else { "false".into() },
        Expr::NoneLit => "none".into(),
        Expr::Ident(n, _, _) => n.clone(),
        Expr::Lambda(..) => "fn(...): ...".into(),
        Expr::Spawn(f, _, _) => match f.body.first() {
            Some(Stmt::Return(Some(e), _, _)) => format!("spawn {}", render_expr(e)),
            _ => "spawn ...".into(),
        },
        Expr::FString(_) => "f\"...\"".into(),
        Expr::List(items) => {
            let inner: Vec<String> = items.iter().map(render_expr).collect();
            format!("[{}]", inner.join(", "))
        }
        Expr::Tuple(items) => {
            let inner: Vec<String> = items.iter().map(render_expr).collect();
            format!("({})", inner.join(", "))
        }
        Expr::Dict(_) => "{...}".into(),
        Expr::Unary(op, a, _, _) => match op {
            UnOp::Neg => format!("-{}", render_expr(a)),
            UnOp::Not => format!("not {}", render_expr(a)),
        },
        Expr::Binary(op, a, b, _, _) => {
            format!("{} {} {}", render_expr(a), op.symbol(), render_expr(b))
        }
        Expr::Call { callee, args, .. } => {
            let inner: Vec<String> = args
                .iter()
                .map(|a| match &a.name {
                    Some(n) => format!("{}: {}", n, render_expr(&a.value)),
                    None => render_expr(&a.value),
                })
                .collect();
            format!("{}({})", render_expr(callee), inner.join(", "))
        }
        Expr::Field(o, n, _, _) => format!("{}.{}", render_expr(o), n),
        Expr::Index(o, i, _, _) => format!("{}[{}]", render_expr(o), render_expr(i)),
        Expr::IfExpr { cond, then, els } => format!(
            "{} if {} else {}",
            render_expr(then),
            render_expr(cond),
            render_expr(els)
        ),
        Expr::Try(inner, _, _) => format!("try {}", render_expr(inner)),
        Expr::OrElse(a, b, _, _) => format!("{} else {}", render_expr(a), render_expr(b)),
    }
}
