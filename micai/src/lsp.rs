//! `siskin lsp` — 에디터와 이야기하는 언어 서버 (Language Server Protocol).
//!
//! VS Code, Neovim, Helix, Zed 같은 에디터가 이 프로그램을 띄워 두고 표준 입출력으로
//! JSON 을 주고받습니다. 첫 버전이 하는 일:
//! - 오류 밑줄: 글자를 칠 때마다 `siskin check` 와 같은 검사를 돌려 알려 줍니다.
//! - 정리: 에디터의 "문서 서식" 명령이 `siskin fmt` 를 부릅니다.
//! - 목차: 파일 안의 함수·구조체·enum 목록 (개요 창, 기호로 이동).
//! - 정의로 이동: 함수·구조체 이름에서 그 선언으로 (같은 파일과 import 한 파일).
//! - 마우스를 올리면: 함수의 모양(시그니처)과 바로 위 주석을 보여 줍니다.

use crate::ast::Stmt;
use crate::error::SiskinError;
use crate::json::{self, JRef, JsonVal};
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};

fn get(v: &JRef, key: &str) -> Option<JRef> {
    match &*v.borrow() {
        JsonVal::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
        _ => None,
    }
}

fn get_str(v: &JRef, key: &str) -> Option<String> {
    get(v, key).and_then(|x| match &*x.borrow() {
        JsonVal::Str(s) => Some(s.clone()),
        _ => None,
    })
}

fn get_int(v: &JRef, key: &str) -> Option<i64> {
    get(v, key).and_then(|x| match &*x.borrow() {
        JsonVal::Int(n) => Some(*n),
        JsonVal::Float(f) => Some(*f as i64),
        _ => None,
    })
}

fn q(s: &str) -> String {
    let mut o = String::new();
    json::escape(s, &mut o); // 따옴표까지 붙여 줍니다
    o
}

fn send(out: &mut impl Write, body: &str) {
    let _ = write!(out, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = out.flush();
}

fn reply(out: &mut impl Write, id: &str, result: &str) {
    send(out, &format!("{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{}}}", id, result));
}

fn uri_to_path(uri: &str) -> String {
    let rest = uri.strip_prefix("file://").unwrap_or(uri);
    let bytes = rest.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let p = String::from_utf8_lossy(&out).to_string();
    // 윈도우: file:///C:/... → C:/...
    if p.len() > 2 && p.as_bytes()[0] == b'/' && p.as_bytes()[2] == b':' {
        return p[1..].to_string();
    }
    p
}

fn path_to_uri(p: &str) -> String {
    let mut o = String::from("file://");
    if !p.starts_with('/') {
        o.push('/');
    }
    for b in p.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~:".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{:02X}", b));
        }
    }
    o
}

/// Siskin 의 열(글자 단위, 1부터)을 LSP 의 열(UTF-16 단위, 0부터)로 바꿉니다.
fn utf16_col(line_text: &str, col1: usize) -> usize {
    line_text.chars().take(col1.saturating_sub(1)).map(|c| c.len_utf16()).sum()
}

fn char_col_from_utf16(line_text: &str, u16col: usize) -> usize {
    let mut n = 0;
    for (i, c) in line_text.chars().enumerate() {
        if n >= u16col {
            return i;
        }
        n += c.len_utf16();
    }
    line_text.chars().count()
}

/// 오류 자리에서 시작하는 이름(또는 글자 하나)의 끝.
fn span_end(line_text: &str, col1: usize) -> usize {
    let chars: Vec<char> = line_text.chars().collect();
    let start = col1.saturating_sub(1);
    let mut end = start;
    while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
        end += 1;
    }
    if end == start {
        end = (start + 1).min(chars.len().max(start));
    }
    end + 1
}

fn range_json(text: &str, line: usize, col: usize, end_col: usize) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let l0 = line.saturating_sub(1).min(lines.len().saturating_sub(1));
    let lt = lines.get(l0).copied().unwrap_or("");
    let c0 = utf16_col(lt, col.max(1));
    let c1 = utf16_col(lt, end_col.max(col.max(1)));
    format!(
        "{{\"start\":{{\"line\":{},\"character\":{}}},\"end\":{{\"line\":{},\"character\":{}}}}}",
        l0, c0, l0, c1.max(c0)
    )
}

fn diag_json(text: &str, e: &SiskinError, severity: u8) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let lt = lines.get(e.line.saturating_sub(1)).copied().unwrap_or("");
    let end = span_end(lt, e.col.max(1));
    let mut msg = e.msg.clone();
    if let Some(f) = &e.fix {
        msg.push_str(tr!("\n도움말: ", "\nhelp: "));
        msg.push_str(f);
    }
    format!(
        "{{\"range\":{},\"severity\":{},\"code\":{},\"source\":\"siskin\",\"message\":{}}}",
        range_json(text, e.line, e.col, end),
        severity,
        q(e.code),
        q(&msg)
    )
}

/// import 한 다른 파일 안의 오류를 import 줄에 붙일 때의 글.
fn in_other_file(file: &str, line: usize, msg: &str) -> String {
    tr!(format!("{} {}번째 줄: {}", file, line, msg), format!("{} line {}: {}", file, line, msg))
}

/// 파일 하나를 검사해서 진단 목록(JSON 배열 안쪽)을 만듭니다. `siskin check` 와 같은 검사입니다.
fn diagnose(path: &str, text: &str) -> Vec<String> {
    let text = text.to_string();
    let path = path.to_string();
    let r = std::panic::catch_unwind(move || {
        let mut out = Vec::new();
        let mut prog = match crate::parser::parse(&text) {
            Ok(p) => p,
            Err(e) => {
                out.push(diag_json(&text, &e, 1));
                return out;
            }
        };
        if let Err((p, _src, e)) = crate::resolve_imports_err(&mut prog, &path) {
            if p == path {
                out.push(diag_json(&text, &e, 1));
            } else {
                // 다른 파일 안의 오류는 import 줄에 붙입니다.
                let short = std::path::Path::new(&p)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or(p.clone());
                let stem = short.trim_end_matches(".skn").to_string();
                let at = text
                    .split('\n')
                    .position(|l| l.trim_start().starts_with("import") && l.contains(&stem))
                    .map(|i| i + 1)
                    .unwrap_or(1);
                let shown_line = crate::error::locate(e.line).map(|x| x.2).unwrap_or(e.line);
                let e2 = SiskinError::new(e.code, in_other_file(&short, shown_line, &e.msg), at, 1);
                out.push(diag_json(&text, &e2, 1));
            }
            return out;
        }
        let fixes = crate::casefold::fold(&mut prog);
        let mut ty = crate::types::Types::new(&prog);
        ty.check_program(&prog);
        let nlines = text.split('\n').count();
        for e in &ty.errors {
            // import 한 파일 안의 오류는 그 import 줄에 붙입니다. 표준 라이브러리 조각은 건너뜁니다.
            if let Some((f, _, l)) = crate::error::locate(e.line) {
                if f.starts_with("<std.") {
                    continue;
                }
                let short = std::path::Path::new(&f)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or(f.clone());
                let stem = short.trim_end_matches(".skn").to_string();
                let at = text
                    .split('\n')
                    .position(|x| (x.trim_start().starts_with("import") || x.trim_start().starts_with("from")) && x.contains(&stem))
                    .map(|i| i + 1)
                    .unwrap_or(1);
                let e2 = SiskinError::new(e.code, in_other_file(&short, l, &e.msg), at, 1);
                out.push(diag_json(&text, &e2, 1));
                continue;
            }
            if e.line > nlines {
                continue;
            }
            out.push(diag_json(&text, e, 1));
        }
        for f in &fixes {
            let e = SiskinError::new(
                "note",
                tr!(
                    format!("`{}` 을(를) `{}` 로 읽었습니다 (대소문자를 맞춰 두었습니다)", f.typed, f.canonical),
                    format!("read `{}` as `{}` (letter case corrected)", f.typed, f.canonical)
                ),
                f.line,
                f.col,
            );
            out.push(diag_json(&text, &e, 3));
        }
        out
    });
    r.unwrap_or_default()
}

/// 이 줄의 이 자리(글자 단위)에 있는 이름.
fn word_at(line_text: &str, ch: usize) -> Option<String> {
    let chars: Vec<char> = line_text.chars().collect();
    let is_w = |c: char| c.is_alphanumeric() || c == '_';
    let mut s = ch.min(chars.len());
    if s == chars.len() || !is_w(chars[s]) {
        if s > 0 && is_w(chars[s - 1]) {
            s -= 1;
        } else {
            return None;
        }
    }
    let mut a = s;
    while a > 0 && is_w(chars[a - 1]) {
        a -= 1;
    }
    let mut b = s;
    while b < chars.len() && is_w(chars[b]) {
        b += 1;
    }
    Some(chars[a..b].iter().collect())
}

struct Decl {
    name: String,
    kind: u8,
    line: usize,
    children: Vec<Decl>,
}

fn decls_of(text: &str) -> Vec<Decl> {
    let prog = match crate::parser::parse(text) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for s in &prog.stmts {
        match s {
            Stmt::Fn(f) if !f.is_lambda() => out.push(Decl { name: f.name.clone(), kind: 12, line: f.line, children: vec![] }),
            Stmt::Struct(st) => out.push(Decl {
                name: st.name.clone(),
                kind: 23,
                line: st.line,
                children: st.methods.iter().map(|m| Decl { name: m.name.clone(), kind: 6, line: m.line, children: vec![] }).collect(),
            }),
            Stmt::Enum(en) => out.push(Decl {
                name: en.name.clone(),
                kind: 10,
                line: en.line,
                children: en.methods.iter().map(|m| Decl { name: m.name.clone(), kind: 6, line: m.line, children: vec![] }).collect(),
            }),
            Stmt::Interface(i) => out.push(Decl { name: i.name.clone(), kind: 11, line: i.line, children: vec![] }),
            _ => {}
        }
    }
    out
}

fn symbols_json(text: &str, ds: &[Decl]) -> String {
    let items: Vec<String> = ds
        .iter()
        .map(|d| {
            let lines: Vec<&str> = text.split('\n').collect();
            let lt = lines.get(d.line.saturating_sub(1)).copied().unwrap_or("");
            let len = lt.chars().count() + 1;
            let r = range_json(text, d.line, 1, len);
            format!(
                "{{\"name\":{},\"kind\":{},\"range\":{},\"selectionRange\":{},\"children\":{}}}",
                q(&d.name),
                d.kind,
                r,
                r,
                symbols_json(text, &d.children)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// 이 파일이 import 하는 내 파일들 (같은 폴더의 `이름.skn`).
fn imported_files(path: &str, text: &str) -> Vec<String> {
    let dir = std::path::Path::new(path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut out = Vec::new();
    if let Ok(prog) = crate::parser::parse(text) {
        for s in &prog.stmts {
            if let Stmt::Import { path: ip, .. } = s {
                if ip.first().map(|f| f.as_str()) != Some("std") && !ip.is_empty() {
                    // 같은 폴더의 파일, 아니면 패키지(`siskin add` 로 넣은 것)
                    crate::pkg::set_project_root(std::path::Path::new(path));
                    let f = match crate::pkg::resolve_module(&dir, ip) {
                        Ok(f) => f,
                        Err(_) => continue,
                    };
                    if f.exists() {
                        out.push(f.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    out
}

/// 이름의 선언을 찾습니다: (파일, 줄).
fn find_decl(path: &str, text: &str, name: &str, docs: &HashMap<String, String>) -> Option<(String, String, usize)> {
    let mut places: Vec<(String, String)> = vec![(path.to_string(), text.to_string())];
    for f in imported_files(path, text) {
        let t = docs
            .get(&path_to_uri(&f))
            .cloned()
            .or_else(|| std::fs::read_to_string(&f).ok())
            .unwrap_or_default();
        places.push((f, t));
    }
    for (p, t) in &places {
        for d in decls_of(t) {
            if d.name == name {
                return Some((p.clone(), t.clone(), d.line));
            }
            for c in &d.children {
                if c.name == name {
                    return Some((p.clone(), t.clone(), c.line));
                }
            }
        }
    }
    None
}

fn hover_text(text: &str, line: usize) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let sig = lines.get(line.saturating_sub(1)).copied().unwrap_or("").trim().trim_end_matches(':').to_string();
    // 바로 위의 주석 줄들을 설명으로 붙입니다.
    let mut doc: Vec<String> = Vec::new();
    let mut i = line.saturating_sub(1);
    while i > 0 {
        let l = lines[i - 1].trim();
        if let Some(c) = l.strip_prefix('#') {
            doc.push(c.trim().to_string());
            i -= 1;
        } else {
            break;
        }
    }
    doc.reverse();
    let mut md = format!("```siskin\n{}\n```", sig);
    if !doc.is_empty() {
        md.push_str("\n\n");
        md.push_str(&doc.join("\n"));
    }
    md
}

pub fn run() -> i32 {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut docs: HashMap<String, String> = HashMap::new();
    let mut shutdown = false;

    loop {
        // 머리: Content-Length: N\r\n ... \r\n
        let mut len: Option<usize> = None;
        loop {
            let mut h = String::new();
            match input.read_line(&mut h) {
                Ok(0) => return if shutdown { 0 } else { 1 },
                Ok(_) => {}
                Err(_) => return 1,
            }
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().ok();
            }
        }
        let n = match len {
            Some(n) => n,
            None => continue,
        };
        let mut buf = vec![0u8; n];
        if input.read_exact(&mut buf).is_err() {
            return 1;
        }
        let body = String::from_utf8_lossy(&buf).to_string();
        let msg = match json::parse(&body) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let method = get_str(&msg, "method").unwrap_or_default();
        let id = get(&msg, "id").map(|v| json::stringify(&v));
        let params = get(&msg, "params");
        let text_doc = params.as_ref().and_then(|p| get(p, "textDocument"));
        let uri = text_doc.as_ref().and_then(|t| get_str(t, "uri")).unwrap_or_default();

        let publish = |out: &mut std::io::StdoutLock, uri: &str, text: &str| {
            let path = uri_to_path(uri);
            let ds = diagnose(&path, text);
            send(
                out,
                &format!(
                    "{{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/publishDiagnostics\",\"params\":{{\"uri\":{},\"diagnostics\":[{}]}}}}",
                    q(uri),
                    ds.join(",")
                ),
            );
        };

        match method.as_str() {
            "initialize" => {
                let caps = "{\"capabilities\":{\"textDocumentSync\":{\"openClose\":true,\"change\":1,\"save\":{\"includeText\":false}},\
                    \"documentFormattingProvider\":true,\"documentSymbolProvider\":true,\"definitionProvider\":true,\"hoverProvider\":true,\"completionProvider\":{\"triggerCharacters\":[]}},\
                    \"serverInfo\":{\"name\":\"siskin\",\"version\":\"0.1.0\"}}";
                if let Some(id) = &id {
                    reply(&mut out, id, caps);
                }
            }
            "shutdown" => {
                shutdown = true;
                if let Some(id) = &id {
                    reply(&mut out, id, "null");
                }
            }
            "exit" => return if shutdown { 0 } else { 1 },
            "textDocument/didOpen" => {
                let text = text_doc.as_ref().and_then(|t| get_str(t, "text")).unwrap_or_default();
                docs.insert(uri.clone(), text.clone());
                publish(&mut out, &uri, &text);
            }
            "textDocument/didChange" => {
                // 전체 동기화: 마지막 변경이 문서 전체입니다.
                if let Some(ch) = params.as_ref().and_then(|p| get(p, "contentChanges")) {
                    let last = match &*ch.borrow() {
                        JsonVal::List(v) => v.last().cloned(),
                        _ => None,
                    };
                    if let Some(c) = last {
                        if let Some(t) = get_str(&c, "text") {
                            docs.insert(uri.clone(), t.clone());
                            publish(&mut out, &uri, &t);
                        }
                    }
                }
            }
            "textDocument/didSave" => {
                if let Some(t) = docs.get(&uri).cloned() {
                    publish(&mut out, &uri, &t);
                }
            }
            "textDocument/didClose" => {
                docs.remove(&uri);
                send(
                    &mut out,
                    &format!(
                        "{{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/publishDiagnostics\",\"params\":{{\"uri\":{},\"diagnostics\":[]}}}}",
                        q(&uri)
                    ),
                );
            }
            "textDocument/formatting" => {
                let text = docs.get(&uri).cloned().unwrap_or_default();
                let f = crate::fmt::format(&text);
                let result = if f == text || crate::fmt::check_same(&text, &f).is_err() {
                    "[]".to_string()
                } else {
                    let nl = text.split('\n').count();
                    format!(
                        "[{{\"range\":{{\"start\":{{\"line\":0,\"character\":0}},\"end\":{{\"line\":{},\"character\":0}}}},\"newText\":{}}}]",
                        nl + 1,
                        q(&f)
                    )
                };
                if let Some(id) = &id {
                    reply(&mut out, id, &result);
                }
            }
            "textDocument/documentSymbol" => {
                let text = docs.get(&uri).cloned().unwrap_or_default();
                let r = symbols_json(&text, &decls_of(&text));
                if let Some(id) = &id {
                    reply(&mut out, id, &r);
                }
            }
            "textDocument/definition" | "textDocument/hover" => {
                let text = docs.get(&uri).cloned().unwrap_or_default();
                let pos = params.as_ref().and_then(|p| get(p, "position"));
                let line = pos.as_ref().and_then(|p| get_int(p, "line")).unwrap_or(0) as usize;
                let chr = pos.as_ref().and_then(|p| get_int(p, "character")).unwrap_or(0) as usize;
                let lt = text.split('\n').nth(line).unwrap_or("").to_string();
                let word = word_at(&lt, char_col_from_utf16(&lt, chr));
                let path = uri_to_path(&uri);
                let found = word.as_ref().and_then(|w| find_decl(&path, &text, w, &docs));
                let result = match (method.as_str(), found) {
                    ("textDocument/definition", Some((p, t, l))) => {
                        let len = t.split('\n').nth(l.saturating_sub(1)).map(|s| s.chars().count()).unwrap_or(0) + 1;
                        format!("{{\"uri\":{},\"range\":{}}}", q(&path_to_uri(&p)), range_json(&t, l, 1, len))
                    }
                    ("textDocument/hover", Some((_, t, l))) => {
                        format!("{{\"contents\":{{\"kind\":\"markdown\",\"value\":{}}}}}", q(&hover_text(&t, l)))
                    }
                    _ => "null".to_string(),
                };
                if let Some(id) = &id {
                    reply(&mut out, id, &result);
                }
            }
            "textDocument/completion" => {
                // 첫 버전: 키워드, 내장 함수, 이 파일과 import 한 파일의 이름들.
                let text = docs.get(&uri).cloned().unwrap_or_default();
                let path = uri_to_path(&uri);
                let mut items: Vec<String> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                let mut add = |label: &str, kind: u8, detail: &str| {
                    if label.starts_with("__") || !seen.insert(label.to_string()) {
                        return;
                    }
                    items.push(format!("{{\"label\":{},\"kind\":{},\"detail\":{}}}", q(label), kind, q(detail)));
                };
                let mut files = vec![(path.clone(), text.clone())];
                for f in imported_files(&path, &text) {
                    let t = std::fs::read_to_string(&f).unwrap_or_default();
                    files.push((f, t));
                }
                for (_, t) in &files {
                    for d in decls_of(t) {
                        let detail = t.split('\n').nth(d.line.saturating_sub(1)).unwrap_or("").trim().trim_end_matches(':').to_string();
                        add(&d.name, if d.kind == 12 { 3 } else { 22 }, &detail);
                    }
                }
                for b in crate::types::SISKIN_BUILTINS {
                    add(b, 3, tr!("내장 함수", "built-in function"));
                }
                for k in crate::lexer::KEYWORDS.iter().chain(["spawn"].iter()) {
                    add(k, 14, tr!("키워드", "keyword"));
                }
                for t in ["Int", "Float", "Bool", "Str", "Task", "Chan"] {
                    add(t, 22, tr!("기본 타입", "primitive type"));
                }
                if let Some(id) = &id {
                    reply(&mut out, id, &format!("[{}]", items.join(",")));
                }
            }
            _ => {
                // 모르는 요청에는 "없는 방법" 이라고 답합니다. 알림(id 없음)은 무시합니다.
                if let Some(id) = &id {
                    send(
                        &mut out,
                        &format!(
                            "{{\"jsonrpc\":\"2.0\",\"id\":{},\"error\":{{\"code\":-32601,\"message\":{}}}}}",
                            id,
                            q(&tr!(format!("지원하지 않는 요청: {}", method), format!("unsupported request: {}", method)))
                        ),
                    );
                }
            }
        }
    }
}
