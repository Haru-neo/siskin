//! 모듈 이름공간.
//!
//! `import a` 로 가져온 파일의 이름은 `a.greet(...)` 처럼 모듈 이름을 붙여 씁니다.
//! 그래서 두 파일(패키지)이 같은 이름의 함수를 만들어도 부딪히지 않습니다.
//!
//! 방법: import 한 파일마다 앞 글자(모듈 이름)를 정하고, 그 파일의 최상위 선언
//! (함수·구조체·열거형과 그 변형·인터페이스)을 `a·greet` 로 바꿔 부릅니다. 그 파일
//! 안과 가져다 쓰는 쪽의 이름도 모두 이 이름으로 바꿔 둡니다. `·` 는 사용자가 쓸 수
//! 없는 글자라 사용자 이름과 절대 안 겹치고, 사람에게 보일 때는 `.` 로 보입니다.
//! 이 뒤 단계(타입 검사, 인터프리터, C 생성)는 이름공간을 전혀 몰라도 됩니다.
//!
//! main 파일은 앞 글자가 없어서 이름이 그대로입니다(한 파일 프로그램은 아무것도 안 바뀜).
//!
//! 규칙:
//! - 내 파일에 선언한 이름이 먼저입니다. 그다음 `from a import x` 로 가져온 이름.
//! - `import a` 만 하고 `greet(...)` 처럼 모듈 이름 없이 써도 가져온 모듈 가운데 한 곳에만
//!   있으면 됩니다(예전 코드를 위해). 이때는 `a.greet` 로 쓰라고 경고합니다.
//!   두 곳 이상에 있으면 오류입니다.
//! - `_` 로 시작하는 이름은 그 파일 안에서만 씁니다(밖에서 부르면 오류).
//! - `extern` 함수와 `import c` 로 가져온 C 함수는 C 쪽 이름 그대로 두어 어디서나 보입니다.

use crate::ast::*;
use crate::error::SiskinError;
use std::collections::{HashMap, HashSet};
use crate::ast::Shared as Rc;

/// 모듈 이름과 선언 이름 사이에 넣는 글자. 식별자에 못 쓰는 글자입니다.
pub const SEP: char = '·';

/// 사람에게 보여 줄 모양: `a·greet` → `a.greet`.
pub fn shown(s: &str) -> String {
    if s.contains(SEP) {
        s.replace(SEP, ".")
    } else {
        s.to_string()
    }
}

/// 값을 찍을 때 쓰는 이름: 모듈 이름을 뗀 `greet`.
pub fn plain(s: &str) -> &str {
    match s.rfind(SEP) {
        Some(i) => &s[i + SEP.len_utf8()..],
        None => s,
    }
}

pub struct Module {
    /// 앞 글자. main 은 빈 글자.
    pub prefix: String,
    pub stmts: Vec<Stmt>,
    pub imports: Vec<ImportRef>,
}

pub struct ImportRef {
    pub target: usize,
    /// `import a` / `import pkg.a as b` 로 붙은 이름. `from` 이면 None.
    pub alias: Option<String>,
    /// `from a import x as y` 의 (x, y)
    pub names: Vec<(String, String)>,
    pub line: usize,
    pub col: usize,
}

impl Module {
    pub fn mangle(&self, n: &str) -> String {
        if self.prefix.is_empty() {
            n.to_string()
        } else {
            format!("{}{}{}", self.prefix, SEP, n)
        }
    }
}

/// 한 모듈이 밖에 내놓는 이름들: 원래 이름 → 바뀐 이름.
fn exports(m: &Module) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for s in &m.stmts {
        for n in top_names(s) {
            out.insert(n.clone(), m.mangle(&n));
        }
    }
    out
}

/// 최상위 선언이 만드는 이름들 (바꿔 부를 것만).
fn top_names(s: &Stmt) -> Vec<String> {
    match s {
        Stmt::Fn(f) if !f.is_extern && f.c_sig.is_none() => vec![f.name.clone()],
        Stmt::Struct(sd) => vec![sd.name.clone()],
        Stmt::Enum(ed) => {
            let mut v = vec![ed.name.clone()];
            v.extend(ed.variants.iter().map(|x| x.name.clone()));
            v
        }
        Stmt::Interface(id) => vec![id.name.clone()],
        Stmt::Let { name, .. } => vec![name.clone()],
        _ => Vec::new(),
    }
}

/// 모든 모듈의 이름을 풀어 하나의 문장 목록으로 합칩니다.
/// 모듈마다 풀린 문장 목록을 돌려줍니다. 실패하면 (모듈 번호, 오류) 입니다. 경고는 `error::push_warning` 으로 냅니다.
/// `globals` 는 이름공간 없이 어디서나 보이는 이름(표준 라이브러리 조각, C 함수).
pub fn resolve(mods: &mut [Module], globals: &HashSet<String>) -> Result<Vec<Vec<Stmt>>, (usize, SiskinError)> {
    let tables: Vec<HashMap<String, String>> = mods.iter().map(exports).collect();
    let prefixes: Vec<String> = mods.iter().map(|m| m.prefix.clone()).collect();
    let mut out = Vec::new();
    for i in 0..mods.len() {
        let env = build_env(i, mods, &tables, &prefixes, globals).map_err(|e| (i, e))?;
        let mut r = Resolver { env, scopes: vec![HashSet::new()], types: vec![HashSet::new()], err: None, quiet: false, line: 0 };
        let m = &mut mods[i];
        let own: HashSet<String> = m.stmts.iter().flat_map(top_names).collect();
        for s in m.stmts.iter_mut() {
            r.top(s, &own, &m.prefix);
            if let Some(e) = r.err.take() {
                return Err((i, e));
            }
        }
        out.push(std::mem::take(&mut m.stmts));
    }
    Ok(out)
}

struct Env {
    /// 모듈 이름 없이 쓸 수 있는 이름 → 바뀐 이름
    direct: HashMap<String, String>,
    /// `import a` 의 a → (그 모듈의 내놓는 이름, 모듈 이름)
    aliases: HashMap<String, (HashMap<String, String>, String, bool)>,
    /// 모듈 이름 없이 썼지만 가져온 모듈에서 찾은 이름 → (바뀐 이름, 모듈 이름들)
    loose: HashMap<String, Vec<(String, String)>>,
}

fn private(n: &str) -> bool {
    n.starts_with('_')
}

fn no_name_err(module: &str, n: &str, table: &HashMap<String, String>, line: usize, col: usize) -> SiskinError {
    let mut have: Vec<&String> = table.keys().filter(|k| !private(k)).collect();
    have.sort();
    let e = SiskinError::new(
        "E0145",
        tr!(format!("모듈 `{}` 에 `{}` 이(가) 없습니다", module, n), format!("module `{}` has no `{}`", module, n)),
        line,
        col,
    );
    if have.is_empty() {
        e
    } else {
        let list = have.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
        e.with_fix(tr!(format!("있는 이름: {}", list), format!("available: {}", list)))
    }
}

fn private_err(module: &str, n: &str, line: usize, col: usize) -> SiskinError {
    SiskinError::new(
        "E0146",
        tr!(
            format!("`{}.{}` 은(는) `_` 로 시작해서 `{}` 파일 안에서만 쓸 수 있습니다", module, n, module),
            format!("`{}.{}` starts with `_`, so it can only be used inside `{}`", module, n, module)
        ),
        line,
        col,
    )
    .with_fix(tr!("밖에서 써야 하면 이름 앞의 `_` 를 빼세요", "remove the leading `_` if it should be usable from other files"))
}

fn build_env(
    i: usize,
    mods: &[Module],
    tables: &[HashMap<String, String>],
    prefixes: &[String],
    globals: &HashSet<String>,
) -> Result<Env, SiskinError> {
    let m = &mods[i];
    let mut direct: HashMap<String, String> = tables[i].clone();
    let mut aliases = HashMap::new();
    let mut loose: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut from_where: HashMap<String, String> = HashMap::new();
    for im in &m.imports {
        let mname = &prefixes[im.target];
        let table = &tables[im.target];
        if let Some(a) = &im.alias {
            if let Some((_, other, _)) = aliases.get(a) {
                if other != mname {
                    return Err(SiskinError::new(
                        "E0148",
                        tr!(
                            format!("`{}` 라는 이름으로 두 모듈을 가져왔습니다", a),
                            format!("two modules are imported under the same name `{}`", a)
                        ),
                        im.line,
                        im.col,
                    )
                    .with_fix(tr!(
                        format!("하나에 다른 이름을 붙이세요: `import ... as 다른이름`"),
                        format!("give one of them another name: `import ... as other_name`")
                    )));
                }
            }
            let pubs: HashMap<String, String> =
                table.iter().filter(|(k, _)| !private(k) || im.target == i).map(|(k, v)| (k.clone(), v.clone())).collect();
            for (k, v) in &pubs {
                if im.target != i {
                    let e = loose.entry(k.clone()).or_default();
                    if !e.iter().any(|(x, _)| x == v) {
                        e.push((v.clone(), a.clone()));
                    }
                }
            }
            aliases.insert(a.clone(), (table.clone(), mname.clone(), im.target == i));
        }
        for (n, alias) in &im.names {
            let target = match table.get(n) {
                Some(t) => t.clone(),
                None => return Err(no_name_err(mname, n, table, im.line, im.col)),
            };
            if private(n) && im.target != i {
                return Err(private_err(mname, n, im.line, im.col));
            }
            if tables[i].contains_key(alias) {
                return Err(SiskinError::new(
                    "E0148",
                    tr!(
                        format!("`{}` 은(는) 이 파일에도 있고 `{}` 에서도 가져왔습니다", alias, mname),
                        format!("`{}` is defined in this file and also imported from `{}`", alias, mname)
                    ),
                    im.line,
                    im.col,
                )
                .with_fix(tr!(
                    format!("가져온 쪽에 다른 이름을 붙이세요: `from {} import {} as 다른이름`, 아니면 `import {}` 뒤 `{}.{}` 로 쓰세요", mname, n, mname, mname, n),
                    format!("rename the import: `from {} import {} as other_name`, or `import {}` and write `{}.{}`", mname, n, mname, mname, n)
                )));
            }
            if let Some(prev) = from_where.get(alias) {
                if direct.get(alias) != Some(&target) {
                    return Err(SiskinError::new(
                        "E0148",
                        tr!(
                            format!("`{}` 을(를) `{}` 와 `{}` 두 곳에서 가져왔습니다", alias, prev, mname),
                            format!("`{}` is imported from both `{}` and `{}`", alias, prev, mname)
                        ),
                        im.line,
                        im.col,
                    )
                    .with_fix(tr!(
                        format!("하나에 다른 이름을 붙이세요: `from {} import {} as 다른이름`", mname, n),
                        format!("rename one of them: `from {} import {} as other_name`", mname, n)
                    )));
                }
            }
            from_where.insert(alias.clone(), mname.clone());
            direct.insert(alias.clone(), target);
        }
    }
    // 어디서나 보이는 이름(표준 조각, C 함수, 내장)은 느슨한 찾기에서 뺍니다.
    loose.retain(|k, _| !direct.contains_key(k) && !globals.contains(k));
    Ok(Env { direct, aliases, loose })
}

struct Resolver {
    env: Env,
    /// 지역 이름(인자, let, for, case …). 모듈 이름보다 먼저입니다.
    scopes: Vec<HashSet<String>>,
    /// 제네릭 타입 이름
    types: Vec<HashSet<String>>,
    err: Option<SiskinError>,
    /// `case Circle(r):` 처럼 match 대상의 타입으로 이미 어디 것인지 보이는 자리. 경고하지 않습니다.
    quiet: bool,
    /// 지금 보고 있는 줄 (타입 표기에는 위치가 없어서 오류를 이 줄에 답니다)
    line: usize,
}

impl Resolver {
    fn fail(&mut self, e: SiskinError) {
        if self.err.is_none() {
            self.err = Some(e);
        }
    }
    fn bind(&mut self, n: &str) {
        self.scopes.last_mut().unwrap().insert(n.to_string());
    }
    fn local(&self, n: &str) -> bool {
        self.scopes.iter().any(|s| s.contains(n))
    }
    fn generic(&self, n: &str) -> bool {
        self.types.iter().any(|s| s.contains(n))
    }

    /// 모듈 이름 없이 쓴 이름. 바꿀 게 없으면 None.
    fn lookup(&mut self, n: &str, line: usize, col: usize) -> Option<String> {
        if let Some(t) = self.env.direct.get(n) {
            return if t == n { None } else { Some(t.clone()) };
        }
        let cands = self.env.loose.get(n)?.clone();
        if cands.len() == 1 {
            let (t, m) = &cands[0];
            if self.quiet {
                return Some(t.clone());
            }
            crate::error::push_warning(
                SiskinError::new(
                    "W0002",
                    tr!(
                        format!("`{}` 은(는) 모듈 `{}` 에 있습니다. `{}.{}` 로 쓰면 어디서 온 이름인지 보입니다", n, m, m, n),
                        format!("`{}` comes from module `{}`; write `{}.{}` so it is clear where it comes from", n, m, m, n)
                    ),
                    line,
                    col,
                )
                .with_fix(tr!(format!("`{}.{}`", m, n), format!("`{}.{}`", m, n))),
            );
            return Some(t.clone());
        }
        let ms: Vec<String> = cands.iter().map(|(_, m)| format!("`{}.{}`", m, n)).collect();
        self.fail(
            SiskinError::new(
                "E0147",
                tr!(
                    format!("`{}` 이(가) 가져온 모듈 여러 곳에 있어서 어느 것인지 모릅니다", n),
                    format!("`{}` is in more than one imported module, so it is ambiguous", n)
                ),
                line,
                col,
            )
            .with_fix(tr!(
                format!("모듈 이름을 붙이세요: {}", ms.join(", ")),
                format!("say which one: {}", ms.join(" or "))
            )),
        );
        None
    }

    /// `a.x` — a 가 가져온 모듈이면 바뀐 이름.
    fn qualified(&mut self, a: &str, x: &str, line: usize, col: usize) -> Option<String> {
        if self.local(a) {
            return None;
        }
        let (table, mname, own) = self.env.aliases.get(a)?.clone();
        match table.get(x) {
            Some(_) if private(x) && !own => {
                self.fail(private_err(&mname, x, line, col));
                None
            }
            Some(t) => Some(t.clone()),
            None => {
                self.fail(no_name_err(&mname, x, &table, line, col));
                None
            }
        }
    }

    fn top(&mut self, s: &mut Stmt, own: &HashSet<String>, prefix: &str) {
        let mangle = |n: &str| if prefix.is_empty() || !own.contains(n) { n.to_string() } else { format!("{}{}{}", prefix, SEP, n) };
        match s {
            Stmt::Fn(f) => {
                if f.is_extern || f.c_sig.is_some() {
                    return;
                }
                let fm = Rc::make_mut(f);
                fm.name = mangle(&fm.name);
                self.func(fm, false);
            }
            Stmt::Struct(sd) => {
                let sm = Rc::make_mut(sd);
                sm.name = mangle(&sm.name);
                self.line = sm.line;
                self.types.push(sm.generics.iter().cloned().collect());
                for i in sm.interfaces.iter_mut() {
                    self.type_name(i, 0, 0);
                }
                self.fields(&mut sm.fields);
                for m in sm.methods.iter_mut() {
                    self.func(Rc::make_mut(m), false);
                }
                self.types.pop();
            }
            Stmt::Enum(ed) => {
                let em = Rc::make_mut(ed);
                em.name = mangle(&em.name);
                self.line = em.line;
                for v in em.variants.iter_mut() {
                    v.name = mangle(&v.name);
                    self.fields(&mut v.fields);
                }
                for m in em.methods.iter_mut() {
                    self.func(Rc::make_mut(m), false);
                }
            }
            Stmt::Interface(id) => {
                let im = Rc::make_mut(id);
                im.name = mangle(&im.name);
            }
            Stmt::Let { name, .. } => {
                let n = mangle(name);
                self.stmt(s);
                if let Stmt::Let { name, .. } = s {
                    *name = n;
                }
                // 최상위 let 은 지역 이름이 아닙니다.
                self.scopes[0].clear();
            }
            _ => self.stmt(s),
        }
    }

    fn fields(&mut self, fs: &mut [FieldDecl]) {
        for f in fs {
            if let Some(t) = &mut f.ty {
                self.ty(t);
            }
            if let Some(d) = &mut f.default {
                self.expr(d);
            }
        }
    }

    fn type_name(&mut self, n: &mut String, line: usize, col: usize) {
        if self.generic(n) {
            return;
        }
        if let Some(i) = n.find('.') {
            let (a, x) = (n[..i].to_string(), n[i + 1..].to_string());
            if let Some(t) = self.qualified(&a, &x, line, col) {
                *n = t;
            }
            return;
        }
        if let Some(t) = self.lookup(n, line, col) {
            *n = t;
        }
    }

    fn ty(&mut self, t: &mut TypeExpr) {
        match t {
            TypeExpr::Named(n, args) => {
                let l = self.line;
                self.type_name(n, l, 1);
                for a in args {
                    self.ty(a);
                }
            }
            TypeExpr::Optional(a) | TypeExpr::List(a) | TypeExpr::Raw(a) => self.ty(a),
            TypeExpr::Fallible(a, e) => {
                self.ty(a);
                if let Some(e) = e {
                    self.ty(e);
                }
            }
            TypeExpr::Dict(k, v) => {
                self.ty(k);
                self.ty(v);
            }
            TypeExpr::Tuple(ts) => {
                for x in ts {
                    self.ty(x);
                }
            }
            TypeExpr::Fn(ps, r) => {
                for x in ps {
                    self.ty(x);
                }
                self.ty(r);
            }
        }
    }

    fn func(&mut self, f: &mut FnDecl, bind_self: bool) {
        if f.line > 0 {
            self.line = f.line;
        }
        if bind_self {
            self.bind(&f.name.clone());
        }
        self.types.push(f.generics.iter().cloned().collect());
        self.scopes.push(HashSet::new());
        for p in f.params.iter_mut() {
            if let Some(t) = &mut p.ty {
                self.ty(t);
            }
            let n = p.name.clone();
            self.bind(&n);
        }
        if let Some(t) = &mut f.ret {
            self.ty(t);
        }
        for r in f.requires.iter_mut() {
            self.expr(r);
        }
        self.block(&mut f.body);
        self.scopes.push(["result".to_string()].into_iter().collect());
        for e in f.ensures.iter_mut() {
            self.expr(e);
        }
        self.scopes.pop();
        self.scopes.pop();
        self.types.pop();
    }

    fn block(&mut self, b: &mut [Stmt]) {
        self.scopes.push(HashSet::new());
        for s in b {
            self.stmt(s);
        }
        self.scopes.pop();
    }

    fn catch(&mut self, c: &mut Option<CatchClause>) {
        if let Some(c) = c {
            self.scopes.push([c.name.clone()].into_iter().collect());
            self.block(&mut c.body);
            self.scopes.pop();
        }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Let { line, .. } | Stmt::LetTuple { line, .. } | Stmt::Assign { line, .. } | Stmt::For { line, .. } | Stmt::Match { line, .. } => {
                self.line = *line;
            }
            _ => {}
        }
        match s {
            Stmt::Let { name, ty, value, catch, .. } => {
                if let Some(t) = ty {
                    self.ty(t);
                }
                self.expr(value);
                self.catch(catch);
                let n = name.clone();
                self.bind(&n);
            }
            Stmt::LetTuple { names, value, .. } => {
                self.expr(value);
                for n in names.clone() {
                    self.bind(&n);
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
                self.scopes.push([var.clone()].into_iter().collect());
                if let Some(v) = var2 {
                    let v = v.clone();
                    self.bind(&v);
                }
                self.block(body);
                self.scopes.pop();
            }
            Stmt::Match { subject, cases, .. } => {
                self.expr(subject);
                for c in cases {
                    self.scopes.push(HashSet::new());
                    let line = c.line;
                    match &mut c.pattern {
                        Pattern::Variant(n, names) => {
                            self.quiet = true;
                            self.type_name(n, line, 0);
                            self.quiet = false;
                            for x in names.clone() {
                                self.bind(&x);
                            }
                        }
                        Pattern::Bind(n) => {
                            let n = n.clone();
                            self.bind(&n);
                        }
                        Pattern::Literal(e) => self.expr(e),
                        Pattern::Wildcard => {}
                    }
                    self.block(&mut c.body);
                    self.scopes.pop();
                }
            }
            Stmt::Return(Some(e), _, _) => self.expr(e),
            Stmt::Fn(f) => {
                let n = f.name.clone();
                self.bind(&n);
                self.func(Rc::make_mut(f), true);
            }
            Stmt::Arena { name, body, .. } => {
                self.scopes.push([name.clone()].into_iter().collect());
                self.block(body);
                self.scopes.pop();
            }
            Stmt::Unsafe { body, .. } => self.block(body),
            Stmt::Struct(_) | Stmt::Enum(_) | Stmt::Interface(_) => {
                let own = HashSet::new();
                self.top(s, &own, "");
            }
            _ => {}
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        let (l, _) = e.pos();
        if l > 0 {
            self.line = l;
        }
        // `a.greet` — 모듈 a 의 이름 하나로 바꿉니다.
        if let Expr::Field(o, x, l, c) = e {
            if let Expr::Ident(a, _, _) = &**o {
                if let Some(t) = self.qualified(&a.clone(), &x.clone(), *l, *c) {
                    let (l, c) = o.pos();
                    *e = Expr::Ident(t, l, c);
                    return;
                }
            }
        }
        match e {
            Expr::Ident(n, l, c) => {
                if !self.local(n) {
                    let (l, c) = (*l, *c);
                    if let Some(t) = self.lookup(&n.clone(), l, c) {
                        *n = t;
                    }
                }
            }
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
            Expr::Call { callee, targs, args, .. } => {
                self.expr(callee);
                for t in targs {
                    self.ty(t);
                }
                for a in args {
                    self.expr(&mut a.value);
                }
            }
            Expr::Field(o, _, _, _) => self.expr(o),
            Expr::IfExpr { cond, then, els } => {
                self.expr(cond);
                self.expr(then);
                self.expr(els);
            }
            Expr::Lambda(f, _, _) | Expr::Spawn(f, _, _) => self.func(Rc::make_mut(f), false),
            _ => {}
        }
    }
}
