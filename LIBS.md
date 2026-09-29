# Using libraries

Siskin can use existing C and C++ libraries as they are.
There's no need to transcribe functions by hand one at a time. Just name the header
file (the library's description) and Siskin reads in the functions it declares.

```
import c "zlib.h" link "z"
```

That one line opens up all 80 zlib functions.

---

## 1. C libraries

```
import c "header_name.h" link "library_name"
```

- `header_name.h` — the same name you would use with `#include` in C.
- `link "name"` — the library to link against. It's the name without the leading `lib`
  and the trailing `.so`, like the `z` in `-lz`. Omit it when not needed (e.g. `string.h`).

### Example

```
import c "zlib.h" link "z"
import c "string.h"

fn main():
    let s = "hello"
    print(str(crc32(0, s, strlen(s))) + "\n")
```

### Seeing what was opened

```
siskin ffi zlib.h          # how many functions are usable
siskin ffi zlib.h --all    # every function name and signature
```

Here are actual measurements:

| Library | Functions usable out of the box |
|---|---|
| zlib (compression) | 80 of 81 (98%) |
| sqlite3 (database) | 283 of 291 (97%) |
| libpng (images) | 246 of 246 (100%) |
| curses (terminal screen) | 444 of 456 (97%) |
| expat (XML) | 66 of 67 (98%) |
| bzip2 | 24 of 24 (100%) |

---

## 2. How C types map to Siskin types

| C side | Siskin side | Notes |
|---|---|---|
| `int`, `long`, `size_t`, `uint32_t` … | `Int` | All integers are `Int`, regardless of size |
| `float`, `double` | `Float` | |
| `_Bool` | `Bool` | |
| `const char *` | `Str` | A string the callee reads |
| `char *` (parameter position) | `Int` | Means "write into here", so it's kept as an address |
| `char *` (return position) | `Str` | |
| Other pointers (`FILE*`, `sqlite3*` …) | `Int` | A handle. Passed back and forth without looking inside |
| `T **` | `inout Int` | A slot meaning "put the result here" |
| Function pointer | function | You can pass your own function |
| `void` | nothing | |

### Slots that receive a result (`T **`)

C libraries often hand back results through a parameter rather than the return value.

```c
int sqlite3_open(const char *filename, sqlite3 **ppDb);
```

In Siskin, just pass a `var` variable and the result is stored in it.

```
var db = 0
if sqlite3_open(":memory:", db) != 0:
    print("failed to open\n")
```

### Passing your own function to a library (callbacks)

This is when a library says "I'll call your function every time something happens".
Write a named function and pass its name as is.

```
fn each_row(ctx: Int, ncols: Int, values: Int, names: Int) -> Int:
    ...
    return 0

sqlite3_exec(db, "select * from people", each_row, 0, err)
```

Anonymous functions that capture outer variables can't be passed. C has no such concept.

### Reading values from an address C gave you

Things like the `values` a callback receives are addresses handed over by C. Read them inside `unsafe:`.

```
unsafe:
    let text = cstr(addr)            # reads a NUL-terminated C string
    let third = ptr_get(addr, 2)     # the value in slot 2 of what the address points to
```

---

## 3. C++ libraries

```
import cpp "header_name.hpp" link "library_name"
```

C++ can't be called directly: names are mangled when stored (`geo::add` is really `_ZN3geo3addEii`),
and it has things C doesn't, like classes, virtual functions and templates.
So Siskin automatically writes a bridge file in between and hands it to the C++ compiler
along with everything else. The C++ compiler then takes care of mangled names and virtual functions itself.

### Naming rules

| C++ | Siskin |
|---|---|
| `geo::add(int,int)` | `geo_add(a, b)` |
| constructing a `geo::Circle` | `geo_Circle_new(...)` → handle |
| destroying a `geo::Circle` | `geo_Circle_delete(handle)` |
| `circle.area()` | `geo_Circle_area(handle)` |
| `std::string` | `Str` |
| class references and pointers | `Int` (handle) |

### Example

```
import cpp "shapes.hpp" from "cpplib" also "cpplib/shapes.cpp"

fn main():
    let circle = geo_Circle_new(2.0)
    print(str(geo_Circle_area(circle)) + "\n")
    print(geo_Circle_kind(circle) + "\n")      # a function returning std::string
    geo_Circle_delete(circle)
```

With `also "shapes.cpp"` you don't need to build the library in advance;
that source is compiled along with your program. If you already have a built library,
use `link "name"` instead.

### What works with C++

- Constructing and destroying classes, calling methods
- **Virtual functions** — ask through the base type and the derived class answers
- **Header-only functions** (inline) — the bridge file instantiates them
- **Templates** — instantiate them with concrete types in the bridge
- Passing `std::string` back and forth
- Namespaces

### What doesn't work with C++ yet

- **Importing templates wholesale** — a template is instantiated anew for each use,
  so it can't all be imported in advance. Only instantiations with fixed types are imported.
- **Functions with the same name but different parameters (overloads)** — only the first is imported.
- **Exceptions** — if one is thrown, the program simply terminates.
- Passing containers like `std::vector` by value — passing them as handles works.

---

## 4. `siskin run` and `siskin build`

Programs that use libraries also run as is with `siskin run`.
Under the hood it quietly compiles and runs the program, so the result is always
the same as with `siskin build`.

---

## 5. What isn't imported automatically yet (C)

| Reason | Examples |
|---|---|
| Functions with a variable number of arguments | `printf`, `sqlite3_config` |
| Functions that pass or return structs by value | `div()`, some of the `localtime` family |
| Names Siskin already has | `abs`, `free`, `pow`, `exit` … the Siskin one wins |

Run `siskin ffi <header>` to see what was left out and why.

---

## 6. Good to know

- **Reading headers requires `clang`.** If it's missing, `apt install clang`
  (on macOS, `xcode-select --install`). Compilation itself uses `cc`.
- The result of reading a header is cached, so from the second time on there's no wait.
  If the header file changes, it's read again.
- A handle (`Int`) is just a number. Siskin does not protect what it points to.
  Anything the C library says to "close when done" must be closed
  (`sqlite3_close`, `geo_Circle_delete` …).
- Strings returned by C functions are copied by Siskin right away. It's fine if the original
  goes away, but anything documented as "you must free this" will leak memory if left alone.
