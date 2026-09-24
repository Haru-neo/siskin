# Siskin 언어 설계 문서 v0.1

> 이름 **Siskin**(검은머리방울새) · 확장자 `.skn` · 2026-09-19 작성, 2026-09-24 이름 확정
> 작고 재빠른 새 이름입니다. 처음에는 임시로 Mica(운모)라고 불렀지만, 같은 이름의 스크립트 언어가 이미 있어서 2026-09-24에 Siskin으로 정했습니다.

---

## 0. 한 줄 정의

**Siskin은 파이썬처럼 읽히고, C만큼 빠르며, C++처럼 메모리를 직접 다룰 수 있고, 컴파일러가 사람뿐 아니라 기계에게도 말을 거는 정적 타입 시스템 언어입니다.**

---

## 1. 요구사항 → 설계 목표

| 사용자 요구 | 설계로 번역하면 | 핵심 수단 |
|---|---|---|
| 파이썬처럼 쉬울 것 | 상용구 최소, 들여쓰기 블록, 지역 타입 추론, 라이프타임 표기 없음 | 문법 §4, 타입 §5 |
| C++처럼 메모리 접근이 자유로울 것 | 원시 포인터·포인터 연산·수동 할당/해제·C ABI를 1급으로 제공 | 메모리 3계층 §6 |
| C처럼 빠를 것 | GC 없음, AOT 네이티브 컴파일, 단형화, 값 의미론, 제로코스트 추상화 | 성능 §7 |
| AI에게 친화적일 것 | 모호성 없는 문법, 구조화된 진단, 구조적 편집 API, 내장 계약/테스트 | AI 친화 §9 |
| 사람에게도 친화적일 것 | 읽기 우선, 방법은 하나, 숨은 제어 흐름 없음 | 원칙 §3 |

### 1.1 가장 큰 긴장 관계

"파이썬처럼 쉬움"과 "C처럼 빠름 + C++처럼 자유로움"은 원래 양립하지 않습니다. 기존 언어들의 선택은 이랬습니다.

- 파이썬: 쉬움을 얻고 속도와 메모리 제어를 포기
- C/C++: 속도와 제어를 얻고 안전성과 쉬움을 포기
- Rust: 속도·제어·안전을 얻고 **쉬움**을 포기 (빌림 검사기의 인지 부하)
- Go: 쉬움과 속도를 어느 정도 얻고 **GC 일시정지와 메모리 제어**를 포기

**Siskin의 답: 하나를 고르지 않고 층으로 나눕니다.** 기본층은 파이썬만큼 쉽고, 필요한 지점에서 한 단어로 아래층을 열어 C++의 자유를 씁니다. 층을 내려가는 행위가 코드에 **항상 눈에 보이게** 남는다는 것이 이 설계의 전부입니다.

---

## 2. 선행 언어에서 취한 것과 버린 것

| 언어 | 취할 것 | 버릴 것 |
|---|---|---|
| **Python** | 들여쓰기 문법, 가독성, 표준 라이브러리 설계 감각 | 동적 타입, GIL, 인터프리터 속도 |
| **Mojo** | 파이썬 문법 위에 시스템 기능을 얹는 전략, `owned`/`inout` 인자 규약, `comptime` | CPython 상위호환이라는 제약(레거시를 통째로 떠안음) |
| **Rust** | 소유권/이동 개념, 값으로서의 에러, 카고급 툴링 | 명시적 라이프타임 표기, 빌림 검사기 노출, 문법 복잡도 |
| **Hylo** | 가변 값 의미론(mutable value semantics) — 라이프타임 표기 없이 안전 확보 | 아직 연구 단계인 일반화 수준 |
| **Vale** | 세대 참조(generational references)로 use-after-free를 저비용 검출 | 실험적 런타임 |
| **Koka / Lobster** | Perceus식 컴파일타임 참조 카운트 소거 및 재사용 | 순수 함수형 제약 |
| **Zig** | 명시적 할당자(allocator) 전달, `comptime`, 숨은 제어 흐름 없음 | 수동 메모리를 유일한 선택지로 두는 것 |
| **Go** | 구조적 동시성 감각, 빠른 컴파일, 단일 포매터 | GC, 인터페이스 암묵 구현 |

참고 자료는 부록 B에 정리했습니다(전부 영문 1차 자료).

---

## 3. 설계 원칙 5가지

1. **읽는 사람이 최우선.** 코드는 쓰이는 횟수보다 읽히는 횟수가 훨씬 많습니다. 짧음보다 명확함을 고릅니다.
2. **안전이 기본값, 자유는 명시적 한 단어.** 위험한 일은 금지하지 않지만 반드시 `unsafe` 같은 표식을 남깁니다.
3. **방법은 하나.** 같은 일을 하는 두 가지 문법을 두지 않습니다. 사람에게는 논쟁이 줄고, AI에게는 선택지가 줄어 정확도가 오릅니다.
4. **지역 추론 가능성(local reasoning).** 함수 하나만 읽고 그 함수가 하는 일을 전부 알 수 있어야 합니다. 예외, 암묵적 형변환, 코드 생성 매크로, 연산자 임의 재정의를 두지 않는 이유입니다.
5. **컴파일러는 API다.** 진단, 포맷, 편집, 설명이 전부 기계가 읽을 수 있는 형태로 나옵니다. 툴링은 나중에 붙이는 것이 아니라 언어의 일부입니다.

---

## 4. 문법

### 4.1 블록 구분 — 들여쓰기 (결정)

파이썬식 들여쓰기를 채택합니다. 중괄호는 받지 않습니다.

- **근거**: "파이썬처럼 쉬울 것"이 1번 요구사항이고, 들여쓰기는 시각적 구조와 논리적 구조가 항상 일치합니다.
- **알려진 위험**: LLM이 코드를 부분 수정할 때 들여쓰기를 깨뜨리기 쉽습니다.
- **완화책**: 정본 포매터 `siskin fmt`가 항상 복구하고, 구조적 편집 API(§9.3)로 AI는 텍스트가 아니라 AST 단위로 수정합니다. 또한 파서가 들여쓰기 오류를 만나면 "몇 칸이어야 하는지"를 fix-it으로 정확히 제시합니다.
- 규칙: 공백 4칸 고정. 탭 금지. 블록은 `:` 으로 엽니다.

### 4.2 기본 예제

```siskin
# hello.skn
fn main():
    print("안녕, Siskin!\n")
```

`print`는 프렐류드(`std.prelude`)에 있어 임포트 없이 쓰입니다. 프렐류드는 `print`, `len`, `range`, `str`, `int`, `float`, `error`, `assert` 정도로 좁게 유지합니다. 그 밖의 것은 전부 명시적 임포트가 필요합니다(§9.7).

**`print`는 받은 것만 그대로 출력하고 아무것도 덧붙이지 않습니다.** 줄바꿈은 C/C++처럼 문자열 안에 `\n`으로 직접 씁니다.

```siskin
print("안녕\n")          # 줄바꿈
print("안녕")            # 줄바꿈 없음
print("a", "b\n")        # "ab" 뒤에 줄바꿈 (구분자도 덧붙이지 않습니다)
```

`print`와 `println`을 둘 다 두지 않는 이유는 원칙 3(방법은 하나) 때문이고, 줄바꿈을 자동으로 붙이지 않는 이유는 원칙 4(숨은 것 없음) 때문입니다. 대신 가장 흔한 한 줄이 파이썬보다 세 글자 길어지는 비용을 치릅니다. 이것은 의도한 교환입니다.

이스케이프는 C 계열과 같습니다: `\n` 줄바꿈, `\t` 탭, `\\` 역슬래시, `\"` 따옴표. 슬래시(`/`)가 아니라 역슬래시(`\`)입니다.

```siskin
fn sum(xs: [Int]) -> Int:
    var total = 0
    for x in xs:
        total += x
    return total
```

- `let` = 불변 바인딩(기본으로 이걸 씁니다), `var` = 가변 바인딩
- 함수 시그니처는 타입 표기 **필수**, 지역 변수는 **추론**
  - 근거: 계약이 드러나는 곳은 함수 경계입니다. 사람도 AI도 거기만 보면 되게 만듭니다. 내부는 파이썬처럼 가볍게 씁니다.

### 4.3 구조체와 인터페이스

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

- 상속은 없습니다. 인터페이스 구현과 합성만 있습니다.
- 인터페이스 구현은 **명시적**입니다(`struct Vec2(Printable)`). Go식 암묵 구현은 "이게 왜 이 인터페이스를 만족하지?"를 전역 검색해야 해서 지역 추론을 깨뜨립니다.

### 4.4 Optional과 에러 — null도 예외도 없습니다

```siskin
fn find(users: [User], id: Int) -> ?User:     # ?T = 값이 없을 수 있음
    for u in users:
        if u.id == id:
            return u
    return none

fn read_config(path: Str) -> !Config:         # !T = 실패할 수 있음
    let text = try fs.read_text(path)         # try = 실패 시 즉시 전파
    return Config.parse(text)

# 호출부
let cfg = read_config("app.toml") catch e:
    print(f"설정 로드 실패: {e}\n")
    return
```

- **null 없음.** 값이 없을 가능성은 타입 `?T`에 나타나고, 컴파일러가 처리를 강제합니다.
- **예외 없음.** 에러는 반환값이고 `!T`로 시그니처에 드러납니다. 눈에 보이지 않는 제어 흐름을 만들지 않습니다.
- `try`는 전파, `catch`는 처리. 두 개뿐입니다.

### 4.5 패턴 매칭

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

`match`는 **전수 검사**됩니다. 빠뜨린 케이스가 있으면 컴파일 에러이고, 빠진 케이스 목록이 진단에 나옵니다.

### 4.6 제네릭 — 대괄호

```siskin
fn max[T: Ord](a: T, b: T) -> T:
    return a if a > b else b

struct Stack[T]:
    items: [T]
```

`<>` 대신 `[]`를 씁니다. `<`가 비교 연산자와 충돌해 파싱이 문맥 의존이 되는 것을 피하기 위해서입니다(C++/Rust의 고전적 문제). 문법을 LL(1)에 가깝게 유지하는 것이 AI 생성 정확도에 직접 기여합니다.

### 4.7 키워드 (34개)

```
fn let var if elif else for while in break continue return
struct enum interface match case
import from as pub
try catch none true false
and or not
owned inout unsafe with comptime
```

`requires`, `ensures`, `self`, `arena`는 문맥 키워드라 이 목록에 없습니다. 키워드가 적으면 사람이 외울 것이 적고, AI가 헷갈릴 것도 적습니다.

### 4.8 대소문자 완화

C 계열은 `myValue`와 `myvalue`를 완전히 다른 이름으로 봅니다. 사람에게는
기억 부담이고, 실수의 흔한 원인입니다. Siskin은 규칙 하나로 완화합니다.

> **정확히 쓴 이름이 있으면 그게 이긴다. 없을 때만, 대소문자만 다른 이름이
> 정확히 하나면 거기에 붙인다. 후보가 둘 이상이면 고치지 않는다.**

- `struct User`와 `let user`는 둘 다 정확한 이름이므로 서로 간섭하지 않습니다.
  전면 대소문자 무시(구식 BASIC, SQL)였다면 이 흔한 조합이 깨집니다.
- `Str` 타입과 `str()` 함수도 같은 이유로 안전하게 공존합니다.
- 고쳐진 자리는 `siskin check`가 정본 철자와 함께 보고하고, `siskin fmt`가 파일을
  정본 철자로 수렴시킵니다. 그래서 검색도 AI도 한 가지 철자만 보게 됩니다.
- C FFI로 가져온 이름은 예외입니다. C는 대소문자를 구분하므로 정확해야 합니다.

즉 **사람에게는 관대하고, 저장된 코드는 한 가지 철자로 수렴합니다.**
원칙 3(방법은 하나)을 어기지 않으면서 원칙 1(읽는 사람이 최우선)을 지키는 지점입니다.

---

## 5. 타입 시스템

- **정적 타입, 단형화(monomorphization).** 런타임 타입 조회 없음, 가상 호출 없음(명시적 `dyn` 사용 시에만).
- **값 의미론 기본.** 구조체는 값입니다. 복사·이동이 기본이고, 참조는 함수 인자 규약으로만 드러납니다.
- **인자 규약 3가지** (Hylo/Mojo에서 차용, 라이프타임 표기는 없습니다):

| 표기 | 의미 | C++ 대응 |
|---|---|---|
| `fn f(x: T)` | 읽기 전용 빌림 (기본) | `const T&` |
| `fn f(inout x: T)` | 가변 빌림 | `T&` |
| `fn f(owned x: T)` | 소유권 이전 | `T&&` |

Rust와 달리 `'a` 같은 라이프타임을 **쓰지 않습니다.** 참조는 호출 구간을 넘어 살아남을 수 없다는 규칙 하나로 대부분의 경우를 덮고, 그 규칙을 넘어서야 하는 경우는 §6의 아래층으로 내려갑니다. 이것이 "쉬움"을 지키는 핵심 트레이드오프입니다.

- **암묵적 형변환 없음.** `Int` → `Float`도 명시적으로 씁니다.
- **컴파일타임 실행** `comptime`: 상수 평가, 제네릭 특수화, 테이블 생성에 사용합니다.

---

## 6. 메모리 모델 — 3계층 (이 언어의 핵심)

```
┌─ Level 0 : safe     기본값. 아무 표기 없이 그냥 쓰면 여기.
│                     값 의미론 + 이동 + 컴파일타임 참조 카운트 소거.
│                     파이썬처럼 쓰는데 GC가 없습니다.
│
├─ Level 1 : region   `with arena` 블록. 직접 할당, 블록 끝에서 일괄 해제.
│                     게임 루프, 요청 단위 서버, 실시간 처리용.
│
└─ Level 2 : unsafe   `unsafe` 블록. 원시 포인터, 포인터 연산,
                      재해석 캐스트, malloc/free, C ABI. C++과 동일한 자유.
```

### 6.1 Level 0 — safe (기본)

```siskin
fn main():
    let names = ["하루", "Siskin"]      # 힙 할당, 자동 해제
    for n in names:
        print(f"{n}\n")              # 참조 카운트 증감은 컴파일타임에 제거됨
```

- 참조 카운팅을 쓰되, **Perceus 방식의 컴파일타임 소거와 재사용**을 적용합니다. 소유권이 정적으로 추적되는 구간에서는 RC 연산이 코드에 아예 남지 않고, 해제 직후 같은 크기의 할당은 메모리를 재사용합니다.
- 순환 참조는 RC의 알려진 약점입니다. `Weak[T]`를 제공하고, 컴파일러가 순환 가능성이 있는 자기 참조 타입에 경고를 냅니다.
- GC 없음 → 일시정지 없음, 해제 시점 결정적.

### 6.2 Level 1 — region / arena

```siskin
fn render_frame(world: World):
    with arena frame:                       # 프레임 전용 아레나
        let batch = frame.list[Sprite]()    # 아레나가 대는 리스트
        for e in world.entities:
            batch.push(e.sprite())
        gpu.submit(batch)
    # 블록을 벗어나는 순간 전부 한 번에 해제. 개별 free 없음.
```

아레나가 주는 것은 두 가지입니다.

- `a.list[T]()` — 늘어나는 리스트. 쓰는 법은 보통 리스트와 똑같고, 해제 시점만 다릅니다. `unsafe`가 필요 없습니다.
- `a.alloc[T](n)` — 칸 n개짜리 원시 메모리. 원시 포인터이므로 `unsafe:` 안이어야 하고, `free`는 부르지 않습니다.

규칙:

- 아레나에서 나온 값은 아레나 블록 밖으로 나갈 수 없습니다. 컴파일러가 막습니다(`T0040`).
- `return`이나 `break`로 블록을 빠져나가도 해제는 반드시 일어납니다.
- 할당 비용이 포인터 증가 하나로 떨어집니다. 측정 결과 개별 할당/해제보다 4.1배 빠르고, 손으로 쓴 C 아레나와 같은 속도입니다([bench/](bench/README.md)).
- **구현 상태:** ✅ 동작합니다. Zig식 명시적 할당자 전달(`fn parse(a: Allocator, ...)`)은 아직입니다.

### 6.3 Level 2 — unsafe

```siskin
unsafe:
    let p: *Int = alloc[Int](16)            # 원시 포인터
    p[0] = 42
    let q = p + 8                           # 포인터 연산
    free(p)
```

- 포인터 산술, 수동 수명 관리, 직접 메모리 레이아웃 제어가 됩니다.
- **어디서 했는지가 항상 코드에 남습니다.** 원시 포인터를 `unsafe:` 밖에서 만지면 컴파일이 막습니다(`T0041`).
- **구현 상태:** `alloc` / `free` / 인덱싱 / 포인터 산술 ✅ 동작합니다.
  `extern "C"` C FFI ✅ 동작합니다(§9.5).
  재해석 캐스트 `cast[*Byte](p)`, `siskin check --unsafe-report`,
  매니페스트의 `unsafe = "deny"`는 아직입니다.

### 6.4 차별점 — 디버그 빌드에서 원시 포인터도 검사합니다

**✅ 구현 완료.** 디버그 빌드(기본값)에서는 Level 2의 할당마다 **세대 번호(generational reference)** 를 붙이고, 접근할 때마다 세대와 범위를 확인합니다. 잡히는 것:

| 실수 | 진단 |
|---|---|
| 해제 후 접근 | `이미 해제된 메모리에 접근했습니다 (use-after-free)` |
| 범위를 벗어난 접근 | `포인터 접근이 범위를 벗어납니다 (위치 N, 크기 M바이트)` |
| 두 번 해제 | `이미 해제한 메모리를 또 해제했습니다` |
| 아레나 블록 밖에서 접근 | use-after-free로 잡힘 |

`--release`에서는 세대도 검사도 전부 사라지고 포인터가 그냥 C 포인터가 됩니다. **오버헤드 0**이고, 아레나 속도가 손으로 쓴 C와 같습니다.

**대가:** 디버그에서 해제한 메모리를 실제로 OS에 돌려주지 않고 붙들어 둡니다. "이 메모리는 죽었다"는 표시를 읽어야 검사가 가능한데, 돌려준 메모리를 읽는 것 자체가 위험한 접근이기 때문입니다. 그래서 디버그 빌드는 메모리를 더 씁니다. 릴리스에서는 정상적으로 돌려줍니다.

즉 개발 중에는 Rust에 가까운 안전망을, 배포 시에는 C와 동일한 코드를 얻습니다. Vale의 세대 참조 아이디어를 "디버그 전용"으로 좁혀 성능 비용 없이 가져오는 설계입니다.

---

## 7. 성능 — C급 속도를 어떻게 달성하는가

목표: **C 대비 1.0~1.1배** (동일 알고리즘, 마이크로벤치 기준).

| 수단 | 효과 |
|---|---|
| AOT 네이티브 컴파일 (LLVM 백엔드) | 인터프리터·JIT 워밍업 없음 |
| GC 없음 | 일시정지 없음, 예측 가능한 지연시간 |
| 정적 타입 + 단형화 | 동적 디스패치·박싱·타입 조회 없음 |
| 값 의미론 | 기본적으로 스택/인라인 배치, 힙 할당은 명시적으로만 |
| Perceus식 RC 소거 + 재사용 | 안전 계층의 참조 카운트 비용 대부분이 컴파일타임에 사라짐 |
| 제로코스트 추상화 | 인터페이스·제네릭·이터레이터가 전부 인라인 후 소멸 |
| C ABI 그대로 사용 | FFI 오버헤드 0, 기존 C 라이브러리 즉시 활용 |
| `comptime` | 런타임 계산을 컴파일타임으로 이동 |
| SIMD 내장 타입 (`Simd[Float, 8]`) | 벡터화를 언어 수준에서 표현 |

**정직한 한계:** Level 0에서 소유권이 정적으로 추적되지 않는 패턴(공유 그래프, 관찰자 패턴 등)에서는 참조 카운트 연산이 일부 남고, 그만큼 C보다 느립니다. 그런 핫패스는 Level 1(아레나)이나 Level 2(원시 포인터)로 내려가면 C와 동일해집니다. **"기본은 쉽고 충분히 빠르며, 마지막 10%는 층을 내려가서 얻는다"** — 이것이 이 언어가 파는 거래입니다.

경계 검사는 기본 켜짐이고, 검증된 핫루프에서 `unsafe` 인덱싱으로 끌 수 있습니다.

---

## 8. 동시성

- **구조적 동시성**: 작업은 블록을 벗어나 살아남지 못합니다.

```siskin
with nursery n:
    n.spawn(fetch("a.com"))
    n.spawn(fetch("b.com"))
# 블록을 벗어날 때 두 작업 모두 완료 보장. 누수되는 백그라운드 작업 없음.
```

- **함수 색깔 문제 없음**: `async`/`await`로 함수를 둘로 쪼개지 않습니다. 경량 스레드 위에서 모든 함수가 그냥 블로킹처럼 쓰입니다(Go 방식).
- **데이터 레이스는 타입으로 차단**: 스레드 간에 넘길 수 있는 것은 소유권을 이전한 값이거나 `Shared[T]`(내부적으로 원자적 RC + 잠금)뿐입니다.

**구현 상태 (2026-09-24):** 첫 버전은 더 단순하게 갔습니다.
- `spawn f(x)` 는 호출 하나를 새 OS 스레드에서 돌리고 `Task[T]` 를 줍니다. `t.wait()` 로 결과를 받습니다.
  경량 스레드가 아니라 OS 스레드이고, `with nursery` 대신 **main 이 끝날 때 남은 작업을 모두 기다립니다.**
- 작업에 넘기는 값은 **복사본**입니다(클로저의 붙잡기와 같은 규칙). 바깥 변수를 작업 안에서 바꿀 수 없고,
  원시 포인터·아레나·`Json` 은 넘길 수 없습니다. 그래서 공유 메모리가 없고 `Shared[T]` 도 아직 필요 없습니다.
- 작업 사이는 `channel[T]()` / `channel[T](n)` 통로로 주고받습니다(`send`, `recv() -> ?T`, `close`, `for x in ch`).
- 모든 작업이 서로를 기다리면 교착(E0260) 실행 오류로 알립니다. 두 백엔드가 같은 규칙입니다.
- 두 백엔드 모두 진짜 병렬입니다. `siskin build` 는 pthread, `siskin run` 은 작업마다 운영체제 스레드와 인터프리터 하나씩(큰 자물쇠 없음). 인터프리터 값은 한 스레드 전용(`Rc`)이지만 작업에 넘기는 값·결과·통로 값은 보내는 쪽이 통째로 새로 만들어(`Value::detach`) 넘기므로 두 스레드가 같은 값을 만지지 않습니다. 문법 나무는 `Arc` 로 같이 읽습니다.
  결과는 같고 속도만 다릅니다. 기다리는 동안(통로, `sleep`, `input`)은 자물쇠를 내려놓습니다.

---

## 9. AI 친화 설계 — 구체적으로 무엇을 하는가

"AI 친화적"을 막연한 구호가 아니라 검증 가능한 기능 8개로 정의합니다.

### 9.1 모호성 없는 문법
LL(1)에 가까운 문법, `[]` 제네릭, 사용자 정의 연산자 없음, 코드 생성 매크로 없음, 키워드 34개. → 생성된 코드가 파싱 실패하는 비율을 구조적으로 낮춥니다.

### 9.2 기계가 읽는 진단
```
$ siskin check --json
{"code":"E0142","severity":"error","file":"main.skn","span":[12,5,12,18],
 "message":"`?User`를 `User`로 바로 쓸 수 없습니다",
 "fixes":[{"label":"none 처리 추가","edit":"if let u = find(...):"}],
 "explain":"siskin explain E0142"}
```
모든 에러에 **안정된 코드**와 **적용 가능한 수정안**이 붙습니다. AI 에이전트가 에러 메시지를 자연어로 해석할 필요가 없어집니다.

### 9.3 구조적 편집 API
```
$ siskin edit --at "app::server::handle_request" --replace-body body.skn
$ siskin symbols --json          # 프로젝트 전체 심볼 트리
```
AI가 텍스트 diff가 아니라 **AST 노드 단위**로 코드를 고칩니다. 들여쓰기를 깨뜨리거나 엉뚱한 줄을 덮어쓰는 사고가 원천적으로 사라집니다. (§4.1의 들여쓰기 위험에 대한 직접적 대응입니다.)

### 9.4 정본 포매터
`siskin fmt`는 옵션이 없습니다. 모든 코드가 한 가지 모양으로 수렴하므로 diff가 안정되고, 스타일 논쟁이 사라지고, AI가 스타일을 추측할 필요가 없습니다.

### 9.5 내장 계약
```siskin
fn div(a: Int, b: Int) -> Int:
    requires b != 0
    ensures result * b <= a
    return a / b
```
디버그 빌드에서 검사, 릴리스에서 제거. AI가 "의도"를 코드에 남길 수 있고, 생성한 코드를 스스로 검증하는 루프가 가능해집니다.

### 9.6 내장 doctest
```siskin
fn slug(s: Str) -> Str:
    """
    >>> slug("Hello World")
    "hello-world"
    """
```
`siskin test`가 문서의 예제를 그대로 실행합니다. 문서와 테스트와 명세가 한 곳에 있습니다.

### 9.7 숨은 것 없음
예외 없음, 암묵적 형변환 없음, 암묵적 전역 없음, 와일드카드 임포트 없음(`from std.fs import read_text` 형태만). 함수 하나를 읽으면 그 함수가 하는 일이 전부입니다. → AI가 필요로 하는 컨텍스트 창이 작아지고, 사람이 리뷰할 때 읽을 범위도 작아집니다.

### 9.8 에디션 기반 안정성
언어 변경은 에디션 단위로만 일어나고 `siskin migrate`가 자동 변환합니다. 학습 데이터의 낡은 문법 때문에 AI가 헛짚는 문제를 줄입니다.

---

## 9.5 라이브러리 생태계 — 새 언어의 가장 큰 약점

새 언어에는 남이 만들어 둔 것이 없습니다. 이것이 새 언어가 죽는 가장 흔한 이유입니다.
Siskin의 답은 **처음부터 C 세계 전체를 그대로 쓰는 것**입니다.

Siskin은 C로 번역된 뒤 컴파일됩니다. 그래서 C 라이브러리를 **변환 계층 없이**
직접 부릅니다. 호출 비용이 0입니다.

```siskin
extern "C" fn sqlite3_open(path: *Byte, db: **Byte) -> Int
```

이것이 실질적으로 무엇을 뜻하냐면, 파이썬에서 유용한 라이브러리 대부분은
사실 **C로 짜여 있고 파이썬은 껍데기만 씌운 것**입니다. numpy의 계산 부분,
pillow의 이미지 처리, cryptography의 암호, lxml의 파싱이 전부 C입니다.
Siskin은 그 알맹이를 껍데기 없이 직접 씁니다. 파이썬 코드를 옮겨오는 것은
방향이 반대입니다. 느린 껍데기를 옮기는 일이 되니까요.

그래서 우선순위는 이렇습니다.

1. **C FFI** (`extern "C"`) — ✅ 완료. zlib `crc32`를 불러 파이썬 `zlib.crc32`와
   같은 값이 나오는 것을 확인했습니다. 이것만 되면 SQLite, OpenSSL, SDL, ffmpeg,
   BLAS 같은 수십 년치 자산이 열립니다.
   - 주고받는 타입: `Int`(포인터 손잡이 포함), `Float`, `Bool`, `Str`(`const char*`), `*T`, 반환 없음.
   - `extern "C" link "z"` 한 줄로 링크까지 처리합니다.
   - 한계: 인터프리터(`siskin run`)에서는 부를 수 없습니다. 실제 링크가 필요하기 때문입니다.
     구조체·콜백·가변인자는 아직입니다.
2. **좁고 좋은 표준 라이브러리** — ✅ 완료. 수학, 난수, 시간, 파일, 리스트, 문자열,
   **정규식**(`std.re`), **JSON**(`std.json`). 정규식 엔진과 JSON 읽기/쓰기는 바깥
   라이브러리 없이 직접 만들었고, 같은 알고리즘을 인터프리터와 C 양쪽에 두어
   결과가 같습니다. 여기까지가 v0.1 표준 라이브러리이고 더 키우지 않습니다.
3. **패키지 관리자** — 나중에. 초기에는 표준 라이브러리와 C FFI로 버팁니다.

정직하게 말하면, 성숙한 생태계는 몇 년이 걸립니다. C FFI는 그 몇 년을
견디게 해주는 장치이지, 생태계를 대신하지는 않습니다.

---

## 10. 비목표 (v0에서 하지 않는 것)

- 예외 처리, 클래스 상속, 사용자 정의 연산자, 코드 생성 매크로
- 가비지 컬렉션
- 파이썬 소스 코드 호환(문법 계열만 빌리고 런타임 호환은 시도하지 않습니다. Mojo가 치르는 비용을 피합니다.)
- 효과 시스템(effect system) — 매력적이지만 v0의 "쉬움"과 충돌합니다. v2 검토.

---

## 11. 구현 로드맵

| 단계 | 내용 | 산출물 |
|---|---|---|
| **P0** | 사양서 v0.1 (이 문서) + EBNF 문법 정의 | `DESIGN-v0.1.md`, `grammar.ebnf` |
| **P1** | 트리워킹 인터프리터 — 문법을 실제로 타이핑해 보며 검증 | ✅ 완료. Rust 4,100줄. `micai/` 참고 |
| **P2** | 정적 타입 검사기 + `?T`/`!T`/패턴 매칭 전수 검사 | ✅ 완료. `siskin check` |
| **P3** | 네이티브 백엔드 (C 경유로 시작 → LLVM) | ✅ 완료. `siskin build`, C 대비 1.0~1.2배, 예제 전부 통과 |
| **P4** | 메모리 Level 1/2, C FFI, 세대 검사 디버그 모드 | ✅ 완료. 아레나·원시 포인터·세대 검사·C FFI 전부 동작 |
| **P5** | 툴링: `fmt`, `--json` 진단, `edit` API, LSP | AI 친화 기능 실사용 |
| **P6** | 표준 라이브러리 + 벤치마크(C 대비 측정) | ✅ 완료. 수학·난수·시간·파일·리스트·문자열·사전·정규식·JSON, [bench/](bench/README.md) |

**P1이 가장 중요합니다.** 문법은 종이 위에서 좋아 보여도 실제로 200줄쯤 써 보면 반드시 어색한 곳이 나옵니다. 인터프리터를 빨리 만들어 직접 써 보고 문법을 고치는 것이 옳은 순서입니다.

---

## 12. 결정이 필요한 사항

1. **구현 언어** — P1 인터프리터를 무엇으로 쓸지. (추천: Rust. 나중에 P3 백엔드와 툴링까지 그대로 재사용 가능하고, Cranelift/LLVM 바인딩이 좋습니다. 대안: Python으로 빠르게 프로토타입 후 폐기.)
2. **언어 이름** — ✅ 2026-09-24 `Siskin`(확장자 `.skn`, 명령어 `siskin`)으로 확정. 예전 임시 이름은 Mica(`.mi`)였습니다.
3. **순환 참조 처리** — `Weak[T]` 수동 관리로 충분한지, 아니면 순환 검출기를 옵션으로 둘지. (추천: v0는 `Weak[T]`만.)

---

## 부록 A. 종합 샘플

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
        print(f"읽기 실패: {e}\n")
        return
    print(f"단어 {n}개\n")
```

같은 프로그램의 성능 버전 — 아레나로 할당을 한 번에 처리:

```siskin
fn count_words_fast(path: Str) -> !Int:
    with arena a:
        let src = try read_text_into(a, path)
        var n = 0
        for word in src.split(" "):
            if word != "":
                n += 1
        return n
    # 파싱에 쓴 메모리 전부 여기서 한 번에 해제
```

---

## 부록 B. 참고 자료 (영문 1차 자료)

- Perceus: Garbage Free Reference Counting with Reuse (PLDI 2021) — https://xnning.github.io/papers/perceus.pdf
- Optimizing Reference Counting with Borrowing (Lorenzen) — https://antonlorenzen.de/papers/master_thesis_perceus_borrowing.pdf
- Borrow checking, RC, GC, and the Eleven Other Memory Safety Approaches (Vale) — https://verdagon.dev/grimoire/grimoire
- Hylo — mutable value semantics — https://github.com/hylo-lang/hylo-lang.github.io/blob/main/index.md
- Move semantics in Rust, C++, and Hylo — https://lukas-prokop.at/articles/2024-11-29-move-semantics-in-rust-cpp-and-hylo
- Ruminating about mutable value semantics — https://www.scattered-thoughts.net/writing/ruminating-about-mutable-value-semantics/
- Mojo (programming language) — https://en.wikipedia.org/wiki/Mojo_(programming_language)
