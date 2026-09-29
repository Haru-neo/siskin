//! The part of the interpreter that talks to the operating system: dates, processes, files and folders.
//!
//! Its native counterpart is `rt_sys.c`. Both call the same C functions (localtime_r, mktime …)
//! and produce error text identical down to the character, because `siskin run` and `siskin build`
//! must give the same answers.

use std::os::raw::c_int;
#[cfg(unix)]
use std::os::raw::{c_char, c_long};

#[cfg(unix)]
#[repr(C)]
struct Tm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
    tm_gmtoff: c_long,
    tm_zone: *const c_char,
}

#[cfg(unix)]
extern "C" {
    fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    fn gmtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    fn mktime(t: *mut Tm) -> i64;
    fn timegm(t: *mut Tm) -> i64;
}

#[cfg(unix)]
fn empty_tm() -> Tm {
    Tm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 0,
        tm_mon: 0,
        tm_year: 0,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: std::ptr::null(),
    }
}

// The Windows C runtime has no tm_gmtoff in `struct tm`, and function names differ.
// Call the same functions as the native side (`rt_sys.c`) to get the same answers.
#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct Tm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
}

#[cfg(windows)]
extern "C" {
    fn _localtime64_s(out: *mut Tm, t: *const i64) -> c_int;
    fn _gmtime64_s(out: *mut Tm, t: *const i64) -> c_int;
    fn _mktime64(t: *mut Tm) -> i64;
    fn _mkgmtime64(t: *mut Tm) -> i64;
}

#[cfg(windows)]
fn empty_tm() -> Tm {
    Tm { tm_sec: 0, tm_min: 0, tm_hour: 0, tm_mday: 0, tm_mon: 0, tm_year: 0, tm_wday: 0, tm_yday: 0, tm_isdst: 0 }
}

#[cfg(unix)]
unsafe fn split_time(tt: i64, utc: bool, r: &mut Tm) -> i64 {
    if utc {
        gmtime_r(&tt, r);
        0
    } else {
        localtime_r(&tt, r);
        r.tm_gmtoff as i64
    }
}

#[cfg(windows)]
unsafe fn split_time(tt: i64, utc: bool, r: &mut Tm) -> i64 {
    if utc {
        _gmtime64_s(r, &tt);
        0
    } else {
        _localtime64_s(r, &tt);
        let mut c = *r;
        _mkgmtime64(&mut c) - tt
    }
}

#[cfg(unix)]
unsafe fn join_time(r: &mut Tm, utc: bool) -> i64 {
    if utc {
        timegm(r)
    } else {
        mktime(r)
    }
}

#[cfg(windows)]
unsafe fn join_time(r: &mut Tm, utc: bool) -> i64 {
    if utc {
        _mkgmtime64(r)
    } else {
        _mktime64(r)
    }
}

/// [year, month, day, hour, minute, second, weekday (1=Mon..7=Sun), offset from UTC (seconds)]
pub fn time_parts(t: f64, utc: bool) -> Vec<i64> {
    let tt = t.floor() as i64;
    let mut r = empty_tm();
    // Safety: the C runtime fills a correctly sized tm slot.
    let off = unsafe { split_time(tt, utc, &mut r) };
    vec![
        r.tm_year as i64 + 1900,
        r.tm_mon as i64 + 1,
        r.tm_mday as i64,
        r.tm_hour as i64,
        r.tm_min as i64,
        r.tm_sec as i64,
        if r.tm_wday == 0 { 7 } else { r.tm_wday as i64 },
        off,
    ]
}

pub fn time_make(p: &[i64], utc: bool) -> f64 {
    let g = |i: usize| p.get(i).copied().unwrap_or(0) as c_int;
    let mut r = empty_tm();
    r.tm_year = g(0) - 1900;
    r.tm_mon = g(1) - 1;
    r.tm_mday = g(2);
    r.tm_hour = g(3);
    r.tm_min = g(4);
    r.tm_sec = g(5);
    r.tm_isdst = -1;
    // Safety: the C runtime reads and normalizes the filled tm slot.
    let v = unsafe { join_time(&mut r, utc) };
    v as f64
}

pub fn sleep(sec: f64) {
    if sec > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f64(sec));
    }
}

/// errno of an error. Unix uses it directly; on Windows OS error numbers differ from errno,
/// so the C runtime's errno is derived from the error kind (Windows ENOTEMPTY is 41).
#[cfg(unix)]
fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(0)
}

#[cfg(windows)]
fn errno_of(e: &std::io::Error) -> i32 {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::NotFound => 2,
        K::PermissionDenied => 13,
        K::AlreadyExists => 17,
        K::NotADirectory => 20,
        K::IsADirectory => 21,
        K::DirectoryNotEmpty => 41,
        _ => e.raw_os_error().unwrap_or(0),
    }
}

/// errno as a human-readable reason. Identical, character for character, to mi_errmsg in `rt_sys`.
pub fn errmsg(path: &str, e: &std::io::Error) -> String {
    let code = errno_of(e);
    let why = match code {
        2 => tr!("없는 경로입니다", "no such file or directory").to_string(),
        13 | 1 => tr!("권한이 없습니다", "permission denied").to_string(),
        17 => tr!("이미 있습니다", "already exists").to_string(),
        20 => tr!("폴더가 아닙니다", "not a directory").to_string(),
        21 => tr!("폴더입니다", "is a directory").to_string(),
        39 | 41 => tr!("폴더가 비어 있지 않습니다", "directory not empty").to_string(),
        n => tr!(format!("할 수 없습니다 (errno {})", n), format!("operation failed (errno {})", n)),
    };
    format!("{}: {}", path, why)
}

/// One shell command line. `sh -c` on Unix, `cmd /C` on Windows (same as native `rt_sys.c`).
#[cfg(unix)]
fn shell_command(prog: &str) -> std::process::Command {
    let mut c = std::process::Command::new("sh");
    c.arg("-c").arg(prog);
    c
}

#[cfg(windows)]
fn shell_command(prog: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new("cmd");
    c.raw_arg("/C").raw_arg(prog);
    c
}

/// Exit code of a program killed by a signal (128 + signal number). Windows has no signals.
#[cfg(unix)]
fn signal_code(st: &std::process::ExitStatus) -> i64 {
    use std::os::unix::process::ExitStatusExt;
    st.signal().map(|s| 128 + s as i64).unwrap_or(-1)
}

#[cfg(not(unix))]
fn signal_code(_: &std::process::ExitStatus) -> i64 {
    -1
}

/// Run a program and return (exit code, stdout, stderr).
pub fn run(prog: &str, args: &[String], shell: bool) -> (i64, String, String) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let _ = std::io::stdout().flush();
    let mut cmd = if shell {
        shell_command(prog)
    } else {
        let mut c = Command::new(prog);
        c.args(args);
        c
    };
    cmd.stdin(Stdio::null());
    match cmd.output() {
        Ok(o) => {
            let code = match o.status.code() {
                Some(c) => c as i64,
                None => signal_code(&o.status),
            };
            (
                code,
                String::from_utf8_lossy(&o.stdout).to_string(),
                String::from_utf8_lossy(&o.stderr).to_string(),
            )
        }
        Err(_) => (127, String::new(), tr!(format!("{}: 실행할 수 없습니다\n", prog), format!("{}: cannot execute\n", prog))),
    }
}

pub fn list_dir(p: &str) -> Result<Vec<String>, String> {
    let rd = std::fs::read_dir(p).map_err(|e| errmsg(p, &e))?;
    let mut out: Vec<String> = Vec::new();
    for e in rd.flatten() {
        out.push(e.file_name().to_string_lossy().to_string());
    }
    // Match the native side's order (byte order).
    out.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    Ok(out)
}

pub fn make_dir(p: &str) -> Result<(), String> {
    match std::fs::create_dir_all(p) {
        Ok(()) => Ok(()),
        Err(e) => Err(errmsg(p, &e)),
    }
}

pub fn is_dir(p: &str) -> bool {
    std::path::Path::new(p).is_dir()
}
