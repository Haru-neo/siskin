//! 인터프리터가 운영체제와 이야기하는 부분: 날짜, 프로세스, 파일 폴더.
//!
//! 네이티브 쪽 짝은 `rt_sys.c` 입니다. 두 쪽이 같은 C 함수(localtime_r, mktime …)를
//! 부르고, 오류 문구도 글자까지 같게 만듭니다. `siskin run` 과 `siskin build` 가
//! 같은 답을 내야 하기 때문입니다.

use std::os::raw::{c_char, c_int, c_long};

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

extern "C" {
    fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    fn gmtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    fn mktime(t: *mut Tm) -> i64;
    fn timegm(t: *mut Tm) -> i64;
}

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

/// [년, 월, 일, 시, 분, 초, 요일(1=월..7=일), UTC와의 차이(초)]
pub fn time_parts(t: f64, utc: bool) -> Vec<i64> {
    let tt = t.floor() as i64;
    let mut r = empty_tm();
    unsafe {
        if utc {
            gmtime_r(&tt, &mut r);
        } else {
            localtime_r(&tt, &mut r);
        }
    }
    vec![
        r.tm_year as i64 + 1900,
        r.tm_mon as i64 + 1,
        r.tm_mday as i64,
        r.tm_hour as i64,
        r.tm_min as i64,
        r.tm_sec as i64,
        if r.tm_wday == 0 { 7 } else { r.tm_wday as i64 },
        if utc { 0 } else { r.tm_gmtoff as i64 },
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
    let v = unsafe {
        if utc {
            timegm(&mut r)
        } else {
            mktime(&mut r)
        }
    };
    v as f64
}

pub fn sleep(sec: f64) {
    if sec > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f64(sec));
    }
}

/// errno 를 사람이 읽는 까닭으로. `rt_sys` 의 mi_errmsg 와 글자까지 같습니다.
pub fn errmsg(path: &str, e: &std::io::Error) -> String {
    let code = e.raw_os_error().unwrap_or(0);
    let why = match code {
        2 => tr!("없는 경로입니다", "no such file or directory").to_string(),
        13 | 1 => tr!("권한이 없습니다", "permission denied").to_string(),
        17 => tr!("이미 있습니다", "already exists").to_string(),
        20 => tr!("폴더가 아닙니다", "not a directory").to_string(),
        21 => tr!("폴더입니다", "is a directory").to_string(),
        39 => tr!("폴더가 비어 있지 않습니다", "directory not empty").to_string(),
        n => tr!(format!("할 수 없습니다 (errno {})", n), format!("operation failed (errno {})", n)),
    };
    format!("{}: {}", path, why)
}

/// 프로그램을 실행하고 (끝난 코드, 표준 출력, 표준 오류)를 돌려줍니다.
pub fn run(prog: &str, args: &[String], shell: bool) -> (i64, String, String) {
    use std::io::Write;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};
    let _ = std::io::stdout().flush();
    let mut cmd = if shell {
        let mut c = Command::new("sh");
        c.arg("-c").arg(prog);
        c
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
                None => o.status.signal().map(|s| 128 + s as i64).unwrap_or(-1),
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
    // 네이티브와 같은 순서(바이트 순)로 맞춥니다.
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
