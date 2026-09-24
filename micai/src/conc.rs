//! 동시성(`spawn`, `channel`)의 인터프리터 쪽 바탕.
//!
//! 작업 하나가 운영체제 스레드 하나이고, 작업들은 **진짜로 동시에** 돕니다(CPU 여러 개).
//! 인터프리터의 값은 `Rc` 라서 여러 스레드가 같이 만지면 안 되지만, Siskin 에서는 작업끼리
//! 메모리를 나누지 않습니다. 작업에 넘기는 값, 작업의 결과, 통로로 보내는 값은 모두
//! 보내는 쪽이 통째로 새로 만들어(`Value::detach`) 넘기므로, 어느 `Rc` 든 언제나 한
//! 스레드만 만집니다. 함수·구조체 선언(문법 나무)은 `Arc` 라서 같이 읽어도 됩니다.
//!
//! 통로·작업 상태는 자물쇠 하나(`SYNC`)를 쥔 채로만 바꾸고 기다립니다. 살아 있는 작업이
//! 모두 기다리고 있으면(교착) 아무도 깨워 줄 수 없으므로 실행 오류로 멈춥니다.
//! 네이티브 런타임(`rt_conc.c`)도 같은 규칙입니다.

use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};

pub struct Sched {
    /// 아직 끝나지 않은 작업 수(main 포함)
    pub live: usize,
    /// 그중 무언가를 기다리고 있는 수
    blocked: usize,
    /// 모두 깨울 때마다 늘어납니다. 깨운 순간 `blocked` 는 0 으로 돌립니다
    /// (깨어난 쪽이 아직 자물쇠를 못 잡았는데 "기다리는 중"으로 세면 교착으로 잘못 봅니다).
    epoch: u64,
}

static SYNC: Mutex<Sched> = Mutex::new(Sched { live: 1, blocked: 0, epoch: 0 });
static CV: Condvar = Condvar::new();
/// 작업 안에서 난 오류를 보여 줄 때 쓸 main 파일 (경로, 글).
static MAIN_SRC: OnceLock<(String, String)> = OnceLock::new();

/// 작업 스레드의 스택 크기. 인터프리터는 재귀가 깊어서 넉넉히 줍니다(실제로 쓰는 만큼만 잡힘).
const STACK: usize = 256 * 1024 * 1024;

fn lock() -> MutexGuard<'static, Sched> {
    SYNC.lock().unwrap_or_else(|e| e.into_inner())
}

fn wake(g: &mut Sched) {
    g.blocked = 0;
    g.epoch += 1;
    CV.notify_all();
}

/// 예전(한 번에 한 작업) 방식의 자리. 지금은 할 일이 없습니다.
pub fn enter() {}

pub fn set_source(path: &str, src: &str) {
    let _ = MAIN_SRC.set((path.to_string(), src.to_string()));
}

pub fn source() -> Option<&'static (String, String)> {
    MAIN_SRC.get()
}

/// 통로·작업 상태를 바꿉니다. 자물쇠를 쥔 채 `f` 를 하고, 기다리던 쪽을 모두 깨웁니다.
pub fn change<T>(f: impl FnOnce() -> T) -> T {
    let mut g = lock();
    let r = f();
    wake(&mut g);
    r
}

/// `ready()` 가 참이 될 때까지 기다린 뒤, 자물쇠를 쥔 그대로 `act()` 를 합니다
/// (확인과 행동 사이에 다른 작업이 끼어들지 않게). 모두가 기다리는 중이면(교착) Err 입니다.
pub fn wait_then<T>(mut ready: impl FnMut(&Sched) -> bool, act: impl FnOnce() -> T) -> Result<T, ()> {
    let mut g = lock();
    loop {
        if ready(&g) {
            let r = act();
            wake(&mut g);
            return Ok(r);
        }
        let epoch = g.epoch;
        g.blocked += 1;
        if g.blocked >= g.live {
            g.blocked -= 1;
            return Err(());
        }
        g = CV.wait(g).unwrap_or_else(|e| e.into_inner());
        if g.epoch == epoch {
            g.blocked -= 1;
        }
    }
}

pub fn wait_until(ready: impl FnMut(&Sched) -> bool) -> Result<(), ()> {
    wait_then(ready, || ())
}

/// 예전에는 기다리는 동안(sleep, input) 큰 자물쇠를 내려놓았습니다. 지금은 그냥 돌립니다.
pub fn without_gil<T>(f: impl FnOnce() -> T) -> T {
    f()
}

/// 남은 작업이 모두 끝날 때까지 기다립니다(main 이 끝날 때).
pub fn wait_all() -> Result<(), ()> {
    wait_until(|s| s.live <= 1)
}

/// 프로그램을 끝내기 전에 부릅니다(표준 출력을 비웁니다).
pub fn shutdown() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

struct SendBox(Box<dyn FnOnce()>);
// 안전: 상자 속 일(job)이 붙잡은 인터프리터 값은 모두 `detach` 로 새로 만든 것이라
// 만든 스레드는 더 이상 만지지 않고, 새 스레드만 씁니다(`Interp::spawn_task`).
unsafe impl Send for SendBox {}

/// 새 작업을 시작합니다. `job` 은 새 스레드에서 돌고, 끝나기 전에 자기가 쥔 값을 모두 내려놓습니다.
pub fn start(job: Box<dyn FnOnce()>) {
    lock().live += 1;
    let b = SendBox(job);
    let r = std::thread::Builder::new().stack_size(STACK).spawn(move || {
        let b = b;
        (b.0)();
        finish();
    });
    if r.is_err() {
        lock().live -= 1;
        report_plain(
            "E0263",
            crate::tr!("새 작업(스레드)을 시작하지 못했습니다", "could not start a new task (thread)").to_string(),
        );
    }
}

/// 작업 하나가 끝났습니다. 기다리던 쪽이 깨어나 다시 확인합니다.
fn finish() {
    let mut g = lock();
    g.live -= 1;
    wake(&mut g);
}

pub fn deadlock_msg() -> String {
    crate::tr!(
        "교착 상태: 모든 작업이 서로를 기다리고 있습니다(통로 받기·보내기 또는 작업 기다리기)",
        "deadlock: every task is waiting (on a channel or on another task)"
    )
    .to_string()
}

pub fn deadlock_help() -> &'static str {
    crate::tr!(
        "보내는 쪽이 다 보낸 뒤 `ch.close()` 를 부르는지, 기다리는 작업이 실제로 끝나는지 보세요",
        "make sure the sender calls `ch.close()` when it is done, and that the task being waited on can finish"
    )
}

/// 오류를 알리는 동안 다른 작업이 글을 끼워 넣지 않게 합니다.
static REPORT: Mutex<()> = Mutex::new(());

/// 위치 없는 실행 오류를 알리고 프로그램을 끝냅니다.
pub fn report_plain(code: &str, msg: String) -> ! {
    let _g = REPORT.lock().unwrap_or_else(|e| e.into_inner());
    shutdown();
    let kind = crate::tr!("오류", "error");
    eprintln!("{}[{}]: {}", kind, code, msg);
    if code == "E0260" {
        eprintln!("  help: {}", deadlock_help());
    }
    std::process::exit(1);
}

/// 작업 안에서 난 실행 오류를 main 과 같은 모양으로 알리고 끝냅니다.
pub fn report_error(e: &crate::error::SiskinError) -> ! {
    let _g = REPORT.lock().unwrap_or_else(|e| e.into_inner());
    shutdown();
    match source() {
        Some((p, s)) => eprint!("{}", e.render(p, s)),
        None => eprintln!("error[{}]: {}", e.code, e.msg),
    }
    std::process::exit(1);
}
