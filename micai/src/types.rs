//! P2 타입 검사기.
//!
//! P1 인터프리터는 타입 표기를 파싱만 하고 무시했습니다. 여기서부터는
//! 실행 전에 검사합니다. 이것이 있어야 P3 네이티브 코드 생성이 가능합니다.
//! (타입을 알아야 C의 `long long`인지 `double`인지 정할 수 있습니다.)

use crate::ast::*;
use crate::error::SiskinError;
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Siskin 이 이미 가지고 있는 이름들. C 헤더에 같은 이름이 있어도
/// Siskin 쪽이 이깁니다 (`abs`, `exit`, `free`, `pow` 처럼 겹치는 게 많습니다).
pub const SISKIN_BUILTINS: &[&str] = &[
    "print", "eprint", "len", "range", "str", "input", "args", "exit", "int", "float", "error", "assert",
    "abs", "min", "max", "sqrt", "sin", "cos", "tan", "log", "log10", "exp", "pi", "e", "now",
    "clock", "rand", "seed", "rand_int", "exists", "append_text", "remove", "sum", "parse",
    "stringify", "jnull", "jlist", "jdict", "jbool", "jint", "jfloat", "jstr", "test", "find",
    "find_all", "groups", "split_re", "replace", "round", "floor", "ceil", "pow", "read_text",
    "write_text", "alloc", "free", "cast", "main", "cstr", "ptr_get", "sleep", "env", "set_env",
    "cwd", "set_cwd", "pid", "list_dir", "make_dir", "is_dir", "channel",
];

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    Str,
    /// `none` 리터럴 자체의 타입
    NoneTy,
    Unit,
    List(Box<Ty>),
    Dict(Box<Ty>, Box<Ty>),
    Struct(String),
    Enum(String),
    Optional(Box<Ty>),
    /// 실패할 수 있는 값: (성공 타입, 오류 타입). `!T` 는 오류 타입이 Str.
    Fallible(Box<Ty>, Box<Ty>),
    /// `*T` — 원시 포인터 (메모리 Level 2)
    Raw(Box<Ty>),
    /// `with arena a:` 가 만드는 아레나 (메모리 Level 1)
    Arena,
    /// `std.json` 의 값
    Json,
    /// `(T, U, ...)` — 튜플
    Tuple(Vec<Ty>),
    /// `(A, B) -> R` — 함수 값의 타입
    Fn(Vec<Ty>, Box<Ty>),
    /// `fn f[T](...)` 의 `T` 같은 타입 매개변수. 호출 때 실제 타입으로 채워집니다.
    Var(String),
    /// `spawn f(x)` 가 돌려주는 작업 손잡이. `t.wait()` 가 f 의 결과를 줍니다.
    Task(Box<Ty>),
    /// `channel[T]()` 로 만든 통로. 작업끼리 값을 주고받습니다(보낼 때 복사).
    Chan(Box<Ty>),
    /// 아직 결정되지 않음 (빈 리스트 등). 무엇과도 맞습니다.
    Unknown,
}

impl Ty {
    /// 오류 값이 글자인 보통의 `!T`.
    pub fn fallible(t: Ty) -> Ty {
        Ty::Fallible(Box::new(t), Box::new(Ty::Str))
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Int => write!(f, "Int"),
            Ty::Float => write!(f, "Float"),
            Ty::Bool => write!(f, "Bool"),
            Ty::Str => write!(f, "Str"),
            Ty::NoneTy => write!(f, "none"),
            Ty::Unit => write!(f, "()"),
            Ty::List(t) => write!(f, "[{}]", t),
            Ty::Dict(k, v) => write!(f, "{{{}: {}}}", k, v),
            Ty::Struct(n) => write!(f, "{}", n),
            Ty::Enum(n) => write!(f, "{}", n),
            Ty::Optional(t) => write!(f, "?{}", t),
            Ty::Fallible(t, e) if **e == Ty::Str || **e == Ty::Unknown => write!(f, "!{}", t),
            Ty::Fallible(t, e) => write!(f, "{}!{}", e, t),
            Ty::Raw(t) => write!(f, "*{}", t),
            Ty::Arena => write!(f, "Arena"),
            Ty::Json => write!(f, "Json"),
            Ty::Tuple(ts) => {
                let inner: Vec<String> = ts.iter().map(|t| t.to_string()).collect();
                write!(f, "({})", inner.join(", "))
            }
            Ty::Fn(ps, r) => {
                let inner: Vec<String> = ps.iter().map(|t| t.to_string()).collect();
                write!(f, "({}) -> {}", inner.join(", "), r)
            }
            Ty::Var(n) => write!(f, "{}", n),
            Ty::Task(t) => write!(f, "Task[{}]", t),
            Ty::Chan(t) => write!(f, "Chan[{}]", t),
            Ty::Unknown => write!(f, "_"),
        }
    }
}

impl Ty {
    pub fn is_numeric(&self) -> bool {
        matches!(self, Ty::Int | Ty::Float)
    }
}

#[derive(Debug, Clone)]
pub struct FnSig {
    pub params: Vec<(String, Ty)>,
    pub ret: Ty,
    pub decl: crate::ast::Shared<FnDecl>,
}

pub struct Types {
    pub fns: HashMap<String, FnSig>,
    pub structs: HashMap<String, crate::ast::Shared<StructDecl>>,
    pub enums: HashMap<String, crate::ast::Shared<EnumDecl>>,
    pub variant_of: HashMap<String, String>,
    pub methods: HashMap<String, FnSig>,
    scopes: Vec<HashMap<String, Ty>>,
    pub errors: Vec<SiskinError>,
    cur_ret: Ty,
    cur_fn: String,
    imported: HashSet<String>,
    /// `extern "C" fn ...` 으로 선언된 함수 이름들.
    pub externs: HashSet<String>,
    /// `extern "C" link "..."` 으로 적힌 라이브러리 이름들.
    pub links: Vec<String>,
    /// 헤더에 있지만 자동으로 못 가져온 함수: 이름 -> (헤더, 이유)
    pub c_skipped: std::collections::HashMap<String, (String, String)>,
    /// `unsafe:` 안인가. 원시 포인터 연산은 여기서만 됩니다.
    unsafe_depth: usize,
    /// `with arena:` 안인가.
    arena_depth: usize,
    /// 아레나에서 나온 값의 이름들. 블록 밖으로 내보낼 수 없습니다.
    tainted: HashSet<String>,
    /// 열려 있는 `with arena` 블록마다, 블록이 시작할 때의 스코프 깊이.
    arena_bases: Vec<usize>,
    /// 바꿀 수 있는 이름들(scopes와 짝을 이룸). `var`, `inout`/`owned` 인자,
    /// `inout self`가 여기 들어갑니다. `let`과 읽기 전용 인자는 안 들어갑니다.
    mutables: Vec<HashSet<String>>,
    /// 지금 검사 중인 함수의 타입 매개변수들(`fn f[T, U]`의 T, U).
    /// `resolve`가 이 안의 이름을 만나면 `Ty::Var`로 봅니다.
    cur_generics: HashSet<String>,
    /// 이미 알린 "없는 타입" 이름. 같은 이름을 쓰는 곳마다 되풀이하지 않습니다.
    reported_types: HashSet<String>,
    /// 네이티브 단형화(monomorphization)에서 타입 매개변수의 실제 타입.
    /// 비어 있으면(타입 검사 중) `resolve`는 `Ty::Var`를 냅니다.
    pub mono_subst: HashMap<String, Ty>,
    /// 익명 함수 인자 타입을 적지 않았을 때 쓸 "들어갈 자리의 타입".
    /// `map(xs, fn(x): x * 2)` 에서 `x` 가 Int 인 것을 여기서 압니다.
    lambda_hint: Option<Ty>,
    /// 익명 함수마다 확정된 (인자 타입들, 반환 타입). 선언 주소로 찾습니다.
    /// 네이티브 코드 생성이 인자 타입을 적지 않은 익명 함수를 만들 때 씁니다.
    pub lambda_sigs: HashMap<usize, (Vec<Ty>, Ty)>,
    /// 클로저 본문을 검사 중이면 그 클로저가 시작한 스코프 깊이.
    /// 이보다 바깥 스코프의 이름은 붙잡은 값이라 읽기만 됩니다.
    closure_bases: Vec<usize>,
    /// 지금 검사(또는 코드 생성) 중인 함수. 가드 절 뒤의 필드 좁히기가
    /// 함수 나머지에서 `inout` 으로 바뀌는지 볼 때 씁니다.
    pub cur_decl: Option<crate::ast::Shared<FnDecl>>,
    /// `closure_bases` 와 짝: 그 클로저가 `spawn` 이 만든 작업 본문인가.
    spawn_bodies: Vec<bool>,
}

/// `math.sqrt(x)` 처럼 모듈 이름으로 부를 수 있는 표준 모듈인가.
pub fn is_std_module(m: &str) -> bool {
    matches!(m, "math" | "fs" | "io" | "time" | "random" | "re" | "json" | "process" | "net")
}

fn err(code: &'static str, msg: impl Into<String>, line: usize, col: usize) -> SiskinError {
    SiskinError::new(code, msg, line, col)
}

/// 대입 대상의 뿌리 이름을 찾습니다. `x`, `x.f`, `x[i]`, `x.f[i].g` 모두 `x`.
fn root_ident(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n, _, _) => Some(n),
        Expr::Field(o, _, _, _) => root_ident(o),
        Expr::Index(o, _, _, _) => root_ident(o),
        _ => None,
    }
}

/// `t.due`, `a.b.c` 처럼 이름과 필드만으로 된 식을 `"t.due"` 글자로 바꿉니다.
/// 구조체 필드 좁히기(`if t.due != none:`)의 열쇠로 씁니다. 스코프에 이 글자로
/// 좁혀진 타입을 넣어 두는데, 점이 들어 있어 보통 이름과 겹치지 않습니다.
pub fn field_path(e: &Expr) -> Option<String> {
    match e {
        Expr::Ident(n, _, _) => Some(n.clone()),
        Expr::Field(o, f, _, _) => field_path(o).map(|p| format!("{}.{}", p, f)),
        _ => None,
    }
}

/// 대입·`inout` 이 실제로 바꾸는 경로. `t.xs[0] = ..` 는 `t.xs` 를 바꿉니다.
fn write_path(e: &Expr) -> Option<String> {
    match e {
        Expr::Ident(n, _, _) => Some(n.clone()),
        Expr::Field(o, f, _, _) => write_path(o).map(|p| format!("{}.{}", p, f)),
        Expr::Index(o, _, _, _) => write_path(o),
        _ => None,
    }
}

/// 한쪽이 `?T` 라서 연산이 안 될 때의 안내. 먼저 none 인지 확인하라고 알려 줍니다.
fn optional_fix(a: &Ty, b: &Ty) -> Option<String> {
    let t = if matches!(a, Ty::Optional(_)) { a } else if matches!(b, Ty::Optional(_)) { b } else { return None };
    Some(tr!(
        format!("{} 값은 none 일 수 있습니다. 먼저 `if x != none:` 로 확인하거나(구조체 필드도 `if t.due != none:`), `x else 0` 처럼 기본값을 주세요. 확인한 뒤 `inout` 으로 넘기거나 다시 대입하면 확인이 풀립니다", t),
        format!("a {} value may be none; check it first with `if x != none:` (struct fields too: `if t.due != none:`), or give a default like `x else 0`. Passing it as `inout` or reassigning it after the check cancels the check", t)
    ))
}

/// `w` 가 `k` 의 바깥 경로인가 (`t` 는 `t.due` 의, `a.b` 는 `a.b.c` 의).
fn is_outer_path(w: &str, k: &str) -> bool {
    k.len() > w.len() && k.starts_with(w) && k.as_bytes()[w.len()] == b'.'
}

/// 좁히기를 지켜야 하는 범위. 이 안에서 `inout` 으로 바뀔 수 있으면 좁히지 않습니다.
pub enum Region<'a> {
    Block(&'a [Stmt]),
    Expr(&'a Expr),
    /// 가드 절(`if t.due == none: return`) 뒤: 지금 함수의 이 줄 이후 전부.
    After(usize),
}

/// 문장들 안에서 `inout` 으로 넘겨지는 경로를 (경로, 줄) 로 모읍니다.
/// `inout` 인자를 가진 함수·메서드 이름(`inout`)만 봅니다. 이름만 보고 판단하므로
/// 같은 이름의 다른 함수까지 조심스럽게 셉니다.
fn call_writes_stmts(stmts: &[Stmt], inout: &HashSet<String>, out: &mut Vec<(String, usize)>) {
    for s in stmts {
        call_writes_stmt(s, inout, out);
    }
}

fn call_writes_stmt(s: &Stmt, inout: &HashSet<String>, out: &mut Vec<(String, usize)>) {
    let catch = |c: &Option<CatchClause>, out: &mut Vec<(String, usize)>| {
        if let Some(c) = c {
            call_writes_stmts(&c.body, inout, out);
        }
    };
    match s {
        Stmt::Let { value, catch: c, .. } => {
            call_writes_expr(value, inout, out);
            catch(c, out);
        }
        Stmt::LetTuple { value, .. } => call_writes_expr(value, inout, out),
        Stmt::Assign { target, value, catch: c, .. } => {
            call_writes_expr(target, inout, out);
            call_writes_expr(value, inout, out);
            catch(c, out);
        }
        Stmt::Expr(e, c) => {
            call_writes_expr(e, inout, out);
            catch(c, out);
        }
        Stmt::If { arms, els } => {
            for (c, b) in arms {
                call_writes_expr(c, inout, out);
                call_writes_stmts(b, inout, out);
            }
            if let Some(b) = els {
                call_writes_stmts(b, inout, out);
            }
        }
        Stmt::While { cond, body } => {
            call_writes_expr(cond, inout, out);
            call_writes_stmts(body, inout, out);
        }
        Stmt::For { iter, body, .. } => {
            call_writes_expr(iter, inout, out);
            call_writes_stmts(body, inout, out);
        }
        Stmt::Match { subject, cases, .. } => {
            call_writes_expr(subject, inout, out);
            for c in cases {
                call_writes_stmts(&c.body, inout, out);
            }
        }
        Stmt::Return(Some(e), _, _) => call_writes_expr(e, inout, out),
        Stmt::Arena { body, .. } | Stmt::Unsafe { body, .. } => call_writes_stmts(body, inout, out),
        _ => {}
    }
}

fn call_writes_expr(e: &Expr, inout: &HashSet<String>, out: &mut Vec<(String, usize)>) {
    match e {
        Expr::Call { callee, args, line, .. } => {
            let name = match callee.as_ref() {
                Expr::Ident(n, _, _) => Some(n.as_str()),
                Expr::Field(_, m, _, _) => Some(m.as_str()),
                _ => None,
            };
            if name.map_or(false, |n| inout.contains(n)) {
                if let Expr::Field(recv, _, _, _) = callee.as_ref() {
                    if let Some(p) = write_path(recv) {
                        out.push((p, *line));
                    }
                }
                for a in args {
                    if let Some(p) = write_path(&a.value) {
                        out.push((p, *line));
                    }
                }
            }
            call_writes_expr(callee, inout, out);
            for a in args {
                call_writes_expr(&a.value, inout, out);
            }
        }
        Expr::Unary(_, a, _, _) | Expr::Field(a, _, _, _) | Expr::Try(a, _, _) => call_writes_expr(a, inout, out),
        Expr::Binary(_, a, b, _, _) | Expr::Index(a, b, _, _) | Expr::OrElse(a, b, _, _) => {
            call_writes_expr(a, inout, out);
            call_writes_expr(b, inout, out);
        }
        Expr::IfExpr { cond, then, els } => {
            call_writes_expr(cond, inout, out);
            call_writes_expr(then, inout, out);
            call_writes_expr(els, inout, out);
        }
        Expr::List(xs) | Expr::Tuple(xs) => {
            for x in xs {
                call_writes_expr(x, inout, out);
            }
        }
        Expr::Dict(kvs) => {
            for (k, v) in kvs {
                call_writes_expr(k, inout, out);
                call_writes_expr(v, inout, out);
            }
        }
        Expr::FString(parts) => {
            for p in parts {
                if let FStrPart::Expr(x, _) = p {
                    call_writes_expr(x, inout, out);
                }
            }
        }
        _ => {}
    }
}

/// 문장들 안의 대입(`경로 = 값`)을 모읍니다. 반복문 앞에서 좁히기를 풀지 정할 때 씁니다.
fn assign_writes(stmts: &[Stmt], out: &mut Vec<(String, Expr)>) {
    for s in stmts {
        match s {
            Stmt::Assign { target, value, catch, .. } => {
                if let Some(p) = write_path(target) {
                    // `x = f() catch e:` 는 값 타입을 여기서 알기 어려우니 늘 "없을 수 있음"으로 봅니다.
                    let v = if catch.is_some() { Expr::NoneLit } else { value.clone() };
                    out.push((p, v));
                }
                if let Some(c) = catch {
                    assign_writes(&c.body, out);
                }
            }
            Stmt::Let { catch: Some(c), .. } | Stmt::Expr(_, Some(c)) => assign_writes(&c.body, out),
            Stmt::If { arms, els } => {
                for (_, b) in arms {
                    assign_writes(b, out);
                }
                if let Some(b) = els {
                    assign_writes(b, out);
                }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::Arena { body, .. } | Stmt::Unsafe { body, .. } => {
                assign_writes(body, out)
            }
            Stmt::Match { cases, .. } => {
                for c in cases {
                    assign_writes(&c.body, out);
                }
            }
            _ => {}
        }
    }
}

/// `a.alloc[T](n)` 같은 호출에서 실제 대상 식을 꺼냅니다.
fn value_arena_src(e: &Expr) -> Option<&Expr> {
    match e {
        Expr::Call { callee, .. } => Some(callee),
        _ => None,
    }
}

impl Types {
    /// 오류를 내지 않고 타입만 살짝 봅니다 (오염 추적용).
    fn infer_peek(&self, e: &Expr) -> Option<Ty> {
        match e {
            Expr::Ident(n, _, _) => self.lookup(n),
            _ => None,
        }
    }

    /// 타입만 알아내고 그 과정에서 생긴 오류는 버립니다.
    /// 진짜 오류는 나중에 정식 `infer`가 한 번만 냅니다.
    /// `let x = f() catch e:` 의 catch 블록을 검사합니다. 블록은 빠져나가거나
    /// (return / break / continue), 마지막 줄에 x 대신 넣을 값을 적어야 합니다.
    fn check_catch_value(&mut self, c: &CatchClause, name: &str, want: &Ty, err_ty: &Ty, line: usize, col: usize) {
        self.push_scope();
        self.declare(&c.name, err_ty.clone());
        for s in &c.body {
            self.check_stmt(s);
        }
        let leaves = crate::ast::block_leaves(&c.body);
        let fallback = crate::ast::catch_fallback(c).map(|e| (e.clone(), self.infer_quiet(e)));
        self.pop_scope();
        if leaves {
            return;
        }
        match fallback {
            Some((_, Ty::Unit)) | None => {
                self.errors.push(
                    err(
                        "T0059",
                        tr!(
                            format!("`catch` 블록이 끝나고 나면 `{}` 에 넣을 값이 없습니다", name),
                            format!("the `catch` block ends without a value for `{}`", name)
                        ),
                        line,
                        col,
                    )
                    .with_fix(tr!(
                        format!(
                            "블록 끝에서 `return` / `continue` / `break` 로 빠져나가거나, 마지막 줄에 `{}` 대신 쓸 값을 적으세요 (예: `0`, `\"\"`)",
                            name
                        ),
                        format!(
                            "leave the block with `return` / `continue` / `break`, or end it with a value to use for `{}` instead (e.g. `0`, `\"\"`)",
                            name
                        )
                    )),
                );
            }
            Some((e, ft)) => {
                if *want != Ty::Unknown && ft != Ty::Unknown && !self.compatible(want, &ft) {
                    let (l, cc) = e.pos();
                    self.errors.push(
                        err(
                            "T0060",
                            tr!(
                                format!("`catch` 블록의 마지막 값은 {} 인데 `{}` 은(는) {} 입니다", ft, name, want),
                                format!("the `catch` block ends with {}, but `{}` is {}", ft, name, want)
                            ),
                            if l > 0 { l } else { line },
                            if l > 0 { cc } else { col },
                        )
                        .with_fix(tr!(
                            format!("실패했을 때 대신 쓸 {} 값을 적으세요", want),
                            format!("end the block with a value of type {} to use on failure", want)
                        )),
                    );
                }
            }
        }
    }

    pub fn infer_quiet_pub(&mut self, e: &Expr) -> Ty {
        self.infer_quiet(e)
    }

    fn infer_quiet(&mut self, e: &Expr) -> Ty {
        let saved = self.errors.len();
        let t = self.infer(e);
        self.errors.truncate(saved);
        t
    }

    /// 대입 대상이 실제로 어떤 바인딩의 값을 바꾸는지 봅니다.
    /// 포인터를 거쳐 쓰는 경우(`p[i] = ...`, `p`가 `*T`)는 그 포인터가
    /// 가리키는 메모리를 바꿀 뿐 포인터 바인딩 자체는 그대로이므로 None입니다.
    /// 반대로 리스트·딕셔너리·구조체는 값이라, 원소를 바꾸면 바인딩의 값이 바뀝니다.
    fn mutated_binding(&mut self, target: &Expr) -> Option<String> {
        match target {
            Expr::Ident(n, _, _) => Some(n.clone()),
            Expr::Field(o, _, _, _) => self.mutated_binding(o),
            Expr::Index(o, _, _, _) => {
                if matches!(self.infer_quiet(o), Ty::Raw(_)) {
                    None
                } else {
                    self.mutated_binding(o)
                }
            }
            _ => None,
        }
    }

    /// `inout` 인자로 넘길 수 있는지 봅니다. 바꿀 수 있는 lvalue여야 합니다.
    /// 리터럴·계산식(바꿀 곳이 없음)이나 `let`·읽기 전용 값은 넘길 수 없습니다.
    fn check_inout_arg(&mut self, arg: &Expr, fname: &str, l: usize, c: usize) {
        let is_lvalue = matches!(
            arg,
            Expr::Ident(..) | Expr::Field(..) | Expr::Index(..)
        );
        if !is_lvalue {
            self.errors.push(
                err(
                    "T0046",
                    tr!(
                        format!("`{}`의 `inout` 인자에는 바꿀 수 있는 변수를 넘겨야 합니다", fname),
                        format!("an `inout` argument of `{}` must be a mutable variable", fname)
                    ),
                    l,
                    c,
                )
                .with_fix(tr!(
                    "리터럴이나 계산식 말고 `var`로 선언한 변수를 넘기세요",
                    "pass a variable declared with `var`, not a literal or an expression"
                )),
            );
            return;
        }
        if let Some(root) = self.mutated_binding(arg) {
            if self.is_captured(&root) {
                let e = self.captured_error(&root, l, c);
                self.errors.push(e);
            } else if !self.is_mutable(&root) {
                self.errors.push(
                    err(
                        "T0045",
                        tr!(
                            format!("`{}`은(는) 읽기 전용이라 `inout` 인자로 넘길 수 없습니다", root),
                            format!("`{}` is read-only and cannot be passed as an `inout` argument", root)
                        ),
                        l,
                        c,
                    )
                    .with_fix(tr!(format!("`{}`을(를) `var`로 선언하세요", root), format!("declare `{}` with `var`", root))),
                );
            }
        }
    }

    pub fn new(prog: &Program) -> Self {
        let mut t = Types {
            fns: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            variant_of: HashMap::new(),
            methods: HashMap::new(),
            scopes: vec![HashMap::new()],
            errors: Vec::new(),
            cur_ret: Ty::Unit,
            cur_fn: String::new(),
            imported: HashSet::new(),
            externs: HashSet::new(),
            links: Vec::new(),
            c_skipped: std::collections::HashMap::new(),
            unsafe_depth: 0,
            arena_depth: 0,
            tainted: HashSet::new(),
            arena_bases: Vec::new(),
            mutables: vec![HashSet::new()],
            cur_generics: HashSet::new(),
            reported_types: HashSet::new(),
            mono_subst: HashMap::new(),
            lambda_hint: None,
            lambda_sigs: HashMap::new(),
            closure_bases: Vec::new(),
            cur_decl: None,
            spawn_bodies: Vec::new(),
        };
        t.collect(prog);
        t
    }

    fn collect(&mut self, prog: &Program) {
        // 1차: 구조체와 열거형 이름을 먼저 등록해야 타입 표기를 풀 수 있습니다.
        for s in &prog.stmts {
            match s {
                Stmt::Struct(sd) => {
                    self.structs.insert(sd.name.clone(), sd.clone());
                }
                Stmt::Enum(ed) => {
                    for v in &ed.variants {
                        self.variant_of.insert(v.name.clone(), ed.name.clone());
                    }
                    self.enums.insert(ed.name.clone(), ed.clone());
                }
                Stmt::Import { names, path, line, col, .. } => {
                    for n in names {
                        self.imported.insert(n.clone());
                    }
                    self.check_std_import(path, names, *line, *col);
                    // `import std.math` 처럼 모듈째 가져오면 `math.sqrt(...)` 로 씁니다.
                    if names.is_empty() && path.len() == 2 && path[0] == "std" {
                        self.imported.insert(format!("@{}", path[1]));
                    }
                }
                _ => {}
            }
        }
        // 2차: 함수 시그니처
        for s in &prog.stmts {
            if let Stmt::Fn(f) = s {
                let sig = self.sig_of(f, None);
                if f.is_extern {
                    self.externs.insert(f.name.clone());
                }
                self.fns.insert(f.name.clone(), sig);
            }
            if let Stmt::Link(lib, _) = s {
                if !self.links.contains(lib) {
                    self.links.push(lib.clone());
                }
            }
            // 헤더에는 있지만 아직 자동으로 못 가져오는 함수. 이름을 기억해 뒀다가
            // 그 이름을 부르면 "없는 이름"이 아니라 왜 못 쓰는지 알려 줍니다.
            if let Stmt::CHeader { header, only, .. } = s {
                if only.len() == 2 {
                    self.c_skipped
                        .insert(only[0].clone(), (header.clone(), only[1].clone()));
                }
            }
        }
        // 3차: 메서드
        let structs: Vec<crate::ast::Shared<StructDecl>> = self.structs.values().cloned().collect();
        for sd in structs {
            for m in &sd.methods {
                let sig = self.sig_of(m, Some(Ty::Struct(sd.name.clone())));
                self.methods.insert(format!("{}.{}", sd.name, m.name), sig);
            }
        }
        let enums: Vec<crate::ast::Shared<EnumDecl>> = self.enums.values().cloned().collect();
        for ed in enums {
            for m in &ed.methods {
                let sig = self.sig_of(m, Some(Ty::Enum(ed.name.clone())));
                self.methods.insert(format!("{}.{}", ed.name, m.name), sig);
            }
        }
    }

    fn sig_of(&mut self, f: &crate::ast::Shared<FnDecl>, self_ty: Option<Ty>) -> FnSig {
        let saved_generics = std::mem::take(&mut self.cur_generics);
        self.cur_generics = f.generics.iter().cloned().collect();
        let mut params = Vec::new();
        for p in &f.params {
            if p.is_self {
                params.push(("self".to_string(), self_ty.clone().unwrap_or(Ty::Unknown)));
            } else {
                let t = match &p.ty {
                    Some(te) => self.resolve(te, f.line),
                    None => Ty::Unknown,
                };
                params.push((p.name.clone(), t));
            }
        }
        let ret = match &f.ret {
            Some(te) => self.resolve(te, f.line),
            None => Ty::Unit,
        };
        self.cur_generics = saved_generics;
        FnSig { params, ret, decl: f.clone() }
    }

    /// 문법 상의 타입 표기를 실제 타입으로 바꿉니다.
    pub fn resolve(&mut self, te: &TypeExpr, line: usize) -> Ty {
        if let TypeExpr::Named(n, targs) = te {
            if (n == "Task" || n == "Chan") && !self.structs.contains_key(n.as_str()) {
                let inner = match targs.first() {
                    Some(t) => self.resolve(t, line),
                    None => {
                        self.errors.push(
                            err(
                                "T0074",
                                tr!(format!("`{}` 에는 속 타입을 적어야 합니다", n), format!("`{}` needs an element type", n)),
                                line,
                                1,
                            )
                            .with_fix(tr!(format!("`{}[Int]` 처럼 씁니다", n), format!("write it like `{}[Int]`", n))),
                        );
                        Ty::Unknown
                    }
                };
                return if n == "Task" { Ty::Task(Box::new(inner)) } else { Ty::Chan(Box::new(inner)) };
            }
        }
        match te {
            TypeExpr::Named(n, _) => match n.as_str() {
                "Int" => Ty::Int,
                "Float" => Ty::Float,
                "Bool" => Ty::Bool,
                "Str" => Ty::Str,
                // 돌려줄 값이 없다는 뜻. `-> !Unit` 처럼 실패만 알릴 때 씁니다.
                "Unit" => Ty::Unit,
                "Json" => Ty::Json,
                other => {
                    if self.cur_generics.contains(other) {
                        // 단형화 중이면 실제 타입으로, 아니면 타입 매개변수로.
                        match self.mono_subst.get(other) {
                            Some(concrete) => concrete.clone(),
                            None => Ty::Var(other.to_string()),
                        }
                    } else if self.structs.contains_key(other) {
                        Ty::Struct(other.to_string())
                    } else if self.enums.contains_key(other) {
                        Ty::Enum(other.to_string())
                    } else {
                        let fix = match other {
                            "str" | "string" | "String" | "char" | "Char" | "text" => tr!("글자는 `Str` 입니다", "strings are `Str`").to_string(),
                            "int" | "i32" | "i64" | "long" | "integer" | "Integer" | "usize" => tr!("정수는 `Int` 입니다", "integers are `Int`").to_string(),
                            "float" | "double" | "f64" | "f32" | "number" | "Number" | "Double" => tr!("실수는 `Float` 입니다", "floating-point numbers are `Float`").to_string(),
                            "bool" | "boolean" | "Boolean" => tr!("참/거짓은 `Bool` 입니다", "booleans are `Bool`").to_string(),
                            "list" | "List" | "Vec" | "vector" | "Array" | "array" | "Seq" => tr!("리스트는 `[원소타입]` 으로 씁니다. 예: `[Int]`", "lists are written `[ElemType]`, e.g. `[Int]`").to_string(),
                            "dict" | "Dict" | "Map" | "map" | "HashMap" | "Dictionary" => tr!("사전은 `{키타입: 값타입}` 으로 씁니다. 예: `{Str: Int}`", "dicts are written `{KeyType: ValueType}`, e.g. `{Str: Int}`").to_string(),
                            "Option" | "Optional" | "Maybe" => tr!("값이 없을 수 있으면 앞에 `?` 를 붙입니다. 예: `?Int` (없음은 `none`)", "for an optional value, prefix the type with `?`, e.g. `?Int` (no value is `none`)").to_string(),
                            "Result" | "Either" | "Expected" => tr!("실패할 수 있으면 앞에 `!` 를 붙입니다. 예: `!Int` (실패는 `return error(\"이유\")`)", "for a fallible value, prefix the type with `!`, e.g. `!Int` (fail with `return error(\"reason\")`)").to_string(),
                            "void" | "None" | "none" | "Void" | "unit" => tr!("돌려줄 값이 없으면 `->` 를 쓰지 않습니다. 실패만 알릴 때는 `-> !Unit`", "omit `->` when nothing is returned; use `-> !Unit` when it can only fail").to_string(),
                            "tuple" | "Tuple" => tr!("튜플은 `(Int, Str)` 처럼 씁니다", "tuples are written like `(Int, Str)`").to_string(),
                            _ => {
                                let mut cands: Vec<String> =
                                    ["Int", "Float", "Bool", "Str", "Json", "Unit"].iter().map(|s| s.to_string()).collect();
                                cands.extend(self.structs.keys().cloned());
                                cands.extend(self.enums.keys().cloned());
                                match best_match(&cands, other) {
                                    Some(b) => tr!(format!("`{}` 를 찾으셨나요?", b), format!("did you mean `{}`?", b)),
                                    None => tr!(
                                        "기본 타입은 Int, Float, Bool, Str 입니다. 구조체·enum 이면 선언했는지 보세요",
                                        "the basic types are Int, Float, Bool and Str; for a struct or enum, check that it is declared"
                                    )
                                    .to_string(),
                                }
                            }
                        };
                        if self.reported_types.insert(other.to_string()) {
                            self.errors.push(
                                err(
                                    "T0001",
                                    tr!(format!("타입 `{}`을(를) 찾을 수 없습니다", other), format!("cannot find type `{}`", other)),
                                    line,
                                    1,
                                )
                                .with_fix(fix),
                            );
                        }
                        Ty::Unknown
                    }
                }
            },
            TypeExpr::Optional(t) => Ty::Optional(Box::new(self.resolve(t, line))),
            TypeExpr::Fallible(t, None) => Ty::fallible(self.resolve(t, line)),
            TypeExpr::Fallible(t, Some(e)) => {
                let et = self.resolve(e, line);
                if !matches!(et, Ty::Enum(_) | Ty::Str | Ty::Unknown) {
                    self.errors.push(
                        err(
                            "T0071",
                            tr!(format!("오류 타입은 enum 이어야 하는데 {}입니다", et), format!("error type must be an enum, found {}", et)),
                            line,
                            1,
                        )
                        .with_fix(tr!(
                            "`enum MyError:` 로 오류 종류를 정의하고 `MyError!Int` 처럼 씁니다",
                            "define the error kinds with `enum MyError:` and write `MyError!Int`"
                        )),
                    );
                }
                Ty::Fallible(Box::new(self.resolve(t, line)), Box::new(et))
            }
            TypeExpr::List(t) => Ty::List(Box::new(self.resolve(t, line))),
            TypeExpr::Dict(k, v) => {
                Ty::Dict(Box::new(self.resolve(k, line)), Box::new(self.resolve(v, line)))
            }
            TypeExpr::Raw(t) => Ty::Raw(Box::new(self.resolve(t, line))),
            // `()` 는 돌려줄 값이 없다는 뜻(Unit)입니다. `Task[()]` 처럼 씁니다.
            TypeExpr::Tuple(ts) if ts.is_empty() => Ty::Unit,
            TypeExpr::Tuple(ts) => Ty::Tuple(ts.iter().map(|t| self.resolve(t, line)).collect()),
            TypeExpr::Fn(ps, r) => Ty::Fn(
                ps.iter().map(|t| self.resolve(t, line)).collect(),
                Box::new(self.resolve(r, line)),
            ),
        }
    }

    // --------------------------------------------------------------- 스코프

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.mutables.push(HashSet::new());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
        self.mutables.pop();
    }

    pub fn scope_depth(&self) -> usize {
        self.scopes.len()
    }

    /// 디버거가 보여 줄 지역 변수: `base` 번째 스코프부터 보이는 이름과 타입(바깥 것부터).
    /// 좁혀 둔 필드(`t.due`) 같은 내부 이름은 뺍니다.
    pub fn locals_from(&self, base: usize) -> Vec<(String, Ty)> {
        let mut out: Vec<(String, Ty)> = Vec::new();
        for sc in self.scopes.iter().skip(base) {
            let mut names: Vec<(&String, &Ty)> = sc.iter().filter(|(k, _)| !k.contains('.') && !k.starts_with('λ')).collect();
            names.sort_by(|a, b| a.0.cmp(b.0));
            for (k, t) in names {
                out.retain(|(n, _)| n != k);
                out.push((k.clone(), t.clone()));
            }
        }
        out
    }

    pub fn declare(&mut self, name: &str, t: Ty) {
        let sc = self.scopes.last_mut().unwrap();
        if !name.contains('.') {
            // 같은 스코프에서 이름을 다시 만들면 그 이름에 걸린 필드 좁히기(`t.due`)는 끝납니다.
            let pre = format!("{}.", name);
            sc.retain(|k, _| !k.starts_with(&pre));
        }
        sc.insert(name.to_string(), t);
    }

    // ---------------------------------------------------- 구조체 필드 좁히기

    /// `inout` 인자(또는 `inout self`)를 가진 함수·메서드 이름들.
    fn inout_callables(&self) -> HashSet<String> {
        let mut out = HashSet::new();
        let has_inout = |d: &FnDecl| d.params.iter().any(|p| p.conv == Convention::Inout);
        for (n, sig) in &self.fns {
            if has_inout(&sig.decl) {
                out.insert(n.clone());
            }
        }
        for (n, sig) in &self.methods {
            if has_inout(&sig.decl) {
                out.insert(n.rsplit('.').next().unwrap_or(n).to_string());
            }
        }
        out
    }

    /// 좁혀 둔 필드 경로(`"t.due"`)의 타입. 뿌리 이름(`t`)이 좁힌 뒤 안쪽에서
    /// 다시 선언됐으면 다른 값이므로 없는 걸로 봅니다.
    pub fn narrowed_field(&self, key: &str) -> Option<Ty> {
        let root = key.split('.').next()?;
        let ki = self.scopes.iter().rposition(|s| s.contains_key(key))?;
        match self.scopes.iter().rposition(|s| s.contains_key(root)) {
            Some(ri) if ri > ki => None,
            _ => self.scopes[ki].get(key).cloned(),
        }
    }

    /// 이 식이 좁혀진 필드(`t.due`)면 좁혀진 타입.
    pub fn narrowed_expr(&self, e: &Expr) -> Option<Ty> {
        if !matches!(e, Expr::Field(..)) {
            return None;
        }
        let k = field_path(e)?;
        self.narrowed_field(&k)
    }

    /// `region` 안에서 `key`(또는 그 바깥 경로)가 `inout` 으로 넘겨져 바뀔 수 있는가.
    fn region_writes(&self, key: &str, region: &Region) -> bool {
        let inout = self.inout_callables();
        if inout.is_empty() {
            return false;
        }
        let mut w = Vec::new();
        match region {
            Region::Block(b) => call_writes_stmts(b, &inout, &mut w),
            Region::Expr(e) => call_writes_expr(e, &inout, &mut w),
            Region::After(line) => {
                if let Some(d) = &self.cur_decl {
                    call_writes_stmts(&d.body, &inout, &mut w);
                }
                w.retain(|(_, l)| *l >= *line);
            }
        }
        w.iter().any(|(p, _)| p == key || is_outer_path(p, key))
    }

    fn forget_fields(&mut self, dead: &dyn Fn(&str) -> bool) {
        for s in self.scopes.iter_mut() {
            s.retain(|k, _| !(k.contains('.') && dead(k)));
        }
    }

    /// 대입 앞: 좁혀진 필드에 없을 수 있는 값(`none`, `?T`)을 넣으면 그 좁히기를 풉니다.
    pub fn before_assign(&mut self, target: &Expr, value: &Expr) {
        if let Some(k) = field_path(target) {
            if k.contains('.') && self.narrowed_field(&k).is_some() {
                let vt = self.infer_quiet(value);
                if matches!(vt, Ty::Optional(_) | Ty::NoneTy) {
                    self.forget_fields(&|x| x == k);
                }
            }
        }
    }

    /// 대입 뒤: 바깥 경로를 통째로 바꾸면(`t = 다른값`) 그 안의 좁히기(`t.due`)를 풉니다.
    pub fn after_assign(&mut self, target: &Expr) {
        if let Some(w) = write_path(target) {
            self.forget_fields(&|k| is_outer_path(&w, k));
        }
    }

    /// 반복문 앞: 본문이 좁혀진 필드를 다시 없을 수 있게 만들면, 두 번째 바퀴에서
    /// 틀린 타입을 보게 되므로 반복문에 들어가기 전에 좁히기를 풉니다.
    pub fn loop_forget(&mut self, body: &[Stmt]) {
        let keys: Vec<String> =
            self.scopes.iter().flat_map(|s| s.keys().filter(|k| k.contains('.')).cloned().collect::<Vec<_>>()).collect();
        if keys.is_empty() {
            return;
        }
        let mut w = Vec::new();
        assign_writes(body, &mut w);
        let mut dead = HashSet::new();
        for k in &keys {
            for (p, v) in &w {
                if is_outer_path(p, k) {
                    dead.insert(k.clone());
                } else if p == k {
                    let vt = self.infer_quiet(v);
                    if matches!(vt, Ty::Optional(_) | Ty::NoneTy | Ty::Unknown) {
                        dead.insert(k.clone());
                    }
                }
            }
        }
        if !dead.is_empty() {
            self.forget_fields(&|k| dead.contains(k));
        }
    }

    /// 바꿀 수 있는 이름으로 선언합니다(`var`, `inout`/`owned` 인자).
    pub fn declare_mut(&mut self, name: &str, t: Ty) {
        self.declare(name, t);
        self.mutables.last_mut().unwrap().insert(name.to_string());
    }

    /// 이 이름을 바꿀 수 있는가. 안쪽 스코프부터 봅니다.
    /// 같은 이름이 안쪽에서 `let`으로 가려졌으면 그 가림이 우선합니다.
    fn is_mutable(&self, name: &str) -> bool {
        for (i, s) in self.scopes.iter().enumerate().rev() {
            if s.contains_key(name) {
                if self.closure_bases.last().map_or(false, |b| i < *b) {
                    return false;
                }
                return self.mutables[i].contains(name);
            }
        }
        // 좁혀진 이름 등 scopes에 없으면 막지 않습니다.
        true
    }

    /// 이 이름이 지금 검사 중인 클로저가 바깥에서 붙잡은 값인가.
    fn is_captured(&self, name: &str) -> bool {
        let base = match self.closure_bases.last() {
            Some(b) => *b,
            None => return false,
        };
        for (i, s) in self.scopes.iter().enumerate().rev() {
            if s.contains_key(name) {
                return i < base && i > 0;
            }
        }
        false
    }

    /// 붙잡은 값을 바꾸려 할 때의 오류.
    fn captured_error(&self, name: &str, l: usize, c: usize) -> SiskinError {
        if self.spawn_bodies.last().copied().unwrap_or(false) {
            return err(
                "T0053",
                tr!(
                    format!("`{}`은(는) 새 작업에 복사되어 넘어가므로 작업 안에서 바꿀 수 없습니다", name),
                    format!("`{}` is copied into the new task, so the task cannot change it", name)
                ),
                l,
                c,
            )
            .with_fix(tr!(
                "작업은 값을 복사해 받아 따로 돕니다. 바뀐 값은 돌려주게 하고 `let x = t.wait()` 로 받으세요 (또는 통로로 보내세요)",
                "a task works on its own copies; have it return the new value and read it with `let x = t.wait()` (or send it over a channel)"
            ));
        }
        err(
            "T0053",
            tr!(
                format!("`{}`은(는) 바깥에서 붙잡은 값이라 클로저 안에서 바꿀 수 없습니다", name),
                format!("`{}` is captured from the enclosing scope and cannot be changed inside a closure", name)
            ),
            l,
            c,
        )
        .with_fix(tr!(
            "클로저는 만들 때의 값을 복사해 두고 읽기만 합니다. 바뀐 값이 필요하면 돌려주도록(return) 만드세요",
            "a closure copies captured values when it is created and only reads them; return the new value instead"
        ))
    }

    pub fn lookup(&self, name: &str) -> Option<Ty> {
        for s in self.scopes.iter().rev() {
            if let Some(t) = s.get(name) {
                return Some(t.clone());
            }
        }
        None
    }

    // ------------------------------------------------------------- 타입 호환

    /// `want` 자리에 `got`을 넣을 수 있는가.
    /// `==` / `!=` 의 양쪽이 비교할 수 있는 타입인지. 예전에는 아무거나 받아서
    /// `input() == none` 처럼 늘 거짓인 비교가 조용히 통과했습니다.
    fn check_eq_types(&mut self, at: &Ty, bt: &Ty, l: usize, c: usize) {
        if matches!(at, Ty::Task(_) | Ty::Chan(_)) || matches!(bt, Ty::Task(_) | Ty::Chan(_)) {
            self.errors.push(
                err("T0017", tr!("작업(Task)과 통로(Chan)는 `==` 로 비교할 수 없습니다", "tasks and channels cannot be compared with `==`"), l, c)
                    .with_fix(tr!("끝났는지는 `t.done()`, 결과는 `t.wait()` 로 봅니다", "check `t.done()` or compare the results of `t.wait()`")),
            );
            return;
        }
        fn has_var(t: &Ty) -> bool {
            match t {
                Ty::Var(_) => true,
                Ty::List(a) | Ty::Optional(a) | Ty::Fallible(a, _) | Ty::Raw(a) => has_var(a),
                Ty::Dict(a, b) => has_var(a) || has_var(b),
                Ty::Tuple(ts) => ts.iter().any(has_var),
                _ => false,
            }
        }
        if has_var(at) || has_var(bt) {
            return;
        }
        if matches!(at, Ty::Fallible(..)) || matches!(bt, Ty::Fallible(..)) {
            let t = if matches!(at, Ty::Fallible(..)) { at } else { bt };
            self.errors.push(
                err("T0017", tr!(format!("{} 값은 `==`로 비교할 수 없습니다", t), format!("{} values cannot be compared with `==`", t)), l, c)
                    .with_fix(tr!(
                        "실패했는지는 `try` 나 `x = f() catch e:` 로 확인하세요. 성공한 값을 꺼낸 뒤에 비교합니다",
                        "check for failure with `try` or `x = f() catch e:`, then compare the unwrapped value"
                    )),
            );
            return;
        }
        let none_side = matches!(at, Ty::NoneTy) || matches!(bt, Ty::NoneTy);
        if none_side {
            let other = if matches!(at, Ty::NoneTy) { bt } else { at };
            if !matches!(other, Ty::Optional(_) | Ty::NoneTy | Ty::Unknown) {
                self.errors.push(
                    err(
                        "T0017",
                        tr!(
                            format!("{} 값은 none 이 될 수 없어서 `none` 과 비교해도 늘 같지 않습니다", other),
                            format!("{} can never be none, so comparing it with `none` is always false", other)
                        ),
                        l,
                        c,
                    )
                    .with_fix(tr!(
                        "none 이 될 수 있는 값은 `?T` 타입입니다. 빈 문자열을 보려면 `s == \"\"` 를 쓰세요",
                        "only optional `?T` values can be none; to check for an empty string use `s == \"\"`"
                    )),
                );
            }
            return;
        }
        if !(self.compatible(at, bt) || self.compatible(bt, at)) {
            let fix = if at.is_numeric() && bt.is_numeric() {
                tr!("Siskin에는 암묵적 형변환이 없습니다. `float(x)`로 맞추세요", "Siskin has no implicit conversions; convert with `float(x)`")
            } else {
                tr!("양쪽 타입을 맞춰 주세요", "make both sides the same type")
            };
            self.errors.push(
                err("T0017", tr!(format!("{}와(과) {}을(를) 비교할 수 없습니다", at, bt), format!("cannot compare {} with {}", at, bt)), l, c)
                    .with_fix(fix),
            );
        }
    }

    pub fn compatible(&self, want: &Ty, got: &Ty) -> bool {
        use Ty::*;
        match (want, got) {
            (Unknown, _) | (_, Unknown) => true,
            (Optional(_), NoneTy) => true,
            (Optional(a), Optional(b)) => self.compatible(a, b),
            (Optional(a), b) => self.compatible(a, b),
            (Fallible(a, ea), Fallible(b, eb)) => self.compatible(a, b) && self.compatible(ea, eb),
            (Fallible(a, _), b) => self.compatible(a, b),
            (List(a), List(b)) => self.compatible(a, b),
            (Task(a), Task(b)) => self.compatible(a, b),
            (Chan(a), Chan(b)) => self.compatible(a, b) && self.compatible(b, a),
            (Dict(a, b), Dict(c, d)) => self.compatible(a, c) && self.compatible(b, d),
            (Tuple(a), Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| self.compatible(x, y))
            }
            (Fn(pa, ra), Fn(pb, rb)) => {
                pa.len() == pb.len()
                    && pa.iter().zip(pb).all(|(x, y)| self.compatible(x, y))
                    && self.compatible(ra, rb)
            }
            (a, b) => a == b,
        }
    }

    /// 제네릭 호출에서 타입 매개변수를 실제 타입에 맞춰 채웁니다.
    /// 예: 인자 타입이 `[Int]`이고 매개변수가 `[T]`면 `T=Int`로 기록합니다.
    pub fn unify(&self, pat: &Ty, act: &Ty, subst: &mut HashMap<String, Ty>) {
        use Ty::*;
        match (pat, act) {
            (Var(n), _) => {
                if *act != Unknown {
                    let keep = matches!(subst.get(n), Some(t) if *t != Unknown);
                    if !keep {
                        subst.insert(n.clone(), act.clone());
                    }
                }
            }
            (List(a), List(b)) => self.unify(a, b, subst),
            (Task(a), Task(b)) | (Chan(a), Chan(b)) => self.unify(a, b, subst),
            (Optional(a), Optional(b)) => self.unify(a, b, subst),
            (Optional(a), b) => self.unify(a, b, subst),
            (Fallible(a, ea), Fallible(b, eb)) => {
                self.unify(a, b, subst);
                self.unify(ea, eb, subst);
            }
            (Fallible(a, _), b) => self.unify(a, b, subst),
            (Raw(a), Raw(b)) => self.unify(a, b, subst),
            (Dict(a, b), Dict(c, d)) => {
                self.unify(a, c, subst);
                self.unify(b, d, subst);
            }
            (Tuple(a), Tuple(b)) if a.len() == b.len() => {
                for (x, y) in a.iter().zip(b) {
                    self.unify(x, y, subst);
                }
            }
            (Fn(pa, ra), Fn(pb, rb)) if pa.len() == pb.len() => {
                for (x, y) in pa.iter().zip(pb) {
                    self.unify(x, y, subst);
                }
                self.unify(ra, rb, subst);
            }
            _ => {}
        }
    }

    /// 타입 안의 매개변수(`Ty::Var`)를 채워진 실제 타입으로 바꿉니다.
    pub fn substitute(&self, t: &Ty, subst: &HashMap<String, Ty>) -> Ty {
        use Ty::*;
        match t {
            Var(n) => subst.get(n).cloned().unwrap_or(Ty::Unknown),
            List(a) => List(Box::new(self.substitute(a, subst))),
            Task(a) => Task(Box::new(self.substitute(a, subst))),
            Chan(a) => Chan(Box::new(self.substitute(a, subst))),
            Optional(a) => Optional(Box::new(self.substitute(a, subst))),
            Fallible(a, e) => Fallible(Box::new(self.substitute(a, subst)), Box::new(self.substitute(e, subst))),
            Raw(a) => Raw(Box::new(self.substitute(a, subst))),
            Dict(a, b) => Dict(
                Box::new(self.substitute(a, subst)),
                Box::new(self.substitute(b, subst)),
            ),
            Tuple(ts) => Tuple(ts.iter().map(|x| self.substitute(x, subst)).collect()),
            Fn(ps, r) => Fn(
                ps.iter().map(|x| self.substitute(x, subst)).collect(),
                Box::new(self.substitute(r, subst)),
            ),
            other => other.clone(),
        }
    }

    /// 네이티브 단형화 시작: 이 함수의 타입 매개변수를 실제 타입에 묶습니다.
    /// 이 뒤로 `resolve`가 `T`를 실제 타입으로 풀어 줍니다.
    pub fn enter_mono(&mut self, generics: &[String], subst: HashMap<String, Ty>) {
        self.cur_generics = generics.iter().cloned().collect();
        self.mono_subst = subst;
    }

    /// 단형화 끝: 매개변수 묶음을 비웁니다.
    pub fn exit_mono(&mut self) {
        self.cur_generics.clear();
        self.mono_subst.clear();
    }

    // --------------------------------------------------------------- 검사

    pub fn check_program(&mut self, prog: &Program) {
        self.check_program_inner(prog);
        // 같은 자리의 같은 오류는 한 번만 보여 줍니다 (시그니처를 두 번 읽어서 겹치던 것).
        let mut seen = HashSet::new();
        self.errors.retain(|e| seen.insert((e.code, e.msg.clone(), e.line, e.col)));
    }

    fn check_program_inner(&mut self, prog: &Program) {
        // 구조체·enum 필드의 타입을 선언한 자리에서 먼저 봅니다. 그래야 잘못된 타입이
        // 쓰는 곳마다가 아니라 선언한 줄에서 한 번 알려집니다.
        for s in &prog.stmts {
            match s {
                Stmt::Struct(sd) => {
                    let saved = std::mem::replace(&mut self.cur_generics, sd.generics.iter().cloned().collect());
                    for f in &sd.fields {
                        if let Some(te) = &f.ty {
                            let _ = self.resolve(te, sd.line);
                        }
                    }
                    self.cur_generics = saved;
                }
                Stmt::Enum(ed) => {
                    for v in &ed.variants {
                        for f in &v.fields {
                            if let Some(te) = &f.ty {
                                let _ = self.resolve(te, ed.line);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // 최상위 `let` 은 파일 전체에서 보이는 상수입니다. 함수보다 먼저 봐 두어야
        // 위치와 상관없이(함수가 먼저 적혀 있어도) 씁니다.
        for s in &prog.stmts {
            if let Stmt::Let { name, mutable, catch, line, col, .. } = s {
                if *mutable || catch.is_some() {
                    let (m, fix) = if *mutable {
                        (
                            tr!(
                                format!("최상위에는 `var` 를 둘 수 없습니다: `{}`", name),
                                format!("`var` is not allowed at the top level: `{}`", name)
                            ),
                            tr!(
                                "최상위 값은 바꿀 수 없는 상수(`let`)만 됩니다. 바꿀 값은 `main` 안에 두고 함수에 넘기세요",
                                "top-level values are constants (`let`); keep values that change inside `main` and pass them to functions"
                            ),
                        )
                    } else {
                        (
                            tr!("최상위 `let` 에는 `catch` 를 붙일 수 없습니다", "a top-level `let` cannot have `catch`").to_string(),
                            tr!("실패할 수 있는 값은 `main` 안에서 받으세요", "get values that can fail inside `main`"),
                        )
                    };
                    self.errors.push(err("T0076", m, *line, *col).with_fix(fix));
                    continue;
                }
                self.check_stmt(s);
            }
        }
        for s in &prog.stmts {
            match s {
                Stmt::Let { .. } => {}
                Stmt::Import { .. } | Stmt::Interface(_) => self.check_stmt(s),
                Stmt::Fn(f) => self.check_fn(f, None),
                Stmt::Link(_, _) | Stmt::CHeader { .. } => {}
                Stmt::Struct(sd) => {
                    for m in &sd.methods {
                        self.check_fn(m, Some(Ty::Struct(sd.name.clone())));
                    }
                }
                Stmt::Enum(ed) => {
                    for m in &ed.methods {
                        self.check_fn(m, Some(Ty::Enum(ed.name.clone())));
                    }
                }
                other => {
                    // 실행할 문장은 main 안에만 둡니다(`siskin build` 와 같은 규칙).
                    let line = crate::debug::stmt_line(other).unwrap_or(1);
                    self.errors.push(
                        err("T0077", tr!("최상위에는 함수·타입 선언과 상수(`let`)만 올 수 있습니다", "only functions, type declarations and constants (`let`) are allowed at the top level"), line, 1)
                            .with_fix(tr!("실행할 코드는 `fn main():` 안에 넣으세요", "put code to run inside `fn main():`")),
                    );
                }
            }
        }
        if !self.fns.contains_key("main") {
            self.errors.push(
                err("T0002", tr!("`main` 함수가 없습니다", "no `main` function"), 1, 1)
                    .with_fix(tr!("프로그램은 `fn main():` 에서 시작합니다", "a program starts at `fn main():`")),
            );
        } else if let Some(m) = self.fns.get("main").cloned() {
            if !m.params.is_empty() {
                self.errors.push(
                    err("T0061", tr!("`main`은 인자를 받지 않습니다", "`main` takes no parameters"), m.decl.line, 1)
                        .with_fix(tr!(
                            "명령줄 인자는 `args()` 로 받습니다. 예: `let a = args()` (프로그램 이름은 빠진 [Str])",
                            "get command-line arguments with `args()`, e.g. `let a = args()` (a [Str] without the program name)"
                        )),
                );
            }
            let ok = matches!(m.ret, Ty::Unit) || matches!(&m.ret, Ty::Fallible(t, _) if **t == Ty::Unit);
            if !ok {
                self.errors.push(
                    err(
                        "T0061",
                        tr!(
                            format!("`main`은 값을 돌려주지 않습니다 ({}를 적었습니다)", m.ret),
                            format!("`main` does not return a value (declared {})", m.ret)
                        ),
                        m.decl.line,
                        1,
                    )
                    .with_fix(tr!(
                        "`fn main():` 또는 실패할 수 있으면 `fn main() -> !Unit:` 로 쓰세요. 끝 코드는 `exit(n)` 으로 정합니다",
                        "write `fn main():`, or `fn main() -> !Unit:` if it can fail; set the exit code with `exit(n)`"
                    )),
                );
            }
        }
    }

    fn check_fn(&mut self, f: &crate::ast::Shared<FnDecl>, self_ty: Option<Ty>) {
        let sig = self.sig_of(f, self_ty);
        let prev_ret = std::mem::replace(&mut self.cur_ret, sig.ret.clone());
        let prev_fn = std::mem::replace(&mut self.cur_fn, f.name.clone());
        let prev_generics =
            std::mem::replace(&mut self.cur_generics, f.generics.iter().cloned().collect());
        let prev_decl = std::mem::replace(&mut self.cur_decl, Some(f.clone()));
        self.push_scope();
        for (n, t) in &sig.params {
            // 읽기 전용 인자(기본)와 읽기 전용 self는 못 바꿉니다.
            // `inout`/`owned` 인자와 `inout self`만 바꿀 수 있습니다.
            let mutable = f
                .params
                .iter()
                .find(|p| &p.name == n)
                .map(|p| matches!(p.conv, Convention::Inout | Convention::Owned))
                .unwrap_or(false);
            if mutable {
                self.declare_mut(n, t.clone());
            } else {
                self.declare(n, t.clone());
            }
        }
        for r in &f.requires {
            let t = self.infer(r);
            if !self.compatible(&Ty::Bool, &t) {
                let (l, c) = r.pos();
                self.errors.push(err(
                    "T0003",
                    tr!(format!("requires는 Bool이어야 하는데 {}입니다", t), format!("`requires` must be Bool, found {}", t)),
                    if l == 0 { f.line } else { l },
                    if c == 0 { 1 } else { c },
                ));
            }
        }
        for s in &f.body {
            self.check_stmt(s);
        }
        // 값을 돌려줘야 하는 함수가 끝까지 흘러가면, 예전에는 run 은 `none`, build 는 0 을
        // 조용히 돌려줬습니다. 컴파일 때 막습니다.
        let needs_value = !matches!(sig.ret, Ty::Unit | Ty::Unknown)
            && !matches!(&sig.ret, Ty::Fallible(x, _) if matches!(**x, Ty::Unit));
        if needs_value && !f.is_extern && !f.body.is_empty() && !always_returns(&f.body) {
            let shown = if f.name.starts_with(crate::ast::LAMBDA_PREFIX) { tr!("익명 함수", "the anonymous function").to_string() } else { format!("`{}`", f.name) };
            self.errors.push(
                err(
                    "T0069",
                    tr!(
                        format!("{}은(는) {}를 돌려줘야 하는데, 끝까지 가면 돌려줄 값이 없습니다", shown, sig.ret),
                        format!("{} must return {}, but can reach the end without returning a value", shown, sig.ret)
                    ),
                    f.line,
                    1,
                )
                .with_fix(tr!(
                    "모든 갈래가 `return 값` 으로 끝나게 하세요 (if 에는 else 도, match 에는 `case _:` 도)",
                    "make every path end with `return value` (add an `else` to `if` and a `case _:` to `match`)"
                )),
            );
        }
        // ensures는 result를 볼 수 있습니다.
        self.push_scope();
        self.declare("result", sig.ret.clone());
        for e in &f.ensures {
            let t = self.infer(e);
            if !self.compatible(&Ty::Bool, &t) {
                let (l, c) = e.pos();
                self.errors.push(err(
                    "T0003",
                    tr!(format!("ensures는 Bool이어야 하는데 {}입니다", t), format!("`ensures` must be Bool, found {}", t)),
                    if l == 0 { f.line } else { l },
                    if c == 0 { 1 } else { c },
                ));
            }
        }
        self.pop_scope();
        self.pop_scope();
        self.cur_ret = prev_ret;
        self.cur_fn = prev_fn;
        self.cur_generics = prev_generics;
        self.cur_decl = prev_decl;
    }

    fn check_cond(&mut self, e: &Expr) {
        let t = self.infer(e);
        if !self.compatible(&Ty::Bool, &t) {
            let (l, c) = e.pos();
            self.errors.push(
                err("T0004", tr!(format!("조건은 Bool이어야 하는데 {}입니다", t), format!("condition must be Bool, found {}", t)), l, c)
                    .with_fix(tr!(
                        "Siskin에는 암묵적 참/거짓 변환이 없습니다. `x != 0` 처럼 명시하세요",
                        "Siskin has no implicit truthiness; write the comparison, e.g. `x != 0`"
                    )),
            );
        }
    }

    /// `if x != none:` 안에서 x를 벗겨진 타입으로 보게 합니다. 구조체 필드(`t.due`)도 됩니다.
    /// 필드는 `region` 안에서 `inout` 으로 바뀔 수 있으면 좁히지 않습니다.
    /// `a and b` 의 참쪽, `a or b` 의 거짓쪽에서는 두 조건의 좁히기를 모두 받습니다.
    pub fn narrowing(&mut self, cond: &Expr, positive: bool, region: &Region) -> Vec<(String, Ty)> {
        let mut out = Vec::new();
        self.narrowing_into(cond, positive, region, &mut out);
        out
    }

    fn narrowing_into(&mut self, cond: &Expr, positive: bool, region: &Region, out: &mut Vec<(String, Ty)>) {
        if let Expr::Binary(op, a, b, _, _) = cond {
            if (*op == BinOp::And && positive) || (*op == BinOp::Or && !positive) {
                self.narrowing_into(a, positive, region, out);
                self.narrowing_into(b, positive, region, out);
                return;
            }
            let is_ne = *op == BinOp::Ne;
            let is_eq = *op == BinOp::Eq;
            if !is_ne && !is_eq {
                return;
            }
            let target = match (a.as_ref(), b.as_ref()) {
                (x, Expr::NoneLit) | (Expr::NoneLit, x) => x,
                _ => return,
            };
            // `x != none` 의 참쪽, `x == none` 의 거짓쪽에서 벗겨집니다.
            let unwrap_here = (is_ne && positive) || (is_eq && !positive);
            if !unwrap_here {
                return;
            }
            match target {
                Expr::Ident(n, _, _) => {
                    if let Some(Ty::Optional(inner)) = self.lookup(n) {
                        out.push((n.clone(), *inner));
                    }
                }
                Expr::Field(..) => {
                    if let Some(key) = field_path(target) {
                        if let Ty::Optional(inner) = self.infer_quiet(target) {
                            if !self.region_writes(&key, region) {
                                out.push((key, *inner));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// 다음 `infer` 가 익명 함수를 만나면 쓸 자리 타입을 정해 둡니다(코드 생성용).
    pub fn lambda_hint_set(&mut self, h: Option<Ty>) {
        self.lambda_hint = h;
    }

    /// 식을 추론하되, 그 식이 익명 함수면 "들어갈 자리의 타입"을 알려 줍니다.
    /// 인자 타입을 적지 않은 익명 함수가 여기서 타입을 얻습니다.
    fn infer_hinted(&mut self, e: &Expr, hint: Option<Ty>) -> Ty {
        if matches!(e, Expr::Lambda(..)) {
            self.lambda_hint = hint;
        }
        self.infer(e)
    }

    /// 타입이 아직 다 정해지지 않았는가(`_` 나 `T` 가 섞였는가).
    fn is_open(t: &Ty) -> bool {
        match t {
            Ty::Unknown | Ty::Var(_) => true,
            Ty::List(a) | Ty::Optional(a) | Ty::Fallible(a, _) | Ty::Raw(a) => Self::is_open(a),
            Ty::Dict(a, b) => Self::is_open(a) || Self::is_open(b),
            Ty::Tuple(ts) => ts.iter().any(Self::is_open),
            Ty::Fn(ps, r) => ps.iter().any(Self::is_open) || Self::is_open(r),
            _ => false,
        }
    }

    /// 제네릭 함수 안에서는 그 함수의 타입 매개변수(`T`)가 정해진 타입처럼 쓰입니다.
    fn is_open_here(&self, t: &Ty) -> bool {
        match t {
            Ty::Var(n) => !self.cur_generics.contains(n),
            Ty::Unknown => true,
            Ty::List(a) | Ty::Optional(a) | Ty::Fallible(a, _) | Ty::Raw(a) => self.is_open_here(a),
            Ty::Dict(a, b) => self.is_open_here(a) || self.is_open_here(b),
            Ty::Tuple(ts) => ts.iter().any(|x| self.is_open_here(x)),
            Ty::Fn(ps, r) => ps.iter().any(|x| self.is_open_here(x)) || self.is_open_here(r),
            _ => false,
        }
    }

    /// 함수 안에 선언한 `fn` — 바깥 값을 붙잡는 이름 붙은 클로저입니다.
    fn check_nested_fn(&mut self, f: &crate::ast::Shared<FnDecl>) {
        if !f.generics.is_empty() {
            self.errors.push(
                err(
                    "T0055",
                    tr!(
                        format!("함수 안에 선언한 `{}`은(는) 제네릭으로 만들 수 없습니다", f.name),
                        format!("nested function `{}` cannot be generic", f.name)
                    ),
                    f.line,
                    1,
                )
                .with_fix(tr!("제네릭 함수는 파일 맨 바깥에 선언하세요", "declare generic functions at the top level of the file")),
            );
        }
        let sig = self.sig_of(f, None);
        let ps: Vec<Ty> = sig.params.iter().map(|(_, t)| t.clone()).collect();
        // 본문보다 먼저 이름을 알려야 자기 자신을 부를 수 있습니다(재귀).
        self.declare(&f.name, Ty::Fn(ps, Box::new(sig.ret.clone())));
        self.check_closure(f, None, 1);
    }

    /// 클로저(익명 함수·중첩 함수)의 본문을 검사하고 그 함수 타입을 냅니다.
    /// 바깥 스코프는 그대로 보이지만 읽기만 됩니다(붙잡은 값은 복사본이라서).
    fn check_closure(&mut self, f: &crate::ast::Shared<FnDecl>, hint: Option<Ty>, col: usize) -> Ty {
        let (hp, hr) = match hint {
            Some(Ty::Fn(ps, r)) if ps.len() == f.params.len() => (Some(ps), Some(*r)),
            _ => (None, None),
        };
        let mut ptys = Vec::new();
        for (i, p) in f.params.iter().enumerate() {
            let t = match &p.ty {
                Some(te) => self.resolve(te, f.line),
                None => match hp.as_ref().map(|v| v[i].clone()) {
                    Some(t) if !self.is_open_here(&t) => t,
                    _ => {
                        self.errors.push(
                            err(
                                "T0054",
                                tr!(
                                    format!("익명 함수의 인자 `{}`의 타입을 알 수 없습니다", p.name),
                                    format!("cannot infer the type of anonymous function parameter `{}`", p.name)
                                ),
                                f.line,
                                col,
                            )
                            .with_fix(match self.cur_generics.iter().next() {
                                Some(g) => tr!(
                                    format!("`fn({}: {}): ...` 처럼 타입을 적어 주세요", p.name, g),
                                    format!("annotate the type, e.g. `fn({}: {}): ...`", p.name, g)
                                ),
                                None => tr!(
                                    format!("`fn({}: Int): ...` 처럼 타입을 적어 주세요", p.name),
                                    format!("annotate the type, e.g. `fn({}: Int): ...`", p.name)
                                ),
                            }),
                        );
                        Ty::Unknown
                    }
                },
            };
            ptys.push(t);
        }
        let declared_ret = f.ret.as_ref().map(|te| self.resolve(te, f.line));
        let base = self.scopes.len();
        self.closure_bases.push(base);
        self.spawn_bodies.push(f.name.starts_with(&format!("{}spawn", crate::ast::LAMBDA_PREFIX)));
        let prev_ret = std::mem::replace(&mut self.cur_ret, declared_ret.clone().unwrap_or(Ty::Unit));
        let prev_fn = std::mem::replace(&mut self.cur_fn, f.shown_name());
        let prev_hint = self.lambda_hint.take();
        let prev_unsafe = std::mem::replace(&mut self.unsafe_depth, 0);
        self.push_scope();
        for (p, t) in f.params.iter().zip(&ptys) {
            if matches!(p.conv, Convention::Owned | Convention::Inout) {
                self.declare_mut(&p.name, t.clone());
            } else {
                self.declare(&p.name, t.clone());
            }
        }
        let ret = match (&declared_ret, f.is_lambda(), f.body.first()) {
            (None, true, Some(Stmt::Return(Some(e), _, _))) => {
                // `fn(x): 식` — 반환 타입은 식의 타입입니다.
                self.cur_ret = Ty::Unknown;
                self.infer_hinted(e, hr.clone())
            }
            _ => {
                for r in &f.requires {
                    self.check_cond(r);
                }
                if let (Some(Stmt::Return(Some(e), l, c)), true) = (f.body.first(), f.is_lambda()) {
                    // 반환 타입을 적은 익명 함수: 식이 그 타입이어야 합니다.
                    let want = self.cur_ret.clone();
                    let t = self.infer_hinted(e, Some(want.clone()));
                    if !self.compatible(&want, &t) {
                        self.errors.push(err(
                            "T0013",
                            tr!(
                                format!("익명 함수는 {}을(를) 반환해야 하는데 {}을(를) 반환합니다", want, t),
                                format!("anonymous function should return {}, but returns {}", want, t)
                            ),
                            *l,
                            *c,
                        ));
                    }
                } else {
                    for s in &f.body {
                        self.check_stmt(s);
                    }
                }
                declared_ret.clone().unwrap_or(Ty::Unit)
            }
        };
        self.pop_scope();
        self.unsafe_depth = prev_unsafe;
        self.lambda_hint = prev_hint;
        self.cur_fn = prev_fn;
        self.cur_ret = prev_ret;
        self.closure_bases.pop();
        self.spawn_bodies.pop();
        self.lambda_sigs.insert(crate::ast::Shared::as_ptr(f) as usize, (ptys.clone(), ret.clone()));
        Ty::Fn(ptys, Box::new(ret))
    }

    pub fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Fn(f) => self.check_nested_fn(f),
            Stmt::Link(_, _) | Stmt::CHeader { .. } => {}
            Stmt::Struct(_) | Stmt::Enum(_) | Stmt::Interface(_) | Stmt::Import { .. } => {}

            Stmt::Let { name, ty, value, catch, line, col, mutable } => {
                // 같은 블록에서 같은 이름을 또 선언하면 run 은 실행 중 오류(E0211), build 는 C 오류였습니다.
                if self.scopes.len() > 1 && self.scopes.last().map_or(false, |sc| sc.contains_key(name)) {
                    self.errors.push(
                        err(
                            "T0070",
                            tr!(
                                format!("`{}`은(는) 이 블록에서 이미 선언되었습니다", name),
                                format!("`{}` is already declared in this block", name)
                            ),
                            *line,
                            *col,
                        )
                        .with_fix(tr!(
                            format!("다른 이름을 쓰거나, 값을 바꾸려면 `var {}` 로 선언한 뒤 `{} = ...` 로 대입하세요", name, name),
                            format!("use a different name, or declare it once with `var {}` and assign with `{} = ...`", name, name)
                        )),
                    );
                }
                let hint = match (value, ty) {
                    (Expr::Lambda(..), Some(t)) => {
                        let saved = self.errors.len();
                        let r = self.resolve(t, *line);
                        self.errors.truncate(saved);
                        Some(r)
                    }
                    _ => None,
                };
                let mut vt = self.infer_hinted(value, hint);
                if let Some(c) = catch {
                    // catch가 붙으면 에러 쪽은 처리되었으므로 성공 타입만 남습니다.
                    let mut err_ty = Ty::Str;
                    if let Ty::Fallible(inner, e) = vt.clone() {
                        vt = *inner;
                        err_ty = *e;
                    }
                    let want = match ty {
                        Some(t) => {
                            let saved = self.errors.len();
                            let r = self.resolve(t, *line);
                            self.errors.truncate(saved);
                            r
                        }
                        None => vt.clone(),
                    };
                    self.check_catch_value(c, name, &want, &err_ty, *line, *col);
                } else if let Ty::Fallible(..) = vt {
                    self.errors.push(
                        err(
                            "T0005",
                            tr!(
                                format!("`{}`에 실패할 수 있는 값({})을 그냥 담을 수 없습니다", name, vt),
                                format!("cannot store a fallible value ({}) in `{}` directly", vt, name)
                            ),
                            *line,
                            *col,
                        )
                        .with_fix(tr!("`try`로 전파하거나 `catch e:` 로 받으세요", "propagate it with `try` or handle it with `catch e:`")),
                    );
                }
                let declared = ty.as_ref().map(|t| {
                    let l = *line;
                    self.resolve(t, l)
                });
                if let Some(d) = &declared {
                    if !self.compatible(d, &vt) {
                        self.errors.push(
                            err(
                                "T0006",
                                tr!(
                                    format!("`{}`의 타입은 {}인데 {} 값을 담았습니다", name, d, vt),
                                    format!("`{}` has type {}, but the value is {}", name, d, vt)
                                ),
                                *line,
                                *col,
                            )
                            .with_fix(if matches!(value, Expr::NoneLit) {
                                tr!(
                                    format!("값이 없을 수 있으면 타입 앞에 `?` 를 붙이세요: `?{}`", d),
                                    format!("if the value may be absent, make the type optional: `?{}`", d)
                                )
                            } else {
                                tr!("타입 표기를 고치거나 값을 바꾸세요", "fix the type annotation or change the value").to_string()
                            }),
                        );
                    }
                }
                let final_ty = declared.unwrap_or(vt);
                if self.arena_depth > 0 {
                    let from_arena = matches!(final_ty, Ty::Raw(_))
                        || matches!(value_arena_src(value), Some(Expr::Field(o, m, _, _))
                            if (m == "alloc" || m == "list")
                                && self.infer_peek(o) == Some(Ty::Arena));
                    if from_arena {
                        self.tainted.insert(name.clone());
                    }
                }
                if *mutable {
                    self.declare_mut(name, final_ty);
                } else {
                    self.declare(name, final_ty);
                }
            }

            Stmt::LetTuple { names, value, line, col } => {
                let vt = self.infer(value);
                match &vt {
                    Ty::Tuple(elems) => {
                        if elems.len() != names.len() {
                            self.errors.push(
                                err(
                                    "T0049",
                                    tr!(
                                        format!(
                                            "튜플에는 값이 {}개인데 이름을 {}개 적었습니다",
                                            elems.len(),
                                            names.len()
                                        ),
                                        format!(
                                            "the tuple has {} values, but {} names were given",
                                            elems.len(),
                                            names.len()
                                        )
                                    ),
                                    *line,
                                    *col,
                                )
                                .with_fix(tr!("이름 개수를 튜플 값 개수와 맞추세요", "use as many names as the tuple has values")),
                            );
                            for name in names {
                                self.declare(name, Ty::Unknown);
                            }
                        } else {
                            for (name, ety) in names.iter().zip(elems) {
                                self.declare(name, ety.clone());
                            }
                        }
                    }
                    Ty::Unknown => {
                        for name in names {
                            self.declare(name, Ty::Unknown);
                        }
                    }
                    other => {
                        self.errors.push(
                            err(
                                "T0050",
                                tr!(
                                    format!("튜플로 풀어 받으려면 오른쪽이 튜플이어야 하는데 {}입니다", other),
                                    format!("tuple destructuring needs a tuple on the right, found {}", other)
                                ),
                                *line,
                                *col,
                            )
                            .with_fix(tr!(
                                "`let (a, b) = ...`는 `(값1, 값2)` 형태의 튜플에만 씁니다",
                                "`let (a, b) = ...` only works on a tuple like `(value1, value2)`"
                            )),
                        );
                        for name in names {
                            self.declare(name, Ty::Unknown);
                        }
                    }
                }
            }

            Stmt::Assign { target, op, value, catch, line, col } => {
                self.before_assign(target, value);
                if let Expr::Field(o, n, fl, fc) = target {
                    if n.chars().all(|ch| ch.is_ascii_digit()) && matches!(self.infer_quiet(o), Ty::Tuple(_)) {
                        self.errors.push(
                            err("T0067", tr!("튜플 안의 값은 따로 바꿀 수 없습니다", "tuple elements cannot be assigned individually"), *fl, *fc)
                                .with_fix(tr!("새 튜플을 통째로 넣으세요: `p = (새값, p.1)`", "assign a whole new tuple: `p = (new_value, p.1)`")),
                        );
                    }
                }
                let hint = if matches!(value, Expr::Lambda(..)) { Some(self.infer_quiet(target)) } else { None };
                let mut vt = self.infer_hinted(value, hint);
                if let Some(c) = catch {
                    let mut err_ty = Ty::Str;
                    if let Ty::Fallible(inner, e) = vt.clone() {
                        vt = *inner;
                        err_ty = *e;
                    }
                    let want = self.infer_quiet(target);
                    let shown = crate::interp::render_expr(target);
                    self.check_catch_value(c, &shown, &want, &err_ty, *line, *col);
                }
                // 아레나 안의 값을 블록 바깥 변수에 담으면(구조체 필드를 거쳐도) 블록이 끝난 뒤
                // 사라진 메모리를 가리키게 됩니다. `return` 과 같은 규칙으로 막습니다.
                if let (Some(base), Some(root)) = (self.arena_bases.last().copied(), self.mutated_binding(target)) {
                    let outer = self
                        .scopes
                        .iter()
                        .rposition(|sc| sc.contains_key(&root))
                        .map(|i| i < base)
                        .unwrap_or(false);
                    // 리스트는 담을 때 복사되므로 안전합니다. 포인터(또는 포인터를 품을 수 있는 구조체)만 막습니다.
                    let vt_now = self.infer_quiet(value);
                    let risky = matches!(vt_now, Ty::Raw(_) | Ty::Struct(_) | Ty::Tuple(_) | Ty::Enum(_));
                    if outer && risky {
                        if let Some(name) = self.mentions_tainted(value) {
                            self.errors.push(
                                err(
                                    "T0040",
                                    tr!(
                                        format!("`{}`은(는) 아레나에서 나온 값이라 블록 바깥 변수 `{}` 에 담을 수 없습니다", name, root),
                                        format!("`{}` comes from an arena and cannot be stored in `{}`, which lives outside the block", name, root)
                                    ),
                                    *line,
                                    *col,
                                )
                                .with_fix(tr!(
                                    "아레나 메모리는 블록 끝에서 전부 해제됩니다. 필요한 값은 복사해서 담으세요",
                                    "arena memory is freed at the end of the block; store a copy of the value you need"
                                )),
                            );
                        }
                    }
                }
                // 값 의미론: `let`과 읽기 전용 인자·self는 바꿀 수 없습니다.
                // 단, 포인터를 거쳐 쓰는 `p[i] = ...`는 포인터가 가리키는
                // 메모리를 바꾸는 것이라 `let p`여도 괜찮습니다.
                if let Some(root) = self.mutated_binding(target) {
                    let root = root.as_str();
                    if self.is_captured(root) {
                        let e = self.captured_error(root, *line, *col);
                        self.errors.push(e);
                    } else if !self.is_mutable(root) {
                        let fix = if root == "self" {
                            tr!(
                                "이 값을 바꾸려면 메서드를 `fn 이름(inout self, ...)`으로 선언하세요",
                                "to change it, declare the method as `fn name(inout self, ...)`"
                            )
                            .to_string()
                        } else {
                            tr!(
                                format!("`{}`을(를) `var`로 선언하거나, 함수 인자라면 `inout`으로 받으세요", root),
                                format!("declare `{}` with `var`, or take it as `inout` if it is a parameter", root)
                            )
                        };
                        self.errors.push(
                            err(
                                "T0045",
                                tr!(format!("`{}`은(는) 바꿀 수 없습니다(읽기 전용)", root), format!("cannot assign to `{}` (read-only)", root)),
                                *line,
                                *col,
                            )
                            .with_fix(fix),
                        );
                    }
                }
                let tt = self.infer(target);
                if op.is_some() && !(tt.is_numeric() || tt == Ty::Str || tt == Ty::Unknown) {
                    self.errors.push(err(
                        "T0007",
                        tr!(
                            format!("{} 값에는 `+=` 같은 연산을 쓸 수 없습니다", tt),
                            format!("compound assignment like `+=` cannot be used on {} values", tt)
                        ),
                        *line,
                        *col,
                    ));
                }
                if !self.compatible(&tt, &vt) {
                    self.errors.push(
                        err("T0008", tr!(format!("{} 자리에 {} 값을 넣을 수 없습니다", tt, vt), format!("cannot assign {} to {}", vt, tt)), *line, *col)
                            .with_fix(tr!("Siskin에는 암묵적 형변환이 없습니다", "Siskin has no implicit conversions")),
                    );
                }
                self.after_assign(target);
            }

            Stmt::Expr(e, catch) => {
                let t = self.infer(e);
                if let Some(c) = catch {
                    let err_ty = match &t {
                        Ty::Fallible(_, e) => (**e).clone(),
                        _ => Ty::Str,
                    };
                    self.push_scope();
                    self.declare(&c.name, err_ty);
                    for s in &c.body {
                        self.check_stmt(s);
                    }
                    self.pop_scope();
                }
            }

            Stmt::If { arms, els } => {
                for (cond, body) in arms {
                    self.check_cond(cond);
                    let narrow = self.narrowing(cond, true, &Region::Block(body));
                    self.push_scope();
                    for (n, t) in narrow {
                        self.declare(&n, t);
                    }
                    for s in body {
                        self.check_stmt(s);
                    }
                    self.pop_scope();
                }
                if let Some(b) = els {
                    let narrow = if arms.len() == 1 { self.narrowing(&arms[0].0, false, &Region::Block(b)) } else { Vec::new() };
                    self.push_scope();
                    for (n, t) in narrow {
                        self.declare(&n, t);
                    }
                    for s in b {
                        self.check_stmt(s);
                    }
                    self.pop_scope();
                }
                // 가드 절: `if v == none: return` 처럼 유일한 arm이 반드시 빠져나가면
                // 그 뒤부터 반대 방향으로 좁혀 줍니다. 깊은 중첩을 평평하게 만듭니다.
                if els.is_none() && arms.len() == 1 && block_diverges(&arms[0].1) {
                    let line = arms[0].0.pos().0;
                    for (n, t) in self.narrowing(&arms[0].0, false, &Region::After(line)) {
                        self.declare(&n, t);
                    }
                }
            }

            Stmt::While { cond, body } => {
                self.loop_forget(body);
                self.check_cond(cond);
                self.push_scope();
                for s in body {
                    self.check_stmt(s);
                }
                self.pop_scope();
            }

            Stmt::For { var, var2, iter, body, line } => {
                self.loop_forget(body);
                let it = self.infer(iter);
                self.push_scope();
                if let Some(v2) = var2 {
                    // `for k, v in d:` — 사전만 됩니다. k는 키 타입, v는 값 타입.
                    match &it {
                        Ty::Dict(k, v) => {
                            self.declare_mut(var, (**k).clone());
                            self.declare_mut(v2, (**v).clone());
                        }
                        Ty::Unknown => {
                            self.declare_mut(var, Ty::Unknown);
                            self.declare_mut(v2, Ty::Unknown);
                        }
                        other => {
                            self.errors.push(
                                err(
                                    "T0009",
                                    tr!(
                                        format!("`for 키, 값 in ...`은 사전만 되는데 {}입니다", other),
                                        format!("`for key, value in ...` only works on dicts, found {}", other)
                                    ),
                                    *line,
                                    1,
                                )
                                .with_fix(if matches!(other, Ty::List(e) if matches!(**e, Ty::Tuple(_))) {
                                    tr!(
                                        "튜플 리스트는 괄호로 묶어 풉니다: `for (a, b) in xs:`",
                                        "destructure a list of tuples with parentheses: `for (a, b) in xs:`"
                                    )
                                } else {
                                    tr!(
                                        "`for k, v in d:` 는 사전에만 씁니다. 리스트는 `for x in xs:`",
                                        "`for k, v in d:` is only for dicts; for a list use `for x in xs:`"
                                    )
                                }),
                            );
                            self.declare_mut(var, Ty::Unknown);
                            self.declare_mut(v2, Ty::Unknown);
                        }
                    }
                } else {
                    let elem = match &it {
                        Ty::List(t) => (**t).clone(),
                        Ty::Str => Ty::Str,
                        // JSON 리스트의 원소를 돕니다 (리스트가 아니면 한 번도 돌지 않습니다).
                        Ty::Json => Ty::Json,
                        // 통로가 닫히고 다 비울 때까지 받습니다.
                        Ty::Chan(t) => (**t).clone(),
                        Ty::Unknown => Ty::Unknown,
                        other => {
                            self.errors.push(
                                err("T0009", tr!(format!("{} 값은 반복할 수 없습니다", other), format!("cannot iterate over {}", other)), *line, 1)
                                    .with_fix(tr!(
                                        "리스트나 문자열, `range(n)`, 또는 사전은 `for k, v in d:`",
                                        "iterate over a list, a string, `range(n)`, or a dict with `for k, v in d:`"
                                    )),
                            );
                            Ty::Unknown
                        }
                    };
                    // 반복 변수는 바꿀 수 있게 둡니다(각 회전의 지역 복사본).
                    self.declare_mut(var, elem);
                }
                for s in body {
                    self.check_stmt(s);
                }
                self.pop_scope();
            }

            Stmt::Match { subject, cases, line } => {
                let st = self.infer(subject);
                // 설계 문서 §4.5: match는 전수 검사됩니다. P1에서는 실행 중에
                // 걸렸지만 여기서부터는 컴파일 시점에 걸립니다.
                if let Ty::Enum(ename) = &st {
                    let ed = self.enums.get(ename).cloned();
                    if let Some(ed) = ed {
                        let mut covered: HashSet<String> = HashSet::new();
                        let mut has_wild = false;
                        for c in cases {
                            match &c.pattern {
                                Pattern::Wildcard | Pattern::Bind(_) => has_wild = true,
                                Pattern::Variant(n, binds) => {
                                    match ed.variants.iter().find(|v| &v.name == n) {
                                        Some(vd) => {
                                            if !binds.is_empty() && binds.len() != vd.fields.len() {
                                                self.errors.push(err(
                                                    "T0010",
                                                    tr!(
                                                        format!(
                                                            "`{}`은(는) 필드가 {}개인데 {}개를 받으려 합니다",
                                                            n,
                                                            vd.fields.len(),
                                                            binds.len()
                                                        ),
                                                        format!(
                                                            "`{}` has {} fields, but the pattern binds {}",
                                                            n,
                                                            vd.fields.len(),
                                                            binds.len()
                                                        )
                                                    ),
                                                    c.line,
                                                    1,
                                                ));
                                            }
                                            covered.insert(n.clone());
                                        }
                                        None => self.errors.push(
                                            err(
                                                "T0011",
                                                tr!(format!("`{}`에 `{}` 변형이 없습니다", ename, n), format!("`{}` has no variant `{}`", ename, n)),
                                                c.line,
                                                1,
                                            )
                                            .with_fix(format!(
                                                "{}{}",
                                                tr!("있는 변형: ", "available variants: "),
                                                ed.variants
                                                    .iter()
                                                    .map(|v| v.name.clone())
                                                    .collect::<Vec<_>>()
                                                    .join(", ")
                                            )),
                                        ),
                                    }
                                }
                                Pattern::Literal(_) => {}
                            }
                        }
                        if !has_wild {
                            let missing: Vec<String> = ed
                                .variants
                                .iter()
                                .filter(|v| !covered.contains(&v.name))
                                .map(|v| v.name.clone())
                                .collect();
                            if !missing.is_empty() {
                                self.errors.push(
                                    err(
                                        "T0012",
                                        tr!(
                                            format!("match가 {}을(를) 빠뜨렸습니다", missing.join(", ")),
                                            format!("non-exhaustive match: missing {}", missing.join(", "))
                                        ),
                                        *line,
                                        1,
                                    )
                                    .with_fix({
                                        let has_fields = ed
                                            .variants
                                            .iter()
                                            .find(|v| v.name == missing[0])
                                            .map(|v| !v.fields.is_empty())
                                            .unwrap_or(false);
                                        if has_fields {
                                            tr!(
                                                format!("`case {}(...):` 를 추가하거나 `case _:` 로 나머지를 받으세요", missing[0]),
                                                format!("add `case {}(...):` or cover the rest with `case _:`", missing[0])
                                            )
                                        } else {
                                            tr!(
                                                format!("`case {}:` 를 추가하거나 `case _:` 로 나머지를 받으세요", missing[0]),
                                                format!("add `case {}:` or cover the rest with `case _:`", missing[0])
                                            )
                                        }
                                    }),
                                );
                            }
                        }
                    }
                }
                // enum 이 아닌 값에 `case Some(v):` / `case UnknownAccount(id):` 같은 패턴을 쓰면
                // 예전에는 조용히 넘어가고 "`v`를 찾을 수 없습니다"만 나왔습니다.
                if !matches!(st, Ty::Enum(_) | Ty::Unknown | Ty::Var(_)) {
                    for c in cases {
                        if let Pattern::Variant(n, binds) = &c.pattern {
                            let known_variant = self.variant_of.contains_key(n)
                                || matches!(n.as_str(), "Some" | "None" | "Ok" | "Err");
                            if binds.is_empty() && !known_variant {
                                continue;
                            }
                            let fix = match &st {
                                Ty::Optional(inner) => tr!(
                                    format!(
                                        "`?{}` 는 match 로 풀지 않습니다. `if x != none:` 안에서 x 를 {} 로 씁니다 (Some/None 은 없습니다)",
                                        inner, inner
                                    ),
                                    format!(
                                        "optional `?{}` is not unwrapped with match; inside `if x != none:`, x is {} (there is no Some/None)",
                                        inner, inner
                                    )
                                ),
                                Ty::Fallible(..) => tr!(
                                    "`!T` 는 `try` 나 `x = f() catch e:` 로 풉니다 (Ok/Err 는 없습니다)",
                                    "unwrap a fallible `!T` with `try` or `x = f() catch e:` (there is no Ok/Err)"
                                )
                                .to_string(),
                                Ty::Str => tr!(
                                    "글자는 `case \"값\":` 처럼 맞춥니다. 오류 종류로 나누려면 `E!T` 처럼 enum 오류 타입을 쓰세요",
                                    "match strings with `case \"value\":`; to distinguish error kinds, use an enum error type like `E!T`"
                                )
                                .to_string(),
                                _ => tr!(
                                    format!("{} 값은 리터럴(`case 0:`)이나 `case _:` 로 맞춥니다", st),
                                    format!("match {} values with literals (`case 0:`) or `case _:`", st)
                                ),
                            };
                            self.errors.push(
                                err(
                                    "T0066",
                                    tr!(
                                        format!("{} 값에 `{}(...)` 패턴을 쓸 수 없습니다", st, n),
                                        format!("cannot use the pattern `{}(...)` on {}", n, st)
                                    ),
                                    c.line,
                                    1,
                                )
                                .with_fix(fix),
                            );
                        }
                    }
                }
                // Str·Int 같은 값은 가짓수가 끝이 없으니 `case _:` 가 있어야 합니다.
                // 없으면 예전에는 run 은 실행 중 오류, build 는 조용히 지나갔습니다.
                if !matches!(st, Ty::Enum(_) | Ty::Unknown | Ty::Var(_)) {
                    let has_wild = cases.iter().any(|c| matches!(c.pattern, Pattern::Wildcard | Pattern::Bind(_)));
                    let bool_full = matches!(st, Ty::Bool)
                        && [true, false].iter().all(|b| {
                            cases.iter().any(|c| matches!(&c.pattern, Pattern::Literal(Expr::Bool(x)) if x == b))
                        });
                    let bad_pattern = cases.iter().any(|c| matches!(c.pattern, Pattern::Variant(..)));
                    if !has_wild && !bool_full && !bad_pattern && !cases.is_empty() {
                        self.errors.push(
                            err(
                                "T0068",
                                tr!(
                                    format!("{} 값의 match 에 나머지를 받는 갈래가 없습니다", st),
                                    format!("match on {} has no catch-all case", st)
                                ),
                                *line,
                                1,
                            )
                            .with_fix(tr!(
                                "맞는 case 가 없을 때를 위해 마지막에 `case _:` 를 넣으세요",
                                "add a final `case _:` for values no other case matches"
                            )),
                        );
                    }
                }
                for c in cases {
                    self.push_scope();
                    if let Pattern::Variant(_, binds) = &c.pattern {
                        // 위에서 알린 잘못된 패턴이 "`v`를 찾을 수 없습니다"로 번지지 않게 합니다.
                        if !matches!(st, Ty::Enum(_)) {
                            for b in binds {
                                self.declare(b, Ty::Unknown);
                            }
                        }
                    }
                    if let Pattern::Variant(n, binds) = &c.pattern {
                        if let Ty::Enum(ename) = &st {
                            let ed = self.enums.get(ename).cloned();
                            if let Some(ed) = ed {
                                if let Some(vd) = ed.variants.iter().find(|v| &v.name == n) {
                                    for (i, b) in binds.iter().enumerate() {
                                        let t = vd
                                            .fields
                                            .get(i)
                                            .and_then(|f| f.ty.clone())
                                            .map(|te| self.resolve(&te, c.line))
                                            .unwrap_or(Ty::Unknown);
                                        self.declare(b, t);
                                    }
                                }
                            }
                        }
                    }
                    if let Pattern::Bind(n) = &c.pattern {
                        self.declare(n, st.clone());
                    }
                    for s in &c.body {
                        self.check_stmt(s);
                    }
                    self.pop_scope();
                }
            }

            Stmt::Return(v, l, c) => {
                let t = match v {
                    Some(e) => {
                        let h = Some(self.cur_ret.clone());
                        self.infer_hinted(e, h)
                    }
                    None => Ty::Unit,
                };
                if self.arena_depth > 0 {
                    if let Some(e) = v {
                        if let Some(name) = self.mentions_tainted(e) {
                            self.errors.push(
                                err(
                                    "T0040",
                                    tr!(
                                        format!("`{}`은(는) 아레나에서 나온 값이라 블록 밖으로 내보낼 수 없습니다", name),
                                        format!("`{}` comes from an arena and cannot leave the block", name)
                                    ),
                                    *l,
                                    *c,
                                )
                                .with_fix(tr!(
                                    "아레나 메모리는 블록 끝에서 전부 해제됩니다. 필요한 값은 복사해서 내보내세요",
                                    "arena memory is freed at the end of the block; return a copy of the value you need"
                                )),
                            );
                        }
                    }
                }
                let want = self.cur_ret.clone();
                // `return error(x)` 의 오류 타입이 틀린 경우는 error() 쪽에서 이미 T0073 으로 알렸습니다.
                let from_error_call = matches!(&t, Ty::Fallible(ok, _) if **ok == Ty::Unknown);
                if !from_error_call && !self.compatible(&want, &t) {
                    let fname = self.cur_fn.clone();
                    self.errors.push(
                        err(
                            "T0013",
                            tr!(
                                format!("`{}`은(는) {}을(를) 반환해야 하는데 {}을(를) 반환합니다", fname, want, t),
                                format!("`{}` should return {}, but returns {}", fname, want, t)
                            ),
                            *l,
                            *c,
                        )
                        .with_fix(tr!("반환 타입 표기를 고치거나 반환하는 값을 바꾸세요", "fix the declared return type or change the returned value")),
                    );
                }
            }

            Stmt::Arena { name, body, line } => {
                self.arena_bases.push(self.scopes.len());
                self.push_scope();
                self.declare(name, Ty::Arena);
                self.arena_depth += 1;
                for s in body {
                    self.check_stmt(s);
                }
                self.arena_depth -= 1;
                self.arena_bases.pop();
                self.pop_scope();
                let _ = line;
            }

            Stmt::Unsafe { body, .. } => {
                self.unsafe_depth += 1;
                self.push_scope();
                for s in body {
                    self.check_stmt(s);
                }
                self.pop_scope();
                self.unsafe_depth -= 1;
            }

            Stmt::Break(_, _) | Stmt::Continue(_, _) => {}
        }
    }

    /// 이 식이 아레나에서 나온 값을 건드리는가.
    fn mentions_tainted(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Ident(n, _, _) => {
                if self.tainted.contains(n) {
                    Some(n.clone())
                } else {
                    None
                }
            }
            Expr::Unary(_, i, _, _) | Expr::Try(i, _, _) => self.mentions_tainted(i),
            Expr::Binary(_, a, b, _, _) | Expr::OrElse(a, b, _, _) => {
                self.mentions_tainted(a).or_else(|| self.mentions_tainted(b))
            }
            Expr::Field(o, _, _, _) => self.mentions_tainted(o),
            Expr::Index(o, i, _, _) => {
                self.mentions_tainted(o).or_else(|| self.mentions_tainted(i))
            }
            Expr::Call { callee, args, .. } => self.mentions_tainted(callee).or_else(|| {
                args.iter().find_map(|a| self.mentions_tainted(&a.value))
            }),
            Expr::List(items) | Expr::Tuple(items) => {
                items.iter().find_map(|i| self.mentions_tainted(i))
            }
            _ => None,
        }
    }

    // ------------------------------------------------------------- 표현식

    pub fn infer(&mut self, e: &Expr) -> Ty {
        match e {
            Expr::Lambda(f, _, c) => {
                let h = self.lambda_hint.take();
                self.check_closure(f, h, *c)
            }
            Expr::Spawn(f, l, c) => {
                self.lambda_hint = None;
                self.check_spawn_captures(f, *l, *c);
                match self.check_closure(f, None, *c) {
                    Ty::Fn(_, r) => Ty::Task(r),
                    _ => Ty::Task(Box::new(Ty::Unknown)),
                }
            }
            Expr::Int(_) => Ty::Int,
            Expr::Float(_) => Ty::Float,
            Expr::Str(_) => Ty::Str,
            Expr::Bool(_) => Ty::Bool,
            Expr::NoneLit => Ty::NoneTy,
            Expr::FString(parts) => {
                for p in parts {
                    if let FStrPart::Expr(inner, _) = p {
                        self.infer(inner);
                    }
                }
                Ty::Str
            }

            Expr::Ident(n, l, c) => match self.lookup(n) {
                Some(t) => t,
                None => {
                    if self.structs.contains_key(n) {
                        return Ty::Struct(n.clone());
                    }
                    if let Some(en) = self.variant_of.get(n).cloned() {
                        // 값을 담는 변형을 괄호 없이 쓰면 오류입니다.
                        if let Some(ed) = self.enums.get(&en) {
                            if let Some(v) = ed.variants.iter().find(|v| &v.name == n) {
                                if !v.fields.is_empty() {
                                    self.errors.push(
                                        err(
                                            "T0043",
                                            tr!(
                                                format!("`{}`은(는) 값을 담는 변형이라 `{}(...)` 처럼 만들어야 합니다", n, n),
                                                format!("variant `{}` carries values, so it must be constructed as `{}(...)`", n, n)
                                            ),
                                            *l,
                                            *c,
                                        )
                                        .with_fix(tr!("괄호를 열고 필드 값을 채우세요", "add parentheses and fill in the field values")),
                                    );
                                }
                            }
                        }
                        return Ty::Enum(en.clone());
                    }
                    // 최상위 함수 이름을 값으로 쓰면 함수 타입이 됩니다.
                    if let Some(sig) = self.fns.get(n).cloned() {
                        let params: Vec<Ty> = sig
                            .params
                            .iter()
                            .filter(|(pn, _)| pn != "self")
                            .map(|(_, t)| t.clone())
                            .collect();
                        return Ty::Fn(params, Box::new(sig.ret.clone()));
                    }
                    let fix = match n.as_str() {
                        "None" | "null" | "nil" | "NULL" | "nullptr" => tr!("없음은 소문자 `none` 입니다", "no value is written `none` (lowercase)").to_string(),
                        "True" | "False" => tr!(
                            format!("참/거짓은 소문자 `{}` 입니다", n.to_lowercase()),
                            format!("booleans are lowercase: `{}`", n.to_lowercase())
                        ),
                        "this" => tr!("메서드 안에서 자기 자신은 `self` 입니다", "inside a method, the receiver is `self`").to_string(),
                        _ if self.enums.contains_key(n.as_str()) => {
                            let vs: Vec<String> = self.enums[n.as_str()].variants.iter().map(|v| v.name.clone()).collect();
                            tr!(
                                format!("enum 변형은 `{}.` 없이 이름만 씁니다: {}", n, vs.join(", ")),
                                format!("enum variants are written without `{}.`, just the name: {}", n, vs.join(", "))
                            )
                        }
                        _ if self.structs.contains_key(n.as_str()) => {
                            tr!(
                                format!("구조체 값은 `{}(필드: 값, ...)` 으로 만듭니다", n),
                                format!("create a struct value with `{}(field: value, ...)`", n)
                            )
                        }
                        _ => {
                            let mut cands: Vec<String> = Vec::new();
                            for sc in &self.scopes {
                                cands.extend(sc.keys().filter(|k| !k.contains('.')).cloned());
                            }
                            cands.extend(self.fns.keys().cloned());
                            match best_match(&cands, n) {
                                Some(b) => tr!(
                                    format!("`{}` 를 찾으셨나요? 처음 쓰는 이름이면 `let {} = ...` 이나 `var {} = ...` 로 만드세요", b, n, n),
                                    format!("did you mean `{}`? to introduce a new name, use `let {} = ...` or `var {} = ...`", b, n, n)
                                ),
                                None => tr!(
                                    format!("처음 쓰는 이름이면 `let {} = ...`(안 바꿀 때) 이나 `var {} = ...`(바꿀 때) 로 만드세요. 표준 함수면 import 했는지 보세요", n, n),
                                    format!("to introduce a new name, use `let {} = ...` (immutable) or `var {} = ...` (mutable); for a standard library function, check that it is imported", n, n)
                                ),
                            }
                        }
                    };
                    self.errors.push(
                        err("T0014", tr!(format!("`{}`을(를) 찾을 수 없습니다", n), format!("cannot find `{}`", n)), *l, *c).with_fix(fix),
                    );
                    Ty::Unknown
                }
            },

            Expr::List(items) => {
                if items.is_empty() {
                    return Ty::List(Box::new(Ty::Unknown));
                }
                let mut first = self.infer(&items[0]);
                for it in &items[1..] {
                    let t = self.infer(it);
                    // `[[], [1]]` 처럼 앞 원소 타입이 덜 정해졌으면 뒤 원소로 채웁니다.
                    if Self::is_open(&first) && !Self::is_open(&t) && self.compatible(&first, &t) {
                        first = t.clone();
                    }
                    // `[none, 5]` 이나 `[5, none]` 은 `[?Int]` 입니다.
                    match (&first, &t) {
                        (Ty::NoneTy, Ty::NoneTy) | (Ty::Optional(_), _) => {}
                        (Ty::NoneTy, other) => first = Ty::Optional(Box::new(other.clone())),
                        (f, Ty::NoneTy) => first = Ty::Optional(Box::new(f.clone())),
                        _ => {}
                    }
                    if !self.compatible(&first, &t) {
                        let (l, c) = it.pos();
                        self.errors.push(
                            err(
                                "T0015",
                                tr!(
                                    format!("리스트 안에 {}와(과) {}이(가) 섞여 있습니다", first, t),
                                    format!("list mixes {} and {}", first, t)
                                ),
                                l,
                                c,
                            )
                            .with_fix(tr!("리스트의 원소는 모두 같은 타입이어야 합니다", "all list elements must have the same type")),
                        );
                    }
                }
                Ty::List(Box::new(first))
            }

            Expr::Tuple(items) => {
                Ty::Tuple(items.iter().map(|i| self.infer(i)).collect())
            }

            Expr::Dict(pairs) => {
                if pairs.is_empty() {
                    return Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Unknown));
                }
                let k = self.infer(&pairs[0].0);
                let v = self.infer(&pairs[0].1);
                for (pk, pv) in &pairs[1..] {
                    self.infer(pk);
                    self.infer(pv);
                }
                Ty::Dict(Box::new(k), Box::new(v))
            }

            Expr::Unary(op, inner, l, c) => {
                let t = self.infer(inner);
                match op {
                    UnOp::Not => {
                        if !self.compatible(&Ty::Bool, &t) {
                            self.errors.push(
                                err("T0004", tr!(format!("`not`은 Bool에만 쓸 수 있는데 {}입니다", t), format!("`not` only works on Bool, found {}", t)), *l, *c)
                                    .with_fix(tr!("Siskin에는 암묵적 참/거짓 변환이 없습니다", "Siskin has no implicit truthiness")),
                            );
                        }
                        Ty::Bool
                    }
                    UnOp::Neg => {
                        if !t.is_numeric() && t != Ty::Unknown {
                            self.errors.push(err(
                                "T0016",
                                tr!(format!("{} 값에는 단항 `-`를 쓸 수 없습니다", t), format!("unary `-` cannot be used on {} values", t)),
                                *l,
                                *c,
                            ));
                        }
                        t
                    }
                }
            }

            Expr::Binary(op, a, b, l, c) => {
                let at = self.infer(a);
                // `a and b`: 오른쪽은 왼쪽이 참이라는 가정 아래 봅니다.
                // 그래서 `v != none and v > 0` 에서 오른쪽의 v가 좁혀집니다.
                let bt = if *op == BinOp::And {
                    let narrow = self.narrowing(a, true, &Region::Expr(b));
                    self.push_scope();
                    for (n, t) in narrow {
                        self.declare(&n, t);
                    }
                    let bt = self.infer(b);
                    self.pop_scope();
                    bt
                } else {
                    self.infer(b)
                };
                use BinOp::*;
                match op {
                    And | Or => {
                        for (t, side, side_en) in [(&at, "왼쪽", "left"), (&bt, "오른쪽", "right")] {
                            if !self.compatible(&Ty::Bool, t) {
                                self.errors.push(
                                    err(
                                        "T0004",
                                        tr!(
                                            format!("`{}`의 {}은 Bool이어야 하는데 {}입니다", op.symbol(), side, t),
                                            format!("{} operand of `{}` must be Bool, found {}", side_en, op.symbol(), t)
                                        ),
                                        *l,
                                        *c,
                                    )
                                    .with_fix(tr!("Siskin에는 암묵적 참/거짓 변환이 없습니다", "Siskin has no implicit truthiness")),
                                );
                            }
                        }
                        Ty::Bool
                    }
                    Eq | Ne => {
                        self.check_eq_types(&at, &bt, *l, *c);
                        Ty::Bool
                    }
                    Lt | Le | Gt | Ge => {
                        if at != bt && at != Ty::Unknown && bt != Ty::Unknown {
                            self.errors.push(
                                err(
                                    "T0017",
                                    tr!(format!("{}와(과) {}을(를) 비교할 수 없습니다", at, bt), format!("cannot compare {} with {}", at, bt)),
                                    *l,
                                    *c,
                                )
                                .with_fix(optional_fix(&at, &bt).unwrap_or_else(|| tr!(
                                    "Siskin에는 암묵적 형변환이 없습니다. `float(x)`로 맞추세요",
                                    "Siskin has no implicit conversions; convert with `float(x)`"
                                ).to_string())),
                            );
                        }
                        Ty::Bool
                    }
                    Add | Sub | Mul | Div | Mod => {
                        // 포인터 산술: `p + 8`
                        if let Ty::Raw(_) = &at {
                            if matches!(op, Add | Sub) && bt == Ty::Int {
                                if self.unsafe_depth == 0 {
                                    self.errors.push(
                                        err(
                                            "T0041",
                                            tr!("포인터 연산은 `unsafe:` 안에서만 쓸 수 있습니다", "pointer arithmetic is only allowed inside `unsafe:`"),
                                            *l,
                                            *c,
                                        )
                                            .with_fix(tr!("이 줄을 `unsafe:` 블록 안으로 옮기세요", "move this line into an `unsafe:` block")),
                                    );
                                }
                                return at.clone();
                            }
                        }
                        if at == Ty::Unknown || bt == Ty::Unknown {
                            return if at == Ty::Unknown { bt } else { at };
                        }
                        if at == Ty::Str && bt == Ty::Str && *op == Add {
                            return Ty::Str;
                        }
                        if let (Ty::List(x), Ty::List(_)) = (&at, &bt) {
                            if *op == Add {
                                return Ty::List(x.clone());
                            }
                        }
                        if at != bt {
                            self.errors.push(
                                err(
                                    "T0018",
                                    tr!(
                                        format!("{}와(과) {}에 `{}`을(를) 쓸 수 없습니다", at, bt, op.symbol()),
                                        format!("cannot apply `{}` to {} and {}", op.symbol(), at, bt)
                                    ),
                                    *l,
                                    *c,
                                )
                                .with_fix(optional_fix(&at, &bt).unwrap_or_else(|| tr!(
                                    "Siskin에는 암묵적 형변환이 없습니다. `float(x)` 또는 `int(x)`로 맞추세요",
                                    "Siskin has no implicit conversions; convert with `float(x)` or `int(x)`"
                                ).to_string())),
                            );
                            return Ty::Unknown;
                        }
                        if !at.is_numeric() {
                            self.errors.push(err(
                                "T0018",
                                tr!(
                                    format!("{} 값에 `{}`을(를) 쓸 수 없습니다", at, op.symbol()),
                                    format!("cannot apply `{}` to {} values", op.symbol(), at)
                                ),
                                *l,
                                *c,
                            ));
                            return Ty::Unknown;
                        }
                        at
                    }
                }
            }

            Expr::IfExpr { cond, then, els } => {
                self.check_cond(cond);
                let t = self.infer(then);
                let e2 = self.infer(els);
                if !self.compatible(&t, &e2) {
                    let (l, c) = then.pos();
                    self.errors.push(
                        err(
                            "T0019",
                            tr!(
                                format!("조건 표현식의 두 쪽이 {}와(과) {}로 다릅니다", t, e2),
                                format!("the branches of the conditional expression differ: {} and {}", t, e2)
                            ),
                            l,
                            c,
                        )
                        .with_fix(tr!("양쪽이 같은 타입이어야 합니다", "both branches must have the same type")),
                    );
                }
                t
            }

            Expr::Try(inner, l, c) => {
                let t = self.infer(inner);
                match t {
                    Ty::Fallible(x, inner_err) => {
                        // 오류 타입이 다르면 그대로 전파할 수 없습니다. enum 오류를 Str 오류 함수로
                        // 올릴 때만 글자로 바꿔 줍니다(`NoFunds(need: 5)` 같은 모양).
                        if let Ty::Fallible(_, outer_err) = self.cur_ret.clone() {
                            let same = self.compatible(&outer_err, &inner_err);
                            if !same && !(*outer_err == Ty::Str && matches!(*inner_err, Ty::Enum(_))) {
                                self.errors.push(
                                    err(
                                        "T0072",
                                        tr!(
                                            format!("`try` 로 {} 오류를 올릴 수 없습니다. 이 함수의 오류 타입은 {}입니다", inner_err, outer_err),
                                            format!("`try` cannot propagate an error of type {}; this function's error type is {}", inner_err, outer_err)
                                        ),
                                        *l,
                                        *c,
                                    )
                                    .with_fix(tr!(
                                        format!("`catch e:` 로 받아서 `return error(...)` 로 {} 값을 만드세요", outer_err),
                                        format!("handle it with `catch e:` and build a value of type {} with `return error(...)`", outer_err)
                                    )),
                                );
                            }
                        }
                        if !matches!(self.cur_ret, Ty::Fallible(..)) {
                            let fname = self.cur_fn.clone();
                            let ret = self.cur_ret.clone();
                            self.errors.push(
                                err(
                                    "T0020",
                                    tr!(
                                        format!("`try`는 실패를 전파하는데 `{}`의 반환 타입은 {}입니다", fname, ret),
                                        format!("`try` propagates failure, but `{}` returns {}", fname, ret)
                                    ),
                                    *l,
                                    *c,
                                )
                                .with_fix(tr!(
                                    format!("`fn {}(...) -> !{}` 처럼 `!`를 붙이거나 `catch`로 받으세요", fname, ret),
                                    format!("make it fallible, e.g. `fn {}(...) -> !{}`, or handle the error with `catch`", fname, ret)
                                )),
                            );
                        }
                        *x
                    }
                    Ty::Unknown => Ty::Unknown,
                    other => {
                        self.errors.push(
                            err(
                                "T0021",
                                tr!(format!("`try`는 !T 값에만 쓸 수 있는데 {}입니다", other), format!("`try` only works on fallible !T values, found {}", other)),
                                *l,
                                *c,
                            )
                            .with_fix(tr!("이 함수는 실패하지 않으므로 `try`가 필요 없습니다", "this call cannot fail, so `try` is not needed")),
                        );
                        other
                    }
                }
            }

            Expr::OrElse(a, b, l, c) => {
                let at = self.infer(a);
                let bt = self.infer(b);
                // 왼쪽은 `?T`여야 합니다. 결과는 벗겨진 T입니다.
                let inner = match &at {
                    Ty::Optional(x) => (**x).clone(),
                    Ty::Unknown => Ty::Unknown,
                    other => {
                        self.errors.push(
                            err(
                                "T0048",
                                tr!(
                                    format!("`else` 기본값은 `?T` 값에만 붙일 수 있는데 왼쪽이 {}입니다", other),
                                    format!("an `else` default only applies to optional `?T` values, but the left side is {}", other)
                                ),
                                *l,
                                *c,
                            )
                            .with_fix(tr!("`값이 없을 수 있는 ?T` 뒤에 `else 기본값`을 씁니다", "write `else default` after an optional `?T` value")),
                        );
                        return other.clone();
                    }
                };
                // 기본값은 T(또는 ?T)와 맞아야 합니다.
                if !self.compatible(&inner, &bt) && inner != Ty::Unknown {
                    self.errors.push(
                        err(
                            "T0048",
                            tr!(format!("`else` 기본값은 {}여야 하는데 {}입니다", inner, bt), format!("`else` default must be {}, found {}", inner, bt)),
                            *l,
                            *c,
                        )
                        .with_fix(tr!(
                            "왼쪽 `?T`의 T와 같은 타입을 기본값으로 주세요",
                            "the default must have type T of the optional `?T` on the left"
                        )),
                    );
                }
                inner
            }

            Expr::Index(obj, idx, l, c) => {
                let ot = self.infer(obj);
                let it = self.infer(idx);
                if let Ty::Raw(t) = &ot {
                    if self.unsafe_depth == 0 {
                        self.errors.push(
                            err(
                                "T0041",
                                tr!(
                                    format!("원시 포인터({})는 `unsafe:` 안에서만 쓸 수 있습니다", ot),
                                    format!("raw pointers ({}) can only be used inside `unsafe:`", ot)
                                ),
                                *l,
                                *c,
                            )
                                .with_fix(tr!("이 줄을 `unsafe:` 블록 안으로 옮기세요", "move this line into an `unsafe:` block")),
                        );
                    }
                    if !self.compatible(&Ty::Int, &it) {
                        self.errors.push(err(
                            "T0022",
                            tr!(format!("포인터 인덱스는 Int여야 하는데 {}입니다", it), format!("pointer index must be Int, found {}", it)),
                            *l,
                            *c,
                        ));
                    }
                    return (**t).clone();
                }
                match &ot {
                    Ty::List(t) => {
                        if !self.compatible(&Ty::Int, &it) {
                            self.errors.push(err(
                                "T0022",
                                tr!(format!("리스트 인덱스는 Int여야 하는데 {}입니다", it), format!("list index must be Int, found {}", it)),
                                *l,
                                *c,
                            ));
                        }
                        (**t).clone()
                    }
                    Ty::Str => Ty::Str,
                    Ty::Dict(_, v) => Ty::Optional(v.clone()),
                    Ty::Unknown => Ty::Unknown,
                    other => {
                        self.errors.push(err(
                            "T0023",
                            tr!(format!("{} 값은 인덱싱할 수 없습니다", other), format!("cannot index into {}", other)),
                            *l,
                            *c,
                        ));
                        Ty::Unknown
                    }
                }
            }

            Expr::Field(obj, name, l, c) => {
                if let Some(t) = self.narrowed_expr(e) {
                    return t;
                }
                let ot = self.infer(obj);
                match &ot {
                    Ty::Tuple(elems) if name.chars().all(|ch| ch.is_ascii_digit()) => {
                        let i: usize = name.parse().unwrap_or(usize::MAX);
                        if i < elems.len() {
                            return elems[i].clone();
                        }
                        self.errors.push(
                            err("T0024", tr!(format!("{} 에는 `.{}` 가 없습니다", ot, name), format!("{} has no field `.{}`", ot, name)), *l, *c)
                                .with_fix(tr!(
                                    format!("`.0` 부터 `.{}` 까지 있습니다", elems.len() - 1),
                                    format!("available: `.0` to `.{}`", elems.len() - 1)
                                )),
                        );
                        return Ty::Unknown;
                    }
                    Ty::Struct(sn) => {
                        let sd = self.structs.get(sn).cloned();
                        if let Some(sd) = sd {
                            if let Some(f) = sd.fields.iter().find(|f| &f.name == name) {
                                let line = *l;
                                return match &f.ty {
                                    Some(te) => self.resolve(te, line),
                                    None => Ty::Unknown,
                                };
                            }
                            self.errors.push(
                                err("T0024", tr!(format!("`{}`에 `{}` 필드가 없습니다", sn, name), format!("`{}` has no field `{}`", sn, name)), *l, *c)
                                    .with_fix(format!(
                                        "{}{}",
                                        tr!("있는 필드: ", "available fields: "),
                                        sd.fields
                                            .iter()
                                            .map(|f| f.name.clone())
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    )),
                            );
                        }
                        Ty::Unknown
                    }
                    Ty::Optional(inner) => {
                        self.errors.push(
                            err(
                                "T0025",
                                tr!(
                                    format!("{} 값에서 바로 `{}`을(를) 꺼낼 수 없습니다", ot, name),
                                    format!("cannot access `{}` directly on optional {}", name, ot)
                                ),
                                *l,
                                *c,
                            )
                            .with_fix(tr!(
                                format!("값이 없을 수 있습니다. `if x != none:` 안에서 쓰면 {}로 좁혀집니다", inner),
                                format!("the value may be none; inside `if x != none:` it narrows to {}", inner)
                            )),
                        );
                        Ty::Unknown
                    }
                    Ty::Enum(_) | Ty::Unknown => Ty::Unknown,
                    other => {
                        let fix = match &other {
                            Ty::Fallible(..) => tr!(
                                "실패할 수 있는 값입니다. `let v = try f()` 나 `let v = f() catch e:` 로 먼저 꺼내세요",
                                "this value is fallible; unwrap it first with `let v = try f()` or `let v = f() catch e:`"
                            )
                            .to_string(),
                            Ty::Str | Ty::Int | Ty::Float | Ty::Bool => tr!(
                                format!("{} 에는 필드가 없습니다. 메서드는 `x.{}()` 처럼 괄호를 붙여 부릅니다", other, name),
                                format!("{} has no fields; call a method with parentheses, like `x.{}()`", other, name)
                            ),
                            _ => tr!("필드는 구조체에만 있습니다", "only structs have fields").to_string(),
                        };
                        self.errors.push(
                            err(
                                "T0026",
                                tr!(format!("{} 값에서 `{}`을(를) 꺼낼 수 없습니다", other, name), format!("cannot access `{}` on {}", name, other)),
                                *l,
                                *c,
                            )
                            .with_fix(fix),
                        );
                        Ty::Unknown
                    }
                }
            }

            Expr::Call { callee, targs, args, line, col } => {
                let targs = targs.clone();
                self.infer_call(callee, &targs, args, *line, *col)
            }
        }
    }

    /// 함수 값(값으로 넘긴 함수)을 부르는 경우의 인자 검사. 반환 타입을 냅니다.
    /// 구조체에 같은 이름의 메서드가 없고 함수 타입 필드가 있으면 그 필드 타입.
    pub fn fn_field(&mut self, ot: &Ty, name: &str) -> Option<Ty> {
        let sn = match ot {
            Ty::Struct(n) => n.clone(),
            _ => return None,
        };
        if self.methods.contains_key(&format!("{}.{}", sn, name)) {
            return None;
        }
        let sd = self.structs.get(&sn).cloned()?;
        let fd = sd.fields.iter().find(|f| f.name == name)?;
        let t = self.resolve(fd.ty.as_ref()?, sd.line);
        if matches!(t, Ty::Fn(..)) {
            Some(t)
        } else {
            None
        }
    }

    fn check_indirect_call(&mut self, params: &[Ty], ret: Ty, args: &[Arg], l: usize, c: usize) -> Ty {
        let mut arg_tys: Vec<Ty> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let h = params.get(i).cloned();
            arg_tys.push(self.infer_hinted(&a.value, h));
        }
        if arg_tys.len() != params.len() {
            self.errors.push(err(
                "T0051",
                tr!(
                    format!(
                        "이 함수 값은 인자 {}개를 받는데 {}개를 주었습니다",
                        params.len(),
                        arg_tys.len()
                    ),
                    format!(
                        "this function value takes {} argument{}, but {} {} given",
                        params.len(),
                        if params.len() == 1 { "" } else { "s" },
                        arg_tys.len(),
                        if arg_tys.len() == 1 { "was" } else { "were" }
                    )
                ),
                l,
                c,
            ));
        }
        for (p, at) in params.iter().zip(&arg_tys) {
            if !self.compatible(p, at) {
                self.errors.push(
                    err(
                        "T0052",
                        tr!(
                            format!("함수 값의 인자가 {}여야 하는데 {}을(를) 주었습니다", p, at),
                            format!("function value argument should be {}, found {}", p, at)
                        ),
                        l,
                        c,
                    )
                    .with_fix(tr!("Siskin에는 암묵적 형변환이 없습니다", "Siskin has no implicit conversions")),
                );
            }
        }
        ret
    }

    fn infer_call(&mut self, callee: &Expr, targs: &[TypeExpr], args: &[Arg], line: usize, col: usize) -> Ty {
        // 메서드 호출
        if let Expr::Field(obj, mname, l, c) = callee {
            // 모듈 함수
            if let Expr::Ident(m, _, _) = obj.as_ref() {
                if self.lookup(m).is_none() && is_std_module(m) {
                    // `time.today()` 처럼 Siskin 으로 쓴 표준 함수는 보통 함수처럼 부릅니다.
                    if self.fns.contains_key(mname.as_str()) {
                        let callee = Expr::Ident(mname.clone(), *l, *c);
                        return self.infer_call(&callee, targs, args, line, col);
                    }
                    let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(&a.value)).collect();
                    return self.builtin_ret(mname, &arg_tys, *l, *c);
                }
            }
            let ot = self.infer(obj);
            // 함수 타입 필드를 부르기: `self.on_click(x)`
            if let Some(Ty::Fn(ps, r)) = self.fn_field(&ot, mname) {
                return self.check_indirect_call(&ps, *r, args, *l, *c);
            }
            // `fs.push(fn(x): ...)` — 리스트 원소 타입이 익명 함수 인자 타입을 알려 줍니다.
            let push_hint = match (&ot, mname.as_str()) {
                (Ty::List(inner), "push") => Some((**inner).clone()),
                // `xs.map(fn(x): ...)` — x 는 원소 타입입니다.
                (Ty::List(inner), "map" | "filter" | "any" | "all" | "sort_by") => {
                    Some(Ty::Fn(vec![(**inner).clone()], Box::new(Ty::Unknown)))
                }
                _ => None,
            };
            let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer_hinted(&a.value, push_hint.clone())).collect();
            if ot == Ty::Arena {
                let elem = match targs.first() {
                    Some(te) => self.resolve(te, *l),
                    None => {
                        self.errors.push(
                            err(
                                "T0043",
                                tr!(format!("`a.{}`에는 타입을 적어야 합니다", mname), format!("`a.{}` needs a type argument", mname)),
                                *l,
                                *c,
                            )
                            .with_fix(tr!(
                                "`a.list[Int]()` 처럼 대괄호 안에 타입을 씁니다",
                                "write the type in square brackets, e.g. `a.list[Int]()`"
                            )),
                        );
                        Ty::Unknown
                    }
                };
                return match mname.as_str() {
                    "list" => Ty::List(Box::new(elem)),
                    "alloc" => {
                        if arg_tys.len() != 1 {
                            self.errors.push(err(
                                "T0044",
                                tr!("`a.alloc[T](개수)` 는 인자 하나를 받습니다", "`a.alloc[T](count)` takes exactly one argument"),
                                *l,
                                *c,
                            ));
                        }
                        Ty::Raw(Box::new(elem))
                    }
                    other => {
                        self.errors.push(
                            err("T0042", tr!(format!("아레나에 `{}` 메서드가 없습니다", other), format!("arena has no method `{}`", other)), *l, *c)
                                .with_fix(tr!("쓸 수 있는 것: `a.list[T]()`, `a.alloc[T](개수)`", "available: `a.list[T]()`, `a.alloc[T](count)`")),
                        );
                        Ty::Unknown
                    }
                };
            }
            // 값 의미론: 읽기 전용 값에는 제자리 변경 메서드를 쓸 수 없습니다.
            const MUTATING: &[&str] = &["push", "pop", "sort", "sort_by", "reverse", "clear"];
            if matches!(ot, Ty::List(_)) && MUTATING.contains(&mname.as_str()) {
                if let Some(root) = root_ident(obj) {
                    if self.is_captured(root) {
                        let e = self.captured_error(root, *l, *c);
                        self.errors.push(e);
                    } else if !self.is_mutable(root) {
                        let fix = if root == "self" {
                            tr!("메서드를 `fn 이름(inout self, ...)`으로 선언하세요", "declare the method as `fn name(inout self, ...)`").to_string()
                        } else {
                            tr!(
                                format!("`{}`을(를) `var`로 선언하거나, 함수 인자라면 `inout`으로 받으세요", root),
                                format!("declare `{}` with `var`, or take it as `inout` if it is a parameter", root)
                            )
                        };
                        self.errors.push(
                            err(
                                "T0045",
                                tr!(
                                    format!("`{}`은(는) 읽기 전용이라 `{}`(으)로 바꿀 수 없습니다", root, mname),
                                    format!("`{}` is read-only and cannot be modified with `{}`", root, mname)
                                ),
                                *l,
                                *c,
                            )
                            .with_fix(fix),
                        );
                    }
                }
            }
            // `inout self`나 `inout` 메서드 인자도 바꿀 수 있는 변수여야 합니다.
            if let Some(k) = match &ot {
                Ty::Struct(n) | Ty::Enum(n) => Some(format!("{}.{}", n, mname)),
                _ => None,
            } {
                if let Some(sig) = self.methods.get(&k).cloned() {
                    let decl = sig.decl.clone();
                    if decl.params.iter().any(|p| p.is_self && p.conv == Convention::Inout) {
                        self.check_inout_arg(obj.as_ref(), mname, *l, *c);
                    }
                    let non_self_convs: Vec<Convention> =
                        decl.params.iter().filter(|p| !p.is_self).map(|p| p.conv).collect();
                    for (i, cv) in non_self_convs.iter().enumerate() {
                        if *cv == Convention::Inout {
                            if let Some(a) = args.get(i) {
                                self.check_inout_arg(&a.value, mname, *l, *c);
                            }
                        }
                    }
                }
            }
            return self.method_ret(&ot, mname, &arg_tys, *l, *c);
        }

        if let Expr::Ident(name, l, c) = callee {
            // 함수 값(지역 변수·인자로 받은 함수)을 부르면 간접 호출입니다.
            if let Some(Ty::Fn(params, ret)) = self.lookup(name) {
                return self.check_indirect_call(&params, *ret, args, *l, *c);
            }
            // 구조체 생성
            if let Some(sd) = self.structs.get(name).cloned() {
                return self.check_ctor(&sd.name, &sd.fields, args, *l, *c, Ty::Struct(sd.name.clone()));
            }
            // 열거형 변형 생성
            if let Some(ename) = self.variant_of.get(name).cloned() {
                let ed = self.enums.get(&ename).cloned().unwrap();
                let vd = ed.variants.iter().find(|v| &v.name == name).unwrap().clone();
                return self.check_ctor(&vd.name, &vd.fields, args, *l, *c, Ty::Enum(ename));
            }
            // 사용자 함수
            if let Some(sig) = self.fns.get(name).cloned() {
                let want: Vec<(String, Ty)> =
                    sig.params.iter().filter(|(n, _)| n != "self").cloned().collect();
                // 익명 함수 인자는 나중에 봅니다. 다른 인자로 `T` 가 먼저 정해져야
                // `fn(x): ...` 의 `x` 타입을 알 수 있기 때문입니다.
                let mut arg_tys = Vec::new();
                let mut deferred: Vec<(usize, Option<Ty>)> = Vec::new();
                let mut ppos = 0usize;
                for (i, a) in args.iter().enumerate() {
                    let pty = match &a.name {
                        Some(n) => want.iter().find(|(wn, _)| wn == n).map(|(_, t)| t.clone()),
                        None => {
                            ppos += 1;
                            want.get(ppos - 1).map(|(_, t)| t.clone())
                        }
                    };
                    if matches!(a.value, Expr::Lambda(..)) {
                        arg_tys.push((a.name.clone(), Ty::Unknown));
                        deferred.push((i, pty));
                    } else {
                        arg_tys.push((a.name.clone(), self.infer(&a.value)));
                    }
                }
                let positional = arg_tys.iter().filter(|(n, _)| n.is_none()).count();
                if positional > want.len() {
                    self.errors.push(err(
                        "T0027",
                        tr!(
                            format!("`{}`은(는) 인자 {}개를 받는데 {}개를 주었습니다", name, want.len(), positional),
                            format!(
                                "`{}` takes {} argument{}, but {} {} given",
                                name,
                                want.len(),
                                if want.len() == 1 { "" } else { "s" },
                                positional,
                                if positional == 1 { "was" } else { "were" }
                            )
                        ),
                        *l,
                        *c,
                    ));
                }
                // 제네릭 함수면 인자 타입으로 타입 매개변수(T 등)를 먼저 채웁니다.
                let mut subst: HashMap<String, Ty> = HashMap::new();
                if !sig.decl.generics.is_empty() {
                    let mut gpi = 0usize;
                    for (an, at) in &arg_tys {
                        let pty = match an {
                            Some(n) => want.iter().find(|(wn, _)| wn == n).map(|(_, t)| t.clone()),
                            None => {
                                let t = want.get(gpi).map(|(_, t)| t.clone());
                                gpi += 1;
                                t
                            }
                        };
                        if let Some(pty) = pty {
                            self.unify(&pty, at, &mut subst);
                        }
                    }
                }
                for (i, pty) in deferred {
                    let hint = pty.as_ref().map(|t| {
                        if subst.is_empty() { t.clone() } else { self.substitute(t, &subst) }
                    });
                    let t = self.infer_hinted(&args[i].value, hint);
                    if let Some(p) = &pty {
                        self.unify(p, &t, &mut subst);
                    }
                    arg_tys[i].1 = t;
                }
                let mut pi = 0usize;
                for (i, (an, at)) in arg_tys.iter().enumerate() {
                    let bound: Option<(String, Ty)> = match an {
                        Some(n) => match want.iter().find(|(wn, _)| wn == n) {
                            Some((wn, t)) => Some((wn.clone(), t.clone())),
                            None => {
                                self.errors.push(
                                    err("T0028", tr!(format!("`{}`에 `{}`이라는 인자가 없습니다", name, n), format!("`{}` has no parameter named `{}`", name, n)), *l, *c)
                                        .with_fix(format!(
                                            "{}{}",
                                            tr!("받는 인자: ", "parameters: "),
                                            want.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>().join(", ")
                                        )),
                                );
                                None
                            }
                        },
                        None => {
                            let t = want.get(pi).cloned();
                            pi += 1;
                            t
                        }
                    };
                    if let Some((pname, t)) = bound {
                        let t = if subst.is_empty() { t } else { self.substitute(&t, &subst) };
                        if !self.compatible(&t, at) {
                            self.errors.push(
                                err(
                                    "T0029",
                                    tr!(
                                        format!("`{}`의 인자가 {}여야 하는데 {}을(를) 주었습니다", name, t, at),
                                        format!("argument to `{}` should be {}, found {}", name, t, at)
                                    ),
                                    *l,
                                    *c,
                                )
                                .with_fix(tr!("Siskin에는 암묵적 형변환이 없습니다", "Siskin has no implicit conversions")),
                            );
                        }
                        // `inout` 인자는 바꿀 수 있는 변수여야 합니다.
                        let inout = sig
                            .decl
                            .params
                            .iter()
                            .find(|p| p.name == pname)
                            .map(|p| p.conv == Convention::Inout)
                            .unwrap_or(false);
                        if inout {
                            self.check_inout_arg(&args[i].value, name, *l, *c);
                        }
                    }
                }
                if positional < want.len() && arg_tys.iter().all(|(n, _)| n.is_none()) {
                    self.errors.push(
                        err(
                            "T0030",
                            tr!(
                                format!("`{}`의 인자 `{}`이(가) 빠졌습니다", name, want[positional].0),
                                format!("missing argument `{}` to `{}`", want[positional].0, name)
                            ),
                            *l,
                            *c,
                        )
                        .with_fix(format!(
                            "{}{}",
                            tr!("필요한 인자: ", "required parameters: "),
                            want.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>().join(", ")
                        )),
                    );
                }
                return if subst.is_empty() {
                    sig.ret.clone()
                } else {
                    self.substitute(&sig.ret, &subst)
                };
            }
            // `channel[T]()` / `channel[T](크기)` — 작업끼리 값을 주고받는 통로.
            if name == "channel" {
                let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(&a.value)).collect();
                if arg_tys.len() > 1 || arg_tys.first().map_or(false, |t| !self.compatible(&Ty::Int, t)) {
                    self.errors.push(
                        err("T0047", tr!("`channel[T](크기)` 는 크기(Int) 하나만 받을 수 있습니다", "`channel[T](size)` takes at most one size (Int)"), *l, *c)
                            .with_fix(tr!(
                                "크기를 주면 그만큼 찼을 때 보내는 쪽이 기다립니다. 안 주면 끝없이 쌓입니다",
                                "with a size, senders wait when that many values are queued; without one the queue grows as needed"
                            )),
                    );
                }
                return match targs.first() {
                    Some(te) => {
                        let et = self.resolve(te, *l);
                        if matches!(et, Ty::Json | Ty::Raw(_) | Ty::Arena) {
                            self.errors.push(
                                err("T0075", tr!(format!("통로로 {} 값을 보낼 수 없습니다", et), format!("a channel cannot carry {} values", et)), *l, *c)
                                    .with_fix(tr!(
                                        "통로는 복사되는 값만 나릅니다. JSON 은 `json.stringify` 로 글자로 바꿔 보내세요",
                                        "channels carry copied values only; send JSON as a Str made with `json.stringify`"
                                    )),
                            );
                        }
                        Ty::Chan(Box::new(et))
                    }
                    None => {
                        self.errors.push(
                            err("T0074", tr!("`channel` 에는 주고받을 값의 타입을 적어야 합니다", "`channel` needs the type of the values it carries"), *l, *c)
                                .with_fix(tr!("`channel[Int]()` 처럼 씁니다", "write it like `channel[Int]()`")),
                        );
                        Ty::Chan(Box::new(Ty::Unknown))
                    }
                };
            }
            // 메모리 Level 2 내장 함수
            if matches!(name.as_str(), "alloc" | "free" | "cast" | "cstr" | "ptr_get") {
                if self.unsafe_depth == 0 {
                    self.errors.push(
                        err(
                            "T0041",
                            tr!(format!("`{}`은(는) `unsafe:` 안에서만 쓸 수 있습니다", name), format!("`{}` can only be used inside `unsafe:`", name)),
                            *l,
                            *c,
                        )
                            .with_fix(tr!("이 줄을 `unsafe:` 블록 안으로 옮기세요", "move this line into an `unsafe:` block")),
                    );
                }
                let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(&a.value)).collect();
                let targ = targs.first().map(|te| self.resolve(te, *l));
                return match name.as_str() {
                    // C가 준 손잡이에서 0으로 끝나는 글자열을 읽습니다.
                    "cstr" => {
                        if !matches!(arg_tys.first(), Some(Ty::Int)) {
                            self.errors.push(err(
                                "T0046",
                                tr!("`cstr`은 C에서 받은 손잡이(Int) 하나를 받습니다", "`cstr` takes a single handle (Int) received from C"),
                                *l,
                                *c,
                            ));
                        }
                        Ty::Str
                    }
                    // 손잡이가 가리키는 칸에서 i번째 값을 읽습니다 (8바이트씩).
                    "ptr_get" => {
                        if arg_tys.len() != 2 || !matches!(arg_tys.first(), Some(Ty::Int)) {
                            self.errors.push(
                                err("T0047", tr!("`ptr_get`은 손잡이와 몇 번째인지를 받습니다", "`ptr_get` takes a handle and an index"), *l, *c)
                                    .with_fix(tr!("`ptr_get(손잡이, 0)` 처럼 씁니다", "write it like `ptr_get(handle, 0)`")),
                            );
                        }
                        Ty::Int
                    }
                    "alloc" => match targ {
                        Some(t) => Ty::Raw(Box::new(t)),
                        None => {
                            self.errors.push(
                                err("T0043", tr!("`alloc`에는 타입을 적어야 합니다", "`alloc` needs a type argument"), *l, *c)
                                    .with_fix(tr!("`alloc[Int](16)` 처럼 씁니다", "write it like `alloc[Int](16)`")),
                            );
                            Ty::Unknown
                        }
                    },
                    "free" => {
                        if !matches!(arg_tys.first(), Some(Ty::Raw(_))) {
                            self.errors.push(err("T0045", tr!("`free`는 원시 포인터를 받습니다", "`free` takes a raw pointer"), *l, *c));
                        }
                        Ty::Unit
                    }
                    _ => match targ {
                        Some(t) => t,
                        None => Ty::Unknown,
                    },
                };
            }
            // 내장 함수
            let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(&a.value)).collect();
            return self.builtin_ret(name, &arg_tys, *l, *c);
        }

        // 함수 값을 돌려주는 식을 바로 부르는 경우: `구하기()(x)` 등.
        if let Ty::Fn(params, ret) = self.infer(callee) {
            return self.check_indirect_call(&params, *ret, args, line, col);
        }
        for a in args {
            self.infer(&a.value);
        }
        self.errors.push(err("T0031", tr!("호출할 수 없는 대상입니다", "this expression is not callable"), line, col));
        Ty::Unknown
    }

    fn check_ctor(
        &mut self,
        name: &str,
        fields: &[FieldDecl],
        args: &[Arg],
        l: usize,
        c: usize,
        result: Ty,
    ) -> Ty {
        let mut given: Vec<(Option<String>, Ty)> = Vec::new();
        for a in args {
            given.push((a.name.clone(), self.infer(&a.value)));
        }
        let mut pi = 0usize;
        let mut seen: HashSet<String> = HashSet::new();
        for (an, at) in &given {
            let target = match an {
                Some(n) => {
                    seen.insert(n.clone());
                    match fields.iter().find(|f| &f.name == n) {
                        Some(f) => f.ty.clone(),
                        None => {
                            self.errors.push(
                                err("T0032", tr!(format!("`{}`에 `{}` 필드가 없습니다", name, n), format!("`{}` has no field `{}`", name, n)), l, c)
                                    .with_fix(format!(
                                        "{}{}",
                                        tr!("있는 필드: ", "available fields: "),
                                        fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>().join(", ")
                                    )),
                            );
                            None
                        }
                    }
                }
                None => {
                    let t = fields.get(pi).map(|f| {
                        seen.insert(f.name.clone());
                        f.ty.clone()
                    });
                    pi += 1;
                    t.flatten()
                }
            };
            if let Some(te) = target {
                let want = self.resolve(&te, l);
                if !self.compatible(&want, at) {
                    self.errors.push(
                        err(
                            "T0033",
                            tr!(
                                format!("`{}`의 필드에 {}이(가) 와야 하는데 {}입니다", name, want, at),
                                format!("field of `{}` should be {}, found {}", name, want, at)
                            ),
                            l,
                            c,
                        )
                            .with_fix(tr!("Siskin에는 암묵적 형변환이 없습니다", "Siskin has no implicit conversions")),
                    );
                }
            }
        }
        for f in fields {
            if !seen.contains(&f.name) && f.default.is_none() {
                self.errors.push(
                    err("T0034", tr!(format!("`{}`의 필드 `{}`이(가) 빠졌습니다", name, f.name), format!("missing field `{}` in `{}`", f.name, name)), l, c)
                        .with_fix(format!(
                            "{}{}",
                            tr!("필요한 필드: ", "required fields: "),
                            fields.iter().map(|x| x.name.clone()).collect::<Vec<_>>().join(", ")
                        )),
                );
            }
        }
        result
    }

    fn no_args(&mut self, name: &str, args: &[Ty], l: usize, c: usize) {
        if !args.is_empty() {
            self.errors.push(err(
                "T0027",
                tr!(format!("`{}` 는 인자를 받지 않습니다", name), format!("`{}` takes no arguments", name)),
                l,
                c,
            ));
        }
    }

    /// `spawn` 이 새 작업으로 넘기는 값을 봅니다. 값은 복사되어 넘어가므로 대부분 안전하지만,
    /// 원시 포인터와 아레나 값은 다른 작업이 그 메모리를 풀거나 바꿀 수 있어 막습니다.
    fn check_spawn_captures(&mut self, f: &crate::ast::Shared<FnDecl>, l: usize, c: usize) {
        fn has_json(t: &Ty) -> bool {
            match t {
                Ty::Json => true,
                Ty::List(a) | Ty::Optional(a) | Ty::Chan(a) => has_json(a),
                Ty::Dict(a, b) => has_json(a) || has_json(b),
                Ty::Tuple(ts) => ts.iter().any(has_json),
                _ => false,
            }
        }
        fn has_raw(t: &Ty, ty: &Types, seen: &mut HashSet<String>) -> bool {
            match t {
                Ty::Raw(_) | Ty::Arena | Ty::Json => true,
                Ty::List(a) | Ty::Optional(a) | Ty::Task(a) | Ty::Chan(a) => has_raw(a, ty, seen),
                Ty::Fallible(a, e) => has_raw(a, ty, seen) || has_raw(e, ty, seen),
                Ty::Dict(a, b) => has_raw(a, ty, seen) || has_raw(b, ty, seen),
                Ty::Tuple(ts) => ts.iter().any(|x| has_raw(x, ty, seen)),
                Ty::Struct(n) => {
                    if !seen.insert(n.clone()) {
                        return false;
                    }
                    match ty.structs.get(n) {
                        Some(sd) => sd.fields.iter().any(|fd| match &fd.ty {
                            Some(TypeExpr::Raw(_)) => true,
                            Some(TypeExpr::Named(m, _)) if m == "Json" => true,
                            Some(TypeExpr::Named(m, _)) => has_raw(&Ty::Struct(m.clone()), ty, seen),
                            _ => false,
                        }),
                        None => false,
                    }
                }
                _ => false,
            }
        }
        for n in free_vars(f) {
            let t = match self.lookup(&n) {
                Some(t) => t,
                None => continue,
            };
            let tainted = self.tainted.contains(&n);
            if !tainted && has_json(&t) {
                self.errors.push(
                    err(
                        "T0075",
                        tr!(
                            format!("`{}` 은(는) JSON 값이라 `spawn` 으로 넘길 수 없습니다(JSON 은 여럿이 함께 쓰는 값이라 작업끼리 동시에 바꿀 수 있습니다)", n),
                            format!("`{}` is a JSON value and cannot be passed to `spawn` (JSON values are shared, so two tasks could change one at the same time)", n)
                        ),
                        l,
                        c,
                    )
                    .with_fix(tr!(
                        "`json.stringify(v)` 로 글자로 바꿔 넘기고, 작업 안에서 `json.parse` 로 되살리세요",
                        "pass `json.stringify(v)` as a Str and rebuild it inside the task with `json.parse`"
                    )),
                );
                continue;
            }
            if tainted || has_raw(&t, self, &mut HashSet::new()) {
                self.errors.push(
                    err(
                        "T0075",
                        tr!(
                            format!("`{}` 은(는) 원시 포인터나 아레나 값이라 `spawn` 으로 넘길 수 없습니다", n),
                            format!("`{}` is a raw pointer or arena value and cannot be passed to `spawn`", n)
                        ),
                        l,
                        c,
                    )
                    .with_fix(tr!(
                        "작업에는 복사되는 값(Int, Str, 리스트, 구조체 등)이나 통로(Chan)만 넘깁니다. 필요한 값을 복사해서 넘기세요",
                        "a task only receives copied values (Int, Str, lists, structs, ...) or channels; pass a copy of what it needs"
                    )),
                );
            }
        }
    }

    fn method_ret(&mut self, recv: &Ty, name: &str, args: &[Ty], l: usize, c: usize) -> Ty {
        // 사용자 정의 메서드
        let key = match recv {
            Ty::Struct(n) => Some(format!("{}.{}", n, name)),
            Ty::Enum(n) => Some(format!("{}.{}", n, name)),
            _ => None,
        };
        if let Some(k) = key {
            if let Some(sig) = self.methods.get(&k).cloned() {
                return sig.ret.clone();
            }
        }
        if *recv == Ty::Arena {
            self.errors.push(
                err("T0042", tr!(format!("아레나에 `{}` 메서드가 없습니다", name), format!("arena has no method `{}`", name)), l, c)
                    .with_fix(tr!("쓸 수 있는 것: `a.list[T]()`, `a.alloc[T](개수)`", "available: `a.list[T]()`, `a.alloc[T](count)`")),
            );
            return Ty::Unknown;
        }
        match (recv, name) {
            (Ty::List(_), "len") => Ty::Int,
            (Ty::List(t), "push") => {
                if let Some(a) = args.first() {
                    if !self.compatible(t, a) {
                        self.errors.push(
                            err("T0035", tr!(format!("[{}] 에 {} 값을 넣을 수 없습니다", t, a), format!("cannot push {} onto [{}]", a, t)), l, c)
                                .with_fix(tr!("리스트의 원소는 모두 같은 타입이어야 합니다", "all list elements must have the same type")),
                        );
                    }
                }
                Ty::Unit
            }
            (Ty::List(t), "sort") => {
                let fix = tr!(
                    "기준을 정하려면 `xs.sort_by(fn(x): 기준)` 을 쓰세요. 큰 것부터는 `sort_by(fn(x): -기준)` 또는 `sort()` 뒤 `reverse()`",
                    "to sort by a key, use `xs.sort_by(fn(x): key)`; for descending order use `sort_by(fn(x): -key)` or `sort()` then `reverse()`"
                );
                if !args.is_empty() {
                    self.errors.push(err("T0063", tr!("`sort()`는 인자를 받지 않습니다", "`sort()` takes no arguments"), l, c).with_fix(fix));
                } else if !matches!(**t, Ty::Int | Ty::Float | Ty::Str | Ty::Bool | Ty::Unknown | Ty::Var(_)) {
                    self.errors.push(
                        err(
                            "T0063",
                            tr!(
                                format!("{} 리스트는 `sort()`로 정렬할 수 없습니다 (Int, Float, Str, Bool 만 됩니다)", t),
                                format!("a list of {} cannot be sorted with `sort()` (only Int, Float, Str and Bool)", t)
                            ),
                            l,
                            c,
                        )
                            .with_fix(fix),
                    );
                }
                Ty::Unit
            }
            (Ty::List(t), "map" | "filter" | "any" | "all" | "sort_by") => {
                let f = args.first().cloned().unwrap_or(Ty::Unknown);
                let (ok, r) = match &f {
                    Ty::Fn(ps, r) if ps.len() == 1 && self.compatible(&ps[0], t) => (true, (**r).clone()),
                    Ty::Unknown => (true, Ty::Unknown),
                    _ => (false, Ty::Unknown),
                };
                if !ok || args.len() != 1 {
                    self.errors.push(
                        err(
                            "T0056",
                            tr!(
                                format!("`{}`에는 원소 하나({})를 받는 함수 하나를 넘깁니다", name, t),
                                format!("`{}` takes one function that accepts one element ({})", name, t)
                            ),
                            l,
                            c,
                        )
                        .with_fix(tr!(format!("`xs.{}(fn(x): ...)` 처럼 씁니다", name), format!("write it like `xs.{}(fn(x): ...)`", name))),
                    );
                    return Ty::Unknown;
                }
                let want_bool = matches!(name, "filter" | "any" | "all");
                if want_bool && !self.compatible(&Ty::Bool, &r) {
                    self.errors.push(
                        err(
                            "T0057",
                            tr!(
                                format!("`{}`에 넘긴 함수는 Bool 을 돌려줘야 하는데 {}을(를) 돌려줍니다", name, r),
                                format!("the function passed to `{}` must return Bool, but returns {}", name, r)
                            ),
                            l,
                            c,
                        )
                        .with_fix(tr!("`fn(x): x > 0` 처럼 참/거짓을 돌려주게 하세요", "return a Bool, e.g. `fn(x): x > 0`")),
                    );
                }
                if name == "sort_by" && !matches!(r, Ty::Int | Ty::Float | Ty::Str | Ty::Unknown) {
                    self.errors.push(
                        err(
                            "T0058",
                            tr!(
                                format!("`sort_by`의 기준은 Int, Float, Str 이어야 하는데 {}입니다", r),
                                format!("`sort_by` key must be Int, Float or Str, found {}", r)
                            ),
                            l,
                            c,
                        )
                        .with_fix(tr!(
                            "`xs.sort_by(fn(p): p.age)` 처럼 비교할 값 하나를 돌려주게 하세요",
                            "return a single value to compare, e.g. `xs.sort_by(fn(p): p.age)`"
                        )),
                    );
                }
                match name {
                    "map" => Ty::List(Box::new(r)),
                    "filter" => Ty::List(t.clone()),
                    "any" | "all" => Ty::Bool,
                    _ => Ty::Unit,
                }
            }
            (Ty::List(_), "index_of") => Ty::Int,
            (Ty::List(t), "slice") => Ty::List(t.clone()),
            (Ty::List(_), "clear") => Ty::Unit,
            (Ty::Str, "find") => Ty::Int,
            (Ty::Str, "width") => Ty::Int,
            (Ty::Str, "pad_left") | (Ty::Str, "pad_right") => Ty::Str,
            (Ty::Str, "repeat") | (Ty::Str, "slice") => Ty::Str,
            (Ty::List(t), "pop") => Ty::Optional(t.clone()),
            (Ty::List(_), "reverse") => Ty::Unit,
            (Ty::List(_), "contains") => Ty::Bool,
            (Ty::List(_), "join") => Ty::Str,
            (Ty::Str, "len") => Ty::Int,
            (Ty::Str, "split") => Ty::List(Box::new(Ty::Str)),
            (Ty::Str, "upper") | (Ty::Str, "lower") | (Ty::Str, "strip") | (Ty::Str, "replace") => Ty::Str,
            (Ty::Str, "contains") | (Ty::Str, "starts_with") | (Ty::Str, "ends_with") => Ty::Bool,
            (Ty::Json, "kind") => Ty::Str,
            (Ty::Json, "as_int") => Ty::Optional(Box::new(Ty::Int)),
            (Ty::Json, "as_float") => Ty::Optional(Box::new(Ty::Float)),
            (Ty::Json, "as_str") => Ty::Optional(Box::new(Ty::Str)),
            (Ty::Json, "as_bool") => Ty::Optional(Box::new(Ty::Bool)),
            (Ty::Json, "get") | (Ty::Json, "at") => Ty::Optional(Box::new(Ty::Json)),
            (Ty::Json, "len") => Ty::Int,
            (Ty::Json, "keys") => Ty::List(Box::new(Ty::Str)),
            (Ty::Json, "set") | (Ty::Json, "push") => Ty::Unit,
            (Ty::Dict(_, _), "len") => Ty::Int,
            (Ty::Dict(_, _), "set") => Ty::Unit,
            (Ty::Dict(_, _), "has" | "contains") => Ty::Bool,
            (Ty::Dict(k, _), "keys") => Ty::List(k.clone()),
            (Ty::Dict(k, v), "get") => {
                if args.len() != 2 {
                    self.errors.push(
                        err("T0047", tr!("`get(키, 기본값)`은 인자 두 개를 받습니다", "`get(key, default)` takes two arguments"), l, c)
                            .with_fix(tr!("키가 없을 때 돌려줄 기본값을 함께 주세요", "also pass a default to return when the key is missing")),
                    );
                } else {
                    if !self.compatible(k, &args[0]) {
                        self.errors.push(err(
                            "T0047",
                            tr!(
                                format!("이 사전의 키는 {}인데 {}을(를) 주었습니다", k, args[0]),
                                format!("this dict has {} keys, but {} was given", k, args[0])
                            ),
                            l,
                            c,
                        ));
                    }
                    if !self.compatible(v, &args[1]) {
                        self.errors.push(err(
                            "T0047",
                            tr!(format!("기본값은 {}여야 하는데 {}을(를) 주었습니다", v, args[1]), format!("default must be {}, found {}", v, args[1])),
                            l,
                            c,
                        ));
                    }
                }
                v.as_ref().clone()
            }
            (Ty::Task(t), "wait") => {
                self.no_args(name, args, l, c);
                (**t).clone()
            }
            (Ty::Task(_), "done") => {
                self.no_args(name, args, l, c);
                Ty::Bool
            }
            (Ty::Chan(t), "send") => {
                if args.len() != 1 {
                    self.errors.push(err(
                        "T0047",
                        tr!("`send` 는 보낼 값 하나를 받습니다", "`send` takes exactly one value to send"),
                        l,
                        c,
                    ));
                } else if !self.compatible(t, &args[0]) {
                    self.errors.push(
                        err(
                            "T0035",
                            tr!(format!("Chan[{}] 에 {} 값을 보낼 수 없습니다", t, args[0]), format!("cannot send {} on Chan[{}]", args[0], t)),
                            l,
                            c,
                        )
                        .with_fix(tr!("통로에는 만들 때 정한 타입의 값만 보냅니다", "a channel only carries values of the type it was created with")),
                    );
                }
                Ty::Unit
            }
            (Ty::Chan(t), "recv") => {
                self.no_args(name, args, l, c);
                Ty::Optional(t.clone())
            }
            (Ty::Chan(_), "close") => {
                self.no_args(name, args, l, c);
                Ty::Unit
            }
            (Ty::Unknown, _) => Ty::Unknown,
            (other, _) => {
                self.errors.push(
                    err("T0036", tr!(format!("{}에 `{}` 메서드가 없습니다", other, name), format!("{} has no method `{}`", other, name)), l, c)
                        .with_fix(method_hint(other, name)),
                );
                Ty::Unknown
            }
        }
    }

    /// 프렐류드가 아닌 표준 라이브러리 함수는 반드시 import 해야 합니다.
    /// 설계 문서 §4.7의 "와일드카드 import 없음" 원칙을 검사 단계에서 지킵니다.
    fn check_builtin_import(&mut self, name: &str, l: usize, c: usize) {
        const MODULE_OF: &[(&str, &str)] = &[
            ("sqrt", "math"), ("floor", "math"), ("ceil", "math"), ("pow", "math"),
            ("sin", "math"), ("cos", "math"), ("tan", "math"), ("log", "math"),
            ("log10", "math"), ("exp", "math"), ("round", "math"), ("pi", "math"), ("e", "math"),
            ("read_text", "fs"), ("write_text", "fs"), ("append_text", "fs"),
            ("exists", "fs"), ("remove", "fs"),
            ("now", "time"), ("clock", "time"), ("sleep", "time"),
            ("list_dir", "fs"), ("make_dir", "fs"), ("is_dir", "fs"),
            ("env", "process"), ("set_env", "process"), ("cwd", "process"), ("set_cwd", "process"),
            ("pid", "process"),
            ("seed", "random"), ("rand", "random"), ("rand_int", "random"),
            ("test", "re"), ("find_all", "re"), ("groups", "re"), ("split_re", "re"),
            ("parse", "json"), ("stringify", "json"), ("jnull", "json"), ("jbool", "json"),
            ("jint", "json"), ("jfloat", "json"), ("jstr", "json"), ("jlist", "json"),
            ("jdict", "json"),
        ];
        if let Some((_, m)) = MODULE_OF.iter().find(|(n, _)| *n == name) {
            if !self.imported.contains(name) && !self.imported.contains(&format!("@{}", m)) {
                self.errors.push(
                    err(
                        "T0041",
                        tr!(format!("`{}`은(는) `std.{}`에서 가져와야 합니다", name, m), format!("`{}` must be imported from `std.{}`", name, m)),
                        l,
                        c,
                    )
                    .with_fix(tr!(
                        format!("파일 위에 `from std.{} import {}` 를 추가하세요", m, name),
                        format!("add `from std.{} import {}` at the top of the file", m, name)
                    )),
                );
            }
        }
    }

    /// 표준 모듈에 들어 있는 이름들 (내장 함수 + Siskin 으로 쓴 조각의 공개 선언).
    pub fn std_members(module: &str) -> Option<Vec<String>> {
        let mut out: Vec<String> = crate::interp::MODULES
            .iter()
            .find(|(m, _)| *m == module)
            .map(|(_, fs)| fs.iter().map(|s| s.to_string()).collect())?;
        if let Some((_, src)) = crate::STD_SOURCES.iter().find(|(m, _)| *m == module) {
            if let Ok(p) = crate::parser::parse(src) {
                for st in &p.stmts {
                    let n = match st {
                        Stmt::Fn(f) => f.name.clone(),
                        Stmt::Struct(sd) => sd.name.clone(),
                        Stmt::Enum(ed) => ed.name.clone(),
                        _ => continue,
                    };
                    if !n.starts_with('_') && !out.contains(&n) {
                        out.push(n);
                    }
                }
            }
        }
        Some(out)
    }

    /// `from std.x import a, b` 가 실제로 있는 이름인지 검사 단계에서 봅니다
    /// (예전에는 `siskin run` 을 해야 알 수 있었습니다).
    fn check_std_import(&mut self, path: &[String], names: &[String], l: usize, c: usize) {
        if path.len() != 2 || path[0] != "std" {
            return;
        }
        let m = path[1].as_str();
        if m == "prelude" {
            return;
        }
        let Some(avail) = Self::std_members(m) else {
            self.errors.push(
                err("T0062", tr!(format!("모듈 `std.{}`는 없습니다", m), format!("no module `std.{}`", m)), l, c).with_fix(tr!(
                    "쓸 수 있는 모듈: std.math, std.fs, std.io, std.time, std.random, std.re, std.json, std.process, std.net",
                    "available modules: std.math, std.fs, std.io, std.time, std.random, std.re, std.json, std.process, std.net"
                )),
            );
            return;
        };
        for n in names {
            if avail.iter().any(|a| a == n) {
                continue;
            }
            let fix = if n == "args" || n == "argv" {
                tr!(
                    "명령줄 인자는 import 없이 `args()` 로 받습니다 (프로그램 이름은 빠진 [Str])",
                    "command-line arguments come from `args()`, no import needed (a [Str] without the program name)"
                )
                .to_string()
            } else if n == "exit" || n == "input" {
                tr!(format!("`{}` 는 import 없이 바로 씁니다", n), format!("`{}` is available without an import", n))
            } else {
                match best_match(&avail, n) {
                    Some(a) => tr!(
                        format!("`{}` 를 찾으셨나요? 이 모듈에 있는 것: {}", a, avail.join(", ")),
                        format!("did you mean `{}`? this module has: {}", a, avail.join(", "))
                    ),
                    None => tr!(format!("이 모듈에 있는 것: {}", avail.join(", ")), format!("this module has: {}", avail.join(", "))),
                }
            };
            self.errors.push(
                err("T0062", tr!(format!("`std.{}`에 `{}`이(가) 없습니다", m, n), format!("`std.{}` has no `{}`", m, n)), l, c).with_fix(fix),
            );
        }
    }

    pub fn is_builtin_name(n: &str) -> bool {
        SISKIN_BUILTINS.contains(&n)
    }

    fn builtin_ret(&mut self, name: &str, args: &[Ty], l: usize, c: usize) -> Ty {
        self.check_builtin_import(name, l, c);
        // 인자 개수. 예전에는 `round(x, 1)` 의 둘째 인자를 말없이 버렸습니다.
        const ARITY: &[(&str, usize, usize)] = &[
            ("sqrt", 1, 1), ("floor", 1, 1), ("ceil", 1, 1), ("pow", 2, 2), ("sin", 1, 1), ("cos", 1, 1),
            ("tan", 1, 1), ("log", 1, 1), ("log10", 1, 1), ("exp", 1, 1), ("round", 1, 1), ("pi", 0, 0),
            ("e", 0, 0), ("read_text", 1, 1), ("write_text", 2, 2), ("append_text", 2, 2), ("exists", 1, 1),
            ("remove", 1, 1), ("list_dir", 1, 1), ("make_dir", 1, 1), ("is_dir", 1, 1), ("now", 0, 0),
            ("clock", 0, 0), ("sleep", 1, 1), ("env", 1, 1), ("set_env", 2, 2), ("cwd", 0, 0),
            ("set_cwd", 1, 1), ("pid", 0, 0), ("seed", 1, 1), ("rand", 0, 0), ("rand_int", 2, 2),
            ("test", 2, 2), ("find_all", 2, 2), ("groups", 2, 2), ("split_re", 2, 2), ("stringify", 1, 1),
            ("abs", 1, 1), ("min", 2, 2), ("max", 2, 2), ("len", 1, 1), ("str", 1, 1), ("int", 1, 1),
            ("float", 1, 1), ("args", 0, 0), ("exit", 0, 1), ("input", 0, 1),
            ("sum", 1, 1), ("range", 1, 2), ("error", 1, 1),
        ];
        if let Some((_, lo, hi)) = ARITY.iter().find(|(n, _, _)| *n == name) {
            if args.len() < *lo || args.len() > *hi {
                let want = if lo == hi { format!("{}개", lo) } else { format!("{}~{}개", lo, hi) };
                let word = if *hi == 1 { "argument" } else { "arguments" };
                let want_en = if lo == hi { format!("{} {}", lo, word) } else { format!("{} to {} {}", lo, hi, word) };
                let fix = match name {
                    "round" => tr!(
                        "소수 몇째 자리까지 보이려면 f-문자열 서식을 쓰세요: `f\"{x:.1f}\"`",
                        "to show a fixed number of decimals, use f-string formatting: `f\"{x:.1f}\"`"
                    )
                    .to_string(),
                    "min" | "max" => tr!(
                        format!("리스트에서 고르려면 `xs.sort()` 뒤 첫/끝 원소를 쓰세요. 두 값씩: `{}(a, b)`", name),
                        format!("to pick from a list, `xs.sort()` it and take the first/last element; `{}(a, b)` compares two values", name)
                    ),
                    _ => tr!(format!("`{}` 은(는) 인자를 {} 받습니다", name, want), format!("`{}` takes {}", name, want_en)),
                };
                self.errors.push(
                    err(
                        "T0064",
                        tr!(
                            format!("`{}`에 인자를 {}개 주었는데 {} 받습니다", name, args.len(), want),
                            format!(
                                "`{}` takes {}, but {} {} given",
                                name,
                                want_en,
                                args.len(),
                                if args.len() == 1 { "was" } else { "were" }
                            )
                        ),
                        l,
                        c,
                    )
                    .with_fix(fix),
                );
            }
        }
        if name == "error" {
            if let Some(t) = args.first() {
                if !matches!(t, Ty::Str | Ty::Unknown | Ty::Enum(_)) {
                    self.errors.push(
                        err(
                            "T0065",
                            tr!(
                                format!("`error()`에는 글자(Str)나 enum 값을 넣습니다. {}을(를) 주었습니다", t),
                                format!("`error()` takes a Str or an enum value, found {}", t)
                            ),
                            l,
                            c,
                        )
                        .with_fix(tr!(
                            "오류 종류를 나누려면 `enum BankError:` 를 만들고 함수를 `-> BankError!Int` 로 선언하세요",
                            "to distinguish error kinds, define `enum BankError:` and declare the function `-> BankError!Int`"
                        )),
                    );
                } else if let Ty::Fallible(_, want) = self.cur_ret.clone() {
                    if !matches!(t, Ty::Unknown) && !self.compatible(&want, t) {
                        let fix = if matches!(t, Ty::Enum(_)) && *want == Ty::Str {
                            tr!(
                                format!("enum 오류를 내려면 반환 타입을 `{}!...` 로 선언하세요 (지금은 `!...` = 글자 오류)", t),
                                format!("to fail with an enum error, declare the return type as `{}!...` (plain `!...` means a Str error)", t)
                            )
                        } else if *t == Ty::Str {
                            tr!(
                                format!("이 함수의 오류 타입은 {} 입니다. `error({}의 변형)` 으로 쓰세요", want, want),
                                format!("this function's error type is {}; pass one of its variants: `error(Variant)`", want)
                            )
                        } else {
                            tr!(format!("이 함수의 오류 타입은 {} 입니다", want), format!("this function's error type is {}", want))
                        };
                        self.errors.push(
                            err(
                                "T0073",
                                tr!(
                                    format!("이 함수는 {} 오류를 내는데 {} 값을 넣었습니다", want, t),
                                    format!("this function fails with {} errors, but `error()` was given {}", want, t)
                                ),
                                l,
                                c,
                            )
                            .with_fix(fix),
                        );
                    }
                }
            }
        }
        match name {
            "print" | "eprint" => Ty::Unit,
            "len" => Ty::Int,
            "range" => Ty::List(Box::new(Ty::Int)),
            "str" => Ty::Str,
            "input" => Ty::Optional(Box::new(Ty::Str)),
            "args" => Ty::List(Box::new(Ty::Str)),
            "exit" => Ty::Unit,
            "int" => match args.first() {
                Some(Ty::Str) => Ty::fallible(Ty::Int),
                _ => Ty::Int,
            },
            "float" => match args.first() {
                Some(Ty::Str) => Ty::fallible(Ty::Float),
                _ => Ty::Float,
            },
            "error" => match args.first() {
                Some(t @ Ty::Enum(_)) => Ty::Fallible(Box::new(Ty::Unknown), Box::new(t.clone())),
                _ => Ty::Fallible(Box::new(Ty::Unknown), Box::new(Ty::Unknown)),
            },
            "assert" => Ty::Unit,
            "abs" => args.first().cloned().unwrap_or(Ty::Unknown),
            "min" | "max" => args.first().cloned().unwrap_or(Ty::Unknown),
            "sqrt" => {
                if args.first() == Some(&Ty::Int) {
                    self.errors.push(
                        err("T0037", tr!("sqrt()는 Float를 받습니다", "sqrt() takes a Float"), l, c).with_fix(tr!(
                            "`sqrt(float(n))` 으로 쓰세요. Siskin에는 암묵적 형변환이 없습니다",
                            "write `sqrt(float(n))`; Siskin has no implicit conversions"
                        )),
                    );
                }
                Ty::Float
            }
            "sin" | "cos" | "tan" | "log" | "log10" | "exp" => {
                if args.first() == Some(&Ty::Int) {
                    self.errors.push(
                        err("T0037", tr!(format!("{}()는 Float를 받습니다", name), format!("{}() takes a Float", name)), l, c).with_fix(tr!(
                            format!("`{}(float(n))` 으로 쓰세요. Siskin에는 암묵적 형변환이 없습니다", name),
                            format!("write `{}(float(n))`; Siskin has no implicit conversions", name)
                        )),
                    );
                }
                Ty::Float
            }
            "pi" | "e" => Ty::Float,
            "now" | "clock" | "rand" => Ty::Float,
            "seed" => Ty::Unit,
            "rand_int" => Ty::Int,
            "exists" => Ty::Bool,
            "append_text" | "remove" => Ty::fallible(Ty::Unit),
            "sum" => match args.first() {
                Some(Ty::List(t)) => (**t).clone(),
                _ => {
                    self.errors.push(
                        err("T0039", tr!("sum()은 숫자 리스트를 받습니다", "sum() takes a list of numbers"), l, c)
                            .with_fix(tr!("`sum(xs)` 처럼 리스트를 넘기세요", "pass a list, e.g. `sum(xs)`")),
                    );
                    Ty::Int
                }
            },
            // std.json
            "parse" => Ty::fallible(Ty::Json),
            "stringify" => Ty::Str,
            "jnull" | "jlist" | "jdict" => Ty::Json,
            "jbool" | "jint" | "jfloat" | "jstr" => Ty::Json,
            // std.re
            "test" => Ty::Bool,
            "find" if args.len() >= 2 => Ty::Optional(Box::new(Ty::Str)),
            "find_all" | "groups" | "split_re" => Ty::List(Box::new(Ty::Str)),
            "replace" if args.len() >= 3 => Ty::Str,
            "round" => Ty::Int,
            "floor" | "ceil" => Ty::Int,
            "pow" => args.first().cloned().unwrap_or(Ty::Unknown),
            // std.fs
            "read_text" => Ty::fallible(Ty::Str),
            "write_text" => Ty::fallible(Ty::Unit),
            "list_dir" => Ty::fallible(Ty::List(Box::new(Ty::Str))),
            "make_dir" | "set_cwd" => Ty::fallible(Ty::Unit),
            "is_dir" => Ty::Bool,
            // std.time, std.process
            "sleep" => {
                if args.first() == Some(&Ty::Int) {
                    self.errors.push(
                        err("T0037", tr!("sleep()는 Float(초)를 받습니다", "sleep() takes a Float (seconds)"), l, c)
                            .with_fix(tr!("`sleep(1.0)` 이나 `sleep(0.5)` 처럼 소수점을 붙여 쓰세요", "include a decimal point, e.g. `sleep(1.0)` or `sleep(0.5)`")),
                    );
                }
                Ty::Unit
            }
            "env" => Ty::Optional(Box::new(Ty::Str)),
            "set_env" => Ty::Unit,
            "cwd" => Ty::Str,
            "pid" => Ty::Int,
            // 표준 라이브러리의 Siskin 조각이 쓰는 내장 함수들
            "__time_parts" => Ty::List(Box::new(Ty::Int)),
            "__time_make" => Ty::Float,
            "__run" => Ty::Int,
            "__run_out" | "__run_err" => Ty::Str,
            "__ko" => Ty::Bool,
            "__net_open" | "__net_send" | "__net_listen" | "__net_listen_tls" | "__net_byte_len" | "__net_accept" | "__net_port" | "__http" => Ty::Int,
            "__net_recv" | "__net_recv_n" | "__net_url_decode" | "__net_recv_line" | "__net_peer" | "__net_error" | "__http_body" | "__url_encode" => Ty::Str,
            "__net_close" | "__net_timeout" => Ty::Unit,
            "__http_headers" => Ty::List(Box::new(Ty::Str)),
            other => {
                // 헤더에는 있는데 아직 자동으로 못 가져온 함수면, 왜 못 쓰는지 알려 줍니다.
                if let Some((header, why)) = self.c_skipped.get(other).cloned() {
                    self.errors.push(
                        err(
                            "T0048",
                            tr!(
                                format!("`{}`은(는) `{}` 에 있지만 아직 가져오지 못했습니다: {}", other, header, why),
                                format!("`{}` is declared in `{}` but could not be imported yet: {}", other, header, why)
                            ),
                            l,
                            c,
                        )
                        .with_fix(tr!(
                            "`siskin ffi <헤더>` 로 무엇이 빠졌는지 볼 수 있습니다. \
                             급하면 C 쪽에 이 함수를 감싼 함수를 하나 만들어 그걸 가져오세요",
                            "run `siskin ffi <header>` to see what is missing; as a workaround, write a C wrapper \
                             function around it and import that instead"
                        )),
                    );
                    return Ty::Unknown;
                }
                let fix = match other {
                    "Some" => tr!(
                        "`?T` 에는 Some 이 없습니다. 값을 그냥 돌려주면 됩니다 (`return x`), 없음은 `none`",
                        "optional `?T` has no Some; just return the value (`return x`), or `none` for no value"
                    )
                    .to_string(),
                    "Ok" => tr!(
                        "`!T` 에는 Ok 가 없습니다. 성공 값은 그냥 돌려줍니다 (`return x`)",
                        "fallible `!T` has no Ok; just return the success value (`return x`)"
                    )
                    .to_string(),
                    "Err" | "Error" | "raise" | "throw" | "panic" => {
                        tr!("실패는 `return error(\"이유\")` 로 알립니다", "report failure with `return error(\"reason\")`").to_string()
                    }
                    "sorted" => tr!(
                        "정렬은 `var` 리스트에서 `xs.sort()` 또는 `xs.sort_by(fn(x): 기준)` (제자리에서 바뀝니다)",
                        "sort a `var` list in place with `xs.sort()` or `xs.sort_by(fn(x): key)`"
                    )
                    .to_string(),
                    "Float" | "Str" | "Int" | "Bool" | "String" | "double" | "string" | "to_string" | "parseInt" | "parseFloat" => {
                        tr!(
                            "변환은 소문자 함수입니다: `float(x)`, `str(x)`, `int(s)` (`int`·`float` 로 글자를 읽으면 `!Int`·`!Float`)",
                            "conversions are lowercase functions: `float(x)`, `str(x)`, `int(s)` (parsing a string with `int`/`float` gives `!Int`/`!Float`)"
                        )
                        .to_string()
                    }
                    "println" | "printf" | "puts" | "echo" | "console" => tr!(
                        "출력은 `print(...)` 하나입니다. 줄바꿈은 `\\n` 을 직접 씁니다",
                        "output is just `print(...)`; write `\\n` yourself for a newline"
                    )
                    .to_string(),
                    "enumerate" => tr!(
                        "`for i in range(len(xs)):` 로 번호와 `xs[i]` 를 함께 씁니다",
                        "use `for i in range(len(xs)):` to get both the index and `xs[i]`"
                    )
                    .to_string(),
                    "zip" => tr!(
                        "`for i in range(len(a)):` 로 `a[i]`, `b[i]` 를 함께 씁니다",
                        "use `for i in range(len(a)):` to get both `a[i]` and `b[i]`"
                    )
                    .to_string(),
                    "isinstance" | "type" | "typeof" => tr!(
                        "타입은 컴파일할 때 정해집니다. enum 이면 `match` 로 나눕니다",
                        "types are fixed at compile time; to branch on an enum, use `match`"
                    )
                    .to_string(),
                    "open" | "fopen" => tr!(
                        "파일은 `from std.fs import read_text, write_text` 로 읽고 씁니다",
                        "read and write files with `from std.fs import read_text, write_text`"
                    )
                    .to_string(),
                    "argv" | "sys_argv" => tr!("명령줄 인자는 `args()` 입니다", "command-line arguments come from `args()`").to_string(),
                    "ord" | "chr" => tr!(
                        "글자 코드 함수는 아직 없습니다. 글자끼리는 `c >= \"a\" and c <= \"z\"` 처럼 비교합니다",
                        "there are no character-code functions yet; compare strings directly, e.g. `c >= \"a\" and c <= \"z\"`"
                    )
                    .to_string(),
                    _ => {
                        let mut cands: Vec<String> = self.fns.keys().cloned().collect();
                        cands.extend(SISKIN_BUILTINS.iter().map(|s| s.to_string()));
                        match best_match(&cands, other) {
                            Some(b) => tr!(
                                format!("`{}` 를 찾으셨나요? (표준 모듈 함수면 `from std.… import` 도 필요합니다)", b),
                                format!("did you mean `{}`? (standard module functions also need `from std.… import`)", b)
                            ),
                            None => tr!(
                                "이름의 철자를 확인하거나, 필요한 모듈을 import 했는지 보세요",
                                "check the spelling, or that the module it comes from is imported"
                            )
                            .to_string(),
                        }
                    }
                };
                self.errors.push(
                    err("T0038", tr!(format!("`{}`(이)라는 함수를 찾을 수 없습니다", other), format!("cannot find function `{}`", other)), l, c)
                        .with_fix(fix),
                );
                Ty::Unknown
            }
        }
    }
}
/// 후보 가운데 가장 비슷한 이름 (없으면 None).
pub fn best_match(cands: &[String], n: &str) -> Option<String> {
    cands
        .iter()
        .filter(|c| similar(c, n))
        .min_by_key(|c| edit_distance(&norm(c), &norm(n)))
        .cloned()
}

fn norm(s: &str) -> String {
    s.to_lowercase().replace('_', "")
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (x, y): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=y.len()).collect();
    for i in 1..=x.len() {
        let mut cur = vec![i; y.len() + 1];
        for j in 1..=y.len() {
            let cost = if x[i - 1] == y[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[y.len()]
}

/// 오타 제안용: 대소문자·밑줄 무시 편집 거리가 짧거나 한쪽이 다른 쪽을 품으면 비슷하다고 봅니다.
pub fn similar(a: &str, b: &str) -> bool {
    let a = norm(a);
    let b = norm(b);
    if a == b {
        return true;
    }
    if a.len() >= 3 && b.len() >= 3 && (a.contains(&b) || b.contains(&a)) {
        return true;
    }
    let (x, y): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=y.len()).collect();
    for i in 1..=x.len() {
        let mut cur = vec![i; y.len() + 1];
        for j in 1..=y.len() {
            let cost = if x[i - 1] == y[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    let d = prev[y.len()];
    d <= 1 || (d <= 2 && x.len().max(y.len()) >= 5)
}

/// 없는 메서드를 불렀을 때: 다른 언어 이름이면 Siskin 이름을, 아니면 그 타입의 메서드 목록을 알려 줍니다.
fn method_hint(t: &Ty, name: &str) -> String {
    let list: &[&str] = match t {
        Ty::List(_) => &[
            "len", "push", "pop", "sort", "sort_by", "reverse", "contains", "index_of", "slice", "clear", "join",
            "map", "filter", "any", "all",
        ],
        Ty::Str => &[
            "len", "upper", "lower", "strip", "split", "starts_with", "ends_with", "contains", "replace", "find",
            "repeat", "slice", "width", "pad_left", "pad_right",
        ],
        Ty::Dict(_, _) => &["len", "get", "set", "has", "keys"],
        Ty::Task(_) => &["wait", "done"],
        Ty::Chan(_) => &["send", "recv", "close"],
        _ => &[],
    };
    let alias = match name {
        "join" | "get" | "result" | "await" if matches!(t, Ty::Task(_)) => Some("wait"),
        "put" | "push" | "write" if matches!(t, Ty::Chan(_)) => Some("send"),
        "get" | "take" | "receive" | "read" | "pop" if matches!(t, Ty::Chan(_)) => Some("recv"),
        "append" | "add" | "push_back" | "insert" => Some("push"),
        "trim" | "strip_whitespace" => Some("strip"),
        "toUpperCase" | "to_upper" | "to_uppercase" | "upcase" => Some("upper"),
        "toLowerCase" | "to_lower" | "to_lowercase" | "downcase" => Some("lower"),
        "length" | "size" | "count" => Some("len"),
        "startswith" | "startsWith" => Some("starts_with"),
        "endswith" | "endsWith" => Some("ends_with"),
        "indexOf" | "index" => Some(if matches!(t, Ty::Str) { "find" } else { "index_of" }),
        "includes" | "has" if matches!(t, Ty::List(_) | Ty::Str) => Some("contains"),
        "sorted" | "sort_by_key" | "sortBy" => Some("sort_by"),
        "forEach" | "for_each" | "each" => None,
        "values" | "items" if matches!(t, Ty::Dict(_, _)) => None,
        "to_int" | "parse" | "to_i" | "parseInt" => None,
        "format" => None,
        "subscript" | "substring" | "substr" => Some("slice"),
        _ => None,
    };
    if let Some(a) = alias {
        return tr!(format!("Siskin 에서는 `{}` 입니다", a), format!("in Siskin this is `{}`", a));
    }
    match name {
        "forEach" | "for_each" | "each" => return tr!("`for x in xs:` 로 돕니다", "iterate with `for x in xs:`").into(),
        "values" | "items" => {
            return tr!(
                "`for k, v in d:` 로 키와 값을 함께 돕니다. 키만은 `d.keys()`",
                "iterate over keys and values with `for k, v in d:`; keys only with `d.keys()`"
            )
            .into()
        }
        "to_int" | "parse" | "to_i" | "parseInt" => {
            return tr!(
                "글자를 수로: `int(s)` / `float(s)` (실패할 수 있어 `!Int`)",
                "parse a string into a number with `int(s)` / `float(s)` (fallible, `!Int`)"
            )
            .into()
        }
        "format" => return tr!("f-문자열을 씁니다: `f\"{x:.2f} {name:>10}\"`", "use an f-string: `f\"{x:.2f} {name:>10}\"`").into(),
        _ => {}
    }
    if list.is_empty() {
        return tr!(format!("{} 에는 메서드가 없습니다", t), format!("{} has no methods", t));
    }
    let cands: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    match best_match(&cands, name) {
        Some(b) => tr!(
            format!("`{}` 를 찾으셨나요? 쓸 수 있는 것: {}", b, list.join(", ")),
            format!("did you mean `{}`? available: {}", b, list.join(", "))
        ),
        None => tr!(format!("쓸 수 있는 것: {}", list.join(", ")), format!("available: {}", list.join(", "))),
    }
}

/// 블록이 어느 길로 가든 `return`(또는 `exit`)으로 끝나는가.
/// match 는 전수 검사(T0012/T0068)를 통과했다고 보고, 모든 갈래가 끝나면 끝난다고 봅니다.
fn always_returns(body: &[Stmt]) -> bool {
    body.iter().any(stmt_returns)
}

fn stmt_returns(s: &Stmt) -> bool {
    match s {
        Stmt::Return(..) => true,
        Stmt::Expr(Expr::Call { callee, .. }, None) => matches!(&**callee, Expr::Ident(n, ..) if n == "exit"),
        Stmt::If { arms, els: Some(e) } => arms.iter().all(|(_, b)| always_returns(b)) && always_returns(e),
        Stmt::Match { cases, .. } => !cases.is_empty() && cases.iter().all(|c| always_returns(&c.body)),
        Stmt::While { cond: Expr::Bool(true), body } => !has_break(body),
        Stmt::Arena { body, .. } | Stmt::Unsafe { body, .. } => always_returns(body),
        _ => false,
    }
}

/// 이 반복문 자신을 빠져나가는 `break` 가 있는가 (안쪽 반복문의 break 는 세지 않음).
fn has_break(body: &[Stmt]) -> bool {
    body.iter().any(|s| match s {
        Stmt::Break(..) => true,
        Stmt::If { arms, els } => arms.iter().any(|(_, b)| has_break(b)) || els.as_ref().map_or(false, |e| has_break(e)),
        Stmt::Match { cases, .. } => cases.iter().any(|c| has_break(&c.body)),
        Stmt::Arena { body, .. } | Stmt::Unsafe { body, .. } => has_break(body),
        Stmt::Let { catch: Some(c), .. } | Stmt::Assign { catch: Some(c), .. } | Stmt::Expr(_, Some(c)) => has_break(&c.body),
        _ => false,
    })
}
