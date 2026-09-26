//! Package management (first version).
//!
//! - `siskin new NAME`       Creates a new project folder (siskin.toml, main.skn).
//! - `siskin add NAME SRC`   Uses a package made by someone else. SRC is a git repository or a local folder.
//! - `siskin install`        Fetches the packages listed in siskin.toml. The fetched revision (commit) is recorded
//!                         in siskin.lock so other machines fetch exactly the same revision.
//! - `siskin remove NAME`    Removes a package.
//!
//! Fetched packages go into the project's `.siskin/deps/NAME/`; `import NAME` reads that folder's
//! `lib.skn`, and `import NAME.part` reads `part.skn`.
//! - `siskin add NAME`       Given only a name, looks up the source in the package index (registry).
//! - `siskin search WORD`    Searches the package index.
//! - `siskin publish`        Generates the file to submit when listing your package in the index.
//!
//! The package index is a single git repository, not a server (the same approach as crates.io-index and Homebrew taps).
//! Each `packages/NAME.toml` file in the repository is one package, containing `git = "SRC"` and
//! `description = "..."`. To publish, send a PR adding one file to that repository.
//! The index location can be changed with the `SISKIN_REGISTRY` environment variable or `[registry] url = "..."` in siskin.toml
//! (a git URL or a local folder). The fetched index is kept in `~/.siskin/registry/` and refreshed on each use.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Git { url: String, rev: Option<String> },
    Path(String),
}

#[derive(Debug, Default)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub deps: BTreeMap<String, Source>,
    pub description: String,
    /// `[registry] url = "..."`: the package index this project uses
    pub registry: Option<String>,
}

/// Reads only a tiny subset of TOML: `[section]`, `key = "text"`, `key = { key = "text", ... }`.
fn parse_toml(text: &str) -> Result<BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>, String> {
    // section → key → (keys inside if the value is a table; a single "" key if it is plain text)
    let mut out: BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>> = BTreeMap::new();
    let mut section = String::new();
    for (i, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim().to_string();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') {
                return Err(tr!(format!("{}번째 줄: `[` 를 `]` 로 닫아 주세요", i + 1), format!("line {}: close `[` with `]`", i + 1)));
            }
            section = line[1..line.len() - 1].trim().trim_matches('"').to_string();
            out.entry(section.clone()).or_default();
            continue;
        }
        let (k, v) = match line.split_once('=') {
            Some((k, v)) => (k.trim().trim_matches('"').to_string(), v.trim().to_string()),
            None => {
                return Err(tr!(
                    format!("{}번째 줄: `이름 = 값` 모양이어야 합니다", i + 1),
                    format!("line {}: expected `name = value`", i + 1)
                ))
            }
        };
        let mut val: BTreeMap<String, String> = BTreeMap::new();
        if v.starts_with('{') {
            if !v.ends_with('}') {
                return Err(tr!(
                    format!("{}번째 줄: `{{` 를 같은 줄에서 `}}` 로 닫아 주세요", i + 1),
                    format!("line {}: close `{{` with `}}` on the same line", i + 1)
                ));
            }
            for part in split_top(&v[1..v.len() - 1]) {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                match part.split_once('=') {
                    Some((a, b)) => {
                        val.insert(a.trim().to_string(), unquote(b.trim()).map_err(|e| line_err(i + 1, e))?);
                    }
                    None => {
                        return Err(tr!(
                            format!("{}번째 줄: `{{ 이름 = 값 }}` 모양이어야 합니다", i + 1),
                            format!("line {}: expected `{{ name = value }}`", i + 1)
                        ))
                    }
                }
            }
        } else {
            val.insert(String::new(), unquote(&v).map_err(|e| line_err(i + 1, e))?);
        }
        out.entry(section.clone()).or_default().insert(k, val);
    }
    Ok(out)
}

fn line_err(n: usize, e: String) -> String {
    tr!(format!("{}번째 줄: {}", n, e), format!("line {}: {}", n, e))
}

fn strip_comment(s: &str) -> String {
    let mut out = String::new();
    let mut in_str = false;
    for c in s.chars() {
        if c == '"' {
            in_str = !in_str;
        }
        if c == '#' && !in_str {
            break;
        }
        out.push(c);
    }
    out
}

fn split_top(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    for c in s.chars() {
        if c == '"' {
            in_str = !in_str;
        }
        if c == ',' && !in_str {
            parts.push(std::mem::take(&mut cur));
            continue;
        }
        cur.push(c);
    }
    parts.push(cur);
    parts
}

fn unquote(v: &str) -> Result<String, String> {
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        Ok(v[1..v.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\"))
    } else {
        Err(tr!(format!("`{}` 는 따옴표로 감싸 주세요", v), format!("put `{}` in double quotes", v)))
    }
}

pub fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    let p = dir.join("siskin.toml");
    let text = std::fs::read_to_string(&p).map_err(|e| {
        tr!(format!("{} 을(를) 읽을 수 없습니다 ({})", p.display(), e), format!("cannot read {} ({})", p.display(), e))
    })?;
    let t = parse_toml(&text).map_err(|e| format!("siskin.toml {}", e))?;
    let mut m = Manifest::default();
    if let Some(pk) = t.get("package") {
        m.name = pk.get("name").and_then(|v| v.get("")).cloned().unwrap_or_default();
        m.version = pk.get("version").and_then(|v| v.get("")).cloned().unwrap_or_default();
        m.description = pk.get("description").and_then(|v| v.get("")).cloned().unwrap_or_default();
    }
    if let Some(r) = t.get("registry") {
        m.registry = r.get("url").and_then(|v| v.get("")).cloned();
    }
    if let Some(ds) = t.get("dependencies") {
        for (name, v) in ds {
            let src = if let Some(p) = v.get("path") {
                Source::Path(p.clone())
            } else if let Some(g) = v.get("git").or_else(|| v.get("")) {
                Source::Git { url: g.clone(), rev: v.get("rev").cloned() }
            } else {
                return Err(tr!(
                    format!("siskin.toml: 패키지 `{}` 에 git 주소나 path 가 없습니다", name),
                    format!("siskin.toml: package `{}` has no git URL or path", name)
                ));
            };
            m.deps.insert(name.clone(), src);
        }
    }
    Ok(m)
}

/// Find the folder containing siskin.toml (the project root) in this folder or above.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let start = crate::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    let mut cur: Option<&Path> = Some(&start);
    while let Some(d) = cur {
        if d.join("siskin.toml").is_file() {
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

thread_local! {
    /// Project root of the program being compiled (the main file). All packages fetched via git
    /// are placed flat in `.siskin/deps/` here (including packages used by packages).
    static PROJECT_ROOT: std::cell::RefCell<Option<PathBuf>> = std::cell::RefCell::new(None);
}

pub fn set_project_root(main_file: &Path) {
    let dir = main_file.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let dir = if dir.as_os_str().is_empty() { PathBuf::from(".") } else { dir };
    PROJECT_ROOT.with(|r| *r.borrow_mut() = find_root(&dir));
}

fn package_file(name: &str, root: &Path, ipath: &[String]) -> Result<PathBuf, String> {
    if !root.is_dir() {
        return Err(tr!(
            format!("패키지 `{}` 이(가) 아직 설치되지 않았습니다. `siskin install` 을 실행하세요", name),
            format!("package `{}` is not installed yet; run `siskin install`", name)
        ));
    }
    let file = if ipath.len() == 1 { root.join("lib.skn") } else { root.join(format!("{}.skn", ipath[1..].join("/"))) };
    if !file.exists() {
        let rel = file.strip_prefix(root).map(|p| p.display().to_string()).unwrap_or_default();
        return Err(tr!(
            format!("패키지 `{}` 안에 `{}` 이(가) 없습니다", name, rel),
            format!("package `{}` has no `{}`", name, rel)
        ));
    }
    Ok(file)
}

/// Find the file `import a.b` refers to. `a/b.skn` in the same folder comes first;
/// otherwise look in package `a`.
pub fn resolve_module(from_dir: &Path, ipath: &[String]) -> Result<PathBuf, String> {
    let local = from_dir.join(format!("{}.skn", ipath.join("/")));
    if local.exists() || ipath.is_empty() {
        return Ok(local);
    }
    let name = &ipath[0];
    let top = PROJECT_ROOT.with(|r| r.borrow().clone());
    let installed = |d: &Path| -> PathBuf {
        top.clone().unwrap_or_else(|| d.to_path_buf()).join(".siskin").join("deps").join(name)
    };
    // Walk up from this file looking for siskin.toml. Folder packages use paths relative to that siskin.toml.
    let start = crate::canonicalize(from_dir).unwrap_or_else(|_| from_dir.to_path_buf());
    let mut cur: Option<&Path> = Some(&start);
    while let Some(d) = cur {
        if d.join("siskin.toml").is_file() {
            if let Ok(m) = read_manifest(d) {
                match m.deps.get(name) {
                    Some(Source::Path(p)) => return package_file(name, &d.join(p), ipath),
                    Some(Source::Git { .. }) => return package_file(name, &installed(d), ipath),
                    None => {}
                }
            }
        }
        cur = d.parent();
    }
    // Packages not listed but already fetched (packages used by packages).
    if let Some(t) = &top {
        let r = t.join(".siskin").join("deps").join(name);
        if r.is_dir() {
            return package_file(name, &r, ipath);
        }
    }
    let fname = local.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    Err(tr!(
        format!(
            "`{}` 을(를) 찾을 수 없습니다. 같은 폴더에 `{}` 이(가) 없고, 패키지도 아닙니다",
            ipath.join("."),
            fname
        ),
        format!(
            "cannot find `{}`: there is no `{}` in the same folder, and it is not a package",
            ipath.join("."),
            fname
        )
    ))
}

// ---------------------------------------------------------------- commands

fn git(args: &[&str], dir: Option<&Path>) -> Result<String, String> {
    let mut c = Command::new("git");
    if let Some(d) = dir {
        c.arg("-C").arg(d);
    }
    c.args(args);
    c.env("GIT_TERMINAL_PROMPT", "0");
    let out = c.output().map_err(|_| tr!("git 을 찾을 수 없습니다. git 을 먼저 설치해 주세요", "cannot find git; please install git first").to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        // With multiple lines, the `fatal:` line is the most useful.
        let line = err.lines().find(|l| l.starts_with("fatal:")).or_else(|| err.lines().last());
        return Err(line.unwrap_or(tr!("git 이 실패했습니다", "git failed")).to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn read_lock(root: &Path) -> BTreeMap<String, (String, String)> {
    let mut out = BTreeMap::new();
    if let Ok(text) = std::fs::read_to_string(root.join("siskin.lock")) {
        if let Ok(t) = parse_toml(&text) {
            for (name, kv) in t {
                let url = kv.get("git").and_then(|v| v.get("")).cloned().unwrap_or_default();
                let commit = kv.get("commit").and_then(|v| v.get("")).cloned().unwrap_or_default();
                if !url.is_empty() && !commit.is_empty() {
                    out.insert(name, (url, commit));
                }
            }
        }
    }
    out
}

fn write_lock(root: &Path, lock: &BTreeMap<String, (String, String)>) -> Result<(), String> {
    // The header comment is always the same regardless of language, so team members with different languages don't cause git diffs.
    let mut s = String::from(
        "# Generated by `siskin install`. Records the exact version of each fetched package.\n# Do not edit by hand; commit this file to git too.\n",
    );
    for (name, (url, commit)) in lock {
        s.push_str(&format!("\n[{}]\ngit = \"{}\"\ncommit = \"{}\"\n", name, url, commit));
    }
    std::fs::write(root.join("siskin.lock"), s).map_err(|e| tr!(format!("siskin.lock 을 쓸 수 없습니다 ({})", e), format!("cannot write siskin.lock ({})", e)))
}

/// Fetch packages (including the packages they use). Record the fetched revisions in the lock file.
fn install_all(root: &Path, update: bool, fresh: Option<&str>) -> Result<usize, String> {
    let manifest = read_manifest(root)?;
    let old_lock = read_lock(root);
    let mut lock: BTreeMap<String, (String, String)> = BTreeMap::new();
    let deps_dir = root.join(".siskin").join("deps");
    // (name, source, folder where this requirement was declared, who required it)
    let mut queue: Vec<(String, Source, PathBuf, String)> =
        manifest.deps.iter().map(|(n, s)| (n.clone(), s.clone(), root.to_path_buf(), "siskin.toml".to_string())).collect();
    let mut done: BTreeMap<String, (Source, String)> = BTreeMap::new();
    let mut count = 0;
    while let Some((name, src, base, who)) = queue.pop() {
        if let Some((prev, prev_who)) = done.get(&name) {
            let same = match (prev, &src) {
                (Source::Git { url: a, .. }, Source::Git { url: b, .. }) => a == b,
                (a, b) => a == b,
            };
            if !same {
                return Err(tr!(
                    format!(
                        "패키지 `{}` 을(를) {} 와(과) {} 가 서로 다른 곳에서 가져오려 합니다. 한쪽으로 맞춰 주세요",
                        name, prev_who, who
                    ),
                    format!(
                        "{} and {} want package `{}` from different sources; make them agree",
                        prev_who, who, name
                    )
                ));
            }
            continue;
        }
        done.insert(name.clone(), (src.clone(), who.clone()));
        let pkg_dir = match &src {
            Source::Path(p) => {
                let d = base.join(p);
                if !d.is_dir() {
                    return Err(tr!(
                        format!("패키지 `{}` 의 폴더 `{}` 가 없습니다", name, d.display()),
                        format!("folder `{}` for package `{}` does not exist", d.display(), name)
                    ));
                }
                println!("  {} ({} {})", name, tr!("폴더", "folder"), d.display());
                d
            }
            Source::Git { url, rev } => {
                let dest = deps_dir.join(&name);
                std::fs::create_dir_all(&deps_dir).map_err(|e| {
                    tr!(format!(".siskin/deps 를 만들 수 없습니다 ({})", e), format!("cannot create .siskin/deps ({})", e))
                })?;
                if !dest.join(".git").exists() {
                    let _ = std::fs::remove_dir_all(&dest);
                    println!("  {} {} ({})", name, tr!("받는 중...", "fetching..."), url);
                    git(&["clone", "--quiet", url, &dest.to_string_lossy()], None)
                        .map_err(|e| tr!(format!("`{}` 를 받을 수 없습니다: {}", url, e), format!("cannot fetch `{}`: {}", url, e)))?;
                } else if update || rev.is_some() {
                    let _ = git(&["fetch", "--quiet", "--tags", "origin"], Some(&dest));
                }
                // Revision to pick: the one in the lock file (if the source matches), else rev, else the latest on the default branch.
                let locked = if fresh == Some(name.as_str()) {
                    None
                } else {
                    old_lock.get(&name).filter(|(u, _)| u == url).map(|(_, c)| c.clone())
                };
                let target = match (&locked, rev, update) {
                    (Some(c), _, false) => c.clone(),
                    (_, Some(r), _) => r.clone(),
                    _ => {
                        let _ = git(&["fetch", "--quiet", "origin"], Some(&dest));
                        let head = git(&["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"], Some(&dest))
                            .unwrap_or_else(|_| "origin/HEAD".to_string());
                        head
                    }
                };
                let spec = format!("{}^{{commit}}", target);
                let found = git(&["rev-parse", "--verify", "--quiet", &spec], Some(&dest))
                    .or_else(|_| git(&["rev-parse", "--verify", "--quiet", &format!("origin/{}^{{commit}}", target)], Some(&dest)))
                    .map_err(|_| {
                        tr!(
                            format!("패키지 `{}` 에 `{}` 이라는 판(태그, 가지, commit)이 없습니다", name, target),
                            format!("package `{}` has no version (tag, branch or commit) named `{}`", name, target)
                        )
                    })?;
                git(&["checkout", "--quiet", "--detach", &found], Some(&dest))
                    .map_err(|e| {
                        tr!(
                            format!("패키지 `{}` 의 판 `{}` 으로 바꿀 수 없습니다: {}", name, target, e),
                            format!("cannot switch package `{}` to version `{}`: {}", name, target, e)
                        )
                    })?;
                let commit = git(&["rev-parse", "HEAD"], Some(&dest))?;
                println!("  {} {}", name, &commit[..commit.len().min(10)]);
                lock.insert(name.clone(), (url.clone(), commit));
                dest
            }
        };
        count += 1;
        if !pkg_dir.join("lib.skn").exists() {
            if crate::lang::ko() {
                println!("  (주의: `{}` 에 lib.skn 가 없어서 `import {}` 는 안 되고 `import {}.파일이름` 으로만 씁니다)", name, name, name);
            } else {
                println!(
                    "  (note: `{}` has no lib.skn, so `import {}` will not work; use `import {}.filename` instead)",
                    name, name, name
                );
            }
        }
        if pkg_dir.join("siskin.toml").is_file() {
            let sub = read_manifest(&pkg_dir)?;
            for (n, s) in sub.deps {
                queue.push((n, s, pkg_dir.clone(), format!("`{}`", name)));
            }
        }
    }
    write_lock(root, &lock)?;
    Ok(count)
}

fn add_dep_line(root: &Path, name: &str, value: &str) -> Result<(), String> {
    let p = root.join("siskin.toml");
    let text = std::fs::read_to_string(&p).map_err(toml_read_err)?;
    let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    // Replace it if it already exists.
    let mut in_deps = false;
    let mut deps_at: Option<usize> = None;
    let mut last_in_deps: Option<usize> = None;
    let mut replaced = false;
    for i in 0..lines.len() {
        let t = strip_comment(&lines[i]).trim().to_string();
        if t.starts_with('[') {
            in_deps = t == "[dependencies]";
            if in_deps {
                deps_at = Some(i);
                last_in_deps = Some(i);
            }
            continue;
        }
        if in_deps && !t.is_empty() {
            last_in_deps = Some(i);
            if t.split('=').next().map(|k| k.trim().trim_matches('"')) == Some(name) {
                lines[i] = format!("{} = {}", name, value);
                replaced = true;
            }
        }
    }
    if !replaced {
        match (deps_at, last_in_deps) {
            (Some(_), Some(at)) => lines.insert(at + 1, format!("{} = {}", name, value)),
            _ => {
                if lines.last().map(|l| !l.trim().is_empty()).unwrap_or(false) {
                    lines.push(String::new());
                }
                lines.push("[dependencies]".into());
                lines.push(format!("{} = {}", name, value));
            }
        }
    }
    let mut s = lines.join("\n");
    s.push('\n');
    std::fs::write(&p, s).map_err(toml_write_err)
}

fn toml_read_err(e: std::io::Error) -> String {
    tr!(format!("siskin.toml 을 읽을 수 없습니다 ({})", e), format!("cannot read siskin.toml ({})", e))
}

fn toml_write_err(e: std::io::Error) -> String {
    tr!(format!("siskin.toml 을 쓸 수 없습니다 ({})", e), format!("cannot write siskin.toml ({})", e))
}

fn remove_dep_line(root: &Path, name: &str) -> Result<bool, String> {
    let p = root.join("siskin.toml");
    let text = std::fs::read_to_string(&p).map_err(toml_read_err)?;
    let mut in_deps = false;
    let mut found = false;
    let mut out: Vec<&str> = Vec::new();
    for l in text.lines() {
        let t = strip_comment(l).trim().to_string();
        if t.starts_with('[') {
            in_deps = t == "[dependencies]";
        } else if in_deps && t.split('=').next().map(|k| k.trim().trim_matches('"')) == Some(name) {
            found = true;
            continue;
        }
        out.push(l);
    }
    let mut s = out.join("\n");
    s.push('\n');
    std::fs::write(&p, s).map_err(toml_write_err)?;
    Ok(found)
}

fn root_or_complain() -> Option<PathBuf> {
    let r = find_root(Path::new("."));
    if r.is_none() {
        eprintln!(
            "{}",
            tr!(
                "siskin.toml 이 없습니다. 프로젝트 폴더 안에서 실행하거나, `siskin new 이름` 으로 새로 만드세요",
                "no siskin.toml found; run this inside a project folder, or create one with `siskin new <name>`"
            )
        );
    }
    r
}

// ---------------------------------------------------------------- package index (registry)

/// Default package index. If it does not actually exist yet, point elsewhere with `SISKIN_REGISTRY`.
pub const DEFAULT_REGISTRY: &str = "https://github.com/Haru-neo/siskin-registry";

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    git: String,
    description: String,
}

/// Location of the index to use: SISKIN_REGISTRY > [registry] url in siskin.toml > default.
fn registry_url(root: Option<&Path>) -> String {
    if let Ok(v) = std::env::var("SISKIN_REGISTRY") {
        if !v.trim().is_empty() {
            return v.trim().to_string();
        }
    }
    if let Some(r) = root {
        if let Ok(m) = read_manifest(r) {
            if let Some(u) = m.registry {
                // Relative folders are resolved against the folder containing siskin.toml.
                if !looks_git(&u) && Path::new(&u).is_relative() {
                    let p = r.join(&u);
                    return crate::canonicalize(&p).unwrap_or(p).to_string_lossy().to_string();
                }
                return u;
            }
        }
    }
    DEFAULT_REGISTRY.to_string()
}

fn looks_git(s: &str) -> bool {
    s.contains("://") || s.starts_with("git@") || s.ends_with(".git")
}

fn siskin_home() -> PathBuf {
    if let Ok(h) = std::env::var("SISKIN_HOME") {
        if !h.is_empty() {
            return PathBuf::from(h);
        }
    }
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".siskin")
}

/// Prepare the index locally. A folder is used as is; a git index is cloned or refreshed.
/// Without internet access, the previously fetched copy is used.
fn registry_dir(url: &str) -> Result<PathBuf, String> {
    if !looks_git(url) {
        let d = PathBuf::from(url);
        if d.is_dir() {
            return Ok(d);
        }
        return Err(tr!(
            format!("패키지 목록 폴더 `{}` 가 없습니다", url),
            format!("package registry folder `{}` does not exist", url)
        ));
    }
    let key: String = url
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
        .collect();
    let dir = siskin_home().join("registry").join(key);
    if dir.join(".git").exists() {
        let fetched = git(&["fetch", "--quiet", "origin"], Some(&dir)).and_then(|_| {
            let head = git(&["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"], Some(&dir))
                .unwrap_or_else(|_| "origin/HEAD".to_string());
            git(&["reset", "--quiet", "--hard", &head], Some(&dir))
        });
        if fetched.is_err() {
            eprintln!(
                "{}",
                tr!(
                    "(패키지 목록을 새로 받지 못해 전에 받아 둔 것을 씁니다. 인터넷 연결을 확인하세요)",
                    "(could not refresh the package registry; using the saved copy. Check your internet connection)"
                )
            );
        }
        return Ok(dir);
    }
    let _ = std::fs::create_dir_all(dir.parent().unwrap_or(&dir));
    println!("{}", tr!(format!("패키지 목록을 받는 중... ({})", url), format!("fetching package registry... ({})", url)));
    git(&["clone", "--quiet", "--depth", "1", url, &dir.to_string_lossy()], None).map_err(|e| {
        let _ = std::fs::remove_dir_all(&dir);
        tr!(
            format!("패키지 목록 `{}` 을(를) 받을 수 없습니다 (인터넷이 안 되거나, 그 저장소가 없습니다)\n  git: {}\n  다른 목록을 쓰려면 SISKIN_REGISTRY=<git 주소나 폴더> 를 정하세요", url, e),
            format!("cannot fetch package registry `{}` (you may be offline, or the repository does not exist)\n  git: {}\n  to use another registry, set SISKIN_REGISTRY=<git URL or folder>", url, e)
        )
    })?;
    Ok(dir)
}

/// One package in the index (`packages/NAME.toml`).
fn registry_entry(dir: &Path, name: &str) -> Result<Option<Entry>, String> {
    let p = dir.join("packages").join(format!("{}.toml", name));
    if !p.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
    let t = parse_toml(&text).map_err(|e| format!("{}: {}", p.display(), e))?;
    let top = t.get("").cloned().unwrap_or_default();
    let get = |k: &str| top.get(k).and_then(|v| v.get("")).cloned().unwrap_or_default();
    let git = get("git");
    if git.is_empty() {
        return Err(tr!(
            format!("패키지 목록의 `{}` 에 git 주소가 없습니다", p.display()),
            format!("`{}` in the package registry has no git URL", p.display())
        ));
    }
    Ok(Some(Entry { name: name.to_string(), git, description: get("description") }))
}

fn registry_all(dir: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("packages")) {
        let mut names: Vec<String> = rd
            .flatten()
            .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".toml")).map(|s| s.to_string()))
            .collect();
        names.sort();
        for n in names {
            if let Ok(Some(e)) = registry_entry(dir, &n) {
                out.push(e);
            }
        }
    }
    out
}

/// Similar names (off by one or two characters).
fn near_names(entries: &[Entry], name: &str) -> Vec<String> {
    let lower = name.to_lowercase();
    entries
        .iter()
        .filter(|e| {
            let n = e.name.to_lowercase();
            n.contains(&lower) || lower.contains(&n) || edit_distance(&n, &lower) <= 2
        })
        .map(|e| e.name.clone())
        .take(5)
        .collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let c = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + c);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Look up the source for `siskin add NAME`.
fn lookup(root: &Path, name: &str) -> Result<String, String> {
    let url = registry_url(Some(root));
    let dir = registry_dir(&url)?;
    match registry_entry(&dir, name)? {
        Some(e) => {
            println!("{}", tr!(format!("패키지 목록에서 찾음: {} → {}", name, e.git), format!("found in registry: {} → {}", name, e.git)));
            Ok(e.git)
        }
        None => {
            let near = near_names(&registry_all(&dir), name);
            let hint = if near.is_empty() {
                tr!(
                    "`siskin search 단어` 로 찾아보거나, 주소를 직접 주세요: siskin add 이름 <git 주소 또는 폴더>".to_string(),
                    "search with `siskin search <word>`, or give the address yourself: siskin add <name> <git URL or folder>".to_string()
                )
            } else {
                tr!(format!("혹시 이것인가요? {}", near.join(", ")), format!("did you mean: {}?", near.join(", ")))
            };
            Err(tr!(
                format!("패키지 목록({})에 `{}` 이(가) 없습니다\n  {}", url, name, hint),
                format!("no package named `{}` in the registry ({})\n  {}", name, url, hint)
            ))
        }
    }
}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
        && n.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !crate::lexer::is_keyword(n)
        && !crate::STDLIB_MODULES.contains(&n)
}

pub fn cli(cmd: &str, args: &[String]) -> ExitCode {
    let pos: Vec<&String> = args.iter().skip(1).filter(|a| !a.starts_with("--")).collect();
    let flag = |f: &str| -> Option<String> {
        args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned()
    };
    let fail = |msg: String| {
        eprintln!("{}", msg);
        ExitCode::FAILURE
    };
    match cmd {
        "new" => {
            let name = match pos.first() {
                Some(n) => n.as_str(),
                None => {
                    return fail(
                        tr!("만들 프로젝트 이름을 적어 주세요. 예: siskin new 할일", "give a name for the new project, e.g. siskin new todo")
                            .into(),
                    )
                }
            };
            let dir = Path::new(name);
            if dir.exists() {
                return fail(tr!(format!("`{}` 이(가) 이미 있습니다", name), format!("`{}` already exists", name)));
            }
            let pkg = dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(name.to_string());
            let r = (|| -> std::io::Result<()> {
                std::fs::create_dir_all(dir)?;
                std::fs::write(
                    dir.join("siskin.toml"),
                    format!("[package]\nname = \"{}\"\nversion = \"0.1.0\"\n\n[dependencies]\n", pkg),
                )?;
                std::fs::write(
                    dir.join("main.skn"),
                    tr!("fn main():\n    print(\"안녕, Siskin!\\n\")\n", "fn main():\n    print(\"Hello, Siskin!\\n\")\n"),
                )?;
                std::fs::write(dir.join(".gitignore"), ".siskin/\n")?;
                Ok(())
            })();
            if let Err(e) = r {
                return fail(tr!(format!("만들 수 없습니다 ({})", e), format!("cannot create it ({})", e)));
            }
            if crate::lang::ko() {
                println!("`{}` 를 만들었습니다.\n  cd {}\n  siskin run main.skn", name, name);
            } else {
                println!("created `{}`\n  cd {}\n  siskin run main.skn", name, name);
            }
            ExitCode::SUCCESS
        }
        "add" => {
            let root = match root_or_complain() {
                Some(r) => r,
                None => return ExitCode::FAILURE,
            };
            let (name, from) = match (pos.first(), pos.get(1)) {
                (Some(n), Some(f)) => (n.to_string(), Some(f.to_string())),
                (Some(n), None) => (n.to_string(), None),
                _ => {
                    return fail(
                        tr!(
                            "이렇게 씁니다: siskin add 이름 [git 주소 또는 폴더] [--rev 태그]\n  예: siskin add colors                (패키지 목록에서 찾기)\n      siskin add colors https://github.com/someone/siskin-colors --rev v1.0\n      siskin add util ../util",
                            "usage: siskin add <name> [git URL or folder] [--rev tag]\n  e.g. siskin add colors                (look it up in the package registry)\n       siskin add colors https://github.com/someone/siskin-colors --rev v1.0\n       siskin add util ../util"
                        )
                        .into(),
                    )
                }
            };
            if !valid_name(&name) {
                return fail(tr!(
                    format!("`{}` 는 패키지 이름으로 쓸 수 없습니다 (글자·숫자·_ 만, 키워드와 std 모듈 이름은 안 됩니다)", name),
                    format!(
                        "`{}` cannot be used as a package name (letters, digits and _ only; no keywords or std module names)",
                        name
                    )
                ));
            }
            // Without a source, look it up in the package index.
            let from = match from {
                Some(f) => f,
                None => match lookup(&root, &name) {
                    Ok(u) => u,
                    Err(e) => return fail(e),
                },
            };
            let looks_git = looks_git(&from);
            let value = if !looks_git && root.join(&from).is_dir() || (!looks_git && Path::new(&from).is_dir()) {
                let abs = crate::canonicalize(&from).unwrap_or_else(|_| PathBuf::from(&from));
                let rel = pathdiff(&abs, &root).unwrap_or_else(|| abs.to_string_lossy().to_string());
                format!("{{ path = \"{}\" }}", rel)
            } else {
                match flag("--rev") {
                    Some(r) => format!("{{ git = \"{}\", rev = \"{}\" }}", from, r),
                    None => format!("{{ git = \"{}\" }}", from),
                }
            };
            let before = std::fs::read_to_string(root.join("siskin.toml")).unwrap_or_default();
            let had_dir = root.join(".siskin").join("deps").join(&name).exists();
            if let Err(e) = add_dep_line(&root, &name, &value) {
                return fail(e);
            }
            match install_all(&root, false, Some(&name)) {
                Ok(_) => {
                    if crate::lang::ko() {
                        println!("siskin.toml 에 `{}` 를 넣었습니다. 이제 `import {}` 로 씁니다", name, name);
                    } else {
                        println!("added `{}` to siskin.toml; use it with `import {}`", name, name);
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    // On failure, restore siskin.toml to its original state.
                    let _ = std::fs::write(root.join("siskin.toml"), before);
                    if !had_dir {
                        let _ = std::fs::remove_dir_all(root.join(".siskin").join("deps").join(&name));
                    }
                    fail(tr!(format!("{}\n(siskin.toml 은 그대로 두었습니다)", e), format!("{}\n(siskin.toml left unchanged)", e)))
                }
            }
        }
        "install" | "update" => {
            let root = match root_or_complain() {
                Some(r) => r,
                None => return ExitCode::FAILURE,
            };
            match install_all(&root, cmd == "update", None) {
                Ok(0) => {
                    println!("{}", tr!("siskin.toml 에 적힌 패키지가 없습니다", "no packages listed in siskin.toml"));
                    ExitCode::SUCCESS
                }
                Ok(n) => {
                    if crate::lang::ko() {
                        println!("패키지 {}개 준비됨", n);
                    } else {
                        println!("{} package{} ready", n, if n == 1 { "" } else { "s" });
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => fail(e),
            }
        }
        "remove" => {
            let root = match root_or_complain() {
                Some(r) => r,
                None => return ExitCode::FAILURE,
            };
            let name = match pos.first() {
                Some(n) => n.to_string(),
                None => {
                    return fail(
                        tr!("뺄 패키지 이름을 적어 주세요. 예: siskin remove colors", "give the name of the package to remove, e.g. siskin remove colors")
                            .into(),
                    )
                }
            };
            match remove_dep_line(&root, &name) {
                Ok(false) => {
                    return fail(tr!(format!("siskin.toml 에 `{}` 가 없습니다", name), format!("`{}` is not in siskin.toml", name)))
                }
                Err(e) => return fail(e),
                Ok(true) => {}
            }
            let _ = std::fs::remove_dir_all(root.join(".siskin").join("deps").join(&name));
            let mut lock = read_lock(&root);
            lock.remove(&name);
            if let Err(e) = write_lock(&root, &lock) {
                return fail(e);
            }
            println!("{}", tr!(format!("`{}` 를 뺐습니다", name), format!("removed `{}`", name)));
            ExitCode::SUCCESS
        }
        "search" => {
            let root = find_root(Path::new("."));
            let url = registry_url(root.as_deref());
            let dir = match registry_dir(&url) {
                Ok(d) => d,
                Err(e) => return fail(e),
            };
            let all = registry_all(&dir);
            let words: Vec<String> = pos.iter().map(|w| w.to_lowercase()).collect();
            let hits: Vec<&Entry> = all
                .iter()
                .filter(|e| {
                    let hay = format!("{} {}", e.name, e.description).to_lowercase();
                    words.iter().all(|w| hay.contains(w.as_str()))
                })
                .collect();
            if hits.is_empty() {
                if all.is_empty() {
                    println!("{}", tr!(format!("패키지 목록({})이 비어 있습니다", url), format!("the package registry ({}) is empty", url)));
                } else {
                    println!("{}", tr!(format!("`{}` 에 맞는 패키지가 없습니다", words.join(" ")), format!("no packages match `{}`", words.join(" "))));
                }
                return ExitCode::SUCCESS;
            }
            let w = hits.iter().map(|e| e.name.chars().count()).max().unwrap_or(0);
            for e in &hits {
                let pad = " ".repeat(w - e.name.chars().count());
                if e.description.is_empty() {
                    println!("{}{}  {}", e.name, pad, e.git);
                } else {
                    println!("{}{}  {}", e.name, pad, e.description);
                }
            }
            println!("{}", tr!(format!("\n{}개. 쓰려면: siskin add 이름", hits.len()), format!("\n{} found. To use one: siskin add <name>", hits.len())));
            ExitCode::SUCCESS
        }
        "publish" => {
            let root = match root_or_complain() {
                Some(r) => r,
                None => return ExitCode::FAILURE,
            };
            let m = match read_manifest(&root) {
                Ok(m) => m,
                Err(e) => return fail(e),
            };
            let name = m.name.clone();
            if !valid_name(&name) {
                return fail(tr!(
                    format!("siskin.toml 의 [package] name `{}` 은(는) 패키지 이름으로 쓸 수 없습니다 (글자·숫자·_ 만)", name),
                    format!("[package] name `{}` in siskin.toml cannot be used as a package name (letters, digits and _ only)", name)
                ));
            }
            if !root.join("lib.skn").is_file() {
                return fail(tr!(
                    "lib.skn 가 없습니다. 남이 `import 이름` 으로 쓰려면 프로젝트 뿌리에 lib.skn 가 있어야 합니다",
                    "there is no lib.skn; others use your package with `import <name>`, which reads lib.skn at the project root"
                ).into());
            }
            let git_url = match git(&["remote", "get-url", "origin"], Some(&root)) {
                Ok(u) if !u.is_empty() => u,
                _ => {
                    return fail(tr!(
                        "이 프로젝트에 git 주소(origin)가 없습니다. 먼저 GitHub 같은 곳에 올려 주세요:\n  git init && git add . && git commit -m 첫판\n  git remote add origin <주소> && git push -u origin HEAD",
                        "this project has no git remote (origin). Push it somewhere like GitHub first:\n  git init && git add . && git commit -m first\n  git remote add origin <url> && git push -u origin HEAD"
                    ).into())
                }
            };
            if m.description.is_empty() {
                eprintln!("{}", tr!(
                    "(siskin.toml 의 [package] 에 description = \"한 줄 설명\" 을 넣으면 `siskin search` 에 보입니다)",
                    "(add description = \"one line\" under [package] in siskin.toml so it shows up in `siskin search`)"
                ));
            }
            let url = registry_url(Some(&root));
            let dir = match registry_dir(&url) {
                Ok(d) => d,
                Err(e) => return fail(e),
            };
            match registry_entry(&dir, &name) {
                Ok(Some(e)) if e.git != git_url => {
                    return fail(tr!(
                        format!("`{}` 라는 이름은 이미 다른 패키지({})가 쓰고 있습니다. siskin.toml 의 name 을 바꿔 주세요", name, e.git),
                        format!("the name `{}` is already taken by another package ({}); change name in siskin.toml", name, e.git)
                    ))
                }
                Ok(Some(_)) => {
                    println!("{}", tr!(format!("`{}` 은(는) 이미 목록에 있습니다. 새 판은 git 태그만 올리면 됩니다", name), format!("`{}` is already in the registry; for new versions just push a git tag", name)));
                    return ExitCode::SUCCESS;
                }
                Ok(None) => {}
                Err(e) => return fail(e),
            }
            let desc = m.description.replace('\\', "\\\\").replace('"', "\\\"");
            let body = format!("git = \"{}\"\ndescription = \"{}\"\n", git_url, desc);
            let file = format!("packages/{}.toml", name);
            if !looks_git(&url) {
                // If the index is a local folder, write directly to it.
                let p = dir.join(&file);
                let _ = std::fs::create_dir_all(dir.join("packages"));
                if let Err(e) = std::fs::write(&p, &body) {
                    return fail(format!("{}: {}", p.display(), e));
                }
                println!("{}", tr!(format!("목록 폴더에 `{}` 을(를) 적었습니다. 이제 `siskin add {}` 로 받을 수 있습니다", p.display(), name), format!("wrote `{}` to the registry folder; `siskin add {}` now works", p.display(), name)));
                return ExitCode::SUCCESS;
            }
            if crate::lang::ko() {
                println!("목록에 올리려면 {} 저장소에 아래 파일 하나를 더하는 PR 을 보내세요.\n\n--- {} ---\n{}---\n\n합쳐지면 누구나 `siskin add {}` 로 받습니다.", url, file, body, name);
            } else {
                println!("To publish, open a pull request on {} that adds this one file:\n\n--- {} ---\n{}---\n\nOnce merged, anyone can run `siskin add {}`.", url, file, body, name);
            }
            ExitCode::SUCCESS
        }
        _ => ExitCode::FAILURE,
    }
}

/// `to` as a path relative to `base`.
fn pathdiff(to: &Path, base: &Path) -> Option<String> {
    let base = crate::canonicalize(base).ok()?;
    let a: Vec<_> = to.components().collect();
    let b: Vec<_> = base.components().collect();
    let mut i = 0;
    while i < a.len() && i < b.len() && a[i] == b[i] {
        i += 1;
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in i..b.len() {
        parts.push("..".into());
    }
    for c in &a[i..] {
        parts.push(c.as_os_str().to_string_lossy().to_string());
    }
    Some(if parts.is_empty() { ".".into() } else { parts.join("/") })
}
