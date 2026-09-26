#!/bin/sh
# 도구 시험: siskin fmt, siskin lsp, siskin debug, 패키지 관리. 사용법: sh tests/tools.sh <siskin 경로>
M="${1:-siskin}"
HERE="$(cd "$(dirname "$0")" && pwd)"
T="$(mktemp -d)"
# 윈도우(Git Bash): siskin 과 git 이 알아듣는 C:/... 모양 경로를 씁니다.
if command -v cygpath > /dev/null; then T="$(cygpath -m "$T")"; fi
# git 에 넘길 file:// 주소 (윈도우는 file:///C:/...)
furl() { case "$1" in /*) echo "file://$1" ;; *) echo "file:///$1" ;; esac; }
PY="$(command -v python3 || command -v python)"
pass=0; fail=0
# 맥에는 timeout 명령이 없습니다(Homebrew 의 gtimeout 이 있으면 그것을 씁니다).
if ! command -v timeout > /dev/null; then
    if command -v gtimeout > /dev/null; then timeout() { gtimeout "$@"; }; else timeout() { shift; "$@"; }; fi
fi
ok() { pass=$((pass+1)); }
bad() { fail=$((fail+1)); echo "실패: $1"; }

# fmt: 정해 둔 결과와 같고, 두 번 해도 같아야 함
"$M" fmt --stdout "$HERE/fmt/messy.skn" > "$T/f1.skn" && cmp -s "$T/f1.skn" "$HERE/fmt/messy.expected" && ok || { bad "fmt 결과"; diff "$T/f1.skn" "$HERE/fmt/messy.expected" | head -5; }
"$M" fmt --stdout "$T/f1.skn" > "$T/f2.skn" && cmp -s "$T/f1.skn" "$T/f2.skn" && ok || bad "fmt 두 번"

# lsp: 진단, 서식, 정의, 설명, 끝내기
(cd "$HERE/lsp" && "$PY" client.py "$M") > "$T/lsp.txt" 2>&1
grep -q "T0018" "$T/lsp.txt" && grep -q "exit code 0" "$T/lsp.txt" && grep -q "두 수를 더합니다" "$T/lsp.txt" && ok || { bad "lsp"; tail -5 "$T/lsp.txt"; }

# debug: 멈출 곳에서 변수 보기
printf 'b 7\nc\np t\nc\nd 7\nc\n' | "$M" debug "$HERE/debug/sample.skn" > "$T/dbg.out" 2> "$T/dbg.err"
grep -q "breakpoint at line 7" "$T/dbg.err" && grep -q "합의 두 배: 12" "$T/dbg.out" && grep -q "program finished" "$T/dbg.err" && ok || bad "debug"

# debug 네이티브 방식(C 라이브러리·std.net·spawn 프로그램용): 같은 명령, 식 계산, 작업 안에서 멈추기
printf 'b 7\nc\np t + 100\nc\nd 7\nc\n' | "$M" debug --native "$HERE/debug/sample.skn" > "$T/ndbg.out" 2> "$T/ndbg.err"
grep -q "breakpoint at line 7" "$T/ndbg.err" && grep -q "^(siskin) 100" "$T/ndbg.err" && grep -q "합의 두 배: 12" "$T/ndbg.out" && grep -q "program finished" "$T/ndbg.err" && ok || bad "debug 네이티브"
printf 'b 10\nc\nv\nd 10\nc\n' | "$M" debug "$HERE/conc.skn" > "$T/cdbg.out" 2> "$T/cdbg.err"
grep -q "(task" "$T/cdbg.err" && grep -q "ch = <channel>" "$T/cdbg.err" && grep -q "received \[0, 1, 4, 9, 16\]" "$T/cdbg.out" && ok || bad "debug 작업(spawn)"

# 패키지: git 저장소 하나를 만들어 add → run → lock 으로 다시 받기
mkdir -p "$T/lib" && cd "$T/lib" && git init -q -b main && printf 'fn shout(s: Str) -> Str:\n    return s.upper() + "!"\n' > lib.skn \
  && git add -A && git -c user.email=t@t -c user.name=t commit -qm v1 && git tag v1
cd "$T" && "$M" new app > /dev/null && cd app && printf 'import loud\nfn main():\n    print(loud.shout("hi") + "\\n")\n' > main.skn
"$M" add loud "$(furl "$T/lib")" --rev v1 > /dev/null && [ "$("$M" run main.skn)" = "HI!" ] && ok || bad "패키지 add/run"
rm -rf .siskin && "$M" install > /dev/null && [ "$("$M" run main.skn)" = "HI!" ] && ok || bad "패키지 install"
"$M" build main.skn -o "$T/appbin" > /dev/null && [ "$("$T/appbin")" = "HI!" ] && ok || bad "패키지 build"

# 이름공간: 두 패키지가 같은 이름(shout)을 써도 `모듈.이름` 으로 갈라 씁니다
mkdir -p "$T/lib2" && cd "$T/lib2" && git init -q -b main && printf 'fn shout(s: Str) -> Str:\n    return s.lower() + "..."\n' > lib.skn \
  && git add -A && git -c user.email=t@t -c user.name=t commit -qm v1
cd "$T/app" && "$M" add soft "$(furl "$T/lib2")" > /dev/null \
  && printf 'import loud\nimport soft as s\nfn main():\n    print(loud.shout("Hi") + " " + s.shout("Hi") + "\\n")\n' > both.skn \
  && [ "$("$M" run both.skn)" = "HI! hi..." ] && "$M" build both.skn -o "$T/both" > /dev/null && [ "$("$T/both")" = "HI! hi..." ] && ok || bad "패키지 이름공간"
printf 'import loud\nimport soft\nfn main():\n    print(shout("x"))\n' > amb.skn
"$M" run amb.skn 2>&1 | grep -q "E0147" && ok || bad "이름공간 모호함"

# 패키지 목록(레지스트리): git 저장소 하나에 packages/이름.toml. `siskin add 이름` 이 주소를 찾습니다.
mkdir -p "$T/reg/packages" && cd "$T/reg" && git init -q -b main \
  && printf 'git = "%s"\ndescription = "Loud text"\n' "$(furl "$T/lib")" > packages/loud.toml \
  && git add -A && git -c user.email=t@t -c user.name=t commit -qm init
cd "$T" && "$M" new app2 > /dev/null && cd app2 && printf 'import loud\nfn main():\n    print(loud.shout("hey") + "\\n")\n' > main.skn
export SISKIN_HOME="$T/home"
SISKIN_REGISTRY="$(furl "$T/reg")" "$M" search loud | grep -q "Loud text" && ok || bad "패키지 목록 search"
SISKIN_REGISTRY="$(furl "$T/reg")" "$M" add loud > /dev/null && [ "$("$M" run main.skn)" = "HEY!" ] && ok || bad "패키지 목록 add"
SISKIN_REGISTRY="$(furl "$T/reg")" "$M" add lod 2>&1 | grep -q "did you mean: loud" && ok || bad "패키지 목록 비슷한 이름"
unset SISKIN_HOME

# https 서버: 시험용 인증서를 만들어 run 과 build 가 같은지
if command -v openssl > /dev/null; then
  cd "$T" && openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 1 -subj /CN=localhost \
    -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" > /dev/null 2>&1 && cp "$HERE/net/https_server.skn" .
  r1="$(SSL_CERT_FILE="$T/cert.pem" timeout 60 "$M" run https_server.skn 2>&1)"
  "$M" build https_server.skn -o "$T/hs" > /dev/null 2>&1
  r2="$(SSL_CERT_FILE="$T/cert.pem" timeout 60 "$T/hs" 2>&1)"
  echo "$r1" | grep -q "처리한 요청: 3" && [ "$r1" = "$r2" ] && ok || { bad "https 서버"; echo "run: $r1"; echo "build: $r2"; }
fi

rm -rf "$T"
echo "tools pass=$pass fail=$fail"
[ $fail -eq 0 ]
