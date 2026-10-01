#[macro_use]
mod lang;
mod ast;
mod casefold;
mod conc;
mod cheader;
mod cgen;
mod error;
mod json;
mod interp;
mod lexer;
mod parser;
mod regex;
mod sys;
mod fmt;
mod lsp;
mod debug;
mod ns;
mod pkg;
mod types;
mod value;

use std::collections::HashSet;
use std::process::ExitCode;

/// Standard library module names. Imports starting with these names are built in,
/// so they are not read from files. Any other name is a user-written `.skn` file.
const STDLIB_MODULES: &[&str] =
    &["std", "math", "fs", "io", "time", "random", "re", "json", "process", "net"];

/// Standard library pieces written in Siskin. Importing a module merges its piece into the program.
/// The rest is built in Siskin on top of small builtins (starting with `__`) that query the OS.
const STD_SOURCES: &[(&str, &str)] = &[
    ("time", include_str!("std/time.skn")),
    ("process", include_str!("std/process.skn")),
    ("net", include_str!("std/net.skn")),
];

/// On a line like `import std.time`, append that module's Siskin piece (only once).
fn inject_std(path: &[String], injected: &mut HashSet<String>, out: &mut Vec<ast::Stmt>) {
    let m = match path.last() {
        Some(m) => m.as_str(),
        None => return,
    };
    if let Some((_, src)) = STD_SOURCES.iter().find(|(n, _)| *n == m) {
        if !injected.insert(m.to_string()) {
            return;
        }
        let base = error::register_file(&format!("<std.{}>", m), src);
        match parser::parse_at(src, base) {
            Ok(p) => {
                for s in p.stmts {
                    if let ast::Stmt::Import { path: ip, .. } = &s {
                        inject_std(ip, injected, out);
                    }
                    out.push(s);
                }
            }
            Err(e) => eprint!("{}", e.render(&format!("<std.{}>", m), src)),
        }
    }
}

fn is_user_module(path: &[String]) -> bool {
    match path.first() {
        Some(first) => !STDLIB_MODULES.contains(&first.as_str()),
        None => false,
    }
}

type LoadErr = (String, String, error::SiskinError);

/// Files read via import. Each file is one module (`ns::Module`); index 0 is main.
struct Loader {
    mods: Vec<ns::Module>,
    /// Per module: (display path, source) — used to report errors relative to that file.
    files: Vec<(String, String)>,
    /// Canonical path → module index. Each file is read only once (mutual imports are fine).
    index: std::collections::HashMap<String, usize>,
    /// Completion order (dependencies first). Modules are merged in this order.
    order: Vec<usize>,
    /// Standard library pieces (visible everywhere)
    globals: Vec<ast::Stmt>,
    injected: HashSet<String>,
    /// Names of everything imported from C headers (functions, constants, structs, function pointer types).
    c_names: HashSet<String>,
    /// C functions already declared by an `import c` in any file.
    c_fns: HashSet<String>,
}

impl Loader {
    /// Prefix for a new module: the last name of the import path. If already taken, a number is appended.
    fn prefix_for(&self, ipath: &[String]) -> String {
        let base = ipath.last().cloned().unwrap_or_default();
        let mut p = base.clone();
        let mut k = 2;
        while self.mods.iter().any(|m| m.prefix == p) {
            p = format!("{}{}", base, k);
            k += 1;
        }
        p
    }

    /// Read one file, turn it into a module and return its index.
    fn load(&mut self, path: &std::path::Path, ipath: &[String]) -> Result<usize, LoadErr> {
        let shown = path.to_string_lossy().to_string();
        let canon = crate::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| shown.clone());
        if let Some(&i) = self.index.get(&canon) {
            return Ok(i);
        }
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                return Err((
                    shown.clone(),
                    String::new(),
                    error::SiskinError::new(
                        "E0117",
                        tr!(
                            format!("import한 파일을 읽을 수 없습니다: {} ({})", shown, e),
                            format!("cannot read imported file: {} ({})", shown, e)
                        ),
                        0,
                        1,
                    )
                    .with_fix(tr!(
                        "파일 이름과 위치를 확인하세요. import는 이 파일과 같은 폴더에서 찾습니다",
                        "check the file name and location; imports are looked up in the same folder as this file"
                    )),
                ));
            }
        };
        let base = error::register_file(&shown, &src);
        let prog = parser::parse_at(&src, base).map_err(|e| (shown.clone(), src.clone(), e))?;
        let prefix = self.prefix_for(ipath);
        let idx = self.mods.len();
        self.mods.push(ns::Module { prefix, stmts: Vec::new(), imports: Vec::new() });
        self.files.push((shown, src));
        self.index.insert(canon, idx);
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        self.take(idx, prog, &dir)?;
        Ok(idx)
    }

    /// Process the statements of module `idx`. User file imports are followed and read; only a record is kept.
    fn take(&mut self, idx: usize, prog: ast::Program, dir: &std::path::Path) -> Result<(), LoadErr> {
        let here = |me: &Self, e: error::SiskinError| -> LoadErr {
            let (p, s) = me.files[idx].clone();
            (p, s, e)
        };
        let mut stmts = Vec::new();
        let mut imports = Vec::new();
        for s in prog.stmts {
            if let ast::Stmt::Import { path, names, renames, alias, line, col } = &s {
                // `import fs` used to be silently accepted without defining any names.
                if idx == 0 && path.len() == 1 && path[0] != "std" && !is_user_module(path) {
                    let fix = if names.is_empty() {
                        tr!(format!("`import std.{}` 로 쓰세요", path[0]), format!("write `import std.{}`", path[0]))
                    } else {
                        tr!(
                            format!("`from std.{} import {}` 로 쓰세요", path[0], names.join(", ")),
                            format!("write `from std.{} import {}`", path[0], names.join(", "))
                        )
                    };
                    let m = tr!(
                        format!("표준 모듈은 앞에 `std.` 를 붙입니다: `std.{}`", path[0]),
                        format!("standard modules are prefixed with `std.`: `std.{}`", path[0])
                    );
                    return Err(here(self, error::SiskinError::new("E0144", m, *line, *col).with_fix(fix)));
                }
                if is_user_module(path) {
                    let child = pkg::resolve_module(dir, path).map_err(|m| {
                        let fix = tr!(
                            format!(
                                "파일 이름을 확인하세요. 남이 만든 패키지면 `siskin add {}` 로 넣습니다 (패키지 목록에 없으면 뒤에 git 주소를 붙입니다)",
                                path[0]
                            ),
                            format!(
                                "check the file name; for a third-party package, add it with `siskin add {}` (append a git URL if it is not in the package registry)",
                                path[0]
                            )
                        );
                        here(self, error::SiskinError::new("E0118", m, *line, *col).with_fix(fix))
                    })?;
                    let target = self.load(&child, path)?;
                    let alias = if names.is_empty() {
                        Some(alias.clone().unwrap_or_else(|| path.last().cloned().unwrap_or_default()))
                    } else {
                        None
                    };
                    let names = names.iter().cloned().zip(renames.iter().cloned()).collect();
                    imports.push(ns::ImportRef { target, alias, names, line: *line, col: *col });
                    continue;
                }
                if alias.is_some() || names != renames {
                    return Err(here(
                        self,
                        error::SiskinError::new(
                            "E0149",
                            tr!(
                                "표준 모듈에는 아직 `as` 로 다른 이름을 붙일 수 없습니다",
                                "`as` cannot be used with standard modules yet"
                            ),
                            *line,
                            *col,
                        )
                        .with_fix(tr!(
                            "`as` 없이 가져오세요. `as` 는 내 파일과 패키지를 가져올 때 씁니다",
                            "import it without `as`; `as` is for your own files and packages"
                        )),
                    ));
                }
                inject_std(path, &mut self.injected, &mut self.globals);
            }
            if matches!(s, ast::Stmt::CHeader { .. }) {
                expand_cheader(&s, dir, &mut stmts, &mut self.globals, &mut self.c_names, &mut self.c_fns).map_err(|e| here(self, e))?;
                continue;
            }
            stmts.push(s);
        }
        self.mods[idx].stmts = stmts;
        self.mods[idx].imports = imports;
        self.order.push(idx);
        Ok(())
    }
}

/// Turns type text from the header reader (`CPtr[U32]`, `[F32; 4]`, `(Int, Str) -> Int`) into a type.
fn parse_ty_text(s: &str) -> ast::TypeExpr {
    fn one(s: &[char], i: &mut usize) -> ast::TypeExpr {
        let skip = |i: &mut usize| {
            while *i < s.len() && s[*i] == ' ' {
                *i += 1;
            }
        };
        skip(i);
        if *i < s.len() && s[*i] == '[' {
            *i += 1;
            let inner = one(s, i);
            skip(i);
            if *i < s.len() && s[*i] == ';' {
                *i += 1;
                skip(i);
                let st = *i;
                while *i < s.len() && s[*i].is_ascii_digit() {
                    *i += 1;
                }
                let n: usize = s[st..*i].iter().collect::<String>().parse().unwrap_or(1);
                skip(i);
                *i += 1; // ]
                return ast::TypeExpr::Array(Box::new(inner), n);
            }
            *i += 1; // ]
            return ast::TypeExpr::List(Box::new(inner));
        }
        if *i < s.len() && s[*i] == '(' {
            *i += 1;
            let mut ps = Vec::new();
            skip(i);
            while *i < s.len() && s[*i] != ')' {
                ps.push(one(s, i));
                skip(i);
                if *i < s.len() && s[*i] == ',' {
                    *i += 1;
                }
                skip(i);
            }
            *i += 1; // )
            skip(i);
            if *i + 1 < s.len() && s[*i] == '-' && s[*i + 1] == '>' {
                *i += 2;
                let r = one(s, i);
                return ast::TypeExpr::Fn(ps, Box::new(r));
            }
            return ast::TypeExpr::Tuple(ps);
        }
        let st = *i;
        while *i < s.len() && (s[*i].is_alphanumeric() || s[*i] == '_') {
            *i += 1;
        }
        let name: String = s[st..*i].iter().collect();
        let mut args = Vec::new();
        if *i < s.len() && s[*i] == '[' {
            *i += 1;
            loop {
                args.push(one(s, i));
                skip(i);
                if *i < s.len() && s[*i] == ',' {
                    *i += 1;
                    continue;
                }
                break;
            }
            *i += 1; // ]
        }
        ast::TypeExpr::Named(name, args)
    }
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    one(&cs, &mut i)
}

fn named(n: &str) -> ast::TypeExpr {
    ast::TypeExpr::Named(n.to_string(), Vec::new())
}

fn cb_pair(cb: &Option<cheader::CbSig>) -> Option<(Vec<String>, String)> {
    cb.as_ref().map(|cb| (cb.c_params.clone(), cb.c_ret.clone()))
}

/// Expand one line like `import c "zlib.h" link "z"` into real declarations.
/// Functions read from the header are treated like `extern` functions and carry their original
/// C types in `c_sig`, which cgen uses to emit type-correct wrappers. Constants, structs and
/// function pointer types go into `globals` (visible from every file, once each).
fn expand_cheader(
    s: &ast::Stmt,
    src_dir: &std::path::Path,
    out: &mut Vec<ast::Stmt>,
    globals: &mut Vec<ast::Stmt>,
    c_names: &mut HashSet<String>,
    c_fns: &mut HashSet<String>,
) -> Result<(), error::SiskinError> {
    let (header, cpp, links, incdirs, only, defines, follow, line, col) = match s {
        ast::Stmt::CHeader { header, cpp, links, incdirs, only, defines, follow, line, col } => {
            (header, *cpp, links, incdirs, only, defines, follow, *line, *col)
        }
        _ => return Ok(()),
    };
    // Look in the folders given with `from "..."` as well as the folder of this source.
    // Folder names are resolved relative to this `.skn` file's location (same as `import`).
    let mut dirs: Vec<String> = Vec::new();
    for d in incdirs {
        let near = src_dir.join(d);
        if near.is_dir() {
            dirs.push(near.to_string_lossy().to_string());
        }
        dirs.push(d.clone());
    }
    if let Some(d) = src_dir.to_str() {
        // An empty folder means the current one (`siskin run app.skn`).
        dirs.push(if d.is_empty() { ".".to_string() } else { d.to_string() });
    }
    // `import c "./vec.h"`: a header next to this file, found through this file's folder.
    let header = &match header.strip_prefix("./") {
        Some(rest) if src_dir.join(rest).is_file() => rest.to_string(),
        _ => header.clone(),
    };
    let im = match cheader::import_header(header, cpp, &dirs, only, defines, follow) {
        Ok(x) => x,
        Err(msg) => {
            return Err(error::SiskinError::new("E0155", msg, line, col)
                .with_fix(tr!(
                    "헤더 이름과 `from \"폴더\"` 를 확인하세요. C로 쓴 `#include` 와 같은 이름을 적습니다",
                    "check the header name and `from \"dir\"`; use the same name you would `#include` in C"
                )))
        }
    };
    if im.fns.is_empty() && im.consts.is_empty() && im.structs.is_empty() && im.fnptrs.is_empty() {
        return Err(error::SiskinError::new(
            "E0156",
            tr!(
                format!("`{}` 에서 가져올 수 있는 것(함수·상수·구조체)을 찾지 못했습니다", header),
                format!("found nothing importable (functions, constants, structs) in `{}`", header)
            ),
            line,
            col,
        )
        .with_fix(tr!(
            "헤더 이름이 맞는지 확인하세요. 선언이 `#ifdef` 뒤에 숨어 있으면 `define \"이름\"` 을 붙입니다",
            "check the header name; if the declarations sit behind an `#ifdef`, add `define \"NAME\"`"
        )));
    }
    // `define "X=1"` also applies to the C files compiled along with `also`.
    for d in defines {
        out.push(ast::Stmt::Link(format!(":def:{}", d), line));
    }
    // `from "dir"` is also a search folder when compiling, so headers the imported one pulls in
    // with `#include <...>` from that folder are found by the C compiler too.
    for d in incdirs {
        let near = src_dir.join(d);
        let p = if near.is_dir() { near } else { std::path::PathBuf::from(d) };
        if p.is_dir() {
            let abs = crate::canonicalize(&p).unwrap_or(p);
            out.push(ast::Stmt::Link(format!(":inc:{}", abs.to_string_lossy().replace('\\', "/")), line));
        }
    }
    for lib in links {
        // `also "x.cpp"` is a source file, resolved relative to this file's folder.
        if let Some(rel) = lib.strip_prefix(":src:") {
            let near = src_dir.join(rel);
            let p = if near.exists() { near } else { std::path::PathBuf::from(rel) };
            let abs = crate::canonicalize(&p).unwrap_or(p);
            out.push(ast::Stmt::Link(format!(":src:{}", abs.to_string_lossy()), line));
            continue;
        }
        out.push(ast::Stmt::Link(lib.clone(), line));
    }
    // Use the location clang actually found, so that the C compiler sees the same file without `-I`.
    let header_c = if std::path::Path::new(&im.header_path).is_absolute() || im.header_path.starts_with('/') {
        im.header_path.clone()
    } else {
        header.clone()
    };
    let mk_sig = |ret: String, params: Vec<String>, call: String, shim: Option<String>, cbs: Vec<Option<(Vec<String>, String)>>, fnptr: bool| ast::CSig {
        ret,
        params,
        header: header_c.clone(),
        cpp,
        call,
        shim,
        cbs,
        defines: defines.clone(),
        fnptr,
    };
    let mut shadowed: Vec<String> = Vec::new();
    for f in &im.fns {
        // Names Siskin already has (`abs`, `free`, `pow` ...) take precedence on the Siskin side.
        if types::SISKIN_BUILTINS.contains(&f.name.as_str()) {
            shadowed.push(f.name.clone());
            continue;
        }
        let params: Vec<ast::Param> = f
            .params
            .iter()
            .enumerate()
            .map(|(i, t)| ast::Param {
                name: format!("a{}", i),
                ty: Some(match f.tys.get(i) {
                    Some(text) => parse_ty_text(text),
                    // A parameter that takes a function becomes a Siskin function type `(Int, Str) -> Int`.
                    None => match f.cbs.get(i).and_then(|c| c.as_ref()) {
                        Some(cb) => ast::TypeExpr::Fn(
                            cb.params.iter().map(|m| named(m.siskin())).collect(),
                            Box::new(named(cb.ret.map(|m| m.siskin()).unwrap_or("Unit"))),
                        ),
                        None => named(t.siskin()),
                    },
                }),
                conv: if f.outs.get(i).copied().unwrap_or(false) {
                    ast::Convention::Inout
                } else {
                    ast::Convention::Borrow
                },
                is_self: false,
            })
            .collect();
        let ret = if !f.ret_ty.is_empty() {
            if f.ret_ty == "Unit" { None } else { Some(parse_ty_text(&f.ret_ty)) }
        } else if f.ret == cheader::MTy::Unit {
            None
        } else {
            Some(named(f.ret.siskin()))
        };
        // The same function may come from a header imported in several files (or twice in
        // one file). C function names are global, so declare it only once.
        if !c_fns.insert(f.name.clone()) {
            continue;
        }
        c_names.insert(f.name.clone());
        out.push(ast::Stmt::Fn(crate::ast::Shared::new(ast::FnDecl {
            name: f.name.clone(),
            generics: Vec::new(),
            params,
            ret,
            doc: None,
            requires: Vec::new(),
            ensures: Vec::new(),
            body: Vec::new(),
            line,
            is_extern: true,
            c_sig: Some(mk_sig(
                f.c_ret.clone(),
                f.c_params.clone(),
                f.name.clone(),
                f.cpp_shim.clone(),
                f.cbs.iter().map(|c| cb_pair(c)).collect(),
                false,
            )),
        })));
    }
    // Constants: `let NAME = value` visible everywhere.
    for c in &im.consts {
        if !c_names.insert(c.name.clone()) {
            continue;
        }
        let value = match &c.val {
            cheader::CVal::Int(i) => ast::Expr::Int(*i),
            cheader::CVal::Float(f) => ast::Expr::Float(*f),
            cheader::CVal::Str(s) => ast::Expr::Str(crate::ast::Shared::new(s.clone())),
        };
        globals.push(ast::Stmt::Let { name: c.name.clone(), ty: None, value, mutable: false, catch: None, line, col });
    }
    // Structs: the C type itself is used in generated code, so the layout is exactly C's.
    for st in &im.structs {
        if !c_names.insert(st.name.clone()) {
            continue;
        }
        let fields = st
            .fields
            .iter()
            .map(|f| ast::FieldDecl { name: f.name.clone(), ty: Some(parse_ty_text(&f.ty)), default: None })
            .collect();
        let infos = st
            .fields
            .iter()
            .map(|f| ast::CFieldInfo { c_ty: f.c_ty.clone(), ptr: f.ptr, charp: f.charp, chars: f.chars, cb: cb_pair(&f.cb) })
            .collect();
        globals.push(ast::Stmt::Struct(crate::ast::Shared::new(ast::StructDecl {
            name: st.name.clone(),
            generics: Vec::new(),
            interfaces: Vec::new(),
            fields,
            methods: Vec::new(),
            doc: None,
            line,
            c: Some(ast::CStructInfo {
                c_name: st.c_name.clone(),
                header: header_c.clone(),
                defines: defines.clone(),
                is_union: st.is_union,
                fields: infos,
            }),
        })));
    }
    // Function pointer types: `GLFWkeyfun(addr)` makes a callable function value from an address.
    for p in &im.fnptrs {
        if !c_names.insert(p.name.clone()) {
            continue;
        }
        let fty = ast::TypeExpr::Fn(
            p.sig.params.iter().map(|m| named(m.siskin())).collect(),
            Box::new(named(p.sig.ret.map(|m| m.siskin()).unwrap_or("Unit"))),
        );
        globals.push(ast::Stmt::Fn(crate::ast::Shared::new(ast::FnDecl {
            name: p.name.clone(),
            generics: Vec::new(),
            params: vec![ast::Param { name: "addr".into(), ty: Some(named("Int")), conv: ast::Convention::Borrow, is_self: false }],
            ret: Some(fty),
            doc: None,
            requires: Vec::new(),
            ensures: Vec::new(),
            body: Vec::new(),
            line,
            is_extern: true,
            c_sig: Some(mk_sig(
                p.sig.c_ret.clone(),
                p.sig.c_params.clone(),
                p.name.clone(),
                None,
                vec![Some((p.sig.c_params.clone(), p.sig.c_ret.clone()))],
                true,
            )),
        })));
    }
    // Remember the names of functions that could not be imported, to explain why if they are called.
    let mut skipped = im.skipped.clone();
    for (n, why) in &im.skipped_fnptrs {
        skipped.push((n.clone(), format!("{} ({})", why, tr!("함수 포인터 타입", "function pointer type"))));
    }
    for n in shadowed {
        skipped.push((n, SHADOWED_WHY().into()));
    }
    for (n, why) in &skipped {
        out.push(ast::Stmt::CHeader {
            header: header.clone(),
            cpp,
            links: Vec::new(),
            incdirs: Vec::new(),
            only: vec![n.clone(), why.clone()],
            defines: Vec::new(),
            follow: Vec::new(),
            line,
            col,
        });
    }
    // Functions declared in headers this one includes but that were not followed (`#include <...>`):
    // calling one says which header to add with `follow "..."`.
    for (n, file) in &im.hidden {
        out.push(ast::Stmt::CHeader {
            header: header.clone(),
            cpp,
            links: Vec::new(),
            incdirs: Vec::new(),
            only: vec![n.clone(), String::new(), file.clone()],
            defines: Vec::new(),
            follow: Vec::new(),
            line,
            col,
        });
    }
    Ok(())
}

/// Every struct name a type mentions (`CPtr[VkExtent2D]` → VkExtent2D).
fn type_names(t: &ast::TypeExpr, out: &mut Vec<String>) {
    match t {
        ast::TypeExpr::Named(n, args) => {
            out.push(n.clone());
            for a in args {
                type_names(a, out);
            }
        }
        ast::TypeExpr::Optional(a) | ast::TypeExpr::List(a) | ast::TypeExpr::Raw(a) | ast::TypeExpr::Array(a, _) => type_names(a, out),
        ast::TypeExpr::Fallible(a, b) => {
            type_names(a, out);
            if let Some(b) = b {
                type_names(b, out);
            }
        }
        ast::TypeExpr::Dict(a, b) => {
            type_names(a, out);
            type_names(b, out);
        }
        ast::TypeExpr::Tuple(ts) => ts.iter().for_each(|x| type_names(x, out)),
        ast::TypeExpr::Fn(ps, r) => {
            ps.iter().for_each(|x| type_names(x, out));
            type_names(r, out);
        }
    }
}

/// A header like `vulkan.h` brings in thousands of functions, constants and structs.
/// Keep only the ones the program's source mentions (letter case ignored, since Siskin forgives
/// case mistakes), plus the structs those need. Everything else would only slow the build down.
fn drop_unused_c(ld: &mut Loader) {
    if ld.c_names.is_empty() {
        return;
    }
    let mut used: HashSet<String> = HashSet::new();
    for (_, src) in &ld.files {
        for w in src.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if !w.is_empty() {
                used.insert(w.to_lowercase());
            }
        }
    }
    let c_names = ld.c_names.clone();
    let keep_direct = |n: &str| !c_names.contains(n) || used.contains(&n.to_lowercase());
    // Structs reachable from what is kept (field types, parameter and return types).
    let mut need: HashSet<String> = HashSet::new();
    let mut struct_fields: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut scan_fn = |f: &ast::FnDecl, need: &mut HashSet<String>| {
        let mut names = Vec::new();
        for p in &f.params {
            if let Some(t) = &p.ty {
                type_names(t, &mut names);
            }
        }
        if let Some(r) = &f.ret {
            type_names(r, &mut names);
        }
        need.extend(names);
    };
    let all: Vec<&ast::Stmt> = ld.globals.iter().chain(ld.mods.iter().flat_map(|m| m.stmts.iter())).collect();
    for s in &all {
        match s {
            ast::Stmt::Fn(f) if f.c_sig.is_some() && keep_direct(&f.name) => scan_fn(f, &mut need),
            ast::Stmt::Struct(sd) if sd.c.is_some() => {
                let mut names = Vec::new();
                for fd in &sd.fields {
                    if let Some(t) = &fd.ty {
                        type_names(t, &mut names);
                    }
                }
                struct_fields.insert(sd.name.clone(), names);
                if keep_direct(&sd.name) {
                    need.insert(sd.name.clone());
                }
            }
            _ => {}
        }
    }
    let mut stack: Vec<String> = need.iter().cloned().collect();
    while let Some(n) = stack.pop() {
        if let Some(fs) = struct_fields.get(&n) {
            for x in fs {
                if need.insert(x.clone()) {
                    stack.push(x.clone());
                }
            }
        }
    }
    let keep = |s: &ast::Stmt| -> bool {
        match s {
            ast::Stmt::Fn(f) if f.c_sig.is_some() => keep_direct(&f.name),
            ast::Stmt::Struct(sd) if sd.c.is_some() => need.contains(&sd.name),
            ast::Stmt::Let { name, .. } if c_names.contains(name) => keep_direct(name),
            _ => true,
        }
    };
    ld.globals.retain(|s| keep(s));
    for m in ld.mods.iter_mut() {
        m.stmts.retain(|s| keep(s));
    }
}

/// Resolve all user file imports and merge them into one program. Each file has its own
/// namespace (`ns`), so two files may use the same name without clashing.
/// On failure, returns (file path, that file's source, error).
fn resolve_imports_err(
    prog: &mut ast::Program,
    main_path: &str,
) -> Result<(), (String, String, error::SiskinError)> {
    pkg::set_project_root(std::path::Path::new(main_path));
    let src = std::fs::read_to_string(main_path).unwrap_or_default();
    let mut ld = Loader {
        mods: vec![ns::Module { prefix: String::new(), stmts: Vec::new(), imports: Vec::new() }],
        files: vec![(main_path.to_string(), src)],
        index: std::collections::HashMap::new(),
        order: Vec::new(),
        globals: Vec::new(),
        injected: HashSet::new(),
        c_names: HashSet::new(),
        c_fns: HashSet::new(),
    };
    if let Ok(c) = crate::canonicalize(main_path) {
        ld.index.insert(c.to_string_lossy().to_string(), 0);
    }
    let dir = std::path::Path::new(main_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let main = ast::Program { stmts: std::mem::take(&mut prog.stmts) };
    ld.take(0, main, &dir)?;
    drop_unused_c(&mut ld);
    // Names visible everywhere: standard pieces, builtins, C functions.
    let mut globals: HashSet<String> = types::SISKIN_BUILTINS.iter().map(|s| s.to_string()).collect();
    for s in ld.globals.iter().chain(ld.mods.iter().flat_map(|m| m.stmts.iter())) {
        match s {
            ast::Stmt::Fn(f) if ld.globals.iter().any(|g| std::ptr::eq(g, s)) || f.is_extern || f.c_sig.is_some() => {
                globals.insert(f.name.clone());
            }
            ast::Stmt::Struct(sd) if sd.c.is_some() || ld.globals.iter().any(|g| std::ptr::eq(g, s)) => {
                globals.insert(sd.name.clone());
            }
            ast::Stmt::Let { name, .. } if ld.c_names.contains(name) => {
                globals.insert(name.clone());
            }
            _ => {}
        }
    }
    let order = ld.order.clone();
    let mut parts = ns::resolve(&mut ld.mods, &globals).map_err(|(i, e)| {
        let (p, s) = ld.files[i].clone();
        (p, s, e)
    })?;
    let mut merged = std::mem::take(&mut ld.globals);
    for i in order {
        merged.append(&mut parts[i]);
    }
    prog.stmts = merged;
    Ok(())
}

fn resolve_imports(prog: &mut ast::Program, main_path: &str) -> Result<(), ()> {
    resolve_imports_err(prog, main_path).map_err(|(p, src, e)| {
        eprint!("{}", e.render(&p, &src));
    })
}



/// `siskin fmt` — format a file (or every .skn inside a folder).
fn run_fmt(files: &[&String], args: &[String]) -> ExitCode {
    let check = args.iter().any(|a| a == "--check");
    let to_stdout = args.iter().any(|a| a == "--stdout");
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    fn walk(p: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        if p.is_dir() {
            let mut ents: Vec<_> = match std::fs::read_dir(p) {
                Ok(r) => r.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
                Err(_) => return,
            };
            ents.sort();
            for e in ents {
                let name = e.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if name.starts_with('.') || name == "target" {
                    continue;
                }
                if e.is_dir() || name.ends_with(".skn") {
                    walk(&e, out);
                }
            }
        } else {
            out.push(p.to_path_buf());
        }
    }
    if files.is_empty() {
        eprintln!(
            "{}",
            tr!(
                "정리할 파일이나 폴더를 적어 주세요. 예: siskin fmt main.skn  /  siskin fmt .",
                "give a file or folder to format, e.g. siskin fmt main.skn  /  siskin fmt ."
            )
        );
        return ExitCode::from(2);
    }
    for f in files {
        walk(std::path::Path::new(f.as_str()), &mut paths);
    }
    let mut changed = 0usize;
    let mut failed = false;
    for p in &paths {
        let shown = p.to_string_lossy().to_string();
        let src = match std::fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}: {} ({})", shown, tr!("읽을 수 없습니다", "cannot read"), e);
                failed = true;
                continue;
            }
        };
        let out = fmt::format(&src);
        if let Err(msg) = fmt::check_same(&src, &out) {
            eprintln!("{}: {}", shown, msg);
            failed = true;
            continue;
        }
        if to_stdout {
            print!("{}", out);
            continue;
        }
        if out == src {
            continue;
        }
        changed += 1;
        if check {
            println!("{}: {}", shown, tr!("정리할 곳이 있습니다", "needs formatting"));
            continue;
        }
        if let Err(e) = std::fs::write(p, &out) {
            eprintln!("{}: {} ({})", shown, tr!("쓸 수 없습니다", "cannot write"), e);
            failed = true;
            continue;
        }
        println!("{}: {}", shown, tr!("정리했습니다", "formatted"));
    }
    if !to_stdout && !check && changed == 0 && !failed {
        if lang::ko() {
            println!("이미 깔끔합니다 ({}개 파일)", paths.len());
        } else {
            println!("already formatted ({} file{})", paths.len(), if paths.len() == 1 { "" } else { "s" });
        }
    }
    if failed || (check && changed > 0) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Whether the program contains a `spawn` anywhere.
fn uses_spawn(prog: &ast::Program) -> bool {
    format!("{:?}", prog.stmts).contains("Spawn(")
}

/// Whether the program cannot run in the interpreter: it calls C functions or uses std.net.
fn needs_native(prog: &ast::Program) -> bool {
    // Reading and writing C addresses only means something next to real C code.
    let dbg = format!("{:?}", prog.stmts);
    if ["\"c_string\"", "\"address_of\"", "\"cast\"", "\"ptr_set\"", "\"cstr\"", "\"ptr_get\""].iter().any(|n| dbg.contains(&format!("Ident({}", n))) {
        return true;
    }
    prog.stmts.iter().any(|s| match s {
        ast::Stmt::Fn(f) => f.is_extern,
        ast::Stmt::Struct(sd) => sd.c.is_some(),
        ast::Stmt::Import { path, .. } => path.len() == 2 && path[0] == "std" && path[1] == "net",
        _ => false,
    })
}

/// Like `std::fs::canonicalize`, but strips the `\\?\` prefix added on Windows.
/// Paths with that prefix keep the C compiler from finding `#include "same_folder.h"`.
pub fn canonicalize<P: AsRef<std::path::Path>>(p: P) -> std::io::Result<std::path::PathBuf> {
    let c = std::fs::canonicalize(p)?;
    if cfg!(windows) {
        let s = c.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            if !rest.starts_with("UNC\\") {
                return Ok(std::path::PathBuf::from(rest));
            }
        }
    }
    Ok(c)
}

/// C / C++ compiler names. Can be overridden with the `CC` / `CXX` environment variables.
/// Windows usually has no `cc`, so look for clang, then gcc. clang comes first because
/// header import (`import c`) uses clang, and we want the same compiler for both.
fn c_compiler(cpp: bool) -> String {
    if let Ok(c) = std::env::var(if cpp { "CXX" } else { "CC" }) {
        if !c.trim().is_empty() {
            return c;
        }
    }
    // If the C compiler was set via CC, use the matching C++ compiler from the same family.
    if cpp {
        if let Ok(c) = std::env::var("CC") {
            let c = c.trim().to_string();
            if c.ends_with("clang") || c.ends_with("clang.exe") {
                return format!("{}++", c.trim_end_matches(".exe"));
            }
            if c.ends_with("gcc") || c.ends_with("gcc.exe") {
                return format!("{}g++", c.trim_end_matches(".exe").trim_end_matches("gcc"));
            }
        }
    }
    if cfg!(windows) {
        static FOUND: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();
        let (c, cxx) = FOUND.get_or_init(|| {
            let works = |c: &str| {
                std::process::Command::new(c)
                    .arg("--version")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            };
            if works("clang") {
                ("clang".to_string(), "clang++".to_string())
            } else if works("gcc") {
                ("gcc".to_string(), "g++".to_string())
            } else {
                ("clang".to_string(), "clang++".to_string())
            }
        });
        return if cpp { cxx.clone() } else { c.clone() };
    }
    (if cpp { "c++" } else { "cc" }).to_string()
}

/// The clang used to read headers. Uses `CC` if it is clang, otherwise `clang`.
pub fn header_clang() -> String {
    match std::env::var("CC") {
        Ok(c) if c.contains("clang") => c,
        _ => "clang".to_string(),
    }
}

/// Whether the compiler targets MinGW (gcc on Windows, llvm-mingw clang). Only then link statically.
fn targets_mingw(cc: &str) -> bool {
    std::process::Command::new(cc)
        .arg("-dumpmachine")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("mingw"))
        .unwrap_or(false)
}

/// Hint shown when no compiler is found, including what to install.
fn no_compiler(what: &str, e: std::io::Error) -> String {
    let hint = if cfg!(windows) {
        tr!(
            "\nC 컴파일러가 필요합니다: `winget install MartinStorsjo.LLVM-MinGW.UCRT` 로 깔고 새 터미널을 여세요",
            "\na C compiler is needed: install it with `winget install MartinStorsjo.LLVM-MinGW.UCRT`, then open a new terminal"
        )
    } else if cfg!(target_os = "macos") {
        tr!("\nC 컴파일러가 필요합니다: `xcode-select --install`", "\na C compiler is needed: `xcode-select --install`")
    } else {
        tr!("\nC 컴파일러가 필요합니다. 예: `sudo apt install gcc`", "\na C compiler is needed. e.g. `sudo apt install gcc`")
    };
    format!("{}: {}{}", what, e, hint)
}

/// Turn the generated C (and C++ bridge) files into an actual executable.
/// When a C++ library is used, the bridge file is compiled separately with the C++ compiler and linked in.
fn compile_native(
    csrc: &str,
    cppsrc: Option<&String>,
    all_links: &[String],
    args: &[String],
    cpath: &std::path::Path,
    outname: &std::path::Path,
    release: bool,
    opt: &str,
) -> Result<(), String> {
    // Items given with `also "x.cpp"` are files to compile together, not names to link.
    let mut links: Vec<String> = Vec::new();
    let mut extra_srcs: Vec<String> = Vec::new();
    // Macros from `define "X"` on an `import c` line.
    let mut defs: Vec<String> = Vec::new();
    // Folders from `from "dir"` on an `import c` line.
    let mut incs: Vec<String> = Vec::new();
    for l in all_links {
        if let Some(d) = l.strip_prefix(":def:") {
            defs.push(format!("-D{}", d));
            continue;
        }
        if let Some(d) = l.strip_prefix(":inc:") {
            incs.push(format!("-I{}", d));
            continue;
        }
        match l.strip_prefix(":src:") {
            Some(f) => extra_srcs.push(f.to_string()),
            None => links.push(l.clone()),
        }
    }
    let links = &links[..];
    let mut objs: Vec<std::path::PathBuf> = Vec::new();
    std::fs::write(cpath, csrc)
        .map_err(|e| tr!(format!("C 파일을 쓸 수 없습니다: {}", e), format!("cannot write C file: {}", e)))?;
    // C / C++ sources the user asked to compile along.
    for (n, f) in extra_srcs.iter().enumerate() {
        let is_cpp = f.ends_with(".cpp") || f.ends_with(".cc") || f.ends_with(".cxx") || f.ends_with(".C");
        let o = cpath.with_extension(format!("extra{}.o", n));
        let mut cc = std::process::Command::new(c_compiler(is_cpp));
        cc.arg(opt).arg("-w");
        if cfg!(windows) {
            cc.arg("-D_USE_MATH_DEFINES");
        }
        if is_cpp {
            cc.arg("-std=c++17");
        }
        cc.args(&defs).args(&incs);
        cc.arg("-c").arg(f).arg("-o").arg(&o);
        match cc.status() {
            Ok(st) if st.success() => {}
            Ok(_) => return Err(tr!(format!("`{}` 을(를) 컴파일하지 못했습니다", f), format!("failed to compile `{}`", f))),
            Err(e) => return Err(no_compiler(tr!("컴파일러를 실행할 수 없습니다", "cannot run the compiler"), e)),
        }
        objs.push(o);
    }
    let mut cpp_path: Option<std::path::PathBuf> = None;

    if let Some(src) = cppsrc {
        let p = cpath.with_extension("ffi.cpp");
        std::fs::write(&p, src).map_err(|e| {
            tr!(format!("C++ 다리 파일을 쓸 수 없습니다: {}", e), format!("cannot write C++ bridge file: {}", e))
        })?;
        let o = cpath.with_extension("ffi.o");
        let mut cxx = std::process::Command::new(c_compiler(true));
        cxx.arg(opt).arg("-std=c++17").arg("-w").args(&defs).args(&incs).arg("-c").arg(&p).arg("-o").arg(&o);
        if cfg!(windows) {
            cxx.arg("-D_USE_MATH_DEFINES");
        }
        for a in args {
            if let Some(dir) = a.strip_prefix("-I") {
                cxx.arg(format!("-I{}", dir));
            }
        }
        match cxx.status() {
            Ok(st) if st.success() => {}
            Ok(_) => {
                return Err(tr!(
                    format!("C++ 다리 파일을 컴파일하지 못했습니다. 남겨 둔 파일: {}", p.display()),
                    format!("failed to compile the C++ bridge file; kept it at: {}", p.display())
                ))
            }
            Err(e) => return Err(no_compiler(tr!("C++ 컴파일러를 실행할 수 없습니다", "cannot run the C++ compiler"), e)),
        }
        objs.push(o);
        cpp_path = Some(p);
    }

    // With a C++ bridge, link with the C++ compiler so the standard library comes along.
    let needs_cxx = cppsrc.is_some()
        || extra_srcs.iter().any(|f| {
            f.ends_with(".cpp") || f.ends_with(".cc") || f.ends_with(".cxx") || f.ends_with(".C")
        });
    let mut cc = std::process::Command::new(c_compiler(needs_cxx));
    // `--debug`: add debug info and disable optimization so gdb can follow .skn lines.
    if args.iter().any(|a| a == "--debug") {
        cc.arg("-g").arg("-O0").arg("-w");
    } else {
        cc.arg(opt).arg("-w");
    }
    if cfg!(windows) {
        // MSVC headers need this to provide math constants like M_PI (MinGW provides them by default).
        cc.arg("-D_USE_MATH_DEFINES");
    }
    if !needs_cxx {
        cc.arg("-std=gnu11");
    } else {
        cc.arg("-xc").arg(cpath).arg("-xnone");
    }
    if release {
        cc.arg("-DMI_RELEASE");
    }
    cc.args(&incs);
    if !needs_cxx {
        cc.arg(cpath);
    }
    for o in &objs {
        cc.arg(o);
    }
    cc.arg("-o").arg(outname);
    if cfg!(windows) {
        // Windows: link statically so the .exe runs without MinGW DLLs,
        // and add shell32, used to read command-line arguments as UTF-8.
        // (-static is for MinGW. MSVC-targeting clang never needs MinGW DLLs.)
        if targets_mingw(&c_compiler(needs_cxx)) {
            cc.arg("-static");
        }
        cc.arg("-lshell32");
    } else {
        cc.arg("-lm");
    }
    for lib in links {
        cc.arg(format!("-l{}", lib));
    }
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-l" {
            if let Some(lib) = args.get(i + 1) {
                cc.arg(format!("-l{}", lib));
            }
            i += 2;
            continue;
        }
        if args[i] == "-L" {
            if let Some(dir) = args.get(i + 1) {
                cc.arg(format!("-L{}", dir));
            }
            i += 2;
            continue;
        }
        if args[i].starts_with("-L") && args[i].len() > 2 {
            cc.arg(&args[i]);
        }
        i += 1;
    }
    let out = cc.output().map_err(|e| no_compiler(tr!("C 컴파일러를 실행할 수 없습니다", "cannot run the C compiler"), e))?;
    for o in &objs {
        let _ = std::fs::remove_file(o);
    }
    let err_text = String::from_utf8_lossy(&out.stderr).to_string();
    eprint!("{}", err_text);
    if !out.status.success() {
        // Report link failures (library not found) separately from Siskin generating invalid C.
        let link_fail = err_text.contains("undefined reference")
            || err_text.contains("cannot find -l")
            || err_text.contains("ld returned")
            || err_text.contains("library not found");
        if link_fail {
            return Err(tr!(
                "라이브러리를 이어 붙이지 못했습니다.\n\
                 라이브러리가 깔려 있는지, `link \"이름\"` 이 맞는지 확인하세요.\n\
                 (예: zlib 이면 `link \"z\"`, 그리고 `apt install zlib1g-dev`)",
                "failed to link the library.\n\
                 check that the library is installed and that `link \"name\"` is correct.\n\
                 (e.g. for zlib: `link \"z\"`, and `apt install zlib1g-dev`)"
            )
            .to_string());
        }
        return Err(tr!(
            "Siskin 컴파일러 내부 오류: 만든 C 코드를 C 컴파일러가 받아들이지 않았습니다.\n\
             프로그램 잘못이 아니라 Siskin 의 버그입니다. `siskin run` 으로는 돌릴 수 있습니다.\n\
             위 C 오류의 줄 번호가 .skn 파일의 어디쯤인지 알려 줍니다. 이 파일과 함께 알려 주세요.",
            "Siskin internal error: the C compiler rejected the generated C code.\n\
             this is a bug in Siskin, not in your program; `siskin run` can still run it.\n\
             the line numbers in the C errors above point to where in the .skn file it happened. please report it along with this file."
        )
        .to_string());
    }
    if let Some(p) = cpp_path {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

/// Called when running a program that uses C libraries with `siskin run`.
///
/// The interpreter cannot call C functions, because they have to be actually
/// linked. So it silently compiles natively and runs the result. This way
/// `siskin run` and `siskin build` always give the same result.
fn run_via_native(
    prog: &ast::Program,
    path: &str,
    src: &str,
    files: &[&String],
    args: &[String],
) -> ExitCode {
    let (csrc, links, cppsrc) = match cgen::generate(prog, path) {
        Ok(c) => c,
        Err(errs) => {
            for e in &errs {
                eprint!("{}", e.render(path, src));
            }
            eprintln!("{}", error_count(errs.len()));
            return ExitCode::FAILURE;
        }
    };
    let dir = std::env::temp_dir().join("siskin-run");
    let _ = std::fs::create_dir_all(&dir);
    let stem = std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "a".into());
    let cpath = dir.join(format!("{}.c", stem));
    let exe = dir.join(format!("{}{}", stem, std::env::consts::EXE_SUFFIX));
    if let Err(msg) = compile_native(
        &csrc,
        cppsrc.as_ref(),
        &links,
        args,
        &cpath,
        &exe,
        false,
        "-O1",
    ) {
        eprintln!("{}", msg);
        return ExitCode::FAILURE;
    }
    let mut run = std::process::Command::new(&exe);
    for a in files.iter().skip(1) {
        run.arg(a.as_str());
    }
    match run.status() {
        Ok(st) => {
            let _ = std::fs::remove_file(&cpath);
            match st.code() {
                Some(0) => ExitCode::SUCCESS,
                Some(c) => ExitCode::from((c & 0xff) as u8),
                None => ExitCode::FAILURE,
            }
        }
        Err(e) => {
            eprintln!("{}: {}", tr!("컴파일한 프로그램을 실행할 수 없습니다", "cannot run the compiled program"), e);
            ExitCode::from(2)
        }
    }
}

/// Native mode of `siskin debug`: compile with stop points, run as a child and follow it.
fn debug_native(
    prog: &ast::Program,
    path: &str,
    src: &str,
    files: &[&String],
    args: &[String],
    d: &mut debug::Debugger,
) -> ExitCode {
    let (csrc, links, cppsrc) = match cgen::generate_debug(prog, path) {
        Ok(c) => c,
        Err(errs) => {
            for e in &errs {
                eprint!("{}", e.render(path, src));
            }
            eprintln!("{}", error_count(errs.len()));
            return ExitCode::FAILURE;
        }
    };
    let dir = std::env::temp_dir().join("siskin-debug");
    let _ = std::fs::create_dir_all(&dir);
    let stem = std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "a".into());
    let cpath = dir.join(format!("{}.c", stem));
    let exe = dir.join(format!("{}{}", stem, std::env::consts::EXE_SUFFIX));
    if let Err(msg) = compile_native(&csrc, cppsrc.as_ref(), &links, args, &cpath, &exe, false, "-O0") {
        eprintln!("{}", msg);
        return ExitCode::FAILURE;
    }
    let pargs: Vec<String> = files.iter().skip(1).map(|s| (*s).clone()).collect();
    let code = debug::run_native(&exe, &pargs, d, prog);
    let _ = std::fs::remove_file(&cpath);
    if !d.quit {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        eprintln!("{}", tr!("프로그램이 끝났습니다", "program finished"));
    }
    ExitCode::from((code & 0xff) as u8)
}

/// Error count text: `N errors` (or its Korean form).
fn error_count(n: usize) -> String {
    tr!(format!("오류 {}개", n), format!("{} error{}", n, if n == 1 { "" } else { "s" }))
}

/// Reason given when a C header function name clashes with a Siskin builtin name.
#[allow(non_snake_case)]
fn SHADOWED_WHY() -> &'static str {
    tr!(
        "Siskin 에 같은 이름이 이미 있어서 Siskin 쪽을 씁니다",
        "Siskin already has something with this name, so Siskin's is used"
    )
}

fn usage() -> &'static str {
    tr!(USAGE_KO, USAGE_EN)
}

const USAGE_EN: &str = "\
Siskin P1 interpreter

Usage:
  siskin run <file.skn>       run a program
  siskin check <file.skn>     check syntax and types without running
  siskin check --json <file>  emit machine-readable diagnostics (design doc §9.2)
  siskin build <file.skn>     compile to a native executable
      -o <name>               output file name (default: source name)
      --emit-c                write out the C code and stop
      --release               drop contract checks and optimize
      --debug                 let gdb step through .skn lines
  siskin test <file.skn>      run the >>> examples in docstrings (§9.6)
  siskin tokens <file.skn>    print tokens (for debugging)
  siskin ffi <header.h>       show what you can use from that library
      --cpp                   read it as a C++ header
      --from <dir>            folder to look for the header in
      --all                   list every imported function
  siskin fmt <file|dir>...    format code (indentation, spacing)
      --check                 only report files that need formatting, and fail if any do
      --stdout                leave files alone and print the result
  siskin debug <file.skn>     step through line by line and inspect values (debugger)
      -b <line>               stop at that line (may be repeated)
  siskin new <name>           create a new project folder
  siskin add <name> [source]  add a package: by name from the package registry,
                              or from a git URL or folder (--rev tag)
  siskin search [words]       search the package registry
  siskin publish              show how to list this package in the registry
  siskin install              fetch the packages in siskin.toml (at the versions in siskin.lock)
  siskin update               upgrade packages to newer versions
  siskin remove <name>        remove a package
  siskin lsp                  language server for editors (launched by VS Code, Neovim, etc.)
  siskin version

Global options:
  --lang ko                   show messages in Korean (or set SISKIN_LANG=ko); English is the default
";

const USAGE_KO: &str = "\
Siskin P1 인터프리터

사용법:
  siskin run <파일.skn>       프로그램을 실행합니다
  siskin check <파일.skn>     실행하지 않고 문법과 타입을 검사합니다
  siskin check --json <파일>  기계가 읽는 진단을 냅니다 (설계 문서 §9.2)
  siskin build <파일.skn>     네이티브 실행 파일로 컴파일합니다
      -o <이름>               출력 파일 이름 (기본: 소스 이름)
      --emit-c                C 코드만 내보내고 멈춥니다
      --release               계약 검사를 빼고 최적화합니다
      --debug                 gdb 로 .skn 줄을 따라갈 수 있게 만듭니다
  siskin test <파일.skn>      docstring 안의 >>> 예제를 실행합니다 (§9.6)
  siskin tokens <파일.skn>    토큰을 출력합니다 (디버그용)
  siskin ffi <헤더.h>        그 라이브러리에서 뭘 쓸 수 있는지 봅니다
      --cpp                   C++ 헤더로 읽습니다
      --from <폴더>           헤더를 찾을 폴더
      --all                   가져온 함수 이름을 전부 보여줍니다
  siskin fmt <파일|폴더>...    코드 모양을 정리합니다 (들여쓰기, 띄어쓰기)
      --check                 고칠 곳이 있으면 알려만 주고 실패로 끝납니다
      --stdout                파일은 그대로 두고 결과를 화면에 냅니다
  siskin debug <파일.skn>     한 줄씩 따라가며 값을 봅니다 (디버거)
      -b <줄>                 그 줄에서 멈춥니다 (여러 번 써도 됩니다)
  siskin new <이름>           새 프로젝트 폴더를 만듭니다
  siskin add <이름> [주소]    패키지를 씁니다. 이름만 주면 패키지 목록에서 찾고,
                              git 주소나 폴더를 줄 수도 있습니다 (--rev 태그)
  siskin search [단어]        패키지 목록에서 찾습니다
  siskin publish              내 패키지를 목록에 올리는 방법을 알려 줍니다
  siskin install              siskin.toml 의 패키지를 받아 옵니다 (siskin.lock 의 판 그대로)
  siskin update               패키지를 새 판으로 올립니다
  siskin remove <이름>        패키지를 뺍니다
  siskin lsp                  에디터용 언어 서버 (VS Code, Neovim 등이 띄웁니다)
  siskin version

공통 옵션:
  --lang ko                   메시지를 한국어로 보여 줍니다 (SISKIN_LANG=ko 도 됩니다). 기본은 영어입니다
";

#[cfg(unix)]
extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
}

fn main() -> ExitCode {
    // When the reader closes first, as in `siskin run x.skn | head`, exit quietly like a C program
    // (Rust ignores this signal by default, which made print emit a panic message).
    #[cfg(unix)]
    unsafe {
        signal(13, 0);
    }
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    lang::init(&mut args);
    if args.is_empty() {
        print!("{}", usage());
        return ExitCode::from(2);
    }

    let cmd = args[0].as_str();
    let json = args.iter().any(|a| a == "--json");
    // Keep the values of `-l name` / `-L folder` from leaking into file names.
    let files: Vec<&String> = {
        let mut out = Vec::new();
        let mut i = 1usize;
        while i < args.len() {
            let a = &args[i];
            if a == "-l" || a == "-L" || a == "-o" || a == "-b" || a == "--break" {
                i += 2;
                continue;
            }
            if !a.starts_with('-') {
                out.push(a);
            }
            i += 1;
        }
        out
    };

    if cmd == "ffi" {
        let header = match args.get(1).filter(|a| !a.starts_with('-')) {
            Some(h) => h.clone(),
            None => {
                eprintln!(
                    "{}",
                    tr!("사용법: siskin ffi <헤더.h> [--cpp] [--from <폴더>] [--follow <헤더>]", "usage: siskin ffi <header.h> [--cpp] [--from <dir>] [--follow <header>]")
                );
                return ExitCode::from(2);
            }
        };
        let cpp = args.iter().any(|a| a == "--cpp" || a == "--c++");
        let mut dirs: Vec<String> = vec![".".into()];
        let mut defines: Vec<String> = Vec::new();
        let mut follow: Vec<String> = Vec::new();
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--from" || args[i] == "--define" || args[i] == "--follow" {
                if let Some(d) = args.get(i + 1) {
                    match args[i].as_str() {
                        "--from" => dirs.push(d.clone()),
                        "--define" => defines.push(d.clone()),
                        _ => follow.push(d.clone()),
                    }
                }
                i += 2;
                continue;
            }
            i += 1;
        }
        let im = match cheader::import_header(&header, cpp, &dirs, &[], &defines, &follow) {
            Ok(x) => x,
            Err(msg) => {
                eprintln!("{}", msg);
                return ExitCode::FAILURE;
            }
        };
        let shadowed: Vec<&String> = im
            .fns
            .iter()
            .map(|f| &f.name)
            .filter(|n| types::SISKIN_BUILTINS.contains(&n.as_str()))
            .collect();
        let usable = im.fns.len() - shadowed.len();
        let total = im.fns.len() + im.skipped.len();
        println!("{}  ({})", header, im.header_path);
        if total > 0 {
            println!(
                "  {}: {} / {} ({}%)",
                tr!("바로 쓸 수 있는 함수", "functions usable as-is"),
                usable,
                total,
                usable * 100 / total
            );
        }
        let fields: usize = im.structs.iter().map(|s| s.fields.len()).sum();
        let skipped_fields: usize = im.structs.iter().map(|s| s.skipped.len()).sum();
        println!("  {}: {}", tr!("상수 (#define, enum, static const)", "constants (#define, enum, static const)"), im.consts.len());
        println!(
            "  {}: {} ({} {}, {} {})",
            tr!("구조체", "structs"),
            im.structs.len(),
            fields,
            tr!("필드", "fields"),
            skipped_fields,
            tr!("필드 못 가져옴", "fields skipped")
        );
        if im.skipped_fnptrs.is_empty() {
            println!("  {}: {}", tr!("함수 포인터 타입", "function pointer types"), im.fnptrs.len());
        } else {
            println!(
                "  {}: {} ({} {})",
                tr!("함수 포인터 타입", "function pointer types"),
                im.fnptrs.len(),
                im.skipped_fnptrs.len(),
                tr!("못 가져옴", "skipped")
            );
        }
        if im.files.len() > 1 {
            println!("  {}: {}", tr!("따라 읽은 헤더", "headers followed"), im.files.len());
        }
        let mut why: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for (_, w) in &im.skipped {
            *why.entry(w.clone()).or_insert(0) += 1;
        }
        if !shadowed.is_empty() {
            *why.entry(SHADOWED_WHY().into()).or_insert(0) +=
                shadowed.len();
        }
        if !why.is_empty() {
            println!("  {}", tr!("아직 못 가져오는 것:", "not importable yet:"));
            for (w, n) in &why {
                if lang::ko() {
                    println!("    {:3}개  {}", n, w);
                } else {
                    println!("    {:3}  {}", n, w);
                }
            }
        }
        if !im.skipped_fnptrs.is_empty() {
            println!("  {}", tr!("못 가져온 함수 포인터 타입:", "function pointer types not importable yet:"));
            let all = args.iter().any(|a| a == "--all");
            for (n, w) in im.skipped_fnptrs.iter().take(if all { usize::MAX } else { 20 }) {
                println!("    {}: {}", n, w);
            }
            if !all && im.skipped_fnptrs.len() > 20 {
                println!("    ... {} {}", im.skipped_fnptrs.len() - 20, tr!("개 더 (--all 로 모두 보기)", "more (--all shows all)"));
            }
        }
        // Functions in headers pulled in with `#include <...>`, grouped by file, most first.
        if !im.hidden.is_empty() {
            let mut by_file: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
            for (_, f) in &im.hidden {
                *by_file.entry(f.as_str()).or_insert(0) += 1;
            }
            let mut v: Vec<(&str, usize)> = by_file.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            println!(
                "  {}",
                tr!(
                    "따라가지 않은 헤더의 함수 (`follow \"이름\"` 으로 가져옴):",
                    "functions in headers not followed (import them with `follow \"name\"`):"
                )
            );
            let all = args.iter().any(|a| a == "--all");
            for (f, n) in v.iter().take(if all { usize::MAX } else { 10 }) {
                println!("    {:4}  {}", n, f);
            }
            if !all && v.len() > 10 {
                println!("    ... {} {}", v.len() - 10, tr!("개 더 (--all 로 모두 보기)", "more (--all shows all)"));
            }
        }
        if args.iter().any(|a| a == "--all") {
            println!("  {}", tr!("가져온 함수:", "imported functions:"));
            for f in &im.fns {
                if types::SISKIN_BUILTINS.contains(&f.name.as_str()) {
                    continue;
                }
                let ps: Vec<String> = f
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, t)| match f.tys.get(i) {
                        Some(text) => text.clone(),
                        None => {
                            if f.cbs.get(i).map(|c| c.is_some()).unwrap_or(false) {
                                tr!("함수", "fn").to_string()
                            } else {
                                t.siskin().to_string()
                            }
                        }
                    })
                    .collect();
                let r = if !f.ret_ty.is_empty() {
                    if f.ret_ty == "Unit" { String::new() } else { format!(" -> {}", f.ret_ty) }
                } else if f.ret == cheader::MTy::Unit {
                    String::new()
                } else {
                    format!(" -> {}", f.ret.siskin())
                };
                println!("    {}({}){}", f.name, ps.join(", "), r);
            }
            if !im.consts.is_empty() {
                println!("  {}", tr!("가져온 상수:", "imported constants:"));
                for c in &im.consts {
                    let v = match &c.val {
                        cheader::CVal::Int(i) => i.to_string(),
                        cheader::CVal::Float(f) => format!("{:?}", f),
                        cheader::CVal::Str(s) => format!("{:?}", s),
                    };
                    println!("    {} = {}", c.name, v);
                }
            }
            if !im.structs.is_empty() {
                println!("  {}", tr!("가져온 구조체:", "imported structs:"));
                for st in &im.structs {
                    let fs: Vec<String> = st.fields.iter().map(|f| format!("{}: {}", f.name, f.ty)).collect();
                    println!("    {}{}({})", if st.is_union { "union " } else { "" }, st.name, fs.join(", "));
                    for (n, w) in &st.skipped {
                        println!("      - {}: {}", n, w);
                    }
                }
            }
            if !im.fnptrs.is_empty() {
                println!("  {}", tr!("함수 포인터 타입:", "function pointer types:"));
                for p in &im.fnptrs {
                    println!(
                        "    {}({})",
                        p.name,
                        p.sig.params.iter().map(|m| m.siskin()).collect::<Vec<_>>().join(", ")
                    );
                }
            }
        }
        return ExitCode::SUCCESS;
    }

    if matches!(cmd, "new" | "add" | "install" | "update" | "remove" | "search" | "publish") {
        return pkg::cli(cmd, &args);
    }

    if cmd == "lsp" {
        return ExitCode::from(lsp::run() as u8);
    }

    if cmd == "fmt" {
        return run_fmt(&files, &args);
    }

    if cmd == "version" {
        println!("{}", tr!("siskin 0.1.0 (P1 인터프리터)", "siskin 0.1.0 (P1 interpreter)"));
        return ExitCode::SUCCESS;
    }

    let path = match files.first() {
        Some(p) => (*p).clone(),
        None => {
            print!("{}", usage());
            return ExitCode::from(2);
        }
    };

    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: {} ({})", tr!("파일을 읽을 수 없습니다", "cannot read file"), path, e);
            return ExitCode::from(2);
        }
    };

    match cmd {
        "tokens" => match lexer::tokenize(&src) {
            Ok(toks) => {
                for t in toks {
                    println!("{:>4}:{:<3} {}", t.line, t.col, t.tok);
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprint!("{}", e.render(&path, &src));
                ExitCode::FAILURE
            }
        },

        "check" => {
            let mut prog = match parser::parse(&src) {
                Ok(p) => p,
                Err(e) => {
                    if json {
                        println!("{{\"file\":\"{}\",\"diagnostics\":[{}]}}", path, e.with_best_col(&src).to_json(&path));
                    } else {
                        eprint!("{}", e.render(&path, &src));
                    }
                    return ExitCode::FAILURE;
                }
            };
            if resolve_imports(&mut prog, &path).is_err() {
                return ExitCode::FAILURE;
            }
            let fixes = casefold::fold(&mut prog);
            let mut ty = types::Types::new(&prog);
            ty.check_program(&prog);
            if ty.errors.is_empty() {
                for f in &fixes {
                    if lang::ko() {
                        println!(
                            "참고: {}:{} `{}` -> `{}` (대소문자를 맞춰 두었습니다)",
                            f.line, f.col, f.typed, f.canonical
                        );
                    } else {
                        println!(
                            "note: {}:{} `{}` -> `{}` (letter case corrected)",
                            f.line, f.col, f.typed, f.canonical
                        );
                    }
                }
                if json {
                    let mine = prog
                        .stmts
                        .iter()
                        .filter(|s| match s {
                            ast::Stmt::CHeader { .. } => false,
                            ast::Stmt::Fn(f) => f.c_sig.is_none(),
                            _ => true,
                        })
                        .count();
                    let ws: Vec<String> =
                        error::take_warnings().iter().map(|w| w.with_best_col(&src).to_json(&path)).collect();
                    println!("{{\"file\":\"{}\",\"diagnostics\":[{}],\"decls\":{}}}", path, ws.join(","), mine);
                } else {
                    // Declarations imported automatically from headers are not counted; only hand-written ones.
                    let mine = prog
                        .stmts
                        .iter()
                        .filter(|s| match s {
                            ast::Stmt::CHeader { .. } => false,
                            ast::Stmt::Fn(f) => f.c_sig.is_none(),
                            _ => true,
                        })
                        .count();
                    show_warnings(&path, &src);
                    if lang::ko() {
                        println!("검사 통과: 최상위 선언 {}개", mine);
                    } else {
                        println!(
                            "check passed: {} top-level declaration{}",
                            mine,
                            if mine == 1 { "" } else { "s" }
                        );
                    }
                }
                ExitCode::SUCCESS
            } else {
                if json {
                    let mut all = ty.errors.clone();
                    all.extend(error::take_warnings());
                    let items: Vec<String> = all.iter().map(|e| e.with_best_col(&src).to_json(&path)).collect();
                    println!("{{\"file\":\"{}\",\"diagnostics\":[{}]}}", path, items.join(","));
                } else {
                    show_warnings(&path, &src);
                    for e in &ty.errors {
                        eprint!("{}", e.render(&path, &src));
                    }
                    eprintln!("{}", error_count(ty.errors.len()));
                }
                ExitCode::FAILURE
            }
        }

        "run" | "debug" => {
            let mut prog = match parser::parse(&src) {
                Ok(p) => p,
                Err(e) => {
                    if json {
                        println!("{{\"file\":\"{}\",\"diagnostics\":[{}]}}", path, e.with_best_col(&src).to_json(&path));
                    } else {
                        eprint!("{}", e.render(&path, &src));
                    }
                    return ExitCode::FAILURE;
                }
            };
            if resolve_imports(&mut prog, &path).is_err() {
                return ExitCode::FAILURE;
            }
            casefold::fold(&mut prog);
            if !args.iter().any(|a| a == "--no-check") {
                let mut ty = types::Types::new(&prog);
                ty.check_program(&prog);
                if !ty.errors.is_empty() {
                    if json {
                        let items: Vec<String> = ty.errors.iter().map(|e| e.with_best_col(&src).to_json(&path)).collect();
                        println!("{{\"file\":\"{}\",\"diagnostics\":[{}]}}", path, items.join(","));
                    } else {
                        for e in &ty.errors {
                            eprint!("{}", e.render(&path, &src));
                        }
                        eprintln!("{}", error_count(ty.errors.len()));
                    }
                    return ExitCode::FAILURE;
                }
                if !json {
                    show_warnings(&path, &src);
                }
            }
            let mut dbg = None;
            if cmd == "debug" {
                let mut breaks = std::collections::BTreeSet::new();
                let mut i = 0;
                while i < args.len() {
                    if args[i] == "-b" || args[i] == "--break" {
                        if let Some(b) = args.get(i + 1) {
                            match debug::parse_break(b) {
                                Ok(n) => {
                                    breaks.insert(n);
                                }
                                Err(m) => {
                                    eprintln!("{}", m);
                                    return ExitCode::from(2);
                                }
                            }
                        }
                        i += 1;
                    }
                    i += 1;
                }
                if lang::ko() {
                    eprintln!("siskin 디버거: {} 을(를) 한 줄씩 따라갑니다. 명령 목록은 h", path);
                } else {
                    eprintln!("siskin debugger: stepping through {} line by line. type h for commands", path);
                }
                dbg = Some(debug::Debugger::new(&path, &src, breaks));
            }
            // Programs that use C functions need linking, so run them natively.
            // In that case the debugger also compiles natively with stop points and follows along.
            // Programs using `spawn` are also debugged natively (so it can stop inside tasks too).
            let native_dbg = cmd == "debug" && (args.iter().any(|a| a == "--native") || uses_spawn(&prog));
            if needs_native(&prog) || native_dbg {
                if let Some(mut d) = dbg {
                    return debug_native(&prog, &path, &src, &files, &args, &mut d);
                }
                return run_via_native(&prog, &path, &src, &files, &args);
            }
            let mut it = interp::Interp::new();
            if let Some(d) = dbg {
                it.dbg = Some(Box::new(d));
            }
            // Positional arguments after the script path are passed to the program, via `args()`.
            it.prog_args = files.iter().skip(1).map(|s| (*s).clone()).collect();
            let debugging = it.dbg.is_some();
            conc::set_source(&path, &src);
            let result = it.run_program(&prog);
            if debugging {
                use std::io::Write;
                let _ = std::io::stdout().flush();
                eprintln!("{}", tr!("프로그램이 끝났습니다", "program finished"));
            }
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    if json {
                        println!("{{\"file\":\"{}\",\"diagnostics\":[{}]}}", path, e.with_best_col(&src).to_json(&path));
                    } else {
                        eprint!("{}", e.render(&path, &src));
                    }
                    ExitCode::FAILURE
                }
            }
        }

        "build" => {
            let mut prog = match parser::parse(&src) {
                Ok(p) => p,
                Err(e) => {
                    eprint!("{}", e.render(&path, &src));
                    return ExitCode::FAILURE;
                }
            };
            if resolve_imports(&mut prog, &path).is_err() {
                return ExitCode::FAILURE;
            }
            casefold::fold(&mut prog);
            let mut ty = types::Types::new(&prog);
            ty.check_program(&prog);
            show_warnings(&path, &src);
            if !ty.errors.is_empty() {
                for e in &ty.errors {
                    eprint!("{}", e.render(&path, &src));
                }
                eprintln!("{}", error_count(ty.errors.len()));
                return ExitCode::FAILURE;
            }
            let (csrc, links, cppsrc) = match cgen::generate(&prog, &path) {
                Ok(c) => c,
                Err(errs) => {
                    for e in &errs {
                        eprint!("{}", e.render(&path, &src));
                    }
                    if lang::ko() {
                        eprintln!("네이티브 컴파일 오류 {}개", errs.len());
                    } else {
                        eprintln!("{} native build error{}", errs.len(), if errs.len() == 1 { "" } else { "s" });
                    }
                    return ExitCode::FAILURE;
                }
            };

            let stem = std::path::Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "a".into());
            let outname = args
                .iter()
                .position(|a| a == "-o")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| stem.clone());
            // Windows executables must end in `.exe`.
            let outname = if cfg!(windows) && !outname.ends_with(".c") && std::path::Path::new(&outname).extension().is_none() {
                format!("{}.exe", outname)
            } else {
                outname
            };

            // Avoid `x.c.c` when given `-o x.c`.
            let cpath = if outname.ends_with(".c") {
                outname.clone()
            } else {
                format!("{}.c", outname.strip_suffix(".exe").unwrap_or(&outname))
            };
            if args.iter().any(|a| a == "--emit-c") {
                if let Err(e) = std::fs::write(&cpath, &csrc) {
                    eprintln!("{}: {}", tr!("C 파일을 쓸 수 없습니다", "cannot write C file"), e);
                    return ExitCode::from(2);
                }
                if let Some(cs) = &cppsrc {
                    let p = format!("{}.ffi.cpp", outname);
                    let _ = std::fs::write(&p, cs);
                    if lang::ko() {
                        println!("C++ 다리 파일도 {}에 썼습니다", p);
                    } else {
                        println!("also wrote the C++ bridge file to {}", p);
                    }
                }
                if lang::ko() {
                    println!("C 코드를 {}에 썼습니다 ({}줄)", cpath, csrc.lines().count());
                } else {
                    println!("wrote C code to {} ({} lines)", cpath, csrc.lines().count());
                }
                return ExitCode::SUCCESS;
            }

            let release = args.iter().any(|a| a == "--release");
            match compile_native(
                &csrc,
                cppsrc.as_ref(),
                &links,
                &args,
                std::path::Path::new(&cpath),
                std::path::Path::new(&outname),
                release,
                "-O2",
            ) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&cpath);
                    println!(
                        "{}: {}{}",
                        tr!("컴파일 완료", "compiled"),
                        if outname.contains('/') || outname.contains('\\') {
                            ""
                        } else if cfg!(windows) {
                            ".\\"
                        } else {
                            "./"
                        },
                        outname
                    );
                    ExitCode::SUCCESS
                }
                Err(msg) => {
                    eprintln!("{}", msg);
                    if lang::ko() {
                        eprintln!("생성된 C 코드는 {}에 남겨둡니다", cpath);
                    } else {
                        eprintln!("generated C code kept at {}", cpath);
                    }
                    ExitCode::FAILURE
                }
            }
        }

        "test" => {
            let mut prog = match parser::parse(&src) {
                Ok(p) => p,
                Err(e) => {
                    eprint!("{}", e.render(&path, &src));
                    return ExitCode::FAILURE;
                }
            };
            if resolve_imports(&mut prog, &path).is_err() {
                return ExitCode::FAILURE;
            }
            casefold::fold(&mut prog);
            let mut it = interp::Interp::new();
            let (pass, failed, msgs) = it.run_doctests(&prog);
            for m in &msgs {
                println!("{}", m);
            }
            if failed == 0 {
                if lang::ko() {
                    println!("doctest {}개 통과", pass);
                } else {
                    println!("{} doctest{} passed", pass, if pass == 1 { "" } else { "s" });
                }
                ExitCode::SUCCESS
            } else {
                if lang::ko() {
                    println!("doctest {}개 통과, {}개 실패", pass, failed);
                } else {
                    println!("{} doctest{} passed, {} failed", pass, if pass == 1 { "" } else { "s" }, failed);
                }
                ExitCode::FAILURE
            }
        }

        other => {
            eprintln!("{}: {}\n", tr!("알 수 없는 명령", "unknown command"), other);
            print!("{}", usage());
            ExitCode::from(2)
        }
    }
}

/// Print collected warnings to stderr (so they don't mix with program output).
fn show_warnings(path: &str, src: &str) {
    for w in error::take_warnings() {
        eprint!("{}", w.render(path, src));
    }
}
