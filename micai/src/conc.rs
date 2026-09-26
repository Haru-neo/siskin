//! Interpreter-side foundation for concurrency (`spawn`, `channel`).
//!
//! Each task is one OS thread, and tasks run **truly in parallel** (on multiple CPUs).
//! Interpreter values are `Rc`, so multiple threads must not touch them together, but in Siskin tasks
//! share no memory. Values passed to a task, task results, and values sent over channels are all
//! rebuilt from scratch by the sender (`Value::detach`) before being handed over, so any given `Rc` is
//! only ever touched by one thread. Function/struct declarations (the syntax tree) are `Arc`, so sharing reads is fine.
//!
//! Channel/task state is changed and waited on only while holding a single lock (`SYNC`). If every live
//! task is waiting (deadlock), nobody can wake them, so execution stops with a runtime error.
//! The native runtime (`rt_conc.c`) follows the same rules.

use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};

pub struct Sched {
    /// Number of tasks not yet finished (including main)
    pub live: usize,
    /// How many of them are waiting on something
    blocked: usize,
    /// Incremented on every wake-all. At the moment of waking, `blocked` is reset to 0
    /// (counting a woken task that hasn't yet grabbed the lock as "waiting" would be misread as deadlock).
    epoch: u64,
}

static SYNC: Mutex<Sched> = Mutex::new(Sched { live: 1, blocked: 0, epoch: 0 });
static CV: Condvar = Condvar::new();
/// The main file (path, text) used when reporting an error raised inside a task.
static MAIN_SRC: OnceLock<(String, String)> = OnceLock::new();

/// Stack size for task threads. The interpreter recurses deeply, so be generous (only what is used gets committed).
const STACK: usize = 256 * 1024 * 1024;

fn lock() -> MutexGuard<'static, Sched> {
    SYNC.lock().unwrap_or_else(|e| e.into_inner())
}

fn wake(g: &mut Sched) {
    g.blocked = 0;
    g.epoch += 1;
    CV.notify_all();
}

/// Placeholder from the old (one task at a time) scheme. Nothing to do now.
pub fn enter() {}

pub fn set_source(path: &str, src: &str) {
    let _ = MAIN_SRC.set((path.to_string(), src.to_string()));
}

pub fn source() -> Option<&'static (String, String)> {
    MAIN_SRC.get()
}

/// Changes channel/task state. Runs `f` while holding the lock, then wakes all waiters.
pub fn change<T>(f: impl FnOnce() -> T) -> T {
    let mut g = lock();
    let r = f();
    wake(&mut g);
    r
}

/// Waits until `ready()` is true, then runs `act()` while still holding the lock
/// (so no other task can slip in between the check and the action). Returns Err if everyone is waiting (deadlock).
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

/// Previously the big lock was released while waiting (sleep, input). Now it just runs.
pub fn without_gil<T>(f: impl FnOnce() -> T) -> T {
    f()
}

/// Waits until all remaining tasks finish (when main ends).
pub fn wait_all() -> Result<(), ()> {
    wait_until(|s| s.live <= 1)
}

/// Called before the program exits (flushes stdout).
pub fn shutdown() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

struct SendBox(Box<dyn FnOnce()>);
// Safety: all interpreter values captured by the boxed job were freshly created via `detach`,
// so the creating thread never touches them again; only the new thread uses them (`Interp::spawn_task`).
unsafe impl Send for SendBox {}

/// Starts a new task. `job` runs on a new thread and drops every value it holds before finishing.
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

/// One task has finished. Waiters wake up and re-check.
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

/// Keeps other tasks from interleaving output while an error is being reported.
static REPORT: Mutex<()> = Mutex::new(());

/// Reports a runtime error with no location and exits the program.
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

/// Reports a runtime error raised inside a task in the same format as main, then exits.
pub fn report_error(e: &crate::error::SiskinError) -> ! {
    let _g = REPORT.lock().unwrap_or_else(|e| e.into_inner());
    shutdown();
    match source() {
        Some((p, s)) => eprint!("{}", e.render(p, s)),
        None => eprintln!("error[{}]: {}", e.code, e.msg),
    }
    std::process::exit(1);
}
