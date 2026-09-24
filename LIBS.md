# 라이브러리 가져오기

세상에 이미 나와 있는 C·C++ 라이브러리를 Siskin 에서 그대로 씁니다.
함수를 하나씩 손으로 옮겨 적을 필요가 없습니다. 헤더 파일(라이브러리 설명서)
이름만 적으면 그 안에 있는 함수를 Siskin 이 알아서 읽어 옵니다.

```
import c "zlib.h" link "z"
```

이 한 줄이면 zlib 함수 80개가 전부 열립니다.

---

## 1. C 라이브러리

```
import c "헤더이름.h" link "라이브러리이름"
```

- `헤더이름.h` — C 에서 `#include` 할 때 쓰는 그 이름 그대로 적습니다.
- `link "이름"` — 이어 붙일 라이브러리. `-lz` 의 `z` 처럼 앞의 `lib` 과
  뒤의 `.so` 를 뗀 이름입니다. 필요 없으면(예: `string.h`) 생략합니다.

### 예

```
import c "zlib.h" link "z"
import c "string.h"

fn main():
    let s = "hello"
    print(str(crc32(0, s, strlen(s))) + "\n")
```

### 무엇이 열렸는지 보기

```
siskin ffi zlib.h          # 몇 개나 쓸 수 있는지
siskin ffi zlib.h --all    # 함수 이름과 생김새를 전부
```

실제로 재 본 결과입니다.

| 라이브러리 | 바로 쓸 수 있는 함수 |
|---|---|
| zlib (압축) | 81개 중 80개 (98%) |
| sqlite3 (데이터베이스) | 291개 중 283개 (97%) |
| libpng (이미지) | 246개 중 246개 (100%) |
| curses (터미널 화면) | 456개 중 444개 (97%) |
| expat (XML) | 67개 중 66개 (98%) |
| bzip2 | 24개 중 24개 (100%) |

---

## 2. C 타입이 Siskin 타입으로 바뀌는 규칙

| C 쪽 | Siskin 쪽 | 설명 |
|---|---|---|
| `int`, `long`, `size_t`, `uint32_t` … | `Int` | 정수는 크기와 상관없이 전부 `Int` |
| `float`, `double` | `Float` | |
| `_Bool` | `Bool` | |
| `const char *` | `Str` | 읽어 가는 글자열 |
| `char *` (인자 자리) | `Int` | "여기에 써 넣어라"는 뜻이라 주소로 둡니다 |
| `char *` (돌려주는 자리) | `Str` | |
| 그 밖의 포인터 (`FILE*`, `sqlite3*` …) | `Int` | 손잡이. 내용은 안 보고 그대로 주고받습니다 |
| `T **` | `inout Int` | "여기에 결과를 넣어 달라"는 자리 |
| 함수 포인터 | 함수 | 내 함수를 넘겨줄 수 있습니다 |
| `void` | 없음 | |

### 결과를 받아 오는 자리 (`T **`)

C 라이브러리는 결과를 반환값이 아니라 인자로 돌려주는 일이 많습니다.

```c
int sqlite3_open(const char *filename, sqlite3 **ppDb);
```

Siskin 에서는 그냥 `var` 변수를 넘기면 그 자리에 결과가 들어옵니다.

```
var db = 0
if sqlite3_open(":memory:", db) != 0:
    print("열기 실패\n")
```

### 내 함수를 라이브러리에 넘기기 (콜백)

라이브러리가 "일이 생길 때마다 네 함수를 부르겠다"고 하는 경우입니다.
이름을 붙인 함수를 만들고 그 이름을 그대로 넘깁니다.

```
fn 한줄씩(ctx: Int, 칸수: Int, 값들: Int, 이름들: Int) -> Int:
    ...
    return 0

sqlite3_exec(db, "select * from 사람", 한줄씩, 0, err)
```

바깥 변수를 붙잡는 익명 함수는 넘길 수 없습니다. C 쪽에 그런 개념이 없습니다.

### C 가 준 주소에서 값 읽기

콜백이 받은 `값들` 같은 것은 C 가 준 주소입니다. `unsafe:` 안에서 읽습니다.

```
unsafe:
    let 글자 = cstr(주소)            # 0 으로 끝나는 C 글자열을 읽습니다
    let 셋째 = ptr_get(주소, 2)      # 주소가 가리키는 칸의 2번째 값
```

---

## 3. C++ 라이브러리

```
import cpp "헤더이름.hpp" link "라이브러리이름"
```

C++ 은 이름이 안에서 뒤틀려 저장되고(`geo::add` 가 실제로는 `_ZN3geo3addEii`),
클래스·가상 함수·템플릿처럼 C 에 없는 것들이 있어서 그대로는 못 부릅니다.
그래서 Siskin 이 가운데에 다리 놓는 파일을 자동으로 써 주고 C++ 컴파일러에게
같이 넘깁니다. 그러면 뒤틀린 이름도 가상 함수도 C++ 컴파일러가 알아서 처리합니다.

### 이름 붙는 규칙

| C++ | Siskin |
|---|---|
| `geo::add(int,int)` | `geo_add(a, b)` |
| `geo::Circle` 만들기 | `geo_Circle_new(...)` → 손잡이 |
| `geo::Circle` 지우기 | `geo_Circle_delete(손잡이)` |
| `circle.area()` | `geo_Circle_area(손잡이)` |
| `std::string` | `Str` |
| 클래스 참조·포인터 | `Int` (손잡이) |

### 예

```
import cpp "shapes.hpp" from "cpplib" also "cpplib/shapes.cpp"

fn main():
    let 원 = geo_Circle_new(2.0)
    print(str(geo_Circle_area(원)) + "\n")
    print(geo_Circle_kind(원) + "\n")      # std::string 을 돌려주는 함수
    geo_Circle_delete(원)
```

`also "shapes.cpp"` 를 쓰면 라이브러리를 미리 만들어 둘 필요가 없습니다.
그 소스를 같이 컴파일해 줍니다. 이미 만들어진 라이브러리가 있으면
`link "이름"` 을 쓰면 됩니다.

### C++ 에서 되는 것

- 클래스 만들기·지우기, 메서드 부르기
- **가상 함수** — 부모 타입으로 물어봐도 자식이 답합니다
- **헤더 안에만 있는 함수**(inline) — 다리 파일이 실체를 만들어 줍니다
- **템플릿** — 다리에서 실제 타입으로 찍어 내면 됩니다
- `std::string` 주고받기
- 이름공간(namespace)

### C++ 에서 아직 안 되는 것

- **템플릿을 통째로 미리 가져오기** — 템플릿은 쓸 때마다 새로 찍어내는 것이라
  미리 다 가져올 수가 없습니다. 쓸 타입이 정해진 것만 가져옵니다.
- **이름이 같고 인자만 다른 함수(오버로드)** — 첫 번째 것만 가져옵니다.
- **예외(exception)** — 던지면 그대로 프로그램이 멈춥니다.
- `std::vector` 같은 그릇을 값으로 주고받기 — 손잡이로는 됩니다.

---

## 4. `siskin run` 과 `siskin build`

라이브러리를 쓰는 프로그램은 `siskin run` 으로도 그대로 돌아갑니다.
속을 들여다보면 조용히 컴파일해서 돌리는 것이라, `siskin build` 로 만든 것과
결과가 항상 같습니다.

---

## 5. 아직 자동으로 안 가져오는 것 (C)

| 이유 | 예 |
|---|---|
| 인자 개수가 정해지지 않은 함수 | `printf`, `sqlite3_config` |
| 구조체를 통째로 주고받는 함수 | `div()`, `localtime` 계열 일부 |
| Siskin 에 같은 이름이 이미 있는 것 | `abs`, `free`, `pow`, `exit` … Siskin 쪽이 이깁니다 |

`siskin ffi <헤더> ` 를 돌리면 무엇이 왜 빠졌는지 알려 줍니다.

---

## 6. 알아 둘 것

- **헤더를 읽으려면 `clang` 이 필요합니다.** 없으면 `apt install clang`
  (맥이면 `xcode-select --install`). 컴파일 자체는 `cc` 로 합니다.
- 헤더를 읽은 결과는 저장해 둡니다. 두 번째부터는 기다리지 않습니다.
  헤더 파일이 바뀌면 다시 읽습니다.
- 손잡이(`Int`)는 그냥 숫자입니다. Siskin 이 내용을 지켜 주지 않습니다.
  C 라이브러리가 "다 쓰면 닫아라"라고 한 것은 반드시 닫아야 합니다
  (`sqlite3_close`, `geo_Circle_delete` …).
- C 함수가 돌려준 글자열은 Siskin 이 바로 복사해 갑니다. 원본이 사라져도
  괜찮지만, "이건 네가 free 해라"라고 한 것은 그대로 두면 메모리가 샙니다.
