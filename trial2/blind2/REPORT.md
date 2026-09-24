# REPORT: blind2 (no docs, error messages only)

## 1. Result
- **It worked.** `stats.mi` reads `scores.csv`, skips the blank line and warns (on stderr) about `Kim,history,abc`,
  keeps per-student totals in a `{Str: Student}` map, sorts with `sort_by(fn(s: Student): -average(s))`,
  prints averages with `f"{average(s):.1f}"` and prints `max score: 100`. I checked the values by hand (Kim 209/3=69.7, Lim 223/3=74.3).
- **Run and build give the same output:** `./mica run` output == `./prog > f 2>&1` output, byte for byte (the wrapper merges
  stderr into stdout, so I compared that way. With stdout only, the run log has the warning line and prog's stdout doesn't. That comes from the wrapper, not a bug).
- **Total compiler calls: 18.** 16 were for the task. 2 were a repro of the `import fs` problem (Bugs).
- **Calls until the first successful `run`: 13.** 11 failed `check` calls, then 1 passing `check`, then `run`.
- **Docs: GUIDE.md never opened.** I was never stuck on the same error twice, and every error except the parse errors came with a usable hint.

## 2. Error log
(Messages are in Korean. The English in brackets is my translation. "Fixed from msg?" = could I fix it from the message alone.)

| # | Key line (verbatim, short) | What I wrote / guessed from | Fixed from msg? | Fix |
|---|---|---|---|---|
| 1 | `E0101: 줄바꿈을(를) 기대했는데 키워드 \`as\`이(가) 왔습니다` [expected newline, got keyword `as`] | `s.total as float` (Rust) | partly: no hint about how to convert | guessed `float(x)`; it worked |
| 2 | `E0100: \`:\`을(를) 기대했는데 이름 \`is\`이(가) 왔습니다` | `if score is None` (Python) | partly: generic parse error | `== none` (later replaced by `catch`) |
| 3 | `E0100: \`:\`...키워드 \`not\`이(가) 왔습니다` | `if name not in students` (Python) | no: generic parse error | tried `not (name in d)` |
| 4 | `E0116: \`)\`을(를) 기대했는데 키워드 \`in\`이(가) 왔습니다` | `name in students` | no: nothing says "use `.has()`" | guessed `.contains()`; `.has()` came from msg 9 |
| 5 | `E0116: 이름 붙은 인자는 \`=\` 가 아니라 \`:\` 로 씁니다` [named args use `:`, not `=`] | `Student(name=name, ...)` (Python kwargs) | **yes**, with an example | `Student(name: name, ...)` |
| 6 | same as 5, for `sort(key=...)` | `list.sort(key=fn...)` | yes | `key:` (later replaced) |
| 7 | `E0118: \`strings\` 을(를) 찾을 수 없습니다 ... 패키지도 아닙니다` [`strings` not found, not a package] | `import strings` (Go) | partly: hint suggests `mica add`, not std | dropped the import, used methods |
| 8 | 15 errors in one batch: `타입 \`float\`을(를) 찾을 수 없습니다 / 도움말: 실수는 \`Float\` 입니다` [type `float` not found; floats are `Float`]; same for `str`/`int`; `enumerate` → `for i in range(len(xs))`; `None` → `none`; `map[..]` → `{Str: Int}`; `let` → `var`; lambda param type needed | a Python/Go mix | **yes**: every one had a direct hint | applied them all at once |
| 9 | 12 JSON diagnostics: `read` → "did you mean `read_text`"; `parse` must come from `std.json`; `!Json` value can't go in `score`, use `try`/`catch e:`; `{Str: Student}` has no `contains`, "쓸 수 있는 것: len, get, set, has, keys" [available: len, get, set, has, keys]; no `values`, use `for k, v in d:` | `fs.read(...)`, `parse(...)`, `d.contains`, `d.values()` | **yes**: listing the available methods helped a lot | `read_text`, `int(x) catch e:`, `for k, v in d` |
| 10 | `read_text은(는) std.fs에서 가져와야 합니다` [must import from std.fs]; `get(키, 기본값)` needs 2 args; `append` → "Mica 에서는 \`push\` 입니다" [in Mica it's `push`]; `sort()` takes no args, use `sort_by(fn(x): -key)` | `fs.read_text`, `d.get(k)`, `append`, `sort(key:)` | **yes** | `from std.fs import read_text`, `get(k, default)`, `push`, `sort_by` |
| 11 | `Str에 \`trim\` 메서드가 없습니다 / Mica 에서는 \`strip\` 입니다` [`Str` has no `trim`; it's `strip`] | `.trim()` (Rust/JS) | yes | `.strip()` |

## 3. Wrong guesses
- `x as float` cast → `float(x)`. `as` is a keyword but not a cast operator.
- `is None` / `None` → `== none` (lowercase). Optional types are `?T`, and fallible types are `!T`, which are a different thing.
- `x in dict` / `not in` → `d.has(k)`. There is no `in` operator in expressions.
- Python kwargs `f(a=1)` → `f(a: 1)`.
- `import strings` / `import fs` + `fs.read_file` → `from std.fs import read_text` (or `import std.fs`). String functions are methods.
- Lowercase types `int/str/float`, `map[K,V]` → `Int/Str/Float`, `{K: V}`. Lists are `[T]`.
- `let` for everything → `let` is immutable, `var` is mutable.
- `enumerate`, `d.values()`, `list.append`, `str.trim`, `d.get(k)` with one arg, `sort(key=...)` → none of these exist. Use `range(len)`,
  `for k, v in d`, `push`, `strip`, `get(k, default)`, `sort_by`.
- `parse_int(s)` returning an optional → `int(s)` is fallible (`!Int`) and is handled with `x = f() catch e:` + block.
- Lambdas need annotated parameter types (`fn(s: Student): expr`). The type isn't inferred from `sort_by`.
- Guesses that turned out right: `fn name(a: T) -> R:`, `struct` with `field: Type` lines, `Student(...)` construction,
  f-strings with `{x:.1f}`, `eprint`, `continue`, `return` from `main`, `for x in list`, `range`, `len`, `d[k] = v`,
  `+=` on a `var` struct copy's fields.

## 4. Doc gaps
Not applicable: I never opened GUIDE.md. Gaps in the *messages* instead:
- Parse errors (E0100/E0101/E0116) for `as`, `is`, `not in`, `in` only say "expected X, got Y". Unlike the type errors, they carry no
  "Mica uses ..." hint.
- `import strings` error (E0118) hints `mica add strings <git url>` but never mentions `std.` (e.g. "did you mean `import std.strings`?"
  or "string functions are methods on Str").
- All diagnostics are Korean-only. I found no flag or env var for English (and didn't look further).
- `_ 값에는 \`+=\` 같은 연산을 쓸 수 없습니다` [can't use ops like `+=` on a `_` value] (T0007, `"fix": null`) is a cascade error: it shows a
  placeholder type `_` and adds noise to the real `?Student` error.

## 5. Bugs
- **`import fs` is silently accepted but useless (misleading).** Repro (`repro_import.mi`):
  ```
  import fs
  fn main():
      let t = fs.read_text("scores.csv") catch e:
          return
      print(t)
  ```
  → `T0041: read_text은(는) std.fs에서 가져와야 합니다` [must import from std.fs], but the user did import `fs`. `import std.fs` +
  `fs.read_text` passes (`repro_import2.mi`). Expected: an error on the `import fs` line itself ("no module `fs`; did you mean `std.fs`?").
  Earlier, `fs.read_file` gave "function `read_file` not found" (T0038) instead of "module fs has no `read_file`".
- No crashes, hangs, or run/build differences.

## 6. What worked well
- The type checker reports **all** errors at once (15 in one batch), and nearly every one has a concrete hint:
  `실수는 Float 입니다` [floats are `Float`], `Mica 에서는 push 입니다` [in Mica it's `push`], `for i in range(len(xs))`, `var` vs `inout`.
  Most of the language was learned in 3 calls.
- Listing the available methods ("쓸 수 있는 것: len, get, set, has, keys" [available: len, get, set, has, keys]) made dictionaries easy to learn.
- The "did you mean" hints across modules are useful (`read` → `read_text`, then `from std.fs import read_text`), and so is the hint that
  `parse` lives in `std.json` (this stopped me from misusing a JSON parser).
- The `!T` fallible type errors taught the `x = f() catch e:` block without docs, and explained that `!Json` can't be compared with `==`.
- The named-argument error shows a worked example (`Item(name: "사과", qty: 3)`).
- `check --json` is clean and has a `fix` field. f-strings with format specs worked as guessed. Run == build.

## 7. Top 3 suggestions
1. Add "Mica uses ..." hints to **parse** errors for common foreign syntax: `as` → `float(x)`, `is None` → `== none`,
   `k in d` / `not in` → `d.has(k)`. These were the only errors I fixed by blind guessing.
2. Reject `import fs` / `import strings` with "did you mean `import std.fs`?" (and say that string functions are `Str` methods)
   instead of accepting it silently or suggesting `mica add`.
3. Offer English (or bilingual) diagnostics (e.g. a `--lang en` flag), and suppress cascade errors on `_` types (T0007 with a null fix).
