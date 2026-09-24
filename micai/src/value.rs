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

/// 바깥 값을 붙잡은 함수(클로저). 붙잡은 값은 만들 때 복사해 둔 것입니다.
#[derive(Debug)]
pub struct Closure {
    pub decl: crate::ast::Shared<FnDecl>,
    pub env: Vec<(String, Value)>,
}

/// 작업(스레드) 사이로 옮기는 값.
///
/// 인터프리터의 값은 `Rc` 라서 두 스레드가 같이 만지면 안 됩니다. 그래서 작업 사이로
/// 넘기는 값(작업의 결과, 통로로 보내는 값)은 보내는 쪽이 `detach` 로 통째로 새로 만들어,
/// 아무와도 공유하지 않는 값만 이 상자에 담습니다. 받는 쪽은 꺼내 가거나(`take`),
/// 상자를 지키는 자물쇠를 쥔 채 다시 새로 만들어(`copy`) 가져갑니다.
pub struct Moved(Value);

// 안전: 상자 속 값의 `Rc` 들은 이 상자만 가리킵니다(`detach` 로 새로 만든 것). 상자는 언제나
// 한 스레드만 만지고(자물쇠 안이나 소유권 이동), 옮기는 순간 자물쇠가 메모리 순서를 맞춰 줍니다.
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

/// `spawn` 이 만든 작업. 끝나면 결과가 들어옵니다. 여러 작업이 같이 보는 손잡이입니다.
#[derive(Debug)]
pub struct TaskCell {
    pub done: std::sync::atomic::AtomicBool,
    pub result: std::sync::Mutex<Option<Moved>>,
}

/// `channel[T]()` 가 만든 통로. `cap` 이 0 이면 끝없이 쌓입니다.
/// 상태는 `conc` 의 큰 자물쇠를 쥔 채로만 바꿉니다(기다리는 쪽을 놓치지 않게).
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

/// 원시 포인터가 가리키는 실제 메모리 한 덩어리.
#[derive(Debug)]
pub struct RawBuf {
    pub data: Vec<Value>,
    /// `free` 하면 false. 해제 후 접근을 잡아내기 위한 표시입니다.
    pub alive: bool,
    /// 아레나가 소유한 메모리인가 (그렇다면 개별 free 금지).
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
    /// `(a, b, ...)` — 튜플. 불변이라 RefCell이 없습니다.
    Tuple(Rc<Vec<Value>>),
    Dict(Rc<RefCell<Vec<(Value, Value)>>>),
    Struct(Rc<RefCell<StructVal>>),
    Enum(Rc<RefCell<EnumVal>>),
    /// `!T`의 에러 쪽. `try`가 전파하고 `catch`가 받습니다.
    Error(Rc<String>),
    /// enum 오류 타입(`BankError!T`)의 에러 쪽. 안에 enum 값이 들어 있습니다.
    ErrorOf(Rc<Value>),
    /// 프렐류드/표준 라이브러리 함수
    Builtin(&'static str),
    /// 값으로 넘긴 사용자 함수. `(Int) -> Int` 같은 함수 타입의 값입니다.
    Func(crate::ast::Shared<FnDecl>),
    /// `fn(x): x + k` 나 함수 안의 `fn` 처럼 바깥 값을 붙잡은 함수.
    Closure(Rc<Closure>),
    /// `import std.fs` 로 들어온 모듈 이름
    Module(&'static str),
    /// `with arena a:` 가 만든 아레나
    Arena(Rc<RefCell<Vec<Rc<RefCell<RawBuf>>>>>),
    /// `*T` — 원시 포인터. 덩어리와 그 안의 위치.
    Raw(Rc<RefCell<RawBuf>>, usize),
    /// `std.json` 이 다루는 값.
    Json(crate::json::JRef),
    /// `spawn` 이 돌려준 작업 손잡이
    Task(std::sync::Arc<TaskCell>),
    /// 작업끼리 값을 주고받는 통로(손잡이라 복사해도 같은 통로)
    Chan(std::sync::Arc<ChanCell>),
}

/// 실수를 보기 좋게. 정수처럼 딱 떨어지면 `.0` 을 붙입니다.
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

    /// 사람이 보는 형태. `print`와 `str()`이 씁니다.
    pub fn display(&self) -> String {
        match self {
            Value::Str(s) => s.as_ref().clone(),
            other => other.repr(),
        }
    }

    /// 디버그/doctest 형태. 문자열에 따옴표가 붙습니다.
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

    /// 값 의미론(설계 문서 §5): `let`/`var`/대입에서 값을 복사합니다.
    /// 함수 인자는 기본이 빌림이라 복사하지 않습니다.
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

    /// 작업 사이로 넘길 값: `deep_clone` 과 같지만 글자·오류·클로저·JSON 까지 모두 새로 만들어
    /// 원래 값과 `Rc` 를 하나도 나누지 않습니다. 원시 포인터와 아레나는 넘길 수 없습니다
    /// (타입 검사가 막음, T0075) — 혹시 오면 none 이 됩니다.
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
                // 순서와 상관없이 같은 키에 같은 값이 있으면 같습니다 (네이티브와 같은 규칙).
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

/// f-string 서식 스펙: `{x:[[fill]align][0][width][.prec][type]}`.
/// 파이썬 서식의 실용적인 부분집합입니다. cgen과 interp가 함께 씁니다.
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

/// 서식 스펙에 읽을 수 없는 부분이 있으면 무엇이 문제인지 돌려줍니다.
/// (조용히 무시하면 표가 어긋나도 아무도 모릅니다.)
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

/// 폭/정렬/채움을 문자열에 적용합니다. 폭은 글자 수 기준입니다(Str.len과 같음).
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

/// interp 쪽: 값 하나에 서식을 적용해 문자열을 만듭니다.
pub fn format_value(v: &Value, spec: &str) -> String {
    let fs = parse_spec(spec);
    let numeric = matches!(v, Value::Int(_) | Value::Float(_));
    let base = format_base(v, &fs);
    pad_spec(&base, &fs, numeric)
}

fn format_base(v: &Value, fs: &FmtSpec) -> String {
    // 소수 자릿수: 타입이 f이거나, prec가 있고 값이 수일 때.
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
