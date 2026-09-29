#!/bin/sh
# Tool tests: siskin fmt, siskin lsp, siskin debug, package management. Usage: sh tests/tools.sh <path to siskin>
M="${1:-siskin}"
HERE="$(cd "$(dirname "$0")" && pwd)"
T="$(mktemp -d)"
# Windows (Git Bash): use C:/...-style paths that both siskin and git understand.
if command -v cygpath > /dev/null; then T="$(cygpath -m "$T")"; fi
# file:// URL to pass to git (file:///C:/... on Windows)
furl() { case "$1" in /*) echo "file://$1" ;; *) echo "file:///$1" ;; esac; }
PY="$(command -v python3 || command -v python)"
pass=0; fail=0
# macOS has no timeout command (use Homebrew's gtimeout if available).
if ! command -v timeout > /dev/null; then
    if command -v gtimeout > /dev/null; then timeout() { gtimeout "$@"; }
    else timeout() { t="$1"; shift; perl -e 'alarm shift; exec @ARGV or exit 127' "$t" "$@"; }; fi
fi
ok() { pass=$((pass+1)); }
bad() { fail=$((fail+1)); echo "FAILED: $1"; }

# fmt: output must match the expected file, and formatting twice must give the same result
"$M" fmt --stdout "$HERE/fmt/messy.skn" > "$T/f1.skn" && cmp -s "$T/f1.skn" "$HERE/fmt/messy.expected" && ok || { bad "fmt output"; diff "$T/f1.skn" "$HERE/fmt/messy.expected" | head -5; }
"$M" fmt --stdout "$T/f1.skn" > "$T/f2.skn" && cmp -s "$T/f1.skn" "$T/f2.skn" && ok || bad "fmt twice"

# lsp: diagnostics, formatting, definition, hover, shutdown
(cd "$HERE/lsp" && "$PY" client.py "$M") > "$T/lsp.txt" 2>&1
grep -q "T0018" "$T/lsp.txt" && grep -q "exit code 0" "$T/lsp.txt" && grep -q "Adds two numbers" "$T/lsp.txt" && ok || { bad "lsp"; tail -5 "$T/lsp.txt"; }

# debug: inspect variables at a breakpoint
printf 'b 7\nc\np t\nc\nd 7\nc\n' | "$M" debug "$HERE/debug/sample.skn" > "$T/dbg.out" 2> "$T/dbg.err"
grep -q "breakpoint at line 7" "$T/dbg.err" && grep -q "twice the sum: 12" "$T/dbg.out" && grep -q "program finished" "$T/dbg.err" && ok || bad "debug"

# debug in native mode (for programs using C libraries, std.net or spawn): same commands, expression evaluation, stopping inside a task
printf 'b 7\nc\np t + 100\nc\nd 7\nc\n' | "$M" debug --native "$HERE/debug/sample.skn" > "$T/ndbg.out" 2> "$T/ndbg.err"
grep -q "breakpoint at line 7" "$T/ndbg.err" && grep -q "^(siskin) 100" "$T/ndbg.err" && grep -q "twice the sum: 12" "$T/ndbg.out" && grep -q "program finished" "$T/ndbg.err" && ok || bad "debug native"
printf 'b 10\nc\nv\nd 10\nc\n' | "$M" debug "$HERE/conc.skn" > "$T/cdbg.out" 2> "$T/cdbg.err"
grep -q "(task" "$T/cdbg.err" && grep -q "ch = <channel>" "$T/cdbg.err" && grep -q "received \[0, 1, 4, 9, 16\]" "$T/cdbg.out" && ok || bad "debug task (spawn)"

# packages: create a git repository, then add -> run -> re-fetch from the lock file
mkdir -p "$T/lib" && cd "$T/lib" && git init -q -b main && printf 'fn shout(s: Str) -> Str:\n    return s.upper() + "!"\n' > lib.skn \
  && git add -A && git -c user.email=t@t -c user.name=t commit -qm v1 && git tag v1
cd "$T" && "$M" new app > /dev/null && cd app && printf 'import loud\nfn main():\n    print(loud.shout("hi") + "\\n")\n' > main.skn
"$M" add loud "$(furl "$T/lib")" --rev v1 > /dev/null && [ "$("$M" run main.skn)" = "HI!" ] && ok || bad "package add/run"
rm -rf .siskin && "$M" install > /dev/null && [ "$("$M" run main.skn)" = "HI!" ] && ok || bad "package install"
"$M" build main.skn -o "$T/appbin" > /dev/null && [ "$("$T/appbin")" = "HI!" ] && ok || bad "package build"

# namespaces: two packages may define the same name (shout); `module.name` tells them apart
mkdir -p "$T/lib2" && cd "$T/lib2" && git init -q -b main && printf 'fn shout(s: Str) -> Str:\n    return s.lower() + "..."\n' > lib.skn \
  && git add -A && git -c user.email=t@t -c user.name=t commit -qm v1
cd "$T/app" && "$M" add soft "$(furl "$T/lib2")" > /dev/null \
  && printf 'import loud\nimport soft as s\nfn main():\n    print(loud.shout("Hi") + " " + s.shout("Hi") + "\\n")\n' > both.skn \
  && [ "$("$M" run both.skn)" = "HI! hi..." ] && "$M" build both.skn -o "$T/both" > /dev/null && [ "$("$T/both")" = "HI! hi..." ] && ok || bad "package namespaces"
printf 'import loud\nimport soft\nfn main():\n    print(shout("x"))\n' > amb.skn
"$M" run amb.skn 2>&1 | grep -q "E0147" && ok || bad "namespace ambiguity"

# package list (registry): a git repository with packages/<name>.toml. `siskin add <name>` looks up the source URL.
mkdir -p "$T/reg/packages" && cd "$T/reg" && git init -q -b main \
  && printf 'git = "%s"\ndescription = "Loud text"\n' "$(furl "$T/lib")" > packages/loud.toml \
  && git add -A && git -c user.email=t@t -c user.name=t commit -qm init
cd "$T" && "$M" new app2 > /dev/null && cd app2 && printf 'import loud\nfn main():\n    print(loud.shout("hey") + "\\n")\n' > main.skn
export SISKIN_HOME="$T/home"
SISKIN_REGISTRY="$(furl "$T/reg")" "$M" search loud | grep -q "Loud text" && ok || bad "registry search"
SISKIN_REGISTRY="$(furl "$T/reg")" "$M" add loud > /dev/null && [ "$("$M" run main.skn)" = "HEY!" ] && ok || bad "registry add"
SISKIN_REGISTRY="$(furl "$T/reg")" "$M" add lod 2>&1 | grep -q "did you mean: loud" && ok || bad "registry similar name"
unset SISKIN_HOME

# https server: create a test certificate and check that run and build match
if command -v openssl > /dev/null; then
  # (MSYS_NO_PATHCONV: stops Git Bash from converting /CN=localhost into a path)
  cd "$T" && MSYS_NO_PATHCONV=1 openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 1 -subj /CN=localhost \
    -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" > /dev/null 2>&1 && cp "$HERE/net/https_server.skn" .
  r1="$(SSL_CERT_FILE="$T/cert.pem" timeout 60 "$M" run https_server.skn 2>&1)"
  "$M" build https_server.skn -o "$T/hs" > /dev/null 2>&1
  hs="$T/hs"; [ -f "$hs.exe" ] && hs="$hs.exe"
  r2="$(SSL_CERT_FILE="$T/cert.pem" timeout 60 "$hs" 2>&1)"
  echo "$r1" | grep -q "requests handled: 3" && [ "$r1" = "$r2" ] && ok || { bad "https server"; echo "run: $r1"; echo "build: $r2"; }
fi

rm -rf "$T"
echo "tools pass=$pass fail=$fail"
[ $fail -eq 0 ]
