//! Diagnostic message language. English by default; Korean with `--lang ko` or `SISKIN_LANG=ko`.
//! `siskin build` bakes the chosen language into the executable (`MI_KO`), so
//! `siskin run` in the same language and the built program print identical error text.

use std::sync::atomic::{AtomicBool, Ordering};

static KO: AtomicBool = AtomicBool::new(false);

/// Whether we are currently speaking Korean.
pub fn ko() -> bool {
    KO.load(Ordering::Relaxed)
}

pub fn set_ko(v: bool) {
    KO.store(v, Ordering::Relaxed);
}

/// Pick the language from the environment and the command line. `--lang` is treated as a
/// compiler option and removed only before the first `.skn` file (after that it may be a program argument).
pub fn init(args: &mut Vec<String>) {
    if let Ok(v) = std::env::var("SISKIN_LANG") {
        set_ko(v.to_ascii_lowercase().starts_with("ko"));
    }
    let mut i = 0;
    while i < args.len() {
        if args[i].ends_with(".skn") {
            break;
        }
        if args[i] == "--lang" && i + 1 < args.len() {
            set_ko(args[i + 1].to_ascii_lowercase().starts_with("ko"));
            args.drain(i..i + 2);
            continue;
        }
        if let Some(v) = args[i].strip_prefix("--lang=") {
            set_ko(v.to_ascii_lowercase().starts_with("ko"));
            args.remove(i);
            continue;
        }
        i += 1;
    }
}

/// `tr!(korean, english)` — picks the side for the current language. Both sides must have the same type.
#[macro_export]
macro_rules! tr {
    ($ko:expr, $en:expr $(,)?) => {
        if $crate::lang::ko() {
            $ko
        } else {
            $en
        }
    };
}
