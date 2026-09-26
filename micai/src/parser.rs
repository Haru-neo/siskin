use crate::ast::*;
use crate::error::SiskinError;
use crate::lexer::{tokenize, FPart, Tok, Token};
use crate::ast::Shared as Rc;

pub fn parse(src: &str) -> Result<Program, SiskinError> {
    let toks = tokenize(src)?;
    let mut p = Parser { toks, pos: 0 };
    p.program()
}

/// When reading another file: line numbers are shifted by `base` (`error::register_file`).
pub fn parse_at(src: &str, base: usize) -> Result<Program, SiskinError> {
    let toks = crate::lexer::tokenize_at(src, base)?;
    let mut p = Parser { toks, pos: 0 };
    p.program()
}

/// Parses a `{...}` piece inside an f-string on its own.
pub fn parse_expr_str(src: &str) -> Result<Expr, SiskinError> {
    let toks = tokenize(src)?;
    let mut p = Parser { toks, pos: 0 };
    let e = p.expr()?;
    Ok(e)
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

/// A `{...}` inside an f-string is parsed as separate source, so its line/column
/// info is relative to the piece (1:1). It must be shifted back to where the
/// original f-string was, or error messages would point at the wrong line.
fn retag(e: &mut Expr, line: usize, col: usize) {
    match e {
        Expr::Ident(_, l, c) => {
            *l = line;
            *c = col;
        }
        Expr::Unary(_, inner, l, c) => {
            *l = line;
            *c = col;
            retag(inner, line, col);
        }
        Expr::Binary(_, a, b, l, c) => {
            *l = line;
            *c = col;
            retag(a, line, col);
            retag(b, line, col);
        }
        Expr::Call { callee, args, line: l, col: c, .. } => {
            *l = line;
            *c = col;
            retag(callee, line, col);
            for a in args {
                retag(&mut a.value, line, col);
            }
        }
        Expr::Field(o, _, l, c) => {
            *l = line;
            *c = col;
            retag(o, line, col);
        }
        Expr::Index(o, i, l, c) => {
            *l = line;
            *c = col;
            retag(o, line, col);
            retag(i, line, col);
        }
        Expr::Try(inner, l, c) => {
            *l = line;
            *c = col;
            retag(inner, line, col);
        }
        Expr::IfExpr { cond, then, els } => {
            retag(cond, line, col);
            retag(then, line, col);
            retag(els, line, col);
        }
        Expr::List(items) => {
            for i in items {
                retag(i, line, col);
            }
        }
        Expr::Dict(pairs) => {
            for (k, v) in pairs {
                retag(k, line, col);
                retag(v, line, col);
            }
        }
        Expr::FString(parts) => {
            for p in parts {
                if let FStrPart::Expr(inner, _) = p {
                    retag(inner, line, col);
                }
            }
        }
        _ => {}
    }
}

impl Parser {
    fn cur(&self) -> &Token {
        &self.toks[self.pos.min(self.toks.len() - 1)]
    }

    fn tok(&self) -> &Tok {
        &self.cur().tok
    }

    fn line(&self) -> usize {
        self.cur().line
    }

    fn col(&self) -> usize {
        self.cur().col
    }

    fn bump(&mut self) -> Tok {
        let t = self.cur().tok.clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at(&self, t: &Tok) -> bool {
        self.tok() == t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.at(t) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn at_kw(&self, k: &str) -> bool {
        matches!(self.tok(), Tok::Kw(s) if s == k)
    }

    fn eat_kw(&mut self, k: &str) -> bool {
        if self.at_kw(k) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn at_ident(&self, name: &str) -> bool {
        matches!(self.tok(), Tok::Ident(s) if s == name)
    }

    fn peek_tok(&self, ahead: usize) -> Option<&Tok> {
        self.toks.get(self.pos + ahead).map(|t| &t.tok)
    }

    fn peek_kw_at(&self, ahead: usize, k: &str) -> bool {
        matches!(self.toks.get(self.pos + ahead).map(|t| &t.tok), Some(Tok::Kw(x)) if x == k)
    }

    fn err(&self, code: &'static str, msg: impl Into<String>) -> SiskinError {
        SiskinError::new(code, msg, self.line(), self.col())
    }

    fn expect(&mut self, t: Tok, code: &'static str, what: &str) -> Result<(), SiskinError> {
        if self.at(&t) {
            self.bump();
            Ok(())
        } else {
            let fix = self.foreign_hint().unwrap_or_else(|| tr!(format!("여기에 {}을(를) 넣으세요", what), format!("insert {} here", what)));
            Err(self
                .err(code, tr!(format!("{}을(를) 기대했는데 {}이(가) 왔습니다", what, self.tok()), format!("expected {}, found {}", what, self.tok())))
                .with_fix(fix))
        }
    }

    /// If syntax from another language shows up here, suggest the Siskin alternative.
    fn foreign_hint(&self) -> Option<String> {
        let prev = if self.pos > 0 { self.toks.get(self.pos - 1).map(|t| &t.tok) } else { None };
        match self.tok() {
            Tok::Question => Some(tr!("없을 수도 있는 타입은 앞에 `?` 를 붙입니다: `?Int` (`Int?` 가 아님)", "optional types put `?` in front: `?Int` (not `Int?`)").into()),
            Tok::Kw(k) if k == "as" => Some(tr!("타입 바꾸기는 함수로 씁니다: `float(x)` `int(x)` `str(x)`", "convert types with functions: `float(x)` `int(x)` `str(x)`").into()),
            Tok::Ident(k) if k == "is" => Some(tr!("`x is None` 대신 `x == none`, `x is not None` 대신 `x != none`", "write `x == none` instead of `x is None`, and `x != none` instead of `x is not None`").into()),
            Tok::Assign if matches!(prev, Some(Tok::Ident(_))) => {
                Some(tr!("이름 붙인 인자는 `=` 가 아니라 `:` 로 줍니다: `P(a: 1)`", "named arguments use `:`, not `=`: `P(a: 1)`").into())
            }
            _ => None,
        }
    }

    fn expect_kw(&mut self, k: &str, code: &'static str) -> Result<(), SiskinError> {
        if self.eat_kw(k) {
            Ok(())
        } else {
            Err(self.err(code, tr!(format!("`{}`을(를) 기대했는데 {}이(가) 왔습니다", k, self.tok()), format!("expected `{}`, found {}", k, self.tok()))))
        }
    }

    fn expect_ident(&mut self, code: &'static str, what: &str) -> Result<String, SiskinError> {
        match self.tok().clone() {
            Tok::Ident(s) => {
                self.bump();
                Ok(s)
            }
            Tok::Kw(k) => Err(self
                .err(code, tr!(format!("{}이(가) 필요한데 키워드 `{}`가 왔습니다", what, k), format!("expected {}, found keyword `{}`", what, k)))
                .with_fix(tr!("키워드는 이름으로 쓸 수 없습니다. 다른 이름을 고르세요", "keywords cannot be used as names; pick a different name"))),
            other => Err(self.err(code, tr!(format!("{}이(가) 필요한데 {}이(가) 왔습니다", what, other), format!("expected {}, found {}", what, other)))),
        }
    }

    fn skip_newlines(&mut self) {
        while self.at(&Tok::Newline) {
            self.bump();
        }
    }

    // ---------------------------------------------------------------- program

    fn program(&mut self) -> Result<Program, SiskinError> {
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            if self.at(&Tok::Eof) {
                break;
            }
            stmts.push(self.stmt()?);
        }
        Ok(Program { stmts })
    }

    /// `: NEWLINE INDENT stmt+ DEDENT`
    fn block(&mut self) -> Result<Vec<Stmt>, SiskinError> {
        self.expect(Tok::Colon, "E0100", "`:`")?;
        self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
        self.expect(Tok::Indent, "E0102", tr!("들여쓴 블록", "indented block"))
            .map_err(|e| e.with_fix(tr!("다음 줄을 공백 4칸 더 들여쓰세요", "indent the next line by 4 more spaces")))?;
        let mut out = Vec::new();
        loop {
            self.skip_newlines();
            if self.at(&Tok::Dedent) || self.at(&Tok::Eof) {
                break;
            }
            out.push(self.stmt()?);
        }
        self.eat(&Tok::Dedent);
        if out.is_empty() {
            return Err(self.err("E0103", tr!("블록이 비어 있습니다", "empty block")));
        }
        Ok(out)
    }

    // ------------------------------------------------------------------ statements

    fn stmt(&mut self) -> Result<Stmt, SiskinError> {
        // `pub` is only parsed and ignored in P1 (the module system comes in P5).
        self.eat_kw("pub");

        if self.at_kw("extern") {
            return self.extern_decl();
        }
        // `pass` — a do-nothing placeholder (same as Python).
        if matches!(self.tok(), Tok::Ident(s) if s == "pass")
            && matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Newline) | Some(Tok::Dedent) | None)
        {
            self.bump();
            self.eat(&Tok::Newline);
            return Ok(Stmt::Expr(Expr::Bool(true), None));
        }
        if self.at_kw("fn") {
            return Ok(Stmt::Fn(Rc::new(self.fn_decl()?)));
        }
        if self.at_kw("struct") {
            return Ok(Stmt::Struct(Rc::new(self.struct_decl()?)));
        }
        if self.at_kw("enum") {
            return Ok(Stmt::Enum(Rc::new(self.enum_decl()?)));
        }
        if self.at_kw("interface") {
            return Ok(Stmt::Interface(Rc::new(self.interface_decl()?)));
        }
        if self.at_kw("let") || self.at_kw("var") {
            return self.let_stmt();
        }
        if self.at_kw("if") {
            return self.if_stmt();
        }
        if self.at_kw("while") {
            return self.while_stmt();
        }
        if self.at_kw("for") {
            return self.for_stmt();
        }
        if self.at_kw("match") {
            return self.match_stmt();
        }
        if self.at_kw("import") || self.at_kw("from") {
            return self.import_stmt();
        }
        if self.at_kw("with") {
            let line = self.line();
            self.bump();
            if !self.at_ident("arena") {
                return Err(self
                    .err("E0104", tr!("`with` 다음에는 `arena`가 와야 합니다", "expected `arena` after `with`"))
                    .with_fix(tr!("`with arena 이름:` 형태로 씁니다. 구조적 동시성(`with nursery`)은 아직입니다", "write `with arena name:`; structured concurrency (`with nursery`) is not supported yet")));
            }
            self.bump();
            let name = self.expect_ident("E0142", tr!("아레나 이름", "arena name"))?;
            let body = self.block()?;
            return Ok(Stmt::Arena { name, body, line });
        }
        if self.at_kw("unsafe") {
            let line = self.line();
            self.bump();
            let body = self.block()?;
            return Ok(Stmt::Unsafe { body, line });
        }
        if self.at_kw("return") {
            let (l, c) = (self.line(), self.col());
            self.bump();
            let v = if self.at(&Tok::Newline) { None } else { Some(self.expr()?) };
            self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
            return Ok(Stmt::Return(v, l, c));
        }
        if self.at_kw("break") {
            let (l, c) = (self.line(), self.col());
            self.bump();
            self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
            return Ok(Stmt::Break(l, c));
        }
        if self.at_kw("continue") {
            let (l, c) = (self.line(), self.col());
            self.bump();
            self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
            return Ok(Stmt::Continue(l, c));
        }

        // Expression statement or assignment
        let (l, c) = (self.line(), self.col());
        let e = self.expr()?;
        let op = match self.tok() {
            Tok::Assign => None,
            Tok::PlusEq => Some(BinOp::Add),
            Tok::MinusEq => Some(BinOp::Sub),
            Tok::StarEq => Some(BinOp::Mul),
            Tok::SlashEq => Some(BinOp::Div),
            Tok::PercentEq => Some(BinOp::Mod),
            _ => {
                let catch = self.opt_catch()?;
                if catch.is_none() {
                    self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
                }
                return Ok(Stmt::Expr(e, catch));
            }
        };
        if !matches!(e, Expr::Ident(..) | Expr::Field(..) | Expr::Index(..)) {
            return Err(SiskinError::new("E0106", tr!("대입할 수 없는 대상입니다", "invalid assignment target"), l, c)
                .with_fix(tr!("대입의 왼쪽은 변수, 필드, 인덱스여야 합니다", "the left side of an assignment must be a variable, field, or index")));
        }
        self.bump();
        let value = self.expr()?;
        let catch = self.opt_catch()?;
        if catch.is_none() {
            self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
        }
        Ok(Stmt::Assign { target: e, op, value, catch, line: l, col: c })
    }

    /// `catch NAME: BLOCK` (optional)
    fn opt_catch(&mut self) -> Result<Option<CatchClause>, SiskinError> {
        if !self.at_kw("catch") {
            return Ok(None);
        }
        self.bump();
        let name = self.expect_ident("E0107", tr!("에러를 받을 이름", "name for the error"))?;
        let body = self.block()?;
        Ok(Some(CatchClause { name, body }))
    }

    fn let_stmt(&mut self) -> Result<Stmt, SiskinError> {
        let (l, c) = (self.line(), self.col());
        let mutable = self.at_kw("var");
        self.bump();
        // Tuple destructuring: `let (a, b) = tuple_expr`
        if self.at(&Tok::LParen) {
            self.bump();
            let mut names = Vec::new();
            loop {
                names.push(self.expect_ident("E0108", tr!("변수 이름", "variable name"))?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
                if self.at(&Tok::RParen) {
                    break;
                }
            }
            self.expect(Tok::RParen, "E0116", "`)`")?;
            self.expect(Tok::Assign, "E0109", "`=`")
                .map_err(|e| e.with_fix(tr!("`let (a, b) = 튜플` 처럼 씁니다", "write `let (a, b) = tuple`")))?;
            let value = self.expr()?;
            self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
            return Ok(Stmt::LetTuple { names, value, line: l, col: c });
        }
        let name = self.expect_ident("E0108", tr!("변수 이름", "variable name"))?;
        let ty = if self.eat(&Tok::Colon) { Some(self.type_expr()?) } else { None };
        self.expect(Tok::Assign, "E0109", "`=`")
            .map_err(|e| e.with_fix(tr!("Siskin에는 초기화 없는 선언이 없습니다. `= 값`을 붙이세요", "Siskin has no declarations without an initializer; add `= value`")))?;
        let value = self.expr()?;
        let catch = self.opt_catch()?;
        if catch.is_none() {
            self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
        }
        Ok(Stmt::Let { name, ty, value, mutable, catch, line: l, col: c })
    }

    fn if_stmt(&mut self) -> Result<Stmt, SiskinError> {
        self.expect_kw("if", "E0110")?;
        let mut arms = Vec::new();
        let cond = self.expr()?;
        let body = self.block()?;
        arms.push((cond, body));
        let mut els = None;
        loop {
            let save = self.pos;
            self.skip_newlines();
            if self.at_kw("elif") {
                self.bump();
                let c = self.expr()?;
                let b = self.block()?;
                arms.push((c, b));
                continue;
            }
            if self.at_kw("else") {
                self.bump();
                els = Some(self.block()?);
                break;
            }
            self.pos = save;
            break;
        }
        Ok(Stmt::If { arms, els })
    }

    fn while_stmt(&mut self) -> Result<Stmt, SiskinError> {
        self.expect_kw("while", "E0110")?;
        let cond = self.expr()?;
        let body = self.block()?;
        Ok(Stmt::While { cond, body })
    }

    fn for_stmt(&mut self) -> Result<Stmt, SiskinError> {
        let line = self.line();
        self.expect_kw("for", "E0110")?;
        // `for (a, b) in tuples:` — unpacks the tuple on each iteration.
        // Same as binding a hidden variable and putting `let (a, b) = hidden` at the top of the block.
        if self.at(&Tok::LParen) {
            let (l, c) = (self.line(), self.col());
            self.bump();
            let mut names = vec![self.expect_ident("E0111", tr!("반복 변수 이름", "loop variable name"))?];
            while self.eat(&Tok::Comma) {
                names.push(self.expect_ident("E0111", tr!("반복 변수 이름", "loop variable name"))?);
            }
            self.expect(Tok::RParen, "E0125", "`)`")?;
            self.expect_kw("in", "E0112")
                .map_err(|e| e.with_fix(tr!("Siskin의 for는 `for x in xs:` 또는 `for (a, b) in 튜플들:` 형태입니다", "Siskin's for loop is `for x in xs:` or `for (a, b) in tuples:`")))?;
            let iter = self.expr()?;
            let mut body = self.block()?;
            let var = format!("_for_{}_{}", l, c);
            body.insert(0, Stmt::LetTuple { names, value: Expr::Ident(var.clone(), l, c), line: l, col: c });
            return Ok(Stmt::For { var, var2: None, iter, body, line });
        }
        let var = self.expect_ident("E0111", tr!("반복 변수 이름", "loop variable name"))?;
        // `for k, v in d:` — iterates a dict by key and value together.
        let var2 = if self.eat(&Tok::Comma) {
            Some(self.expect_ident("E0111", tr!("둘째 반복 변수 이름", "second loop variable name"))?)
        } else {
            None
        };
        self.expect_kw("in", "E0112")
            .map_err(|e| e.with_fix(tr!("Siskin의 for는 `for x in xs:` 또는 `for k, v in d:` 형태입니다", "Siskin's for loop is `for x in xs:` or `for k, v in d:`")))?;
        let iter = self.expr()?;
        let body = self.block()?;
        Ok(Stmt::For { var, var2, iter, body, line })
    }

    fn match_stmt(&mut self) -> Result<Stmt, SiskinError> {
        let line = self.line();
        self.expect_kw("match", "E0110")?;
        let subject = self.expr()?;
        self.expect(Tok::Colon, "E0100", "`:`")?;
        self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
        self.expect(Tok::Indent, "E0102", tr!("들여쓴 블록", "indented block"))?;
        let mut cases = Vec::new();
        loop {
            self.skip_newlines();
            if self.at(&Tok::Dedent) || self.at(&Tok::Eof) {
                break;
            }
            let cline = self.line();
            self.expect_kw("case", "E0113")
                .map_err(|e| e.with_fix(tr!("match 블록 안에는 `case` 만 올 수 있습니다", "only `case` can appear inside a match block")))?;
            let pattern = self.pattern()?;
            let body = self.block()?;
            cases.push(MatchCase { pattern, body, line: cline });
        }
        self.eat(&Tok::Dedent);
        if cases.is_empty() {
            return Err(self.err("E0114", tr!("match에 case가 하나도 없습니다", "match has no cases")));
        }
        Ok(Stmt::Match { subject, cases, line })
    }

    fn pattern(&mut self) -> Result<Pattern, SiskinError> {
        match self.tok().clone() {
            Tok::Ident(name) => {
                self.bump();
                if name == "_" {
                    return Ok(Pattern::Wildcard);
                }
                // `case shapes.Circle(r):` — variants from an imported module may be prefixed with the module name.
                // Module names start lowercase; enum names start uppercase.
                let module_q = name.chars().next().map_or(false, |c| c.is_lowercase())
                    && matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Ident(_)));
                let name = if self.at(&Tok::Dot) && module_q {
                    self.bump();
                    let v = self.expect_ident("E0115", tr!("변형 이름", "variant name"))?;
                    format!("{}.{}", name, v)
                } else {
                    name
                };
                // `case Color.Red:` — variants are written without the enum name.
                if self.at(&Tok::Dot) {
                    self.bump();
                    let v = match self.tok() {
                        Tok::Ident(v) => v.clone(),
                        _ => tr!("변형", "Variant").to_string(),
                    };
                    return Err(self
                        .err("E0100", tr!(format!("`case {}.{}` 처럼 enum 이름을 붙이지 않습니다", name, v), format!("do not qualify the variant with the enum name as in `case {}.{}`", name, v)))
                        .with_fix(tr!(format!("`case {}:` 처럼 변형 이름만 쓰세요", v), format!("write just the variant name: `case {}:`", v))));
                }
                if self.eat(&Tok::LParen) {
                    let mut binds = Vec::new();
                    if !self.at(&Tok::RParen) {
                        loop {
                            binds.push(self.expect_ident("E0115", tr!("바인딩 이름", "binding name"))?);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(Tok::RParen, "E0116", "`)`")?;
                    return Ok(Pattern::Variant(name, binds));
                }
                // Uppercase first letter means a variant name; otherwise a binding.
                if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    Ok(Pattern::Variant(name, Vec::new()))
                } else {
                    Ok(Pattern::Bind(name))
                }
            }
            Tok::Int(_) | Tok::Str(_) | Tok::Float(_) => {
                let e = self.primary()?;
                Ok(Pattern::Literal(e))
            }
            Tok::Kw(k) if k == "true" || k == "false" => {
                self.bump();
                Ok(Pattern::Literal(Expr::Bool(k == "true")))
            }
            Tok::Kw(k) if k == "none" => {
                self.bump();
                Ok(Pattern::Literal(Expr::NoneLit))
            }
            other => Err(self.err("E0117", tr!(format!("패턴을 기대했는데 {}이(가) 왔습니다", other), format!("expected a pattern, found {}", other)))),
        }
    }

    /// `import c "zlib.h" link "z"` / `import cpp "lib.hpp" link "mylib" from "/inc"`
    /// Reads a header file and imports all functions in it.
    fn cheader_stmt(&mut self, line: usize, col: usize) -> Result<Stmt, SiskinError> {
        let cpp = !self.at_ident("c");
        self.bump(); // c / cpp / cxx
        let header = match self.tok().clone() {
            Tok::Str(h) => {
                self.bump();
                h
            }
            _ => {
                return Err(self
                    .err("E0153", tr!("가져올 헤더 파일 이름이 필요합니다", "expected a header file name to import"))
                    .with_fix(tr!("`import c \"zlib.h\" link \"z\"` 처럼 따옴표로 적습니다", "write it in quotes: `import c \"zlib.h\" link \"z\"`")))
            }
        };
        let mut links = Vec::new();
        let mut incdirs = Vec::new();
        let mut only = Vec::new();
        loop {
            if self.at_ident("link") {
                self.bump();
                loop {
                    match self.tok().clone() {
                        Tok::Str(s) => {
                            self.bump();
                            links.push(s);
                        }
                        _ => {
                            return Err(self
                                .err("E0152", tr!("링크할 라이브러리 이름이 필요합니다", "expected a library name to link"))
                                .with_fix(tr!("`link \"z\"` 처럼 따옴표로 적습니다", "write it in quotes: `link \"z\"`")))
                        }
                    }
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                continue;
            }
            if self.at_ident("from") || self.at_kw("from") {
                self.bump();
                match self.tok().clone() {
                    Tok::Str(s) => {
                        self.bump();
                        incdirs.push(s);
                    }
                    _ => {
                        return Err(self
                            .err("E0154", tr!("헤더를 찾을 폴더 이름이 필요합니다", "expected a header search directory"))
                            .with_fix(tr!("`from \"/usr/local/include\"` 처럼 따옴표로 적습니다", "write it in quotes: `from \"/usr/local/include\"`")))
                    }
                }
                continue;
            }
            // `also "shapes.cpp"` — also compile this source file.
            if self.at_ident("also") {
                self.bump();
                loop {
                    match self.tok().clone() {
                        Tok::Str(f) => {
                            self.bump();
                            // Sent along in the library list. A leading `:src:` means it is
                            // a file to compile together, not a name to link.
                            links.push(format!(":src:{}", f));
                        }
                        _ => {
                            return Err(self
                                .err("E0157", tr!("같이 컴파일할 소스 파일 이름이 필요합니다", "expected a source file name to compile alongside"))
                                .with_fix(tr!("`also \"shapes.cpp\"` 처럼 따옴표로 적습니다", "write it in quotes: `also \"shapes.cpp\"`")))
                        }
                    }
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                continue;
            }
            // `only name, name` — import just these.
            if self.at_ident("only") {
                self.bump();
                loop {
                    only.push(self.expect_ident("E0122", tr!("가져올 이름", "name to import"))?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                continue;
            }
            break;
        }
        self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
        Ok(Stmt::CHeader { header, cpp, links, incdirs, only, line, col })
    }

    fn import_stmt(&mut self) -> Result<Stmt, SiskinError> {
        let (l, c) = (self.line(), self.col());
        if self.eat_kw("import") {
            // `import c "zlib.h" link "z"` — imports an entire C/C++ header.
            if (self.at_ident("c") || self.at_ident("cpp") || self.at_ident("cxx"))
                && matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Str(_)))
            {
                return self.cheader_stmt(l, c);
            }
            let mut path = vec![self.expect_ident("E0118", tr!("모듈 이름", "module name"))?];
            while self.eat(&Tok::Dot) {
                path.push(self.expect_ident("E0118", tr!("모듈 이름", "module name"))?);
            }
            // `import colors.extra as ex` — import under a different name.
            let alias = if self.eat_kw("as") {
                Some(self.expect_ident("E0122", tr!("새 이름", "new name"))?)
            } else {
                None
            };
            self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
            return Ok(Stmt::Import { path, names: Vec::new(), renames: Vec::new(), alias, line: l, col: c });
        }
        self.expect_kw("from", "E0119")?;
        let mut path = vec![self.expect_ident("E0118", tr!("모듈 이름", "module name"))?];
        while self.eat(&Tok::Dot) {
            path.push(self.expect_ident("E0118", tr!("모듈 이름", "module name"))?);
        }
        self.expect_kw("import", "E0120")?;
        if self.at(&Tok::Star) {
            return Err(self
                .err("E0121", tr!("와일드카드 임포트는 없습니다", "wildcard imports are not supported"))
                .with_fix(tr!("필요한 이름을 하나씩 적으세요. 어디서 온 이름인지 항상 보이게 하기 위해서입니다", "list the names you need one by one, so it is always clear where a name comes from")));
        }
        let mut names = Vec::new();
        let mut renames = Vec::new();
        loop {
            let n = self.expect_ident("E0122", tr!("가져올 이름", "name to import"))?;
            // `from a import greet as a_greet`
            if self.eat_kw("as") {
                renames.push(self.expect_ident("E0122", tr!("새 이름", "new name"))?);
            } else {
                renames.push(n.clone());
            }
            names.push(n);
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
        Ok(Stmt::Import { path, names, renames, alias: None, line: l, col: c })
    }

    // ------------------------------------------------------------------ declarations

    fn fn_decl(&mut self) -> Result<FnDecl, SiskinError> {
        let line = self.line();
        self.expect_kw("fn", "E0110")?;
        let name = self.expect_ident("E0123", tr!("함수 이름", "function name"))?;
        let generics = self.opt_generics()?;
        let params = self.params()?;
        let ret = if self.eat(&Tok::Arrow) { Some(self.type_expr()?) } else { None };

        self.expect(Tok::Colon, "E0100", "`:`")?;
        self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
        self.expect(Tok::Indent, "E0102", tr!("들여쓴 함수 본문", "indented function body"))?;

        let mut doc = None;
        self.skip_newlines();
        if let Tok::Str(s) = self.tok().clone() {
            if self.toks.get(self.pos + 1).map(|t| &t.tok) == Some(&Tok::Newline) {
                self.bump();
                self.bump();
                doc = Some(s);
            }
        }

        // `requires` / `ensures` are contextual keywords (design doc §9.5).
        let mut requires = Vec::new();
        let mut ensures = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_ident("requires") {
                self.bump();
                requires.push(self.expr()?);
                self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
                continue;
            }
            if self.at_ident("ensures") {
                self.bump();
                ensures.push(self.expr()?);
                self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
                continue;
            }
            break;
        }

        let mut body = Vec::new();
        loop {
            self.skip_newlines();
            if self.at(&Tok::Dedent) || self.at(&Tok::Eof) {
                break;
            }
            body.push(self.stmt()?);
        }
        self.eat(&Tok::Dedent);

        Ok(FnDecl { name, generics, params, ret, doc, requires, ensures, body, line, is_extern: false, c_sig: None })
    }

    /// A body-less signature inside an interface.
    fn fn_sig(&mut self) -> Result<FnDecl, SiskinError> {
        let line = self.line();
        self.expect_kw("fn", "E0110")?;
        let name = self.expect_ident("E0123", tr!("함수 이름", "function name"))?;
        let generics = self.opt_generics()?;
        let params = self.params()?;
        let ret = if self.eat(&Tok::Arrow) { Some(self.type_expr()?) } else { None };
        self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
        Ok(FnDecl {
            name,
            generics,
            params,
            ret,
            doc: None,
            requires: Vec::new(),
            ensures: Vec::new(),
            body: Vec::new(),
            line,
            is_extern: false,
            c_sig: None,
        })
    }

    /// `extern "C" fn strlen(s: Str) -> Int` — calls a C function directly.
    /// `extern "C" link "m"` — names a library to link.
    fn extern_decl(&mut self) -> Result<Stmt, SiskinError> {
        let line = self.line();
        self.expect_kw("extern", "E0110")?;
        match self.tok().clone() {
            Tok::Str(abi) => {
                if abi != "C" {
                    return Err(SiskinError::new(
                        "E0150",
                        tr!(format!("`{}` 규약은 지원하지 않습니다", abi), format!("calling convention `{}` is not supported", abi)),
                        line,
                        self.col(),
                    )
                    .with_fix(tr!("지금은 `extern \"C\"` 만 됩니다", "only `extern \"C\"` is supported for now")));
                }
                self.bump();
            }
            _ => {
                return Err(SiskinError::new("E0151", tr!("`extern` 뒤에는 규약 이름이 옵니다", "expected a calling convention after `extern`"), line, self.col())
                    .with_fix(tr!("`extern \"C\" fn ...` 형태로 씁니다", "write `extern \"C\" fn ...`")))
            }
        }

        // extern "C" link "name"
        if self.at_ident("link") {
            self.bump();
            let lib = match self.tok().clone() {
                Tok::Str(l) => {
                    self.bump();
                    l
                }
                _ => {
                    return Err(SiskinError::new("E0152", tr!("링크할 라이브러리 이름이 필요합니다", "expected a library name to link"), line, self.col())
                        .with_fix(tr!("`extern \"C\" link \"m\"` 처럼 따옴표로 적습니다", "write it in quotes: `extern \"C\" link \"m\"`")))
                }
            };
            self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
            return Ok(Stmt::Link(lib, line));
        }

        self.expect_kw("fn", "E0110")?;
        let name = self.expect_ident("E0123", tr!("함수 이름", "function name"))?;
        let params = self.params()?;
        let ret = if self.eat(&Tok::Arrow) { Some(self.type_expr()?) } else { None };
        self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))
            .map_err(|e| e.with_fix(tr!("extern 함수에는 본문이 없습니다. `:` 없이 한 줄로 끝냅니다", "extern functions have no body; end the line without `:`")))?;
        Ok(Stmt::Fn(Rc::new(FnDecl {
            name,
            generics: Vec::new(),
            params,
            ret,
            doc: None,
            requires: Vec::new(),
            ensures: Vec::new(),
            body: Vec::new(),
            line,
            is_extern: true,
            c_sig: None,
        })))
    }

    fn opt_generics(&mut self) -> Result<Vec<String>, SiskinError> {
        let mut out = Vec::new();
        if self.eat(&Tok::LBracket) {
            loop {
                let n = self.expect_ident("E0124", tr!("타입 매개변수 이름", "type parameter name"))?;
                // Constraints like `T: Ord` are only parsed and ignored in P1.
                if self.eat(&Tok::Colon) {
                    let _ = self.type_expr()?;
                }
                out.push(n);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(Tok::RBracket, "E0125", "`]`")?;
        }
        Ok(out)
    }

    fn params(&mut self) -> Result<Vec<Param>, SiskinError> {
        self.expect(Tok::LParen, "E0126", "`(`")?;
        let mut out = Vec::new();
        if !self.at(&Tok::RParen) {
            loop {
                let conv = if self.eat_kw("owned") {
                    Convention::Owned
                } else if self.eat_kw("inout") {
                    Convention::Inout
                } else {
                    Convention::Borrow
                };
                // Other languages' `mut self`, `var self`, `mut x: T` — Siskin uses `inout`.
                let foreign_mut = self.at_kw("var")
                    || self.at_kw("let")
                    || matches!(self.tok(), Tok::Ident(s) if s == "mut" || s == "ref");
                if foreign_mut {
                    let word = match self.tok() {
                        Tok::Kw(s) | Tok::Ident(s) => s.clone(),
                        _ => String::new(),
                    };
                    let next_self = matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Ident(s)) if s == "self");
                    let fix = if next_self {
                        tr!("값을 바꾸는 메서드는 `fn 이름(inout self):` 로 씁니다. 부를 때 받는 쪽은 `var` 로 만든 변수여야 합니다", "a mutating method is written `fn name(inout self):`; the receiver must be a variable declared with `var`").to_string()
                    } else {
                        tr!("받은 변수를 바꾸려면 `inout 이름: 타입` 으로 씁니다. 안에서만 고쳐 쓰려면 `var 복사본 = 인자` 로 복사하세요", "to modify the caller's variable, write `inout name: Type`; to change it only locally, copy it with `var copy = param`").to_string()
                    };
                    return Err(self
                        .err("E0127", tr!(format!("인자 앞에 `{}` 를 쓸 수 없습니다", word), format!("`{}` is not allowed before a parameter", word)))
                        .with_fix(fix));
                }
                let name = self.expect_ident("E0127", tr!("인자 이름", "parameter name"))?;
                let is_self = name == "self";
                let ty = if self.eat(&Tok::Colon) { Some(self.type_expr()?) } else { None };
                if !is_self && ty.is_none() {
                    return Err(self
                        .err("E0128", tr!(format!("인자 `{}`에 타입 표기가 없습니다", name), format!("parameter `{}` has no type annotation", name)))
                        .with_fix(tr!("함수 시그니처는 타입 표기가 필수입니다. 지역 변수만 추론됩니다", "function signatures require type annotations; only local variables are inferred")));
                }
                out.push(Param { name, ty, conv, is_self });
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(Tok::RParen, "E0116", "`)`")?;
        Ok(out)
    }

    fn struct_decl(&mut self) -> Result<StructDecl, SiskinError> {
        let line = self.line();
        self.expect_kw("struct", "E0110")?;
        let name = self.expect_ident("E0129", tr!("구조체 이름", "struct name"))?;
        let generics = self.opt_generics()?;
        let mut interfaces = Vec::new();
        if self.eat(&Tok::LParen) {
            if !self.at(&Tok::RParen) {
                loop {
                    interfaces.push(self.expect_ident("E0130", tr!("인터페이스 이름", "interface name"))?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
            }
            self.expect(Tok::RParen, "E0116", "`)`")?;
        }
        self.expect(Tok::Colon, "E0100", "`:`")?;
        self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
        self.expect(Tok::Indent, "E0102", tr!("들여쓴 블록", "indented block"))?;

        let mut doc = None;
        self.skip_newlines();
        if let Tok::Str(s) = self.tok().clone() {
            if self.toks.get(self.pos + 1).map(|t| &t.tok) == Some(&Tok::Newline) {
                self.bump();
                self.bump();
                doc = Some(s);
            }
        }

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        loop {
            self.skip_newlines();
            if self.at(&Tok::Dedent) || self.at(&Tok::Eof) {
                break;
            }
            if self.at_kw("fn") {
                methods.push(Rc::new(self.fn_decl()?));
                continue;
            }
            let fname = self.expect_ident("E0131", tr!("필드 이름", "field name"))?;
            self.expect(Tok::Colon, "E0132", "`:`")
                .map_err(|e| e.with_fix(tr!("필드는 `이름: 타입` 형태로 씁니다", "fields are written `name: Type`")))?;
            let ty = self.type_expr()?;
            let default = if self.eat(&Tok::Assign) { Some(self.expr()?) } else { None };
            self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
            fields.push(FieldDecl { name: fname, ty: Some(ty), default });
        }
        self.eat(&Tok::Dedent);
        Ok(StructDecl { name, generics, interfaces, fields, methods, doc, line })
    }

    fn enum_decl(&mut self) -> Result<EnumDecl, SiskinError> {
        let line = self.line();
        self.expect_kw("enum", "E0110")?;
        let name = self.expect_ident("E0133", tr!("열거형 이름", "enum name"))?;
        self.expect(Tok::Colon, "E0100", "`:`")?;
        self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
        self.expect(Tok::Indent, "E0102", tr!("들여쓴 블록", "indented block"))?;

        let mut doc = None;
        self.skip_newlines();
        if let Tok::Str(s) = self.tok().clone() {
            if self.toks.get(self.pos + 1).map(|t| &t.tok) == Some(&Tok::Newline) {
                self.bump();
                self.bump();
                doc = Some(s);
            }
        }

        let mut variants = Vec::new();
        let mut methods = Vec::new();
        loop {
            self.skip_newlines();
            if self.at(&Tok::Dedent) || self.at(&Tok::Eof) {
                break;
            }
            if self.at_kw("fn") {
                methods.push(Rc::new(self.fn_decl()?));
                continue;
            }
            let vname = self.expect_ident("E0134", tr!("변형 이름", "variant name"))?;
            let mut fields = Vec::new();
            if self.eat(&Tok::LParen) {
                if !self.at(&Tok::RParen) {
                    loop {
                        let fname = self.expect_ident("E0131", tr!("필드 이름", "field name"))?;
                        self.expect(Tok::Colon, "E0132", "`:`")?;
                        let ty = self.type_expr()?;
                        fields.push(FieldDecl { name: fname, ty: Some(ty), default: None });
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                }
                self.expect(Tok::RParen, "E0116", "`)`")?;
            }
            self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
            variants.push(VariantDecl { name: vname, fields });
        }
        self.eat(&Tok::Dedent);
        if variants.is_empty() {
            return Err(self.err("E0135", tr!("열거형에 변형이 하나도 없습니다", "enum has no variants")));
        }
        Ok(EnumDecl { name, variants, methods, doc, line })
    }

    fn interface_decl(&mut self) -> Result<InterfaceDecl, SiskinError> {
        let line = self.line();
        self.expect_kw("interface", "E0110")?;
        let name = self.expect_ident("E0136", tr!("인터페이스 이름", "interface name"))?;
        self.expect(Tok::Colon, "E0100", "`:`")?;
        self.expect(Tok::Newline, "E0101", tr!("줄바꿈", "newline"))?;
        self.expect(Tok::Indent, "E0102", tr!("들여쓴 블록", "indented block"))?;
        let mut methods = Vec::new();
        loop {
            self.skip_newlines();
            if self.at(&Tok::Dedent) || self.at(&Tok::Eof) {
                break;
            }
            let sig = self.fn_sig()?;
            methods.push(sig.name);
        }
        self.eat(&Tok::Dedent);
        Ok(InterfaceDecl { name, methods, line })
    }

    // ------------------------------------------------------------------ types

    fn type_expr(&mut self) -> Result<TypeExpr, SiskinError> {
        let t = self.type_expr_one()?;
        // `BankError!Unit` — a type that yields a BankError value on failure (same shape as Zig's `E!T`).
        if self.at(&Tok::Bang) {
            self.bump();
            let ok = self.type_expr_one()?;
            return Ok(TypeExpr::Fallible(Box::new(ok), Some(Box::new(t))));
        }
        Ok(t)
    }

    fn type_expr_one(&mut self) -> Result<TypeExpr, SiskinError> {
        if self.eat(&Tok::Question) {
            return Ok(TypeExpr::Optional(Box::new(self.type_expr()?)));
        }
        if self.eat(&Tok::Bang) {
            return Ok(TypeExpr::Fallible(Box::new(self.type_expr_one()?), None));
        }
        if self.eat(&Tok::Star) {
            return Ok(TypeExpr::Raw(Box::new(self.type_expr()?)));
        }
        if self.eat(&Tok::LBracket) {
            let inner = self.type_expr()?;
            self.expect(Tok::RBracket, "E0125", "`]`")?;
            return Ok(TypeExpr::List(Box::new(inner)));
        }
        if self.eat(&Tok::LBrace) {
            let k = self.type_expr()?;
            self.expect(Tok::Colon, "E0132", "`:`")?;
            let v = self.type_expr()?;
            self.expect(Tok::RBrace, "E0137", "`}`")?;
            return Ok(TypeExpr::Dict(Box::new(k), Box::new(v)));
        }
        if self.eat(&Tok::LParen) {
            // `(T, U, ...)` — a tuple type, or `(T, U) -> R` — a function type.
            let mut items = Vec::new();
            if !self.at(&Tok::RParen) {
                loop {
                    items.push(self.type_expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                    if self.at(&Tok::RParen) {
                        break;
                    }
                }
            }
            self.expect(Tok::RParen, "E0116", "`)`")?;
            // A following `-> R` makes it a function type.
            if self.eat(&Tok::Arrow) {
                let ret = self.type_expr()?;
                return Ok(TypeExpr::Fn(items, Box::new(ret)));
            }
            return Ok(TypeExpr::Tuple(items));
        }
        let mut name = self.expect_ident("E0138", tr!("타입 이름", "type name"))?;
        // `colors.Rgb` — a type from an imported module.
        if self.at(&Tok::Dot) && matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Ident(_))) {
            self.bump();
            let inner = self.expect_ident("E0138", tr!("타입 이름", "type name"))?;
            name = format!("{}.{}", name, inner);
        }
        let mut args = Vec::new();
        if self.eat(&Tok::LBracket) {
            loop {
                args.push(self.type_expr()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(Tok::RBracket, "E0125", "`]`")?;
        }
        Ok(TypeExpr::Named(name, args))
    }

    // ---------------------------------------------------------------- expressions

    fn expr(&mut self) -> Result<Expr, SiskinError> {
        let e = self.or_expr()?;
        if self.at_kw("if") {
            self.bump();
            let cond = self.or_expr()?;
            self.expect_kw("else", "E0139")
                .map_err(|e| e.with_fix(tr!("조건 표현식은 `a if 조건 else b` 형태로 완성해야 합니다", "a conditional expression must be complete: `a if cond else b`")))?;
            let els = self.expr()?;
            return Ok(Expr::IfExpr { cond: Box::new(cond), then: Box::new(e), els: Box::new(els) });
        }
        // `expr else default` — if the left `?T` is none, use the right value.
        if self.at_kw("else") {
            let (l, c) = (self.line(), self.col());
            self.bump();
            let default = self.expr()?;
            return Ok(Expr::OrElse(Box::new(e), Box::new(default), l, c));
        }
        Ok(e)
    }

    fn or_expr(&mut self) -> Result<Expr, SiskinError> {
        let mut left = self.and_expr()?;
        while self.at_kw("or") {
            let (l, c) = (self.line(), self.col());
            self.bump();
            let right = self.and_expr()?;
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right), l, c);
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Result<Expr, SiskinError> {
        let mut left = self.not_expr()?;
        while self.at_kw("and") {
            let (l, c) = (self.line(), self.col());
            self.bump();
            let right = self.not_expr()?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right), l, c);
        }
        Ok(left)
    }

    fn not_expr(&mut self) -> Result<Expr, SiskinError> {
        if self.at_kw("not") {
            let (l, c) = (self.line(), self.col());
            self.bump();
            let e = self.not_expr()?;
            return Ok(Expr::Unary(UnOp::Not, Box::new(e), l, c));
        }
        self.cmp_expr()
    }

    fn cmp_expr(&mut self) -> Result<Expr, SiskinError> {
        let mut left = self.add_expr()?;
        loop {
            let op = match self.tok() {
                Tok::EqEq => BinOp::Eq,
                Tok::Ne => BinOp::Ne,
                Tok::Lt => BinOp::Lt,
                Tok::Le => BinOp::Le,
                Tok::Gt => BinOp::Gt,
                Tok::Ge => BinOp::Ge,
                // `x in xs` / `x not in xs` — written as in Python. Read as `xs.contains(x)`
                // (for a dict: whether the key exists; for a string: whether the substring exists).
                Tok::Kw(k) if k == "in" || (k == "not" && self.peek_kw_at(1, "in")) => {
                    let negate = k == "not";
                    let (l, c) = (self.line(), self.col());
                    self.bump();
                    if negate {
                        self.bump();
                    }
                    let right = self.add_expr()?;
                    let call = Expr::Call {
                        callee: Box::new(Expr::Field(Box::new(right), "contains".into(), l, c)),
                        targs: Vec::new(),
                        args: vec![Arg { name: None, value: left }],
                        line: l,
                        col: c,
                    };
                    left = if negate { Expr::Unary(UnOp::Not, Box::new(call), l, c) } else { call };
                    continue;
                }
                _ => break,
            };
            let (l, c) = (self.line(), self.col());
            self.bump();
            let right = self.add_expr()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right), l, c);
        }
        Ok(left)
    }

    fn add_expr(&mut self) -> Result<Expr, SiskinError> {
        let mut left = self.mul_expr()?;
        loop {
            let op = match self.tok() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            let (l, c) = (self.line(), self.col());
            self.bump();
            let right = self.mul_expr()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right), l, c);
        }
        Ok(left)
    }

    fn mul_expr(&mut self) -> Result<Expr, SiskinError> {
        let mut left = self.unary()?;
        loop {
            let op = match self.tok() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => break,
            };
            let (l, c) = (self.line(), self.col());
            self.bump();
            let right = self.unary()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right), l, c);
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, SiskinError> {
        if self.at(&Tok::Minus) {
            let (l, c) = (self.line(), self.col());
            self.bump();
            let e = self.unary()?;
            return Ok(Expr::Unary(UnOp::Neg, Box::new(e), l, c));
        }
        if self.at_kw("try") {
            let (l, c) = (self.line(), self.col());
            self.bump();
            let e = self.unary()?;
            return Ok(Expr::Try(Box::new(e), l, c));
        }
        // `spawn call` — a contextual keyword. Treated as spawn only when a name follows directly
        // (so `spawn` can still be used as a variable or function name).
        if matches!(self.tok(), Tok::Ident(n) if n == "spawn") && matches!(self.peek_tok(1), Some(Tok::Ident(_))) {
            let (l, c) = (self.line(), self.col());
            self.bump();
            let e = self.postfix()?;
            if !matches!(e, Expr::Call { .. }) {
                return Err(SiskinError::new("E0162", tr!("`spawn` 뒤에는 함수 호출이 와야 합니다", "`spawn` must be followed by a function call"), l, c)
                    .with_fix(tr!("`spawn 일하기(x)` 처럼 부를 함수를 씁니다", "write the call to run, like `spawn work(x)`")));
            }
            let (bl, bc) = e.pos();
            return Ok(Expr::Spawn(
                Rc::new(FnDecl {
                    name: format!("{}spawn{}_{}", LAMBDA_PREFIX, l, c),
                    generics: Vec::new(),
                    params: Vec::new(),
                    ret: None,
                    doc: None,
                    requires: Vec::new(),
                    ensures: Vec::new(),
                    body: vec![Stmt::Return(Some(e), if bl == 0 { l } else { bl }, if bc == 0 { c } else { bc })],
                    line: l,
                    is_extern: false,
                    c_sig: None,
                }),
                l,
                c,
            ));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, SiskinError> {
        let mut e = self.primary()?;
        loop {
            let (l, c) = (self.line(), self.col());
            if self.eat(&Tok::LParen) {
                let args = self.call_args()?;
                e = Expr::Call { callee: Box::new(e), targs: Vec::new(), args, line: l, col: c };
                continue;
            }
            if self.at(&Tok::LBracket) {
                // `alloc[Int](16)` is a generic call, `xs[0]` is indexing.
                // Tell them apart by whether `(` follows the type list.
                let save = self.pos;
                self.bump();
                let mut targs = Vec::new();
                let ok = loop {
                    match self.type_expr() {
                        Ok(t) => targs.push(t),
                        Err(_) => break false,
                    }
                    if self.eat(&Tok::Comma) {
                        continue;
                    }
                    break self.eat(&Tok::RBracket) && self.at(&Tok::LParen);
                };
                if ok {
                    self.bump();
                    let args = self.call_args()?;
                    e = Expr::Call { callee: Box::new(e), targs, args, line: l, col: c };
                    continue;
                }
                self.pos = save;
                self.bump();
                let idx = self.expr()?;
                self.expect(Tok::RBracket, "E0125", "`]`")?;
                e = Expr::Index(Box::new(e), Box::new(idx), l, c);
                continue;
            }
            if self.eat(&Tok::Dot) {
                // The n-th value of a tuple: `p.0`, `p.1`
                if let Tok::Int(n) = self.tok().clone() {
                    self.bump();
                    e = Expr::Field(Box::new(e), n.to_string(), l, c);
                    continue;
                }
                if let Tok::Float(_) = self.tok().clone() {
                    return Err(self
                        .err("E0140", tr!("`p.0.1` 처럼 이어 쓸 수 없습니다", "tuple indices cannot be chained like `p.0.1`"))
                        .with_fix(tr!("`let (a, b) = p` 로 먼저 풀거나 괄호로 나누세요: `(p.0).1`", "destructure first with `let (a, b) = p`, or add parentheses: `(p.0).1`")));
                }
                let name = self.expect_ident("E0140", tr!("필드 또는 메서드 이름", "field or method name"))?;
                e = Expr::Field(Box::new(e), name, l, c);
                continue;
            }
            break;
        }
        Ok(e)
    }

    /// `fn(x: Int, y) -> Int: expr` — an anonymous function. The body is a single expression.
    /// Parameter types may be omitted if they can be inferred from context (the type checker fills them in).
    /// If you need multiple lines, declare a named `fn` inside the function (that is a closure too).
    fn lambda(&mut self) -> Result<Expr, SiskinError> {
        let (l, c) = (self.line(), self.col());
        self.bump(); // fn
        if !self.at(&Tok::LParen) {
            return Err(self
                .err("E0160", tr!("`fn` 뒤에 이름이 오면 함수 선언이고, 식 안에서는 `fn(인자): 식` 모양이어야 합니다", "`fn` followed by a name is a function declaration; inside an expression write `fn(params): expr`"))
                .with_fix(tr!("익명 함수는 `fn(x: Int): x * 2` 처럼 씁니다", "write an anonymous function like `fn(x: Int): x * 2`")));
        }
        self.bump();
        let mut params = Vec::new();
        if !self.at(&Tok::RParen) {
            loop {
                let conv = if self.eat_kw("owned") { Convention::Owned } else { Convention::Borrow };
                let name = self.expect_ident("E0127", tr!("인자 이름", "parameter name"))?;
                let ty = if self.eat(&Tok::Colon) { Some(self.type_expr()?) } else { None };
                params.push(Param { name, ty, conv, is_self: false });
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(Tok::RParen, "E0116", "`)`")?;
        let ret = if self.eat(&Tok::Arrow) { Some(self.type_expr()?) } else { None };
        self.expect(Tok::Colon, "E0100", "`:`")
            .map_err(|e| e.with_fix(tr!("익명 함수는 `fn(x: Int): x * 2` 처럼 `:` 뒤에 식을 씁니다", "an anonymous function takes an expression after `:`, like `fn(x: Int): x * 2`")))?;
        if self.at(&Tok::Newline) {
            return Err(SiskinError::new("E0161", tr!("익명 함수의 본문은 `:` 뒤 같은 줄에 식 하나로 씁니다", "an anonymous function's body must be a single expression on the same line after `:`"), l, c)
                .with_fix(tr!("여러 줄이 필요하면 함수 안에 `fn 이름(...):` 으로 이름 붙은 함수를 선언하세요. 그것도 바깥 값을 붙잡습니다", "if you need several lines, declare a named function `fn name(...):` inside the function; it captures outer values too")));
        }
        let body_expr = self.expr()?;
        let (bl, bc) = body_expr.pos();
        Ok(Expr::Lambda(
            Rc::new(FnDecl {
                name: format!("{}{}_{}", LAMBDA_PREFIX, l, c),
                generics: Vec::new(),
                params,
                ret,
                doc: None,
                requires: Vec::new(),
                ensures: Vec::new(),
                body: vec![Stmt::Return(Some(body_expr), if bl == 0 { l } else { bl }, if bc == 0 { c } else { bc })],
                line: l,
                is_extern: false,
                c_sig: None,
            }),
            l,
            c,
        ))
    }

    /// Reads the argument list and closing `)`, with the opening `(` already consumed.
    fn call_args(&mut self) -> Result<Vec<Arg>, SiskinError> {
        let mut args = Vec::new();
        if !self.at(&Tok::RParen) {
            loop {
                // Named argument: `name: value`
                let named = match (
                    self.tok().clone(),
                    self.toks.get(self.pos + 1).map(|t| t.tok.clone()),
                ) {
                    (Tok::Ident(n), Some(Tok::Colon)) => Some(n),
                    // Python-style `name=value` is `name: value` in Siskin.
                    (Tok::Ident(n), Some(Tok::Assign)) => {
                        self.bump();
                        return Err(self
                            .err("E0116", tr!(format!("이름 붙은 인자는 `=` 가 아니라 `:` 로 씁니다"), format!("named arguments use `:`, not `=`")))
                            .with_fix(tr!(format!("`{}: 값` 처럼 쓰세요 (예: `Item(name: \"사과\", qty: 3)`)", n), format!("write `{}: value` (e.g. `Item(name: \"apple\", qty: 3)`)", n))));
                    }
                    _ => None,
                };
                if let Some(n) = named {
                    self.bump();
                    self.bump();
                    let v = self.expr()?;
                    args.push(Arg { name: Some(n), value: v });
                } else {
                    let v = self.expr()?;
                    args.push(Arg { name: None, value: v });
                }
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(Tok::RParen, "E0116", "`)`")?;
        Ok(args)
    }

    fn primary(&mut self) -> Result<Expr, SiskinError> {
        let (l, c) = (self.line(), self.col());
        match self.tok().clone() {
            Tok::Int(n) => {
                self.bump();
                Ok(Expr::Int(n))
            }
            Tok::Float(n) => {
                self.bump();
                Ok(Expr::Float(n))
            }
            Tok::Str(s) => {
                let (l, c) = (self.line(), self.col());
                self.bump();
                // Writing `"value is {x}"` without f prints it literally. It may be intentional, so only warn.
                if let Some(name) = looks_interpolated(&s) {
                    crate::error::push_warning(
                        SiskinError::new("W0001", tr!(format!("`{{{}}}` 이 글자 그대로 찍힙니다", name), format!("`{{{}}}` is printed literally", name)), l, c)
                            .with_fix(tr!(format!("값을 넣으려면 따옴표 앞에 f 를 붙이세요: f\"...{{{}}}...\"", name), format!("to interpolate the value, put `f` before the quote: f\"...{{{}}}...\"", name))),
                    );
                }
                Ok(Expr::Str(Rc::new(s)))
            }
            Tok::FStr(parts) => {
                self.bump();
                let mut out = Vec::new();
                for p in parts {
                    match p {
                        FPart::Lit(s) => out.push(FStrPart::Lit(s)),
                        FPart::Expr(src, spec) => {
                            let e = parse_expr_str(&src).map_err(|mut e| {
                                e.line = l;
                                e.col = c;
                                e
                            })?;
                            if let Some(why) = crate::value::spec_problem(&spec) {
                                return Err(SiskinError::new("E0143", why, l, c).with_fix(
                                    tr!("쓸 수 있는 모양: `.2f` `<8` `>8` `^8` `05d` `x`. 폭이 변수라면 `s.pad_left(w)` `s.pad_right(w)` 를 쓰세요", "supported specs: `.2f` `<8` `>8` `^8` `05d` `x`; for a variable width use `s.pad_left(w)` `s.pad_right(w)`"),
                                ));
                            }
                            let mut e = e;
                            retag(&mut e, l, c);
                            out.push(FStrPart::Expr(Box::new(e), spec));
                        }
                    }
                }
                Ok(Expr::FString(out))
            }
            Tok::Ident(name) => {
                self.bump();
                Ok(Expr::Ident(name, l, c))
            }
            Tok::Kw(k) => match k.as_str() {
                "true" => {
                    self.bump();
                    Ok(Expr::Bool(true))
                }
                "false" => {
                    self.bump();
                    Ok(Expr::Bool(false))
                }
                "none" => {
                    self.bump();
                    Ok(Expr::NoneLit)
                }
                "fn" => self.lambda(),
                "inout" | "var" | "mut" | "ref" => Err(self
                    .err("E0141", tr!(format!("표현식을 기대했는데 키워드 `{}`가 왔습니다", k), format!("expected an expression, found keyword `{}`", k)))
                    .with_fix(tr!("부를 때는 표시 없이 `f(x)` 로 넘깁니다. 받는 함수가 `inout` 이면 `x` 는 `var` 여야 합니다", "pass arguments without a marker: `f(x)`; if the parameter is `inout`, `x` must be a `var`"))),
                _ => Err(self.err("E0141", tr!(format!("표현식을 기대했는데 키워드 `{}`가 왔습니다", k), format!("expected an expression, found keyword `{}`", k)))),
            },
            Tok::LParen => {
                self.bump();
                let first = self.expr()?;
                if self.at(&Tok::Comma) {
                    // Tuple: `(a, b, ...)`
                    let mut items = vec![first];
                    while self.eat(&Tok::Comma) {
                        if self.at(&Tok::RParen) {
                            break; // allow a trailing comma
                        }
                        items.push(self.expr()?);
                    }
                    self.expect(Tok::RParen, "E0116", "`)`")?;
                    Ok(Expr::Tuple(items))
                } else {
                    self.expect(Tok::RParen, "E0116", "`)`")?;
                    Ok(first)
                }
            }
            Tok::LBracket => {
                self.bump();
                let mut items = Vec::new();
                self.skip_newlines();
                if !self.at(&Tok::RBracket) {
                    loop {
                        self.skip_newlines();
                        items.push(self.expr()?);
                        self.skip_newlines();
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                        self.skip_newlines();
                        if self.at(&Tok::RBracket) {
                            break;
                        }
                    }
                }
                self.expect(Tok::RBracket, "E0125", "`]`")?;
                Ok(Expr::List(items))
            }
            Tok::LBrace => {
                self.bump();
                let mut pairs = Vec::new();
                self.skip_newlines();
                if !self.at(&Tok::RBrace) {
                    loop {
                        self.skip_newlines();
                        let k = self.expr()?;
                        self.expect(Tok::Colon, "E0132", "`:`")?;
                        let v = self.expr()?;
                        pairs.push((k, v));
                        self.skip_newlines();
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                        self.skip_newlines();
                        if self.at(&Tok::RBrace) {
                            break;
                        }
                    }
                }
                self.expect(Tok::RBrace, "E0137", "`}`")?;
                Ok(Expr::Dict(pairs))
            }
            other => Err(self
                .err("E0141", tr!(format!("표현식을 기대했는데 {}이(가) 왔습니다", other), format!("expected an expression, found {}", other)))),
        }
    }
}

/// If the text contains `{name}` or `{name.field}`, returns that name.
fn looks_interpolated(s: &str) -> Option<String> {
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        let after = &rest[i + 1..];
        if let Some(j) = after.find('}') {
            let inner = &after[..j];
            let ok = !inner.is_empty()
                && inner.chars().next().map_or(false, |c| c.is_alphabetic() || c == '_')
                && inner.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '(' || c == ')');
            if ok {
                return Some(inner.to_string());
            }
            rest = &after[j + 1..];
        } else {
            break;
        }
    }
    None
}
