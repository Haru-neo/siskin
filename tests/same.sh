#!/bin/bash
# run == build 시험: 같은 프로그램을 `siskin run` 과 `siskin build` 로 돌려 출력과 끝난 코드가 같은지 봅니다.
# 사용법: bash tests/same.sh <siskin 경로>
# 원래부터 다른 프로그램(일부러 틀린 시험 파일, 시간을 재는 프로그램)은 tests/same-expected-diff.txt 에 있습니다.
# zlib·sqlite 예제(08, 10)는 그 라이브러리가 깔린 곳에서만 돌립니다(SISKIN_TEST_LIBS=1).
M="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
T="$(mktemp -d)"
pass=0; fail=0; known=0
cd "$ROOT"
for f in examples/*.skn tests/*.skn tests/ns/main.skn realworld/*.skn trial/*/*.skn trial2/*/*.skn; do
    n="$(basename "$f")"
    case "$n" in 08_cffi.skn|10_sqlite.skn) [ "$SISKIN_TEST_LIBS" = 1 ] || continue ;; esac
    d="$(dirname "$f")"
    a="$(cd "$d" && timeout 60 "$M" run "$n" </dev/null 2>&1; echo "code $?")"
    if (cd "$d" && timeout 120 "$M" build "$n" -o "$T/prog" >"$T/build.log" 2>&1); then
        b="$(cd "$d" && timeout 60 "$T/prog" </dev/null 2>&1; echo "code $?")"
    else
        b="build failed: $(cat "$T/build.log")"
    fi
    if [ "$a" = "$b" ]; then
        pass=$((pass+1))
    elif grep -qx "$f" tests/same-expected-diff.txt; then
        known=$((known+1))
    else
        fail=$((fail+1))
        echo "다름: $f"
        diff <(echo "$a") <(echo "$b") | head -20
    fi
done
rm -rf "$T"
echo "same pass=$pass fail=$fail (known different: $known)"
[ $fail = 0 ]
