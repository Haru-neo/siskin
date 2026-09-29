# Reading Siskin — A Guide for People Who Know a Little C++

The goal of this document is to let you read the example code line by line.
Siskin is shown on one side and the equivalent C++ on the other.

Compiler error messages are in English by default. To see them in Korean, set `SISKIN_LANG=ko` (see 12.0).
The error samples in this guide use the default English output.

---

## 1. The smallest program

```siskin
fn main():
    print("Hello\n")
```

```cpp
int main() {
    std::cout << "Hello\n";
}
```

| Siskin | C++ | Meaning |
|---|---|---|
| `fn` | where the return type (`int`, `void`) goes | Marks "a function starts here" |
| `:` and indentation | `{ }` | Start and end of the function body |
| `print(...)` | `std::cout << ...` | Print to the screen |
| `\n` | `\n` | Newline. Same as C++ |

**There are no curly braces.** Instead, a `:` at the end of a line "opens" a block, the indented lines are the "body",
and the block "closes" when the indentation ends. Four spaces take the place of C++'s `{` `}`.

---

## 2. Functions

```siskin
fn sum(xs: [Int]) -> Int:
    var total = 0
    for x in xs:
        total += x
    return total
```

```cpp
int sum(const std::vector<int>& xs) {
    int total = 0;
    for (int x : xs) {
        total += x;
    }
    return total;
}
```

| Siskin | C++ | Meaning |
|---|---|---|
| `xs: [Int]` | `const std::vector<int>& xs` | Name first, type after |
| `[Int]` | `std::vector<int>` | Square brackets mean "a list of" |
| `-> Int` | the `int` before the function name | Return type, written after an arrow |
| `var total = 0` | `int total = 0;` | The compiler figures out the type |
| `for x in xs:` | `for (int x : xs)` | Same as a range-based for |
| no semicolons | `;` required | A line break ends the statement |

### `let` and `var`

```siskin
let a = 10      # cannot change
var b = 10      # can change
b = 20          # OK
a = 20          # error
```

```cpp
const int a = 10;   // cannot change
int b = 10;         // can change
```

In C++ you have to **add** `const` to make something unchangeable;
in Siskin it's the other way around: you add `var` to make it changeable. The default is the safe choice.

A `let` outside any function (at the top of the file) is a **constant visible throughout the file**.
Any function can read it, and nothing can change it. There is no `var` outside functions
(keep changing values inside `main` and pass them to functions, so things stay safe even when several tasks run at once).

```siskin
let TAX = 0.1
let UNITS = ["kg", "g"]

fn with_tax(price: Float) -> Float:
    return price * (1.0 + TAX)
```

---

## 2.5 Basic types, and why `fn` is not a type

`fn` only **marks a function declaration**; it is not a type.
Saying earlier that it sits where C++'s `int` goes was about position only, and that could be misleading.

Variable types are a separate thing.

| Siskin | C++ | Meaning |
|---|---|---|
| `Int` | `long long` | Integer. 64-bit |
| `Float` | `double` | Floating point. 64-bit |
| `Bool` | `bool` | True/false |
| `Str` | `std::string` | String |
| `[T]` | `std::vector<T>` | List |
| `{K: V}` | `std::map<K,V>` | Dictionary |
| `Byte` | `unsigned char` | Byte. Only for working with memory directly |

**There is no `char`.** A single character is just a `Str`.
`"안녕"[0]` gives `"안"`, a `Str` of length 1.
C++'s `char` is one byte, so it cannot even hold a single Korean character;
Siskin left it out so as not to inherit that problem.

### You usually don't write types on variables

```siskin
let n = 10           # inferred as Int
let pi = 3.14        # Float
let name = "Haru"    # Str
let ok = true        # Bool
```

```cpp
int n = 10;
double pi = 3.14;
std::string name = "Haru";
bool ok = true;
```

If you want to, put a `:` after the name and write the type. It is optional.

```siskin
let n: Int = 10
```

### The only place types are required is the function signature

```siskin
fn add(a: Int, b: Int) -> Int:
    let result = a + b      # no type needed here
    return result
```

Inside a function you write lightly, like Python; you only write types at the function's entrance and exit.
That's because the entrance and exit are all that others (and AI) need to look at when using the function.

---

## 2.7 Getting letter case wrong is OK

C++ treats `myValue` and `myvalue` as completely different things. Siskin relaxes this.

There is one rule. **If a name is written exactly right, that one wins. Only when there is none,
and there is exactly one name that differs only in letter case, does it attach to that name.**

```siskin
struct UserAccount:
    displayName: Str

fn main():
    let account = useraccount(displayName: "Haru")   # attaches to UserAccount
    print(f"{ACCOUNT.displayname}\n")               # attaches to account.displayName
```

`siskin check` tells you what it fixed.

```
note: 5:19 `useraccount` -> `UserAccount` (letter case corrected)
```

If two names that differ only in case really both exist, like `struct User` and `let user`,
both are exact names, so neither is touched. If you then write `USER`, there are
two candidates, so nothing is fixed and you get the usual error. When in doubt, it doesn't fix anything.

---

## 3. Putting values inside strings

```siskin
print(f"Total: {sum(xs)}\n")
```

```cpp
std::cout << "Total: " << sum(xs) << "\n";
```

The `f` before the quote marks "the `{ }` inside this string are places to insert values".
It is easier to read than chaining pieces together with C++'s `<<`.

### Digits and width

Inside `{ }`, put a `:` after the value to control its format. Same as Python.

```siskin
print(f"{3.14159:.2f}\n")     # 3.14      two decimal places
print(f"[{"abc":<6}]\n")      # [abc   ]  left-aligned, width 6
print(f"[{42:>5}]\n")         # [   42]  right-aligned
print(f"[{7:03d}]\n")         # [007]     pad with zeros
print(f"{255:x}\n")           # ff        hexadecimal
```

| Format | Meaning |
|---|---|
| `.2f` | Two decimal places |
| `<6` `>6` `^6` | Width 6, aligned left, right, or center |
| `05d` | Width 5, pad with 0 |
| `x` `X` | Hexadecimal |

The width here is a **number of characters** (same as Python). Korean (and other wide) characters take two columns on screen,
so to line up a table containing them, use `s.pad_right(10)` / `s.pad_left(10)`, which count screen columns.

There is no rounding function that takes a number of digits, like `round(x, 1)`. Use `{x:.1f}` when displaying.

---

## 3.5 A few common expressions

```siskin
let label = "even" if n % 2 == 0 else "odd"    # pick a value by condition (C++'s ? :)
var k = 10
k += 1        # -= *= /= %= also exist
if [1, 2] == [1, 2]:                           # lists, structs, enums, tuples and dicts compare with == too
    pass                                       # a do-nothing placeholder (same as Python)
```

| Siskin | C++ | Meaning |
|---|---|---|
| `a if cond else b` | `cond ? a : b` | a if the condition is true, otherwise b |
| `pass` | `;` (empty statement) | When a block has nothing to do |
| `and` `or` `not` | `&&` `\|\|` `!` | Logical operators. Words, not symbols |
| `true` `false` `none` | `true` `false` `nullptr` | All lowercase |
| `x in xs` `x not in xs` | `std::find(...) != end` | In a list, a key in a dict, or a substring in a string |

If you forget the f, `"value is {x}"` prints literally. The compiler warns you about it (W0001).
Format widths must be written as numbers (`{s:<8}`). If the width is a variable, use `s.pad_right(w)`.

A function that returns a value must end in `return value` **on every path**.
If there is an `if` without an `else`, the compiler tells you (T0069).

---

## 4. Structs and methods

```siskin
struct Vec2:
    x: Float
    y: Float

    fn len(self) -> Float:
        return sqrt(self.x * self.x + self.y * self.y)
```

```cpp
struct Vec2 {
    double x;
    double y;

    double len() const {
        return std::sqrt(x * x + y * y);
    }
};
```

| Siskin | C++ | Meaning |
|---|---|---|
| `self` | `this` | The object itself. Unlike C++, it is written **explicitly** as a parameter |
| `self.x` | `x` or `this->x` | Always write `self.`, so you can always see where a value comes from |
| `fn len(self)` | `double len() const` | Nothing before `self` means read-only |

You use it like this.

```siskin
let v = Vec2(x: 3.0, y: 4.0)
print(f"{v.len()}\n")
```

```cpp
Vec2 v{3.0, 4.0};
std::cout << v.len() << "\n";
```

**You write the field names and give the values,** as in `x:` `y:`. You don't need to remember the order,
and a reader immediately knows what 3.0 is. It's `x: 3.0`, not Python's `x=3.0`.
If you give them in field order, you can also omit the names: `Vec2(3.0, 4.0)`.

### Methods that change the value — `inout self`

With nothing before `self`, a method can only read. To change fields, write **`inout self`**.
(Not `mut self` or `var self`.)

```siskin
struct Account:
    owner: Str
    balance: Int

    fn deposit(inout self, amount: Int):
        self.balance += amount

fn main():
    var acc = Account(owner: "Haru", balance: 0)   # var, because we will change it
    acc.deposit(100)
    print(f"{acc.balance}\n")                      # 100
```

```cpp
struct Account {
    std::string owner;
    long long balance;
    void deposit(long long amount) { balance += amount; }   // a method without const
};
```

| Siskin | C++ | Meaning |
|---|---|---|
| `fn f(self)` | `void f() const` | Read only |
| `fn f(inout self)` | `void f()` | Can change fields |
| `fn f(inout n: Int)` | `void f(int& n)` | Changes the variable passed in. The caller's variable must be a `var` |

A struct inside a list can also be changed in place: `accounts[i].deposit(50)`.

### Values are copied

`let b = a` and passing to a function behave like **copies**. Changing `b` leaves `a` as it was.
It's the same as passing by value in C++, without a reference (`&`). So nothing gets changed behind your back.

On the other hand, if a recursive function keeps returning and receiving a big list, each step makes a copy, which can be slow.
In that case, pass the list that collects the results as `inout`.

```siskin
fn collect(t: Tree, inout out: [Int]):   # put results straight into out instead of returning them
    ...
```

---

## 5. `?T` — the value may be missing

C++ has many ways to say "not found": `nullptr`, `-1`,
`std::optional`, exceptions... so every time you look at a function you have to check which one it uses.

Siskin has only one. You put `?` before the type.

```siskin
fn find(users: [User], id: Int) -> ?User:
    for u in users:
        if u.id == id:
            return u
    return none
```

```cpp
std::optional<User> find(const std::vector<User>& users, int id) {
    for (const auto& u : users) {
        if (u.id == id) return u;
    }
    return std::nullopt;
}
```

- `?User` = "there may or may not be a User"
- `none` = C++'s `nullptr` / `std::nullopt`
- **There is no such thing as `null`.** The possibility of a missing value is written in the type,
  and the compiler forces you to check it. The null-pointer dereference accidents of C++ structurally cannot happen.

The receiving side uses one of two forms.

```siskin
let u = find(users, 7)
if u != none:
    print(u.name)          # in here, u is just a User
else:
    print("not found")

let name = env("USER") else "guest"     # use the value after else if missing (env is in std.process)
```

| Siskin | C++ | Meaning |
|---|---|---|
| `if x != none:` | `if (x.has_value())` | Inside, x is the value with the `?` removed |
| `x else default` | `x.value_or(default)` | The default if missing |
| `if x == none: return` | | After this line, x is the unwrapped value |

There are no names like `Some(x)`, `None` or `Option[T]`. A present value is just `return u`; a missing one is `return none`.

**Struct fields are unwrapped the same way.**

```siskin
struct Todo:
    title: Str
    due: ?Str

fn show(t: Todo):
    if t.due != none:
        print(t.title + " — due " + t.due + "\n")   # in here, t.due is just a Str
```

If you assign a new value to `t.due`, or pass `t` to a function that can change it (`inout`), it becomes `?Str` again from then on.

`?SomeStruct` and `!SomeStruct` also work as field types. A field can even refer to its own struct type,
so you can build linked lists and trees directly.

```siskin
struct Node:
    value: Int
    next: ?Node          # there may be no next node

let list = Node(value: 1, next: Node(value: 2, next: none))
```

---

## 6. `!T` — it may fail

```siskin
fn parse_age(text: Str) -> !Int:
    let n = try int(text)
    if n < 0:
        return error("age cannot be negative")
    return n
```

In C++, this is where you would throw an exception or return an error code.

```cpp
int parse_age(const std::string& text) {
    int n = std::stoi(text);        // throws on failure
    if (n < 0) throw std::runtime_error("age cannot be negative");
    return n;
}
```

- `!Int` = "gives an Int, or gives an error"
- `error("...")` = create an error and return it
- `try` = "if this fails, end my function right here as a failure too"

**There are no exceptions.** In C++ you can't tell from a function's signature whether it throws,
and an exception jumps upward past the caller. In Siskin the possibility of failure is always written with `!`,
and a failure is just a return value, so the control flow is visible.

The receiving side looks like this.

```siskin
let age = parse_age("34") catch e:
    print(f"failed: {e}\n")
    return
print(f"age {age}\n")
```

It serves the same purpose as C++'s `try { } catch { }`, but it attaches to that one line
instead of wrapping a block. You can see at a glance which call can fail.

If you want to continue with a **fallback value** instead of stopping on failure, write that value
on the last line of the `catch` block.

```siskin
let age = parse_age(text) catch e:
    print(f"invalid age, using 0: {e}\n")
    0
```

A `catch` block must do one of two things: leave with `return` / `continue` / `break`,
or put a fallback value on its last line. If it does neither, there's nothing to put in `age`,
so the compiler tells you (T0059).

The error value of `!T` is text (`Str`). The `e` in `catch e:` is the text you passed to `error("...")`.

### Error kinds as an enum — `E!T`

If you want to handle different kinds of failure differently, make the error kinds an enum
and write its name before the `!`. `BankError!Int` means "gives an Int, or on failure gives a BankError value".

```siskin
enum BankError:
    NoFunds(need: Int)
    BadAmount

struct Account:
    balance: Int

    fn withdraw(inout self, amount: Int) -> BankError!Unit:
        if amount <= 0:
            return error(BadAmount)
        if amount > self.balance:
            return error(NoFunds(need: amount - self.balance))
        self.balance -= amount
        return

fn main():
    var acc = Account(balance: 100)
    acc.withdraw(500) catch e:          # e is a BankError
        match e:
            case NoFunds(need):
                print(f"short by {need}\n")
            case BadAmount:
                print("invalid amount\n")
```

```cpp
enum class BankError { NoFunds, BadAmount };
std::expected<void, BankError> withdraw(long long amount);   // C++23
```

- As with any enum, the compiler tells you if `match e:` is missing a kind.
- `try` propagates errors of the same error type. If a function that calls `BankError!T` is a plain `!T` (text error),
  `try` converts the error to text like `NoFunds(430)` and propagates that. If the error types are different enums,
  receive it with `catch e:` and convert it with `return error(...)`.
- Only an enum can go in the error-type position. `!T` is the same as `Str!T`.

There is no `Result`, `Ok` or `Err`. If something can fail, put `!` before the type; a failure is `error(...)`; the receiver uses `try` or `catch`.

`main` can also be written as `fn main() -> !Unit:`. Then you can use `try` directly inside it, and
on failure it reports `error: <message>` (`오류: <message>` if Korean is selected) and exits with code 1.
Enum errors work too, as in `fn main() -> BankError!Unit:`.

---

## 7. `enum` and `match`

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

A C++ `enum` is just an integer, so it can't carry values along with it.
So you usually write this instead.

```cpp
struct Circle { double r; };
struct Rect   { double w, h; };
using Shape = std::variant<Circle, Rect>;

double area(const Shape& s) {
    if (auto* c = std::get_if<Circle>(&s)) return 3.14159 * c->r * c->r;
    if (auto* r = std::get_if<Rect>(&s))   return r->w * r->h;
    // still compiles if you forget one
}
```

A Siskin `enum` **gives each variant its own data.** And `match`
**checks that every variant is handled.** If you add `Tri` and don't write a `case` for it, you're told.

```
error[T0012]: non-exhaustive match: missing Tri
  help: add `case Tri(...):` or cover the rest with `case _:`
```

In `case Circle(r):`, `r` is the value extracted right there.
C++'s two steps, `std::get_if` + `c->r`, shrink to one line.

---

### Things to know when using `match`

```siskin
enum Tree:
    Leaf                                   # a variant with no fields
    Node(left: Tree, key: Int, right: Tree)   # it may contain itself

fn size(t: Tree) -> Int:
    match t:
        case Leaf:                          # no parentheses when there are no fields
            return 0
        case Node(l, k, r):
            return 1 + size(l) + size(r)

fn grade(score: Int) -> Str:
    match score:
        case 100:                          # numbers and strings can be matched too
            return "perfect"
        case _:                            # everything else
            return "other"
```

- Variants are written **without the enum name**: `case Leaf:` (not `case Tree.Leaf:`). Same when constructing: `Leaf`, `Node(left: ..., key: 3, right: ...)`.
- `case _:` is everything else. If it's there, missing variants aren't reported.
- When matching strings or numbers, the possibilities are endless, so `case _:` is required (T0068).
- `match` is a statement. It isn't used as an expression that returns a value. `return` from each arm or assign to a `var`.
- `?T` and `!T` are not unpacked with `match`. Use `if x != none:` and `catch` from sections 5 and 6.

---

## 7.3 Tuples — grouping a few values without names

```siskin
fn min_max(xs: [Int]) -> (Int, Int):
    var lo = xs[0]
    var hi = xs[0]
    for x in xs:
        lo = min(lo, x)
        hi = max(hi, x)
    return (lo, hi)

let (lo, hi) = min_max([3, 9, 1])     # destructure
let pair = min_max([3, 9, 1])
print(f"{pair.0} {pair.1}\n")         # access by position
```

`(Int, Str)` is C++'s `std::pair<int, std::string>` / `std::tuple`.
When a function returns two or three values, you don't need to make a separate struct.
A list of tuples can be destructured directly in a loop: `for (name, score) in pairs:`.
You can't change just one value inside a tuple. Assign the whole thing, as in `p = (new_value, p.1)`.

---

## 7.4 Generics — functions that take any type

```siskin
fn first[T](xs: [T]) -> ?T:
    if xs.len() == 0:
        return none
    return xs[0]

fn top_n[T](xs: [T], n: Int, key: (T) -> Int) -> [T]:
    var ys = xs
    ys.sort_by(fn(x): -key(x))        # here x is a T
    return ys.slice(0, n)
```

The `[T]` after the function name is C++'s `template <typename T>`. You don't write the type when calling:
`first([1, 2])`, `first(["a", "b"])`. The compiler generates a separate copy for each type, so it's as fast as C.

---

## 7.5 Passing functions as values, and closures

Functions are values too. You can store them in variables, pass them to other functions, and get them back.
A function's type is written `(what it takes) -> what it returns`.

```siskin
fn apply(f: (Int) -> Int, x: Int) -> Int:
    return f(x)

fn twice(x: Int) -> Int:
    return x * 2

fn main():
    print(f"{apply(twice, 21)}\n")          # 42
```

```cpp
int apply(std::function<int(int)> f, int x) { return f(x); }
```

### Anonymous functions — made on the spot, without a name

`fn(args): expression` is a function without a name. It's like the C++ lambda `[=](int x) { return x * k; }`.

```siskin
let k = 3
let times_k = fn(x: Int): x * k      # captured the outer k
print(f"{times_k(5)}\n")              # 15

let big = nums.filter(fn(x): x > 10)  # x's type can be omitted when the element type is known
```

| Siskin | C++ | Meaning |
|---|---|---|
| `fn(x: Int): x * k` | `[=](int x) { return x * k; }` | The body is a single expression after `:` |
| `fn(x): x * k` | (none) | Argument types can be omitted when the expected type is known |
| `fn(x: Int) -> Int: ...` | `[=](int x) -> int {...}` | The return type is usually omitted (inferred from the expression) |
| `(Int) -> Int` | `std::function<int(int)>` | Function type |

Where the argument type can't be omitted (when the expected type is unknown, as in `let f = fn(x): ...`),
the compiler tells you to write the type (T0054).

### When you need several lines — a function inside a function

An anonymous function is a single expression. If you need several lines, declare a `fn` inside the function.
This is also a closure that captures outer values, and it can call itself by its own name (recursion).

```siskin
fn main():
    let prefix = "value"
    fn describe(n: Int) -> Str:
        if n <= 0:
            return prefix
        return describe(n - 1) + "!"
    print(describe(3) + "\n")     # value!!!
```

### Captured values are "copies taken at creation" and are read-only

```siskin
var base = 1
let g = fn(x: Int): x + base
base = 100
print(f"{g(1)}\n")      # 2  <- base was 1 when g was created
```

In C++ terms it's always `[=]` (capture by value). It's the same idea as Siskin's rule that
"changing a copy leaves the original alone". So if you try to change a captured value inside a closure, compilation stops you (T0053).
If you need the changed value, make the function return the new value.

```siskin
var n = 0
fn inc():
    n += 1        # error[T0053]: `n` is captured from the enclosing scope and cannot be changed inside a closure
```

### Storing functions in struct fields

```siskin
struct Button:
    label: Str
    on_click: (Str) -> Str

    fn click(self) -> Str:
        return self.on_click(self.label)   # call the function stored in the field
```

Only when passing a callback to a C library must it be a named top-level function.
The C side has no place to pass the "captured values" along with it.

## 8. Contracts (`requires` / `ensures`)

```siskin
fn div(a: Int, b: Int) -> Int:
    requires b != 0
    ensures result * b <= a
    return a / b
```

```cpp
int div(int a, int b) {
    assert(b != 0);
    int result = a / b;
    assert(result * b <= a);
    return result;
}
```

- `requires` = what must be true on the way in
- `ensures` = what must be true on the way out (`result` is the return value)

It does the same job as C++'s `assert`, but because it sits **right below the function signature**,
anyone who wants to use the function sees the conditions without reading the body.
Like `assert`, it is checked only in debug builds and disappears in release builds.
`siskin run` and a plain `siskin build` check it; only `siskin build --release` leaves it out.

- You can write several `requires` / `ensures` lines. All of them must be true.
- They work on methods too. In the `ensures` of an `inout self` method, `self.field` is the value **after the change**.

```siskin
fn withdraw(inout self, amount: Int):
    requires amount > 0
    requires amount <= self.balance
    ensures self.balance >= 0
    self.balance -= amount
```

---

## 9. doctest

```siskin
fn slug(s: Str) -> Str:
    """
    >>> slug("Hello World")
    "hello-world"
    """
    return s.lower().replace(" ", "-")
```

If you write examples with `>>>` inside a `"""`-quoted description, `siskin test`
**actually runs** those examples and checks that the results match. The documentation and the tests live in the same place.

Expected values are written like Python: strings `"abc"`, numbers `3` `2.5`, `true` `false`, `none`,
lists `[1, 2]`, dicts `{"a": 1}`, tuples `(1, "a")`.

---

## 9.5 Memory — three levels

This is the part of C++ that takes the most care. Siskin divides it into three levels
and lets you go down only as far as you need.

### 9.5.1 Level 0 — just use it

```siskin
let names = ["Haru", "Mika"]
names.push("Ruby")
```

There is no `delete` and no `free`. The compiler decides when to release memory.
It feels the same as using a C++ `std::vector` by value.

### 9.5.2 Level 1 — arenas: throw everything away when the block ends

```siskin
with arena a:              # from here, memory is supplied by a
    var xs = a.list[Int]() # a list backed by a (var, since push changes it)
    xs.push(1)
    xs.push(2)
    print(str(xs) + "\n")
# here everything a allocated disappears at once
```

Line by line:

| Line | Meaning |
|---|---|
| `with arena a:` | Opens a memory store just for this block. Its name is `a` |
| `a.list[Int]()` | An Int list backed by that store. You use it exactly like an ordinary list |
| end of block | The whole store is thrown away. Nothing is freed one by one |

In C++ terms, it's like using a container with a custom `allocator` only inside a scope
and emptying the whole pool at the end of the scope. The difference is that Siskin
always empties it, even if you leave with `return`.

**Why it's fast:** an ordinary allocation means "find a free spot, mark it, and later find it again and give it back".
An arena just "pushes the end-of-store marker forward", and giving memory back happens only once,
at the end of the block. In our measurement it was 4.1 times faster.

**What's blocked:** if you try to send a value made in an arena out of the block, compilation stops you.
Once the block ends, that memory is gone.

```
error[T0040]: `xs` comes from an arena and cannot leave the block
  help: arena memory is freed at the end of the block; return a copy of the value you need
```

### 9.5.3 Level 2 — raw pointers: exactly like C

```siskin
unsafe:
    let p = alloc[Int](4)   # get memory for 4 slots
    p[0] = 10
    p[1] = 20
    let q = p + 1           # a pointer to the next slot
    print(str(q[0]) + "\n") # 20
    free(p)                 # give it back yourself
```

Side by side with C++:

| Siskin | C++ |
|---|---|
| `let p = alloc[Int](4)` | `long long* p = new long long[4];` |
| `p[0] = 10` | `p[0] = 10;` |
| `let q = p + 1` | `long long* q = p + 1;` |
| `free(p)` | `delete[] p;` |

This only works inside `unsafe:`. Using it outside is a compile error.
It's a marker in the code that says "I take responsibility here".

### 9.5.4 What happens when you make a mistake

In C++ these slip by silently and blow up somewhere unrelated much later.
Siskin's **debug build** (the default when you build without `--release`) catches them on the spot.

```siskin
unsafe:
    let p = alloc[Int](4)
    p[0] = 42
    free(p)
    print(str(p[0]) + "\n")   # reads memory that was already given back
```

```
runtime error: access to freed memory (use-after-free)
```

Out-of-bounds access, double free, and using a pointer outside its arena block are caught the same way.

**With `--release`** all these checks disappear. Pointers become plain C pointers and
the speed matches hand-written C. It catches things while you develop and gets out of the way when you ship.

**Just one caveat:** a debug build doesn't really give `free`d memory back; it holds on to it.
The checks only work if the "this memory is dead" mark can still be read.
So running in debug uses more memory. It's normal with `--release`.

### 9.5.5 Which level to use when

| Situation | Level |
|---|---|
| Most code | Level 0 |
| Many allocations inside work with a "clear end", like one frame or one request | Level 1 |
| When you must control memory layout yourself, or interface with a C library | Level 2 |

Start from the top and go down only where things are slow.

---

## 9.7 The standard library

You bring things in with `import`. It's similar to Python, but **there are no wildcard imports.**
You have to list every name you use, so you can always see where a name came from.

```
from std.math import sqrt, pi
```

Line by line:

| Line | Meaning |
|---|---|
| `from std.math import sqrt, pi` | Imports only `sqrt` and `pi` from `std.math` |
| `import std.math` | Imports the whole module. Use it as `math.sqrt(...)` (works for every standard module) |

### What's where

**Things you don't need to import** (always available)

`print` `eprint` `len` `range` `str` `int` `float` `abs` `min` `max` `sum` `assert` `error`
`args` `input` `exit`

| Name | What it does |
|---|---|
| `args()` | Command-line arguments `[Str]`. For `siskin run file.skn a b` or `./program a b` it's `["a", "b"]` (the program name is left out) |
| `input()` | Reads one line, `?Str`. `none` at end of input; an empty line is `""` |
| `exit(n)` | Exits immediately with exit code n |
| `eprint(...)` | Writes to error output (stderr). Used just like `print` |
| `int(text)` `float(text)` | Reads text as a number. Can fail, so `!Int` `!Float` |

Reading standard input line by line to the end:

```siskin
fn main():
    while true:
        let line = input()
        if line == none:
            break
        print(f"read: {line}\n")
```

**`std.math`**

| Name | What it does |
|---|---|
| `sqrt(x)` | Square root |
| `sin(x)` `cos(x)` `tan(x)` | Trigonometric functions (radians) |
| `log(x)` `log10(x)` `exp(x)` | Logarithms and exponential |
| `floor(x)` `ceil(x)` `round(x)` | Round down, up, or to nearest (returns an Int) |
| `pow(a, b)` | Power |
| `pi()` `e()` | Pi and Euler's number |

`pi` and `e` take parentheses. You write `pi()`.

Math functions **only take Float.** `sqrt(2)` is an error; you must write `sqrt(2.0)`.
That's because Siskin has no hidden type conversions. To pass an Int, use `sqrt(float(n))`.

**`std.random`**

| Name | What it does |
|---|---|
| `seed(n)` | Sets the seed. The same seed always produces the same values |
| `rand()` | A Float from 0 (inclusive) to 1 (exclusive) |
| `rand_int(a, b)` | An Int from a (inclusive) to b (exclusive) |

**`std.time`** — time and dates

| Name | What it does |
|---|---|
| `now()` | Seconds since 1970 (Float) |
| `clock()` | Seconds elapsed since the program started. Used for timing |
| `sleep(secs)` | Pauses briefly. Takes a Float, as in `sleep(0.5)` |
| `today()` | The current date and time in this computer's time zone (`DateTime`) |
| `date(year, month, day)` | Midnight of that day (`DateTime`) |
| `local_time(secs)` `utc_time(secs)` | Converts seconds like `now()` into a date |
| `parse_time(text, pattern)` | Reads a date with `"2026-03-01"` and `"%Y-%m-%d"` (`!DateTime`) |

`DateTime` has `year` `month` `day` `hour` `minute` `second` `weekday` (Monday=1),
and its methods are `format(pattern)` `date_str()` `time_str()` `to_str()` `weekday_name()`
`add_days(n)` `add_seconds(n)` `days_until(other)` `timestamp()`.

Pattern letters for `format`: `%Y`(2026) `%m`(09) `%d`(05) `%H` `%M` `%S` `%y`(26)
`%a`(Mon) `%A`(Monday) `%b`(Sep) `%K`(the Korean short weekday name, e.g. `월`) `%z`(+0900) `%%`(%)

```siskin
import std.time
let d = date(2026, 12, 25)
print(d.format("%A, %Y-%m-%d"))            # Friday, 2026-12-25
print(d.add_days(7).date_str())            # 2027-01-01
```

If you import the whole module with `import std.time`, you can use names directly like `today()` or as `time.today()`.

**`std.fs`** — everything here can fail, so it returns `!T`. Use it with `try`.

| Name | What it does |
|---|---|
| `read_text(path)` | The whole file as a string |
| `write_text(path, content)` | Writes a new file |
| `append_text(path, content)` | Appends to the end |
| `remove(path)` | Deletes |
| `exists(path)` | Only checks whether it exists (`Bool`) |
| `list_dir(dir)` | Names inside, sorted by name (`![Str]`) |
| `make_dir(dir)` | Creates it, including intermediate folders; does nothing if it already exists |
| `is_dir(path)` | Checks whether it's a folder (`Bool`) |

Error messages are the same whichever way you run the program: things like "no such file or directory", "permission denied",
"already exists", "not a directory".

**`std.process`** — running other programs, environment variables

| Name | What it does |
|---|---|
| `run(command)` | Runs one shell command line and waits for it to finish. `run("ls -l \| wc -l")` |
| `run_args(program, [args...])` | Runs without going through the shell |
| `env(name)` | Environment variable (`?Str`, `none` if unset) |
| `set_env(name, value)` | Sets an environment variable. Programs run afterwards see it too |
| `cwd()` `set_cwd(dir)` | Current folder / change the current folder |
| `pid()` | This program's process ID |

`run` and `run_args` give an `Output`: `code` (the exit code, 0 means success), `out` (standard output),
`err` (standard error), and `ok()`. If the program doesn't exist, `code` is 127 and `err` says why.

**Use `run_args` when you put text received from a user into a command.** With `run`, the shell interprets the text,
so if a file name arrives containing something like `; rm -rf ~`, it gets executed as is.
`run_args` passes the arguments as they are, without a shell.

**`std.net`** — the internet. Covered separately in [9.10](#910-the-internet--stdnet).

### Things you can do with lists

`push` `pop` `len` `reverse` `contains` `join` `sort` `index_of` `slice` `clear`

```
var xs = [5, 3, 9]         # var, to use methods that change it in place
xs.sort()                  # [3, 5, 9] — only for lists of Int, Float, Str, Bool. For other keys, sort_by
# Two keys (highest score first, then by name): sort_by keeps the order of equal values, so sort twice.
# people.sort_by(fn(p): p.name)
# people.sort_by(fn(p): -p.score)
xs.index_of(9)             # 2. -1 if not found
xs.slice(0, 2)             # [3, 5] — start included, end excluded
```

Methods that take a function (used with the anonymous functions from section 7.5):

```
xs.map(fn(x): x * 2)       # [6, 10, 18] — a new list with each element transformed
xs.filter(fn(x): x > 4)    # [5, 9] — only the ones that match
xs.any(fn(x): x > 8)       # true — does any match?
xs.all(fn(x): x > 0)       # true — do all match?
people.sort_by(fn(p): p.age)   # sort by a key (in place). Equal keys keep their original order
```

### Things you can do with strings

`len` `split` `upper` `lower` `strip` `replace` `contains` `starts_with` `ends_with`
`find` `repeat` `slice` `width` `pad_left` `pad_right`

To go through a string one character at a time, use `for c in s:`. `c` is a one-character `Str`, and Korean text also comes out one character at a time.
Characters can be compared by order, as in `c >= "a" and c <= "z"` or `c >= "가" and c <= "힣"` (the Hangul syllable range).

```
"안녕하세요".len()          # 5 — the number of characters, not bytes
"hello".find("ll")         # 2. -1 if not found
"-".repeat(10)             # "----------"
"안녕하세요".slice(0, 2)    # "안녕"
```

In C++, `std::string`'s `.size()` is a byte count, so it goes wrong once Korean (or any non-ASCII) text is mixed in.
Siskin counts characters.

---

### Splitting your code into files

You can import other `.skn` files in the same folder. The file name is the module name.

```siskin
# csvutil.skn
fn parse_line(line: Str) -> [Str]:
    return line.split(",")
```

```siskin
# main.skn
import csvutil

fn main():
    print(f"{csvutil.parse_line("a,b")}\n")
```

**Names from an imported file are written as `module.name`.** Each file has its own namespace,
so your file and an imported file (or two packages) can define the same name without clashing.
Files do the same job as C++'s `namespace`.

| Form | Meaning |
|---|---|
| `import csvutil` | `csvutil.parse_line(...)`; types are `csvutil.Row` |
| `from csvutil import parse_line` | Use it directly as `parse_line(...)` |
| `from csvutil import parse_line as parse` | Import it under a different name |
| `import pkg.tools as t` | Give the module a short name: `t.run(...)` |
| `fn _helper()` | Names starting with `_` are only usable inside that file (calling them from outside is an error) |

If you only `import csvutil` and write `parse_line(...)` without the module name, it works as long as the name exists in only one place,
but you get a warning (W0002) telling you to write `csvutil.parse_line`. If two modules have the same
name, it's an error (E0147) asking you to say which one. Where the type of the target already tells which one it is,
like `case Circle(r):` in a `match`, you don't need the module name.

If an imported file contains an error, you're told that file's name and line.
An imported file doesn't need a `main` (running `siskin check` on that file alone will report that main is missing).
See the examples `examples/16_modules.skn` and `examples/shapes.skn`.

---

## 9.8 Using C and C++ libraries

The biggest weakness of a new language is that nobody has built anything for it yet.
Siskin compiles by translating to C first, so **it calls the C and C++ libraries that already exist
directly.** There's no conversion layer in between, so calls cost nothing extra.

You don't need to copy functions over by hand one at a time either. Just name the header file (the library's description)
and Siskin reads in the functions it declares.

```
import c "zlib.h" link "z"

fn main():
    print(str(crc32(0, "hello siskin", 10)) + "\n")
```

Line by line:

| Line | Meaning |
|---|---|
| `import c "zlib.h"` | Reads zlib's header and imports all its functions |
| `link "z"` | Says to link zlib (`-lz`) as well |
| `crc32(0, "hello siskin", 10)` | Called like an ordinary function |

For C++ it's `import cpp "header.hpp"`. C++ can't be called as is, because names are mangled internally
and there are things like classes and virtual functions, so Siskin automatically writes a bridging
file in between and hands it to the C++ compiler along with everything else.

```
import cpp "shapes.hpp" from "cpplib" also "cpplib/shapes.cpp"

fn main():
    let circle = geo_Circle_new(2.0)
    print(str(geo_Circle_area(circle)) + "\n")
    geo_Circle_delete(circle)
```

### Seeing what was opened up

```
siskin ffi zlib.h          # how many functions are usable
siskin ffi zlib.h --all    # every function name and signature
```

For zlib, 80 of 81 functions are available right away; for sqlite3, 283 of 291; for libpng, all 246.

### Types that can be passed back and forth

| C side | Siskin side |
|---|---|
| `int`, `long`, `size_t` … all integers | `Int` |
| `float`, `double` | `Float` |
| `_Bool` | `Bool` |
| `const char *` | `Str` |
| other pointers (`FILE*`, `sqlite3*`) | `Int` — a handle |
| `T **` ("put the result here") | `inout Int` — just pass a `var` variable |
| function pointers (callbacks) | a function — pass one of your named functions |

```
var db = 0
sqlite3_open(":memory:", db)     # the result lands in db
```

### Two things to note

**`siskin run` works as well.** A program that uses a library is quietly compiled and then
run, so the result is always the same as what `siskin build` produces.

**Passing freed memory to C is caught.** In debug builds, it's checked once
right before crossing over to C.

```
runtime error: access to freed memory (use-after-free)
```

In C++ this mistake slips through and blows up much later.

### Writing declarations by hand (the old way)

When there's no header, or you only need one or two functions, you can write them yourself.

```
extern "C" link "z"
extern "C" fn crc32(crc: Int, buf: Str, len: Int) -> Int
```

In this case, though, Siskin doesn't know the real C types. If a C function returns a 32-bit `int`
and you write `Int`, the value may be corrupted. **Use `import c` when you can.**
Then the C compiler checks the types for you.

Details are in [LIBS.md](LIBS.md).

---

## 9.9 Dictionaries, regular expressions, JSON

### 9.9.1 Dictionaries — boxes with name tags

```
var ages = {"Haru": 20, "Mika": 3}
ages["Ruby"] = 7              # add a new entry
ages["Haru"] = 21             # change it if it already exists
```

This corresponds to C++'s `std::map` or `unordered_map`. There are two differences, though.

**It remembers insertion order.** `keys()` returns keys in the order they were inserted.
C++'s `map` uses sorted order, and `unordered_map` uses no particular order.

**Asking for a missing key gives `none`.**

```
let n = ages["nobody"]
if n != none:
    print(str(n))
```

C++'s `m["missing_key"]` silently creates and inserts a 0. That leads to "where did this entry come from?"
bugs. Siskin tells you when something isn't there.

Available: `len()` `set(key, value)` `has(key)` `keys()` `get(key, default)`

To loop over keys and values together, use `for k, v in ages:`.

To add one more item to a list inside a dictionary, take it out, change it, and put it back (because values are copied).

```siskin
var groups: {Str: [Str]} = {}
var names = groups["fruit"] else []
names.push("apple")
groups["fruit"] = names
```

### 9.9.2 Regular expressions — finding text by its shape

```
from std.re import test, find_all, groups, replace

test(r"\d+", "order of 42")          # true — is there a number?
find_all(r"\d+", "42, 17, 8")        # ["42", "17", "8"]
replace(r"\d", "010-1234", "*")      # "***-****"
```

**Watch out for `r"..."`.** Regular expressions contain lots of backslashes, and in an ordinary string
`\n` turns into a newline. Putting `r` in front reads the text exactly as written.
If you forget the `r`, the compiler tells you like this.

```
error[E0008]: unknown escape `\d`
  help: for a literal backslash write `\\d`, or use a raw string `r"..."`
```

Available:

| Name | What it does |
|---|---|
| `test(regex, text)` | Just whether there is a match (`Bool`) |
| `find(regex, text)` | The first matching part (`?Str`) |
| `find_all(regex, text)` | All matching parts (`[Str]`) |
| `groups(regex, text)` | The parenthesized groups of the first match. Group 0 is the whole match |
| `replace(regex, text, replacement)` | Replaces every match |
| `split_re(regex, text)` | Splits at each match |

Supported syntax: literal characters · `.` · `*` `+` `?` (add `?` after them for a lazy match) ·
`[a-z]` `[^...]` · `\d \w \s` and their uppercase negations · `^` `$` · `|` · `(...)` `(?:...)`

Korean works too. `[가-힣]+` (Hangul syllables) works as is.

### 9.9.3 JSON

```
from std.json import parse, stringify, jdict, jstr

fn main() -> !Unit:
    let doc = try parse(text)
    let name = doc.get("name")
    if name != none:
        let n = name.as_str()
        if n != none:
            print(n)
    return
```

Line by line:

| Line | Meaning |
|---|---|
| `try parse(text)` | Reads text as JSON. Fails if it's malformed |
| `doc.get("name")` | The value if that name exists, otherwise `none` |
| `name.as_str()` | The string if it's a string, otherwise `none` |

Checking for `none` twice may look tedious, but those are exactly the two places where accidents happen when handling JSON:
the name is missing, or it's there but not the kind you expected. You're made to ask about both.

To build JSON, do this.

```
let out = jdict()
out.set("lang", jstr("Siskin"))
out.set("version", jint(1))
print(stringify(out))        # {"lang":"Siskin","version":1}
```

| Building | |
|---|---|
| `jnull()` `jbool(b)` `jint(n)` `jfloat(f)` `jstr(s)` | A single value |
| `jlist()` `jdict()` | Empty array / empty object |

| Reading | |
|---|---|
| `kind()` | One of "null" "bool" "int" "float" "str" "list" "dict" |
| `as_int()` `as_float()` `as_str()` `as_bool()` | The value if it's that kind, otherwise `none` |
| `get(name)` `at(index)` | The value if present, otherwise `none` |
| `len()` `keys()` | Count and names |
| `set(name, value)` `push(value)` | Insert into objects and arrays |

The type name for JSON values is **`Json`**. Use it for function parameters, as in `fn f(item: Json)`.
`as_float()` also returns integers like `3` as `3.0`. Loop over an array with `for x in doc:` (if it isn't an array, the loop runs zero times).

A common pattern for turning JSON into a struct:

```siskin
from std.json import parse
from std.fs import read_text

struct Item:
    name: Str
    price: Float
    discount: ?Float          # may be missing or null

fn to_item(j: Json) -> !Item:
    let n = j.get("name") else j
    let name = n.as_str() else ""
    if name == "":
        return error("name is missing")
    let p = j.get("price") else j
    let price = p.as_float()
    if price == none:
        return error(f"{name}: price is not a number")
    var discount: ?Float = none
    let d = j.get("discount")
    if d != none:
        discount = d.as_float()           # none if null
    return Item(name: name, price: price, discount: discount)

fn main() -> !Unit:
    let doc = try parse(try read_text("inventory.json"))
    let items = doc.get("items") else doc
    for j in items:
        let it = try to_item(j)
        let off = it.discount else 0.0
        print(f"{it.name:<8} {it.price * (1.0 - off):>10.1f}\n")
    return
```

---

## 9.10 The Internet — `std.net`

```siskin
import std.net

let r = http_get("https://pypi.org/pypi/requests/json") catch e:
    print("could not reach the internet: " + e + "\n")
    return
print(f"{r.status} {r.ok()}\n")          # 200 true
print(r.body)                              # the content received (text)
```

**HTTP**

| Name | What it does |
|---|---|
| `http_get(url)` | Fetches (`!Response`) |
| `http_post(url, body)` | Sends. If the body starts with `{` or `[`, it's sent as JSON |
| `http_request(method, url, headers, body)` | Sends with everything specified. `http_request("PUT", url, {"Authorization": "Bearer ..."}, body)` |
| `url_encode(text)` | Converts text so it can go in a URL. `"김 밥"` → `"%EA%B9%80%20%EB%B0%A5"` |
| `set_timeout(secs)` | Maximum time to wait. 30 seconds initially |

`Response` has `status` (200, 404 ...), `headers` (a dict with lowercase names) and `body`,
plus `ok()` (is it 200–299?) and `header(name)` (case-insensitive, `?Str`).

**404 and 500 are not errors.** The server did answer, so they arrive as a `Response`.
What goes to `catch` is when the address can't be found, the connection fails, or it times out.
Redirect responses (301, 302 ...) are followed automatically.

`https` uses the OpenSSL installed on the computer (almost always present on Linux).
Certificates are verified, so a fake server stops with "untrusted certificate".
If `HTTPS_PROXY` is set, as on a corporate network, it is honored.

**TCP connections and servers**

| Name | What it does |
|---|---|
| `connect(host, port)` | Connects (`!Conn`) |
| `connect_tls(host, port)` | Secure connection (`!Conn`) |
| `listen(port)` | Opens a port and waits (`!Server`). `listen_on("127.0.0.1", port)` accepts only from this computer |

`Conn` has `send(text)` `recv()` `recv_line()` `peer()` `close()`,
and `Server` has `accept()` (waits until someone connects and gives a `Conn`) `port()` `close()`.
`recv()` and `recv_line()` return `""` when the other side closes the connection.

```siskin
let server = listen(8080) catch e:
    print(e + "\n")
    return
while true:
    let c = server.accept() catch e:
        continue
    let line = c.recv_line() catch e:
        ""
    c.send("received: " + line) catch e:
        print(e)
    c.close()
```

**Web servers and https servers**

`server.next_request()` accepts a client, reads one HTTP request, and gives it to you as a `Request`.
If you open with `listen_tls` instead of `listen`, it becomes an https server; the rest of the code is the same.

```siskin
import std.net

fn main():
    # open with https. For http, just change it to listen(8080).
    let server = listen_tls(8443, "cert.pem", "key.pem") catch e:
        print(e + "\n")
        return
    while true:
        let req = server.next_request() catch e:
            return                               # only happens when the server is closed
        if req.path == "/hello":
            let name = req.query["name"] else "guest"
            req.respond(200, f"Hello, {name}") catch e:
                print(e + "\n")
        else:
            req.respond(404, "<h1>page not found</h1>") catch e:
                print(e + "\n")
```

Go to `https://localhost:8443/hello?name=Haru` and you'll see `Hello, Haru`.

| Name | What it does |
|---|---|
| `listen_tls(port, cert, key)` | Opens an https server (`!Server`). Both files are in PEM format |
| `listen_tls_on("127.0.0.1", port, cert, key)` | An https server that accepts only from this computer |
| `server.next_request()` | Waits for the next request and gives a `Request`. Clients that send malformed requests are skipped automatically |
| `read_request(conn)` | Reads one request from a connection obtained with `accept()` |
| `url_decode(text)` | The reverse of `url_encode` |

`Request` has `method` ("GET" ...), `path` ("/hello", without the part after `?`), `query` (a dict), `headers` (a dict with lowercase names)
and `body`, plus `header(name)` `respond(status, body)` `respond_with(status, headers, body)`.
`respond` announces the body as JSON if it starts with `{`/`[`, as HTML if it starts with `<`, and as plain text otherwise, then closes the connection.

**Certificates.** An https server needs a certificate file and a private key file. For testing, make them like this.

```
openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 365 -subj /CN=localhost
```

A certificate made this way is self-signed, so browsers show a "Not secure" warning (fine for testing).
For a real server open to the internet, use the `fullchain.pem` and `privkey.pem` you get from a place like Let's Encrypt (free).
Clients that fail to establish a secure connection (a browser that doesn't trust the certificate, a client that came in over plain http) are skipped without stopping the server.

It serves one client at a time, so if you have many clients, handle each request separately with `spawn` (9.11).

**`siskin run` also runs it natively.** Networking has to talk to the operating system directly, so
programs that use `std.net` are quietly compiled and run, just like when using a C library.
That's why the first run takes about a second longer.

To fetch several URLs at once, send them concurrently with `spawn http_get(url)` (see 9.11 below).

---

## 9.11 Doing several things at once — `spawn` and `channel`

```siskin
fn count_primes(lo: Int, hi: Int) -> Int:
    ...

fn main():
    let a = spawn count_primes(0, 50000)       # runs in a new task and returns immediately
    let b = spawn count_primes(50000, 100000)
    print(f"{a.wait() + b.wait()}\n")           # waits until they finish and gets the results
```

```cpp
auto a = std::async(std::launch::async, count_primes, 0, 50000);
auto b = std::async(std::launch::async, count_primes, 50000, 100000);
std::cout << a.get() + b.get() << "\n";
```

| Siskin | C++ | Meaning |
|---|---|---|
| `spawn f(x)` | `std::async(f, x)` | A new task (thread). The result is a `Task[T]` |
| `t.wait()` | `future.get()` | Waits until it finishes and gets the result |
| `t.done()` | `wait_for(0s) == ready` | Only checks whether it has finished |
| `channel[Int]()` | thread-safe queue | A pipe for passing values between tasks (`Chan[Int]`) |
| `channel[Int](10)` | bounded queue | When 10 items are waiting, the sender waits |
| `ch.send(v)` / `ch.recv()` | push / pop | Receiving gives `?T`: `none` when closed and empty |
| `ch.close()` | | Announces that everything has been sent |
| `for x in ch:` | | Receives one at a time until it's closed |

```siskin
fn producer(ch: Chan[Int]):
    for i in range(5):
        ch.send(i * i)
    ch.close()                  # without this, the receiver's for loop never ends

fn main():
    let ch = channel[Int]()
    let p = spawn producer(ch)
    for v in ch:
        print(f"{v}\n")
    p.wait()
```

**Tasks don't touch the same memory.** Values passed to `spawn` are copies
(the same rule as closures), so two tasks can't change one list at the same time (a data race).
That's why you don't need locks (mutexes). Get results back with `wait()` or send them through a channel.
Raw pointers (`*T`), arenas, and `Json` values can't be passed to tasks (`stringify` a `Json` and pass it as text).

If every task is waiting on another (for example, nobody calls `close`, so the receiver waits forever),
the program doesn't just hang; it reports a "deadlock" runtime error.
When `main` ends, it waits for all still-running tasks before exiting. A runtime error inside a task stops the program.

**`siskin run` and `siskin build`:** the results are the same, and both really use multiple CPUs at once
(on 4 cores, about 3.5x for `siskin build` and about 3.2x for `siskin run`). Unlike Python, `siskin run` has no
"one task at a time" limit (GIL). That's possible because tasks don't share values; they pass copies.
See the example `examples/15_concurrency.skn`.


---

## 10. Symbols at a glance

| Symbol | Meaning | In C++ |
|---|---|---|
| `fn` | Marks a function declaration (not a type) | where `int` goes in `int main()` |
| `Int` `Float` `Str` `Bool` | Basic types | `long long` `double` `std::string` `bool` |
| `:` + indentation | Block | `{ }` |
| `->` | Return type | the type before the function name |
| `let` / `var` | Unchangeable / changeable | `const T` / `T` |
| `[Int]` | List of Int | `std::vector<int>` |
| `{Str: Int}` | Dictionary | `std::map<std::string,int>` |
| `?T` | A T that may be missing | `std::optional<T>` |
| `!T` | A T that may fail | exception or error code |
| `E!T` | A T that gives an enum E value on failure | `std::expected<T, E>` |
| `none` | No value | `nullptr` / `nullopt` |
| `try` | Propagate on failure | exception propagation |
| `catch e:` | Handle failure | `catch (...)` |
| `f"{x}"` | Insert a value into a string | chaining with `<<` |
| `self` | The object itself | `this` |
| `#` | Comment | `//` |
| `with arena a:` | A memory store for one block | a scope-bound memory pool |
| `unsafe:` | My responsibility from here | (no marker) |
| `*Int` | Raw pointer to an Int | `long long*` |
| `alloc[Int](4)` | Get 4 slots | `new long long[4]` |
| `free(p)` | Give back | `delete[] p` |
| `from std.x import y` | Import one name | `#include` + `using` |
| `extern "C" fn ...` | Declares a C function to use | a function declaration in a header |
| `extern "C" link "z"` | Link a library | `-lz` |
| `{"a": 1}` | Dictionary | `std::map<std::string,int>` |
| `r"\d+"` | Raw string | `R"(\d+)"` |
| `(Int) -> Int` | Function type | `std::function<int(int)>` |
| `fn(x: Int): x * k` | Anonymous function (closure) | `[=](int x) { return x * k; }` |
| `inout self` | Method that changes fields | a method without `const` |
| `inout n: Int` | Changes the variable passed in | `int& n` |
| `(Int, Str)` / `p.0` | Tuple / first value | `std::pair` / `p.first` |
| `fn f[T](x: T)` | Generic function | `template <typename T>` |
| `x else default` | The default if missing | `x.value_or(default)` |
| `a if cond else b` | Choose by condition | `cond ? a : b` |
| `case _:` | Everything else | `default:` |
| `pass` | Empty statement | `;` |
| `f"{x:.2f}"` | Two decimal places | `std::format("{:.2f}", x)` |
| `args()` | Command-line arguments | `argv` |
| `import a` / `a.f()` | Use a name from another file | `namespace a` / `a::f()` |
| `import a as b` | Another name for a module | `namespace b = a;` |
| `spawn f(x)` | Run concurrently | `std::async(f, x)` |
| `channel[Int]()` | Pipe between tasks | thread-safe queue |

---

## 11. Order for reading the examples

1. `examples/01_hello.skn` — output only
2. `examples/02_basics.skn` — functions, loops, lists
3. `examples/03_types.skn` — structs, enums, match
4. `examples/04_errors.skn` — `?T`, `!T`
5. `examples/05_contracts.skn` — contracts, doctest
6. `examples/06_memory.skn` — the three memory levels
7. `examples/07_stdlib.skn` — the standard library
8. `examples/08_cffi.skn` — using a C library (runs only with `siskin build`)
9. `examples/09_data.skn` — dictionaries, regular expressions and JSON together
10. `examples/12_closures.skn` — passing functions as values, anonymous functions, closures
11. `examples/13_system.skn` — dates, running other programs, folders
12. `examples/14_net.skn` — fetching JSON from the web, a small server
13. `examples/15_concurrency.skn` — doing several things at once (`spawn`, `channel`)
14. `examples/16_modules.skn` — splitting files and namespaces (imports `shapes.skn`)

The fastest way to learn is to edit each file yourself and run it.

```
~/siskin-target/release/siskin run examples/02_basics.skn
```

---

## 12. Tools

### 12.0 Message language

Compiler and runtime error messages are **in English by default**. To see them in Korean:

```
siskin --lang ko check main.skn     # just this once
export SISKIN_LANG=ko              # always Korean (put it in your shell config)
```

`siskin build` embeds the language selected at build time into the executable. So the error text of
`siskin run` and of the built program is identical when run with the same language.
Text the program prints itself (`print`) is, of course, not changed.

### 12.1 Code formatting — `siskin fmt`

```
siskin fmt main.skn        # one file
siskin fmt .              # every .skn under this folder
siskin fmt --check .      # only reports whether anything needs fixing (used in CI)
```

It normalizes indentation to 4-space steps (tabs too), turns `a+b` into `a + b` and `f( x,y )` into
`f(x, y)`, and cleans up trailing whitespace and excess blank lines. It doesn't split or join lines,
and it doesn't touch the contents of strings or comments. There's nothing to configure. The goal is for all Siskin code to
look the same.

After formatting, it reads the result back and checks that **not a single token of meaning has changed.**
If anything did change, it leaves the file as it was and tells you.

### 12.2 Editors

For VS Code, install `siskin-0.1.0.vsix` from [editors/vscode/](editors/vscode/)
(the `…` at the top right of the Extensions view → "Install from VSIX..."). Then

- as you type, it runs the same checks as `siskin check` and underlines errors
- "Format Document" (Shift+Alt+F) formats with `siskin fmt`
- pressing F12 on a function name goes to its declaration (including in imported files)
- hovering over a function name shows its signature and the comment right above it
- the ▷ button at the top right of the editor runs `siskin run`

For other editors, configure `siskin lsp` as the language server. [editors/README.md](editors/README.md) has
settings for Neovim and Helix.

### 12.3 Debugger — `siskin debug`

```
siskin debug main.skn              # stops at the first line
siskin debug main.skn -b 12        # runs until line 12 and stops
siskin debug main.skn -b util.skn:5 # line 5 of the imported file util.skn
```

When it stops, `(siskin)` appears and it takes commands.

| Command | What it does |
|---|---|
| `n` | Next line. If the line calls a function, runs that function in one go |
| `s` | Next line. If it calls a function, steps into it |
| `o` | Until the current function returns |
| `c` | Continue until a breakpoint |
| `b 12` / `d 12` | Set / delete a breakpoint at line 12 (`b util.skn:5` for an imported file) |
| `p expr` | Show a value. `p xs`, `p xs.len()`, `p a + b` |
| `v` | All variables of the current function |
| `l` | The code around the current line |
| `w` | Which functions were called to get here |
| `q` | Quit |

Pressing Enter alone repeats the last command. The debugger's own messages go to standard error, so
they don't mix with the program's output.

`s` also steps into functions in imported files, and you can set breakpoints in those files.
It doesn't stop inside the standard library.

**Programs that use C libraries, `std.net` or `spawn` are traced with the same commands.**
In that case `siskin debug` compiles the program natively with stopping points inserted, then runs it
(this takes about a second at first). When it stops inside a task (`spawn`), it shows which task, as in `(task 2)`.
`n` and `s` follow the task that stopped; other tasks stop only at breakpoints (`b`).
Ordinary programs can use this mode too, with `siskin debug --native main.skn`.

In this mode, `p` shows variable values as they are, and when you give an expression like `p a + b`, it copies the values
at the moment of stopping and evaluates on the copies. So even if a function called from `p` changes a value, the program is unaffected,
and C functions can't be called inside `p`.

If you want to use gdb, build with `siskin build --debug main.skn` and trace from there
(`gdb ./main` → `break main.skn:12` → `run`). Line numbers are those of the `.skn` file,
and variable names show up with a `v_` prefix and function names with a `mu_` prefix.

### 12.4 Packages — using code other people wrote

```
siskin new todo-app            # new project: siskin.toml and main.skn
cd todo-app
siskin add colors https://github.com/someone/siskin-colors --rev v1.0
siskin add util ../my-utils    # a folder on your own computer works too
```

Then in your code, `import colors` and use it as `colors.paint(...)`. It reads `lib.skn` in the package folder,
and `import colors.extra` reads `extra.skn` in that folder (used as `extra.function()`).
Each package has its own namespace, so two packages can use the same function name without clashing.
If two packages have modules with the same name, give one a different name, as in `import other.util as util2`.

`siskin.toml` looks like this.

```toml
[package]
name = "todo-app"
version = "0.1.0"

[dependencies]
colors = { git = "https://github.com/someone/siskin-colors", rev = "v1.0" }
util = { path = "../my-utils" }
```

| Command | What it does |
|---|---|
| `siskin install` | Fetches the packages in siskin.toml. Fetches exactly the versions recorded in `siskin.lock` |
| `siskin update` | Upgrades packages to new versions and rewrites `siskin.lock` |
| `siskin remove name` | Removes a package |

**Commit `siskin.lock` to git too.** It records the fetched versions (commits), so running
`siskin install` on another computer fetches exactly the same code. Fetched packages go into the `.siskin/` folder,
which you don't commit (`siskin new` adds it to `.gitignore`).

If a package uses other packages, those are fetched too. If the same name would be fetched from different places, you're told.

**Fetching by name only — the package list**

Packages in the package list (registry) can be fetched by name alone, without a URL. It's like Python's `pip install`.

```
siskin search color            # search the list
siskin add colors              # look up the URL in the list and fetch it
```

The list is not a server but **a single GitHub repository**. Each `packages/name.toml` file in it is one package,
and all it contains is this.

```toml
git = "https://github.com/someone/siskin-colors"
description = "Add color to terminal text"
```

The default list is `https://github.com/Haru-neo/siskin-registry` (published 2026-09-24; to use a different list, see "Using another list" below).
`siskin add colors` downloads the list to `~/.siskin/registry/` (refreshed on every use), looks up the URL there, and
writes `colors = { git = "..." }` into `siskin.toml`. From then on it's exactly the same as giving the URL directly.
Rust's crates.io-index and macOS's Homebrew also keep their lists in git repositories like this. There's no server to run, so it costs nothing,
and every change and who made it stays in the git history.

**Publishing your own package.** Push the project to GitHub (it must have `lib.skn` at the root) and run `siskin publish`;
it shows the contents of the file to add to the list repository. Send a PR adding that one file, and once it's merged, anyone can fetch it with `siskin add name`.
For a new version, just push a git tag to your repository (`siskin add name --rev v1.1`, `siskin update`).
If you put `description = "one-line description"` under `[package]` in `siskin.toml`, it shows up in `siskin search`.

**Using another list.** To point to a different place, such as a list used only inside your company, use one of these two.

```
SISKIN_REGISTRY=https://github.com/our-company/siskin-registry siskin add internal-tool
```

```toml
[registry]
url = "../our-list"          # in siskin.toml. A git URL or a folder on your computer
```

If the list is a folder on your computer, `siskin publish` writes the file for you directly. Even without an internet connection, it searches the previously downloaded list.
