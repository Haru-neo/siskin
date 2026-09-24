# Siskin 읽는 법 — C++을 조금 아는 사람을 위한 안내

이 문서는 예제 코드를 한 줄씩 읽을 수 있게 하는 것이 목적입니다.
왼쪽이 Siskin, 오른쪽이 같은 뜻의 C++입니다.

컴파일러 오류 메시지는 영어가 기본입니다. 한국어로 보려면 `SISKIN_LANG=ko` 를 설정하세요(12.0).
이 글에 나오는 오류 예시는 한국어 설정으로 찍은 것입니다.

---

## 1. 가장 작은 프로그램

```siskin
fn main():
    print("안녕\n")
```

```cpp
int main() {
    std::cout << "안녕\n";
}
```

| Siskin | C++ | 설명 |
|---|---|---|
| `fn` | `int`, `void` 같은 반환 타입 자리 | "여기서부터 함수다"라는 표시 |
| `:` 와 들여쓰기 | `{ }` | 함수 몸통의 시작과 끝 |
| `print(...)` | `std::cout << ...` | 화면에 출력 |
| `\n` | `\n` | 줄바꿈. 똑같습니다 |

**중괄호가 없습니다.** 대신 줄 끝의 `:` 가 "열고", 들여쓰기가 "몸통"이고,
들여쓰기가 풀리면 "닫힙니다". C++에서 `{` `}` 를 쓰던 자리를 공백 4칸이 대신합니다.

---

## 2. 함수

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

| Siskin | C++ | 설명 |
|---|---|---|
| `xs: [Int]` | `const std::vector<int>& xs` | 이름이 먼저, 타입이 뒤 |
| `[Int]` | `std::vector<int>` | 대괄호가 "~의 목록" |
| `-> Int` | 함수 이름 앞의 `int` | 반환 타입. 화살표로 뒤에 씁니다 |
| `var total = 0` | `int total = 0;` | 타입은 컴파일러가 알아서 알아냅니다 |
| `for x in xs:` | `for (int x : xs)` | 범위 기반 for와 같습니다 |
| 세미콜론 없음 | `;` 필수 | 줄바꿈이 문장의 끝입니다 |

### `let` 과 `var`

```siskin
let a = 10      # 못 바꿈
var b = 10      # 바꿀 수 있음
b = 20          # OK
a = 20          # 오류
```

```cpp
const int a = 10;   // 못 바꿈
int b = 10;         // 바꿀 수 있음
```

C++은 안 바뀌게 하려면 `const`를 **붙여야** 하는데,
Siskin은 반대로 바뀌게 하려면 `var`를 붙여야 합니다. 기본이 안전한 쪽입니다.

함수 바깥(파일 맨 위)에 둔 `let` 은 **파일 전체에서 보이는 상수**입니다.
어느 함수에서나 읽을 수 있고, 바꿀 수는 없습니다. 함수 바깥의 `var` 는 없습니다
(바뀌는 값은 `main` 안에 두고 함수에 넘깁니다. 여러 작업이 동시에 돌아도 안전하게).

```siskin
let TAX = 0.1
let UNITS = ["kg", "g"]

fn with_tax(price: Float) -> Float:
    return price * (1.0 + TAX)
```

---

## 2.5 기본 타입, 그리고 `fn` 은 타입이 아닙니다

`fn` 은 **함수를 선언한다는 표시**일 뿐, 타입이 아닙니다.
C++의 `int` 자리에 온다고 했던 건 위치 이야기였는데 오해를 살 설명이었습니다.

변수 타입은 따로 있습니다.

| Siskin | C++ | 설명 |
|---|---|---|
| `Int` | `long long` | 정수. 64비트 |
| `Float` | `double` | 실수. 64비트 |
| `Bool` | `bool` | 참/거짓 |
| `Str` | `std::string` | 문자열 |
| `[T]` | `std::vector<T>` | 목록 |
| `{K: V}` | `std::map<K,V>` | 사전 |
| `Byte` | `unsigned char` | 바이트. 메모리를 직접 다룰 때만 |

**`char` 는 없습니다.** 글자 하나도 그냥 `Str` 입니다.
`"안녕"[0]` 을 하면 길이 1짜리 `Str` 인 `"안"` 이 나옵니다.
C++의 `char` 는 1바이트라서 한글 한 글자도 담지 못하는데,
그 문제를 물려받지 않으려고 뺐습니다.

### 변수에는 보통 타입을 안 씁니다

```siskin
let n = 10           # Int 로 알아서 정해짐
let pi = 3.14        # Float
let name = "하루"    # Str
let ok = true        # Bool
```

```cpp
int n = 10;
double pi = 3.14;
std::string name = "하루";
bool ok = true;
```

쓰고 싶으면 이름 뒤에 `:` 를 붙여 씁니다. 필수는 아닙니다.

```siskin
let n: Int = 10
```

### 타입을 반드시 써야 하는 곳은 함수 시그니처뿐입니다

```siskin
fn add(a: Int, b: Int) -> Int:
    let result = a + b      # 여기는 안 써도 됨
    return result
```

함수 안쪽은 파이썬처럼 가볍게 쓰고, 함수의 입구와 출구에만 타입을 적습니다.
남이(그리고 AI가) 이 함수를 쓸 때 봐야 하는 건 입구와 출구뿐이기 때문입니다.

---

## 2.7 대소문자는 틀려도 됩니다

C++은 `myValue`와 `myvalue`를 완전히 다른 것으로 봅니다. Siskin은 완화했습니다.

규칙은 하나입니다. **정확히 쓴 이름이 있으면 그게 이깁니다. 없을 때만, 대소문자만
다른 이름이 딱 하나 있으면 거기에 붙여 줍니다.**

```siskin
struct UserAccount:
    displayName: Str

fn main():
    let account = useraccount(displayName: "하루")   # UserAccount 로 붙음
    print(f"{ACCOUNT.displayname}\n")               # account.displayName 으로 붙음
```

`siskin check`가 무엇을 고쳤는지 알려줍니다.

```
참고: 5:19 `useraccount` -> `UserAccount` (대소문자를 맞춰 두었습니다)
```

`struct User`와 `let user`처럼 대소문자만 다른 이름이 둘 다 진짜로 있으면,
둘 다 정확한 이름이라 서로 건드리지 않습니다. 이때 `USER`라고 쓰면 후보가
둘이라 고치지 않고 평소대로 오류를 냅니다. 애매하면 안 고칩니다.

---

## 3. 문자열 안에 값 끼워넣기

```siskin
print(f"합계: {sum(xs)}\n")
```

```cpp
std::cout << "합계: " << sum(xs) << "\n";
```

따옴표 앞의 `f` 가 "이 문자열 안의 `{ }` 는 값을 넣는 자리"라는 표시입니다.
C++의 `<<` 로 이어 붙이는 것보다 눈으로 읽기 쉽습니다.

### 자릿수와 폭 맞추기

`{ }` 안에서 값 뒤에 `:` 를 붙이면 모양을 정합니다. 파이썬과 같습니다.

```siskin
print(f"{3.14159:.2f}\n")     # 3.14      소수 둘째 자리까지
print(f"[{"abc":<6}]\n")      # [abc   ]  왼쪽 맞춤, 폭 6
print(f"[{42:>5}]\n")         # [   42]  오른쪽 맞춤
print(f"[{7:03d}]\n")         # [007]     0으로 채우기
print(f"{255:x}\n")           # ff        16진수
```

| 모양 | 뜻 |
|---|---|
| `.2f` | 소수 둘째 자리까지 |
| `<6` `>6` `^6` | 폭 6에 왼쪽·오른쪽·가운데 맞춤 |
| `05d` | 폭 5, 빈자리는 0 |
| `x` `X` | 16진수 |

여기의 폭은 **글자 수**입니다(파이썬과 같음). 한글은 화면에서 두 칸을 차지하므로,
한글이 섞인 표의 줄을 맞추려면 화면 칸으로 세는 `s.pad_right(10)` `s.pad_left(10)` 을 쓰세요.

`round(x, 1)` 처럼 반올림 자릿수를 주는 함수는 없습니다. 보여 줄 때 `{x:.1f}` 를 씁니다.

---

## 3.5 자주 쓰는 식 몇 가지

```siskin
let label = "짝수" if n % 2 == 0 else "홀수"    # 조건에 따라 값 고르기 (C++의 ? :)
var k = 10
k += 1        # -= *= /= %= 도 있습니다
if [1, 2] == [1, 2]:                           # 리스트·구조체·enum·튜플·사전도 == 로 비교합니다
    pass                                       # 아무것도 안 하는 자리 (파이썬과 같음)
```

| Siskin | C++ | 설명 |
|---|---|---|
| `a if 조건 else b` | `조건 ? a : b` | 조건이 참이면 a, 아니면 b |
| `pass` | `;` (빈 문장) | 블록 안에 쓸 것이 없을 때 |
| `and` `or` `not` | `&&` `\|\|` `!` | 논리 연산. 기호가 아니라 낱말입니다 |
| `true` `false` `none` | `true` `false` `nullptr` | 모두 소문자 |
| `x in xs` `x not in xs` | `std::find(...) != end` | 리스트에 있는지, 사전에 키가 있는지, 글자 안에 있는지 |

f 를 빼먹은 `"값은 {x}"` 는 글자 그대로 찍힙니다. 컴파일러가 경고(W0001)로 알려 줍니다.
서식의 폭은 숫자로만 씁니다(`{s:<8}`). 폭이 변수라면 `s.pad_right(w)` 를 쓰세요.

값을 돌려주는 함수는 **어느 길로 가든** `return 값` 으로 끝나야 합니다.
`if` 만 있고 `else` 가 없으면 컴파일러가 알려 줍니다(T0069).

---

## 4. 구조체와 메서드

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

| Siskin | C++ | 설명 |
|---|---|---|
| `self` | `this` | 자기 자신. C++과 달리 인자로 **명시**합니다 |
| `self.x` | `x` 또는 `this->x` | 항상 `self.` 를 붙입니다. 어디서 온 값인지 항상 보이게 |
| `fn len(self)` | `double len() const` | `self` 앞에 아무것도 없으면 읽기 전용 |

쓸 때는 이렇게 씁니다.

```siskin
let v = Vec2(x: 3.0, y: 4.0)
print(f"{v.len()}\n")
```

```cpp
Vec2 v{3.0, 4.0};
std::cout << v.len() << "\n";
```

`x:` `y:` 처럼 **필드 이름을 적고 값을 줍니다.** 순서를 외울 필요가 없고,
읽는 사람이 3.0이 뭔지 바로 압니다. 파이썬의 `x=3.0` 이 아니라 `x: 3.0` 입니다.
필드 순서대로라면 `Vec2(3.0, 4.0)` 처럼 이름 없이 줘도 됩니다.

### 값을 바꾸는 메서드 — `inout self`

`self` 앞에 아무것도 없으면 읽기만 합니다. 필드를 바꾸려면 **`inout self`** 라고 씁니다.
(`mut self` 나 `var self` 가 아닙니다.)

```siskin
struct Account:
    owner: Str
    balance: Int

    fn deposit(inout self, amount: Int):
        self.balance += amount

fn main():
    var acc = Account(owner: "하루", balance: 0)   # 바꿀 것이니 var
    acc.deposit(100)
    print(f"{acc.balance}\n")                      # 100
```

```cpp
struct Account {
    std::string owner;
    long long balance;
    void deposit(long long amount) { balance += amount; }   // const 가 없는 메서드
};
```

| Siskin | C++ | 설명 |
|---|---|---|
| `fn f(self)` | `void f() const` | 읽기만 |
| `fn f(inout self)` | `void f()` | 필드를 바꿀 수 있음 |
| `fn f(inout n: Int)` | `void f(int& n)` | 넘겨받은 변수를 바꿈. 부르는 쪽 변수는 `var` 여야 합니다 |

리스트 안에 든 구조체도 그 자리에서 바꿀 수 있습니다: `accounts[i].deposit(50)`.

### 값은 복사됩니다

`let b = a` 나 함수에 넘기기는 **복사본**처럼 동작합니다. `b` 를 바꿔도 `a` 는 그대로입니다.
C++에서 참조(`&`) 없이 값으로 넘기는 것과 같습니다. 그래서 어디선가 몰래 바뀌는 일이 없습니다.

대신 큰 리스트를 재귀 함수가 계속 돌려주고 받으면 그때마다 복사가 생겨 느려질 수 있습니다.
그럴 때는 결과를 모을 리스트를 `inout` 으로 넘기세요.

```siskin
fn collect(t: Tree, inout out: [Int]):   # 돌려주지 않고 out 에 바로 넣습니다
    ...
```

---

## 5. `?T` — 값이 없을 수도 있다

C++에서 "못 찾았다"를 알리는 방법은 여러 가지입니다. `nullptr`, `-1`,
`std::optional`, 예외... 그래서 함수를 볼 때마다 어느 쪽인지 확인해야 합니다.

Siskin은 하나뿐입니다. 타입 앞에 `?` 를 붙입니다.

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

- `?User` = "User가 있을 수도, 없을 수도"
- `none` = C++의 `nullptr` / `std::nullopt`
- **`null` 이라는 것이 없습니다.** 값이 없을 가능성은 타입에 적히고,
  컴파일러가 확인을 강제합니다. C++의 널 포인터 역참조 사고가 구조적으로 안 납니다.

받는 쪽은 둘 중 하나로 씁니다.

```siskin
let u = find(users, 7)
if u != none:
    print(u.name)          # 이 안에서 u 는 그냥 User 입니다
else:
    print("없음")

let name = env("USER") else "손님"     # 없으면 뒤의 값을 씁니다 (env 는 std.process)
```

| Siskin | C++ | 설명 |
|---|---|---|
| `if x != none:` | `if (x.has_value())` | 안쪽에서 x 는 `?` 가 벗겨진 값 |
| `x else 기본값` | `x.value_or(기본값)` | 없으면 기본값 |
| `if x == none: return` | | 이 줄 뒤로는 x 가 벗겨진 값 |

`Some(x)` `None` `Option[T]` 같은 이름은 없습니다. 있는 값은 그냥 `return u`, 없으면 `return none`.

**구조체 필드도 똑같이 벗겨집니다.**

```siskin
struct Todo:
    title: Str
    due: ?Str

fn show(t: Todo):
    if t.due != none:
        print(t.title + " — 마감 " + t.due + "\n")   # 이 안에서 t.due 는 그냥 Str
```

`t.due` 에 새 값을 넣거나, `t` 를 바꿀 수 있는 함수(`inout`)에 넘기면 그 뒤로는 다시 `?Str` 입니다.

필드 타입으로 `?구조체` 와 `!구조체` 도 됩니다. 자기 자신을 가리키는 필드도 되어서
연결 리스트나 트리를 바로 만듭니다.

```siskin
struct Node:
    value: Int
    next: ?Node          # 다음 칸이 없을 수도 있음

let list = Node(value: 1, next: Node(value: 2, next: none))
```

---

## 6. `!T` — 실패할 수도 있다

```siskin
fn parse_age(text: Str) -> !Int:
    let n = try int(text)
    if n < 0:
        return error("나이는 음수일 수 없습니다")
    return n
```

C++이라면 예외를 던지거나 에러 코드를 반환했을 자리입니다.

```cpp
int parse_age(const std::string& text) {
    int n = std::stoi(text);        // 실패하면 예외를 던짐
    if (n < 0) throw std::runtime_error("나이는 음수일 수 없습니다");
    return n;
}
```

- `!Int` = "Int를 주거나, 에러를 주거나"
- `error("...")` = 에러를 만들어 반환
- `try` = "이게 실패하면 여기서 바로 내 함수도 실패로 끝내라"

**예외가 없습니다.** C++의 예외는 함수 시그니처만 봐서는 던지는지 알 수 없고,
호출한 쪽을 건너뛰고 튀어 올라갑니다. Siskin은 실패 가능성이 `!` 로 항상 적혀 있고,
실패가 그냥 반환값이라 제어 흐름이 눈에 보입니다.

받는 쪽은 이렇게 씁니다.

```siskin
let age = parse_age("34") catch e:
    print(f"실패: {e}\n")
    return
print(f"나이 {age}\n")
```

C++의 `try { } catch { }` 와 목적은 같은데, 감싸는 블록이 아니라
그 한 줄에 붙습니다. 어느 호출이 실패할 수 있는지가 한눈에 보입니다.

실패했을 때 멈추지 않고 **대신 쓸 값**으로 넘어가고 싶으면, `catch` 블록의
마지막 줄에 그 값을 적습니다.

```siskin
let age = parse_age(text) catch e:
    print(f"잘못된 나이라서 0으로 둡니다: {e}\n")
    0
```

`catch` 블록은 둘 중 하나여야 합니다. `return` / `continue` / `break` 로 빠져나가거나,
마지막 줄에 대신 쓸 값을 적거나. 둘 다 아니면 `age` 에 넣을 것이 없으므로
컴파일이 알려 줍니다(T0059).

`!T` 의 오류 값은 글자(`Str`)입니다. `catch e:` 의 `e` 는 `error("...")` 에 넣은 글입니다.

### 오류 종류를 enum 으로 — `E!T`

실패의 종류에 따라 다르게 처리하고 싶으면 오류 종류를 enum 으로 만들고,
`!` 앞에 그 이름을 적습니다. `BankError!Int` 는 "Int 를 주거나, 실패하면 BankError 값을 준다"는 뜻입니다.

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
    acc.withdraw(500) catch e:          # e 는 BankError
        match e:
            case NoFunds(need):
                print(f"{need}원 모자랍니다\n")
            case BadAmount:
                print("금액이 잘못됐습니다\n")
```

```cpp
enum class BankError { NoFunds, BadAmount };
std::expected<void, BankError> withdraw(long long amount);   // C++23
```

- `match e:` 도 보통 enum 처럼 빠진 종류가 있으면 컴파일러가 알려 줍니다.
- `try` 는 같은 오류 타입끼리 올립니다. `BankError!T` 를 부르는 함수가 그냥 `!T`(글자 오류)면
  `try` 가 `NoFunds(430)` 같은 글자로 바꿔서 올립니다. 오류 타입이 서로 다른 enum 이면
  `catch e:` 로 받아서 `return error(...)` 로 바꿔 주세요.
- 오류 타입 자리에는 enum 만 옵니다. `!T` 는 `Str!T` 와 같습니다.

`Result` `Ok` `Err` 는 없습니다. 실패할 수 있으면 타입 앞에 `!`, 실패는 `error(...)`, 받는 쪽은 `try` 나 `catch`.

`main` 도 `fn main() -> !Unit:` 으로 쓸 수 있습니다. 그러면 안에서 `try` 를 바로 쓸 수 있고,
실패하면 `error: <내용>`(한국어로 설정했으면 `오류: <내용>`)을 알리고 끝 코드 1로 끝납니다.
`fn main() -> BankError!Unit:` 처럼 enum 오류도 됩니다.

---

## 7. `enum` 과 `match`

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

C++의 `enum` 은 그냥 정수라서 값을 같이 담지 못합니다.
그래서 보통 이렇게 씁니다.

```cpp
struct Circle { double r; };
struct Rect   { double w, h; };
using Shape = std::variant<Circle, Rect>;

double area(const Shape& s) {
    if (auto* c = std::get_if<Circle>(&s)) return 3.14159 * c->r * c->r;
    if (auto* r = std::get_if<Rect>(&s))   return r->w * r->h;
    // 빠뜨려도 컴파일은 됨
}
```

Siskin의 `enum` 은 **변형마다 자기 데이터를 가집니다.** 그리고 `match` 는
**모든 변형을 다뤘는지 확인합니다.** `Tri` 를 추가하고 `case` 를 안 쓰면 알려줍니다.

```
오류[T0012]: match가 Tri을(를) 빠뜨렸습니다
  도움말: `case Tri(...):` 를 추가하거나 `case _:` 로 나머지를 받으세요
```

`case Circle(r):` 에서 `r` 은 그 자리에서 꺼내진 값입니다.
C++의 `std::get_if` + `c->r` 두 단계가 한 줄로 줄어듭니다.

---

### `match` 를 쓸 때 알아 둘 것

```siskin
enum Tree:
    Leaf                                   # 필드 없는 변형
    Node(left: Tree, key: Int, right: Tree)   # 자기 자신을 담아도 됩니다

fn size(t: Tree) -> Int:
    match t:
        case Leaf:                          # 필드가 없으면 괄호 없이
            return 0
        case Node(l, k, r):
            return 1 + size(l) + size(r)

fn grade(score: Int) -> Str:
    match score:
        case 100:                          # 숫자·글자도 맞출 수 있습니다
            return "만점"
        case _:                            # 나머지 전부
            return "그 밖"
```

- 변형은 **enum 이름 없이** 씁니다: `case Leaf:` (`case Tree.Leaf:` 가 아님). 만들 때도 `Leaf`, `Node(left: ..., key: 3, right: ...)`.
- `case _:` 는 나머지 전부입니다. 이것이 있으면 빠진 변형을 알리지 않습니다.
- 글자·수를 맞출 때는 가짓수가 끝이 없으므로 `case _:` 가 꼭 있어야 합니다(T0068).
- `match` 는 문장입니다. 값을 돌려주는 식으로 쓰지 않습니다. 각 갈래에서 `return` 하거나 `var` 에 넣으세요.
- `?T` 와 `!T` 는 `match` 로 풀지 않습니다. 5장·6장의 `if x != none:`, `catch` 를 씁니다.

---

## 7.3 튜플 — 이름 없이 몇 개를 묶기

```siskin
fn min_max(xs: [Int]) -> (Int, Int):
    var lo = xs[0]
    var hi = xs[0]
    for x in xs:
        lo = min(lo, x)
        hi = max(hi, x)
    return (lo, hi)

let (lo, hi) = min_max([3, 9, 1])     # 풀어서 받기
let pair = min_max([3, 9, 1])
print(f"{pair.0} {pair.1}\n")         # 번호로 꺼내기
```

`(Int, Str)` 가 C++의 `std::pair<int, std::string>` / `std::tuple` 입니다.
함수가 값 두세 개를 돌려줄 때 구조체를 따로 만들지 않아도 됩니다.
튜플 리스트는 반복문에서 바로 풀 수 있습니다: `for (name, score) in pairs:`.
튜플 안의 값 하나만 바꾸지는 못합니다. `p = (새값, p.1)` 처럼 통째로 넣으세요.

---

## 7.4 제네릭 — 어떤 타입이든 받는 함수

```siskin
fn first[T](xs: [T]) -> ?T:
    if xs.len() == 0:
        return none
    return xs[0]

fn top_n[T](xs: [T], n: Int, key: (T) -> Int) -> [T]:
    var ys = xs
    ys.sort_by(fn(x): -key(x))        # 여기서 x 는 T 입니다
    return ys.slice(0, n)
```

함수 이름 뒤 `[T]` 가 C++의 `template <typename T>` 입니다. 부를 때는 타입을 적지 않습니다:
`first([1, 2])`, `first(["가", "나"])`. 컴파일러가 타입마다 따로 만들어 C 만큼 빠릅니다.

---

## 7.5 함수를 값으로 넘기기, 그리고 클로저

함수도 값입니다. 변수에 담고, 다른 함수에 넘기고, 돌려받을 수 있습니다.
함수의 타입은 `(받는 것) -> 돌려주는 것` 으로 적습니다.

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

### 익명 함수 — 이름 없이 그 자리에서 만들기

`fn(인자): 식` 이 이름 없는 함수입니다. C++의 람다 `[=](int x) { return x * k; }` 와 같습니다.

```siskin
let k = 3
let times_k = fn(x: Int): x * k      # 바깥의 k 를 붙잡았습니다
print(f"{times_k(5)}\n")              # 15

let big = nums.filter(fn(x): x > 10)  # 원소 타입을 알면 x 의 타입은 생략 가능
```

| Siskin | C++ | 설명 |
|---|---|---|
| `fn(x: Int): x * k` | `[=](int x) { return x * k; }` | 본문은 `:` 뒤 식 하나 |
| `fn(x): x * k` | (없음) | 들어갈 자리의 타입을 알면 인자 타입 생략 |
| `fn(x: Int) -> Int: ...` | `[=](int x) -> int {...}` | 반환 타입은 보통 생략 (식에서 알아냄) |
| `(Int) -> Int` | `std::function<int(int)>` | 함수 타입 |

인자 타입을 생략할 수 없는 곳(`let f = fn(x): ...` 처럼 들어갈 자리를 모를 때)에서는
컴파일러가 "타입을 적어 주세요"(T0054)라고 알려 줍니다.

### 여러 줄이 필요하면 — 함수 안에 함수

익명 함수는 식 하나뿐입니다. 여러 줄이 필요하면 함수 안에 `fn` 을 선언합니다.
이것도 바깥 값을 붙잡는 클로저이고, 자기 이름으로 자기를 부를 수 있습니다(재귀).

```siskin
fn main():
    let prefix = "값"
    fn describe(n: Int) -> Str:
        if n <= 0:
            return prefix
        return describe(n - 1) + "!"
    print(describe(3) + "\n")     # 값!!!
```

### 붙잡은 값은 "만들 때의 복사본"이고, 읽기만 합니다

```siskin
var base = 1
let g = fn(x: Int): x + base
base = 100
print(f"{g(1)}\n")      # 2  ← 만들 때 base 는 1 이었습니다
```

C++로 치면 언제나 `[=]`(값으로 붙잡기)입니다. Siskin의 "복사한 뒤 고쳐도 원본은 그대로"
규칙과 같은 생각입니다. 그래서 클로저 안에서 붙잡은 값을 바꾸려 하면 컴파일이 막습니다(T0053).
바뀐 값이 필요하면 함수가 새 값을 돌려주게 만듭니다.

```siskin
var n = 0
fn inc():
    n += 1        # 오류[T0053]: `n`은(는) 바깥에서 붙잡은 값이라 클로저 안에서 바꿀 수 없습니다
```

### 구조체 필드에 함수 담기

```siskin
struct Button:
    label: Str
    on_click: (Str) -> Str

    fn click(self) -> Str:
        return self.on_click(self.label)   # 필드에 담긴 함수를 부릅니다
```

C 라이브러리에 콜백으로 넘길 때만은 이름 붙은 최상위 함수여야 합니다.
C 쪽에는 "붙잡은 값" 을 함께 넘길 자리가 없기 때문입니다.

## 8. 계약 (`requires` / `ensures`)

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

- `requires` = 들어오기 전에 반드시 참이어야 하는 것
- `ensures` = 나갈 때 반드시 참이어야 하는 것 (`result` 가 반환값)

C++의 `assert` 와 같은 일인데, 함수 **시그니처 바로 아래**에 있어서
이 함수를 쓰려는 사람이 본문을 안 읽어도 조건을 봅니다.
디버그 빌드에서만 검사하고 릴리스에서는 사라지는 것도 `assert` 와 같습니다.
`siskin run` 과 그냥 `siskin build` 는 검사하고, `siskin build --release` 만 뺍니다.

- `requires` `ensures` 는 여러 줄 써도 됩니다. 모두 참이어야 합니다.
- 메서드에도 씁니다. `inout self` 메서드의 `ensures` 에서 `self.필드` 는 **바뀐 뒤의** 값입니다.

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

`"""` 로 감싼 설명 안에 `>>>` 로 예제를 적으면, `siskin test` 가 그 예제를
**진짜로 실행해서** 결과가 맞는지 확인합니다. 문서와 테스트가 같은 자리에 있습니다.

기대값은 파이썬처럼 적습니다: 글자 `"abc"`, 수 `3` `2.5`, `true` `false`, `none`,
리스트 `[1, 2]`, 사전 `{"a": 1}`, 튜플 `(1, "a")`.

---

## 9.5 메모리 — 세 단계

C++에서 가장 신경 쓰이는 부분입니다. Siskin은 여기를 세 칸으로 나눠 놓고,
필요한 만큼만 아래로 내려가게 합니다.

### 9.5.1 Level 0 — 그냥 씁니다

```siskin
let names = ["하루", "미카"]
names.push("루비")
```

`delete`도 `free`도 없습니다. 언제 지울지는 컴파일러가 정합니다.
C++의 `std::vector`를 값으로 쓰는 것과 같은 감각입니다.

### 9.5.2 Level 1 — 아레나: 블록이 끝나면 통째로 버립니다

```siskin
with arena a:              # 여기서부터 a가 대는 메모리
    var xs = a.list[Int]() # a가 대는 리스트 (push 로 바꿀 것이니 var)
    xs.push(1)
    xs.push(2)
    print(str(xs) + "\n")
# 여기서 a가 잡아 둔 것 전부 한 번에 사라집니다
```

한 줄씩:

| 줄 | 뜻 |
|---|---|
| `with arena a:` | 이 블록 전용 메모리 창고를 하나 엽니다. 이름은 `a` |
| `a.list[Int]()` | 그 창고에서 대는 Int 리스트. 쓰는 법은 보통 리스트와 똑같습니다 |
| 블록 끝 | 창고째로 버립니다. 하나씩 지우지 않습니다 |

C++로 치면 커스텀 `allocator`를 붙인 컨테이너를 스코프 안에서만 쓰고
스코프 끝에서 풀(pool)을 통째로 비우는 것입니다. 다만 Siskin은
`return`으로 빠져나가도 반드시 비웁니다.

**왜 이게 빠른가:** 보통 할당은 "빈 자리를 찾고, 표시하고, 나중에 찾아서 돌려주는"
일입니다. 아레나는 "창고 끝 표시를 앞으로 미는" 것이 전부이고, 돌려주는 일은
블록 끝에 한 번뿐입니다. 재 본 결과 4.1배 빨랐습니다.

**막히는 것:** 아레나에서 만든 값을 블록 밖으로 내보내려 하면 컴파일이 막습니다.
블록이 끝나면 그 메모리는 없어지니까요.

```
오류[T0040]: `xs`은(는) 아레나에서 나온 값이라 블록 밖으로 내보낼 수 없습니다
  도움말: 아레나 메모리는 블록 끝에서 전부 해제됩니다. 필요한 값은 복사해서 내보내세요
```

### 9.5.3 Level 2 — 원시 포인터: C와 똑같이

```siskin
unsafe:
    let p = alloc[Int](4)   # 칸 4개짜리 메모리를 받습니다
    p[0] = 10
    p[1] = 20
    let q = p + 1           # 한 칸 뒤를 가리키는 포인터
    print(str(q[0]) + "\n") # 20
    free(p)                 # 직접 돌려줍니다
```

C++와 나란히:

| Siskin | C++ |
|---|---|
| `let p = alloc[Int](4)` | `long long* p = new long long[4];` |
| `p[0] = 10` | `p[0] = 10;` |
| `let q = p + 1` | `long long* q = p + 1;` |
| `free(p)` | `delete[] p;` |

`unsafe:` 안에서만 됩니다. 밖에서 쓰면 컴파일이 막습니다.
"여기는 내가 책임진다"고 코드에 적어 두는 표시입니다.

### 9.5.4 실수하면 어떻게 되나

C++에서는 조용히 넘어가서 한참 뒤에 엉뚱한 곳이 터지는 것들입니다.
Siskin의 **디버그 빌드**(`--release` 없이 빌드하면 기본값)는 그 자리에서 잡습니다.

```siskin
unsafe:
    let p = alloc[Int](4)
    p[0] = 42
    free(p)
    print(str(p[0]) + "\n")   # 이미 돌려준 메모리를 읽습니다
```

```
실행 오류: 이미 해제된 메모리에 접근했습니다 (use-after-free)
```

범위를 벗어난 접근, 두 번 해제, 아레나 블록을 벗어난 포인터 사용도 같은 방식으로 잡힙니다.

**`--release`를 붙이면** 이 검사가 전부 사라집니다. 포인터가 그냥 C 포인터가 되고
속도는 손으로 쓴 C와 같아집니다. 개발할 때는 잡아 주고, 배포할 때는 비키는 구조입니다.

**단 하나 주의:** 디버그 빌드는 `free`한 메모리를 실제로 돌려주지 않고 붙들어 둡니다.
"이 메모리는 죽었다"는 표시를 읽을 수 있어야 검사가 되기 때문입니다.
그래서 디버그로 돌리면 메모리를 더 씁니다. `--release`에서는 정상입니다.

### 9.5.5 언제 어느 칸을 쓰나

| 상황 | 칸 |
|---|---|
| 대부분의 코드 | Level 0 |
| 한 프레임, 한 요청처럼 "끝이 분명한" 작업 안에서 할당이 많을 때 | Level 1 |
| 메모리 배치를 직접 정해야 할 때, C 라이브러리와 맞닿을 때 | Level 2 |

위에서부터 쓰다가 느린 곳만 내려가면 됩니다.

---

## 9.7 표준 라이브러리

`import`로 가져와 씁니다. 파이썬과 비슷하지만 **와일드카드 임포트는 없습니다.**
쓸 이름을 하나하나 적어야 하고, 그래서 이 이름이 어디서 왔는지 항상 보입니다.

```
from std.math import sqrt, pi
```

한 줄씩:

| 줄 | 뜻 |
|---|---|
| `from std.math import sqrt, pi` | `std.math`에서 `sqrt`와 `pi`만 가져옵니다 |
| `import std.math` | 모듈째로 가져옵니다. 쓸 때 `math.sqrt(...)` (모든 표준 모듈이 됩니다) |

### 어디에 무엇이 있나

**따로 가져올 필요 없는 것** (항상 쓸 수 있습니다)

`print` `eprint` `len` `range` `str` `int` `float` `abs` `min` `max` `sum` `assert` `error`
`args` `input` `exit`

| 이름 | 하는 일 |
|---|---|
| `args()` | 명령줄 인자 `[Str]`. `siskin run 파일.skn a b` 나 `./프로그램 a b` 의 `["a", "b"]` (프로그램 이름은 빠짐) |
| `input()` | 한 줄 읽기 `?Str`. 입력이 끝나면 `none`, 빈 줄은 `""` |
| `exit(n)` | 끝 코드 n 으로 바로 끝냅니다 |
| `eprint(...)` | 오류 출력(stderr)으로 씁니다. 쓰는 법은 `print` 와 같습니다 |
| `int(글)` `float(글)` | 글을 수로 읽습니다. 실패할 수 있어 `!Int` `!Float` |

표준 입력을 끝까지 한 줄씩 읽는 모양:

```siskin
fn main():
    while true:
        let line = input()
        if line == none:
            break
        print(f"읽음: {line}\n")
```

**`std.math`**

| 이름 | 하는 일 |
|---|---|
| `sqrt(x)` | 제곱근 |
| `sin(x)` `cos(x)` `tan(x)` | 삼각함수 (라디안) |
| `log(x)` `log10(x)` `exp(x)` | 로그와 지수 |
| `floor(x)` `ceil(x)` `round(x)` | 내림·올림·반올림 (Int를 냅니다) |
| `pow(a, b)` | 거듭제곱 |
| `pi()` `e()` | 원주율과 자연상수 |

`pi`와 `e`는 괄호가 붙습니다. `pi()`라고 씁니다.

수학 함수는 **Float만 받습니다.** `sqrt(2)`는 오류이고 `sqrt(2.0)`이라고 써야 합니다.
Siskin에는 몰래 일어나는 형변환이 없기 때문입니다. Int를 넘기려면 `sqrt(float(n))`.

**`std.random`**

| 이름 | 하는 일 |
|---|---|
| `seed(n)` | 씨앗을 심습니다. 같은 씨앗이면 항상 같은 값이 나옵니다 |
| `rand()` | 0 이상 1 미만의 Float |
| `rand_int(a, b)` | a 이상 b 미만의 Int |

**`std.time`** — 시각과 날짜

| 이름 | 하는 일 |
|---|---|
| `now()` | 1970년부터 지금까지의 초 (Float) |
| `clock()` | 프로그램이 시작한 뒤 흐른 초. 속도 재기에 씁니다 |
| `sleep(초)` | 잠깐 쉽니다. `sleep(0.5)` 처럼 Float 로 |
| `today()` | 지금 이 컴퓨터 시간대의 날짜·시각 (`DateTime`) |
| `date(년, 월, 일)` | 그 날 0시 (`DateTime`) |
| `local_time(초)` `utc_time(초)` | `now()` 같은 초를 날짜로 |
| `parse_time(글, 모양)` | `"2026-03-01"` 과 `"%Y-%m-%d"` 로 날짜 읽기 (`!DateTime`) |

`DateTime` 에는 `year` `month` `day` `hour` `minute` `second` `weekday`(월요일=1) 가 있고,
붙는 것은 `format(모양)` `date_str()` `time_str()` `to_str()` `weekday_name()`
`add_days(n)` `add_seconds(n)` `days_until(다른날)` `timestamp()` 입니다.

`format` 의 모양 글자: `%Y`(2026) `%m`(09) `%d`(05) `%H` `%M` `%S` `%y`(26)
`%a`(Mon) `%A`(Monday) `%b`(Sep) `%K`(월) `%z`(+0900) `%%`(%)

```siskin
import std.time
let d = date(2026, 12, 25)
print(d.format("%Y년 %m월 %d일 (%K)"))     # 2026년 12월 25일 (금)
print(d.add_days(7).date_str())            # 2027-01-01
```

`import std.time` 으로 모듈째 가져오면 `today()` 처럼 바로 쓰거나 `time.today()` 로 씁니다.

**`std.fs`** — 전부 실패할 수 있으므로 `!T`를 냅니다. `try`를 붙여 씁니다.

| 이름 | 하는 일 |
|---|---|
| `read_text(경로)` | 파일 전체를 문자열로 |
| `write_text(경로, 내용)` | 새로 씁니다 |
| `append_text(경로, 내용)` | 뒤에 이어 붙입니다 |
| `remove(경로)` | 지웁니다 |
| `exists(경로)` | 있는지만 봅니다 (`Bool`) |
| `list_dir(폴더)` | 안에 든 이름들, 이름순 (`![Str]`) |
| `make_dir(폴더)` | 만듭니다. 중간 폴더까지 한 번에, 이미 있으면 그냥 넘어갑니다 |
| `is_dir(경로)` | 폴더인지 봅니다 (`Bool`) |

오류 글은 어느 쪽으로 돌려도 같습니다: "없는 경로입니다", "권한이 없습니다",
"이미 있습니다", "폴더가 아닙니다" 같은 식입니다.

**`std.process`** — 다른 프로그램 실행, 환경 변수

| 이름 | 하는 일 |
|---|---|
| `run(명령)` | 셸 명령 한 줄을 실행하고 끝나기를 기다립니다. `run("ls -l \| wc -l")` |
| `run_args(프로그램, [인자...])` | 셸을 거치지 않고 실행합니다 |
| `env(이름)` | 환경 변수 (`?Str`, 없으면 `none`) |
| `set_env(이름, 값)` | 환경 변수를 정합니다. 이후 실행하는 프로그램도 봅니다 |
| `cwd()` `set_cwd(폴더)` | 지금 폴더 / 지금 폴더 바꾸기 |
| `pid()` | 이 프로그램의 번호 |

`run` 과 `run_args` 는 `Output` 을 줍니다. `code`(끝난 코드, 0 이면 성공), `out`(표준 출력),
`err`(표준 오류), 그리고 `ok()`. 없는 프로그램이면 `code` 가 127 이고 `err` 에 이유가 적힙니다.

**사용자에게 받은 글자를 명령에 넣을 때는 `run_args` 를 쓰세요.** `run` 은 셸이 글자를
해석하므로, 파일 이름에 `; rm -rf ~` 같은 것이 섞여 오면 그대로 실행됩니다.
`run_args` 는 인자를 셸 없이 그대로 넘깁니다.

**`std.net`** — 인터넷. [9.10](#910-인터넷--stdnet) 에서 따로 설명합니다.

### 리스트에 붙는 것

`push` `pop` `len` `reverse` `contains` `join` `sort` `index_of` `slice` `clear`

```
var xs = [5, 3, 9]         # 제자리에서 바꾸는 메서드를 쓰려면 var
xs.sort()                  # [3, 5, 9] — Int·Float·Str·Bool 리스트만. 다른 기준은 sort_by
# 기준 두 개(점수 큰 순, 같으면 이름 순): sort_by 는 같은 값의 순서를 지키므로 두 번 정렬합니다.
# people.sort_by(fn(p): p.name)
# people.sort_by(fn(p): -p.score)
xs.index_of(9)             # 2. 없으면 -1
xs.slice(0, 2)             # [3, 5] — 앞은 포함, 뒤는 제외
```

함수를 받는 것(7.5절의 익명 함수와 함께 씁니다):

```
xs.map(fn(x): x * 2)       # [6, 10, 18] — 원소마다 바꾼 새 리스트
xs.filter(fn(x): x > 4)    # [5, 9] — 조건에 맞는 것만
xs.any(fn(x): x > 8)       # true — 하나라도 맞나
xs.all(fn(x): x > 0)       # true — 모두 맞나
people.sort_by(fn(p): p.age)   # 기준값으로 정렬(제자리). 기준이 같으면 원래 순서 유지
```

### 문자열에 붙는 것

`len` `split` `upper` `lower` `strip` `replace` `contains` `starts_with` `ends_with`
`find` `repeat` `slice` `width` `pad_left` `pad_right`

한 글자씩 돌 때는 `for c in s:` 입니다. `c` 는 한 글자짜리 `Str` 이고, 한글도 한 글자씩 나옵니다.
글자끼리는 `c >= "a" and c <= "z"`, `c >= "가" and c <= "힣"` 처럼 크기를 비교할 수 있습니다.

```
"안녕하세요".len()          # 5 — 바이트가 아니라 글자 수입니다
"hello".find("ll")         # 2. 없으면 -1
"-".repeat(10)             # "----------"
"안녕하세요".slice(0, 2)    # "안녕"
```

C++의 `std::string`은 `.size()`가 바이트 수라서 한글이 섞이면 어긋납니다.
Siskin은 글자 수로 셉니다.

---

### 내 파일 나누기

같은 폴더의 다른 `.skn` 파일을 가져올 수 있습니다. 파일 이름이 곧 모듈 이름입니다.

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

**가져온 파일의 이름은 `모듈.이름` 으로 씁니다.** 파일마다 이름 칸이 따로라서,
내 파일과 가져온 파일이(또는 두 패키지가) 같은 이름을 만들어도 부딪히지 않습니다.
C++ 의 `namespace` 와 같은 일을 파일이 해 줍니다.

| 쓰는 법 | 뜻 |
|---|---|
| `import csvutil` | `csvutil.parse_line(...)`, 타입은 `csvutil.Row` |
| `from csvutil import parse_line` | `parse_line(...)` 로 바로 씁니다 |
| `from csvutil import parse_line as parse` | 다른 이름으로 가져옵니다 |
| `import pkg.tools as t` | 모듈에 짧은 이름을 붙입니다: `t.run(...)` |
| `fn _helper()` | `_` 로 시작하면 그 파일 안에서만 씁니다(밖에서 부르면 오류) |

`import csvutil` 만 하고 `parse_line(...)` 처럼 모듈 이름 없이 써도 한 곳에만 있으면
동작하지만, `csvutil.parse_line` 으로 쓰라는 경고(W0002)가 나옵니다. 두 모듈에 같은
이름이 있으면 어느 것인지 적으라는 오류(E0147)입니다. `match` 의 `case Circle(r):` 처럼
대상의 타입으로 이미 알 수 있는 자리는 모듈 이름을 안 붙여도 됩니다.

가져온 파일 안에 오류가 있으면 그 파일 이름과 줄로 알려 줍니다.
가져온 파일에는 `main` 이 없어도 됩니다(그 파일만 `siskin check` 하면 main 이 없다고 나옵니다).
예제는 `examples/16_modules.skn` 와 `examples/shapes.skn` 입니다.

---

## 9.8 C·C++ 라이브러리 쓰기

새 언어의 가장 큰 약점은 남이 만들어 둔 것이 없다는 점입니다.
Siskin 은 C 로 번역된 뒤 컴파일되므로, **세상에 나와 있는 C·C++ 라이브러리를
그대로 부릅니다.** 중간에 끼는 변환 계층이 없어서 호출 비용이 0 입니다.

함수를 하나씩 손으로 옮겨 적을 필요도 없습니다. 헤더 파일(라이브러리 설명서)
이름만 적으면 그 안의 함수를 Siskin 이 알아서 읽어 옵니다.

```
import c "zlib.h" link "z"

fn main():
    print(str(crc32(0, "hello siskin", 10)) + "\n")
```

한 줄씩:

| 줄 | 뜻 |
|---|---|
| `import c "zlib.h"` | zlib 의 설명서를 읽어 함수를 전부 가져옵니다 |
| `link "z"` | zlib(`-lz`)을 함께 묶으라는 표시 |
| `crc32(0, "hello siskin", 10)` | 보통 함수처럼 부릅니다 |

C++ 은 `import cpp "헤더.hpp"` 입니다. C++ 은 이름이 안에서 뒤틀려 저장되고
클래스·가상 함수 같은 게 있어서 그대로는 못 부르는데, Siskin 이 가운데에 다리 놓는
파일을 자동으로 써서 C++ 컴파일러에게 같이 넘깁니다.

```
import cpp "shapes.hpp" from "cpplib" also "cpplib/shapes.cpp"

fn main():
    let 원 = geo_Circle_new(2.0)
    print(str(geo_Circle_area(원)) + "\n")
    geo_Circle_delete(원)
```

### 무엇이 열렸는지 보기

```
siskin ffi zlib.h          # 몇 개나 쓸 수 있는지
siskin ffi zlib.h --all    # 함수 이름과 생김새를 전부
```

zlib 은 81개 중 80개, sqlite3 은 291개 중 283개, libpng 은 246개 전부가
바로 열립니다.

### 주고받을 수 있는 타입

| C 쪽 | Siskin 쪽 |
|---|---|
| `int`, `long`, `size_t` … 정수 전부 | `Int` |
| `float`, `double` | `Float` |
| `_Bool` | `Bool` |
| `const char *` | `Str` |
| 그 밖의 포인터 (`FILE*`, `sqlite3*`) | `Int` — 손잡이 |
| `T **` ("여기 결과를 넣어라") | `inout Int` — `var` 변수를 그냥 넘깁니다 |
| 함수 포인터 (콜백) | 함수 — 이름 붙인 내 함수를 넘깁니다 |

```
var db = 0
sqlite3_open(":memory:", db)     # db 에 결과가 들어옵니다
```

### 두 가지 주의

**`siskin run` 도 그대로 됩니다.** 라이브러리를 쓰는 프로그램은 조용히 컴파일해서
돌리므로, `siskin build` 로 만든 것과 결과가 항상 같습니다.

**해제한 메모리를 C 에 넘기면 잡힙니다.** 디버그 빌드에서는 C 로 넘어가기
직전에 한 번 확인합니다.

```
실행 오류: 이미 해제된 메모리에 접근했습니다 (use-after-free)
```

C++ 에서는 이 실수가 그대로 통과해 한참 뒤에 터집니다.

### 손으로 적는 방법 (예전 방식)

헤더가 없거나 한두 개만 쓸 때는 직접 적을 수도 있습니다.

```
extern "C" link "z"
extern "C" fn crc32(crc: Int, buf: Str, len: Int) -> Int
```

다만 이때는 C 쪽 진짜 타입을 Siskin 이 모릅니다. C 함수가 32비트 `int` 를
돌려주는데 `Int` 로 적으면 값이 깨질 수 있습니다. **가능하면 `import c` 를
쓰세요.** 그쪽은 C 컴파일러가 타입을 대신 검사해 줍니다.

자세한 것은 [LIBS.md](LIBS.md) 에 있습니다.

---

## 9.9 사전, 정규식, JSON

### 9.9.1 사전 — 이름표를 붙여 담는 상자

```
var ages = {"하루": 20, "미카": 3}
ages["루비"] = 7              # 새로 넣기
ages["하루"] = 21             # 이미 있으면 바꾸기
```

C++의 `std::map`이나 `unordered_map`에 해당합니다. 다만 두 가지가 다릅니다.

**넣은 순서를 기억합니다.** `keys()`는 넣은 순서대로 나옵니다.
C++의 `map`은 정렬 순서, `unordered_map`은 아무 순서입니다.

**없는 이름을 물으면 `none`이 나옵니다.**

```
let n = ages["없는사람"]
if n != none:
    print(str(n))
```

C++의 `m["없는키"]`는 조용히 0을 만들어 넣습니다. 그래서 "왜 없는 항목이 생겼지?"
하는 버그가 납니다. Siskin은 없으면 없다고 말합니다.

쓸 수 있는 것: `len()` `set(키, 값)` `has(키)` `keys()` `get(키, 기본값)`

키와 값을 함께 돌 때는 `for k, v in ages:` 입니다.

사전 안의 리스트에 하나 더 넣을 때는 꺼내서 바꾼 뒤 다시 넣습니다(값이 복사되기 때문입니다).

```siskin
var groups: {Str: [Str]} = {}
var names = groups["과일"] else []
names.push("사과")
groups["과일"] = names
```

### 9.9.2 정규식 — 글자 모양으로 찾기

```
from std.re import test, find_all, groups, replace

test(r"\d+", "주문 42개")            # true — 숫자가 있나?
find_all(r"\d+", "42, 17, 8")        # ["42", "17", "8"]
replace(r"\d", "010-1234", "*")      # "***-****"
```

**`r"..."` 에 주의하세요.** 정규식에는 역슬래시가 많이 나오는데, 보통 문자열에서는
`\n`이 줄바꿈이 되어 버립니다. 앞에 `r`을 붙이면 적은 그대로 읽습니다.
`r`을 빠뜨리면 컴파일이 이렇게 알려 줍니다.

```
오류[E0008]: `\d` 는 모르는 표기입니다
  도움말: 역슬래시를 그대로 쓰려면 `\\d` 또는 원시 문자열 `r"..."` 을 쓰세요
```

쓸 수 있는 것:

| 이름 | 하는 일 |
|---|---|
| `test(정규식, 글)` | 있나 없나만 (`Bool`) |
| `find(정규식, 글)` | 처음 맞는 부분 (`?Str`) |
| `find_all(정규식, 글)` | 맞는 부분 전부 (`[Str]`) |
| `groups(정규식, 글)` | 첫 일치의 괄호 조각들. 0번은 전체입니다 |
| `replace(정규식, 글, 바꿀것)` | 맞는 곳을 전부 바꿉니다 |
| `split_re(정규식, 글)` | 맞는 곳에서 쪼갭니다 |

쓸 수 있는 표기: 글자 · `.` · `*` `+` `?` (뒤에 `?`를 붙이면 최소 일치) ·
`[a-z]` `[^...]` · `\d \w \s`와 대문자 반대 · `^` `$` · `|` · `(...)` `(?:...)`

한글도 됩니다. `[가-힣]+` 이 그대로 통합니다.

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

한 줄씩:

| 줄 | 뜻 |
|---|---|
| `try parse(text)` | 글을 JSON으로 읽습니다. 잘못됐으면 실패를 냅니다 |
| `doc.get("name")` | 그 이름이 있으면 값, 없으면 `none` |
| `name.as_str()` | 문자열이면 문자열, 아니면 `none` |

`none` 확인이 두 번 나오는 게 번거로워 보이지만, 이게 JSON을 다룰 때 사고가 나는
자리 두 곳입니다. 이름이 없거나, 있는데 기대한 종류가 아니거나. 둘 다 물어보게 합니다.

만들 때는 이렇게 합니다.

```
let out = jdict()
out.set("lang", jstr("Siskin"))
out.set("version", jint(1))
print(stringify(out))        # {"lang":"Siskin","version":1}
```

| 만드는 것 | |
|---|---|
| `jnull()` `jbool(b)` `jint(n)` `jfloat(f)` `jstr(s)` | 값 하나 |
| `jlist()` `jdict()` | 빈 배열 / 빈 객체 |

| 읽는 것 | |
|---|---|
| `kind()` | "null" "bool" "int" "float" "str" "list" "dict" 중 하나 |
| `as_int()` `as_float()` `as_str()` `as_bool()` | 맞으면 값, 아니면 `none` |
| `get(이름)` `at(번호)` | 있으면 값, 없으면 `none` |
| `len()` `keys()` | 개수와 이름들 |
| `set(이름, 값)` `push(값)` | 객체와 배열에 넣기 |

JSON 값의 타입 이름은 **`Json`** 입니다. 함수 인자로 받을 때 `fn f(item: Json)` 처럼 씁니다.
`as_float()` 는 `3` 같은 정수도 `3.0` 으로 줍니다. 배열은 `for x in doc:` 로 돕니다(배열이 아니면 한 번도 돌지 않습니다).

JSON 을 구조체로 옮기는 흔한 모양:

```siskin
from std.json import parse
from std.fs import read_text

struct Item:
    name: Str
    price: Float
    discount: ?Float          # 없거나 null 일 수 있음

fn to_item(j: Json) -> !Item:
    let n = j.get("name") else j
    let name = n.as_str() else ""
    if name == "":
        return error("name 이 없습니다")
    let p = j.get("price") else j
    let price = p.as_float()
    if price == none:
        return error(f"{name}: price 가 수가 아닙니다")
    var discount: ?Float = none
    let d = j.get("discount")
    if d != none:
        discount = d.as_float()           # null 이면 none
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

## 9.10 인터넷 — `std.net`

```siskin
import std.net

let r = http_get("https://pypi.org/pypi/requests/json") catch e:
    print("인터넷에 닿지 못했습니다: " + e + "\n")
    return
print(f"{r.status} {r.ok()}\n")          # 200 true
print(r.body)                              # 받은 내용 (글자)
```

**HTTP**

| 이름 | 하는 일 |
|---|---|
| `http_get(주소)` | 가져옵니다 (`!Response`) |
| `http_post(주소, 본문)` | 보냅니다. 본문이 `{` 나 `[` 로 시작하면 JSON 으로 보냅니다 |
| `http_request(방법, 주소, 헤더, 본문)` | 전부 정해서 보냅니다. `http_request("PUT", url, {"Authorization": "Bearer ..."}, body)` |
| `url_encode(글)` | 주소에 넣을 수 있게 바꿉니다. `"김 밥"` → `"%EA%B9%80%20%EB%B0%A5"` |
| `set_timeout(초)` | 기다리는 최대 시간. 처음엔 30초 |

`Response` 에는 `status`(200, 404 ...) `headers`(사전, 이름은 소문자) `body` 가 있고,
`ok()`(200~299 인가) 와 `header(이름)`(대소문자 상관없이, `?Str`) 이 붙습니다.

**404 나 500 은 오류가 아닙니다.** 서버가 대답은 한 것이라 `Response` 로 옵니다.
`catch` 로 가는 것은 주소를 못 찾거나, 연결이 안 되거나, 시간이 지난 경우입니다.
다른 주소로 넘기는 응답(301, 302 ...)은 알아서 따라갑니다.

`https` 는 컴퓨터에 깔린 OpenSSL 을 씁니다(리눅스에는 거의 항상 있습니다).
인증서를 확인하므로 가짜 서버에는 "인증서를 믿을 수 없습니다" 로 멈춥니다.
회사망처럼 `HTTPS_PROXY` 가 정해져 있으면 그걸 따릅니다.

**TCP 연결과 서버**

| 이름 | 하는 일 |
|---|---|
| `connect(호스트, 포트)` | 연결합니다 (`!Conn`) |
| `connect_tls(호스트, 포트)` | 보안 연결 (`!Conn`) |
| `listen(포트)` | 포트를 열고 기다립니다 (`!Server`). `listen_on("127.0.0.1", 포트)` 는 이 컴퓨터 안에서만 |

`Conn` 에는 `send(글)` `recv()` `recv_line()` `peer()` `close()`,
`Server` 에는 `accept()`(누가 올 때까지 기다렸다가 `Conn` 을 줍니다) `port()` `close()` 가 붙습니다.
`recv()` 와 `recv_line()` 은 상대가 연결을 끊으면 `""` 을 줍니다.

```siskin
let server = listen(8080) catch e:
    print(e + "\n")
    return
while true:
    let c = server.accept() catch e:
        continue
    let line = c.recv_line() catch e:
        ""
    c.send("받았습니다: " + line) catch e:
        print(e)
    c.close()
```

**웹 서버와 https 서버**

`server.next_request()` 는 손님을 받아 HTTP 요청 하나를 읽어 `Request` 로 줍니다.
`listen` 대신 `listen_tls` 로 열면 https 서버가 되고, 나머지 코드는 똑같습니다.

```siskin
import std.net

fn main():
    # https 로 열기. http 면 listen(8080) 으로 바꾸면 끝입니다.
    let server = listen_tls(8443, "cert.pem", "key.pem") catch e:
        print(e + "\n")
        return
    while true:
        let req = server.next_request() catch e:
            return                               # 서버가 닫혔을 때만 옵니다
        if req.path == "/hello":
            let name = req.query["name"] else "손님"
            req.respond(200, f"안녕하세요, {name}님") catch e:
                print(e + "\n")
        else:
            req.respond(404, "<h1>없는 쪽</h1>") catch e:
                print(e + "\n")
```

`https://localhost:8443/hello?name=김밥` 으로 들어가면 `안녕하세요, 김밥님` 이 보입니다.

| 이름 | 하는 일 |
|---|---|
| `listen_tls(포트, 인증서, 비밀열쇠)` | https 서버를 엽니다 (`!Server`). 두 파일은 PEM 형식 |
| `listen_tls_on("127.0.0.1", 포트, 인증서, 비밀열쇠)` | 이 컴퓨터 안에서만 받는 https 서버 |
| `server.next_request()` | 다음 요청을 기다려 `Request` 로 줍니다. 요청을 잘못 보낸 손님은 알아서 건너뜁니다 |
| `read_request(conn)` | `accept()` 로 받은 연결에서 요청 하나를 읽습니다 |
| `url_decode(글)` | `url_encode` 의 반대 |

`Request` 에는 `method`("GET" ...) `path`("/hello", `?` 뒤는 빠짐) `query`(사전) `headers`(사전, 이름은 소문자)
`body` 가 있고, `header(이름)` `respond(상태, 본문)` `respond_with(상태, 헤더, 본문)` 이 붙습니다.
`respond` 는 본문이 `{`/`[` 로 시작하면 JSON, `<` 면 HTML, 아니면 글자로 알리고 연결을 닫습니다.

**인증서.** https 서버에는 인증서 파일과 비밀 열쇠 파일이 필요합니다. 시험용은 이렇게 만듭니다.

```
openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 365 -subj /CN=localhost
```

이렇게 만든 인증서는 내가 스스로 서명한 것이라 브라우저가 "안전하지 않음" 경고를 띄웁니다(시험에는 괜찮습니다).
인터넷에 여는 진짜 서버는 Let's Encrypt(무료) 같은 곳에서 받은 `fullchain.pem` 과 `privkey.pem` 을 넣으면 됩니다.
보안 연결을 맺지 못한 손님(인증서를 안 믿는 브라우저, http 로 잘못 온 손님)은 서버를 멈추지 않고 건너뜁니다.

한 번에 한 손님씩 받으므로, 손님이 많으면 요청마다 `spawn` 으로 나눠 처리하세요(9.11).

**`siskin run` 도 네이티브로 돕니다.** 네트워크는 운영체제와 직접 이야기해야 해서,
`std.net` 을 쓰는 프로그램은 C 라이브러리를 쓸 때처럼 조용히 컴파일해서 돌립니다.
그래서 처음 한 번은 1초쯤 더 걸립니다.

여러 주소를 한꺼번에 받으려면 `spawn http_get(주소)` 로 동시에 보냅니다(아래 9.11).

---

## 9.11 여러 일을 동시에 — `spawn` 과 `channel`

```siskin
fn count_primes(lo: Int, hi: Int) -> Int:
    ...

fn main():
    let a = spawn count_primes(0, 50000)       # 새 작업에서 돌리고 바로 돌아옵니다
    let b = spawn count_primes(50000, 100000)
    print(f"{a.wait() + b.wait()}\n")           # 끝날 때까지 기다려 결과를 받습니다
```

```cpp
auto a = std::async(std::launch::async, count_primes, 0, 50000);
auto b = std::async(std::launch::async, count_primes, 50000, 100000);
std::cout << a.get() + b.get() << "\n";
```

| Siskin | C++ | 설명 |
|---|---|---|
| `spawn f(x)` | `std::async(f, x)` | 새 작업(스레드). 결과는 `Task[T]` |
| `t.wait()` | `future.get()` | 끝날 때까지 기다려 결과를 받습니다 |
| `t.done()` | `wait_for(0s) == ready` | 끝났는지만 봅니다 |
| `channel[Int]()` | 스레드 안전한 큐 | 작업끼리 값을 주고받는 통로 (`Chan[Int]`) |
| `channel[Int](10)` | 크기 정한 큐 | 10개가 차면 보내는 쪽이 기다립니다 |
| `ch.send(v)` / `ch.recv()` | push / pop | 받을 때는 `?T`: 닫히고 비었으면 `none` |
| `ch.close()` | | 다 보냈다고 알립니다 |
| `for x in ch:` | | 닫힐 때까지 하나씩 받습니다 |

```siskin
fn producer(ch: Chan[Int]):
    for i in range(5):
        ch.send(i * i)
    ch.close()                  # 이게 있어야 받는 쪽의 for 가 끝납니다

fn main():
    let ch = channel[Int]()
    let p = spawn producer(ch)
    for v in ch:
        print(f"{v}\n")
    p.wait()
```

**작업끼리 메모리를 같이 만지지 않습니다.** `spawn` 에 넘기는 값은 복사본이라
(클로저와 같은 규칙) 두 작업이 한 리스트를 동시에 바꾸는 사고(데이터 경쟁)가 생길 수 없습니다.
그래서 자물쇠(mutex)가 필요 없습니다. 결과는 `wait()` 로 돌려받거나 통로로 보냅니다.
원시 포인터(`*T`), 아레나, `Json` 값은 작업에 넘길 수 없습니다(`Json` 은 `stringify` 해서 글자로 넘기세요).

모든 작업이 서로를 기다리고 있으면(아무도 `close` 를 안 해서 받는 쪽이 끝없이 기다리는 등)
그대로 멈춰 있지 않고 "교착 상태(deadlock)" 실행 오류로 알려 줍니다.
`main` 이 끝나면 아직 도는 작업을 다 기다린 뒤 끝납니다. 작업 안에서 난 실행 오류는 프로그램을 멈춥니다.

**`siskin run` 과 `siskin build`:** 결과는 같고, 둘 다 CPU 여러 개를 진짜로 같이 씁니다
(4코어에서 `siskin build` 약 3.5배, `siskin run` 약 3.2배). 파이썬과 달리 `siskin run` 에도
"한 번에 한 작업" 제한(GIL)이 없습니다. 작업끼리 값을 나누지 않고 복사해서 넘기기 때문에
가능한 일입니다. 예제는 `examples/15_concurrency.skn` 입니다.


---

## 10. 기호 한눈에

| 기호 | 뜻 | C++로 치면 |
|---|---|---|
| `fn` | 함수 선언 표시 (타입 아님) | `int main()` 의 위치 |
| `Int` `Float` `Str` `Bool` | 기본 타입 | `long long` `double` `std::string` `bool` |
| `:` + 들여쓰기 | 블록 | `{ }` |
| `->` | 반환 타입 | 함수 이름 앞 타입 |
| `let` / `var` | 못 바꿈 / 바꿈 | `const T` / `T` |
| `[Int]` | Int의 목록 | `std::vector<int>` |
| `{Str: Int}` | 사전 | `std::map<std::string,int>` |
| `?T` | 없을 수도 있는 T | `std::optional<T>` |
| `!T` | 실패할 수도 있는 T | 예외 또는 에러 코드 |
| `E!T` | 실패하면 enum E 값을 주는 T | `std::expected<T, E>` |
| `none` | 값 없음 | `nullptr` / `nullopt` |
| `try` | 실패하면 전파 | 예외 전파 |
| `catch e:` | 실패 처리 | `catch (...)` |
| `f"{x}"` | 문자열에 값 끼움 | `<<` 로 이어 붙이기 |
| `self` | 자기 자신 | `this` |
| `#` | 주석 | `//` |
| `with arena a:` | 블록 전용 메모리 창고 | 스코프에 묶인 메모리 풀 |
| `unsafe:` | 여기부터 내 책임 | (표시가 따로 없음) |
| `*Int` | Int를 가리키는 원시 포인터 | `long long*` |
| `alloc[Int](4)` | 칸 4개 받기 | `new long long[4]` |
| `free(p)` | 돌려주기 | `delete[] p` |
| `from std.x import y` | 이름 하나 가져오기 | `#include` + `using` |
| `extern "C" fn ...` | C 함수 쓰겠다는 선언 | 헤더의 함수 선언 |
| `extern "C" link "z"` | 라이브러리 묶기 | `-lz` |
| `{"가": 1}` | 사전 | `std::map<std::string,int>` |
| `r"\d+"` | 원시 문자열 | `R"(\d+)"` |
| `(Int) -> Int` | 함수 타입 | `std::function<int(int)>` |
| `fn(x: Int): x * k` | 익명 함수(클로저) | `[=](int x) { return x * k; }` |
| `inout self` | 필드를 바꾸는 메서드 | `const` 없는 메서드 |
| `inout n: Int` | 넘겨받은 변수를 바꿈 | `int& n` |
| `(Int, Str)` / `p.0` | 튜플 / 첫째 값 | `std::pair` / `p.first` |
| `fn f[T](x: T)` | 제네릭 함수 | `template <typename T>` |
| `x else 기본값` | 없으면 기본값 | `x.value_or(기본값)` |
| `a if 조건 else b` | 조건에 따라 고르기 | `조건 ? a : b` |
| `case _:` | 나머지 전부 | `default:` |
| `pass` | 빈 문장 | `;` |
| `f"{x:.2f}"` | 소수 둘째 자리 | `std::format("{:.2f}", x)` |
| `args()` | 명령줄 인자 | `argv` |
| `import a` / `a.f()` | 다른 파일의 이름 쓰기 | `namespace a` / `a::f()` |
| `import a as b` | 모듈에 다른 이름 | `namespace b = a;` |
| `spawn f(x)` | 동시에 돌리기 | `std::async(f, x)` |
| `channel[Int]()` | 작업 사이 통로 | 스레드 안전 큐 |

---

## 11. 예제 읽는 순서

1. `examples/01_hello.skn` — 출력만
2. `examples/02_basics.skn` — 함수, 반복문, 리스트
3. `examples/03_types.skn` — 구조체, enum, match
4. `examples/04_errors.skn` — `?T`, `!T`
5. `examples/05_contracts.skn` — 계약, doctest
6. `examples/06_memory.skn` — 메모리 세 단계
7. `examples/07_stdlib.skn` — 표준 라이브러리
8. `examples/08_cffi.skn` — C 라이브러리 쓰기 (`siskin build` 로만 실행됩니다)
9. `examples/09_data.skn` — 사전·정규식·JSON 을 한 번에
10. `examples/12_closures.skn` — 함수를 값으로 넘기기, 익명 함수, 클로저
11. `examples/13_system.skn` — 날짜, 다른 프로그램 실행, 폴더
12. `examples/14_net.skn` — 웹에서 JSON 받아 오기, 작은 서버
13. `examples/15_concurrency.skn` — 여러 일을 동시에 (`spawn`, `channel`)
14. `examples/16_modules.skn` — 파일 나누기와 이름공간 (`shapes.skn` 를 가져다 씀)

각 파일을 직접 고쳐서 돌려보는 게 가장 빠릅니다.

```
~/siskin-target/release/siskin run examples/02_basics.skn
```

---

## 12. 도구

### 12.0 메시지 언어

컴파일러와 런타임의 오류 메시지는 **영어가 기본**입니다. 한국어로 보려면:

```
siskin --lang ko check main.skn     # 이번 한 번만
export SISKIN_LANG=ko              # 늘 한국어로 (셸 설정에 넣어 두세요)
```

`siskin build` 는 그때 고른 언어를 실행 파일에 넣습니다. 그래서 같은 언어로 돌린
`siskin run` 과 만든 프로그램의 오류 글이 똑같습니다.
프로그램이 직접 찍는 글(`print`)은 당연히 바뀌지 않습니다.

### 12.1 코드 정리 — `siskin fmt`

```
siskin fmt main.skn        # 파일 하나
siskin fmt .              # 이 폴더 아래 .skn 전부
siskin fmt --check .      # 고칠 곳이 있는지만 봅니다 (CI 에서 씁니다)
```

들여쓰기를 공백 4칸 단위로 맞추고(탭도 고칩니다), `a+b` 를 `a + b` 로, `f( x,y )` 를
`f(x, y)` 로, 줄 끝 공백과 너무 많은 빈 줄을 정리합니다. 줄을 나누거나 합치지는 않고,
문자열과 주석의 내용은 건드리지 않습니다. 설정할 것은 없습니다. 모든 Siskin 코드가 같은 모양이
되는 것이 목적입니다.

정리한 뒤에는 결과를 다시 읽어서 **뜻이 한 토큰도 바뀌지 않았는지** 확인합니다.
만에 하나 달라지면 파일을 그대로 두고 알려 줍니다.

### 12.2 에디터

VS Code 는 [editors/vscode/](editors/vscode/) 의 `siskin-0.1.0.vsix` 를 설치합니다
(확장 창 오른쪽 위 `…` → "VSIX에서 설치..."). 그러면

- 글자를 칠 때마다 `siskin check` 와 같은 검사를 해서 오류에 밑줄을 긋습니다
- "문서 서식"(Shift+Alt+F)이 `siskin fmt` 로 정리합니다
- 함수 이름에서 F12 를 누르면 선언으로 갑니다 (import 한 파일까지)
- 함수 이름에 마우스를 올리면 모양과 바로 위 주석이 보입니다
- 편집기 오른쪽 위 ▷ 버튼이 `siskin run` 을 돌립니다

다른 에디터는 `siskin lsp` 를 언어 서버로 적으면 됩니다. [editors/README.md](editors/README.md) 에
Neovim 과 Helix 설정이 있습니다.

### 12.3 디버거 — `siskin debug`

```
siskin debug main.skn              # 첫 줄에서 멈춥니다
siskin debug main.skn -b 12        # 12번째 줄까지 실행하고 멈춥니다
siskin debug main.skn -b util.skn:5 # import 한 파일 util.skn 의 5번째 줄
```

멈추면 `(siskin)` 이 나오고 명령을 받습니다.

| 명령 | 하는 일 |
|---|---|
| `n` | 다음 줄. 함수를 부르는 줄이면 그 함수는 한 번에 실행합니다 |
| `s` | 다음 줄. 함수를 부르면 그 안으로 들어갑니다 |
| `o` | 지금 함수가 끝날 때까지 |
| `c` | 멈출 곳까지 계속 |
| `b 12` / `d 12` | 12번째 줄에 멈출 곳 만들기 / 지우기 (`b util.skn:5` 는 import 한 파일) |
| `p 식` | 값 보기. `p xs`, `p xs.len()`, `p a + b` |
| `v` | 지금 함수의 변수 전부 |
| `l` | 지금 줄 둘레의 코드 |
| `w` | 어떤 함수들을 거쳐 여기 왔는지 |
| `q` | 끝내기 |

그냥 Enter 는 방금 한 명령을 한 번 더 합니다. 디버거의 말은 표준 오류로 나가서
프로그램의 출력과 섞이지 않습니다.

`s` 로 import 한 파일의 함수 안에도 들어가고, 그 파일에 멈출 곳을 둘 수도 있습니다.
표준 라이브러리 안에서는 멈추지 않습니다.

**C 라이브러리, `std.net`, `spawn` 을 쓰는 프로그램도 같은 명령으로 따라갑니다.**
이때 `siskin debug` 는 프로그램을 멈출 자리를 넣어 네이티브로 컴파일한 뒤 돌립니다
(처음에 1초쯤 걸립니다). 작업(`spawn`) 안에서 멈추면 `(task 2)` 처럼 몇 번째 작업인지 보여 줍니다.
`n` `s` 는 멈춘 그 작업을 따라가고, 다른 작업은 멈출 곳(`b`)에서만 멈춥니다.
보통 프로그램도 `siskin debug --native main.skn` 로 이 방식을 쓸 수 있습니다.

이 방식에서 `p` 는 변수 값을 그대로 보여 주고, `p a + b` 처럼 식을 주면 멈춘 순간의 값을
복사해서 계산합니다. 그래서 `p` 로 부른 함수가 값을 바꿔도 프로그램에는 영향이 없고,
C 함수는 `p` 안에서 부를 수 없습니다.

gdb 를 쓰고 싶으면 `siskin build --debug main.skn` 로 만든 뒤 따라갈 수 있습니다
(`gdb ./main` → `break main.skn:12` → `run`). 줄 번호는 `.skn` 파일 그대로이고,
변수 이름 앞에는 `v_`, 함수 이름 앞에는 `mu_` 가 붙어 보입니다.

### 12.4 패키지 — 남이 만든 코드 쓰기

```
siskin new 할일앱              # 새 프로젝트: siskin.toml 과 main.skn
cd 할일앱
siskin add colors https://github.com/누군가/siskin-colors --rev v1.0
siskin add util ../내-유틸     # 내 컴퓨터의 폴더도 됩니다
```

그다음 코드에서 `import colors` 한 뒤 `colors.paint(...)` 처럼 씁니다. 패키지 폴더의 `lib.skn` 를 읽고,
`import colors.extra` 는 그 폴더의 `extra.skn` 를 읽습니다(`extra.함수()` 로 씀).
패키지마다 이름 칸이 따로라서, 두 패키지가 같은 함수 이름을 써도 부딪히지 않습니다.
두 패키지의 모듈 이름이 같으면 `import other.util as util2` 처럼 다른 이름을 붙입니다.

`siskin.toml` 은 이렇게 생겼습니다.

```toml
[package]
name = "할일앱"
version = "0.1.0"

[dependencies]
colors = { git = "https://github.com/누군가/siskin-colors", rev = "v1.0" }
util = { path = "../내-유틸" }
```

| 명령 | 하는 일 |
|---|---|
| `siskin install` | siskin.toml 의 패키지를 받아 옵니다. `siskin.lock` 에 적힌 판을 그대로 받습니다 |
| `siskin update` | 패키지를 새 판으로 올리고 `siskin.lock` 을 새로 씁니다 |
| `siskin remove 이름` | 패키지를 뺍니다 |

**`siskin.lock` 도 git 에 같이 올리세요.** 받은 판(commit)이 적혀 있어서, 다른 컴퓨터에서
`siskin install` 해도 똑같은 코드를 받습니다. 받은 패키지는 `.siskin/` 폴더에 들어가는데,
이 폴더는 올리지 않습니다(`siskin new` 가 `.gitignore` 에 넣어 둡니다).

패키지가 다른 패키지를 쓰면 그것까지 받습니다. 같은 이름을 서로 다른 곳에서 받으려 하면 알려 줍니다.

**이름만으로 받기 — 패키지 목록**

패키지 목록(레지스트리)에 올라간 패키지는 주소 없이 이름만으로 받습니다. 파이썬의 `pip install` 과 같습니다.

```
siskin search color            # 목록에서 찾기
siskin add colors              # 목록에서 주소를 찾아 받기
```

목록은 서버가 아니라 **GitHub 저장소 하나**입니다. 그 안의 `packages/이름.toml` 파일 하나가 패키지 하나이고,
안에는 이것만 적혀 있습니다.

```toml
git = "https://github.com/누군가/siskin-colors"
description = "터미널 글자에 색 입히기"
```

기본 목록은 `https://github.com/Haru-neo/siskin-registry` 입니다(2026-09-24 공개. 다른 목록을 쓰려면 아래 "다른 목록 쓰기" 를 봅니다).
`siskin add colors` 는 목록을 `~/.siskin/registry/` 에 받아 두고(쓸 때마다 새로 받음), 거기서 주소를 찾아
`siskin.toml` 에 `colors = { git = "..." }` 로 적습니다. 그다음은 주소를 직접 준 것과 똑같습니다.
Rust 의 crates.io-index 와 맥의 Homebrew 도 이렇게 git 저장소로 목록을 둡니다. 서버를 돌리지 않아서 돈이 들지 않고,
누가 무엇을 바꿨는지 git 기록에 모두 남습니다.

**내 패키지 올리기.** 프로젝트를 GitHub 에 올리고(뿌리에 `lib.skn` 가 있어야 합니다) `siskin publish` 를 치면,
목록 저장소에 더할 파일 내용을 보여 줍니다. 그 파일 하나를 더하는 PR 을 보내고, 합쳐지면 누구나 `siskin add 이름` 으로 받습니다.
새 판은 내 저장소에 git 태그만 올리면 됩니다(`siskin add 이름 --rev v1.1`, `siskin update`).
`siskin.toml` 의 `[package]` 에 `description = "한 줄 설명"` 을 넣으면 `siskin search` 에 보입니다.

**다른 목록 쓰기.** 회사 안에서만 쓰는 목록처럼 다른 곳을 가리키려면 둘 중 하나를 씁니다.

```
SISKIN_REGISTRY=https://github.com/우리회사/siskin-registry siskin add 사내도구
```

```toml
[registry]
url = "../우리-목록"          # siskin.toml 에. git 주소나 내 컴퓨터의 폴더
```

목록이 내 컴퓨터의 폴더면 `siskin publish` 가 파일을 바로 적어 줍니다. 인터넷이 끊겨도 전에 받아 둔 목록으로 찾습니다.
