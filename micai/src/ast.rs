/// 문법 나무는 여러 작업(스레드)이 같이 읽으므로 원자적 참조 계수(Arc)를 씁니다.
pub type Shared<T> = std::sync::Arc<T>;

/// 타입 표기. P1에서는 파싱해서 보관만 하고 검사하지 않습니다.
/// 타입 검사기는 P2입니다.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// `Int`, `Str`, `Stack[Int]`
    Named(String, Vec<TypeExpr>),
    /// `?T` — 값이 없을 수 있음
    Optional(Box<TypeExpr>),
    /// `!T` — 실패할 수 있음
    /// `!T` 또는 `E!T` — 둘째가 오류 타입(없으면 Str).
    Fallible(Box<TypeExpr>, Option<Box<TypeExpr>>),
    /// `[T]`
    List(Box<TypeExpr>),
    /// `{K: V}`
    Dict(Box<TypeExpr>, Box<TypeExpr>),
    /// `*T` — 원시 포인터 (Level 2, P4)
    Raw(Box<TypeExpr>),
    /// `(T, U, ...)` — 튜플
    Tuple(Vec<TypeExpr>),
    /// `(A, B) -> R` — 함수 타입
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

/// 호출 인자. `Token(kind: "word", text: w)` 처럼 이름 붙은 인자를 지원합니다.
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
    /// `(a, b, ...)` — 튜플 리터럴 (원소 2개 이상)
    Tuple(Vec<Expr>),
    Dict(Vec<(Expr, Expr)>),
    Unary(UnOp, Box<Expr>, usize, usize),
    Binary(BinOp, Box<Expr>, Box<Expr>, usize, usize),
    Call {
        callee: Box<Expr>,
        /// `alloc[Int](16)` 처럼 명시한 타입 인자. 보통은 비어 있습니다.
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
    /// `try expr` — 실패하면 현재 함수에서 즉시 그 에러를 반환합니다.
    Try(Box<Expr>, usize, usize),
    /// `expr else 기본값` — 왼쪽 `?T`가 없으면(none이면) 오른쪽을 씁니다.
    OrElse(Box<Expr>, Box<Expr>, usize, usize),
    /// `fn(x: Int): x * k` — 익명 함수(클로저). 본문은 `return 식` 한 문장입니다.
    /// 바깥 지역 변수를 쓰면 만들 때의 값을 복사해 붙잡습니다.
    Lambda(Shared<FnDecl>, usize, usize),
    /// `spawn f(x)` — 호출을 새 작업(스레드)에서 돌립니다. 인자 없는 익명 함수
    /// `fn(): f(x)` 로 감싸 두어, 쓰는 바깥 값은 클로저처럼 복사해 붙잡습니다.
    Spawn(Shared<FnDecl>, usize, usize),
}

#[derive(Debug, Clone)]
pub enum FStrPart {
    Lit(String),
    /// 표현식과 서식 스펙(`{x:.2f}`의 `.2f`). 스펙이 없으면 빈 문자열.
    Expr(Box<Expr>, String),
}

/// 블록이 반드시 빠져나가는지(return/break/continue, 또는 모든 갈래가 빠져나가는
/// if/match) 봅니다. 가드 절 뒤에서 `?T`를 좁혀 주기 위해 씁니다.
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

/// 함수 인자 전달 규약 (설계 문서 §5).
/// 라이프타임 표기 없이 이 세 가지만으로 소유권을 표현합니다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Convention {
    /// 읽기 전용 빌림 (기본) — C++의 `const T&`
    Borrow,
    /// 가변 빌림 — C++의 `T&`
    Inout,
    /// 소유권 이전 — C++의 `T&&`
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
    /// 사전 조건 (설계 문서 §9.5). 디버그 빌드에서 검사합니다.
    pub requires: Vec<Expr>,
    /// 사후 조건. `result`로 반환값을 참조할 수 있습니다.
    pub ensures: Vec<Expr>,
    pub body: Vec<Stmt>,
    pub line: usize,
    /// `extern "C" fn ...` — 본문이 없고, C 쪽 함수를 그대로 부릅니다.
    pub is_extern: bool,
    /// 헤더에서 자동으로 가져온 함수면 원래 C 시그니처가 들어 있습니다.
    /// 이게 있으면 cgen이 헤더를 include하고 타입을 맞춘 껍데기 함수를 냅니다.
    pub c_sig: Option<CSig>,
}

/// 익명 함수(`fn(x): ...`)의 이름은 이 글자로 시작합니다.
pub const LAMBDA_PREFIX: &str = "λ";

impl FnDecl {
    /// `fn(x): 식` 으로 만든 익명 함수인가. 반환 타입을 적지 않으면 식에서 추론합니다.
    pub fn is_lambda(&self) -> bool {
        self.name.starts_with(LAMBDA_PREFIX)
    }

    /// 사람에게 보여 줄 이름. 익명 함수는 "익명 함수".
    pub fn shown_name(&self) -> String {
        if self.is_lambda() {
            tr!("익명 함수", "anonymous function").to_string()
        } else {
            self.name.clone()
        }
    }
}

/// 함수 본문이 바깥에서 가져다 쓰는 이름들(등장 순서). 인자와 본문 안에서
/// 선언한 이름은 뺍니다. 클로저가 무엇을 붙잡아야 하는지 정하는 데 씁니다.
/// 여기 나온 이름 가운데 "만드는 순간 보이는 지역 변수"만 실제로 붙잡힙니다.
pub fn free_vars(f: &FnDecl) -> Vec<String> {
    let mut fv = FreeVars { scopes: vec![Vec::new()], out: Vec::new() };
    for p in &f.params {
        fv.bind(&p.name);
    }
    if !f.is_lambda() {
        // 이름 붙은 중첩 함수는 자기 이름으로 자기를 부를 수 있습니다.
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

/// 헤더에서 읽어 온 C(또는 C++) 함수의 원래 모습.
#[derive(Debug, Clone)]
pub struct CSig {
    /// 원래 C 반환 타입 (`uLong`, `const char *` 같은 것). 캐스트에 씁니다.
    pub ret: String,
    /// 원래 C 인자 타입들.
    pub params: Vec<String>,
    /// 이 함수를 선언한 헤더. `#include` 에 씁니다.
    pub header: String,
    /// C++ 이면 별도 파일로 빼서 C++ 컴파일러로 컴파일합니다.
    pub cpp: bool,
    /// C에서 실제로 부를 이름. C는 함수 이름 그대로, C++은 껍데기 이름.
    pub call: String,
    /// C++ 이면 통째로 만들어 둔 `extern "C"` 껍데기 함수의 본문.
    pub shim: Option<String>,
    /// 각 자리가 "함수를 넘겨 달라"는 자리면 `(C 인자 타입들, C 반환 타입)`.
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

/// `case` 패턴.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// `case _:`
    Wildcard,
    /// `case Circle(r):` — 열거형 변형과 바인딩 이름들
    Variant(String, Vec<String>),
    /// `case 0:` 같은 리터럴
    Literal(Expr),
    /// `case n:` — 이름 하나에 통째로 바인딩
    Bind(String),
}

#[derive(Debug, Clone)]
pub struct MatchCase {
    pub pattern: Pattern,
    pub body: Vec<Stmt>,
    pub line: usize,
}

/// `expr catch e:` 블록
#[derive(Debug, Clone)]
pub struct CatchClause {
    pub name: String,
    pub body: Vec<Stmt>,
}

/// 블록이 끝까지 흘러가지 않고 반드시 빠져나가는가 (return / break / continue / exit).
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

/// `let x = f() catch e:` 블록의 마지막 줄이 식이면, 실패했을 때 x 에 대신 넣을 값 후보입니다.
/// (값이 없는 식, 예를 들어 `print(...)` 인지는 타입 검사가 가립니다.)
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
    /// `let (a, b) = 튜플식` — 튜플을 이름들로 풀어 받습니다. 모두 불변(let).
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
        /// `for k, v in d:` 의 둘째 이름(값). 보통은 None입니다.
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
    /// `with arena a:` — 메모리 Level 1. 블록을 벗어나면 통째로 해제됩니다.
    Arena {
        name: String,
        body: Vec<Stmt>,
        line: usize,
    },
    /// `extern "C" link "m"` — 링크할 C 라이브러리 이름.
    Link(String, usize),
    /// `import c "zlib.h" link "z"` — 헤더를 읽어 함수를 통째로 가져옵니다.
    /// `resolve_imports` 단계에서 `Fn`/`Link` 들로 펼쳐집니다.
    CHeader {
        header: String,
        cpp: bool,
        links: Vec<String>,
        incdirs: Vec<String>,
        only: Vec<String>,
        line: usize,
        col: usize,
    },
    /// `unsafe:` — 메모리 Level 2. 원시 포인터를 쓸 수 있는 구간.
    Unsafe {
        body: Vec<Stmt>,
        line: usize,
    },
    /// `from std.io import println` / `import std.math`
    Import {
        path: Vec<String>,
        names: Vec<String>,
        /// `from a import x as y` 의 y 들 (`as` 가 없으면 이름 그대로). `names` 와 길이가 같습니다.
        renames: Vec<String>,
        /// `import pkg.a as b` 의 b
        alias: Option<String>,
        line: usize,
        col: usize,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Program {
    pub stmts: Vec<Stmt>,
}

// 문법 나무는 스레드 사이에 같이 읽어도 안전해야 합니다(`siskin run` 의 spawn 이 진짜 병렬).
const _: fn() = || {
    fn shareable<T: Send + Sync>() {}
    shareable::<FnDecl>();
    shareable::<Program>();
};
