# Notes from actually using Siskin (2026-09-19)

Instead of short snippets written for examples, I wrote three genuinely useful
programs in Siskin from start to finish and wrote down every place I got stuck.
The programs are in `realworld/`.

| Program | What it does | Result |
|---|---|---|
| `realworld/loganalyze.skn` | Reads 400 lines of web server logs and builds a per-path table of requests, errors, average response time and bytes | Works with both `run` and compilation |
| `realworld/calc.skn` | Expression calculator. Tokenize → parse → evaluate, with precedence, parentheses and error handling | Works only with `run`, **compilation fails** |
| `realworld/todo.skn` | Reads a to-do JSON file, tidies it up and saves it back | Works with both `run` and compilation |

All three eventually ran, but getting there took a lot of workarounds.
Below they are listed from most to least severe.

---

## Resolution status (updated 2026-09-20 — all 20 resolved)

The problems below have been fixed. **For every one of them I confirmed that
`siskin run` and `siskin build` give the same result**, and a regression check
confirmed that the 8 existing examples and the 6 realworld programs behave
identically under both.

**Fixed (16, verified on both backends):**

- **(1)(2)(20) Value semantics** — There is now a single rule. `let`, read-only
  parameters and `self` cannot be modified. To modify something, bind it with `var`
  or take the parameter as `inout`. Code that breaks the rule is caught by
  `siskin check` before compilation. Only a `var` can be passed to an `inout` parameter.
- **(3) Recursive enums** — `enum Tree: Node(l: Tree, r: Tree)` now compiles.
- **(4) inout** — Works correctly for scalars, lists and `self` in both modes.
  (Fixed both the interpreter, which silently ignored it, and the compiler, which rejected it outright.)
- **(5) `?T` fields in structs** — `?Str`, `?Int`, `?Float`, `?Bool` and `?Json` fields work.
  (`?Struct`, an optional holding a struct, is still `siskin run` only.)
- **(6) f-string formatting** — `f"{x:.2f}"`, `f"{s:>10}"` and the like work.
- **(7) Import checking** — `check` and `build` catch uses of standard library modules that were not imported.
- **(8) Input** — Added `input()`, `args()` and `exit()`.
- **(13) The `Json` type name** — Case folding no longer gets in the way.
- **(14) All three ways of unwrapping `?T`** — ① narrowing after a guard (after `if v == none: return`, v is narrowed),
  ② `if v != none and v > 0`, ③ `let v = find(xs) else 0`.
- **(15) `for k, v in d`** — Iterates over a dictionary's keys and values together (in insertion order).
- **(16) `d.get(key, default)`** — Counting is now one line: `counts[w] = counts.get(w, 0) + 1`.
- **(17) `pad_left` / `pad_right` / `width`** — Based on display width, so tables line up even with Korean text (two columns per character).
- **(18) Quotes inside raw strings** — `'single-quoted string'`, `r'...'` and `"""..."""` work.
- **(19) Empty enum variants** — Trying to construct one as `LPar` now tells you to write `LPar()`.
- (Bonus) Also fixed a compiler bug where a `!T` function skipped the following statements after a `try`.

**The 4 big features (9, 10, 11, 12) were added too (verified on both backends, 2026-09-20):**

- **(12) Tuples** — Return several values with `fn f() -> (Int, Str)` and
  destructure them with `let (a, b) = f()`. `check` catches a count mismatch.
- **(11) Generics** — Leave a type slot open, as in `fn first_value[T](xs: [T]) -> ?T`,
  and the actual type is filled in at the call site. Two type parameters (`[A, B]`),
  generics calling generics, and passing structs all work. The native backend
  automatically generates one function per type (monomorphization).
- **(9) Splitting into files (modules)** — `import filename` imports a `.skn` file
  from the same folder. Transitive imports (A→B→C) and duplicate imports are
  resolved automatically.
- **(10) Passing functions as values** — Function types such as `(Int) -> Int` can be used
  for parameters, return values and variables, and named functions can be passed as values.
  With this you can write your own map/filter/callbacks. (Anonymous functions/closures
  that capture their environment are not supported yet, because they cannot be
  represented as C function pointers. Named functions cover most cases.)

These four are major parts of the language's skeleton, so to make sure the tier-1
problem of "the two modes giving different answers" never comes back, both backends
were implemented together and verified with regression tests.

**Note (partial):**

- (5) Narrowing a struct field directly with `if t.due != none:` is not supported yet.
  Binding it once with `let d = t.due` does narrow (confirmed on both backends).

---

## Tier 1 — The same program gives different answers under `siskin run` and compilation

This is the most urgent problem. The answer changes with no warning and no error.

### (1) When a method modifies its own value

```siskin
struct Counter:
    n: Int
    fn bump(self):
        self.n += 1

fn main():
    var c = Counter(n: 0)
    c.bump()
    c.bump()
    print(str(c.n) + "\n")
```

- `siskin run` → `2`
- run after `siskin build` → `0`
- `siskin check` → passes

Repro file: `realworld/selftest.skn`

### (2) When a list is passed to a function that adds an item

```siskin
fn add_item(xs: [Str]):
    xs.push("sneakily added")

fn main():
    let names = ["Haru"]
    add_item(names)
    print(str(names.len()) + "\n")
```

- `siskin run` → `2`
- run after `siskin build` → `1`

Repro file: `realworld/aliastest.skn`

### Why these are the same problem

Both happen because it was never decided **"when you pass a value to a function,
is it copied, or does it refer to the same thing?"** The interpreter chose "shared"
and the C compiler chose "copied", each on its own.
This is not a bug to fix but **a question the design has to settle first**.
Until it is settled, you cannot trust any program to behave the same in both modes.

For reference, `box[0] += 1` (modifying an element in place) behaves as shared in both modes.
Only `push` differs. That means there isn't one rule, but several.

---

## Tier 2 — Things that work with `run` but don't compile

### (3) Recursive enums — you can't represent trees or expressions

```siskin
enum Tree:
    Leaf(v: Int)
    Node(l: Tree, r: Tree)
```

`siskin check` passes, `siskin run` works, `siskin build` **fails**. Repro: `realworld/tree.skn`

This is why the calculator (`calc.skn`) doesn't compile. Holding expressions or trees is
the most typical use of `enum`, so with this blocked you can't build parsers,
interpreters, calculators, JSON processors and similar programs natively.

On top of that, the error looks like this:

```
tb.c:1586:33: error: field 'v_l' has incomplete type
```

It shows a line number in a `tb.c` file the author has never seen. There's no way to tell what it means.
When the generated C code is wrong, it should be turned into a Siskin-level error.

### (4) `inout` parameters

- Compiler: honestly says "not supported yet, use `siskin run`". Good.
- Interpreter: works for structs, but **silently does nothing for Int, Float, Bool and Str.**

```siskin
fn bump(inout n: Int):
    n += 1

fn main():
    var x = 0
    bump(x)
    print(str(x) + "\n")   # should print 1, but prints 0
```

Raising an error is better than silently letting something that doesn't work slip through.

### (5) `?T` fields in structs

`struct Task: due: ?Str` — compilation honestly rejects it. But structs where something
"may not have a due date", like a to-do list, are extremely common, so this is high priority.

---

## Tier 3 — Things that silently give wrong answers

### (6) `f"{x:.2f}"` ignores the format spec

```siskin
let x = 3.14159
print(f"{x:.2f}\n")     # expected 3.14 → prints 3.14159
print(f"[{x:>10}]\n")   # alignment is ignored too
```

It passes `siskin check` and runs. It just silently ignores the spec.
A beginner will assume they wrote it wrong and waste a long time.
The right thing is to implement formatting or, until then, **raise an error.**

Because of this I had to hand-roll two-decimal output in `loganalyze.skn`.
`0.1 + 0.2` printing as `0.30000000000000004` is the same problem.
Any program that prints numbers runs into this.

### (7) `check` and `build` pass without the `import`

```siskin
fn main():
    print(str(sqrt(2.0)) + "\n")   # std.math was not imported
```

- `siskin check` → passes
- `siskin build` → compiles, and running it prints the correct answer
- `siskin run` → "function `sqrt` not found"

The docs say "you must list every name you use", so `check` and `build` should catch this.
Right now it first blows up the moment you run something with `siskin run` that you had been compiling.

---

## Tier 4 — Missing things that make real programs impossible

### (8) There is no input at all

Keyboard input (`input`), command-line arguments (`args`) and exit codes (`exit`) are all missing.
So **you can't write a single command-line tool.** I couldn't use the to-do program
as `todo add "groceries"`, so I had to switch to writing everything into a file
beforehand and reading it all at once. The file path has to be hard-coded too.

I think this is the first gap to fill. It's only three functions, and without them
you can't hand a program you wrote to anyone else.

### (9) You can't split code into files

You can import the standard library, like `std.math`, but **you can't import your own files.**
Neither `import mylib` nor `from mylib import hello` works.
One program is always exactly one file. It breaks down as soon as things get a bit bigger.

### (10) You can't pass functions as values

There are no nested functions, no function types and no anonymous functions. So:

- You can't pass a sort key. In `loganalyze.skn` **I wrote the sort by hand**
  (15 lines). Something as common as ordering by request count has to be hand-written every time.
- There is no `map` / `filter`.
- You can't write a function that takes a callback.

### (11) There are no generics

```siskin
fn first[T](xs: [T]) -> ?T:     # type `T` not found
```

A function like "first item of a list" has to be copied for every type.

### (12) There are no tuples

You can't write `return (node, pos)`. In the calculator, the parser had to return both
"the node it built" and "the next position", so I made a struct (`Parsed`) just for that.
Needing to return two values comes up very often.

### (13) You can't name the type of a JSON value

```siskin
fn read_task(item: Json) -> !Task:    # type `json` not found
```

A `Json` type exists internally but can't be used. **It looks like the case-folding rule
lowers `Json` to `json` and then fails to find it** (the error message prints
lowercase `json`). This is a case where case folding actually blocks a perfectly good name.

Result: reading JSON couldn't be split out into functions, so `todo.skn` had to cram
everything into one function. Combined with (14) below, this is the worst of it.

---

## Tier 5 — Things that exist but are painful to use

### (14) Unwrapping "may have no value" (`?T`) is far too cumbersome

None of these three work:

```siskin
# ① Guard first, then use — not narrowed
if v == none:
    return error("missing")
print(str(v + 1))          # still rejected as ?Int

# ② Check on the same line — not narrowed
if v != none and v > 0:    # cannot compare ?Int with Int

# ③ Default if missing — no such syntax
let v = find(xs) else 0
```

The only thing that works is using it **inside** an `if v != none:` block.
To extract four values you have to stack four levels of `if`.

The JSON-reading part of `todo.skn` actually ended up **nested 16 levels deep (64 columns of indentation)**.
That's the code for reading a single struct with five fields.
This is where it departs most from "syntax as easy as Python".

Having any one of the three fixes would help a lot. Personally I think
① (narrowing after a guard) is the most valuable. It flattens the shape of the code.

### (15) Iterating over a dictionary is awkward

```siskin
for k, v in d:      # doesn't exist
```

You have to loop over `keys()` and fetch again with `d[k]`, and **even though you know the key
exists, you still have to check for `none`.** The key came from `keys()`, so it can't be missing.

### (16) "Insert if missing, add if present" takes four lines every time

Counting shows up in almost every program. Right now you write it like this:

```siskin
let cur = by_tag[g]
if cur != none:
    by_tag[g] = cur + 1
else:
    by_tag[g] = 1
```

### (17) There is no padding or decimal formatting

To print a table I hand-wrote three functions, `padr`, `padl` and `fixed2` (`loganalyze.skn`).
There's `"-".repeat(10)` but no `pad_right`.

Also: padding by **character count** doesn't line up Korean text on screen.
Each Korean character takes two columns on screen. The table header in `loganalyze.skn`
really was misaligned. A separate function that measures display width is needed.

### (18) You can't put `"` inside a raw string

A log line looks like `"GET /api HTTP/1.1"`, so the regex needed a `"` in it, but
neither `r"...\"..."` nor `'...'` works (there are no single-quoted strings).
In the end I concatenated it like this:

```siskin
let Q = "\""
let pat = Q + r"(\w+) (\S+) HTTP/1\.1" + Q + r" (\d+) (\d+)"
```

Log parsing is the most common use of regular expressions, and this is exactly where it gets stuck.

### (19) A no-argument enum variant is written differently in three places

```siskin
enum Tok:
    LPar            # declared without parentheses
...
    out.push(LPar())   # parentheses required when constructing
...
    case LPar:         # no parentheses again in match
```

I tried to construct it as `LPar`, got only "`LPar` not found", and spent a long time lost.
The error message should just say "write `LPar()`".

### (20) A method can modify a value bound with `let`

```siskin
let c = Counter(n: 0)
c.bump()              # should be blocked, but it just works
```

The docs describe `fn bump(self)` as "read-only", but it isn't actually enforced.
`let` is supposed to mean "can't change", yet it changes through a method. Same root as the tier-1 problem.

---

## What works well — please keep these as they are

Things that turned out better than I expected while using it.

- **Error message quality.** Errors point at the line and column, explain in plain language, and a `help:` line tells you how to fix it.
  For nearly every type error I knew right away what to do.
- **Runtime safety checks.** Division by zero and list/string out-of-range are caught on the spot,
  and the messages are identical between `siskin run` and compiled programs.
- **`!T` with `try` / `catch`.** I wrote the calculator's error handling with only these, and it was very clean.
  You can see which lines can fail. This design is a success.
- **`match` exhaustiveness checking and `case _`.** It tells you exactly which variant you missed.
- **Contracts (`requires`) and doctests.** Both work in both modes, and
  contracts disappearing under `--release` is exactly as designed.
- **Regex, dictionaries, JSON.** Regex handles Korean text fine, and JSON files containing Korean
  are preserved exactly when read and written back.
- **Counting strings by character.** `"안녕하세요".len()` is 5. A clear advantage over C++.
- **Mutually recursive functions.** In the parser, a function can call one declared later. No forward declarations needed.
- **Expressions spanning multiple lines.** When printing tables, continuing lines with `+` read well.
- `while` `break` `continue` `elif`, nesting lists, dictionaries and structs, and `print`ing structs.

---

## The order I would suggest

**First:** (1)(2) Decide whether passing a value to a function copies or shares it. This is a design
decision, so it comes before everything else. Adding features while it's undecided only multiplies the inconsistencies.

**Next (small, high-impact):**
- (6) If formatting isn't supported, at least raise an error — prevent silently wrong output
- (7) Have `check` verify imports
- (8) The three functions `input` / `args` / `exit` — with these, what you build becomes usable
- (17)(18) `pad_right` / `pad_left`, and a way to put `"` in a regex

**Then (nicer syntax):**
- (14) Narrow `?T` after a guard — flattens code
- (15)(16) `for k, v in d`, dictionary counting
- (12) Tuples
- (13) Fix the `Json` type name

**Then (major work):**
- (3) Compiling recursive enums — without it, programs like parsers stay `run`-only
- (9) Splitting into files
- (10) Functions as values
- (4)(5) `inout`, `?T` in structs
- (11) Generics
