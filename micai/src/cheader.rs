//! Reads C / C++ header files and turns their functions into Siskin declarations.
//!
//! A single line `import c "zlib.h" link "z"` makes every zlib function available.
//! Instead of parsing headers ourselves we ask `clang`, because imitating
//! macros, typedefs and `#ifdef` by hand is bound to go wrong.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use crate::json::{self, JsonVal, JRef};

/// Types on the Siskin side. All of C's many integer types collapse into Int.
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
    /// The C type used in the Siskin ABI.
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

/// Shape of a parameter that takes a function (callback) to pass to the library.
#[derive(Clone, Debug, Default)]
pub struct CbSig {
    pub ret: Option<MTy>,
    pub params: Vec<MTy>,
    /// Original C types. Used when generating bridge functions.
    pub c_ret: String,
    pub c_params: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ImportedFn {
    /// Name to call from Siskin.
    pub name: String,
    pub ret: MTy,
    pub params: Vec<MTy>,
    /// Original C types. Used for casts in the wrapper function.
    pub c_ret: String,
    pub c_params: Vec<String>,
    /// For C++, the complete pre-built `extern "C"` wrapper function.
    pub cpp_shim: Option<String>,
    /// Whether each parameter is an out-parameter ("put the result here"), taken as `T**`
    /// like the second parameter of `sqlite3_open`. In Siskin these become `inout`.
    pub outs: Vec<bool>,
    /// For each parameter that takes a function, that function's shape.
    pub cbs: Vec<Option<CbSig>>,
    /// Siskin-side parameter types written as type text (`CPtr[U32]`, `VkExtent2D`, ...).
    /// Empty for C++ imports, which use `params` instead.
    pub tys: Vec<String>,
    /// Siskin-side return type as type text. Empty means "use `ret`".
    pub ret_ty: String,
}

/// A value from `#define NAME 42` or a C `enum` constant.
#[derive(Clone, Debug)]
pub enum CVal {
    Int(i64),
    Float(f64),
    Str(String),
}

#[derive(Clone, Debug)]
pub struct CConst {
    pub name: String,
    pub val: CVal,
}

/// One field of a C struct/union imported from a header.
#[derive(Clone, Debug)]
pub struct CField {
    pub name: String,
    /// Siskin storage type as type text: `F32`, `[F32; 4]`, `VkExtent2D`, `Int` (pointer), `Str` (char array).
    pub ty: String,
    /// The C type as written in the header. Used for casts when storing pointers.
    pub c_ty: String,
    /// A pointer field (the Siskin side sees an address as Int).
    pub ptr: bool,
    /// A `const char*` / `char*` field: a Siskin string may be stored into it.
    pub charp: bool,
    /// A `char name[N]` field: read as Str, and a Str is copied in.
    pub chars: usize,
    /// A function-pointer field: a named Siskin function may be stored into it.
    pub cb: Option<CbSig>,
}

#[derive(Clone, Debug)]
pub struct CStruct {
    /// Name on the Siskin side (the typedef name if there is one, otherwise the tag).
    pub name: String,
    /// How C spells the type: `VkExtent2D` or `struct timespec`.
    pub c_name: String,
    pub is_union: bool,
    pub fields: Vec<CField>,
    /// (field, reason it could not be imported)
    pub skipped: Vec<(String, String)>,
}

/// A function pointer type such as `typedef void (*GLFWkeyfun)(GLFWwindow*, int, int, int, int)`.
/// `GLFWkeyfun(addr)` turns an address into a Siskin function value that can be called.
#[derive(Clone, Debug)]
pub struct CFnPtr {
    pub name: String,
    pub sig: CbSig,
}

#[derive(Clone, Debug, Default)]
pub struct Imported {
    pub fns: Vec<ImportedFn>,
    /// (name, reason it could not be imported)
    pub skipped: Vec<(String, String)>,
    /// The actual path of the header found by clang.
    pub header_path: String,
    /// `#define` number/string constants, `enum` constants and `static const` values.
    pub consts: Vec<CConst>,
    /// Complete struct/union definitions.
    pub structs: Vec<CStruct>,
    /// Function pointer typedefs.
    pub fnptrs: Vec<CFnPtr>,
    /// (function pointer typedef, reason it could not be imported)
    pub skipped_fnptrs: Vec<(String, String)>,
    /// Every header file read for this import (the header plus the `#include "..."` it follows),
    /// with modification times, so the cache is dropped when any of them changes.
    pub files: Vec<(String, u64)>,
}

// --------------------------------------------------------------- JSON helpers

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

/// Which file a node came from. clang omits `file` when it is the same as the
/// previous node, so if it is missing, reuse the previous value.
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

// ------------------------------------------------------------- C types → Siskin

const INT_TYPES: &[&str] = &[
    "char", "signed char", "unsigned char", "short", "unsigned short", "short int",
    "unsigned short int", "int", "unsigned int", "unsigned", "long", "unsigned long",
    "long int", "unsigned long int", "long long", "unsigned long long", "long long int",
    "unsigned long long int", "wchar_t", "__int128", "unsigned __int128", "signed",
    "signed int", "signed long", "signed short", "signed char int",
    // Newer clang (21+) shows size_t under this name.
    "__size_t", "__signed_size_t", "__ptrdiff_t",
];

/// Newer clang (21+) prints some standard types under internal names (`__size_t`) that C
/// and C++ code cannot use. Type text copied into generated wrappers goes through this so
/// it spells them the standard way. Type analysis still sees the original text.
pub fn c_spelling(q: &str) -> String {
    const NAMES: &[(&str, &str)] = &[
        ("__size_t", "size_t"),
        ("__signed_size_t", "ptrdiff_t"),
        ("__ptrdiff_t", "ptrdiff_t"),
        ("__wchar_t", "wchar_t"),
    ];
    if !q.contains("__") {
        return q.to_string();
    }
    let mut out = String::with_capacity(q.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        let w = NAMES.iter().find(|(from, _)| from == word).map(|(_, to)| *to).unwrap_or(word);
        out.push_str(w);
        word.clear();
    };
    for c in q.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Collapse whitespace and drop meaningless qualifiers.
fn tidy(q: &str) -> String {
    let mut s = q.to_string();
    for junk in ["const ", "volatile ", "restrict", "_Nullable", "_Nonnull", "_Null_unspecified"] {
        s = s.replace(junk, " ");
    }
    // `const` at the very end (`char * const`)
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
    // Leave attached stars as in `foo**` alone.
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Follow typedefs all the way. `uLong` → `unsigned long`.
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

/// Map one C type written in a header to a Siskin type.
/// `is_ret` says whether it is the return position; `char*` means different things depending on position.
fn map_ty(q: &str, td: &HashMap<String, String>, is_ret: bool) -> Result<MTy, String> {
    let orig = tidy(q);
    // Whether `const` is present must be known even after following typedefs,
    // because `const char*` means "read this" (a string) while a plain `char*` means "write here" (a handle):
    // exact opposites.
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
        // Arrays in parameter position are passed as pointers in C.
        return if is_ret { Err(tr!("배열을 돌려주는 함수", "function returning an array").into()) } else { Ok(MTy::Int) };
    }
    if rs.ends_with('*') {
        let pointee = rs.trim_end_matches('*').trim();
        let depth = rs.chars().filter(|c| *c == '*').count();
        // Only a single pointer to a 1-byte type is treated as a string.
        // zlib's `const Bytef*` (= `const unsigned char*`) also lands here.
        let bytelike = matches!(pointee, "char" | "signed char" | "unsigned char");
        if depth == 1 && bytelike {
            // Returned: a string. Passed: a string only when `const`.
            if is_ret || had_const {
                return Ok(MTy::Str);
            }
            return Ok(MTy::Int);
        }
        return Ok(MTy::Int); // every other pointer is a handle (Int)
    }
    if rs.starts_with("enum ") {
        return Ok(MTy::Int);
    }
    if rs.starts_with("struct ") || rs.starts_with("union ") {
        return Err(tr!("구조체를 통째로 주고받는 함수", "function passing or returning a struct by value").into());
    }
    Err(format!("{} `{}`", why_unknown_type(), orig))
}

/// Reason text `map_ty` uses to signal a callback parameter. Callers recognize it by this text.
fn why_callback() -> &'static str {
    tr!("콜백(함수를 넘기는 인자)", "callback (a parameter that takes a function)")
}

/// Prefix of the reason text `map_ty` uses to signal an unknown type.
fn why_unknown_type() -> &'static str {
    tr!("모르는 타입", "unknown type")
}

fn why_variadic() -> String {
    tr!("인자 개수가 정해지지 않은 함수", "variadic function").into()
}

/// Parse a function pointer like `int (*)(void *, int)`,
/// so that our own functions can be passed to the library.
fn parse_fnptr(q: &str, td: &HashMap<String, String>) -> Result<CbSig, String> {
    let r = resolve(q, td, 0);
    let bad_shape = || tr!("함수 포인터 모양을 읽지 못함", "unrecognized function pointer shape").to_string();
    if !is_plain_fnptr(&r) {
        return Err(bad_shape());
    }
    let open = r.find("(*").ok_or_else(bad_shape)?;
    let ret_c = r[..open].trim().to_string();
    // Skip past `(*)` and find the opening parenthesis of the parameter list.
    let after = &r[open..];
    let close = after.find(')').ok_or_else(bad_shape)?;
    let rest = after[close + 1..].trim_start();
    if !rest.starts_with('(') {
        return Err(bad_shape());
    }
    let (_, mut params) = split_sig(&format!("x {}", rest));
    if params.iter().any(|p| p.contains("...")) {
        return Err(why_variadic());
    }
    if params.iter().any(|p| is_fnptr_text(p)) {
        return Err(tr!("함수를 받는 매개변수가 있는 함수 포인터", "function pointer with a parameter that takes a function").into());
    }
    // Resolving typedefs drops `const`, but `const char*` (a string C hands us) and `char*`
    // (a buffer) mean different things. Take the parameters from the written text when it has them.
    let mut raw = q.trim().to_string();
    for _ in 0..16 {
        if raw.contains("(*") {
            break;
        }
        match td.get(tidy(&raw).trim()) {
            Some(u) if *u != raw => raw = u.clone(),
            _ => break,
        }
    }
    if let Some(o) = raw.find("(*") {
        let after = &raw[o..];
        if let Some(c) = after.find(')') {
            let rest = after[c + 1..].trim_start();
            if rest.starts_with('(') {
                let (_, written) = split_sig(&format!("x {}", rest));
                if written.len() == params.len() {
                    params = written;
                }
            }
        }
    }
    let mut ps = Vec::new();
    for p in &params {
        // `void` alone means "no parameters" (`void (*)(void)`).
        if params.len() == 1 && resolve(p, td, 0) == "void" {
            break;
        }
        ps.push(map_ty(p, td, false)?);
    }
    if ps.is_empty() {
        params.clear();
    }
    // A function pointer that returns a function pointer (`PFN_vkGetInstanceProcAddr`):
    // the returned one comes back as an address, as it does from a plain C function.
    let rt = if is_fnptr_text(&resolve(&ret_c, td, 0)) {
        Some(MTy::Int)
    } else {
        match map_ty(&ret_c, td, true)? {
            MTy::Unit => None,
            m => Some(m),
        }
    };
    Ok(CbSig { ret: rt, params: ps, c_ret: c_spelling(&ret_c), c_params: params.iter().map(|p| c_spelling(p)).collect() })
}

/// A pointer to a function, `ret (*)(params)`, as opposed to a pointer to such a pointer
/// (`ret (**)(params)`) or a block (`ret (^)(params)`). Pointer-to-pointer parameters inside the
/// parameter list (`const T* const*`) are fine.
fn is_plain_fnptr(r: &str) -> bool {
    match r.find("(*") {
        Some(o) => r[o + 2..].trim_start().starts_with(')'),
        None => false,
    }
}

/// A parameter pointing to another pointer, like `T**`, almost always means "put the result here"
/// (`sqlite3_open(file, &db)`). Such parameters become Siskin `inout`,
/// so it can be written as `sqlite3_open("t.db", inout db)`.
fn is_out_param(q: &str, td: &HashMap<String, String>) -> bool {
    let r = resolve(q, td, 0).replace(" *", "*");
    let r = r.trim();
    if !r.ends_with("**") || r.ends_with("***") {
        return false;
    }
    if r.contains("(*") {
        return false;
    }
    // If the outer pointer is `const`, it cannot be modified, so it is read-only.
    !q.trim_end().ends_with("const *")
}

// --------------------------------------------------------------- invoking clang

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

/// `#include <zlib.h>` or `#include "./local.h"`.
fn include_line(header: &str) -> String {
    if header.starts_with('.') || std::path::Path::new(header).is_absolute() {
        format!("#include \"{}\"\n", header)
    } else {
        format!("#include <{}>\n", header)
    }
}

fn clang_missing(e: std::io::Error) -> String {
    tr!(
        format!(
            "clang을 실행할 수 없습니다 ({}).\n\
             헤더를 자동으로 가져오려면 clang이 필요합니다. \
             우분투/데비안이면 `apt install clang`, macOS면 `xcode-select --install`, \
             윈도우면 `winget install MartinStorsjo.LLVM-MinGW.UCRT`.",
            e
        ),
        format!(
            "cannot run clang ({}).\n\
             clang is needed to import headers automatically. \
             on Ubuntu/Debian: `apt install clang`; on macOS: `xcode-select --install`; \
             on Windows: `winget install MartinStorsjo.LLVM-MinGW.UCRT`",
            e
        )
    )
}

/// Writes `text` to a probe file and runs clang on it with `mode` (`-E -dD` or the JSON AST dump).
fn run_clang(text: &str, cpp: bool, incdirs: &[String], defines: &[String], mode: &[&str], header: &str) -> Result<String, String> {
    let d = temp_dir();
    // One probe file per process, so two `siskin` runs at once don't overwrite each other's probe.
    let src = d.join(format!("probe-{}{}", std::process::id(), if cpp { ".cpp" } else { ".c" }));
    std::fs::write(&src, text).map_err(|e| tr!(format!("임시 파일을 쓸 수 없습니다: {}", e), format!("cannot write temporary file: {}", e)))?;

    // Read with the same clang used for compiling so type sizes and header locations match.
    let mut cmd = Command::new(crate::header_clang());
    cmd.arg("-x").arg(if cpp { "c++" } else { "c" });
    if cpp {
        cmd.arg("-std=c++17");
    }
    for m in mode {
        cmd.arg(m);
    }
    cmd.arg("-w").arg("-ferror-limit=0");
    for def in defines {
        cmd.arg(format!("-D{}", def));
    }
    for i in incdirs {
        cmd.arg(format!("-I{}", i));
    }
    cmd.arg(&src);

    let out = cmd.output().map_err(clang_missing)?;
    let _ = std::fs::remove_file(&src);
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

// ---------------------------------------------------------------- typedef table

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

// ------------------------------------------------------------ which files count

/// Whether a header path ends with the requested name. `"sys/stat.h"`
/// matches `/usr/include/x86_64-linux-gnu/sys/stat.h`.
fn is_wanted(path: &str, header: &str) -> bool {
    let want = header.trim_start_matches("./").replace('\\', "/");
    let p = path.replace('\\', "/");
    if p == want || p.ends_with(&format!("/{}", want)) {
        return true;
    }
    // Newer macOS SDKs declare the functions of `string.h` in `_string.h` in the same folder.
    let (dir, file) = match want.rsplit_once('/') {
        Some((d, f)) => (format!("/{}/", d), f.to_string()),
        None => ("/".to_string(), want.clone()),
    };
    p.ends_with(&format!("{}_{}", dir, file))
}

fn canon(p: &str) -> String {
    match crate::canonicalize(p) {
        Ok(c) => c.to_string_lossy().replace('\\', "/"),
        Err(_) => p.replace('\\', "/"),
    }
}

fn mtime(p: &str) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The header plus every file it pulls in with `#include "..."` (quotes), transitively.
/// `vulkan/vulkan.h` is only a list of `#include "vulkan_core.h"` lines, and
/// `SDL.h` includes `"SDL_video.h"` and friends this way, so those count as the same library.
/// `#include <...>` (angle brackets) usually means another library (`<stdio.h>`), so it is not followed.
fn follow_includes(start: &str, incdirs: &[String]) -> Vec<String> {
    let mut out = vec![canon(start)];
    let mut i = 0;
    while i < out.len() {
        let f = out[i].clone();
        i += 1;
        let text = match std::fs::read(&f) {
            Ok(b) => String::from_utf8_lossy(&b).to_string(),
            Err(_) => continue,
        };
        let dir = std::path::Path::new(&f).parent().map(|p| p.to_path_buf()).unwrap_or_default();
        for line in text.lines() {
            let t = line.trim_start();
            let t = match t.strip_prefix('#') {
                Some(r) => r.trim_start(),
                None => continue,
            };
            let rest = match t.strip_prefix("include") {
                Some(r) => r.trim_start(),
                None => continue,
            };
            let name = match rest.strip_prefix('"').and_then(|r| r.split('"').next()) {
                Some(n) if !n.is_empty() => n,
                _ => continue,
            };
            let mut cands = vec![dir.join(name)];
            for d in incdirs {
                cands.push(std::path::Path::new(d).join(name));
            }
            if let Some(found) = cands.iter().find(|p| p.is_file()) {
                let c = canon(&found.to_string_lossy());
                if !out.contains(&c) {
                    out.push(c);
                }
            }
        }
    }
    out
}

struct Wanted {
    set: std::collections::HashSet<String>,
    header: String,
    memo: HashMap<String, bool>,
}

impl Wanted {
    fn has(&mut self, file: &str) -> bool {
        if let Some(b) = self.memo.get(file) {
            return *b;
        }
        let b = self.set.contains(&canon(file)) || is_wanted(file, &self.header);
        self.memo.insert(file.to_string(), b);
        b
    }
}

// ---------------------------------------------------------------- #define values

enum MacroKind {
    Str(String),
    Float(f64),
    /// Anything else. Evaluated by clang as an integer constant expression.
    Expr,
}

/// Strip parentheses that wrap the whole text: `((1 << 3))` → `1 << 3`.
fn strip_parens(s: &str) -> &str {
    let mut t = s.trim();
    loop {
        if !(t.starts_with('(') && t.ends_with(')')) {
            return t;
        }
        let mut depth = 0i32;
        let mut whole = true;
        for (i, ch) in t.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && i != t.len() - 1 {
                        whole = false;
                        break;
                    }
                }
                _ => {}
            }
        }
        if !whole {
            return t;
        }
        t = t[1..t.len() - 1].trim();
    }
}

/// `"abc" "def"` → `abcdef`. None if it is not only string literals.
fn c_strings(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut it = s.trim().chars().peekable();
    let mut any = false;
    loop {
        while matches!(it.peek(), Some(c) if c.is_whitespace()) {
            it.next();
        }
        match it.next() {
            None => return if any { Some(out) } else { None },
            Some('"') => {}
            Some(_) => return None,
        }
        any = true;
        loop {
            match it.next()? {
                '"' => break,
                '\\' => match it.next()? {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    '0' => out.push('\0'),
                    other => out.push(other),
                },
                c => out.push(c),
            }
        }
    }
}

fn classify_macro(body: &str) -> Option<MacroKind> {
    let t = strip_parens(body);
    if t.is_empty() {
        return None;
    }
    if t.starts_with('"') {
        return c_strings(t).map(MacroKind::Str);
    }
    // A float literal: `3.14f`, `-0.5`, `1e-3`. Hex numbers and integers are left to clang.
    let lit = t.trim_start_matches('-');
    if !lit.starts_with("0x") && !lit.starts_with("0X") && (lit.contains('.') || lit.contains('e') || lit.contains('E')) {
        let is_f32 = lit.ends_with('f') || lit.ends_with('F');
        let num = lit.trim_end_matches(['f', 'F', 'l', 'L']);
        if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')) {
            if let Ok(v) = num.parse::<f64>() {
                let v = if t.starts_with('-') { -v } else { v };
                // `3.14f` is a C float: keep exactly the value C sees.
                let v = if is_f32 { v as f32 as f64 } else { v };
                return Some(MacroKind::Float(v));
            }
        }
    }
    Some(MacroKind::Expr)
}

/// Runs the preprocessor and collects object-like `#define`s from the wanted files.
/// Returns (the header path clang found, [(name, body, file)]).
fn read_macros(
    header: &str,
    cpp: bool,
    incdirs: &[String],
    defines: &[String],
) -> Result<(String, Vec<(String, String, String)>), String> {
    let text = run_clang(&include_line(header), cpp, incdirs, defines, &["-E", "-dD"], header)?;
    let mut cur = String::new();
    let mut header_path = String::new();
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# ") {
            // `# 12 "/usr/include/zlib.h" 1`
            if let Some(q) = rest.find('"') {
                let after = &rest[q + 1..];
                if let Some(end) = after.rfind('"') {
                    let file = after[..end].replace("\\\\", "\\");
                    let flags = after[end + 1..].trim();
                    let special = file.starts_with('<') || file.contains("probe-");
                    if header_path.is_empty() && !special && flags.split_whitespace().next() == Some("1") {
                        header_path = file.clone();
                    }
                    cur = file;
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("#define ") {
            let name_end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
            let name = &rest[..name_end];
            if name.is_empty() || rest[name_end..].starts_with('(') {
                continue; // function-like macro
            }
            out.push((name.to_string(), rest[name_end..].trim().to_string(), cur.clone()));
        }
    }
    Ok((header_path, out))
}

/// The first `"value"` in a subtree. For `enum { X = (expr) }` clang records the
/// computed value on the `ConstantExpr` that wraps the expression.
fn first_value(n: &JRef) -> Option<String> {
    if let Some(v) = dstr(n, "value") {
        return Some(v);
    }
    for c in dlist(n, "inner") {
        if let Some(v) = first_value(&c) {
            return Some(v);
        }
    }
    None
}

/// A `const` variable with a constant initialiser, such as Vulkan 1.3's
/// `static const VkPipelineStageFlagBits2 VK_PIPELINE_STAGE_2_NONE = 0ULL;`.
/// These are not macros or enum values, so they are read from clang's syntax tree.
fn const_var(n: &JRef, td: &HashMap<String, String>, known: &HashMap<String, CVal>) -> Option<(String, CVal)> {
    let name = dstr(n, "name").filter(|s| !s.is_empty() && !s.starts_with('_'))?;
    if dstr(n, "storageClass").as_deref() == Some("extern") || dstr(n, "init").is_none() {
        return None;
    }
    let q = dget(n, "type").and_then(|t| dstr(&t, "qualType")).unwrap_or_default();
    let is_const = q.starts_with("const ") || q.ends_with(" const");
    if !is_const {
        return None;
    }
    let init = dlist(n, "inner").into_iter().next()?;
    let v = eval_const(&init, td, known)?;
    // Convert to the declared type: `static const float F = 1;` is a Float.
    let ty = c_scalar(n, td);
    let v = match (v, ty.as_deref()) {
        (CVal::Str(s), _) => CVal::Str(s),
        (v, Some(t)) => convert_scalar(v, t),
        (_, None) => return None,
    };
    Some((name, v))
}

/// The storage name (`I32`, `U64`, `F32`, ...) of a node's C type, or `Str` for `const char *`.
fn c_scalar(n: &JRef, td: &HashMap<String, String>) -> Option<String> {
    let t = dget(n, "type")?;
    let q = dstr(&t, "qualType").unwrap_or_default();
    let r = tidy(&resolve(&q, td, 0));
    if r == "char *" || r == "char*" {
        return Some("Str".into());
    }
    let d = dstr(&t, "desugaredQualType").map(|d| tidy(&d));
    scalar_storage(&r).or_else(|| d.as_deref().and_then(scalar_storage)).map(|s| s.to_string()).or_else(|| {
        // An enum type converts like `int`.
        if r.starts_with("enum ") || d.as_deref().map_or(false, |d| d.starts_with("enum ")) {
            Some("I32".into())
        } else {
            None
        }
    })
}

/// Wrap or convert a value the way C does when storing it in type `t`.
fn convert_scalar(v: CVal, t: &str) -> CVal {
    let as_int = |v: &CVal| match v {
        CVal::Int(i) => *i,
        CVal::Float(f) => *f as i64,
        CVal::Str(_) => 0,
    };
    match t {
        "F32" => CVal::Float(match v { CVal::Float(f) => f as f32 as f64, _ => as_int(&v) as f32 as f64 }),
        "Float" => CVal::Float(match v { CVal::Float(f) => f, _ => as_int(&v) as f64 }),
        "Bool" => CVal::Int((as_int(&v) != 0) as i64),
        "I8" => CVal::Int(as_int(&v) as i8 as i64),
        "U8" => CVal::Int(as_int(&v) as u8 as i64),
        "I16" => CVal::Int(as_int(&v) as i16 as i64),
        "U16" => CVal::Int(as_int(&v) as u16 as i64),
        "I32" => CVal::Int(as_int(&v) as i32 as i64),
        "U32" => CVal::Int(as_int(&v) as u32 as i64),
        "Str" => v,
        _ => CVal::Int(as_int(&v)),
    }
}

/// Evaluate a constant initialiser from clang's syntax tree: literals, casts, parentheses,
/// unary and binary arithmetic/bit operators, and names of enum values or earlier constants.
fn eval_const(n: &JRef, td: &HashMap<String, String>, known: &HashMap<String, CVal>) -> Option<CVal> {
    let kind = dstr(n, "kind").unwrap_or_default();
    let inner = dlist(n, "inner");
    let unsigned = || c_scalar(n, td).map_or(false, |t| t.starts_with('U'));
    match kind.as_str() {
        "IntegerLiteral" | "CharacterLiteral" => dstr(n, "value").and_then(|v| parse_c_int(&v)).map(CVal::Int),
        "FloatingLiteral" => dstr(n, "value").and_then(|v| v.parse::<f64>().ok()).map(CVal::Float),
        "StringLiteral" => {
            let v = dstr(n, "value")?;
            let s = json::parse(&v).ok()?;
            let out = match &*s.borrow() {
                JsonVal::Str(x) => Some(CVal::Str(x.clone())),
                _ => None,
            };
            out
        }
        "ParenExpr" | "ConstantExpr" => eval_const(inner.first()?, td, known),
        "ImplicitCastExpr" | "CStyleCastExpr" => {
            let v = eval_const(inner.first()?, td, known)?;
            match c_scalar(n, td) {
                Some(t) => Some(convert_scalar(v, &t)),
                None => match v {
                    CVal::Str(_) => Some(v),
                    _ => None,
                },
            }
        }
        "DeclRefExpr" => {
            let r = dget(n, "referencedDecl")?;
            known.get(&dstr(&r, "name")?).cloned()
        }
        "UnaryOperator" => {
            let v = eval_const(inner.first()?, td, known)?;
            let op = dstr(n, "opcode")?;
            Some(match (op.as_str(), v) {
                ("-", CVal::Int(i)) => CVal::Int(i.wrapping_neg()),
                ("-", CVal::Float(f)) => CVal::Float(-f),
                ("+", v) => v,
                ("~", CVal::Int(i)) => CVal::Int(!i),
                ("!", CVal::Int(i)) => CVal::Int((i == 0) as i64),
                _ => return None,
            })
        }
        "BinaryOperator" => {
            if inner.len() != 2 {
                return None;
            }
            let a = eval_const(&inner[0], td, known)?;
            let b = eval_const(&inner[1], td, known)?;
            let op = dstr(n, "opcode")?;
            match (a, b) {
                (CVal::Int(x), CVal::Int(y)) => {
                    let u = unsigned();
                    let v = match op.as_str() {
                        "+" => x.wrapping_add(y),
                        "-" => x.wrapping_sub(y),
                        "*" => x.wrapping_mul(y),
                        "/" if y != 0 => if u { ((x as u64) / (y as u64)) as i64 } else { x.wrapping_div(y) },
                        "%" if y != 0 => if u { ((x as u64) % (y as u64)) as i64 } else { x.wrapping_rem(y) },
                        "<<" if (0..64).contains(&y) => x.wrapping_shl(y as u32),
                        ">>" if (0..64).contains(&y) => if u { ((x as u64) >> y) as i64 } else { x >> y },
                        "|" => x | y,
                        "&" => x & y,
                        "^" => x ^ y,
                        "==" => (x == y) as i64,
                        "!=" => (x != y) as i64,
                        "<" => (x < y) as i64,
                        ">" => (x > y) as i64,
                        "<=" => (x <= y) as i64,
                        ">=" => (x >= y) as i64,
                        "&&" => (x != 0 && y != 0) as i64,
                        "||" => (x != 0 || y != 0) as i64,
                        _ => return None,
                    };
                    // Keep the width of the result type (`int` arithmetic wraps at 32 bits).
                    Some(match c_scalar(n, td) {
                        Some(t) => convert_scalar(CVal::Int(v), &t),
                        None => CVal::Int(v),
                    })
                }
                (a, b) => {
                    let f = |v: CVal| match v {
                        CVal::Int(i) => Some(i as f64),
                        CVal::Float(f) => Some(f),
                        CVal::Str(_) => None,
                    };
                    let (x, y) = (f(a)?, f(b)?);
                    Some(CVal::Float(match op.as_str() {
                        "+" => x + y,
                        "-" => x - y,
                        "*" => x * y,
                        "/" => x / y,
                        _ => return None,
                    }))
                }
            }
        }
        _ => None,
    }
}

fn parse_c_int(v: &str) -> Option<i64> {
    v.parse::<i64>().ok().or_else(|| v.parse::<u64>().ok().map(|u| u as i64))
}

// ----------------------------------------------------------- C types → storage

/// C scalar type → Siskin storage type (`F32`, `U32`, ...). The name is after typedefs are followed.
fn scalar_storage(r: &str) -> Option<&'static str> {
    let win = cfg!(windows);
    // Plain `char` is unsigned on ARM Linux, signed elsewhere.
    let char_unsigned = cfg!(all(target_arch = "aarch64", not(target_vendor = "apple"), not(windows)));
    Some(match r {
        "char" => if char_unsigned { "U8" } else { "I8" },
        "signed char" => "I8",
        "unsigned char" => "U8",
        "short" | "short int" | "signed short" | "signed short int" => "I16",
        "unsigned short" | "unsigned short int" => "U16",
        "int" | "signed" | "signed int" => "I32",
        "unsigned int" | "unsigned" => "U32",
        "long" | "long int" | "signed long" | "signed long int" => if win { "I32" } else { "Int" },
        "unsigned long" | "unsigned long int" => if win { "U32" } else { "U64" },
        "long long" | "long long int" | "signed long long" => "Int",
        "unsigned long long" | "unsigned long long int" => "U64",
        "__size_t" => "U64",
        "__signed_size_t" | "__ptrdiff_t" => "Int",
        "wchar_t" => if win { "U16" } else { "I32" },
        "_Bool" | "bool" => "Bool",
        "float" => "F32",
        "double" => "Float",
        _ => return None,
    })
}

/// Siskin value type of a storage type: what you get when you read it.
fn value_of_storage(s: &str) -> &'static str {
    match s {
        "F32" | "Float" => "Float",
        "Bool" => "Bool",
        _ => "Int",
    }
}

fn is_fnptr_text(r: &str) -> bool {
    r.contains("(*") || r.contains("(^")
}

fn cb_ty_text(cb: &CbSig) -> String {
    format!(
        "({}) -> {}",
        cb.params.iter().map(|m| m.siskin()).collect::<Vec<_>>().join(", "),
        cb.ret.map(|m| m.siskin()).unwrap_or("Unit")
    )
}

/// Whether the thing a pointer points to is `const` (`const T *`, `T *const *`).
fn pointee_const(q: &str) -> bool {
    let t = q.trim_end();
    let t = t.strip_suffix('*').unwrap_or(t).trim_end();
    t.ends_with("const") || (!t.contains('*') && t.split_whitespace().any(|w| w == "const"))
}

struct Scope<'a> {
    td: &'a HashMap<String, String>,
    /// How C spells a struct type (`VkExtent2D`, `struct VkExtent2D`) → Siskin name.
    smap: HashMap<String, String>,
}

impl<'a> Scope<'a> {
    fn struct_of(&self, q: &str) -> Option<String> {
        let t = tidy(q);
        if let Some(n) = self.smap.get(&t) {
            return Some(n.clone());
        }
        let r = resolve(q, self.td, 0);
        self.smap.get(&r).cloned()
    }

    /// The storage type of what a pointer points to, for `CPtr[...]`.
    fn pointee(&self, q: &str) -> String {
        let r = resolve(q, self.td, 0).replace(" *", "*");
        let r = r.trim();
        if r.ends_with('*') || is_fnptr_text(r) {
            return "Int".into(); // a pointer to a pointer: each slot is an address
        }
        if let Some(s) = self.struct_of(q) {
            return s;
        }
        if let Some(s) = scalar_storage(r) {
            return s.into();
        }
        if r.starts_with("enum ") {
            return "I32".into();
        }
        "Unit".into() // void, or a struct we only know by name (a handle)
    }

    /// One function parameter. Yields (Siskin type text, callback shape if it takes a function).
    fn param(&self, q: &str) -> Result<(String, MTy, Option<CbSig>), String> {
        let r = resolve(q, self.td, 0).replace(" *", "*");
        let r = r.trim().to_string();
        if is_plain_fnptr(&r) {
            if let Ok(cb) = parse_fnptr(q, self.td) {
                return Ok((cb_ty_text(&cb), MTy::Int, Some(cb)));
            }
            return Err(why_callback().into());
        }
        if let Some(s) = self.struct_of(q) {
            if !r.ends_with('*') {
                return Ok((s, MTy::Int, None)); // struct by value
            }
        }
        if r.ends_with('*') || r.ends_with(']') {
            let inner = if r.ends_with(']') {
                r[..r.find('[').unwrap_or(r.len())].trim().to_string()
            } else {
                r[..r.len() - 1].trim().to_string()
            };
            let depth = r.chars().filter(|c| *c == '*').count();
            let bytelike = matches!(inner.as_str(), "char" | "signed char" | "unsigned char");
            let konst = pointee_const(q) || {
                let base = tidy(q).trim_end_matches('*').trim().to_string();
                self.td.get(&base).map(|u| u.contains("const")).unwrap_or(false)
            };
            if depth == 1 && bytelike && konst {
                return Ok(("Str".into(), MTy::Str, None));
            }
            // What it points to, as written when the star is visible (keeps typedef names like `VkExtent2D`);
            // for a pointer hidden in a typedef (`VkDevice` = `struct VkDevice_T *`), the resolved type.
            let written = tidy(q).replace(" *", "*");
            let target = if r.ends_with(']') {
                self.pointee(&inner)
            } else if let Some(w) = written.strip_suffix('*') {
                self.pointee(w.trim())
            } else {
                self.pointee(&inner)
            };
            let k = if konst { "CConst" } else { "CPtr" };
            return Ok((format!("{}[{}]", k, target), MTy::Int, None));
        }
        match map_ty(q, self.td, false) {
            Ok(m) => Ok((m.siskin().into(), m, None)),
            Err(e) => Err(e),
        }
    }

    fn ret(&self, q: &str) -> Result<(String, MTy), String> {
        let r = resolve(q, self.td, 0).replace(" *", "*");
        if is_fnptr_text(&r) {
            return Ok(("Int".into(), MTy::Int)); // a function pointer comes back as an address
        }
        if !r.trim().ends_with('*') {
            if let Some(s) = self.struct_of(q) {
                return Ok((s, MTy::Int));
            }
        }
        map_ty(q, self.td, true).map(|m| (m.siskin().to_string(), m))
    }

    /// One struct field.
    fn field(&self, name: &str, q: &str) -> Result<CField, String> {
        let t = tidy(q);
        let mut f = CField { name: name.to_string(), ty: String::new(), c_ty: c_spelling(q), ptr: false, charp: false, chars: 0, cb: None };
        if let Some(open) = t.find('[') {
            let base = t[..open].trim().to_string();
            let mut dims = Vec::new();
            let mut rest = &t[open..];
            while let Some(r) = rest.strip_prefix('[') {
                let close = r.find(']').ok_or("?")?;
                let n: usize = r[..close].trim().parse().map_err(|_| tr!("크기가 정해지지 않은 배열", "array without a fixed size").to_string())?;
                dims.push(n);
                rest = r[close + 1..].trim_start();
            }
            if dims.is_empty() || dims.contains(&0) {
                return Err(tr!("크기가 정해지지 않은 배열", "array without a fixed size").into());
            }
            let rb = resolve(&base, self.td, 0);
            if dims.len() == 1 && rb == "char" {
                f.ty = "Str".into();
                f.chars = dims[0];
                return Ok(f);
            }
            let elem = self.field("", &base)?;
            if elem.chars > 0 || elem.cb.is_some() {
                return Err(tr!("배열 안에 배열·함수", "array of arrays of text or of functions").into());
            }
            let mut ty = elem.ty;
            for d in dims.iter().rev() {
                ty = format!("[{}; {}]", ty, d);
            }
            f.ty = ty;
            return Ok(f);
        }
        if let Some(s) = self.struct_of(q) {
            f.ty = s;
            return Ok(f);
        }
        let r = resolve(q, self.td, 0).replace(" *", "*");
        let r = r.trim();
        if is_plain_fnptr(r) {
            f.ty = "Int".into();
            f.ptr = true;
            f.cb = parse_fnptr(q, self.td).ok();
            return Ok(f);
        }
        if r.ends_with('*') {
            f.ty = "Int".into();
            f.ptr = true;
            let pointee = r[..r.len() - 1].trim();
            f.charp = matches!(pointee, "char" | "signed char" | "unsigned char");
            return Ok(f);
        }
        if let Some(s) = scalar_storage(r) {
            f.ty = s.into();
            return Ok(f);
        }
        if r.starts_with("enum ") {
            f.ty = "I32".into();
            return Ok(f);
        }
        if r.contains("(anonymous") || r.contains("(unnamed") {
            return Err(tr!("이름 없는 구조체 타입의 필드", "field of an unnamed struct type").into());
        }
        if r.starts_with("struct ") || r.starts_with("union ") {
            return Err(tr!("가져오지 않은 구조체", "a struct that was not imported").into());
        }
        Err(format!("{} `{}`", why_unknown_type(), t))
    }
}

fn is_true(n: &JRef, k: &str) -> bool {
    matches!(dget(n, k).map(|v| matches!(&*v.borrow(), JsonVal::Bool(true))), Some(true))
}

/// Fields of one record. Fields of an unnamed inner struct/union are lifted up, as C lets you write them.
fn record_fields(rec: &JRef, sc: &Scope, out: &mut CStruct) {
    let mut last_anon: Option<JRef> = None;
    for c in dlist(rec, "inner") {
        match dstr(&c, "kind").as_deref() {
            Some("RecordDecl") => {
                last_anon = if dstr(&c, "name").map(|n| n.is_empty()).unwrap_or(true) { Some(c.clone()) } else { None };
            }
            Some("FieldDecl") => {
                let q = dget(&c, "type").and_then(|t| dstr(&t, "qualType")).unwrap_or_default();
                match dstr(&c, "name").filter(|n| !n.is_empty()) {
                    None => {
                        if let Some(inner) = last_anon.take() {
                            record_fields(&inner, sc, out);
                        }
                    }
                    Some(name) => match sc.field(&name, &q) {
                        Ok(f) => out.fields.push(f),
                        Err(why) => out.skipped.push((name, why)),
                    },
                }
            }
            _ => {}
        }
    }
}

/// The record id a typedef names: `typedef struct {..} Vector2;` → the unnamed record.
fn typedef_record_id(n: &JRef) -> Option<String> {
    for c in dlist(n, "inner") {
        if dstr(&c, "kind").as_deref() == Some("RecordType") {
            if let Some(d) = dget(&c, "decl") {
                return dstr(&d, "id");
            }
        }
        if let Some(id) = typedef_record_id(&c) {
            return Some(id);
        }
    }
    None
}

// ------------------------------------------------------------------ main work

pub fn import_header(
    header: &str,
    cpp: bool,
    incdirs: &[String],
    only: &[String],
    defines: &[String],
) -> Result<Imported, String> {
    // Skip-reason texts differ by language, so the cache is per language too.
    let key = format!("{}|{}|{}|{}|v12{}", header, cpp, incdirs.join(":"), defines.join(":"), tr!("", "|en"));
    let cache = temp_dir().join(format!("{:016x}.txt", hash64(&key)));
    if let Some(hit) = load_cache(&cache) {
        return Ok(filter_only(hit, only));
    }

    if cpp {
        let text = run_clang(&include_line(header), true, incdirs, defines, &["-Xclang", "-ast-dump=json", "-fsyntax-only"], header)?;
        let root = json::parse(&text).map_err(|e| tr!(format!("clang이 낸 내용을 읽지 못했습니다: {}", e), format!("cannot parse clang output: {}", e)))?;
        let mut td: HashMap<String, String> = HashMap::new();
        collect_typedefs(&root, &mut td);
        let mut res = import_cpp(&root, header, &td);
        abs_path(&mut res);
        res.files = vec![(res.header_path.clone(), mtime(&res.header_path))];
        save_cache(&cache, &res);
        return Ok(filter_only(res, only));
    }

    // 1. The preprocessor: where the header is, which files it pulls in, and its `#define`s.
    let (found, macros) = read_macros(header, false, incdirs, defines)?;
    let files = if found.is_empty() { Vec::new() } else { follow_includes(&found, incdirs) };
    let mut wanted = Wanted { set: files.iter().cloned().collect(), header: header.to_string(), memo: HashMap::new() };

    // 2. Macros that might be numbers are handed back to clang to compute, as `enum` values
    //    appended after the header. Ones that are not integer constants simply fail and are dropped.
    let mut cands: Vec<(String, MacroKind)> = Vec::new();
    for (name, body, file) in &macros {
        if name.starts_with('_') || !wanted.has(file) {
            continue;
        }
        if let Some(k) = classify_macro(body) {
            cands.push((name.clone(), k));
        }
    }
    let mut probe = include_line(header);
    for (i, (name, k)) in cands.iter().enumerate() {
        if matches!(k, MacroKind::Expr) {
            probe.push_str(&format!("enum {{ __siskin_c{} = ({}) }};\n", i, name));
        }
    }

    let text = run_clang(&probe, false, incdirs, defines, &["-Xclang", "-ast-dump=json", "-fsyntax-only"], header)?;
    let root = json::parse(&text).map_err(|e| tr!(format!("clang이 낸 내용을 읽지 못했습니다: {}", e), format!("cannot parse clang output: {}", e)))?;

    let mut td: HashMap<String, String> = HashMap::new();
    collect_typedefs(&root, &mut td);

    let mut res = Imported::default();
    res.header_path = found.clone();
    let top = dlist(&root, "inner");

    // 3. Structs: first learn every name each record goes by, then read the fields.
    let mut cur = String::new();
    let mut recs: Vec<(String, JRef, Option<String>, bool)> = Vec::new(); // (id, node, tag, is_union)
    let mut td_of: HashMap<String, String> = HashMap::new();
    let mut evals: HashMap<String, i64> = HashMap::new();
    let mut enum_consts: Vec<(String, i64)> = Vec::new();
    // Every enum value by name, so `static const` initialisers can refer to them.
    let mut all_enums: HashMap<String, CVal> = HashMap::new();
    for n in &top {
        cur = loc_file(n, &cur);
        let kind = dstr(n, "kind").unwrap_or_default();
        if kind == "EnumDecl" {
            let mine = wanted.has(&cur);
            let mut prev: i64 = -1;
            for c in dlist(n, "inner") {
                if dstr(&c, "kind").as_deref() != Some("EnumConstantDecl") {
                    continue;
                }
                let name = dstr(&c, "name").unwrap_or_default();
                let v = first_value(&c).and_then(|v| parse_c_int(&v)).unwrap_or(prev.wrapping_add(1));
                prev = v;
                if !name.is_empty() {
                    all_enums.insert(name.clone(), CVal::Int(v));
                }
                if let Some(i) = name.strip_prefix("__siskin_c").and_then(|x| x.parse::<usize>().ok()) {
                    if first_value(&c).is_some() {
                        if let Some((mname, _)) = cands.get(i) {
                            evals.insert(mname.clone(), v);
                        }
                    }
                } else if mine && !name.is_empty() {
                    enum_consts.push((name, v));
                }
            }
            continue;
        }
        if !wanted.has(&cur) {
            continue;
        }
        if kind == "RecordDecl" && is_true(n, "completeDefinition") {
            let id = dstr(n, "id").unwrap_or_default();
            let tag = dstr(n, "name").filter(|s| !s.is_empty());
            let is_union = dstr(n, "tagUsed").as_deref() == Some("union");
            recs.push((id, n.clone(), tag, is_union));
        } else if kind == "TypedefDecl" {
            let name = dstr(n, "name").unwrap_or_default();
            if let Some(id) = typedef_record_id(n) {
                let q = dget(n, "type").and_then(|t| dstr(&t, "qualType")).unwrap_or_default();
                // `typedef struct {..} png_image, *png_imagep;` — the pointer name is not the struct's name.
                if !q.trim_end().ends_with('*') {
                    td_of.entry(id).or_insert(name);
                }
            }
        }
    }
    let mut sc = Scope { td: &td, smap: HashMap::new() };
    let mut named: Vec<(String, String, JRef, bool)> = Vec::new(); // (siskin name, c name, node, union)
    for (id, node, tag, is_union) in &recs {
        let kw = if *is_union { "union" } else { "struct" };
        let (name, c_name) = match (td_of.get(id), tag) {
            (Some(t), _) => (t.clone(), t.clone()),
            (None, Some(tag)) => (tag.clone(), format!("{} {}", kw, tag)),
            (None, None) => continue,
        };
        if sc.smap.values().any(|v| v == &name) {
            continue;
        }
        sc.smap.insert(c_name.clone(), name.clone());
        sc.smap.insert(name.clone(), name.clone());
        // How clang spells an unnamed struct behind a typedef: `struct png_image` (newer) or
        // `struct (unnamed struct at png.h:2667:9)` (older). A pointer typedef such as
        // `typedef struct {..} png_image, *png_imagep;` resolves to that spelling plus `*`.
        if let Some(q) = td.get(&name) {
            sc.smap.entry(tidy(q)).or_insert(name.clone());
        }
        if let Some(tag) = tag {
            sc.smap.insert(format!("{} {}", kw, tag), name.clone());
        }
        named.push((name, c_name, node.clone(), *is_union));
    }
    for (name, c_name, node, is_union) in &named {
        let mut st = CStruct { name: name.clone(), c_name: c_name.clone(), is_union: *is_union, fields: Vec::new(), skipped: Vec::new() };
        record_fields(node, &sc, &mut st);
        res.structs.push(st);
    }

    // 4. Functions, function pointer types and `static const` values.
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut var_consts: Vec<(String, CVal)> = Vec::new();
    let mut known = all_enums;
    cur.clear();
    for n in &top {
        cur = loc_file(n, &cur);
        let kind = dstr(n, "kind").unwrap_or_default();
        if !wanted.has(&cur) {
            continue;
        }
        if kind == "VarDecl" {
            // `static const VkPipelineStageFlagBits2 VK_PIPELINE_STAGE_2_NONE = 0ULL;`
            if let Some((name, v)) = const_var(n, &td, &known) {
                known.insert(name.clone(), v.clone());
                var_consts.push((name, v));
            }
            continue;
        }
        if kind == "TypedefDecl" {
            let name = dstr(n, "name").unwrap_or_default();
            let q = dget(n, "type").and_then(|t| dstr(&t, "qualType")).unwrap_or_default();
            let r = resolve(&q, &td, 0);
            if is_fnptr_text(&r) && !name.starts_with('_') {
                match parse_fnptr(&q, &td) {
                    Ok(sig) => res.fnptrs.push(CFnPtr { name, sig }),
                    Err(why) => res.skipped_fnptrs.push((name, why)),
                }
            }
            continue;
        }
        if kind != "FunctionDecl" {
            continue;
        }
        let name = match dstr(n, "name") {
            Some(x) => x,
            None => continue,
        };
        if seen.contains_key(&name) {
            continue;
        }
        seen.insert(name.clone(), ());

        let ty = match dget(n, "type") {
            Some(t) => t,
            None => continue,
        };
        let qual = dstr(&ty, "qualType").unwrap_or_default();
        if qual.contains("...") {
            res.skipped.push((name, why_variadic()));
            continue;
        }
        let (ret_c, _) = split_sig(&qual);
        let params_c = param_types(n);

        let (ret_ty, ret) = match sc.ret(&ret_c) {
            Ok(t) => t,
            Err(why) => {
                res.skipped.push((name, why));
                continue;
            }
        };
        let mut params = Vec::new();
        let mut tys = Vec::new();
        let mut cbs: Vec<Option<CbSig>> = Vec::new();
        let mut bad = None;
        for p in &params_c {
            match sc.param(p) {
                Ok((t, m, cb)) => {
                    tys.push(t);
                    params.push(m);
                    cbs.push(cb);
                }
                Err(why) => {
                    bad = Some(why);
                    break;
                }
            }
        }
        if let Some(why) = bad {
            res.skipped.push((name, why));
            continue;
        }
        let outs = vec![false; params.len()];
        let c_ret = c_spelling(&ret_c);
        let c_params = params_c.iter().map(|p| c_spelling(p)).collect();
        res.fns.push(ImportedFn { name, ret, params, c_ret, c_params, cpp_shim: None, outs, cbs, tys, ret_ty });
    }

    // 5. Constants: enum values first, then `#define`s, in the order they appear.
    let mut taken: std::collections::HashSet<String> = res.fns.iter().map(|f| f.name.clone()).collect();
    taken.extend(res.structs.iter().map(|s| s.name.clone()));
    taken.extend(res.fnptrs.iter().map(|f| f.name.clone()));
    let mut push = |res: &mut Imported, name: &str, val: CVal| {
        if taken.contains(name) || crate::lexer::is_keyword(name) || crate::types::SISKIN_BUILTINS.contains(&name) {
            return;
        }
        taken.insert(name.to_string());
        res.consts.push(CConst { name: name.to_string(), val });
    };
    for (n, v) in &enum_consts {
        push(&mut res, n, CVal::Int(*v));
    }
    for (n, v) in var_consts {
        push(&mut res, &n, v);
    }
    for (name, k) in cands {
        let val = match k {
            MacroKind::Str(s) => CVal::Str(s),
            MacroKind::Float(f) => CVal::Float(f),
            MacroKind::Expr => match evals.get(&name) {
                Some(v) => CVal::Int(*v),
                None => continue,
            },
        };
        push(&mut res, &name, val);
    }

    abs_path(&mut res);
    res.files = files.iter().map(|f| (f.clone(), mtime(f))).collect();
    save_cache(&cache, &res);
    Ok(filter_only(res, only))
}

/// Turn the location reported by clang into an absolute path, so the same header
/// is seen even when compiling later from a different folder.
fn abs_path(im: &mut Imported) {
    if let Ok(p) = crate::canonicalize(&im.header_path) {
        // Inside C's `#include "..."`, `\` is an escape character, so Windows paths are written with `/` too.
        im.header_path = p.to_string_lossy().replace('\\', "/");
    }
}

fn filter_only(mut im: Imported, only: &[String]) -> Imported {
    if only.is_empty() {
        return im;
    }
    im.fns.retain(|f| only.iter().any(|o| o == &f.name));
    im.consts.retain(|f| only.iter().any(|o| o == &f.name));
    im.fnptrs.retain(|f| only.iter().any(|o| o == &f.name));
    im
}

// -------------------------------------------------------------------- cache
// clang takes over a second, so results are cached. Discarded when a header changes.
// Lists inside one column are joined with \x1f, and a callback shape's parts with \x1e.

const SEP: char = '\u{1f}';
const SUB: char = '\u{1e}';

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t")
}

fn enc_cb(cb: &Option<CbSig>) -> String {
    match cb {
        None => "-".into(),
        Some(cb) => format!(
            "{}{s}{}{s}{}{s}{}",
            cb.ret.map(|m| m.siskin().to_string()).unwrap_or_default(),
            cb.params.iter().map(|m| m.siskin()).collect::<Vec<_>>().join(","),
            cb.c_ret,
            cb.c_params.join("\u{1d}"),
            s = SUB
        ),
    }
}

fn dec_cb(x: &str) -> Option<CbSig> {
    if x == "-" {
        return None;
    }
    let q: Vec<&str> = x.split(SUB).collect();
    if q.len() < 4 {
        return None;
    }
    Some(CbSig {
        ret: if q[0].is_empty() { None } else { Some(mty_of(q[0])) },
        params: if q[1].is_empty() { Vec::new() } else { q[1].split(',').map(mty_of).collect() },
        c_ret: q[2].to_string(),
        c_params: if q[3].is_empty() { Vec::new() } else { q[3].split('\u{1d}').map(|x| x.to_string()).collect() },
    })
}

fn join(v: &[String]) -> String {
    v.join(&SEP.to_string())
}

fn split(s: &str) -> Vec<String> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.split(SEP).map(|x| x.to_string()).collect()
    }
}

fn save_cache(path: &PathBuf, im: &Imported) {
    let mut s = String::from("siskin-ffi 15\n");
    s.push_str(&format!("H\t{}\n", im.header_path));
    for (f, t) in &im.files {
        s.push_str(&format!("W\t{}\t{}\n", f, t));
    }
    for f in &im.fns {
        let ps: Vec<String> = f.params.iter().map(|t| t.siskin().to_string()).collect();
        s.push_str(&format!(
            "F\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            f.name,
            f.ret.siskin(),
            join(&ps),
            f.c_ret,
            join(&f.c_params),
            f.outs.iter().map(|b| if *b { '1' } else { '0' }).collect::<String>(),
            f.cbs.iter().map(enc_cb).collect::<Vec<_>>().join(&SEP.to_string()),
            esc(&f.cpp_shim.clone().unwrap_or_default()),
            join(&f.tys),
            f.ret_ty,
        ));
    }
    for c in &im.consts {
        let (k, v) = match &c.val {
            CVal::Int(i) => ("i", i.to_string()),
            CVal::Float(f) => ("f", format!("{:?}", f)),
            CVal::Str(x) => ("s", esc(x)),
        };
        s.push_str(&format!("C\t{}\t{}\t{}\n", c.name, k, v));
    }
    for st in &im.structs {
        s.push_str(&format!("T\t{}\t{}\t{}\n", st.name, st.c_name, if st.is_union { 1 } else { 0 }));
        for f in &st.fields {
            s.push_str(&format!(
                "f\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                f.name,
                f.ty,
                f.c_ty,
                if f.ptr { 1 } else { 0 },
                if f.charp { 1 } else { 0 },
                f.chars,
                enc_cb(&f.cb)
            ));
        }
        for (n, w) in &st.skipped {
            s.push_str(&format!("x\t{}\t{}\n", n, w));
        }
    }
    for p in &im.fnptrs {
        s.push_str(&format!("P\t{}\t{}\n", p.name, enc_cb(&Some(p.sig.clone()))));
    }
    for (n, w) in &im.skipped {
        s.push_str(&format!("S\t{}\t{}\n", n, w));
    }
    for (n, w) in &im.skipped_fnptrs {
        s.push_str(&format!("Q\t{}\t{}\n", n, w));
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
    if lines.next()? != "siskin-ffi 15" {
        return None;
    }
    let mut im = Imported::default();
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first() {
            Some(&"H") if f.len() >= 2 => im.header_path = f[1].to_string(),
            Some(&"W") if f.len() >= 3 => {
                let want: u64 = f[2].parse().unwrap_or(0);
                if mtime(f[1]) != want {
                    return None; // a header changed
                }
                im.files.push((f[1].to_string(), want));
            }
            Some(&"F") if f.len() >= 11 => {
                im.fns.push(ImportedFn {
                    name: f[1].to_string(),
                    ret: mty_of(f[2]),
                    params: split(f[3]).iter().map(|x| mty_of(x)).collect(),
                    c_ret: f[4].to_string(),
                    c_params: split(f[5]),
                    outs: f[6].chars().map(|c| c == '1').collect(),
                    cbs: if f[7].is_empty() { Vec::new() } else { f[7].split(SEP).map(dec_cb).collect() },
                    cpp_shim: Some(unesc(f[8])).filter(|x| !x.is_empty()),
                    tys: split(f[9]),
                    ret_ty: f[10].to_string(),
                });
            }
            Some(&"C") if f.len() >= 4 => {
                let val = match f[2] {
                    "i" => CVal::Int(f[3].parse().ok()?),
                    "f" => CVal::Float(f[3].parse().ok()?),
                    _ => CVal::Str(unesc(f[3])),
                };
                im.consts.push(CConst { name: f[1].to_string(), val });
            }
            Some(&"T") if f.len() >= 4 => im.structs.push(CStruct {
                name: f[1].to_string(),
                c_name: f[2].to_string(),
                is_union: f[3] == "1",
                fields: Vec::new(),
                skipped: Vec::new(),
            }),
            Some(&"f") if f.len() >= 8 => {
                let st = im.structs.last_mut()?;
                st.fields.push(CField {
                    name: f[1].to_string(),
                    ty: f[2].to_string(),
                    c_ty: f[3].to_string(),
                    ptr: f[4] == "1",
                    charp: f[5] == "1",
                    chars: f[6].parse().unwrap_or(0),
                    cb: dec_cb(f[7]),
                });
            }
            Some(&"x") if f.len() >= 3 => {
                let st = im.structs.last_mut()?;
                st.skipped.push((f[1].to_string(), f[2].to_string()));
            }
            Some(&"P") if f.len() >= 3 => im.fnptrs.push(CFnPtr { name: f[1].to_string(), sig: dec_cb(f[2])? }),
            Some(&"S") if f.len() >= 3 => im.skipped.push((f[1].to_string(), f[2].to_string())),
            Some(&"Q") if f.len() >= 3 => im.skipped_fnptrs.push((f[1].to_string(), f[2].to_string())),
            _ => {}
        }
    }
    Some(im)
}

// ===================================================================== C++
//
// C++ stores names mangled (`geo::add(int,int)` -> `_ZN3geo3addEii`), and has things C lacks
// such as classes, virtual functions and templates, so C cannot call it directly.
// So we put a bridge in between: for each C++ function we generate
// a wrapper in `extern "C"` and hand it to the C++ compiler as well; the C++ compiler then
// takes care of mangled names, virtual functions and templates.

/// How to translate one C++ parameter/return value.
#[derive(Clone, Debug)]
struct CppSlot {
    mty: MTy,
    /// Original type on the C++ side.
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

/// How to pass one parameter: `(siskin type, expression used inside the wrapper)`.
fn cpp_param(q: &str, td: &HashMap<String, String>, idx: usize) -> Result<(CppSlot, String), String> {
    let a = format!("a{}", idx);
    let t = q.trim();
    if is_string_type(t) {
        return Ok((CppSlot { mty: MTy::Str, cpp: t.into() }, format!("std::string({})", a)));
    }
    let bare = strip_quals_cpp(t);
    // References (`T&`) take a handle and point at that location.
    if let Some(inner) = bare.strip_suffix('&') {
        let inner = inner.trim().trim_end_matches('&').trim();
        if is_string_type(inner) {
            return Ok((CppSlot { mty: MTy::Str, cpp: t.into() }, format!("std::string({})", a)));
        }
        if let Ok(m) = map_ty(inner, td, false) {
            if m != MTy::Int {
                // Things like `double&` cannot be taken by value.
                return Err(tr!("참조로 값을 돌려주는 인자", "parameter that returns a value by reference").into());
            }
        }
        let base = strip_quals_cpp(inner).to_string();
        return Ok((
            CppSlot { mty: MTy::Int, cpp: t.into() },
            format!("(*({}*)(void*){})", base, a),
        ));
    }
    // Otherwise use the same rules as C.
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
            // Taking a class by value needs a copy, so it is not supported yet.
            if why.starts_with(why_unknown_type()) {
                Err(tr!("C++ 값(클래스)을 그대로 받는 인자", "parameter taking a C++ class by value").into())
            } else {
                Err(why)
            }
        }
    }
}

/// How to return: `(siskin type, template wrapping the call expression)`.
/// The actual call expression goes into the `{}` slot of the template.
fn cpp_ret(q: &str, td: &HashMap<String, String>) -> Result<(MTy, String), String> {
    let t = q.trim();
    if t == "void" {
        return Ok((MTy::Unit, "{};".into()));
    }
    if is_string_type(t) {
        // std::string is returned as a string. Siskin copies it immediately, so
        // a few rotating slots are enough.
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
            // A class returned by value is copied to the heap and a handle is returned.
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
        params: Vec<(String, String)>, // (C++ type, name unused)
        recv: Option<String>,          // class name (for methods)
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
        let shim = c_spelling(&format!(
            "/* {} */\nextern \"C\" {} mx_{}({}) {{\n    {}\n}}\n",
            pretty,
            rty.cty(),
            siskin_name,
            plist,
            body
        ));
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
            tys: Vec::new(),
            ret_ty: String::new(),
        });
    }
}

fn split_sig(qual: &str) -> (String, Vec<String>) {
    // `int (int, double)` -> ("int", ["int","double"])  — counts matching parentheses.
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
                // The destructor is generated by C++ implicitly, so it is not in the header.
                // Whoever receives a handle must be able to delete it, so we generate one.
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
                if ctx.out.header_path.is_empty() || !std::path::Path::new(&ctx.out.header_path).is_absolute() {
                    ctx.out.header_path = cur.clone();
                }
                cpp_entity(&c, &kind, ns, &name, ctx);
            }
            // Templates are instantiated at each use, so they cannot be imported ahead of time.
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
    // Skip compiler-generated, deleted and non-public items.
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
            // Constructors must return a handle, so patch the return template directly.
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
