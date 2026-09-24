# REPORT — grades (csvutil.mi + grades.mi)

## 1. Result
- Works. `./mica run grades.mi` and `./mica build grades.mi -o prog && ./prog` give byte-identical stdout
  and byte-identical `report.txt` (diffed both).
- Total compiler calls: 11 (lines starting `===` in attempts.log), incl. 1 extra call for a bug repro.
- Calls until first successful `run`: 5 (3 failed `check`s, 1 passing `check`, then `run` worked first time).
- Doctests in csvutil.mi: 2/2 pass (after one fix). `mica fmt`: "이미 깔끔합니다 (2개 파일)", changed nothing.
- Output checked by hand against scores.csv: averages, subject toppers, empty "all >= 60" list (every student
  has at least one score < 60 in this data), top 3 (Lim 74.3, Jung 71.0, Park 70.0) all correct.
  Bad row `Kim,history,abc` reported as line 6; the blank line (line 10) is skipped silently.

Features used: `from csvutil import ...` (sibling module), generic fns `group_by[T]`, `top_n[T]`,
`!T`/`catch` with `return error(...)` inside catch, tuple list `[(Int, Str)]` + `let (ln, why) = b`,
`filter`/`map`/`all`/`sort_by` with closures (nested: `s.scores.all(fn(x): x >= 60)`),
dict `keys()` + `none` narrowing, `f"{x:.1f}"`, `std.fs.write_text`, doctests.

## 2. Error log
1. `오류[E0111]: 반복 변수 이름이(가) 필요한데 `(`이(가) 왔습니다` (grades.mi:45)
   - Wrote `for (ln, why) in bad:` (Python/Rust tuple destructuring in for).
   - Message alone: yes. Fix: `for b in bad:` then `let (ln, why) = b` (a guess; undocumented).
2. `오류[T0054]: 익명 함수의 인자 `x`의 타입을 알 수 없습니다  --> grades.mi:55:20`
   - Real problem was csvutil.mi:55 `sorted.sort_by(fn(x): -score(x))` inside `fn top_n[T]`.
   - Guessed (Rust/TS) that `x` is inferred as `T` from `sorted: [T]`.
   - Message alone: partly. Text right, but location pointed into grades.mi (`if g != none:`), a line with
     no lambda. Had to grep for `fn(x` (see Bugs #1). Fix: `fn(x: T): -score(x)`.
3. `오류[T0002]: `main` 함수가 없습니다` from `./mica check csvutil.mi`
   - Ran check on the module alone to get a correct location for #2; it also demands `main`.
   - Message alone: yes, but a library file can't be checked cleanly (see Doc gaps).
4. Doctest: `문법 오류: [E0116] `)`을(를) 기대했는데 이름 `x`이(가) 왔습니다` for `>>> parse_line("\"x, y\",2")`
   - Expected `\"` inside the `"""` docstring to reach the example verbatim.
   - Message alone: yes; the echoed example `parse_line(""x, y",2")` shows backslashes were eaten.
     Fix: `\\"`. Same as Python non-raw docstrings, so arguably correct, but surprising.

## 3. Wrong guesses
- `for (a, b) in pairs:` is not allowed; `let (a, b) = t` is.
- Lambda param type inside a generic function is not inferred from the `[T]` receiver (it IS inferred for
  concrete element types, e.g. `rows.map(fn(r): r.score)` worked).
- Expected `\"` in a docstring example to stay `\"`.
- Guessed right with zero docs: `fn f[T](...)` generics, `(Int, Str)` tuple type + `(a, b)` literal,
  `from csvutil import ...` for a sibling file, `f"{x:.1f}"`, unary minus on a call,
  `return error(...)` / `continue` from inside a `catch` block.

## 4. Doc gaps
- Local modules / multi-file programs: nothing explains importing a sibling `.mi` file. GUIDE 12.4 covers only
  packages ("`import colors` 로 씁니다. 패키지 폴더의 `lib.mi` 를 읽고"); 9.7 only `std.*`. No word on
  visibility (is everything exported? `pub`?).
- Generics: no mention of `fn f[T]`, type parameters or constraints. `| [T] | std::vector<T> |` (GUIDE 2.5)
  is only notation.
- Tuples: not documented at all (type, literal, destructuring, `.0` access?).
- Format specs: `f"{x:.1f}"` works but GUIDE §3 only shows `{expr}`.
- `mica check` on a library file demands `main` (T0002); docs don't say how to check a module
  (`mica test` on it did work without main).
- Doctest escape handling (`\"` vs `\\"`) not mentioned in GUIDE §9.
- `int(str)` returning `!Int` is only implied by `try int(text)` in §6; builtin table gives no types.

## 5. Bugs
1. Error inside an imported module is reported with the importer's file name and source snippet.
   Repro (in `repro/`):
   ```
   # lib.mi
   fn bad[T](xs: [T]) -> [T]:
       var ys = xs
       ys.sort_by(fn(x): 0)
       return ys
   # main.mi
   from lib import bad
   fn main():
       print(f"{bad([3, 1])}\n")
   ```
   `mica check main.mi` -> `오류[T0054] ... --> main.mi:3:16` showing `fn main():` with a caret past the end
   of the line. Line/col are lib.mi's; file name and snippet are main.mi's. Bad for the "machine-readable
   diagnostics" goal the README stresses.
2. (Inference gap, maybe by design) lambda arg not inferred from `[T]` in a generic body; the hint
   `fn(x: Int): ...` suggests `Int` where the right annotation is `T`.
No run-vs-build differences, crashes or wrong results found.

## 6. What worked well
- Nearly everything guessed from Python/Rust just worked: generics, tuples, sibling import, format spec,
  catch blocks that `return error(...)` / `continue`, `none` narrowing on `d[k]`.
- Closures with list methods read naturally, including nested ones and capturing a local (`best`).
- Dict insertion order made per-student / per-subject output deterministic with no extra sorting.
- Stable `sort_by` (documented in GUIDE 9.7) gives `top_n` well-defined tie behaviour.
- run and build identical; build fast. `mica fmt` changed nothing on 4-space-indented hand code, no surprises.
- Error messages are short with a concrete 도움말 hint.

## 7. Top 3 suggestions
1. Fix diagnostic locations for errors in imported files (file name + snippet must match line/col).
2. Document multi-file programs, generics (`fn f[T]`) and tuples (type, literal, `let (a, b) =`), and
   ideally allow `for (a, b) in xs`. Core features with zero coverage in GUIDE.
3. Infer lambda params from a generic receiver (`[T]` -> `x: T`) or make the T0054 hint suggest `T`;
   let `mica check` accept a library file without `main` (warning or `--lib`).
