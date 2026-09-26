# Siskin

A statically typed language that reads like Python, runs as fast as C, and lets you manage memory
directly like C++ — with a compiler that talks not only to people but also to machines (AI).

Programs run in an interpreter (`siskin run`) or compile to a native binary through C (`siskin build`),
with the same output either way. Error messages are in English by default.

```
fn main():
    let names = ["Haru", "Siskin"]
    for n in names:
        print(f"Hello, {n}!\n")
```

## Installation

Requirements:

- **Rust** (only to build the compiler): install it from https://rustup.rs.
- **A C compiler** (used by `siskin build`). On Linux, `gcc`; on macOS, `xcode-select --install`.
  **On Windows, `winget install MartinStorsjo.LLVM-MinGW.UCRT` is all you need** (it includes clang plus the headers and libraries; open a new terminal after installing).
  An existing Visual Studio + LLVM (clang) setup, or MinGW gcc, also works. Pick a different compiler with the `CC` environment variable.
- Nice to have: `clang` for importing C headers (`import c`; on Windows it comes with the installer above), and OpenSSL for https
  (usually already present on Linux; on macOS, `brew install openssl@3`; on Windows, the one bundled with Git for Windows is used).

```
git clone https://github.com/Haru-neo/siskin
cd siskin
cargo install --path micai
```

This installs the `siskin` command into `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows). To check:

```
siskin run examples/01_hello.skn
```

> Every change is tested on Linux, Windows and macOS with GitHub Actions (`tests/same.sh`: checks that `run` and `build` produce the same results).
> Windows passes the same tests as Linux, including run, build, the debugger, packages and https. C libraries (zlib, etc.) must be installed on the OS itself.

## Quick start

```
siskin new hello          # new project (siskin.toml, main.skn)
cd hello
siskin run main.skn       # run right away (interpreter)
siskin build main.skn     # compile to an executable (via C; --release for more speed)
siskin check main.skn     # check for errors without running
siskin fmt main.skn       # format the code
```

Error messages are in English by default. To see them in Korean, add `--lang ko` or set `SISKIN_LANG=ko`.

Editors: for VS Code, install [`editors/vscode/siskin-0.1.0.vsix`](editors/vscode/) to get syntax highlighting, error squiggles, autocompletion and a run button. Neovim and Helix setups are in [editors/](editors/).

Packages: fetch one with `siskin add <name>` and search with `siskin search`. The package list lives in the [siskin-registry](https://github.com/Haru-neo/siskin-registry) repository.

## Documentation

- [GUIDE.md](GUIDE.md): the user guide. Explains the syntax side by side with C++. Start here if you are new.
- [examples/](examples/): read them in numbered order, starting with `01_hello.skn` (GUIDE chapter 11).
- [LIBS.md](LIBS.md): using C and C++ libraries
- [DESIGN-v0.1.md](DESIGN-v0.1.md): the design document
- [bench/](bench/): benchmarks. 1.03–1.11x the time of C

## Layout

| Folder | Contents |
|---|---|
| `micai/` | Compiler and interpreter (Rust) |
| `examples/` | Example programs; `examples/errors/` holds intentionally broken ones |
| `tests/` | Tests (`bash tests/same.sh siskin`, `sh tests/tools.sh siskin`) |
| `editors/` | VS Code extension, Neovim and Helix setups |
| `bench/` | Speed comparison of the same programs in Siskin and C |
| `realworld/` | Small programs written to try the language for real |
| `registry-template/` | The layout of the package list repository |
| `trial/`, `trial2/` | Records of AI agents trying the language for the first time ([FEEDBACK-agents-v0.1.md](FEEDBACK-agents-v0.1.md)) |

## Commands

| Command | What it does |
|---|---|
| `siskin run <file>` | Run a program |
| `siskin check <file>` | Check for errors |
| `siskin check --json <file>` | Machine-readable diagnostics (design §9.2) |
| `siskin build <file>` | Compile to a native executable (`--release`, `--emit-c`) |
| `siskin test <file>` | Run the `>>>` examples in docstrings (design §9.6) |
| `siskin ffi <header.h>` | Show what a library makes available (`--all`, `--cpp`, `--from`) |
| `siskin debug <file>` | Step through line by line and inspect values (`-b <line>` sets a breakpoint) |
| `siskin fmt <file/folder>` | Format code (`--check`, `--stdout`) |
| `siskin new <name>` | New project (siskin.toml, main.skn) |
| `siskin add <name> <source>` | Add a package (a git URL or a folder, `--rev <tag>`) |
| `siskin install` / `update` / `remove` | Fetch packages / update to new versions / remove |
| `siskin lsp` | Language server for editors ([editors/](editors/)) |
| `siskin tokens <file>` | Token dump (for debugging) |

Error messages are in English by default. For Korean, add `--lang ko` or set `SISKIN_LANG=ko`.

## What works now (execution)

Functions · recursion · `let`/`var` · `if`/`elif`/`else` · `while` · `for ... in` ·
`break`/`continue` · lists · dictionaries · f-strings · structs and methods ·
`interface` · `enum` and `match` · `?T` and `none` · `!T` and `try`/`catch` · error enums (`E!T`) ·
`requires`/`ensures` contracts · doctests · standard library ·
value semantics (modifying a copy leaves the original unchanged) · passing functions as values ·
**closures** (anonymous functions like `fn(x): x * k`, functions inside functions)

## What works now (type checking)

`siskin check` catches these before the program runs: type mismatches, wrong argument count or types, missing fields and methods,
using a `?T` without unwrapping it, using `try` in a function that doesn't return `!T`,
and **variants missing from a `match`** (the promise of design §4.5, now caught at compile time).

## What works now (native compilation)

`siskin build` produces an executable by way of C. 1.0–1.2x the time of C ([bench/](bench/)).

**Every example compiles natively, and its output matches the interpreter character for character.**
(Only the examples that call C libraries are `siskin build`-only.)

Int, Float, Bool, Str, lists, structs and methods, `enum` and `match`,
`?T`/`none`/narrowing, `!T`/`try`/`catch`/`error`, functions and recursion, all control flow,
f-strings (including printing lists), string methods, `requires`/`ensures` contracts, bounds checks,
**three memory levels** (`with arena`, `unsafe`, raw pointers `*T`, `alloc`/`free`, pointer arithmetic),
**C library calls** (`extern "C"`), the entire standard library,
**dictionaries (Dict)**, **regular expressions**, **JSON**.

The generated C contains `#line` markers, so C compiler errors and debuggers
point at lines of the original `.skn` file rather than the translated C.

## Tools

- **Formatter** `siskin fmt`: normalizes indentation (including tabs) and spacing to a single style.
  After formatting, it verifies that not a single token of meaning changed; if anything differs, it leaves the file untouched.
- **Editors**: the VS Code extension ([editors/vscode/](editors/vscode/)) provides highlighting, error squiggles, formatting,
  go to definition, autocompletion and a run button. Neovim, Helix and others connect to `siskin lsp` ([editors/](editors/)).
- **Debugger** `siskin debug`: step over (`n`), step into (`s`), continue to a breakpoint (`c`), inspect values (`p <expr>`, `v`).
  The same commands also work in programs that use C libraries, `std.net` or `spawn`, and inside imported files.
- **Packages** `siskin add` / `siskin install`: use a git repository or a folder as a package.
  Fetched versions are recorded in `siskin.lock`, so every machine gets the same versions.
  Packages listed in the package list (a single git repository) can be fetched by name alone, as in `siskin add <name>`, and found with `siskin search`.
- **Web server** `std.net`: `listen(port)` starts an http server and `listen_tls(port, cert, key)` an https server.
  Receive requests with `server.next_request()` and answer with `req.respond(200, body)`.

## Not yet supported

**Not yet in std.net**: HTTP/2, and multiple requests over one connection (keep-alive).
To handle several connections at once, `spawn` a task per connection.

**Package list**: the default package list repository is [`https://github.com/Haru-neo/siskin-registry`](https://github.com/Haru-neo/siskin-registry) (public since 2026-09-24).
No packages have been published yet. How to publish one is described in that repository's README and in [registry-template/](registry-template/).

**Not yet on the library side**: variadic functions like `printf`,
functions that pass or return structs by value, and importing C++ templates wholesale ahead of time.

## Libraries

A new language has nothing that others have already built. Siskin's answer is to **use the C and C++ world as is**.
Since Siskin compiles by way of C, it calls libraries with no translation layer.

Name a header file (the library's description) and every function in it becomes available.

```
import c "zlib.h" link "z"          # opens 80 zlib functions
import c "sqlite3.h" link "sqlite3" # opens 283 sqlite3 functions
import cpp "shapes.hpp" also "shapes.cpp"
```

Measured results:

| Library | Functions usable out of the box |
|---|---|
| zlib (compression) | 80 of 81 (98%) |
| sqlite3 (database) | 283 of 291 (97%) |
| libpng (images) | 246 of 246 (100%) |
| curses (terminal UI) | 444 of 456 (97%) |
| expat (XML) | 66 of 67 (98%) |

`siskin ffi <header>` shows what was opened and what was left out, and why.
Out-parameters (such as the second argument of `sqlite3_open`) work, as do callbacks
that pass your own functions to a library. From C++, classes, virtual functions, templates and `std::string`
all come through. What doesn't work yet: variadic functions like `printf` and
functions that pass or return structs by value.

See the examples [examples/08_cffi.skn](examples/08_cffi.skn) (C),
[examples/10_sqlite.skn](examples/10_sqlite.skn) (database) and
[examples/11_cpp.skn](examples/11_cpp.skn) (C++); the full guide is [LIBS.md](LIBS.md).

The standard library is kept deliberately small.

| Module | Contents |
|---|---|
| (built-in) | `print` `len` `range` `str` `int` `float` `abs` `min` `max` `sum` `assert` `error` |
| `std.math` | `sqrt` `sin` `cos` `tan` `log` `log10` `exp` `floor` `ceil` `round` `pow` `pi` `e` |
| `std.random` | `seed` `rand` `rand_int` |
| `std.time` | `now` `clock` `sleep` `today` `date` `local_time` `utc_time` `parse_time`, dates via `DateTime` (`format` `add_days` `days_until` ...) |
| `std.fs` | `read_text` `write_text` `append_text` `remove` `exists` `list_dir` `make_dir` `is_dir` |
| `std.process` | `run` `run_args` `env` `set_env` `cwd` `set_cwd` `pid` |
| `std.net` | `http_get` `http_post` `http_request` `url_encode` `set_timeout`, TCP `connect` `connect_tls` `listen` `listen_on` |
| `std.re` | `test` `find` `find_all` `groups` `replace` `split_re` |
| `std.json` | `parse` `stringify` `jnull` `jbool` `jint` `jfloat` `jstr` `jlist` `jdict` |
| Dict | `len` `get(key, default)` `set` `has` `keys`, `k in d`, `for k, v in d:`, `d[k]` gives `none` if missing |
| List | `push` `pop` `len` `reverse` `contains` `join` `sort` `index_of` `slice` `clear`, and `map` `filter` `any` `all` `sort_by`, which take functions |
| Str | `len` `split` `upper` `lower` `strip` `replace` `contains` `starts_with` `ends_with` `find` `repeat` `slice` `width` `pad_left` `pad_right` |

The regular expression engine is written from scratch, with no external library. The same engine lives in both the interpreter
and the C runtime, so results are identical either way. The same goes for JSON.

Regular expressions are full of backslashes, so use **raw strings** such as `r"\d+"`.
If you forget the `r`, the compiler tells you.

For details see sections 9.7 and 9.9 of [GUIDE.md](GUIDE.md); for examples see
[examples/07_stdlib.skn](examples/07_stdlib.skn) and
[examples/09_data.skn](examples/09_data.skn).

## Three levels of memory

| | How | Freed | If you get it wrong |
|---|---|---|---|
| Level 0 | Just use values | Automatically | — |
| Level 1 | `with arena a:` | All at once at the end of the block | Letting a value escape the block is **rejected at compile time** |
| Level 2 | `unsafe:` + `alloc`/`free` | Manually | Debug builds **catch it at run time** |

- Raw pointers may only be used inside `unsafe:`. Using them outside is a compile error.
- An arena block is always freed, even when you leave it with `return`.
- **Debug builds** catch use-after-free, out-of-bounds access and double free,
  and tell you where and how it went wrong — things C silently lets slide.
- **`--release`** builds drop those checks entirely. Pointers become plain C pointers,
  with zero overhead. Arenas run as fast as hand-written C ([bench/](bench/)).

See [examples/06_memory.skn](examples/06_memory.skn) for an example, and
`nounsafe.skn`, `arena_escape.skn` and `uaf.skn` in [examples/errors/](examples/errors/) for the mistakes that get caught.

## Case sensitivity

A name spelled exactly right always wins. Only when there is none, and exactly one name
differs from it just in letter case, does the reference bind to that name. `siskin check` points out the places it fixed.
See section 2.7 of [GUIDE.md](GUIDE.md).

## Problems actually found while building it

These are things that weren't visible on paper — which is exactly why P1 was built first.

1. **Imports didn't cross function boundaries** — `from std.math import sqrt` at the top of a file,
   yet `sqrt` wasn't visible inside functions. Imports must go into the module's globals, not the current frame.
2. **Errors inside f-strings pointed at the wrong line** — the inside of `{...}` is parsed as a separate source,
   so line numbers came out as 1. They must be shifted back to the original position after parsing.
   Diagnostic quality is at the core of being AI-friendly; if this breaks, §9.2 becomes meaningless.
3. **User function names collided with runtime functions** — an example's `fn find` ended up with the same
   name as the generated C's internal function `mi_find`. Fixed by giving user names the prefix `mu_`
   and runtime names `mi_`.
4. **Actually returning freed memory in debug mode makes checking impossible** — catching use-after-free
   means reading a "this memory is dead" marker, but once the memory is returned to the OS,
   reading that marker is itself an unsafe access. So in debug mode freed chunks are held on to
   instead of being returned. That's why debug builds use more memory.
   Under `--release` memory is returned normally.
5. **String length differed between the interpreter and native code** — `"안녕하세요".len()` was
   5 in the interpreter but 15 (bytes) natively. It went unnoticed until we wrote examples with Korean text.
   The native side now counts characters. It's the same trap as C++'s
   `std::string::size()` counting bytes.
6. **C function names clashed with headers** — emitting `extern "C" fn strlen(...)` to C as is
   collides with the `strlen` already declared in `string.h`. Solved by using a different name on our side
   and binding it to the real symbol.
7. **Sliced strings broke when passed to C** — `s.slice(0, 2)` is a view into the middle of the original,
   so it has no terminating 0. Passing it to C as is reads past the end. Strings passed to C now always
   get a 0-terminated copy.
8. **Large floats were printed differently on each side** — putting a very large number into JSON gave
   `100000000000000000000.0` in the interpreter and `1e+20` in compiled code.
   The same program must not give different answers, so the rules were unified.
   This only surfaced while building JSON support.
9. **Backslashes silently vanished in regex strings** — writing `"\d+"` dropped the `\`,
   leaving `d+`, because unknown escapes were being passed through silently.
   This is now an error, and raw strings `r"..."` were added.
10. **Float output differed from the interpreter** — C's `%g` cuts off at 6 significant digits,
   so `12.56636` came out as `12.5664`. Fixed to find the shortest representation that reads back
   as the same value. The same program must not give different answers depending on how it is run.

## License

[Apache License 2.0](LICENSE). The VS Code extension (`editors/vscode/`) is under the MIT license in that folder.
