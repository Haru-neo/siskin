use crate::ast::FnDecl;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug)]
pub struct StructVal {
    pub name: String,
    pub fields: Vec<(String, Value)>,
}

#[derive(Debug)]
pub struct EnumVal {
    pub enum_name: String,
    pub variant: String,
    pub fields: Vec<(String, Value)>,
}

/// A function that captures outer values (a closure). Captured values are copies taken at creation time.
#[derive(Debug)]
pub struct Closure {
    pub decl: crate::ast::Shared<FnDecl>,
    pub env: Vec<(String, Value)>,
}

/// A value moved between tasks (threads).
///
/// Interpreter values are `Rc`, so two threads must never touch the same one. Therefore a value passed
/// between tasks (a task's result, a value sent through a channel) is rebuilt from scratch by the sender via `detach`,
/// and only values shared with nobody go into this box. The receiver either takes it out (`take`),
/// or, while holding the lock guarding the box, rebuilds a fresh copy (`copy`) to take away.
pub struct Moved(Value);

// Safety: the `Rc`s in the boxed value point only into this box (freshly built by `detach`). The box is only ever
// touched by one thread at a time (under the lock, or via ownership transfer), and the lock orders memory at the hand-off.
unsafe impl Send for Moved {}

impl Moved {
    pub fn new(v: &Value) -> Moved {
        Moved(v.detach())
    }
    pub fn take(self) -> Value {
        self.0
    }
    pub fn copy(&self) -> Value {
        self.0.detach()
    }
}

impl std::fmt::Debug for Moved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Moved(..)")
    }
}

/// A task created by `spawn`. Its result arrives when it finishes. A handle that several tasks can share.
#[derive(Debug)]
pub struct TaskCell {
    pub done: std::sync::atomic::AtomicBool,
    pub result: std::sync::Mutex<Option<Moved>>,
}

/// A channel created by `channel[T]()`. If `cap` is 0 it is unbounded.
/// State is only changed while holding the big `conc` lock (so no waiter is missed).
#[derive(Debug)]
pub struct ChanCell {
    pub state: std::sync::Mutex<ChanState>,
    pub cap: usize,
}

impl ChanCell {
    pub fn st(&self) -> std::sync::MutexGuard<'_, ChanState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Debug, Default)]
pub struct ChanState {
    pub q: std::collections::VecDeque<Moved>,
    pub closed: bool,
}

/// One chunk of actual memory that a raw pointer points to.
#[derive(Debug)]
pub struct RawBuf {
    pub data: Vec<Value>,
    /// false once `free`d. A marker for catching use-after-free.
    pub alive: bool,
    /// Whether the memory is owned by an arena (if so, individual free is forbidden).
    pub in_arena: bool,
}

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(Rc<String>),
    Bool(bool),
    None,
    List(Rc<RefCell<Vec<Value>>>),
    /// `(a, b, ...)` — tuple. Immutable, so no RefCell.
    Tuple(Rc<Vec<Value>>),
    Dict(Rc<RefCell<Vec<(Value, Value)>>>),
    Struct(Rc<RefCell<StructVal>>),
    Enum(Rc<RefCell<EnumVal>>),
    /// The error side of `!T`. Propagated by `try`, received by `catch`.
    Error(Rc<String>),
    /// The error side of an enum error type (`BankError!T`). Holds an enum value.
    ErrorOf(Rc<Value>),
    /// Prelude / standard library function
    Builtin(&'static str),
    /// A user function passed as a value. A value of a function type such as `(Int) -> Int`.
    Func(crate::ast::Shared<FnDecl>),
    /// A function that captures outer values, like `fn(x): x + k` or a `fn` inside a function.
    Closure(Rc<Closure>),
    /// Module name brought in by `import std.fs`
    Module(&'static str),
    /// Arena created by `with arena a:`
    Arena(Rc<RefCell<Vec<Rc<RefCell<RawBuf>>>>>),
    /// `*T` — raw pointer. A chunk and a position within it.
    Raw(Rc<RefCell<RawBuf>>, usize),
    /// A value handled by `std.json`.
    Json(crate::json::JRef),
    /// Task handle returned by `spawn`
    Task(std::sync::Arc<TaskCell>),
    /// Channel for passing values between tasks (a handle, so copies refer to the same channel)
    Chan(std::sync::Arc<ChanCell>),
}

/// Formats a float nicely. Appends `.0` if it is a whole number.
pub fn float_repr(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 {
        format!("{:.1}", f)
    } else {
        format!("{}", f)
    }
}

impl Value {
    pub fn type_name(&self) -> String {
        match self {
            Value::Int(_) => "Int".into(),
            Value::Float(_) => "Float".into(),
            Value::Str(_) => "Str".into(),
            Value::Bool(_) => "Bool".into(),
            Value::None => "none".into(),
            Value::List(_) => "List".into(),
            Value::Tuple(_) => "Tuple".into(),
            Value::Dict(_) => "Dict".into(),
            Value::Struct(s) => crate::ns::shown(&s.borrow().name),
            Value::Enum(e) => crate::ns::shown(&e.borrow().enum_name),
            Value::Error(_) | Value::ErrorOf(_) => "Error".into(),
            Value::Builtin(_) => "Fn".into(),
            Value::Func(_) => "Fn".into(),
            Value::Closure(_) => "Fn".into(),
            Value::Module(_) => "Module".into(),
            Value::Arena(_) => "Arena".into(),
            Value::Raw(_, _) => "Ptr".into(),
            Value::Json(_) => "Json".into(),
            Value::Task(_) => "Task".into(),
            Value::Chan(_) => "Chan".into(),
        }
    }

    /// Human-readable form. Used by `print` and `str()`.
    pub fn display(&self) -> String {
        match self {
            Value::Str(s) => s.as_ref().clone(),
            other => other.repr(),
        }
    }

    /// Debug/doctest form. Strings get quotes.
    pub fn repr(&self) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Float(f) => float_repr(*f),
            Value::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
            Value::Bool(b) => if *b { "true".into() } else { "false".into() },
            Value::None => "none".into(),
            Value::List(items) => {
                let inner: Vec<String> = items.borrow().iter().map(|v| v.repr()).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Tuple(items) => {
                let inner: Vec<String> = items.iter().map(|v| v.repr()).collect();
                format!("({})", inner.join(", "))
            }
            Value::Dict(pairs) => {
                let inner: Vec<String> = pairs
                    .borrow()
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.repr(), v.repr()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
            Value::Struct(s) => {
                let s = s.borrow();
                let inner: Vec<String> =
                    s.fields.iter().map(|(n, v)| format!("{}: {}", n, v.repr())).collect();
                format!("{}({})", crate::ns::plain(&s.name), inner.join(", "))
            }
            Value::Enum(e) => {
                let e = e.borrow();
                if e.fields.is_empty() {
                    crate::ns::plain(&e.variant).to_string()
                } else {
                    let inner: Vec<String> = e.fields.iter().map(|(_, v)| v.repr()).collect();
                    format!("{}({})", crate::ns::plain(&e.variant), inner.join(", "))
                }
            }
            Value::Error(m) => format!("error(\"{}\")", m),
            Value::ErrorOf(v) => format!("error({})", v.repr()),
            Value::Builtin(n) => tr!(format!("<내장 함수 {}>", n), format!("<builtin fn {}>", n)),
            Value::Func(f) => tr!(format!("<함수 {}>", crate::ns::shown(&f.name)), format!("<fn {}>", crate::ns::shown(&f.name))),
            Value::Closure(c) => tr!(format!("<함수 {}>", c.decl.shown_name()), format!("<fn {}>", c.decl.shown_name())),
            Value::Module(n) => tr!(format!("<모듈 {}>", n), format!("<module {}>", n)),
            Value::Json(j) => crate::json::stringify(j),
            Value::Arena(_) => tr!("<아레나>", "<arena>").into(),
            Value::Task(_) => tr!("<작업>", "<task>").into(),
            Value::Chan(_) => tr!("<통로>", "<channel>").into(),
            Value::Raw(b, off) => {
                if b.borrow().alive {
                    tr!(format!("<포인터 +{}>", off), format!("<pointer +{}>", off))
                } else {
                    tr!("<해제된 포인터>", "<freed pointer>").into()
                }
            }
        }
    }

    /// Value semantics (design doc §5): `let`/`var`/assignment copy the value.
    /// Function arguments are borrowed by default, so they are not copied.
    pub fn deep_clone(&self) -> Value {
        match self {
            Value::List(items) => {
                let copied: Vec<Value> = items.borrow().iter().map(|v| v.deep_clone()).collect();
                Value::List(Rc::new(RefCell::new(copied)))
            }
            Value::Tuple(items) => {
                let copied: Vec<Value> = items.iter().map(|v| v.deep_clone()).collect();
                Value::Tuple(Rc::new(copied))
            }
            Value::Dict(pairs) => {
                let copied: Vec<(Value, Value)> = pairs
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.deep_clone(), v.deep_clone()))
                    .collect();
                Value::Dict(Rc::new(RefCell::new(copied)))
            }
            Value::Struct(s) => {
                let s = s.borrow();
                Value::Struct(Rc::new(RefCell::new(StructVal {
                    name: s.name.clone(),
                    fields: s.fields.iter().map(|(n, v)| (n.clone(), v.deep_clone())).collect(),
                })))
            }
            Value::Enum(e) => {
                let e = e.borrow();
                Value::Enum(Rc::new(RefCell::new(EnumVal {
                    enum_name: e.enum_name.clone(),
                    variant: e.variant.clone(),
                    fields: e.fields.iter().map(|(n, v)| (n.clone(), v.deep_clone())).collect(),
                })))
            }
            other => other.clone(),
        }
    }

    /// A value to hand across tasks: like `deep_clone`, but rebuilds strings, errors, closures and JSON too,
    /// sharing no `Rc` at all with the original. Raw pointers and arenas cannot be passed
    /// (the type checker forbids it, T0075) — if one slips through it becomes none.
    pub fn detach(&self) -> Value {
        match self {
            Value::Str(s) => Value::Str(Rc::new(s.as_ref().clone())),
            Value::Error(s) => Value::Error(Rc::new(s.as_ref().clone())),
            Value::ErrorOf(v) => Value::ErrorOf(Rc::new(v.detach())),
            Value::List(items) => Value::List(Rc::new(RefCell::new(items.borrow().iter().map(|v| v.detach()).collect()))),
            Value::Tuple(items) => Value::Tuple(Rc::new(items.iter().map(|v| v.detach()).collect())),
            Value::Dict(pairs) => Value::Dict(Rc::new(RefCell::new(
                pairs.borrow().iter().map(|(k, v)| (k.detach(), v.detach())).collect(),
            ))),
            Value::Struct(s) => {
                let s = s.borrow();
                Value::Struct(Rc::new(RefCell::new(StructVal {
                    name: s.name.clone(),
                    fields: s.fields.iter().map(|(n, v)| (n.clone(), v.detach())).collect(),
                })))
            }
            Value::Enum(e) => {
                let e = e.borrow();
                Value::Enum(Rc::new(RefCell::new(EnumVal {
                    enum_name: e.enum_name.clone(),
                    variant: e.variant.clone(),
                    fields: e.fields.iter().map(|(n, v)| (n.clone(), v.detach())).collect(),
                })))
            }
            Value::Closure(c) => Value::Closure(Rc::new(Closure {
                decl: c.decl.clone(),
                env: c.env.iter().map(|(n, v)| (n.clone(), v.detach())).collect(),
            })),
            Value::Json(j) => Value::Json(crate::json::detach(j)),
            Value::Raw(..) | Value::Arena(_) => Value::None,
            Value::Int(_)
            | Value::Float(_)
            | Value::Bool(_)
            | Value::None
            | Value::Builtin(_)
            | Value::Func(_)
            | Value::Module(_)
            | Value::Task(_)
            | Value::Chan(_) => self.clone(),
        }
    }

    pub fn eq_value(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::None, Value::None) => true,
            (Value::Error(a), Value::Error(b)) => a == b,
            (Value::ErrorOf(a), Value::ErrorOf(b)) => a.eq_value(b),
            (Value::List(a), Value::List(b)) => {
                let (a, b) = (a.borrow(), b.borrow());
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.eq_value(y))
            }
            (Value::Tuple(a), Value::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.eq_value(y))
            }
            (Value::Struct(a), Value::Struct(b)) => {
                let (a, b) = (a.borrow(), b.borrow());
                a.name == b.name
                    && a.fields.len() == b.fields.len()
                    && a.fields.iter().zip(b.fields.iter()).all(|(x, y)| x.1.eq_value(&y.1))
            }
            (Value::Dict(a), Value::Dict(b)) => {
                // Equal if the same keys have the same values, regardless of order (same rule as native).
                let (a, b) = (a.borrow(), b.borrow());
                a.len() == b.len()
                    && a.iter().all(|(k, v)| b.iter().any(|(k2, v2)| k.eq_value(k2) && v.eq_value(v2)))
            }
            (Value::Json(a), Value::Json(b)) => crate::json::stringify(a) == crate::json::stringify(b),
            (Value::Enum(a), Value::Enum(b)) => {
                let (a, b) = (a.borrow(), b.borrow());
                a.enum_name == b.enum_name
                    && a.variant == b.variant
                    && a.fields.iter().zip(b.fields.iter()).all(|(x, y)| x.1.eq_value(&y.1))
            }
            _ => false,
        }
    }
}

pub fn str_value(s: impl Into<String>) -> Value {
    Value::Str(Rc::new(s.into()))
}

/// f-string format spec: `{x:[[fill]align][0][width][.prec][type]}`.
/// A practical subset of Python's format mini-language. Shared by cgen and interp.
#[derive(Debug, Clone)]
pub struct FmtSpec {
    pub fill: char,
    pub align: Option<char>, // '<' '>' '^'
    pub width: usize,
    pub prec: Option<usize>,
    pub ty: Option<char>, // 'f' 'd' 's' 'x' 'X'
}

pub fn parse_spec(spec: &str) -> FmtSpec {
    let cs: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let mut fill = ' ';
    let mut align = None;
    if cs.len() >= 2 && matches!(cs[1], '<' | '>' | '^') {
        fill = cs[0];
        align = Some(cs[1]);
        i = 2;
    } else if !cs.is_empty() && matches!(cs[0], '<' | '>' | '^') {
        align = Some(cs[0]);
        i = 1;
    }
    if i < cs.len() && cs[i] == '0' {
        fill = '0';
        if align.is_none() {
            align = Some('>');
        }
        i += 1;
    }
    let mut width = 0usize;
    while i < cs.len() && cs[i].is_ascii_digit() {
        width = width * 10 + (cs[i] as usize - '0' as usize);
        i += 1;
    }
    let mut prec = None;
    if i < cs.len() && cs[i] == '.' {
        i += 1;
        let mut p = 0usize;
        while i < cs.len() && cs[i].is_ascii_digit() {
            p = p * 10 + (cs[i] as usize - '0' as usize);
            i += 1;
        }
        prec = Some(p);
    }
    let ty = if i < cs.len() { Some(cs[i]) } else { None };
    FmtSpec { fill, align, width, prec, ty }
}

/// If the format spec has a part that cannot be parsed, returns what the problem is.
/// (If silently ignored, nobody would notice a misaligned table.)
pub fn spec_problem(spec: &str) -> Option<String> {
    if spec.contains('{') {
        return Some(tr!("서식 안에는 `{w}` 같은 변수를 쓸 수 없습니다", "variables like `{w}` cannot be used inside a format spec").into());
    }
    let cs: Vec<char> = spec.chars().collect();
    let mut i = 0;
    if cs.len() >= 2 && matches!(cs[1], '<' | '>' | '^') {
        i = 2;
    } else if !cs.is_empty() && matches!(cs[0], '<' | '>' | '^') {
        i = 1;
    }
    while i < cs.len() && cs[i].is_ascii_digit() {
        i += 1;
    }
    if i < cs.len() && cs[i] == '.' {
        i += 1;
        while i < cs.len() && cs[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < cs.len() && matches!(cs[i], 'f' | 'd' | 's' | 'x' | 'X') {
        i += 1;
    }
    if i < cs.len() {
        return Some(tr!(format!("서식 `{}` 의 `{}` 부분을 읽을 수 없습니다", spec, cs[i..].iter().collect::<String>()), format!("cannot parse `{1}` in format spec `{0}`", spec, cs[i..].iter().collect::<String>())));
    }
    None
}

/// Applies width/alignment/fill to a string. Width counts characters (same as Str.len).
pub fn pad_spec(s: &str, fs: &FmtSpec, numeric: bool) -> String {
    let len = s.chars().count();
    if len >= fs.width {
        return s.to_string();
    }
    let pad = fs.width - len;
    let align = fs.align.unwrap_or(if numeric { '>' } else { '<' });
    let f: String = fs.fill.to_string();
    match align {
        '<' => format!("{}{}", s, f.repeat(pad)),
        '^' => {
            let l = pad / 2;
            format!("{}{}{}", f.repeat(l), s, f.repeat(pad - l))
        }
        _ => format!("{}{}", f.repeat(pad), s),
    }
}

/// interp side: formats a single value into a string.
pub fn format_value(v: &Value, spec: &str) -> String {
    let fs = parse_spec(spec);
    let numeric = matches!(v, Value::Int(_) | Value::Float(_));
    let base = format_base(v, &fs);
    pad_spec(&base, &fs, numeric)
}

fn format_base(v: &Value, fs: &FmtSpec) -> String {
    // Decimal places: when the type is f, or when prec is given and the value is numeric.
    let want_prec = fs.ty == Some('f') || (fs.prec.is_some() && matches!(v, Value::Float(_) | Value::Int(_)));
    if want_prec {
        let x = match v {
            Value::Float(f) => *f,
            Value::Int(n) => *n as f64,
            _ => return v.display(),
        };
        return format!("{:.*}", fs.prec.unwrap_or(6), x);
    }
    match (fs.ty, v) {
        (Some('x'), Value::Int(n)) => format!("{:x}", n),
        (Some('X'), Value::Int(n)) => format!("{:X}", n),
        _ => v.display(),
    }
}

pub fn list_value(items: Vec<Value>) -> Value {
    Value::List(Rc::new(RefCell::new(items)))
}
