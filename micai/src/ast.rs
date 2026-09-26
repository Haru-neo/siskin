/// The syntax tree is read by several tasks (threads) at once, so it uses atomic reference counting (Arc).
pub type Shared<T> = std::sync::Arc<T>;

/// Type annotation. In P1 it is only parsed and stored, not checked.
/// The type checker is P2.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// `Int`, `Str`, `Stack[Int]`
    Named(String, Vec<TypeExpr>),
    /// `?T` — value may be absent
    Optional(Box<TypeExpr>),
    /// `!T` — may fail
    /// `!T` or `E!T` — the second is the error type (Str if absent).
    Fallible(Box<TypeExpr>, Option<Box<TypeExpr>>),
    /// `[T]`
    List(Box<TypeExpr>),
    /// `{K: V}`
    Dict(Box<TypeExpr>, Box<TypeExpr>),
    /// `*T` — raw pointer (Level 2, P4)
    Raw(Box<TypeExpr>),
    /// `(T, U, ...)` — tuple
    Tuple(Vec<TypeExpr>),
    /// `(A, B) -> R` — function type
    Fn(Vec<TypeExpr>, Box<TypeExpr>),
}

impl TypeExpr {
    pub fn render(&self) -> String {
        match self {
            TypeExpr::Named(n, args) => {
                if args.is_empty() {
                    n.clone()
                } else {
                    let inner: Vec<String> = args.iter().map(|a| a.render()).collect();
                    format!("{}[{}]", n, inner.join(", "))
                }
            }
            TypeExpr::Optional(t) => format!("?{}", t.render()),
            TypeExpr::Fallible(t, None) => format!("!{}", t.render()),
            TypeExpr::Fallible(t, Some(e)) => format!("{}!{}", e.render(), t.render()),
            TypeExpr::List(t) => format!("[{}]", t.render()),
            TypeExpr::Dict(k, v) => format!("{{{}: {}}}", k.render(), v.render()),
            TypeExpr::Raw(t) => format!("*{}", t.render()),
            TypeExpr::Tuple(ts) => {
                let inner: Vec<String> = ts.iter().map(|t| t.render()).collect();
                format!("({})", inner.join(", "))
            }
            TypeExpr::Fn(ps, r) => {
                let inner: Vec<String> = ps.iter().map(|t| t.render()).collect();
                format!("({}) -> {}", inner.join(", "), r.render())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn symbol(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}

/// Call argument. Supports named arguments like `Token(kind: "word", text: w)`.
#[derive(Debug, Clone)]
pub struct Arg {
    pub name: Option<String>,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Str(Shared<String>),
    Bool(bool),
    NoneLit,
    Ident(String, usize, usize),
    FString(Vec<FStrPart>),
    List(Vec<Expr>),
    /// `(a, b, ...)` — tuple literal (2 or more elements)
    Tuple(Vec<Expr>),
    Dict(Vec<(Expr, Expr)>),
    Unary(UnOp, Box<Expr>, usize, usize),
    Binary(BinOp, Box<Expr>, Box<Expr>, usize, usize),
    Call {
        callee: Box<Expr>,
        /// Explicit type arguments as in `alloc[Int](16)`. Usually empty.
        targs: Vec<TypeExpr>,
        args: Vec<Arg>,
        line: usize,
        col: usize,
    },
    Field(Box<Expr>, String, usize, usize),
    Index(Box<Expr>, Box<Expr>, usize, usize),
    /// `a if cond else b`
    IfExpr {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
    },
    /// `try expr` — on failure, immediately returns that error from the current function.
    Try(Box<Expr>, usize, usize),
    /// `expr else default` — if the left `?T` is absent (none), uses the right side.
    OrElse(Box<Expr>, Box<Expr>, usize, usize),
    /// `fn(x: Int): x * k` — anonymous function (closure). The body is a single `return expr` statement.
    /// Outer local variables it uses are captured by copying their values at creation time.
    Lambda(Shared<FnDecl>, usize, usize),
    /// `spawn f(x)` — runs the call in a new task (thread). It is wrapped in a parameterless anonymous
    /// function `fn(): f(x)`, so outer values it uses are captured by copy, like a closure.
    Spawn(Shared<FnDecl>, usize, usize),
}

#[derive(Debug, Clone)]
pub enum FStrPart {
    Lit(String),
    /// The expression and its format spec (the `.2f` in `{x:.2f}`). Empty string if there is no spec.
    Expr(Box<Expr>, String),
}

/// Checks whether a block always exits (return/break/continue, or an if/match whose every
/// branch exits). Used to narrow `?T` after a guard clause.
pub fn block_diverges(body: &[Stmt]) -> bool {
    match body.last() {
        Some(Stmt::Return(..)) | Some(Stmt::Break(..)) | Some(Stmt::Continue(..)) => true,
        Some(Stmt::If { arms, els }) => {
            els.as_ref().map_or(false, |e| block_diverges(e))
                && arms.iter().all(|(_, b)| block_diverges(b))
        }
        Some(Stmt::Match { cases, .. }) => {
            !cases.is_empty() && cases.iter().all(|c| block_diverges(&c.body))
        }
        _ => false,
    }
}

impl Expr {
    pub fn pos(&self) -> (usize, usize) {
        match self {
            Expr::Ident(_, l, c) => (*l, *c),
            Expr::Unary(_, _, l, c) => (*l, *c),
            Expr::Binary(_, _, _, l, c) => (*l, *c),
            Expr::Call { line, col, .. } => (*line, *col),
            Expr::Field(_, _, l, c) => (*l, *c),
            Expr::Index(_, _, l, c) => (*l, *c),
            Expr::Try(_, l, c) => (*l, *c),
            Expr::OrElse(_, _, l, c) => (*l, *c),
            Expr::Lambda(_, l, c) => (*l, *c),
            Expr::Spawn(_, l, c) => (*l, *c),
            _ => (0, 0),
        }
    }
}

/// Argument passing conventions (design doc §5).
/// Ownership is expressed with just these three, without lifetime annotations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Convention {
    /// Read-only borrow (default) — C++'s `const T&`
    Borrow,
    /// Mutable borrow — C++'s `T&`
    Inout,
    /// Ownership transfer — C++'s `T&&`
    Owned,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Option<TypeExpr>,
    pub conv: Convention,
    pub is_self: bool,
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    pub name: String,
    pub generics: Vec<String>,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub doc: Option<String>,
    /// Preconditions (design doc §9.5). Checked in debug builds.
    pub requires: Vec<Expr>,
    /// Postconditions. The return value can be referred to as `result`.
    pub ensures: Vec<Expr>,
    pub body: Vec<Stmt>,
    pub line: usize,
    /// `extern "C" fn ...` — no body; calls the C function directly.
    pub is_extern: bool,
    /// For a function imported automatically from a header, holds the original C signature.
    /// When present, cgen includes the header and emits a type-adapting wrapper function.
    pub c_sig: Option<CSig>,
}

/// Names of anonymous functions (`fn(x): ...`) start with this character.
pub const LAMBDA_PREFIX: &str = "λ";

impl FnDecl {
    /// Whether this is an anonymous function made with `fn(x): expr`. If no return type is written, it is inferred from the expression.
    pub fn is_lambda(&self) -> bool {
        self.name.starts_with(LAMBDA_PREFIX)
    }

    /// Name shown to people. Anonymous functions show as "anonymous function".
    pub fn shown_name(&self) -> String {
        if self.is_lambda() {
            tr!("익명 함수", "anonymous function").to_string()
        } else {
            self.name.clone()
        }
    }
}

/// Names the function body uses from outside (in order of appearance). Excludes parameters and names
/// declared inside the body. Used to decide what a closure must capture.
/// Of the names listed here, only "local variables visible at creation time" are actually captured.
pub fn free_vars(f: &FnDecl) -> Vec<String> {
    let mut fv = FreeVars { scopes: vec![Vec::new()], out: Vec::new() };
    for p in &f.params {
        fv.bind(&p.name);
    }
    if !f.is_lambda() {
        // A named nested function can call itself by its own name.
        fv.bind(&f.name);
    }
    for r in &f.requires {
        fv.expr(r);
    }
    fv.block(&f.body);
    fv.scopes.push(vec!["result".to_string()]);
    for e in &f.ensures {
        fv.expr(e);
    }
    fv.out
}

struct FreeVars {
    scopes: Vec<Vec<String>>,
    out: Vec<String>,
}

impl FreeVars {
    fn bind(&mut self, n: &str) {
        self.scopes.last_mut().unwrap().push(n.to_string());
    }
    fn bound(&self, n: &str) -> bool {
        self.scopes.iter().any(|s| s.iter().any(|x| x == n))
    }
    fn use_name(&mut self, n: &str) {
        if !self.bound(n) && !self.out.iter().any(|x| x == n) {
            self.out.push(n.to_string());
        }
    }
    fn block(&mut self, b: &[Stmt]) {
        self.scopes.push(Vec::new());
        for s in b {
            self.stmt(s);
        }
        self.scopes.pop();
    }
    fn catch(&mut self, c: &Option<CatchClause>) {
        if let Some(c) = c {
            self.scopes.push(vec![c.name.clone()]);
            for s in &c.body {
                self.stmt(s);
            }
            self.scopes.pop();
        }
    }
    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { name, value, catch, .. } => {
                self.expr(value);
                self.catch(catch);
                self.bind(name);
            }
            Stmt::LetTuple { names, value, .. } => {
                self.expr(value);
                for n in names {
                    self.bind(n);
                }
            }
            Stmt::Assign { target, value, catch, .. } => {
                self.expr(target);
                self.expr(value);
                self.catch(catch);
            }
            Stmt::Expr(e, c) => {
                self.expr(e);
                self.catch(c);
            }
            Stmt::If { arms, els } => {
                for (c, b) in arms {
                    self.expr(c);
                    self.block(b);
                }
                if let Some(b) = els {
                    self.block(b);
                }
            }
            Stmt::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            Stmt::For { var, var2, iter, body, .. } => {
                self.expr(iter);
                self.scopes.push(vec![var.clone()]);
                if let Some(v) = var2 {
                    self.bind(v);
                }
                self.block(body);
                self.scopes.pop();
            }
            Stmt::Match { subject, cases, .. } => {
                self.expr(subject);
                for c in cases {
                    self.scopes.push(Vec::new());
                    match &c.pattern {
                        Pattern::Variant(_, names) => {
                            for n in names {
                                self.bind(n);
                            }
                        }
                        Pattern::Bind(n) => self.bind(n),
                        Pattern::Literal(e) => self.expr(e),
                        Pattern::Wildcard => {}
                    }
                    self.block(&c.body);
                    self.scopes.pop();
                }
            }
            Stmt::Return(Some(e), _, _) => self.expr(e),
            Stmt::Fn(f) => {
                self.bind(&f.name);
                for n in free_vars(f) {
                    self.use_name(&n);
                }
            }
            Stmt::Arena { name, body, .. } => {
                self.scopes.push(vec![name.clone()]);
                self.block(body);
                self.scopes.pop();
            }
            Stmt::Unsafe { body, .. } => self.block(body),
            _ => {}
        }
    }
    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Ident(n, _, _) => self.use_name(n),
            Expr::FString(parts) => {
                for p in parts {
                    if let FStrPart::Expr(e, _) = p {
                        self.expr(e);
                    }
                }
            }
            Expr::List(xs) | Expr::Tuple(xs) => {
                for x in xs {
                    self.expr(x);
                }
            }
            Expr::Dict(ps) => {
                for (k, v) in ps {
                    self.expr(k);
                    self.expr(v);
                }
            }
            Expr::Unary(_, a, _, _) | Expr::Try(a, _, _) => self.expr(a),
            Expr::Binary(_, a, b, _, _) | Expr::OrElse(a, b, _, _) | Expr::Index(a, b, _, _) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Call { callee, args, .. } => {
                self.expr(callee);
                for a in args {
                    self.expr(&a.value);
                }
            }
            Expr::Field(o, _, _, _) => self.expr(o),
            Expr::IfExpr { cond, then, els } => {
                self.expr(cond);
                self.expr(then);
                self.expr(els);
            }
            Expr::Lambda(f, _, _) | Expr::Spawn(f, _, _) => {
                for n in free_vars(f) {
                    self.use_name(&n);
                }
            }
            _ => {}
        }
    }
}

/// The original form of a C (or C++) function read from a header.
#[derive(Debug, Clone)]
pub struct CSig {
    /// Original C return type (things like `uLong`, `const char *`). Used for casts.
    pub ret: String,
    /// Original C parameter types.
    pub params: Vec<String>,
    /// The header that declares this function. Used for `#include`.
    pub header: String,
    /// If C++, it is split into a separate file and compiled with a C++ compiler.
    pub cpp: bool,
    /// The name actually called from C. For C, the function name itself; for C++, the wrapper name.
    pub call: String,
    /// If C++, the body of the fully generated `extern "C"` wrapper function.
    pub shim: Option<String>,
    /// For each position that expects a function to be passed, `(C parameter types, C return type)`.
    pub cbs: Vec<Option<(Vec<String>, String)>>,
}

#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub name: String,
    pub ty: Option<TypeExpr>,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: String,
    pub generics: Vec<String>,
    pub interfaces: Vec<String>,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<Shared<FnDecl>>,
    pub doc: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct VariantDecl {
    pub name: String,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<VariantDecl>,
    pub methods: Vec<Shared<FnDecl>>,
    pub doc: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct InterfaceDecl {
    pub name: String,
    pub methods: Vec<String>,
    pub line: usize,
}

/// `case` pattern.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// `case _:`
    Wildcard,
    /// `case Circle(r):` — enum variant and binding names
    Variant(String, Vec<String>),
    /// A literal such as `case 0:`
    Literal(Expr),
    /// `case n:` — binds the whole value to a single name
    Bind(String),
}

#[derive(Debug, Clone)]
pub struct MatchCase {
    pub pattern: Pattern,
    pub body: Vec<Stmt>,
    pub line: usize,
}

/// `expr catch e:` block
#[derive(Debug, Clone)]
pub struct CatchClause {
    pub name: String,
    pub body: Vec<Stmt>,
}

/// Whether a block never falls through to its end and always exits (return / break / continue / exit).
pub fn block_leaves(body: &[Stmt]) -> bool {
    match body.last() {
        Some(Stmt::Return(..)) | Some(Stmt::Break(..)) | Some(Stmt::Continue(..)) => true,
        Some(Stmt::Expr(Expr::Call { callee, .. }, None)) => {
            matches!(&**callee, Expr::Ident(n, ..) if n == "exit")
        }
        Some(Stmt::If { arms, els: Some(e) }) => arms.iter().all(|(_, b)| block_leaves(b)) && block_leaves(e),
        _ => false,
    }
}

/// If the last line of a `let x = f() catch e:` block is an expression, it is the candidate value to put into x on failure.
/// (Whether it is a valueless expression, e.g. `print(...)`, is decided by the type checker.)
pub fn catch_fallback(c: &CatchClause) -> Option<&Expr> {
    match c.body.last() {
        Some(Stmt::Expr(e, None)) if !block_leaves(&c.body) => Some(e),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        ty: Option<TypeExpr>,
        value: Expr,
        mutable: bool,
        catch: Option<CatchClause>,
        line: usize,
        col: usize,
    },
    Assign {
        target: Expr,
        op: Option<BinOp>,
        value: Expr,
        catch: Option<CatchClause>,
        line: usize,
        col: usize,
    },
    /// `let (a, b) = tuple_expr` — destructures a tuple into names. All immutable (let).
    LetTuple {
        names: Vec<String>,
        value: Expr,
        line: usize,
        col: usize,
    },
    Expr(Expr, Option<CatchClause>),
    If {
        arms: Vec<(Expr, Vec<Stmt>)>,
        els: Option<Vec<Stmt>>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    For {
        var: String,
        /// The second name (the value) in `for k, v in d:`. Usually None.
        var2: Option<String>,
        iter: Expr,
        body: Vec<Stmt>,
        line: usize,
    },
    Match {
        subject: Expr,
        cases: Vec<MatchCase>,
        line: usize,
    },
    Return(Option<Expr>, usize, usize),
    Break(usize, usize),
    Continue(usize, usize),
    Fn(Shared<FnDecl>),
    Struct(Shared<StructDecl>),
    Enum(Shared<EnumDecl>),
    Interface(Shared<InterfaceDecl>),
    /// `with arena a:` — memory Level 1. Freed all at once on leaving the block.
    Arena {
        name: String,
        body: Vec<Stmt>,
        line: usize,
    },
    /// `extern "C" link "m"` — name of a C library to link.
    Link(String, usize),
    /// `import c "zlib.h" link "z"` — reads a header and imports all its functions.
    /// Expanded into `Fn`/`Link` items during the `resolve_imports` stage.
    CHeader {
        header: String,
        cpp: bool,
        links: Vec<String>,
        incdirs: Vec<String>,
        only: Vec<String>,
        line: usize,
        col: usize,
    },
    /// `unsafe:` — memory Level 2. A region where raw pointers may be used.
    Unsafe {
        body: Vec<Stmt>,
        line: usize,
    },
    /// `from std.io import println` / `import std.math`
    Import {
        path: Vec<String>,
        names: Vec<String>,
        /// The y's in `from a import x as y` (the name itself if there is no `as`). Same length as `names`.
        renames: Vec<String>,
        /// The b in `import pkg.a as b`
        alias: Option<String>,
        line: usize,
        col: usize,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Program {
    pub stmts: Vec<Stmt>,
}

// The syntax tree must be safe to read concurrently across threads (spawn in `siskin run` is truly parallel).
const _: fn() = || {
    fn shareable<T: Send + Sync>() {}
    shareable::<FnDecl>();
    shareable::<Program>();
};
