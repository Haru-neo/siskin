# Agent usability trial: results and feedback (2026-09-23)

We had 8 AI agents that had never seen Siskin each write a different program.
6 of them were given only the docs (GUIDE, LIBS, README, examples); the other 2 got no docs and worked from guesses and error messages alone.
Every compiler invocation was logged, and I re-ran every bug the agents reported to confirm it myself (marked ✔).

Programs and logs: the `trial/` folder (one folder per task with the `.skn` files and `attempts.log`).

---

## 1. At a glance

| Task | Docs | Compiler calls | First successful run | run = build |
|---|---|---|---|---|
| Word frequency table (with Korean text) | Yes | 37 | 16th | Same (after workaround) |
| Bank (structs, enums, errors) | Yes | 17 | 6th | Same (after workaround) |
| JSON report | Yes | 20 | 3rd | Same |
| Binary tree (3 memory modes) | Yes | 34 | 5th | Same (after workaround) |
| Calculator REPL | Yes | 20 | 7th | Same |
| Report card (2 modules) | Yes | 11 | 5th | Same |
| Inventory management | **No** | 26 | 19th | Same |
| Grade statistics | **No** | 25 | 16th | Same |

- **All 8 finished.** The 2 agents without docs never opened them and got there on error messages alone.
- The 300-line calculator produced the correct answer on its very first run once the syntax was right.
- However, **4 agents hit programs that "work under run but break, or give a different answer, under build."** That means there are still places where the project's number-one invariant is broken.

---

## 2. What went well (praised by the agents across the board)

- Error message format: location, underline, help text. Especially T0025 (narrowing `?T`), T0045 (declare with `var`), T0011 (list of the real names on an enum typo), T0012 (missing match cases), E0210 (list of names in the module), T0059 (catch values).
- `!T` + `try` + `catch e:` — described as clean for recursive parsers and file-read error handling.
- Recursive enums just work without boxing. Match exhaustiveness checking.
- doctest, `siskin fmt` (none of the 8 had anything to change), fast builds, readable `--emit-c`.
- Korean text: `len` counts characters, `s[i]` is a character, Korean JSON round-trips, and float output is identical in both modes.
- The interpreter's diagnostics for memory mistakes (use after free, double free, out of bounds, freeing arena memory) are excellent, down to line numbers and help text.
- The manual-memory version runs at C speed: 200,000 inserts in 0.05 s with `--release`.

---

## 3. run ≠ build — where the invariant breaks (all ✔ reproduced)

| # | Symptom | Minimal repro |
|---|---|---|
| B1 | Iterating a Korean string with `for c in s` gives a **runtime error** under build (loops once per byte but indexes by character) | `for c in "가a": print(c)` |
| B2 | `upper()`/`lower()` only change ASCII letters under build | `"café".upper()` → run `CAFÉ`, build `CAFé` |
| B3 | `max(a, b)`/`min` **evaluate their arguments twice** under build → side effects happen twice, exponential time when recursive | in `max(f(1), f(2))`, `f(2)` is called twice |
| B4 | List `==` works under run but is a C compile error under build | `[1,2] == [1,2]` |
| B5 | `self.xs[0].bump()` inside an `inout self` method → C error under build | trial/bank/repro/inout_elem.skn |
| B6 | NaN output: run prints `NaN`, build prints `-nan` | `str(sqrt(-1.0))` |
| B7 | `Str == none`: run accepts it (always false), build gives a C0011 error (with location 1:1) | `let l = input(); l == none` |
| B8 | `free()` on arena memory: run gives E0233, debug build silently passes, release crashes | trial/tree/mistakes.skn |
| B9 | `read_text` on a file that reports size 0 (`/proc/...`) returns an empty string under build | `read_text("/proc/version")` |

**Also:** C compile failures like B4 and B5 come with an irrelevant hint, "could not link the library, check `link`" (✔),
even for programs that use no libraries. It should say this is an internal error.

**Suggestion:** a test suite that guarantees "if check passes and run succeeds, build gives the same answer." In particular, run every string method on Korean and accented characters in both modes.

---

## 4. Other bugs (✔ reproduced)

- **When `main() -> !Unit` returns an error, the program exits silently with code 0** — there is no way to tell it failed.
- **doctest cannot see the file's `import`s** — the doctest of a function that uses `from std.re import find_all` gets E0222. (Hit by 3 agents.)
- **`siskin check` misses things run catches:** a nonexistent import (`from std.process import args`), `fn main(args: [Str])`, `list.sort(comparison function)` (E0245). Agents that trusted a passing check were fooled several times.
- **`re.find_all(...)` after `import std.re` gives T0041** — GUIDE 9.7 says importing a whole module works.
- **Errors inside an imported file are reported with the importing file's name and code** (only the line number comes from the original file). trial/grades/repro/
- **`round(x, 1)` silently ignores the second argument** → 74.333 comes out as 74.
- **`error(enum value)` is silently converted to text**, and using an enum pattern on a string only produces "cannot find `id`".
- **`case Some(v):` on an `?Int` is silently accepted** and only produces "cannot find `v`".
- The `std.process` list in the E0210 help text is missing `run` and `run_args` (they do exist).
- `input()` returns `""` forever at end of input instead of `none` → read loops never stop.
- In the interpreter, inserting 200,000 items into the default (safe) tree is practically impossible (287 s for 10,000). Values get deep-copied. build takes 2.3 s.
- Memory errors in debug builds have no file or line (the README says it tells you "where").
- The arena escape check (T0040) only looks at `return`. Smuggling a value out through a struct field passes.
- `--emit-c -o x.c` produces `x.c.c`. `siskin --help` describes check as "syntax check only", but it also checks types.

---

## 5. Missing from the docs (where agents got stuck most)

| Missing | Agents affected | Current state |
|---|---|---|
| **Mutating methods with `inout self`** | 4 (bank, tree, calculator + a guesser) | Works. GUIDE chapter 4 only says "with nothing in front of self it is read-only." Everyone tried `mut self`/`var self` first |
| **Command-line arguments `args()`** | 2 | Works. Not documented, so one agent worked around it with environment variables and another with a hack that reads a `/proc` file |
| **Tuples, generics, importing your own files** | Report card | Big features added on 9/20, but GUIDE has no explanation |
| f-string formatting `{x:.1f}` | 2 | Works, undocumented. One agent hand-wrote decimal formatting |
| `x else default`, `a if c else b`, `exit(n)`, `case _:`, string match | Several | Work; only visible in examples or not at all |
| Reading standard input / what `input()` does at end of input | Calculator | Undocumented |
| The fact that error values are always `Str` | Bank, calculator | Undocumented |
| The JSON value type name `Json`, how to iterate a JSON list, whether `as_float()` accepts integers | JSON | Undocumented |
| **The arena example in GUIDE 9.5.2 does not compile** (`push` after `let xs`) | Tree | `examples/errors/arena_escape.skn` has the same issue |
| Cost of value copies (when copies happen) | Tree | Undocumented |

---

## 6. Error messages — accepting guesses carried over from other languages

This is where the 2 agents without docs spent the most time. Most of the time the message just says "no such thing" without pointing to the Siskin alternative.

| What they tried | Current message | What it should suggest |
|---|---|---|
| `mut self`, `var self` | "parameter `mut` has no type" (off the mark) | `inout self` |
| `P(a=1)` | "expected `)` but found `=`" | `P(a: 1)` |
| `Option[T]`, `T?`, `Some(x)`, `None` | no such type/function | `?T`, `none` |
| `Result[T,E]`, `Ok`, `Err` | no type `result` (shown lowercased) | `!T`, `error(...)`, `catch` |
| `str`, `list[T]`, `Float(x)` | no such type | `Str`, `[T]`, `float(x)` |
| `sorted(xs, key=f)`, `append`, `trim` | no such function/method | `sort_by`, `push`, `strip` |
| `Category.Food` | "put a `:` here" (wrong advice) | `Food` |
| `x = 1` (without let) | "cannot find `x`" | declare with `let`/`var` |
| `pass` | "cannot find `pass`" | none; how to write an empty block |
| Nonexistent method (T0036) | "check with `siskin check`" (that is the command being run) | suggest similar names |
| Accessing a field on `!T` (T0026) | no help text | point to `try`/`catch` |

Other points:
- A few mistakes snowballed into 59 errors (the same type error repeated at every use site).
- Type error locations are often reported at column 1.
- **Every message is in Korean**, including `--json` output. The agents could read it, but 2 of them said a language option (`--lang`, `LANG`) is needed with English-speaking users and tools in mind.

---

## 7. Features they missed

- **Carrying error kinds as an enum** (`match e:` after `catch e:`). Right now the workaround is to encode them into a string and parse them back out.
- `eprint` (print to stderr), character codes (`ord`), character classification like `is_alpha`.
- Using `match` as an expression (returning a value).

---

## 8. Suggested priorities for fixes

1. **The 9 run ≠ build issues (B1–B9)** — these are invariant violations; Korean iteration (B1) and double evaluation in `max` (B3) in particular silently produce wrong answers.
2. **Fill in the docs** — `inout self`, `args()`, tuples/generics/modules, formatting, `else`/`case _`, input, and fix the arena example that does not compile. Removes the most blockers with no code changes.
3. **Make check catch as much as run**, make errors from `main` exit with a message and a nonzero code, and let doctest see imports.
4. **Help text that accepts guesses from other languages** (the table in section 6).
5. Error enums, an English message option, reducing the cost of value copies.

---

## 9. Results of the fixes (2026-09-23, Haru's decision: "fix everything")

**run ≠ build:** fixed all of B1–B9. Also fixed what turned up along the way: evaluation order inside f-strings was reversed under build; float division by zero (build gave inf); list `reverse` and `contains` were missing under build; `index_of` compared structs shallowly; declaring a name twice in the same block was a runtime error under run and a C error under build (check now catches it as T0070); a value-returning function that fell off the end gave `none` under run and `0` under build (T0069); Str/Int matches without `case _` (T0068).

**Other bugs:** a failing `main -> !Unit` now prints `error: ...` and exits with code 1; doctest sees imports; check catches nonexistent imports, `main(args)`, `sort(comparison function)`, `round(x, 1)` and `error(enum)`; every standard module can be used via `import std.X`; errors in imported files are reported with that file's name and line; arena escapes through struct fields are caught; memory errors in debug builds have a location; the irrelevant "library link" hint → "Siskin internal error"; `--emit-c -o x.c`; `input()` returns `none` at end of input; `import fs` → hint to use `import std.fs`; a variable in an f-string width (`{x:>{w}}`) used to be silently ignored → now error E0143.

**Help for habits from other languages:** `mut self`, `P(a=1)`, `Option`/`Some`/`None`, `Result`/`Ok`/`Err`, `str`/`list[T]`, `sorted`/`append`/`trim`, `Category.Food`, `x = 1` without a declaration, `Int?`, `x as Float`, `x is None`, `inout x` at the call site. Reduced cascades of the same type error. Error locations that were reported at column 1 now point at the name on that line. A `"{x}"` missing its `f` gives warning W0001 (does not block running).

**New:** `pass`, `%=`, `eprint`, `for x in json`, tuple `p.0`, `for (a, b) in tuples`, `x in xs` / `x not in xs`, `==` on lists, structs, enums, tuples, dicts and `?T`.

**Docs:** added to GUIDE: formatting (`{x:.2f}` etc.), `inout self`, value copies, `x else default`, errors are always Str, `match` rules, tuples/generics/splitting into files, `args()`/`input()`/`exit`, a JSON→struct example, contracts on methods, the shape of doctest expected values, sorting by two keys, and 10 more rows in the symbol table. Fixed the arena example. Brought the README method table up to date.

**Tests:** added `tests/agentfix.skn` (run = build). All examples, realworld and tests, plus all 8 first-round trial programs, are run = build.

### Retest with 4 new agents (`trial2/`)

| Task | Round 1: calls / first successful run | Round 2: calls / first successful run |
|---|---|---|
| Word frequency table | 37 / 16th | 15 / **2nd** (0 failures) |
| Bank | 17 / 6th | 17 / **2nd** (0 failures; the rest were deliberate experiments) |
| Inventory, no docs | 26 / 19th | 19 / 9th (11th for the program) |
| Grades, no docs | 25 / 16th | 18 / 13th |

All 4 had run = build byte-for-byte, and the 2 agents without docs again never opened the docs.
Everything new that came up in round 2 (T0068, T0069, E0143, W0001, `import fs`, `in`, and parser help in the list above) has also been fixed.

### Not done yet
- **English messages:** every diagnostic is Korean only. In round 2 the 2 agents without docs again named this as the biggest barrier. It is a big job, translating hundreds of messages, and the default language has to be chosen, so it needs Haru's decision.
- **Carrying error kinds as an enum** (`match e` after `catch e:`): currently split by a text prefix. A language design change.
- Contract violations point at the function header instead of the call site. `match` as an expression, character classification functions like `is_alpha`.

---

## 10. Applying Haru's decisions (2026-09-24, "go ahead with all of it")

**1. Error messages are English by default, Korean is an option.** `siskin --lang ko ...` or `SISKIN_LANG=ko`.
English was added in about 1000 places: the compiler, type checker, runtime errors, runtime errors of built programs, and `siskin fmt`/`test`/`debug`/package/LSP messages. Not a single character of the Korean text was changed (the regression results run in Korean are identical to before).
`siskin build` embeds the chosen language in the executable, so a built program's error text matches `siskin run` in the same language. The header comment of `siskin.lock` is fixed in English regardless of language (so teammates don't get diffs).

**2. Error kinds as enums — `E!T`.**
```siskin
fn withdraw(inout self, amount: Int) -> BankError!Unit:
    if amount > self.balance:
        return error(NoFunds(need: amount - self.balance))
...
acc.withdraw(500) catch e:     # e is a BankError
    match e:                   # the compiler reports any missing kinds
        case NoFunds(need): ...
```
`!T` is still `Str!T`. `try` propagates between identical error types, and propagating an enum error into a `!T` function converts it to text like `NoFunds(430)`. New checks: T0071 (the error type must be an enum), T0072 (a different error type cannot be propagated with try), T0073 (the `error()` value differs from the function's error type). `main -> E!Unit` also works. Confirmed run = build (`tests/errenum.skn`).

**Tests:** all examples, realworld and tests are run = build in both English and Korean; the 8 first-round trial programs also match in both languages; tools 7/7.

## 11. Filling the remaining gaps (2026-09-24, Haru: "keep filling the gaps")

1. **Narrowing struct fields.** Inside `if t.due != none:`, `t.due` is directly a `Str`. Guards (`if t.due == none: return`) and `and`/`or` also work. Narrowing is dropped if the field or anything above it is reassigned, passed as `inout`, or changed inside a loop. When it goes wrong, there is a hint saying "it is `?Str`, check it first" (T0017/T0018). `tests/fieldnarrow.skn`.
2. **Native `?Struct`/`!Struct` fields.** Self-referential fields (`next: ?Node`) are stored in a heap box and deep-copied. As a bonus, "struct declared after the code that uses it" and "tuple fields" no longer cause C compile errors (C types are emitted in dependency order). `tests/optstruct.skn`, `recstruct.skn`, `structfields.skn`, `structorder.skn`.
3. **Concurrency.** `spawn f(x)` → `Task[T]` (`wait`, `done`), `channel[T]()`/`channel[T](n)` (`send`, `recv -> ?T`, `close`, `for x in ch`). Values passed across are copies (no shared memory → no data races). Deadlock is a runtime error (E0260). build is truly parallel with pthreads (3.5x on 4 cores), and run is truly parallel too (section 12). Made std.net thread-safe → servers that `spawn` per connection are possible (`tests/net/net_spawn.skn`). `tests/conc.skn`, `examples/15_concurrency.skn`. New codes: T0074, T0075, E0162, E0260–E0263.
4. **Namespaces.** `import a` → `a.f()`, `a.Point`, `case a.Circle(r)`. `import a as b`, `from a import f as g`; `_name` cannot be used outside its file (E0146). Identical names in two files or two packages no longer collide. Using a name without the module name gives a warning if it exists in only one place (W0002) and an error if in several (E0147). Nonexistent name E0145, overlapping imports E0148, `as` on a standard module E0149. Printed values omit the module name (`Point(x: 1)`), same on both backends. `tests/ns/`, `examples/16_modules.skn`.
5. **Debugger.** Programs that use C libraries, std.net or `spawn` are compiled natively with breakpoint hooks and followed with the same commands (n s o c b d p v l w q) (`src/rt_dbg.c`, communicating over a pipe). It also stops inside tasks (`(task 2)`). It stops inside imported files too, with `b util.skn:5`. No gdb needed.
6. **Bonus: top-level constants.** `let PI = 3.14` is visible in every function (both backends). Top-level `var` (T0076) and executable statements outside main (T0077) are rejected by the checker — previously only `siskin run` accepted them and build failed. Also fixed `siskin run x | head` printing a panic message.

**Tests:** all 16 examples, realworld, tests, and the round 1 and 2 trial programs are run = build (56 programs); tools 11/11 (added package namespaces, native debugger, debugger inside tasks).

**Remaining limitations:** in the native debugger, `p expr` evaluates on a copy of the values at the moment of the stop (C functions cannot be called inside `p`, and `module.name` inside `p` is not supported yet). No central package registry.

## 12. `siskin run` is truly parallel too (2026-09-24, Haru: "make it truly parallel")

- Before: `spawn` under `siskin run` ran one task at a time under a single big lock (a GIL), so results matched but nothing got faster.
- Now: one task = one OS thread + one interpreter, no big lock. On 4 cores, 4 prime-counting tasks went from 2.59 s → 0.81 s (about 3.2x; build gets 3.5x).
- Memory safety: interpreter values (`Rc`) are never shared between threads. Values passed to a task, task results, and values sent through channels are rebuilt wholesale by the sending thread with `Value::detach()` and handed over under a lock (Mutex). Function and struct declarations (the syntax tree) were switched to `Arc` and are read concurrently (Send+Sync checked at compile time). The deadlock check (E0260) is unchanged.
- Bonus: `for i in range(a, b)` counts directly without building a list → about 30% faster even with a single task (this also removed contention where multiple tasks blocked each other on memory allocation).
- Tests: 56 programs run = build, tools 11/11, `tests/conc.skn` gives the same result across 40 repetitions, a stress test sending structs, enums, dicts and closures through channels gives the same result across 20 repetitions, and 5 kinds of deadlock behave the same on both backends.

## 13. HTTPS server and package index (2026-09-24, Haru: "make it possible to build an https server, and as for a central package registry...")

- **HTTPS server.** `listen_tls(port, cert, private_key)` / `listen_tls_on(host, ...)`. Accepted connections are already secure, so `send`/`recv` work unchanged. The OpenSSL server-side functions (`TLS_server_method`, `SSL_accept`, etc.) are also looked up at runtime (`rt_net.c`). If the certificate is missing, is not PEM, or does not match the key, the error is reported in both languages along with the openssl command to generate a test certificate. Clients that fail the secure handshake (e.g. ones that mistakenly connect over plain http) are skipped so the server does not stop.
- **HTTP server helpers.** Previously you had to write servers directly on TCP. Now `server.next_request()` → `Request` (method, path, query, headers, body) → `req.respond(status, body)` / `respond_with(status, headers, body)`. Added `read_request(conn)`, `url_decode`, `Conn.recv_n(n)`. All of it is Siskin code in `std/net.skn`.
- **Package index (registry).** No server: a single git repository (`packages/NAME.toml` with `git` and `description`). `siskin add NAME` looks up the address in the index and writes it in the existing form (`{ git = ... }`). `siskin search WORD`, `siskin publish` (shows the file to add to the index, and writes it directly if the index is a local folder). Index address: `SISKIN_REGISTRY` > `[registry] url` in siskin.toml > default `https://github.com/Haru-neo/siskin-registry` (published 2026-09-24). Cached in `~/.siskin/registry/` and refreshed every time; if there is no internet, the cached copy is used. A mistyped name gets similar-name suggestions. The layout of the index repository is in `registry-template/`.
- **Bonus: fixed one run≠build.** Passing an empty dict or empty list to a method (`b.f({}, [])`) failed only under build with "type _". cgen now passes parameter types to method arguments as well.
- **Tests:** all programs are run = build as before; tools 15/15 (added package index search, add and similar names, plus https server run = build). Also verified GET and POST against the https server with curl.
