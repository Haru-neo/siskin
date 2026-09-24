# Mica blind trial: blind2 (stats.mi, no docs)

## 1. Result
- **Works.** `./mica run stats.mi` and `./mica build stats.mi -o prog && ./prog` gave byte-identical output (`diff` clean).
- Total compiler calls: **25** (`===` lines in attempts.log), including 6 probe calls on a scratch `probe.mi`.
- Calls until first successful `run`: **16**. The first output with correctly formatted averages came at call **21**.
- **The docs (GUIDE.md) were never opened.** I was never stuck on the same error for 4 attempts in a row, because each
  error either named the fix or let me cross off one guess.
- All diagnostics are in **Korean** (e.g. `오류[E0100]`, `도움말:`). I can read Korean, so this did not block me. A
  non-Korean reader would lose most of the value of the hints. `--json` is also Korean-only.

## 2. Error log (one entry per failed attempt)
1. `` `:`을(를) 기대했는데 키워드 `in`이(가) 왔습니다 `` (expected `:`, got keyword `in`). Wrote `if name in students:`
   (Python). Message: **partly**. It says `in` is not an operator but gives no alternative. Fix: dropped membership tests.
2. `` `)`을(를) 기대했는데 `=`이(가) 왔습니다 ``. Wrote `Student(name=name, ...)` (Python kwargs). **Partly**. Fix: positional.
3. `줄바꿈을(를) 기대했는데 {이(가) 왔습니다`. Tried the Rust form `Student{name: name, ...}`. **Partly**. Fix: `Student(name, score, 1)`.
4. `` `strings` 을(를) 찾을 수 없습니다 ... 패키지도 아닙니다 ``. Wrote `import strings` (Go). **Partly**. Fix: string
   functions are methods and need no import.
5. Same error for `import string`. **Partly**. Fix: removed the import.
6. 20 errors at once. `타입 float을 찾을 수 없습니다` + `도움말: 기본 타입은 Int, Float, Bool, Str` → **yes**.
   `` `first`은(는) 바꿀 수 없습니다 ... `var`로 선언 `` → **yes**. `익명 함수의 인자 a의 타입을 알 수 없습니다 / fn(a: Int): ...` → **yes**.
   `read_file`, `parse_int`, `format`, and `map` unknown → **no** (no suggestion).
7. (Wasted call: my editor write failed, so the same file was re-checked.)
8. `` `Float`(이)라는 함수를 찾을 수 없습니다 ``, same for `Str(...)`. I guessed that type names are also constructors (Swift/Rust-ish).
   **No**. Fix: I noticed that lowercase `float()`/`str()` were never flagged in attempt 6.
9. `check --json`: same content as text, no extra hints.
10. `` `read_text`은(는) `std.fs`에서 가져와야 합니다 / from std.fs import read_text `` → **yes, excellent**.
    `실패할 수 있는 값(!Str)을 그냥 담을 수 없습니다 / try 또는 catch e:` → **yes**. `Int에 to_float 메서드가 없습니다` → no.
    `타입 Dict을 찾을 수 없습니다` → no.
11. `Str에 trim 메서드가 없습니다`, `{Str: Student}에 contains/values 메서드가 없습니다`, `Float에 round 메서드가 없습니다`
    → **no** (the hint just says "check available methods with `mica check`", which is circular).
    `?Student 값에서 바로 total을 꺼낼 수 없습니다 / if x != none: 안에서 ... 좁혀집니다` → **yes**. The map type
    `{Str: Student}` was a guess that worked.
12. `Str에 to_int 메서드가 없습니다` (masked until now), `[Student]에 append 메서드가 없습니다` → no;
    `round은 std.math에서 가져와야 합니다` → yes. Fixes: `int(s)`, `push`, the import.
13. `score에 실패할 수 있는 값(!Int)` + `Int와(과) !Int에 +을 쓸 수 없습니다 / float(x) 또는 int(x)로 맞추세요`.
    The first message is **yes**. The follow-on hint about conversion is misleading. Fix: `int(parts[2]) catch e: ... continue`.
14. (run) `오류[E0245]: Student 리스트는 정렬할 수 없습니다 / Int, Float, Str, Bool 리스트만 정렬됩니다`. Wrote a comparator
    `list.sort(fn(a,b): ...)` (JS/Rust sort_by). **Partly**: it says what is impossible, but not the alternative. Fix: a
    guessed `list.sort_by(fn(s: Student): -average(s))`, which worked.
15. No error, but wrong output: `str(round(x, 1))` printed `74` instead of `74.3` (see Bugs). Fix: I probed names in
    3 calls and found that `f"{x:.1f}"` works.

## 3. Wrong guesses
- `in` membership operator; `dict.contains`, `.values()` (the map iterates with `for k, v in m:`, and `m[k]` returns `?V`).
- Keyword args or `{}` struct literals: only positional `Student(a, b, c)` works.
- `import strings`/`string`: string ops are methods (`split`, `strip`) with no import.
- Lowercase `int/float/str/map` as *types*: types are `Int Float Bool Str`, `[T]`, `{K: V}`. But the conversion
  *functions* are lowercase `int()`, `float()`, `str()`, and `Float(x)`/`Str(x)` do not exist. This split is surprising.
- `let` is immutable; `var` is mutable (Swift-like). That is fine once the hint told me.
- `fs.read_file` / `fs.read`: it is `from std.fs import read_text`, and the call returns `!Str`.
- `int(s)` returns `!Int` (fallible), not `?Int`. I first compared it to `none`.
- `list.append` → `push`; `trim` → `strip`; `to_int`/`to_float`/`to_str` methods do not exist.
- `list.sort(comparator)` does not work on structs; `sort_by(key)` does.
- `format(...)` / `.to_fixed()` do not exist; Python f-strings with format specs do.

## 4. Doc gaps
I did not read the docs, so these are gaps in what the *compiler* can tell you:
- A "method not found" error never lists the available methods. The hint "`mica check`로 쓸 수 있는 메서드를 확인하세요"
  points back at the tool that just failed.
- A "function not found" error gives no did-you-mean (`read_file`→`read_text`, `parse_int`→`int`, `format`→f-string).
  But when a function exists in an unimported std module, T0041 names the module. I used that as a discovery trick
  by calling candidate names bare.
- There is no hint pointing to f-strings when `format` is unknown.

## 5. Bugs
1. **`check` passes, then `run` fails at runtime on sorting a struct list** (E0245 is emitted *after* earlier prints ran):
   ```
   struct P:
       v: Int
   fn main():
       var xs: [P] = [P(2), P(1)]
       xs.sort(fn(a: P, b: P): a.v < b.v)   # check: OK; run: 오류[E0245] P 리스트는 정렬할 수 없습니다
   ```
   Also, `sort` silently accepts a comparator argument that it apparently ignores or rejects only at runtime.
2. **`round(x, 1)` accepts a second argument and ignores it**, returning a whole number with no error:
   `from std.math import round` then `print(str(round(74.333, 1)) + "\n")` prints `74`. It should either round to
   1 decimal or be rejected by the type checker (wrong arity).
3. **Error message lowercases the type name**: writing `Map[Str, Student]` reports ``타입 `map`을(를) 찾을 수 없습니다``,
   so I could not tell whether my capitalization had been read.
4. **Errors are masked**: `parts[2].to_int()` was only reported in the 5th check that included it. Earlier checks
   never flagged it (probably suppressed by the unknown-type cascade), so the error count was misleadingly low.
5. `if score == none:` where `score: !Int` was not flagged, although the value can never be `none`.
6. For `Int + !Int`, the hint "no implicit conversion, use `float(x)` or `int(x)`" is misleading: the real fix is `try`/`catch`.
7. Type-error spans point at column 1 of the line (`--> stats.mi:8:1`) instead of the offending type token.

## 6. What worked well
- Parse errors stop early and are precise (exact column, "put `:` here").
- Several hints are excellent and fix the error on their own: `var` vs `let`, the basic type list, the typed lambda
  example `fn(a: Int): ...`, `from std.fs import read_text`, narrowing `?T` with `if x != none:`, and `try` / `catch e:` for `!T`.
- `let x = f() catch e:` with a block that can `continue`/`return` is ergonomic for skipping bad CSV rows.
- f-strings with Python format specs (`{x:.1f}`), `for k, v in map`, `split`/`strip`, and `{K: V}` literals all
  matched my Python instincts.
- After the first checker pass, all 20 type errors were reported at once, which made batch fixing fast.
- `run` and `build` agree byte-for-byte.

## 7. Top 3 suggestions
1. **English diagnostics (or a `--lang`/locale switch)** and a did-you-mean on unknown functions and methods,
   e.g. "Str has no `trim`; similar: `strip`", "no `format`; use f\"{x:.1f}\"", "no `append`; did you mean `push`?".
2. **Catch in the checker what currently fails or misbehaves at runtime**: sorting non-primitive lists (and
   suggest `sort_by(key)` in the message), and the wrong arity of `round(x, n)`.
3. **Make method discovery possible from the tool**: list a type's methods in the T0036 error, or add
   `mica doc Str`. Also make "not found" errors for types/conversions explain the convention (types are
   capitalised `Int`, conversions are lowercase `int()`), and stop masking later errors on a line.
