#!/bin/bash
# run == build 시험: 같은 프로그램을 `siskin run` 과 `siskin build` 로 돌려 출력과 끝난 코드가 같은지 봅니다.
# 사용법: bash tests/same.sh <siskin 경로>
# 윈도우·맥·리눅스 어디서나 돌도록 C 라이브러리 예제(08, 10, 11)는 뺍니다.
# std.net(14)은 윈도우에서 아직 안 되므로 윈도우에서만 뺍니다.
M="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
T="$(mktemp -d)"
pass=0; fail=0
case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) win=1 ;; *) win=0 ;; esac
for f in "$ROOT"/examples/*.skn "$ROOT"/tests/*.skn "$ROOT"/tests/ns/main.skn; do
    n="$(basename "$f")"
    case "$n" in 08_cffi.skn|10_sqlite.skn|11_cpp.skn|shapes.skn) continue ;; esac
    if [ "$n" = "14_net.skn" ] && [ $win = 1 ]; then continue; fi
    d="$(dirname "$f")"
    a="$(cd "$d" && "$M" run "$n" </dev/null 2>&1; echo "code $?")"
    if (cd "$d" && "$M" build "$n" -o "$T/prog" >"$T/build.log" 2>&1); then
        b="$(cd "$d" && "$T/prog" </dev/null 2>&1; echo "code $?")"
    else
        b="build failed: $(cat "$T/build.log")"
    fi
    if [ "$a" = "$b" ]; then
        pass=$((pass+1))
    else
        fail=$((fail+1))
        echo "다름: $n"
        diff <(echo "$a") <(echo "$b") | head -20
    fi
done
rm -rf "$T"
echo "same pass=$pass fail=$fail"
[ $fail = 0 ]
