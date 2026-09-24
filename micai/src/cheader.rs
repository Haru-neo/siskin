//! C / C++ 헤더 파일을 읽어 그 안의 함수를 Siskin 선언으로 바꿉니다.
//!
//! `import c "zlib.h" link "z"` 한 줄이면 zlib 함수를 전부 쓸 수 있습니다.
//! 헤더를 직접 뜯어보는 대신 `clang` 에게 물어봅니다. 매크로·typedef·
//! `#ifdef` 를 사람이 흉내 내면 반드시 틀리기 때문입니다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use crate::json::{self, JsonVal, JRef};

/// Siskin 쪽에서 쓸 타입. C의 온갖 정수 타입은 전부 Int 하나로 모읍니다.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MTy {
    Unit,
    Int,
    Float,
    Bool,
    Str,
}

impl MTy {
    pub fn siskin(&self) -> &'static str {
        match self {
            MTy::Unit => "Unit",
            MTy::Int => "Int",
            MTy::Float => "Float",
            MTy::Bool => "Bool",
            MTy::Str => "Str",
        }
    }
    /// Siskin ABI에서 쓰는 C 타입.
    pub fn cty(&self) -> &'static str {
        match self {
            MTy::Unit => "void",
            MTy::Int => "int64_t",
            MTy::Float => "double",
            MTy::Bool => "bool",
            MTy::Str => "const char*",
        }
    }
}

/// 라이브러리에 넘겨줄 함수(콜백) 한 자리의 생김새.
#[derive(Clone, Debug, Default)]
pub struct CbSig {
    pub ret: Option<MTy>,
    pub params: Vec<MTy>,
    /// C 쪽 원래 타입들. 다리 함수를 만들 때 씁니다.
    pub c_ret: String,
    pub c_params: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ImportedFn {
    /// Siskin에서 부를 이름.
    pub name: String,
    pub ret: MTy,
    pub params: Vec<MTy>,
    /// 원래 C 타입들. 껍데기 함수에서 캐스트할 때 씁니다.
    pub c_ret: String,
    pub c_params: Vec<String>,
    /// C++ 이면 통째로 만들어 둔 `extern "C"` 껍데기 함수.
    pub cpp_shim: Option<String>,
    /// 각 자리가 "여기에 결과를 넣어 달라"는 자리인지. `sqlite3_open` 의
    /// 두 번째 자리처럼 `T**` 로 받는 것들입니다. Siskin 에서는 `inout` 이 됩니다.
    pub outs: Vec<bool>,
    /// 각 자리가 "함수를 넘겨 달라"는 자리면 그 함수의 생김새.
    pub cbs: Vec<Option<CbSig>>,
}

#[derive(Clone, Debug, Default)]
pub struct Imported {
    pub fns: Vec<ImportedFn>,
    /// (이름, 못 가져온 이유)
    pub skipped: Vec<(String, String)>,
    /// clang이 찾아낸 헤더의 실제 경로.
    pub header_path: String,
}

// --------------------------------------------------------------- JSON 도우미

fn dget(v: &JRef, k: &str) -> Option<JRef> {
    match &*v.borrow() {
        JsonVal::Dict(items) => items.iter().find(|(n, _)| n == k).map(|(_, x)| x.clone()),
        _ => None,
    }
}

fn dstr(v: &JRef, k: &str) -> Option<String> {
    match dget(v, k) {
        Some(x) => match &*x.borrow() {
            JsonVal::Str(s) => Some(s.clone()),
            _ => None,
        },
        None => None,
    }
}

fn dlist(v: &JRef, k: &str) -> Vec<JRef> {
    match dget(v, k) {
        Some(x) => match &*x.borrow() {
            JsonVal::List(items) => items.clone(),
            _ => Vec::new(),
        },
        None => Vec::new(),
    }
}

/// 노드가 어느 파일에서 왔는지. clang은 앞 노드와 같은 파일이면 `file` 을
/// 아예 빼므로, 못 찾으면 직전 값을 그대로 씁니다.
fn loc_file(n: &JRef, prev: &str) -> String {
    for key in ["range", "loc"] {
        let mut o = match dget(n, key) {
            Some(x) => x,
            None => continue,
        };
        if key == "range" {
            o = match dget(&o, "begin") {
                Some(x) => x,
                None => continue,
            };
        }
        for sub in ["expansionLoc", "spellingLoc"] {
            if let Some(l) = dget(&o, sub) {
                if let Some(f) = dstr(&l, "file") {
                    return f;
                }
            }
        }
        if let Some(f) = dstr(&o, "file") {
            return f;
        }
    }
    prev.to_string()
}

// ------------------------------------------------------------- C 타입 → Siskin

const INT_TYPES: &[&str] = &[
    "char", "signed char", "unsigned char", "short", "unsigned short", "short int",
    "unsigned short int", "int", "unsigned int", "unsigned", "long", "unsigned long",
    "long int", "unsigned long int", "long long", "unsigned long long", "long long int",
    "unsigned long long int", "wchar_t", "__int128", "unsigned __int128", "signed",
    "signed int", "signed long", "signed short", "signed char int",
];

/// 공백을 하나로 줄이고 의미 없는 수식어를 뗍니다.
fn tidy(q: &str) -> String {
    let mut s = q.to_string();
    for junk in ["const ", "volatile ", "restrict", "_Nullable", "_Nonnull", "_Null_unspecified"] {
        s = s.replace(junk, " ");
    }
    // `const` 가 맨 뒤에 붙는 경우 (`char * const`)
    let mut out = String::new();
    for w in s.split_whitespace() {
        if w == "const" || w == "volatile" {
            continue;
        }
        if !out.is_empty() && w != "*" && !out.ends_with('*') {
            out.push(' ');
        } else if !out.is_empty() && !out.ends_with(' ') && !out.ends_with('*') {
            out.push(' ');
        }
        out.push_str(w);
    }
    // `foo**` 처럼 붙어 있는 별은 그대로 둡니다.
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// typedef 를 끝까지 따라갑니다. `uLong` → `unsigned long`.
fn resolve(q: &str, td: &HashMap<String, String>, depth: usize) -> String {
    if depth > 16 {
        return q.to_string();
    }
    let t = tidy(q);
    let mut base = t.clone();
    let mut stars = 0usize;
    loop {
        let b = base.trim_end().to_string();
        if let Some(stripped) = b.strip_suffix('*') {
            base = stripped.trim_end().to_string();
            stars += 1;
        } else {
            base = b;
            break;
        }
    }
    if let Some(under) = td.get(&base) {
        if under != &base {
            let mut next = under.clone();
            for _ in 0..stars {
                next.push('*');
            }
            return resolve(&next, td, depth + 1);
        }
    }
    let mut out = base;
    for _ in 0..stars {
        out.push('*');
    }
    tidy(&out)
}

/// 헤더에 적힌 C 타입 하나를 Siskin 타입으로 옮깁니다.
/// `is_ret` 은 반환 자리인지. `char*` 은 자리에 따라 뜻이 달라서 구분합니다.
fn map_ty(q: &str, td: &HashMap<String, String>, is_ret: bool) -> Result<MTy, String> {
    let orig = tidy(q);
    // `const` 가 붙었는지는 typedef 를 따라간 뒤에도 알아야 합니다.
    // `const char*` 은 "읽어라"(글자열), 그냥 `char*` 은 "여기 써 넣어라"(손잡이)라
    // 뜻이 정반대이기 때문입니다.
    let had_const = q.contains("const") || {
        let base = tidy(q).trim_end_matches('*').trim().to_string();
        td.get(&base).map(|u| u.contains("const")).unwrap_or(false)
    };
    let r = resolve(q, td, 0);
    let r = r.replace(" *", "*");
    let rs = r.trim();

    if rs == "void" {
        return if is_ret { Ok(MTy::Unit) } else { Err(tr!("void 인자", "void parameter").into()) };
    }
    if rs == "_Bool" || rs == "bool" {
        return Ok(MTy::Bool);
    }
    if INT_TYPES.contains(&rs) {
        return Ok(MTy::Int);
    }
    if rs == "float" || rs == "double" || rs == "long double" {
        return Ok(MTy::Float);
    }
    if rs.contains("(*") || rs.contains("(^") {
        return Err(why_callback().into());
    }
    if rs.ends_with(']') {
        // 인자 자리의 배열은 C에서 포인터로 넘어갑니다.
        return if is_ret { Err(tr!("배열을 돌려주는 함수", "function returning an array").into()) } else { Ok(MTy::Int) };
    }
    if rs.ends_with('*') {
        let pointee = rs.trim_end_matches('*').trim();
        let depth = rs.chars().filter(|c| *c == '*').count();
        // 1바이트짜리를 가리키는 포인터 하나만 글자열로 봅니다.
        // zlib 의 `const Bytef*`(= `const unsigned char*`) 도 여기 들어옵니다.
        let bytelike = matches!(pointee, "char" | "signed char" | "unsigned char");
        if depth == 1 && bytelike {
            // 돌려줄 때는 글자열, 넘길 때는 `const` 일 때만 글자열입니다.
            if is_ret || had_const {
                return Ok(MTy::Str);
            }
            return Ok(MTy::Int);
        }
        return Ok(MTy::Int); // 그 밖의 포인터는 전부 손잡이(Int)
    }
    if rs.starts_with("enum ") {
        return Ok(MTy::Int);
    }
    if rs.starts_with("struct ") || rs.starts_with("union ") {
        return Err(tr!("구조체를 통째로 주고받는 함수", "function passing or returning a struct by value").into());
    }
    Err(format!("{} `{}`", why_unknown_type(), orig))
}

/// `map_ty` 가 콜백 자리라고 알리는 이유 글. 호출하는 쪽이 이 글로 알아봅니다.
fn why_callback() -> &'static str {
    tr!("콜백(함수를 넘기는 인자)", "callback (a parameter that takes a function)")
}

/// `map_ty` 가 모르는 타입이라고 알리는 이유 글의 앞부분.
fn why_unknown_type() -> &'static str {
    tr!("모르는 타입", "unknown type")
}

fn why_variadic() -> String {
    tr!("인자 개수가 정해지지 않은 함수", "variadic function").into()
}

/// `int (*)(void *, int)` 같은 함수 포인터를 뜯어봅니다.
/// 라이브러리에 내 함수를 넘겨줄 수 있게 하기 위해서입니다.
fn parse_fnptr(q: &str, td: &HashMap<String, String>) -> Option<CbSig> {
    let r = resolve(q, td, 0);
    let open = r.find("(*")?;
    let ret_c = r[..open].trim().to_string();
    // `(*)` 를 지나 인자 목록의 여는 괄호를 찾습니다.
    let after = &r[open..];
    let close = after.find(')')?;
    let rest = after[close + 1..].trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let (_, params) = split_sig(&format!("x {}", rest));
    if params.iter().any(|p| p.contains("...") || p.contains("(*")) {
        return None;
    }
    let mut ps = Vec::new();
    for p in &params {
        match map_ty(p, td, false) {
            Ok(m) => ps.push(m),
            Err(_) => return None,
        }
    }
    let rt = match map_ty(&ret_c, td, true) {
        Ok(MTy::Unit) => None,
        Ok(m) => Some(m),
        Err(_) => return None,
    };
    Some(CbSig { ret: rt, params: ps, c_ret: ret_c, c_params: params })
}

/// `T**` 처럼 포인터를 또 가리키는 자리는 거의 언제나 "여기에 결과를 넣어라"
/// 라는 뜻입니다 (`sqlite3_open(파일, &db)`). 이런 자리는 Siskin 의 `inout` 으로
/// 옮겨서 `sqlite3_open("t.db", inout db)` 처럼 쓰게 합니다.
fn is_out_param(q: &str, td: &HashMap<String, String>) -> bool {
    let r = resolve(q, td, 0).replace(" *", "*");
    let r = r.trim();
    if !r.ends_with("**") || r.ends_with("***") {
        return false;
    }
    if r.contains("(*") {
        return false;
    }
    // 바깥쪽 포인터가 `const` 면 바꿀 수 없으니 읽기 전용입니다.
    !q.trim_end().ends_with("const *")
}

// --------------------------------------------------------------- clang 호출

fn temp_dir() -> PathBuf {
    let d = std::env::temp_dir().join("siskin-ffi");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn hash64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn run_clang(header: &str, cpp: bool, incdirs: &[String]) -> Result<String, String> {
    let d = temp_dir();
    let src = d.join(if cpp { "probe.cpp" } else { "probe.c" });
    let include = if header.starts_with('.') || header.starts_with('/') {
        format!("#include \"{}\"\n", header)
    } else {
        format!("#include <{}>\n", header)
    };
    std::fs::write(&src, &include).map_err(|e| tr!(format!("임시 파일을 쓸 수 없습니다: {}", e), format!("cannot write temporary file: {}", e)))?;

    let mut cmd = Command::new("clang");
    cmd.arg("-x").arg(if cpp { "c++" } else { "c" });
    if cpp {
        cmd.arg("-std=c++17");
    }
    cmd.arg("-Xclang").arg("-ast-dump=json").arg("-fsyntax-only").arg("-w");
    for i in incdirs {
        cmd.arg(format!("-I{}", i));
    }
    cmd.arg(&src);

    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return Err(tr!(
                format!(
                    "clang을 실행할 수 없습니다 ({}).\n\
                     헤더를 자동으로 가져오려면 clang이 필요합니다. \
                     우분투/데비안이면 `apt install clang`, macOS면 `xcode-select --install`.",
                    e
                ),
                format!(
                    "cannot run clang ({}).\n\
                     clang is needed to import headers automatically. \
                     on Ubuntu/Debian: `apt install clang`; on macOS: `xcode-select --install`",
                    e
                )
            ))
        }
    };
    if out.stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        let first: Vec<&str> = err.lines().filter(|l| l.contains("error")).take(3).collect();
        let detail =
            if first.is_empty() { err.lines().take(3).collect::<Vec<_>>().join("\n") } else { first.join("\n") };
        return Err(tr!(
            format!("헤더를 읽지 못했습니다: {}\n{}", header, detail),
            format!("failed to read header: {}\n{}", header, detail)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

// ---------------------------------------------------------------- typedef 표

fn collect_typedefs(root: &JRef, td: &mut HashMap<String, String>) {
    let mut stack = vec![root.clone()];
    while let Some(n) = stack.pop() {
        for c in dlist(&n, "inner") {
            if dstr(&c, "kind").as_deref() == Some("TypedefDecl") {
                if let (Some(name), Some(t)) = (dstr(&c, "name"), dget(&c, "type")) {
                    if let Some(q) = dstr(&t, "qualType") {
                        td.entry(name).or_insert(q);
                    }
                }
            }
            stack.push(c);
        }
    }
}

// ------------------------------------------------------------------ 본 작업

/// 헤더 경로가 요청한 이름으로 끝나는지. `"sys/stat.h"` 는
/// `/usr/include/x86_64-linux-gnu/sys/stat.h` 와 맞습니다.
fn is_wanted(path: &str, header: &str) -> bool {
    let want = header.trim_start_matches("./").replace('\\', "/");
    let p = path.replace('\\', "/");
    p == want || p.ends_with(&format!("/{}", want))
}

pub fn import_header(
    header: &str,
    cpp: bool,
    incdirs: &[String],
    only: &[String],
) -> Result<Imported, String> {
    // 건너뛴 이유 글은 언어마다 다르므로 캐시도 언어별로 둡니다 (한국어는 예전 키 그대로).
    let key = format!("{}|{}|{}|v9{}", header, cpp, incdirs.join(":"), tr!("", "|en"));
    let cache = temp_dir().join(format!("{:016x}.txt", hash64(&key)));
    if let Some(hit) = load_cache(&cache) {
        return Ok(filter_only(hit, only));
    }

    let text = run_clang(header, cpp, incdirs)?;
    let root = json::parse(&text).map_err(|e| tr!(format!("clang이 낸 내용을 읽지 못했습니다: {}", e), format!("cannot parse clang output: {}", e)))?;

    let mut td: HashMap<String, String> = HashMap::new();
    collect_typedefs(&root, &mut td);

    if cpp {
        let mut res = import_cpp(&root, header, &td);
        abs_path(&mut res);
        save_cache(&cache, &res);
        return Ok(filter_only(res, only));
    }

    let mut res = Imported::default();
    let mut cur = String::new();
    let mut seen: HashMap<String, ()> = HashMap::new();

    for n in dlist(&root, "inner") {
        cur = loc_file(&n, &cur);
        if dstr(&n, "kind").as_deref() != Some("FunctionDecl") {
            continue;
        }
        if !is_wanted(&cur, header) {
            continue;
        }
        if res.header_path.is_empty() {
            res.header_path = cur.clone();
        }
        let name = match dstr(&n, "name") {
            Some(x) => x,
            None => continue,
        };
        if seen.contains_key(&name) {
            continue;
        }
        seen.insert(name.clone(), ());

        let ty = match dget(&n, "type") {
            Some(t) => t,
            None => continue,
        };
        let qual = dstr(&ty, "qualType").unwrap_or_default();
        if qual.contains("...") {
            res.skipped.push((name, why_variadic()));
            continue;
        }
        // 반환 타입은 시그니처에서 첫 `(` 앞까지입니다.
        let ret_c = match qual.find('(') {
            Some(i) => qual[..i].trim().to_string(),
            None => qual.trim().to_string(),
        };

        let mut params_c: Vec<String> = Vec::new();
        for c in dlist(&n, "inner") {
            if dstr(&c, "kind").as_deref() == Some("ParmVarDecl") {
                let pt = dget(&c, "type").and_then(|t| dstr(&t, "qualType")).unwrap_or_default();
                params_c.push(pt);
            }
        }

        let ret = match map_ty(&ret_c, &td, true) {
            Ok(t) => t,
            Err(why) => {
                res.skipped.push((name, why));
                continue;
            }
        };
        let mut params = Vec::new();
        let mut cbs: Vec<Option<CbSig>> = Vec::new();
        let mut bad = None;
        for p in &params_c {
            match map_ty(p, &td, false) {
                Ok(t) => {
                    params.push(t);
                    cbs.push(None);
                }
                Err(why) => {
                    // 함수를 넘겨 달라는 자리면 그 생김새를 적어 두고 계속합니다.
                    if why == why_callback() {
                        if let Some(cb) = parse_fnptr(p, &td) {
                            params.push(MTy::Int);
                            cbs.push(Some(cb));
                            continue;
                        }
                    }
                    bad = Some(why);
                    break;
                }
            }
        }
        if let Some(why) = bad {
            res.skipped.push((name, why));
            continue;
        }
        let outs: Vec<bool> = params_c.iter().map(|p| is_out_param(p, &td)).collect();
        res.fns.push(ImportedFn { name, ret, params, c_ret: ret_c, c_params: params_c, cpp_shim: None, outs, cbs });
    }

    abs_path(&mut res);
    save_cache(&cache, &res);
    Ok(filter_only(res, only))
}

/// clang 이 알려 준 자리를 절대 경로로 바꿉니다. 나중에 다른 폴더에서
/// 컴파일해도 같은 헤더를 보게 하기 위해서입니다.
fn abs_path(im: &mut Imported) {
    if let Ok(p) = std::fs::canonicalize(&im.header_path) {
        im.header_path = p.to_string_lossy().to_string();
    }
}

fn filter_only(mut im: Imported, only: &[String]) -> Imported {
    if only.is_empty() {
        return im;
    }
    im.fns.retain(|f| only.iter().any(|o| o == &f.name));
    im
}

// -------------------------------------------------------------------- 캐시
// clang은 1초 넘게 걸리므로 결과를 저장해 둡니다. 헤더가 바뀌면 버립니다.

fn save_cache(path: &PathBuf, im: &Imported) {
    let mut s = String::from("siskin-ffi 9\n");
    let stamp = std::fs::metadata(&im.header_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    s.push_str(&format!("H\t{}\t{}\n", im.header_path, stamp));
    for f in &im.fns {
        let ps: Vec<String> = f.params.iter().map(|t| t.siskin().to_string()).collect();
        s.push_str(&format!(
            "F\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            f.name,
            f.ret.siskin(),
            ps.join(","),
            f.c_ret,
            f.c_params.join("|"),
            f.outs.iter().map(|b| if *b { '1' } else { '0' }).collect::<String>(),
            f.cbs
                .iter()
                .map(|c| match c {
                    None => String::from("-"),
                    Some(cb) => format!(
                        "{};{};{};{}",
                        cb.ret.map(|m| m.siskin().to_string()).unwrap_or_default(),
                        cb.params.iter().map(|m| m.siskin()).collect::<Vec<_>>().join(","),
                        cb.c_ret,
                        cb.c_params.join(",")
                    ),
                })
                .collect::<Vec<_>>()
                .join("~"),
            f.cpp_shim.clone().unwrap_or_default().replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t")
        ));
    }
    for (n, w) in &im.skipped {
        s.push_str(&format!("S\t{}\t{}\n", n, w));
    }
    let _ = std::fs::write(path, s);
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn mty_of(s: &str) -> MTy {
    match s {
        "Unit" => MTy::Unit,
        "Float" => MTy::Float,
        "Bool" => MTy::Bool,
        "Str" => MTy::Str,
        _ => MTy::Int,
    }
}

fn load_cache(path: &PathBuf) -> Option<Imported> {
    let s = std::fs::read_to_string(path).ok()?;
    let mut lines = s.lines();
    if lines.next()? != "siskin-ffi 9" {
        return None;
    }
    let mut im = Imported::default();
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first() {
            Some(&"H") if f.len() >= 3 => {
                im.header_path = f[1].to_string();
                let want: u64 = f[2].parse().unwrap_or(0);
                let now = std::fs::metadata(f[1])
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                if now != want {
                    return None; // 헤더가 바뀌었습니다
                }
            }
            Some(&"F") if f.len() >= 6 => {
                let outs: Vec<bool> = f.get(6).map(|x| x.chars().map(|c| c == '1').collect()).unwrap_or_default();
                let cbs: Vec<Option<CbSig>> = f
                    .get(7)
                    .map(|x| {
                        if x.is_empty() {
                            Vec::new()
                        } else {
                            x.split('~')
                                .map(|part| {
                                    if part == "-" {
                                        return None;
                                    }
                                    let q: Vec<&str> = part.split(';').collect();
                                    if q.len() < 4 {
                                        return None;
                                    }
                                    Some(CbSig {
                                        ret: if q[0].is_empty() { None } else { Some(mty_of(q[0])) },
                                        params: if q[1].is_empty() { Vec::new() } else { q[1].split(',').map(mty_of).collect() },
                                        c_ret: q[2].to_string(),
                                        c_params: if q[3].is_empty() { Vec::new() } else { q[3].split(',').map(|x| x.to_string()).collect() },
                                    })
                                })
                                .collect()
                        }
                    })
                    .unwrap_or_default();
                let shim = f.get(8).map(|x| unesc(x)).filter(|x| !x.is_empty());
                im.fns.push(ImportedFn {
                    cpp_shim: shim,
                    outs,
                    cbs,
                    name: f[1].to_string(),
                    ret: mty_of(f[2]),
                    params: if f[3].is_empty() {
                        Vec::new()
                    } else {
                        f[3].split(',').map(mty_of).collect()
                    },
                    c_ret: f[4].to_string(),
                    c_params: if f[5].is_empty() {
                        Vec::new()
                    } else {
                        f[5].split('|').map(|x| x.to_string()).collect()
                    },
                });
            }
            Some(&"S") if f.len() >= 3 => im.skipped.push((f[1].to_string(), f[2].to_string())),
            _ => {}
        }
    }
    Some(im)
}

// ===================================================================== C++
//
// C++ 은 이름이 안에서 뒤틀려 저장되고(`geo::add(int,int)` -> `_ZN3geo3addEii`),
// 클래스·가상 함수·템플릿처럼 C에 없는 것들이 있어서 C 쪽에서 그대로 부를 수
// 없습니다. 그래서 가운데에 다리를 놓습니다. C++ 함수 하나마다
// `extern "C"` 로 감싼 껍데기를 만들어서 C++ 컴파일러에게 같이 넘기면,
// 뒤틀린 이름도 가상 함수도 템플릿도 C++ 컴파일러가 알아서 처리해 줍니다.

/// C++ 인자/반환 하나를 어떻게 옮길지.
#[derive(Clone, Debug)]
struct CppSlot {
    mty: MTy,
    /// C++ 쪽 원래 타입.
    cpp: String,
}

fn strip_quals_cpp(q: &str) -> String {
    let mut s = q.trim().to_string();
    while s.starts_with("const ") || s.starts_with("volatile ") {
        s = s.trim_start_matches("const ").trim_start_matches("volatile ").trim().to_string();
    }
    s.trim().to_string()
}

fn is_string_type(t: &str) -> bool {
    let b = strip_quals_cpp(t).trim_end_matches('&').trim().to_string();
    b == "std::string"
        || b == "string"
        || b == "std::basic_string<char>"
        || b == "basic_string<char>"
}

/// 인자 하나를 옮기는 법. `(siskin 타입, 껍데기 안에서 쓸 식)`.
fn cpp_param(q: &str, td: &HashMap<String, String>, idx: usize) -> Result<(CppSlot, String), String> {
    let a = format!("a{}", idx);
    let t = q.trim();
    if is_string_type(t) {
        return Ok((CppSlot { mty: MTy::Str, cpp: t.into() }, format!("std::string({})", a)));
    }
    let bare = strip_quals_cpp(t);
    // 참조(`T&`)는 손잡이를 받아서 그 자리를 가리킵니다.
    if let Some(inner) = bare.strip_suffix('&') {
        let inner = inner.trim().trim_end_matches('&').trim();
        if is_string_type(inner) {
            return Ok((CppSlot { mty: MTy::Str, cpp: t.into() }, format!("std::string({})", a)));
        }
        if let Ok(m) = map_ty(inner, td, false) {
            if m != MTy::Int {
                // `double&` 같은 건 값으로 못 받습니다.
                return Err(tr!("참조로 값을 돌려주는 인자", "parameter that returns a value by reference").into());
            }
        }
        let base = strip_quals_cpp(inner).to_string();
        return Ok((
            CppSlot { mty: MTy::Int, cpp: t.into() },
            format!("(*({}*)(void*){})", base, a),
        ));
    }
    // 그 밖에는 C와 같은 규칙을 씁니다.
    match map_ty(t, td, false) {
        Ok(MTy::Str) => Ok((CppSlot { mty: MTy::Str, cpp: t.into() }, format!("({}){}", t, a))),
        Ok(m @ (MTy::Int | MTy::Float | MTy::Bool)) => {
            if bare.ends_with('*') {
                Ok((CppSlot { mty: MTy::Int, cpp: t.into() }, format!("({})(void*){}", t, a)))
            } else {
                Ok((CppSlot { mty: m, cpp: t.into() }, format!("({}){}", t, a)))
            }
        }
        Ok(MTy::Unit) => Err(tr!("void 인자", "void parameter").into()),
        Err(why) => {
            // 클래스를 값으로 받는 건 복사가 필요해서 아직 안 합니다.
            if why.starts_with(why_unknown_type()) {
                Err(tr!("C++ 값(클래스)을 그대로 받는 인자", "parameter taking a C++ class by value").into())
            } else {
                Err(why)
            }
        }
    }
}

/// 반환을 옮기는 법. `(siskin 타입, call 식을 감쌀 틀)`.
/// 틀 안의 `{}` 자리에 실제 호출식이 들어갑니다.
fn cpp_ret(q: &str, td: &HashMap<String, String>) -> Result<(MTy, String), String> {
    let t = q.trim();
    if t == "void" {
        return Ok((MTy::Unit, "{};".into()));
    }
    if is_string_type(t) {
        // std::string 은 글자열로 옮깁니다. Siskin 이 바로 복사해 가므로
        // 돌려 쓰는 칸 몇 개면 충분합니다.
        return Ok((MTy::Str, "return mi_cpp_hold({});".into()));
    }
    let bare = strip_quals_cpp(t);
    if let Some(inner) = bare.strip_suffix('&') {
        let inner = inner.trim();
        if is_string_type(inner) {
            return Ok((MTy::Str, "return mi_cpp_hold({});".into()));
        }
        return Ok((MTy::Int, "return (int64_t)(void*)&({});".into()));
    }
    match map_ty(t, td, true) {
        Ok(MTy::Unit) => Ok((MTy::Unit, "{};".into())),
        Ok(MTy::Str) => Ok((MTy::Str, "return (const char*)({});".into())),
        Ok(MTy::Float) => Ok((MTy::Float, "return (double)({});".into())),
        Ok(MTy::Bool) => Ok((MTy::Bool, "return (bool)({});".into())),
        Ok(MTy::Int) => {
            if bare.ends_with('*') {
                Ok((MTy::Int, "return (int64_t)(void*)({});".into()))
            } else {
                Ok((MTy::Int, "return (int64_t)({});".into()))
            }
        }
        Err(_) => {
            // 클래스를 값으로 돌려주면 힙에 복사해 두고 손잡이를 줍니다.
            Ok((MTy::Int, format!("return (int64_t)(void*)new {}({{}});", bare)))
        }
    }
}

fn sanitize_name(s: &str) -> String {
    s.replace("::", "_").replace(['<', '>', ' ', ',', '~'], "_")
}

struct CppCtx<'a> {
    td: &'a HashMap<String, String>,
    header: String,
    out: Imported,
    seen: HashMap<String, ()>,
}

impl<'a> CppCtx<'a> {
    fn add(
        &mut self,
        siskin_name: String,
        ret_q: &str,
        params: Vec<(String, String)>, // (C++ 타입, 이름은 안 씀)
        recv: Option<String>,          // 클래스 이름 (메서드면)
        make_call: impl Fn(&[String]) -> String,
        pretty: &str,
    ) {
        if self.seen.contains_key(&siskin_name) {
            self.out
                .skipped
                .push((pretty.to_string(), tr!("이름이 같은 것이 여러 개(오버로드)", "several with the same name (overloads)").into()));
            return;
        }
        let mut slots: Vec<CppSlot> = Vec::new();
        let mut exprs: Vec<String> = Vec::new();
        let mut idx = 0usize;
        if let Some(cls) = &recv {
            slots.push(CppSlot { mty: MTy::Int, cpp: format!("{}*", cls) });
            exprs.push(format!("(({}*)(void*)a0)", cls));
            idx = 1;
        }
        for (ct, _) in &params {
            match cpp_param(ct, self.td, idx) {
                Ok((s, e)) => {
                    slots.push(s);
                    exprs.push(e);
                }
                Err(why) => {
                    self.out.skipped.push((pretty.to_string(), why));
                    return;
                }
            }
            idx += 1;
        }
        let (rty, tmpl) = match cpp_ret(ret_q, self.td) {
            Ok(x) => x,
            Err(why) => {
                self.out.skipped.push((pretty.to_string(), why));
                return;
            }
        };
        let call = make_call(&exprs);
        let body = tmpl.replacen("{}", &call, 1);
        let plist: Vec<String> = slots
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{} a{}", s.mty.cty(), i))
            .collect();
        let plist = if plist.is_empty() { "void".to_string() } else { plist.join(", ") };
        let shim = format!(
            "/* {} */\nextern \"C\" {} mx_{}({}) {{\n    {}\n}}\n",
            pretty,
            rty.cty(),
            siskin_name,
            plist,
            body
        );
        self.seen.insert(siskin_name.clone(), ());
        self.out.fns.push(ImportedFn {
            name: siskin_name,
            ret: rty,
            params: slots.iter().map(|s| s.mty).collect(),
            c_ret: String::new(),
            c_params: Vec::new(),
            cpp_shim: Some(shim),
            outs: vec![false; slots.len()],
            cbs: vec![None; slots.len()],
        });
    }
}

fn split_sig(qual: &str) -> (String, Vec<String>) {
    // `int (int, double)` -> ("int", ["int","double"])  — 괄호 짝을 셉니다.
    let open = match qual.find('(') {
        Some(i) => i,
        None => return (qual.trim().into(), Vec::new()),
    };
    let ret = qual[..open].trim().to_string();
    let rest = &qual[open + 1..];
    let mut depth = 0i32;
    let mut end = rest.len();
    for (i, c) in rest.char_indices() {
        match c {
            '(' | '<' => depth += 1,
            '>' => depth -= 1,
            ')' => {
                if depth == 0 {
                    end = i;
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let inner = &rest[..end];
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut d = 0i32;
    for c in inner.chars() {
        match c {
            '<' | '(' => {
                d += 1;
                cur.push(c);
            }
            '>' | ')' => {
                d -= 1;
                cur.push(c);
            }
            ',' if d == 0 => {
                parts.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur.trim().to_string());
    }
    if parts.len() == 1 && parts[0] == "void" {
        parts.clear();
    }
    (ret, parts)
}

fn param_types(n: &JRef) -> Vec<String> {
    let mut out = Vec::new();
    for c in dlist(n, "inner") {
        if dstr(&c, "kind").as_deref() == Some("ParmVarDecl") {
            out.push(dget(&c, "type").and_then(|t| dstr(&t, "qualType")).unwrap_or_default());
        }
    }
    out
}

fn import_cpp(root: &JRef, header: &str, td: &HashMap<String, String>) -> Imported {
    let mut ctx = CppCtx {
        td,
        header: header.to_string(),
        out: Imported::default(),
        seen: HashMap::new(),
    };
    walk_cpp(root, "", &mut ctx, &mut String::new());
    let h = ctx.header.clone();
    if ctx.out.header_path.is_empty() {
        ctx.out.header_path = h;
    }
    ctx.out
}

fn walk_cpp(node: &JRef, ns: &str, ctx: &mut CppCtx, cur: &mut String) {
    for c in dlist(node, "inner") {
        let kind = dstr(&c, "kind").unwrap_or_default();
        *cur = loc_file(&c, cur);
        let in_header = is_wanted(cur, &ctx.header);
        let name = dstr(&c, "name").unwrap_or_default();
        match kind.as_str() {
            "NamespaceDecl" | "LinkageSpecDecl" => {
                let inner_ns = if name.is_empty() { ns.to_string() } else { format!("{}{}::", ns, name) };
                let mut sub = cur.clone();
                walk_cpp(&c, &inner_ns, ctx, &mut sub);
            }
            "CXXRecordDecl" => {
                if name.is_empty() || dget(&c, "inner").is_none() {
                    continue;
                }
                let cls_ns = format!("{}{}::", ns, name);
                let mut sub = cur.clone();
                walk_cpp(&c, &cls_ns, ctx, &mut sub);
                // 지우는 함수는 C++이 알아서 만들어 주므로 헤더에 안 적혀 있습니다.
                // 손잡이를 받은 쪽에서 반드시 지울 수 있어야 하니 우리가 만들어 둡니다.
                if in_header {
                    let cls = format!("{}{}", ns, name);
                    let siskin = sanitize_name(&format!("{}_delete", cls));
                    if !ctx.seen.contains_key(&siskin) {
                        let pretty = format!("{}  [{}]", cls, tr!("지우기", "delete"));
                        ctx.add(siskin, "void", Vec::new(), Some(cls.clone()), |args| {
                            format!("delete {}", args[0])
                        }, &pretty);
                    }
                }
            }
            "FunctionDecl" | "CXXMethodDecl" | "CXXConstructorDecl" | "CXXDestructorDecl" => {
                if !in_header {
                    continue;
                }
                if ctx.out.header_path.is_empty() || !ctx.out.header_path.starts_with('/') {
                    ctx.out.header_path = cur.clone();
                }
                cpp_entity(&c, &kind, ns, &name, ctx);
            }
            // 템플릿은 쓰는 순간마다 새로 찍어내는 것이라 미리 가져올 수 없습니다.
            "FunctionTemplateDecl" | "ClassTemplateDecl" => {
                if in_header && !name.is_empty() {
                    ctx.out.skipped.push((
                        format!("{}{}", ns, name),
                        tr!("템플릿(쓸 때 타입이 정해지는 것)", "template (types are decided at use)").into(),
                    ));
                }
            }
            _ => {}
        }
    }
}

fn cpp_entity(c: &JRef, kind: &str, ns: &str, name: &str, ctx: &mut CppCtx) {
    // 컴파일러가 스스로 만든 것, 지운 것, 공개되지 않은 것은 건너뜁니다.
    if matches!(dget(c, "isImplicit").map(|v| matches!(&*v.borrow(), JsonVal::Bool(true))), Some(true)) {
        return;
    }
    if matches!(dget(c, "explicitlyDeleted").map(|v| matches!(&*v.borrow(), JsonVal::Bool(true))), Some(true)) {
        return;
    }
    if let Some(acc) = dstr(c, "access") {
        if acc != "public" {
            return;
        }
    }
    if name.starts_with("operator") {
        return;
    }
    let qual = dget(c, "type").and_then(|t| dstr(&t, "qualType")).unwrap_or_default();
    if qual.contains("...") {
        ctx.out.skipped.push((format!("{}{}", ns, name), why_variadic()));
        return;
    }
    let (ret_q, _) = split_sig(&qual);
    let ptypes = param_types(c);
    let cls = ns.trim_end_matches("::").to_string();
    let cls_short = cls.rsplit("::").next().unwrap_or("").to_string();

    match kind {
        "CXXConstructorDecl" => {
            let siskin = sanitize_name(&format!("{}_new", cls));
            let pretty = format!("{}::{}(...)  [{}]", cls, cls_short, tr!("만들기", "new"));
            let cls2 = cls.clone();
            ctx.add(
                siskin,
                "void",
                ptypes.iter().map(|t| (t.clone(), String::new())).collect(),
                None,
                move |args| format!("new {}({})", cls2, args.join(", ")),
                &pretty,
            );
            // 만들기는 손잡이를 돌려줘야 하므로 반환 틀을 직접 고칩니다.
            if let Some(last) = ctx.out.fns.last_mut() {
                if last.ret == MTy::Unit {
                    last.ret = MTy::Int;
                    if let Some(sh) = &last.cpp_shim {
                        let fixed = sh
                            .replace("extern \"C\" void ", "extern \"C\" int64_t ")
                            .replace("    new ", "    return (int64_t)(void*)new ");
                        last.cpp_shim = Some(fixed);
                    }
                }
            }
        }
        "CXXDestructorDecl" => {
            let siskin = sanitize_name(&format!("{}_delete", cls));
            let pretty = format!("{}::~{}()  [{}]", cls, cls_short, tr!("지우기", "delete"));
            let cls2 = cls.clone();
            ctx.add(siskin, "void", Vec::new(), Some(cls.clone()), move |args| {
                let _ = &cls2;
                format!("delete {}", args[0])
            }, &pretty);
        }
        "CXXMethodDecl" => {
            let is_static = matches!(
                dget(c, "storageClass").and_then(|v| match &*v.borrow() {
                    JsonVal::Str(s) => Some(s.clone()),
                    _ => None,
                }),
                Some(ref s) if s == "static"
            );
            let siskin = sanitize_name(&format!("{}_{}", cls, name));
            let pretty = format!("{}::{}()", cls, name);
            if is_static {
                let full = format!("{}::{}", cls, name);
                ctx.add(siskin, &ret_q, ptypes.iter().map(|t| (t.clone(), String::new())).collect(), None,
                    move |args| format!("{}({})", full, args.join(", ")), &pretty);
            } else {
                ctx.add(siskin, &ret_q, ptypes.iter().map(|t| (t.clone(), String::new())).collect(), Some(cls.clone()),
                    move |args| format!("{}->{}({})", args[0], name, args[1..].join(", ")), &pretty);
            }
        }
        _ => {
            let full = format!("{}{}", ns, name);
            let siskin = sanitize_name(&full);
            let pretty = format!("{}()", full);
            let f2 = full.clone();
            ctx.add(siskin, &ret_q, ptypes.iter().map(|t| (t.clone(), String::new())).collect(), None,
                move |args| format!("{}({})", f2, args.join(", ")), &pretty);
        }
    }
}
