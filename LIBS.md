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
| `int`, `long`, `size_t`, `uint32_t` … | `Int` | All integer values are `Int`, regardless of size |
| `float`, `double` | `Float` | |
| `_Bool` | `Bool` | |
| `const char *` | `Str` | A string the callee reads |
| `int *`, `float *`, `sqlite3 **` … | `CPtr[I32]`, `CPtr[F32]`, `CPtr[Int]` | "Write the result here": pass a `var` |
| `const T *` | `CConst[T]` | "Read this": pass the value itself |
| `char *` (parameter position) | `CPtr[I8]` | A buffer to write into, so it stays an address |
| `char *` (return position) | `Str` | |
| `void *`, `FILE *`, other handles | `CPtr[Unit]` | Pass a handle (`Int`) or a `*T` pointer |
| A struct by value | the struct | `div_t`, `VkExtent2D` … |
| Function pointer parameter | function | Pass your own named function |
| Function pointer return value | `Int` | An address; see "Function pointers" below |
| `void` | nothing | |

`siskin ffi header.h --all` shows every signature in these terms.

### What to pass for a C pointer

A pointer parameter takes one of three things:

- **A variable**: C gets its address. If the pointer is not `const`, what C writes lands in the variable,
  so it must be a `var`.
- **A `*T` pointer** (from `alloc`) or an **address** (`Int`) you got from C: passed as it is.
  A number that is not a variable, such as `0`, is an address, so `0` is a null pointer.
- **A value** for a `const` pointer (`CConst[T]`): C reads a copy of it.

```c
int sqlite3_open(const char *filename, sqlite3 **ppDb);
void glfwGetFramebufferSize(GLFWwindow* window, int* width, int* height);
VkResult vkCreateInstance(const VkInstanceCreateInfo* info, const VkAllocationCallbacks* alloc, VkInstance* out);
```

```
var db = 0
sqlite3_open(":memory:", db)        # the handle lands in db

var w = 0
var h = 0
glfwGetFramebufferSize(win, w, h)   # C writes both sizes

var instance = 0
vkCreateInstance(info, 0, instance) # info is read, 0 is a null pointer, instance is written
```

### Constants

`#define` numbers and strings, `enum` values and `static const` values become constants with the same names:

```
import c "GLFW/glfw3.h" link "glfw"

if key == GLFW_KEY_ESCAPE and action == GLFW_PRESS:
    ...
```

A `#define` whose value is an expression of other constants (`#define VK_API_VERSION_1_0 VK_MAKE_API_VERSION(0, 1, 0, 0)`)
is worked out by the C compiler. Function-like macros themselves are not imported.

`static const` values are read from their initialiser, the way Vulkan 1.3 declares its 64-bit flags:

```
import c "vulkan/vulkan.h" link "vulkan"

let stages = VK_PIPELINE_STAGE_2_COLOR_ATTACHMENT_OUTPUT_BIT | VK_PIPELINE_STAGE_2_COPY_BIT
```

All 64 bits are kept; a value with the top bit set reads as a negative Int, as `0x8000000000000000` does.

### Structs and unions

Structs and unions in the header become Siskin structs. They keep C's field names and
the generated code uses the header's own type, so the memory layout is exactly C's.

```
var props = VkPhysicalDeviceProperties()          # every field starts as zero
vkGetPhysicalDeviceProperties(device, props)      # C fills it in
print(f"{props.deviceName} {props.limits.maxImageDimension2D}\n")

var clear = VkClearValue()                        # a union: all views share the same bytes
clear.color.float32 = [0.1, 0.2, 0.3, 1.0]
```

| C field | Siskin field | Reads as |
|---|---|---|
| `uint32_t`, `int16_t`, `float` … | `U32`, `I16`, `F32` … | `Int` / `Float` |
| `float color[4]` | `[F32; 4]` | `[Float]` |
| `char name[256]` | `Str` | `Str` (up to the first NUL; storing truncates) |
| another struct | that struct | the struct |
| `enum` | `I32` | `Int` |
| a pointer | `Int` | an address; also takes a `*T` pointer |
| `const char *` | `Int` | an address (read it with `cstr`); also takes a string, which stays alive |
| a function pointer | `Int` | an address; also takes a named function |

Fields not given when constructing start as zero, like `= {0}` in C.
`size_of[T]()` and `offset_of[T]("field")` give C's `sizeof` and `offsetof`.
The same fixed-size types can be used in your own structs (see GUIDE 9.5.5).

### Function pointers

A function pointer that C returns is an address (`Int`). The header's function pointer type
turns it into a Siskin function you can call:

```
let addr = vkGetInstanceProcAddr(instance, "vkDestroyInstance")
let destroy = PFN_vkDestroyInstance(addr)
destroy(instance, 0)

let previous = glfwSetKeyCallback(win, on_key)    # the callback that was set before (0 if none)
if previous != 0:
    GLFWkeyfun(previous)(win, key, scancode, action, mods)
```

If such a function takes a pointer, it takes it as an address: `address_of(p)` gives the address of a `*T` pointer.

### Headers that include other headers, and macros they expect

Headers included with quotes (`#include "vulkan_core.h"`) are read too, so `vulkan/vulkan.h`
imports everything in `vulkan_core.h`. System headers included with `<...>` are not, so
`stdio.h` functions still need their own `import c "stdio.h"`.

Some headers only declare things when a macro is defined first. Put it on the import line:

```
import c "GL/glext.h" link "GL" define "GL_GLEXT_PROTOTYPES"
import c "mylib.h" define "MYLIB_STATIC", "MYLIB_LEVEL=2"
```

The macros also apply to C files compiled with `also`.

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

To read and write a whole C buffer, turn the address into a `*T` pointer with `cast[*T](addr)`.
This is what `vkMapMemory` and similar "here is my memory" functions need:

```
var data = 0
vkMapMemory(device, memory, 0, size, 0, data)   # C writes the buffer's address into data
unsafe:
    let v = cast[*F32](data)
    v[0] = 0.5                                   # writes straight into the mapped memory
    v[1] = -0.5
vkUnmapMemory(device, memory)
```

`cast[*U8](p)` reinterprets a pointer as another element type, and `cast[Int](p)` gives
the address back (the same as `address_of(p)`). Siskin cannot check bounds on memory it did
not allocate, so stay inside the buffer the library gave you.

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
| Function-like macros | `VK_MAKE_API_VERSION(...)`, `MIN(a, b)` |
| Names Siskin already has | `abs`, `free`, `pow`, `exit` … the Siskin one wins |

Run `siskin ffi <header>` to see what was left out and why.

What Siskin does not try to replace: shaders stay in GLSL (Siskin passes them to OpenGL or Vulkan
like any other file), and windows, physics and audio come from existing libraries such as GLFW, SDL,
Jolt or miniaudio.

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
