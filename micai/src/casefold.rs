//! 대소문자 완화.
//!
//! 규칙은 하나입니다.
//!   **정확히 쓴 이름이 있으면 그게 이깁니다.**
//!   없을 때만, 대소문자만 다른 이름이 딱 하나 있으면 그걸로 고쳐 줍니다.
//!
//! 그래서 `struct User` 와 `let user` 가 같이 있어도 아무 문제가 없고
//! (둘 다 정확히 선언된 이름이라 서로 건드리지 않습니다),
//! `myvalue` 라고 잘못 쳐도 `myValue` 로 알아서 붙습니다.
//! 후보가 둘 이상이면 고치지 않고 평소대로 "찾을 수 없습니다" 오류를 냅니다.
//!
//! 고친 자리는 기록해서 `siskin check`가 정본 철자를 알려줍니다.
//! 파일은 한 가지 철자로 수렴하므로 검색도 AI도 헷갈리지 않습니다.

use crate::ast::*;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct CaseFix {
    pub line: usize,
    pub col: usize,
    pub typed: String,
    pub canonical: String,
}

const BUILTIN_NAMES: &[&str] = &[
    "Int", "Float", "Bool", "Str", "Byte", "Json", "print", "eprint", "len", "range", "str", "int", "float", "error",
    "assert", "abs", "min", "max", "sqrt", "floor", "ceil", "pow", "read_text", "write_text",
    "self", "result", "math", "fs", "io", "push", "pop", "reverse", "contains", "join", "split",
    "upper", "lower", "strip", "starts_with", "ends_with", "replace", "set", "has", "keys",
    "Unit", "sum", "sin", "cos", "tan", "log", "log10", "exp", "round", "pi", "e",
    "append_text", "exists", "remove", "time", "now", "clock", "random", "seed", "rand",
    "rand_int", "sort", "index_of", "slice", "clear", "find", "repeat", "alloc", "free", "arena",
    "get", "width", "pad_left", "pad_right", "sleep", "env", "set_env", "cwd", "set_cwd", "pid",
    "list_dir", "make_dir", "is_dir", "process", "net", "map", "filter", "any", "all", "sort_by",
];

pub fn fold(prog: &mut Program) -> Vec<CaseFix> {
    let mut declared: HashSet<String> = BUILTIN_NAMES.iter().map(|s| s.to_string()).collect();
    for s in &prog.stmts {
        collect_stmt(s, &mut declared);
    }
    let mut by_lower: HashMap<String, Vec<String>> = HashMap::new();
    for n in &declared {
        by_lower.entry(n.to_lowercase()).or_default().push(n.clone());
    }

    let mut cx = Ctx { declared, by_lower, fixes: Vec::new() };
    for s in &mut prog.stmts {
        cx.stmt(s);
    }
    cx.fixes
}

struct Ctx {
    declared: HashSet<String>,
    by_lower: HashMap<String, Vec<String>>,
    fixes: Vec<CaseFix>,
}

// ------------------------------------------------------------- 선언 이름 수집

fn collect_fn(f: &FnDecl, out: &mut HashSet<String>) {
    out.insert(f.name.clone());
    for p in &f.params {
        out.insert(p.name.clone());
    }
    for g in &f.generics {
        out.insert(g.clone());
    }
    for s in &f.body {
        collect_stmt(s, out);
    }
}

fn collect_stmt(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::CHeader { .. } => {}
        Stmt::Fn(f) => collect_fn(f, out),
        Stmt::Struct(sd) => {
            out.insert(sd.name.clone());
            for f in &sd.fields {
                out.insert(f.name.clone());
            }
            for m in &sd.methods {
                collect_fn(m, out);
            }
        }
        Stmt::Enum(ed) => {
            out.insert(ed.name.clone());
            for v in &ed.variants {
                out.insert(v.name.clone());
                for f in &v.fields {
                    out.insert(f.name.clone());
                }
            }
            for m in &ed.methods {
                collect_fn(m, out);
            }
        }
        Stmt::Interface(id) => {
            out.insert(id.name.clone());
            for m in &id.methods {
                out.insert(m.clone());
            }
        }
        Stmt::Import { names, path, .. } => {
            for n in names {
                out.insert(n.clone());
            }
            if let Some(last) = path.last() {
                out.insert(last.clone());
            }
        }
        Stmt::Let { name, catch, .. } => {
            out.insert(name.clone());
            if let Some(c) = catch {
                out.insert(c.name.clone());
                for s in &c.body {
                    collect_stmt(s, out);
                }
            }
        }
        Stmt::LetTuple { names, .. } => {
            for n in names {
                out.insert(n.clone());
            }
        }
        Stmt::Assign { catch, .. } | Stmt::Expr(_, catch) => {
            if let Some(c) = catch {
                out.insert(c.name.clone());
                for s in &c.body {
                    collect_stmt(s, out);
                }
            }
        }
        Stmt::If { arms, els } => {
            for (_, b) in arms {
                for s in b {
                    collect_stmt(s, out);
                }
            }
            if let Some(b) = els {
                for s in b {
                    collect_stmt(s, out);
                }
            }
        }
        Stmt::While { body, .. } => {
            for s in body {
                collect_stmt(s, out);
            }
        }
        Stmt::For { var, var2, body, .. } => {
            out.insert(var.clone());
            if let Some(v2) = var2 {
                out.insert(v2.clone());
            }
            for s in body {
                collect_stmt(s, out);
            }
        }
        Stmt::Match { cases, .. } => {
            for c in cases {
                match &c.pattern {
                    Pattern::Variant(_, binds) => {
                        for b in binds {
                            out.insert(b.clone());
                        }
                    }
                    Pattern::Bind(n) => {
                        out.insert(n.clone());
                    }
                    _ => {}
                }
                for s in &c.body {
                    collect_stmt(s, out);
                }
            }
        }
        Stmt::Link(_, _) => {}
        Stmt::Arena { name, body, .. } => {
            out.insert(name.clone());
            for s in body {
                collect_stmt(s, out);
            }
        }
        Stmt::Unsafe { body, .. } => {
            for s in body {
                collect_stmt(s, out);
            }
        }
        Stmt::Return(_, _, _) | Stmt::Break(_, _) | Stmt::Continue(_, _) => {}
    }
}

// ------------------------------------------------------------------- 고치기

impl Ctx {
    /// 정확한 이름이 있으면 그대로. 없고 후보가 딱 하나면 고칩니다.
    fn fix(&mut self, name: &mut String, line: usize, col: usize) {
        if self.declared.contains(name.as_str()) {
            return;
        }
        let lower = name.to_lowercase();
        let canonical = match self.by_lower.get(&lower) {
            Some(v) if v.len() == 1 => v[0].clone(),
            _ => return,
        };
        self.fixes.push(CaseFix {
            line,
            col,
            typed: name.clone(),
            canonical: canonical.clone(),
        });
        *name = canonical;
    }

    fn ty(&mut self, t: &mut TypeExpr, line: usize) {
        match t {
            TypeExpr::Named(n, args) => {
                // 타입 자리에서는 타입(대문자로 시작하는 이름)으로만 맞춥니다.
                // 예전에는 `Result` 가 키워드 `result` 로 바뀌어 오류 메시지가 헷갈렸습니다.
                let orig = n.clone();
                let before = self.fixes.len();
                self.fix(n, line, 1);
                let upper = |s: &str| s.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
                if upper(&orig) && !upper(n) {
                    *n = orig;
                    self.fixes.truncate(before);
                }
                for a in args {
                    self.ty(a, line);
                }
            }
            TypeExpr::Optional(i) | TypeExpr::Fallible(i, None) | TypeExpr::List(i) | TypeExpr::Raw(i) => {
                self.ty(i, line)
            }
            TypeExpr::Fallible(i, Some(e)) => {
                self.ty(i, line);
                self.ty(e, line);
            }
            TypeExpr::Dict(k, v) => {
                self.ty(k, line);
                self.ty(v, line);
            }
            TypeExpr::Tuple(ts) => {
                for t in ts {
                    self.ty(t, line);
                }
            }
            TypeExpr::Fn(ps, r) => {
                for p in ps {
                    self.ty(p, line);
                }
                self.ty(r, line);
            }
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        match e {
            Expr::Ident(n, l, c) => {
                let (l, c) = (*l, *c);
                self.fix(n, l, c);
            }
            Expr::FString(parts) => {
                for p in parts {
                    if let FStrPart::Expr(inner, _) = p {
                        self.expr(inner);
                    }
                }
            }
            Expr::List(items) | Expr::Tuple(items) => {
                for i in items {
                    self.expr(i);
                }
            }
            Expr::Dict(pairs) => {
                for (k, v) in pairs {
                    self.expr(k);
                    self.expr(v);
                }
            }
            Expr::Unary(_, i, _, _) => self.expr(i),
            Expr::Binary(_, a, b, _, _) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Call { callee, targs, args, .. } => {
                self.expr(callee);
                for t in targs {
                    self.ty(t, 0);
                }
                for a in args {
                    let (l, c) = a.value.pos();
                    if let Some(n) = &mut a.name {
                        self.fix(n, l, c);
                    }
                    self.expr(&mut a.value);
                }
            }
            Expr::Field(o, n, l, c) => {
                let (l, c) = (*l, *c);
                self.expr(o);
                self.fix(n, l, c);
            }
            Expr::Index(o, i, _, _) => {
                self.expr(o);
                self.expr(i);
            }
            Expr::IfExpr { cond, then, els } => {
                self.expr(cond);
                self.expr(then);
                self.expr(els);
            }
            Expr::Try(i, _, _) => self.expr(i),
            Expr::OrElse(a, b, _, _) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Lambda(f, _, _) | Expr::Spawn(f, _, _) => {
                // 익명 함수의 인자 이름도 선언된 이름입니다.
                for p in &f.params {
                    if self.declared.insert(p.name.clone()) {
                        self.by_lower.entry(p.name.to_lowercase()).or_default().push(p.name.clone());
                    }
                }
                if let Some(fd) = crate::ast::Shared::get_mut(f) {
                    self.func(fd);
                }
            }
            _ => {}
        }
    }

    fn func(&mut self, f: &mut FnDecl) {
        let line = f.line;
        for p in &mut f.params {
            if let Some(t) = &mut p.ty {
                self.ty(t, line);
            }
        }
        if let Some(t) = &mut f.ret {
            self.ty(t, line);
        }
        for r in &mut f.requires {
            self.expr(r);
        }
        for en in &mut f.ensures {
            self.expr(en);
        }
        for s in &mut f.body {
            self.stmt(s);
        }
    }

    fn catch(&mut self, c: &mut Option<CatchClause>) {
        if let Some(c) = c {
            for s in &mut c.body {
                self.stmt(s);
            }
        }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::CHeader { .. } => {}
            Stmt::Fn(f) => {
                if let Some(fd) = crate::ast::Shared::get_mut(f) {
                    self.func(fd);
                }
            }
            Stmt::Struct(sd) => {
                if let Some(d) = crate::ast::Shared::get_mut(sd) {
                    let line = d.line;
                    for f in &mut d.fields {
                        if let Some(t) = &mut f.ty {
                            self.ty(t, line);
                        }
                        if let Some(dv) = &mut f.default {
                            self.expr(dv);
                        }
                    }
                    for m in &mut d.methods {
                        if let Some(md) = crate::ast::Shared::get_mut(m) {
                            self.func(md);
                        }
                    }
                    for i in &mut d.interfaces {
                        self.fix(i, line, 1);
                    }
                }
            }
            Stmt::Enum(ed) => {
                if let Some(d) = crate::ast::Shared::get_mut(ed) {
                    let line = d.line;
                    for v in &mut d.variants {
                        for f in &mut v.fields {
                            if let Some(t) = &mut f.ty {
                                self.ty(t, line);
                            }
                        }
                    }
                    for m in &mut d.methods {
                        if let Some(md) = crate::ast::Shared::get_mut(m) {
                            self.func(md);
                        }
                    }
                }
            }
            Stmt::Interface(_) | Stmt::Import { .. } => {}
            Stmt::Let { ty, value, catch, line, .. } => {
                let line = *line;
                if let Some(t) = ty {
                    self.ty(t, line);
                }
                self.expr(value);
                self.catch(catch);
            }
            Stmt::LetTuple { value, .. } => {
                self.expr(value);
            }
            Stmt::Assign { target, value, catch, .. } => {
                self.expr(target);
                self.expr(value);
                self.catch(catch);
            }
            Stmt::Expr(e, catch) => {
                self.expr(e);
                self.catch(catch);
            }
            Stmt::If { arms, els } => {
                for (c, b) in arms {
                    self.expr(c);
                    for s in b {
                        self.stmt(s);
                    }
                }
                if let Some(b) = els {
                    for s in b {
                        self.stmt(s);
                    }
                }
            }
            Stmt::While { cond, body } => {
                self.expr(cond);
                for s in body {
                    self.stmt(s);
                }
            }
            Stmt::For { iter, body, .. } => {
                self.expr(iter);
                for s in body {
                    self.stmt(s);
                }
            }
            Stmt::Match { subject, cases, .. } => {
                self.expr(subject);
                for c in cases {
                    let line = c.line;
                    if let Pattern::Variant(n, _) = &mut c.pattern {
                        self.fix(n, line, 1);
                    }
                    for s in &mut c.body {
                        self.stmt(s);
                    }
                }
            }
            Stmt::Return(v, _, _) => {
                if let Some(e) = v {
                    self.expr(e);
                }
            }
            Stmt::Link(_, _) => {}
            Stmt::Arena { body, .. } | Stmt::Unsafe { body, .. } => {
                for s in body {
                    self.stmt(s);
                }
            }
            Stmt::Break(_, _) | Stmt::Continue(_, _) => {}
        }
    }
}
