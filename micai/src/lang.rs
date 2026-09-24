//! 진단 메시지 언어. 기본은 영어이고, `--lang ko` 나 `SISKIN_LANG=ko` 면 한국어입니다.
//! `siskin build` 는 고른 언어를 실행 파일에 굳혀 넣습니다(`MI_KO`), 그래서
//! 같은 언어로 돌린 `siskin run` 과 만든 프로그램의 오류 글이 똑같습니다.

use std::sync::atomic::{AtomicBool, Ordering};

static KO: AtomicBool = AtomicBool::new(false);

/// 지금 한국어로 말하는가.
pub fn ko() -> bool {
    KO.load(Ordering::Relaxed)
}

pub fn set_ko(v: bool) {
    KO.store(v, Ordering::Relaxed);
}

/// 환경 변수와 명령줄에서 언어를 고릅니다. `--lang` 은 첫 `.skn` 파일 앞에 있을 때만
/// 컴파일러 옵션으로 보고 지웁니다(그 뒤는 프로그램에 넘길 인자일 수 있습니다).
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

/// `tr!(한국어, 영어)` — 지금 언어에 맞는 쪽을 고릅니다. 두 쪽은 같은 타입이어야 합니다.
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
