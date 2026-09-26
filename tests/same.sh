#!/bin/bash
# run == build test: runs each program with both `siskin run` and `siskin build` and checks that the output and exit code match.
# Usage: bash tests/same.sh <path to siskin>
# Programs that are expected to differ (intentionally broken test files, programs that measure time) are listed in tests/same-expected-diff.txt.
# The zlib and sqlite examples (08, 10) only run where those libraries are installed (SISKIN_TEST_LIBS=1).
M="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
T="$(mktemp -d)"
pass=0; fail=0; known=0
# macOS has no timeout command (use Homebrew's gtimeout if available).
if ! command -v timeout > /dev/null; then
    if command -v gtimeout > /dev/null; then timeout() { gtimeout "$@"; }
    else timeout() { t="$1"; shift; perl -e 'alarm shift; exec @ARGV or exit 127' "$t" "$@"; }; fi
fi
cd "$ROOT"
for f in examples/*.skn tests/*.skn tests/ns/main.skn realworld/*.skn trial/*/*.skn trial2/*/*.skn; do
    n="$(basename "$f")"
    case "$n" in 08_cffi.skn|10_sqlite.skn) [ "$SISKIN_TEST_LIBS" = 1 ] || continue ;; esac
    d="$(dirname "$f")"
    [ -n "$SISKIN_TEST_VERBOSE" ] && echo "... $f"
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
        echo "differs: $f"
        diff <(echo "$a") <(echo "$b") | head -20
    fi
done
rm -rf "$T"
echo "same pass=$pass fail=$fail (known different: $known)"
[ $fail = 0 ]
