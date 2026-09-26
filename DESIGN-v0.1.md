# Siskin Language Design Document v0.1

> Name **Siskin** (the Eurasian siskin, a small finch) · extension `.skn` · written 2026-09-19, name finalized 2026-09-24
> It is named after a small, quick bird. At first it was provisionally called Mica, but a scripting language by that name already existed, so on 2026-09-24 we settled on Siskin.

---

## 0. One-line definition

**Siskin is a statically typed systems language that reads like Python, runs as fast as C, lets you manage memory directly like C++, and has a compiler that talks to machines as well as to people.**

---

## 1. Requirements → design goals

| User requirement | Translated into design | Key mechanism |
|---|---|---|
| As easy as Python | Minimal boilerplate, indented blocks, local type inference, no lifetime annotations | Syntax §4, Types §5 |
| Memory access as free as C++ | Raw pointers, pointer arithmetic, manual allocation/deallocation and the C ABI as first-class features | Three memory levels §6 |
| As fast as C | No GC, AOT native compilation, monomorphization, value semantics, zero-cost abstractions | Performance §7 |
| AI-friendly | Unambiguous syntax, structured diagnostics, a structural editing API, built-in contracts/tests | AI-friendliness §9 |
| Human-friendly too | Readability first, one way to do it, no hidden control flow | Principles §3 |

### 1.1 The biggest tension

"Easy like Python" and "fast like C + free like C++" do not naturally go together. Existing languages made these choices:

- Python: gains ease, gives up speed and memory control
- C/C++: gain speed and control, give up safety and ease
- Rust: gains speed, control and safety, gives up **ease** (the cognitive load of the borrow checker)
- Go: gains some ease and speed, gives up **GC pauses and memory control**

**Siskin's answer: instead of picking one, split it into levels.** The base level is as easy as Python, and where you need it, a single word opens a lower level with the freedom of C++. The whole design comes down to this: stepping down a level always leaves a **visible** mark in the code.

---

## 2. What we took from earlier languages, and what we left behind

| Language | Take | Leave |
|---|---|---|
| **Python** | Indentation syntax, readability, a feel for standard library design | Dynamic typing, the GIL, interpreter speed |
| **Mojo** | The strategy of layering systems features on Python syntax, `owned`/`inout` argument conventions, `comptime` | The constraint of being a CPython superset (inheriting the entire legacy) |
| **Rust** | Ownership/move concepts, errors as values, Cargo-grade tooling | Explicit lifetime annotations, an exposed borrow checker, syntactic complexity |
| **Hylo** | Mutable value semantics — safety without lifetime annotations | A level of generality that is still at the research stage |
| **Vale** | Generational references for cheap detection of use-after-free | Experimental runtime |
| **Koka / Lobster** | Perceus-style compile-time elimination and reuse of reference counts | Pure functional constraints |
| **Zig** | Passing allocators explicitly, `comptime`, no hidden control flow | Manual memory as the only option |
| **Go** | A feel for structured concurrency, fast compilation, a single formatter | GC, implicit interface implementation |

References are collected in Appendix B (all primary sources in English).

---

## 3. Five design principles

1. **The reader comes first.** Code is read far more often than it is written. We choose clarity over brevity.
2. **Safety is the default; freedom is one explicit word.** Dangerous things are not forbidden, but they always leave a marker such as `unsafe`.
3. **One way to do it.** We do not keep two syntaxes that do the same thing. For people, that means fewer arguments; for AI, fewer choices and therefore higher accuracy.
4. **Local reasoning.** You should be able to read a single function and know everything it does. That is why there are no exceptions, implicit conversions, code-generating macros or arbitrary operator overloading.
5. **The compiler is an API.** Diagnostics, formatting, editing and explanations all come out in machine-readable form. Tooling is not bolted on later; it is part of the language.

---

## 4. Syntax

### 4.1 Block delimiting — indentation (decided)

We adopt Python-style indentation. Braces are not accepted.

- **Rationale**: "As easy as Python" is requirement number one, and with indentation the visual structure always matches the logical structure.
- **Known risk**: LLMs easily break indentation when they partially edit code.
- **Mitigation**: the canonical formatter `siskin fmt` always restores it, and with the structural editing API (§9.3) AI edits in units of AST nodes rather than text. In addition, when the parser hits an indentation error, it offers a fix-it stating exactly "how many spaces it should be".
- Rules: exactly 4 spaces. No tabs. Blocks are opened with `:`.

### 4.2 Basic examples

```siskin
# hello.skn
fn main():
    print("Hello, Siskin!\n")
```

`print` lives in the prelude (`std.prelude`), so it can be used without an import. The prelude is kept narrow: roughly `print`, `len`, `range`, `str`, `int`, `float`, `error` and `assert`. Everything else requires an explicit import (§9.7).

**`print` outputs exactly what it is given and appends nothing.** As in C/C++, you write the newline yourself as `\n` inside the string.

```siskin
print("Hello\n")         # newline
print("Hello")           # no newline
print("a", "b\n")        # "ab" followed by a newline (no separator is added either)
```

We do not have both `print` and `println` because of principle 3 (one way to do it), and we do not append a newline automatically because of principle 4 (nothing hidden). The price is that the most common one-liner is three characters longer than in Python. This is an intentional trade-off.

Escapes are the same as in the C family: `\n` newline, `\t` tab, `\\` backslash, `\"` double quote. It is a backslash (`\`), not a slash (`/`).

```siskin
fn sum(xs: [Int]) -> Int:
    var total = 0
    for x in xs:
        total += x
    return total
```

- `let` = immutable binding (use this by default), `var` = mutable binding
- Type annotations are **required** in function signatures; local variables are **inferred**
  - Rationale: the function boundary is where the contract is visible. We want both people and AI to only have to look there. Inside, code stays as light as Python.

### 4.3 Structs and interfaces

```siskin
interface Printable:
    fn to_str(self) -> Str

struct Vec2(Printable):
    x: Float
    y: Float

    fn len(self) -> Float:
        return sqrt(self.x * self.x + self.y * self.y)

    fn to_str(self) -> Str:
        return f"({self.x}, {self.y})"
```

- There is no inheritance. There is only interface implementation and composition.
- Interface implementation is **explicit** (`struct Vec2(Printable)`). Go-style implicit implementation breaks local reasoning, because answering "why does this satisfy that interface?" requires a global search.

### 4.4 Optionals and errors — no null, no exceptions

```siskin
fn find(users: [User], id: Int) -> ?User:     # ?T = the value may be absent
    for u in users:
        if u.id == id:
            return u
    return none

fn read_config(path: Str) -> !Config:         # !T = may fail
    let text = try fs.read_text(path)         # try = propagate immediately on failure
    return Config.parse(text)

# call site
let cfg = read_config("app.toml") catch e:
    print(f"Failed to load config: {e}\n")
    return
```

- **No null.** The possibility of a missing value shows up in the type `?T`, and the compiler forces you to handle it.
- **No exceptions.** Errors are return values and appear in the signature as `!T`. We do not create invisible control flow.
- `try` propagates, `catch` handles. Just those two.

### 4.5 Pattern matching

```siskin
enum Shape:
    Circle(r: Float)
    Rect(w: Float, h: Float)

fn area(s: Shape) -> Float:
    match s:
        case Circle(r):
            return 3.14159 * r * r
        case Rect(w, h):
            return w * h
```

`match` is **checked for exhaustiveness**. A missing case is a compile error, and the diagnostic lists the missing cases.

### 4.6 Generics — square brackets

```siskin
fn max[T: Ord](a: T, b: T) -> T:
    return a if a > b else b

struct Stack[T]:
    items: [T]
```

We use `[]` instead of `<>`. This avoids `<` clashing with the comparison operator and making parsing context-dependent (the classic C++/Rust problem). Keeping the grammar close to LL(1) directly improves the accuracy of AI-generated code.

### 4.7 Keywords (34)

```
fn let var if elif else for while in break continue return
struct enum interface match case
import from as pub
try catch none true false
and or not
owned inout unsafe with comptime
```

`requires`, `ensures`, `self` and `arena` are contextual keywords, so they are not in this list. Fewer keywords means less for people to memorize and less for AI to confuse.

### 4.8 Relaxed case sensitivity

The C family treats `myValue` and `myvalue` as completely different names. For people this is
a memory burden and a common source of mistakes. Siskin relaxes it with a single rule.

> **If a name is spelled exactly, it wins. Only when there is none, and exactly one name
> differs only in case, is the reference attached to it. If there are two or more candidates, nothing is corrected.**

- `struct User` and `let user` are both exact names, so they do not interfere with each other.
  Under full case insensitivity (old-style BASIC, SQL), this common combination would break.
- The `Str` type and the `str()` function coexist safely for the same reason.
- `siskin check` reports each corrected spot along with the canonical spelling, and `siskin fmt` converges the file
  to the canonical spelling. So search tools and AI only ever see one spelling.
- Names imported through the C FFI are the exception. C is case-sensitive, so they must be exact.

In short, **it is lenient with people, and stored code converges to one spelling.**
This is where principle 1 (the reader comes first) is honored without violating principle 3 (one way to do it).

---

## 5. Type system

- **Static types, monomorphization.** No runtime type lookups, no virtual calls (only when `dyn` is used explicitly).
- **Value semantics by default.** Structs are values. Copying and moving are the default, and references appear only through argument conventions.
- **Three argument conventions** (borrowed from Hylo/Mojo, with no lifetime annotations):

| Notation | Meaning | C++ equivalent |
|---|---|---|
| `fn f(x: T)` | Read-only borrow (default) | `const T&` |
| `fn f(inout x: T)` | Mutable borrow | `T&` |
| `fn f(owned x: T)` | Ownership transfer | `T&&` |

Unlike Rust, we **do not write** lifetimes like `'a`. A single rule — a reference cannot outlive the call — covers most cases, and cases that need to go beyond that rule drop down to the lower levels of §6. This is the key trade-off that preserves "ease".

- **No implicit conversions.** Even `Int` → `Float` is written explicitly.
- **Compile-time execution** with `comptime`: used for constant evaluation, generic specialization and table generation.

---

## 6. Memory model — three levels (the core of this language)

```
┌─ Level 0 : safe     The default. Plain code with no annotation lives here.
│                     Value semantics + moves + compile-time reference count elimination.
│                     Written like Python, but with no GC.
│
├─ Level 1 : region   `with arena` blocks. Direct allocation, freed all at once at block end.
│                     For game loops, per-request servers, real-time processing.
│
└─ Level 2 : unsafe   `unsafe` blocks. Raw pointers, pointer arithmetic,
                      reinterpret casts, malloc/free, the C ABI. The same freedom as C++.
```

### 6.1 Level 0 — safe (default)

```siskin
fn main():
    let names = ["Haru", "Siskin"]      # heap-allocated, freed automatically
    for n in names:
        print(f"{n}\n")              # reference count increments/decrements are removed at compile time
```

- It uses reference counting, but applies **Perceus-style compile-time elimination and reuse**. Wherever ownership is tracked statically, no RC operations remain in the code at all, and an allocation of the same size right after a free reuses the memory.
- Reference cycles are a known weakness of RC. `Weak[T]` is provided, and the compiler warns about self-referential types that could form cycles.
- No GC → no pauses, deterministic deallocation.

### 6.2 Level 1 — region / arena

```siskin
fn render_frame(world: World):
    with arena frame:                       # per-frame arena
        let batch = frame.list[Sprite]()    # list backed by the arena
        for e in world.entities:
            batch.push(e.sprite())
        gpu.submit(batch)
    # Everything is freed at once as soon as the block is left. No individual frees.
```

An arena provides two things.

- `a.list[T]()` — a growable list. You use it exactly like an ordinary list; only the time of deallocation differs. No `unsafe` is needed.
- `a.alloc[T](n)` — raw memory with n slots. Because it is a raw pointer, it must be inside `unsafe:`, and you do not call `free`.

Rules:

- Values that come from an arena cannot leave the arena block. The compiler prevents it (`T0040`).
- Leaving the block with `return` or `break` still always frees it.
- The cost of an allocation drops to a single pointer increment. Measurements show it is 4.1x faster than individual allocation/deallocation and as fast as a hand-written C arena ([bench/](bench/README.md)).
- **Implementation status:** ✅ works. Zig-style explicit allocator passing (`fn parse(a: Allocator, ...)`) is not done yet.

### 6.3 Level 2 — unsafe

```siskin
unsafe:
    let p: *Int = alloc[Int](16)            # raw pointer
    p[0] = 42
    let q = p + 8                           # pointer arithmetic
    free(p)
```

- Pointer arithmetic, manual lifetime management and direct control over memory layout are available.
- **Where you did it always stays visible in the code.** Touching a raw pointer outside `unsafe:` is a compile error (`T0041`).
- **Implementation status:** `alloc` / `free` / indexing / pointer arithmetic ✅ work.
  `extern "C"` C FFI ✅ works (§9.5).
  Reinterpret casts `cast[*Byte](p)`, `siskin check --unsafe-report`
  and `unsafe = "deny"` in the manifest are not done yet.

### 6.4 What sets it apart — raw pointers are checked too in debug builds

**✅ Implemented.** In debug builds (the default), every Level 2 allocation gets a **generation number (generational reference)**, and every access checks the generation and the bounds. What gets caught:

| Mistake | Diagnostic |
|---|---|
| Access after free | `access to freed memory (use-after-free)` |
| Out-of-bounds access | `pointer access out of bounds (offset N, size M bytes)` |
| Double free | `double free: this memory was already freed` |
| Access outside the arena block | caught as use-after-free |

With `--release`, the generations and checks all disappear and pointers become plain C pointers. **Zero overhead**, and arena speed matches hand-written C.

**The cost:** in debug builds, freed memory is held on to instead of actually being returned to the OS. The check works by reading a "this memory is dead" marker, and reading memory that has been returned would itself be a dangerous access. So debug builds use more memory. Release builds return it normally.

In short, you get a safety net close to Rust's during development and code identical to C's when you ship. The design takes Vale's generational reference idea, narrows it to "debug only", and so brings it in with no performance cost.

---

## 7. Performance — how we reach C-class speed

Goal: **1.0–1.1x of C** (same algorithm, measured with microbenchmarks).

| Mechanism | Effect |
|---|---|
| AOT native compilation (LLVM backend) | No interpreter, no JIT warm-up |
| No GC | No pauses, predictable latency |
| Static types + monomorphization | No dynamic dispatch, boxing or type lookups |
| Value semantics | Stack/inline layout by default; heap allocation only when explicit |
| Perceus-style RC elimination + reuse | Most reference counting cost in the safe level vanishes at compile time |
| Zero-cost abstractions | Interfaces, generics and iterators all disappear after inlining |
| Direct use of the C ABI | Zero FFI overhead; existing C libraries usable immediately |
| `comptime` | Moves runtime computation to compile time |
| Built-in SIMD types (`Simd[Float, 8]`) | Vectorization expressed at the language level |

**Honest limits:** in Level 0, patterns where ownership is not tracked statically (shared graphs, the observer pattern and so on) leave some reference counting operations behind, and are slower than C by that much. Dropping such hot paths down to Level 1 (arenas) or Level 2 (raw pointers) makes them identical to C. **"Easy and fast enough by default; the last 10% comes from stepping down a level"** — that is the deal this language offers.

Bounds checking is on by default, and can be turned off in verified hot loops with `unsafe` indexing.

---

## 8. Concurrency

- **Structured concurrency**: tasks cannot outlive their block.

```siskin
with nursery n:
    n.spawn(fetch("a.com"))
    n.spawn(fetch("b.com"))
# Both tasks are guaranteed to finish when the block is left. No leaked background tasks.
```

- **No function coloring problem**: we do not split functions in two with `async`/`await`. On top of lightweight threads, every function is simply written as if it were blocking (the Go approach).
- **Data races are blocked by the type system**: the only things that can be passed between threads are values whose ownership has been transferred, or `Shared[T]` (internally an atomic RC + lock).

**Implementation status (2026-09-24):** the first version took a simpler route.
- `spawn f(x)` runs a single call on a new OS thread and gives you a `Task[T]`. You get the result with `t.wait()`.
  These are OS threads rather than lightweight threads, and instead of `with nursery`, **all remaining tasks are awaited when main finishes.**
- Values passed to a task are **copies** (the same rule as closure capture). A task cannot change outer variables,
  and raw pointers, arenas and `Json` cannot be passed. So there is no shared memory, and `Shared[T]` is not needed yet.
- Tasks communicate through `channel[T]()` / `channel[T](n)` channels (`send`, `recv() -> ?T`, `close`, `for x in ch`).
- If every task is waiting on another, a deadlock (E0260) runtime error is reported. Both backends follow the same rule.
- Both backends are truly parallel. `siskin build` uses pthreads; `siskin run` uses one OS thread and one interpreter per task (no global lock). Interpreter values are single-threaded (`Rc`), but values passed to tasks, results and channel values are rebuilt wholesale by the sending side (`Value::detach`) before being handed over, so two threads never touch the same value. The syntax tree is shared read-only via `Arc`.
  The results are the same; only the speed differs. While waiting (on channels, `sleep`, `input`), the lock is released.

---

## 9. AI-friendly design — what exactly it does

We define "AI-friendly" not as a vague slogan but as eight verifiable features.

### 9.1 Unambiguous syntax
A grammar close to LL(1), `[]` generics, no user-defined operators, no code-generating macros, 34 keywords. → Structurally lowers the rate at which generated code fails to parse.

### 9.2 Machine-readable diagnostics
```
$ siskin check --json
{"code":"E0142","severity":"error","file":"main.skn","span":[12,5,12,18],
 "message":"cannot use `?User` directly as `User`",
 "fixes":[{"label":"add none handling","edit":"if let u = find(...):"}],
 "explain":"siskin explain E0142"}
```
Every error comes with a **stable code** and **applicable fixes**. AI agents no longer need to interpret error messages as natural language.

### 9.3 Structural editing API
```
$ siskin edit --at "app::server::handle_request" --replace-body body.skn
$ siskin symbols --json          # symbol tree for the whole project
```
AI edits code **by AST node**, not by text diff. Accidents such as breaking indentation or overwriting the wrong line are eliminated at the root. (This is the direct answer to the indentation risk in §4.1.)

### 9.4 Canonical formatter
`siskin fmt` has no options. All code converges to a single shape, so diffs are stable, style arguments disappear, and AI does not have to guess the style.

### 9.5 Built-in contracts
```siskin
fn div(a: Int, b: Int) -> Int:
    requires b != 0
    ensures result * b <= a
    return a / b
```
Checked in debug builds, removed in release. AI can leave its "intent" in the code, which makes a loop of verifying its own generated code possible.

### 9.6 Built-in doctests
```siskin
fn slug(s: Str) -> Str:
    """
    >>> slug("Hello World")
    "hello-world"
    """
```
`siskin test` runs the examples in the documentation as-is. Documentation, tests and specification live in one place.

### 9.7 Nothing hidden
No exceptions, no implicit conversions, no implicit globals, no wildcard imports (only the `from std.fs import read_text` form). Reading a function tells you everything it does. → The context window AI needs gets smaller, and so does the range a person has to read when reviewing.

### 9.8 Edition-based stability
Language changes happen only in editions, and `siskin migrate` converts code automatically. This reduces AI going astray because of outdated syntax in its training data.

---

## 9.5 Library ecosystem — the biggest weakness of a new language

A new language has nothing that others have already built. This is the most common reason new languages die.
Siskin's answer is **to use the entire C world as-is from day one**.

Siskin is translated to C and then compiled. So it calls C libraries directly
**with no translation layer**. The call cost is zero.

```siskin
extern "C" fn sqlite3_open(path: *Byte, db: **Byte) -> Int
```

What this means in practice: most useful Python libraries are
really **written in C, with Python providing only a thin wrapper**. The number crunching in numpy,
the image processing in pillow, the cryptography in cryptography and the parsing in lxml are all C.
Siskin uses that core directly, without the wrapper. Porting Python code would be going
in the opposite direction — it would mean porting the slow wrapper.

So the priorities are:

1. **C FFI** (`extern "C"`) — ✅ done. We called zlib's `crc32` and confirmed it gives
   the same value as Python's `zlib.crc32`. With just this, decades of assets such as SQLite, OpenSSL, SDL, ffmpeg
   and BLAS become available.
   - Types that can be passed: `Int` (including pointer handles), `Float`, `Bool`, `Str` (`const char*`), `*T`, no return value.
   - A single line, `extern "C" link "z"`, also takes care of linking.
   - Limits: it cannot be called from the interpreter (`siskin run`), because real linking is required.
     Structs, callbacks and variadic arguments are not done yet.
2. **A narrow, good standard library** — ✅ done. Math, random numbers, time, files, lists, strings,
   **regular expressions** (`std.re`) and **JSON** (`std.json`). The regex engine and JSON reading/writing were written
   from scratch with no outside libraries, and the same algorithms exist in both the interpreter and the C backend, so
   the results are identical. This is the v0.1 standard library, and it will not grow further.
3. **Package manager** — later. Early on, the standard library and the C FFI carry the load.

To be honest, a mature ecosystem takes years. The C FFI is a device that helps us survive
those years; it does not replace an ecosystem.

---

## 10. Non-goals (things v0 does not do)

- Exception handling, class inheritance, user-defined operators, code-generating macros
- Garbage collection
- Python source compatibility (we borrow only the syntactic family and do not attempt runtime compatibility, avoiding the cost Mojo pays)
- An effect system — attractive, but it conflicts with v0's "ease". To be considered for v2.

---

## 11. Implementation roadmap

| Phase | Content | Deliverables |
|---|---|---|
| **P0** | Specification v0.1 (this document) + EBNF grammar definition | `DESIGN-v0.1.md`, `grammar.ebnf` |
| **P1** | Tree-walking interpreter — validate the syntax by actually typing it out | ✅ Done. 4,100 lines of Rust. See `micai/` |
| **P2** | Static type checker + `?T`/`!T`/exhaustive pattern matching checks | ✅ Done. `siskin check` |
| **P3** | Native backend (start via C → LLVM) | ✅ Done. `siskin build`, 1.0–1.2x of C, all examples pass |
| **P4** | Memory Levels 1/2, C FFI, generation-checking debug mode | ✅ Done. Arenas, raw pointers, generation checks and C FFI all work |
| **P5** | Tooling: `fmt`, `--json` diagnostics, `edit` API, LSP | AI-friendly features in real use |
| **P6** | Standard library + benchmarks (measured against C) | ✅ Done. Math, random, time, files, lists, strings, dictionaries, regex, JSON, [bench/](bench/README.md) |

**P1 matters most.** A syntax can look good on paper, but once you actually write about 200 lines, awkward spots always turn up. The right order is to build an interpreter quickly, use it yourself and fix the syntax.

---

## 12. Open decisions

1. **Implementation language** — what to write the P1 interpreter in. (Recommendation: Rust. It can be reused as-is for the P3 backend and tooling later, and it has good Cranelift/LLVM bindings. Alternative: prototype quickly in Python and throw it away.)
2. **Language name** — ✅ finalized on 2026-09-24 as `Siskin` (extension `.skn`, command `siskin`). The earlier provisional name was Mica (`.mi`).
3. **Handling reference cycles** — whether manual management with `Weak[T]` is enough, or whether to offer a cycle detector as an option. (Recommendation: only `Weak[T]` for v0.)

---

## Appendix A. Comprehensive sample

```siskin
from std.fs import read_text

struct Token:
    kind: Str
    text: Str

fn tokenize(src: Str) -> [Token]:
    """
    >>> tokenize("a b")
    [Token("word", "a"), Token("word", "b")]
    """
    var out: [Token] = []
    for word in src.split(" "):
        if word != "":
            out.push(Token(kind: "word", text: word))
    return out

fn count_words(path: Str) -> !Int:
    let src = try read_text(path)
    return tokenize(src).len()

fn main():
    let n = count_words("input.txt") catch e:
        print(f"Read failed: {e}\n")
        return
    print(f"{n} words\n")
```

A performance version of the same program — handling allocation all at once with an arena:

```siskin
fn count_words_fast(path: Str) -> !Int:
    with arena a:
        let src = try read_text_into(a, path)
        var n = 0
        for word in src.split(" "):
            if word != "":
                n += 1
        return n
    # All memory used for parsing is freed here, all at once
```

---

## Appendix B. References (primary sources in English)

- Perceus: Garbage Free Reference Counting with Reuse (PLDI 2021) — https://xnning.github.io/papers/perceus.pdf
- Optimizing Reference Counting with Borrowing (Lorenzen) — https://antonlorenzen.de/papers/master_thesis_perceus_borrowing.pdf
- Borrow checking, RC, GC, and the Eleven Other Memory Safety Approaches (Vale) — https://verdagon.dev/grimoire/grimoire
- Hylo — mutable value semantics — https://github.com/hylo-lang/hylo-lang.github.io/blob/main/index.md
- Move semantics in Rust, C++, and Hylo — https://lukas-prokop.at/articles/2024-11-29-move-semantics-in-rust-cpp-and-hylo
- Ruminating about mutable value semantics — https://www.scattered-thoughts.net/writing/ruminating-about-mutable-value-semantics/
- Mojo (programming language) — https://en.wikipedia.org/wiki/Mojo_(programming_language)
