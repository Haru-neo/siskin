//! P2 type checker.
//!
//! The P1 interpreter only parsed type annotations and ignored them. From here on
//! they are checked before running. This is what makes P3 native code generation possible.
//! (We need the types to decide whether something is a C `long long` or a `double`.)

use crate::ast::*;
use crate::error::SiskinError;
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Names Siskin already provides. Even if a C header has the same name,
/// Siskin's wins (many overlap, like `abs`, `exit`, `free`, `pow`).
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
    /// The type of the `none` literal itself
    NoneTy,
    Unit,
    List(Box<Ty>),
    Dict(Box<Ty>, Box<Ty>),
    Struct(String),
    Enum(String),
    Optional(Box<Ty>),
    /// A fallible value: (success type, error type). For `!T` the error type is Str.
    Fallible(Box<Ty>, Box<Ty>),
    /// `*T` — raw pointer (memory Level 2)
    Raw(Box<Ty>),
    /// Arena created by `with arena a:` (memory Level 1)
    Arena,
    /// A `std.json` value
    Json,
    /// `(T, U, ...)` — tuple
    Tuple(Vec<Ty>),
    /// `(A, B) -> R` — type of a function value
    Fn(Vec<Ty>, Box<Ty>),
    /// A type parameter such as the `T` in `fn f[T](...)`. Filled with a concrete type at the call.
    Var(String),
    /// Task handle returned by `spawn f(x)`. `t.wait()` yields f's result.
    Task(Box<Ty>),
    /// Channel created by `channel[T]()`. Tasks pass values through it (copied on send).
    Chan(Box<Ty>),
    /// Not yet determined (empty list, etc.). Matches anything.
    Unknown,
}

impl Ty {
    /// The ordinary `!T` whose error value is a string.
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
    /// Names of functions declared with `extern "C" fn ...`.
    pub externs: HashSet<String>,
    /// Library names given with `extern "C" link "..."`.
    pub links: Vec<String>,
    /// Functions present in a header that could not be imported automatically: name -> (header, reason)
    pub c_skipped: std::collections::HashMap<String, (String, String)>,
    /// Whether we are inside `unsafe:`. Raw pointer operations are allowed only here.
    unsafe_depth: usize,
    /// Whether we are inside `with arena:`.
    arena_depth: usize,
    /// Names of values that came from an arena. They cannot escape the block.
    tainted: HashSet<String>,
    /// For each open `with arena` block, the scope depth at which the block started.
    arena_bases: Vec<usize>,
    /// Mutable names (paired with scopes). `var`, `inout`/`owned` parameters
    /// and `inout self` go here. `let` and read-only parameters do not.
    mutables: Vec<HashSet<String>>,
    /// Type parameters of the function currently being checked (the T, U in `fn f[T, U]`).
    /// When `resolve` meets one of these names it treats it as `Ty::Var`.
    cur_generics: HashSet<String>,
    /// "Unknown type" names already reported. Not repeated at every place the name is used.
    reported_types: HashSet<String>,
    /// Concrete types for type parameters during native monomorphization.
    /// If empty (during type checking), `resolve` yields `Ty::Var`.
    pub mono_subst: HashMap<String, Ty>,
    /// The "expected type of the slot" used when an anonymous function omits parameter types.
    /// This is how we know `x` is Int in `map(xs, fn(x): x * 2)`.
    lambda_hint: Option<Ty>,
    /// The resolved (parameter types, return type) for each anonymous function, keyed by declaration address.
    /// Used by native code generation to emit anonymous functions whose parameter types were omitted.
    pub lambda_sigs: HashMap<usize, (Vec<Ty>, Ty)>,
    /// While checking a closure body, the scope depth at which that closure starts.
    /// Names from scopes outside it are captured values and are read-only.
    closure_bases: Vec<usize>,
    /// The function currently being checked (or generated). Used to see whether field narrowing
    /// after a guard clause is changed via `inout` in the rest of the function.
    pub cur_decl: Option<crate::ast::Shared<FnDecl>>,
    /// Paired with `closure_bases`: whether that closure is the body of a task created by `spawn`.
    spawn_bodies: Vec<bool>,
}

/// Whether this is a standard module that can be called through its module name, like `math.sqrt(x)`.
pub fn is_std_module(m: &str) -> bool {
    matches!(m, "math" | "fs" | "io" | "time" | "random" | "re" | "json" | "process" | "net")
}

fn err(code: &'static str, msg: impl Into<String>, line: usize, col: usize) -> SiskinError {
    SiskinError::new(code, msg, line, col)
}

/// Finds the root name of an assignment target. `x`, `x.f`, `x[i]`, `x.f[i].g` all give `x`.
fn root_ident(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n, _, _) => Some(n),
        Expr::Field(o, _, _, _) => root_ident(o),
        Expr::Index(o, _, _, _) => root_ident(o),
        _ => None,
    }
}

/// Turns an expression made only of a name and fields, like `t.due` or `a.b.c`, into the string `"t.due"`.
/// Used as the key for struct field narrowing (`if t.due != none:`). The narrowed type is stored in the
/// scope under this string; since it contains a dot, it never clashes with ordinary names.
pub fn field_path(e: &Expr) -> Option<String> {
    match e {
        Expr::Ident(n, _, _) => Some(n.clone()),
        Expr::Field(o, f, _, _) => field_path(o).map(|p| format!("{}.{}", p, f)),
        _ => None,
    }
}

/// The path actually changed by an assignment or `inout`. `t.xs[0] = ..` changes `t.xs`.
fn write_path(e: &Expr) -> Option<String> {
    match e {
        Expr::Ident(n, _, _) => Some(n.clone()),
        Expr::Field(o, f, _, _) => write_path(o).map(|p| format!("{}.{}", p, f)),
        Expr::Index(o, _, _, _) => write_path(o),
        _ => None,
    }
}

/// Hint for when an operation fails because one side is `?T`. Tells the user to check for none first.
fn optional_fix(a: &Ty, b: &Ty) -> Option<String> {
    let t = if matches!(a, Ty::Optional(_)) { a } else if matches!(b, Ty::Optional(_)) { b } else { return None };
    Some(tr!(
        format!("{} 값은 none 일 수 있습니다. 먼저 `if x != none:` 로 확인하거나(구조체 필드도 `if t.due != none:`), `x else 0` 처럼 기본값을 주세요. 확인한 뒤 `inout` 으로 넘기거나 다시 대입하면 확인이 풀립니다", t),
        format!("a {} value may be none; check it first with `if x != none:` (struct fields too: `if t.due != none:`), or give a default like `x else 0`. Passing it as `inout` or reassigning it after the check cancels the check", t)
    ))
}

/// Whether `w` is an outer path of `k` (`t` of `t.due`, `a.b` of `a.b.c`).
fn is_outer_path(w: &str, k: &str) -> bool {
    k.len() > w.len() && k.starts_with(w) && k.as_bytes()[w.len()] == b'.'
}

/// The range over which a narrowing must hold. If it may be changed via `inout` in here, no narrowing is done.
pub enum Region<'a> {
    Block(&'a [Stmt]),
    Expr(&'a Expr),
    /// After a guard clause (`if t.due == none: return`): everything after this line in the current function.
    After(usize),
}

/// Collects paths passed as `inout` within the statements, as (path, line).
/// Only looks at function/method names that have `inout` parameters (`inout`). Since it judges by name only,
/// it conservatively also counts other functions with the same name.
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

/// Collects assignments (`path = value`) within the statements. Used to decide whether to drop narrowing before a loop.
fn assign_writes(stmts: &[Stmt], out: &mut Vec<(String, Expr)>) {
    for s in stmts {
        match s {
            Stmt::Assign { target, value, catch, .. } => {
                if let Some(p) = write_path(target) {
                    // For `x = f() catch e:` the value type is hard to know here, so it is always treated as "may be absent".
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

/// Extracts the actual target expression from a call like `a.alloc[T](n)`.
fn value_arena_src(e: &Expr) -> Option<&Expr> {
    match e {
        Expr::Call { callee, .. } => Some(callee),
        _ => None,
    }
}

impl Types {
    /// Peeks at the type without reporting errors (for taint tracking).
    fn infer_peek(&self, e: &Expr) -> Option<Ty> {
        match e {
            Expr::Ident(n, _, _) => self.lookup(n),
            _ => None,
        }
    }

    /// Determines only the type and discards any errors produced along the way.
    /// The real errors are reported once, later, by the proper `infer`.
    /// Checks the catch block of `let x = f() catch e:`. The block must either exit
    /// (return / break / continue) or end with a value to put into x instead.
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

    /// Determines which binding's value an assignment target actually changes.
    /// Writes through a pointer (`p[i] = ...` where `p` is `*T`) only change the memory the pointer
    /// points to, not the pointer binding itself, so they yield None.
    /// Lists, dicts and structs, on the other hand, are values: changing an element changes the binding's value.
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

    /// Checks whether this can be passed as an `inout` argument. It must be a mutable lvalue.
    /// Literals and computed expressions (nothing to change) and `let` or read-only values cannot be passed.
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
        // Pass 1: struct and enum names must be registered first so type annotations can be resolved.
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
                    // Importing a whole module, like `import std.math`, means calls are written `math.sqrt(...)`.
                    if names.is_empty() && path.len() == 2 && path[0] == "std" {
                        self.imported.insert(format!("@{}", path[1]));
                    }
                }
                _ => {}
            }
        }
        // Pass 2: function signatures
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
            // Functions that are in a header but cannot be imported automatically yet. Remember the names so that
            // calling one reports why it cannot be used, rather than "unknown name".
            if let Stmt::CHeader { header, only, .. } = s {
                if only.len() == 2 {
                    self.c_skipped
                        .insert(only[0].clone(), (header.clone(), only[1].clone()));
                }
            }
        }
        // Pass 3: methods
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

    /// Turns a syntactic type annotation into an actual type.
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
                // Means there is no value to return. Used when only failure is signalled, as in `-> !Unit`.
                "Unit" => Ty::Unit,
                "Json" => Ty::Json,
                other => {
                    if self.cur_generics.contains(other) {
                        // During monomorphization, the concrete type; otherwise the type parameter.
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
            // `()` means no value to return (Unit). Written like `Task[()]`.
            TypeExpr::Tuple(ts) if ts.is_empty() => Ty::Unit,
            TypeExpr::Tuple(ts) => Ty::Tuple(ts.iter().map(|t| self.resolve(t, line)).collect()),
            TypeExpr::Fn(ps, r) => Ty::Fn(
                ps.iter().map(|t| self.resolve(t, line)).collect(),
                Box::new(self.resolve(r, line)),
            ),
        }
    }

    // --------------------------------------------------------------- scopes

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

    /// Local variables for the debugger to show: names and types visible from scope `base` on (outermost first).
    /// Internal names such as narrowed fields (`t.due`) are excluded.
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
            // Re-declaring a name in the same scope ends any field narrowing (`t.due`) tied to that name.
            let pre = format!("{}.", name);
            sc.retain(|k, _| !k.starts_with(&pre));
        }
        sc.insert(name.to_string(), t);
    }

    // ---------------------------------------------------- struct field narrowing

    /// Names of functions/methods that have an `inout` parameter (or `inout self`).
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

    /// The type of a narrowed field path (`"t.due"`). If the root name (`t`) was re-declared
    /// in an inner scope after narrowing, it is a different value, so treat it as absent.
    pub fn narrowed_field(&self, key: &str) -> Option<Ty> {
        let root = key.split('.').next()?;
        let ki = self.scopes.iter().rposition(|s| s.contains_key(key))?;
        match self.scopes.iter().rposition(|s| s.contains_key(root)) {
            Some(ri) if ri > ki => None,
            _ => self.scopes[ki].get(key).cloned(),
        }
    }

    /// If this expression is a narrowed field (`t.due`), its narrowed type.
    pub fn narrowed_expr(&self, e: &Expr) -> Option<Ty> {
        if !matches!(e, Expr::Field(..)) {
            return None;
        }
        let k = field_path(e)?;
        self.narrowed_field(&k)
    }

    /// Whether `key` (or an outer path of it) may be changed by being passed as `inout` within `region`.
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

    /// Before an assignment: storing a possibly-absent value (`none`, `?T`) into a narrowed field drops that narrowing.
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

    /// After an assignment: replacing an outer path wholesale (`t = other`) drops narrowings inside it (`t.due`).
    pub fn after_assign(&mut self, target: &Expr) {
        if let Some(w) = write_path(target) {
            self.forget_fields(&|k| is_outer_path(&w, k));
        }
    }

    /// Before a loop: if the body makes a narrowed field possibly absent again, the second iteration
    /// would see the wrong type, so narrowing is dropped before entering the loop.
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

    /// Declares a mutable name (`var`, `inout`/`owned` parameters).
    pub fn declare_mut(&mut self, name: &str, t: Ty) {
        self.declare(name, t);
        self.mutables.last_mut().unwrap().insert(name.to_string());
    }

    /// Whether this name is mutable. Looks from the innermost scope outward.
    /// If the same name is shadowed by a `let` in an inner scope, the shadowing takes precedence.
    fn is_mutable(&self, name: &str) -> bool {
        for (i, s) in self.scopes.iter().enumerate().rev() {
            if s.contains_key(name) {
                if self.closure_bases.last().map_or(false, |b| i < *b) {
                    return false;
                }
                return self.mutables[i].contains(name);
            }
        }
        // Names not in scopes (e.g. narrowed names) are not blocked.
        true
    }

    /// Whether this name is a value captured from outside by the closure currently being checked.
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

    /// Error for trying to modify a captured value.
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

    // ------------------------------------------------------------- type compatibility

    /// Whether `got` can be placed where `want` is expected.
    /// Whether both sides of `==` / `!=` are comparable types. It used to accept anything, so
    /// always-false comparisons like `input() == none` slipped through silently.
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

    /// Fills type parameters with concrete types in a generic call.
    /// E.g. if the argument type is `[Int]` and the parameter is `[T]`, records `T=Int`.
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

    /// Replaces parameters (`Ty::Var`) inside a type with the concrete types filled in.
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

    /// Start of native monomorphization: binds this function's type parameters to concrete types.
    /// After this, `resolve` resolves `T` to the concrete type.
    pub fn enter_mono(&mut self, generics: &[String], subst: HashMap<String, Ty>) {
        self.cur_generics = generics.iter().cloned().collect();
        self.mono_subst = subst;
    }

    /// End of monomorphization: clears the parameter bindings.
    pub fn exit_mono(&mut self) {
        self.cur_generics.clear();
        self.mono_subst.clear();
    }

    // --------------------------------------------------------------- checking

    pub fn check_program(&mut self, prog: &Program) {
        self.check_program_inner(prog);
        // The same error at the same position is shown only once (signatures were read twice, causing duplicates).
        let mut seen = HashSet::new();
        self.errors.retain(|e| seen.insert((e.code, e.msg.clone(), e.line, e.col)));
    }

    fn check_program_inner(&mut self, prog: &Program) {
        // Check struct/enum field types at their declaration first, so a bad type is
        // reported once at the declaring line rather than at every use.
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
        // A top-level `let` is a constant visible throughout the file. It must be seen before the functions
        // so it can be used regardless of position (even if a function is written first).
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
                    // Executable statements go only inside main (same rule as `siskin build`).
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
            // Read-only parameters (the default) and read-only self cannot be modified.
            // Only `inout`/`owned` parameters and `inout self` can be modified.
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
        // When a function that must return a value fell off the end, `run` used to silently return `none`
        // and `build` returned 0. This is now rejected at compile time.
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
        // ensures can see result.
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

    /// Inside `if x != none:`, treats x as its unwrapped type. Works for struct fields (`t.due`) too.
    /// A field is not narrowed if it may be changed via `inout` within `region`.
    /// The true side of `a and b` and the false side of `a or b` receive the narrowings of both conditions.
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
            // Unwrapped on the true side of `x != none` and the false side of `x == none`.
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

    /// Sets the slot type to use when the next `infer` meets an anonymous function (for code generation).
    pub fn lambda_hint_set(&mut self, h: Option<Ty>) {
        self.lambda_hint = h;
    }

    /// Infers an expression, and if it is an anonymous function, supplies the "expected type of the slot".
    /// This is where anonymous functions without parameter types get their types.
    fn infer_hinted(&mut self, e: &Expr, hint: Option<Ty>) -> Ty {
        if matches!(e, Expr::Lambda(..)) {
            self.lambda_hint = hint;
        }
        self.infer(e)
    }

    /// Whether the type is not yet fully determined (contains `_` or `T`).
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

    /// Inside a generic function, its type parameters (`T`) are used like determined types.
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

    /// A `fn` declared inside a function — a named closure that captures outer values.
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
        // The name must be known before the body so it can call itself (recursion).
        self.declare(&f.name, Ty::Fn(ps, Box::new(sig.ret.clone())));
        self.check_closure(f, None, 1);
    }

    /// Checks the body of a closure (anonymous or nested function) and yields its function type.
    /// Outer scopes remain visible but are read-only (captured values are copies).
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
                // `fn(x): expr` — the return type is the expression's type.
                self.cur_ret = Ty::Unknown;
                self.infer_hinted(e, hr.clone())
            }
            _ => {
                for r in &f.requires {
                    self.check_cond(r);
                }
                if let (Some(Stmt::Return(Some(e), l, c)), true) = (f.body.first(), f.is_lambda()) {
                    // Anonymous function with a declared return type: the expression must have that type.
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
                // Declaring the same name again in the same block used to be a runtime error (E0211) in run and a C error in build.
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
                    // With catch attached, the error side is handled, so only the success type remains.
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
                // Storing an arena value in a variable outside the block (even via a struct field) would leave it
                // pointing at freed memory after the block ends. Blocked with the same rule as `return`.
                if let (Some(base), Some(root)) = (self.arena_bases.last().copied(), self.mutated_binding(target)) {
                    let outer = self
                        .scopes
                        .iter()
                        .rposition(|sc| sc.contains_key(&root))
                        .map(|i| i < base)
                        .unwrap_or(false);
                    // Lists are copied when stored, so they are safe. Only pointers (or structs that may contain pointers) are blocked.
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
                // Value semantics: `let` and read-only parameters/self cannot be modified.
                // However, `p[i] = ...` through a pointer modifies the memory the pointer
                // points to, so it is fine even with `let p`.
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
                // Guard clause: if the only arm always exits, as in `if v == none: return`,
                // narrow in the opposite direction from there on. This flattens deep nesting.
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
                    // `for k, v in d:` — dicts only. k has the key type, v the value type.
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
                        // Iterates over the elements of a JSON list (does not iterate at all if it is not a list).
                        Ty::Json => Ty::Json,
                        // Receives until the channel is closed and drained.
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
                    // The loop variable is left mutable (a local copy per iteration).
                    self.declare_mut(var, elem);
                }
                for s in body {
                    self.check_stmt(s);
                }
                self.pop_scope();
            }

            Stmt::Match { subject, cases, line } => {
                let st = self.infer(subject);
                // Design doc §4.5: match is checked for exhaustiveness. In P1 this was caught at
                // runtime, but from here on it is caught at compile time.
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
                // Using a pattern like `case Some(v):` / `case UnknownAccount(id):` on a non-enum value
                // used to pass silently, producing only "cannot find `v`".
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
                // Values like Str and Int have unbounded cases, so `case _:` is required.
                // Without it, `run` used to fail at runtime and `build` passed silently.
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
                        // Keeps the bad pattern reported above from cascading into "cannot find `v`".
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
                // A wrong error type in `return error(x)` has already been reported as T0073 by error().
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

    /// Whether this expression touches a value that came from an arena.
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

    // ------------------------------------------------------------- expressions

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
                        // Using a payload-carrying variant without parentheses is an error.
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
                    // Using a top-level function name as a value gives a function type.
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
                    // If an earlier element's type is underdetermined, as in `[[], [1]]`, fill it from later elements.
                    if Self::is_open(&first) && !Self::is_open(&t) && self.compatible(&first, &t) {
                        first = t.clone();
                    }
                    // `[none, 5]` and `[5, none]` are `[?Int]`.
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
                // `a and b`: the right side is checked assuming the left side is true.
                // So in `v != none and v > 0`, v on the right side is narrowed.
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
                        // Pointer arithmetic: `p + 8`
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
                        // Different error types cannot be propagated as is. Only when raising an enum error from a Str-error
                        // function is it converted to a string (in the form `NoFunds(need: 5)`).
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
                // The left side must be `?T`. The result is the unwrapped T.
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
                // The default must match T (or ?T).
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

    /// Argument checking when calling a function value (a function passed as a value). Yields the return type.
    /// If the struct has no method of that name but has a function-typed field, that field's type.
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
        // Method call
        if let Expr::Field(obj, mname, l, c) = callee {
            // Module function
            if let Expr::Ident(m, _, _) = obj.as_ref() {
                if self.lookup(m).is_none() && is_std_module(m) {
                    // Standard functions written in Siskin, like `time.today()`, are called like ordinary functions.
                    if self.fns.contains_key(mname.as_str()) {
                        let callee = Expr::Ident(mname.clone(), *l, *c);
                        return self.infer_call(&callee, targs, args, line, col);
                    }
                    let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(&a.value)).collect();
                    return self.builtin_ret(mname, &arg_tys, *l, *c);
                }
            }
            let ot = self.infer(obj);
            // Calling a function-typed field: `self.on_click(x)`
            if let Some(Ty::Fn(ps, r)) = self.fn_field(&ot, mname) {
                return self.check_indirect_call(&ps, *r, args, *l, *c);
            }
            // `fs.push(fn(x): ...)` — the list element type gives the anonymous function's parameter type.
            let push_hint = match (&ot, mname.as_str()) {
                (Ty::List(inner), "push") => Some((**inner).clone()),
                // `xs.map(fn(x): ...)` — x has the element type.
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
            // Value semantics: in-place mutating methods cannot be used on read-only values.
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
            // `inout self` and `inout` method arguments must also be mutable variables.
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
            // Calling a function value (a function in a local variable or parameter) is an indirect call.
            if let Some(Ty::Fn(params, ret)) = self.lookup(name) {
                return self.check_indirect_call(&params, *ret, args, *l, *c);
            }
            // Struct construction
            if let Some(sd) = self.structs.get(name).cloned() {
                return self.check_ctor(&sd.name, &sd.fields, args, *l, *c, Ty::Struct(sd.name.clone()));
            }
            // Enum variant construction
            if let Some(ename) = self.variant_of.get(name).cloned() {
                let ed = self.enums.get(&ename).cloned().unwrap();
                let vd = ed.variants.iter().find(|v| &v.name == name).unwrap().clone();
                return self.check_ctor(&vd.name, &vd.fields, args, *l, *c, Ty::Enum(ename));
            }
            // User function
            if let Some(sig) = self.fns.get(name).cloned() {
                let want: Vec<(String, Ty)> =
                    sig.params.iter().filter(|(n, _)| n != "self").cloned().collect();
                // Anonymous function arguments are checked later: `T` must first be determined by the other
                // arguments before the type of `x` in `fn(x): ...` can be known.
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
                // For a generic function, first fill the type parameters (T, etc.) from the argument types.
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
                        // `inout` arguments must be mutable variables.
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
            // `channel[T]()` / `channel[T](size)` — a channel for passing values between tasks.
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
            // Memory Level 2 builtins
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
                    // Reads a NUL-terminated string from a handle given by C.
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
                    // Reads the i-th value from the slot the handle points to (8 bytes each).
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
            // Builtin functions
            let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(&a.value)).collect();
            return self.builtin_ret(name, &arg_tys, *l, *c);
        }

        // Directly calling an expression that returns a function value: `get_fn()(x)` etc.
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

    /// Checks values that `spawn` hands to a new task. Values are copied, so most are safe, but
    /// raw pointers and arena values are blocked because another task could free or modify that memory.
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
        // User-defined methods
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

    /// Standard library functions outside the prelude must be imported.
    /// Enforces the "no wildcard import" principle of design doc §4.7 at check time.
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

    /// Names in a standard module (builtins + public declarations of its Siskin-written parts).
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

    /// Checks at check time that `from std.x import a, b` names actually exist
    /// (previously this was only discovered by running `siskin run`).
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
        // Argument count. `round(x, 1)` used to silently drop its second argument.
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
            // Builtins used by the Siskin-written parts of the standard library
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
                // If the function is in a header but could not be imported automatically yet, explain why it cannot be used.
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
/// The most similar name among the candidates (None if none).
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

/// For typo suggestions: considered similar if the case/underscore-insensitive edit distance is small or one contains the other.
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

/// When calling a nonexistent method: if it is another language's name, give the Siskin name; otherwise list that type's methods.
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

/// Whether a block ends with `return` (or `exit`) on every path.
/// match is assumed to have passed exhaustiveness checks (T0012/T0068); it ends if every arm ends.
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

/// Whether there is a `break` that exits this loop itself (breaks in inner loops don't count).
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
