# 에이전트 사용성 시험 — 결과와 피드백 (2026-09-23)

Siskin을 처음 보는 AI 에이전트 8명에게 서로 다른 프로그램을 짜게 했습니다.
6명은 문서(GUIDE·LIBS·README·examples)만 받았고, 2명은 문서 없이 추측과 에러 메시지만으로 짰습니다.
컴파일러 호출은 전부 기록했고, 에이전트가 보고한 버그는 제가 직접 다시 돌려 확인했습니다(✔ 표시).

프로그램과 기록: `trial/` 폴더 (과제별 폴더에 `.skn`, `attempts.log`).

---

## 1. 한눈에

| 과제 | 문서 | 컴파일러 호출 | 첫 성공 실행 | run = build |
|---|---|---|---|---|
| 단어 빈도표 (한글 포함) | 있음 | 37 | 16번째 | 같음 (우회 후) |
| 은행 (구조체·enum·오류) | 있음 | 17 | 6번째 | 같음 (우회 후) |
| JSON 보고서 | 있음 | 20 | 3번째 | 같음 |
| 이진 트리 (메모리 3단계) | 있음 | 34 | 5번째 | 같음 (우회 후) |
| 계산기 REPL | 있음 | 20 | 7번째 | 같음 |
| 성적표 (모듈 2개) | 있음 | 11 | 5번째 | 같음 |
| 재고 관리 | **없음** | 26 | 19번째 | 같음 |
| 성적 통계 | **없음** | 25 | 16번째 | 같음 |

- **8개 모두 완성.** 문서 없는 2명도 끝까지 문서를 열지 않고 에러 메시지만으로 해냈습니다.
- 계산기 300줄은 문법만 맞추자 첫 실행에 바로 정답이었습니다.
- 그런데 **4명이 "run에선 되는데 build에서 깨지거나 답이 다른" 문제를 만났습니다.** 프로젝트의 제1 불변식이 깨진 곳이 아직 있다는 뜻입니다.

---

## 2. 잘 된 것 (에이전트들이 공통으로 칭찬)

- 에러 메시지 형식: 위치·밑줄·도움말. 특히 T0025(`?T` 좁히기), T0045(`var`로 선언), T0011(enum 오타 시 진짜 이름 목록), T0012(match 빠짐), E0210(모듈에 있는 이름 목록), T0059(catch 값).
- `!T` + `try` + `catch e:` — 재귀 파서, 파일 읽기 오류 처리가 깔끔하다는 평.
- 재귀 enum이 상자 없이 그냥 됨. match 빠짐 검사.
- doctest, `siskin fmt`(8명 모두 바꿀 게 없었음), 빠른 빌드, 읽기 쉬운 `--emit-c`.
- 한글: `len`이 글자 수, `s[i]`가 글자, 한글 JSON 왕복, 실수 출력이 두 방식에서 똑같음.
- 인터프리터의 메모리 실수 진단(해제 후 사용, 이중 해제, 범위 초과, 아레나 메모리 해제)은 줄 번호와 도움말까지 훌륭함.
- 수동 메모리 버전은 C 수준 속도: 20만 개 삽입이 `--release`로 0.05초.

---

## 3. run ≠ build — 불변식이 깨진 곳 (전부 ✔ 재현)

| # | 증상 | 최소 재현 |
|---|---|---|
| B1 | 한글 문자열을 `for c in s`로 돌면 build에서 **실행 오류** (바이트 수만큼 돌고 글자 수로 인덱싱) | `for c in "가a": print(c)` |
| B2 | `upper()`/`lower()`가 build에선 영어만 바꿈 | `"café".upper()` → run `CAFÉ`, build `CAFé` |
| B3 | `max(a, b)`/`min`이 build에서 **인자를 두 번 계산** → 부작용 두 번, 재귀면 지수 시간 | `max(f(1), f(2))` 에서 `f(2)`가 두 번 불림 |
| B4 | 리스트 `==`가 run에선 되고 build에선 C 컴파일 오류 | `[1,2] == [1,2]` |
| B5 | `inout self` 메서드 안에서 `self.xs[0].bump()` → build C 오류 | trial/bank/repro/inout_elem.skn |
| B6 | NaN 출력: run `NaN`, build `-nan` | `str(sqrt(-1.0))` |
| B7 | `Str == none`: run은 통과(항상 false), build는 C0011 오류(위치도 1:1) | `let l = input(); l == none` |
| B8 | 아레나 메모리를 `free()`: run은 E0233, 디버그 build는 조용히 통과, release는 강제 종료 | trial/tree/mistakes.skn |
| B9 | 크기 0으로 보이는 파일(`/proc/...`)을 `read_text`하면 build에선 빈 문자열 | `read_text("/proc/version")` |

**덧붙여:** B4·B5 같은 C 컴파일 실패에 "라이브러리를 이어 붙이지 못했습니다, `link` 확인하세요"라는
엉뚱한 안내가 붙습니다(✔). 라이브러리를 안 쓰는 프로그램에서도요. 내부 오류라고 말해야 합니다.

**제안:** "check 통과 + run 성공이면 build도 같은 답"을 보장하는 시험 묶음. 특히 모든 문자열 메서드를 한글·악센트 문자로 두 방식에서 돌려 보기.

---

## 4. 그 밖의 버그 (✔ 재현)

- **`main() -> !Unit`이 오류를 내면 아무 말 없이 종료 코드 0** — 실패를 알 방법이 없음.
- **doctest가 파일의 `import`를 못 봄** — `from std.re import find_all` 쓴 함수의 doctest가 E0222. (3명이 겪음)
- **`siskin check`가 run이 잡는 걸 놓침:** 없는 import(`from std.process import args`), `fn main(args: [Str])`, `list.sort(비교함수)`(E0245). check 통과를 믿은 에이전트가 여러 번 속음.
- **`import std.re` 후 `re.find_all(...)` 가 T0041** — GUIDE 9.7은 모듈째 가져오기가 된다고 씀.
- **import한 파일 안의 오류가 가져온 파일 이름·코드로 표시됨** (줄 번호만 원래 파일 것). trial/grades/repro/
- **`round(x, 1)`이 둘째 인자를 조용히 무시** → 74.333이 74로 나옴.
- **`error(enum값)`이 조용히 글자로 바뀜**, 그리고 문자열에 enum 패턴을 쓰면 "`id`을 찾을 수 없습니다"만 나옴.
- **`case Some(v):`를 `?Int`에 쓰면 조용히 받아들이고** "`v`를 찾을 수 없습니다"만 나옴.
- E0210 도움말의 `std.process` 목록에 `run`, `run_args`가 빠짐(실제로는 있음).
- `input()`이 입력 끝에서 `none` 대신 `""`를 영원히 돌려줌 → 읽기 반복이 멈추지 않음.
- 인터프리터에서 기본(안전) 트리 20만 개 삽입이 사실상 불가능(1만 개에 287초). 값 복사가 깊게 일어남. build는 2.3초.
- 디버그 build의 메모리 오류에 파일·줄이 없음(README는 "어디서" 알려 준다고 함).
- 아레나 탈출 검사(T0040)는 `return`만 봄. 구조체 필드로 빼돌리면 통과.
- `--emit-c -o x.c`가 `x.c.c`를 만듦. `siskin --help`는 check를 "문법만 검사"라고 하지만 타입도 검사함.

---

## 5. 문서에 없는 것 (가장 많이 막힌 곳)

| 빠진 것 | 겪은 에이전트 | 현재 상태 |
|---|---|---|
| **값을 바꾸는 메서드 `inout self`** | 4명 (은행·트리·계산기 + 추측) | 동작함. GUIDE 4장은 "self 앞에 아무것도 없으면 읽기 전용"이라고만 씀. 다들 `mut self`/`var self`를 먼저 시도 |
| **명령줄 인자 `args()`** | 2명 | 동작함. 문서에 없어서 한 명은 환경변수로, 한 명은 `/proc` 파일을 읽는 꼼수로 우회 |
| **튜플, 제네릭, 내 파일 import** | 성적표 | 9/20에 만든 큰 기능인데 GUIDE에 설명이 없음 |
| f-문자열 서식 `{x:.1f}` | 2명 | 동작함, 문서 없음. 한 명은 소수점 출력을 손으로 짬 |
| `x else 기본값`, `a if c else b`, `exit(n)`, `case _:`, 문자열 match | 여러 명 | 동작함, 예제에만 보이거나 아예 없음 |
| 표준 입력 읽기 / `input()`의 끝 동작 | 계산기 | 없음 |
| 오류 값은 항상 `Str`이라는 사실 | 은행·계산기 | 없음 |
| JSON 값 타입 이름 `Json`, JSON 리스트 도는 법, `as_float()`가 정수도 받는지 | JSON | 없음 |
| **GUIDE 9.5.2 아레나 예제가 컴파일 안 됨** (`let xs` 뒤 `push`) | 트리 | `examples/errors/arena_escape.skn`도 같음 |
| 값 복사 비용(언제 복사되는지) | 트리 | 없음 |

---

## 6. 에러 메시지 — 다른 언어에서 온 추측 받아 주기

문서 없는 2명이 가장 많이 시간을 쓴 곳입니다. 대부분 "그런 건 없음"만 나오고 Siskin식 대안을 안 알려 줍니다.

| 써 본 것 | 지금 메시지 | 알려 주면 좋을 것 |
|---|---|---|
| `mut self`, `var self` | "인자 `mut`에 타입이 없음" (엉뚱) | `inout self` |
| `P(a=1)` | "`)`를 기대했는데 `=`" | `P(a: 1)` |
| `Option[T]`, `T?`, `Some(x)`, `None` | 타입/함수 없음 | `?T`, `none` |
| `Result[T,E]`, `Ok`, `Err` | 타입 `result` 없음 (소문자로 바꿔 보여 줌) | `!T`, `error(...)`, `catch` |
| `str`, `list[T]`, `Float(x)` | 타입 없음 | `Str`, `[T]`, `float(x)` |
| `sorted(xs, key=f)`, `append`, `trim` | 함수/메서드 없음 | `sort_by`, `push`, `strip` |
| `Category.Food` | "여기에 `:`를 넣으세요" (틀린 조언) | `Food` |
| `x = 1` (let 없이) | "`x`를 찾을 수 없습니다" | `let`/`var`로 선언 |
| `pass` | "`pass`를 찾을 수 없습니다" | 없음, 빈 블록 쓰는 법 |
| 없는 메서드 (T0036) | "`siskin check`로 확인하세요" (지금 그 명령임) | 비슷한 이름 제안 |
| `!T`에서 필드 꺼내기 (T0026) | 도움말 없음 | `try`/`catch` 안내 |

그 밖에:
- 실수 몇 개가 에러 59개로 불어남(같은 타입 오류를 쓰는 곳마다 반복).
- 타입 오류 위치가 1열로 찍히는 경우가 많음.
- **모든 메시지가 한국어**이고 `--json`도 한국어. 에이전트들은 읽을 수 있었지만, 영어권 사용자·도구를 생각하면 언어 선택(`--lang`, `LANG`)이 필요하다는 의견이 2명.

---

## 7. 없어서 아쉬웠던 기능

- **오류 종류를 enum으로 담기** (`catch e:` 후 `match e:`). 지금은 문자열에 넣었다 꺼내는 꼼수를 씀.
- `eprint`(오류 출력), 문자 코드(`ord`), `is_alpha` 같은 글자 판별.
- `match`를 식으로 쓰기(값 돌려주기).

---

## 8. 고친다면 우선순위 (제안)

1. **run ≠ build 9개(B1~B9)** — 불변식 문제이고, 특히 한글 반복(B1)과 `max` 두 번 계산(B3)은 조용히 틀린 답을 냄.
2. **문서 채우기** — `inout self`, `args()`, 튜플·제네릭·모듈, 서식, `else`/`case _`, 입력, 그리고 컴파일 안 되는 아레나 예제 수정. 코드 변경 없이 가장 많은 막힘을 없앰.
3. **check가 run만큼 잡게**, `main`의 오류는 메시지와 함께 0이 아닌 코드로 끝나게, doctest가 import를 보게.
4. **다른 언어 추측 받아 주는 도움말** (6장 표).
5. 오류 enum, 영어 메시지 선택, 값 복사 비용 줄이기.

---

## 9. 고친 결과 (2026-09-23, 하루 결정 "전부 고치기")

**run ≠ build:** B1~B9 모두 고침. 고치다 찾은 것도 함께 고침: f-문자열 안 계산 순서가 build 에서 거꾸로였던 것, 실수 0 나누기(build 는 inf), 리스트 `reverse`·`contains` 가 build 에 없던 것, `index_of` 가 구조체를 얕게 비교하던 것, 같은 블록에서 이름을 두 번 선언하면 run 은 실행 중 오류·build 는 C 오류였던 것(이제 check 가 T0070 으로 잡음), 값 돌려줄 함수가 끝까지 흘러가면 run `none`·build `0` 이던 것(T0069), `case _` 없는 Str/Int match(T0068).

**그 밖의 버그:** `main -> !Unit` 실패 시 `오류: ...` 와 끝 코드 1, doctest 가 import 를 봄, check 가 없는 import·`main(args)`·`sort(비교함수)`·`round(x, 1)`·`error(enum)` 을 잡음, 모든 표준 모듈을 `import std.X` 로 씀, import 한 파일 오류가 그 파일 이름·줄로 나옴, 아레나 탈출을 구조체 필드로도 잡음, 디버그 build 메모리 오류에 위치, "라이브러리 link" 엉뚱한 안내 → "Siskin 내부 오류", `--emit-c -o x.c`, `input()` 이 끝에서 `none`, `import fs` → `import std.fs` 안내, f-문자열 폭에 변수(`{x:>{w}}`)를 쓰면 조용히 무시하던 것 → 오류 E0143.

**다른 언어 습관 도움말:** `mut self`, `P(a=1)`, `Option`/`Some`/`None`, `Result`/`Ok`/`Err`, `str`/`list[T]`, `sorted`/`append`/`trim`, `Category.Food`, 선언 없는 `x = 1`, `Int?`, `x as Float`, `x is None`, 부를 때 `inout x`. 같은 타입 오류의 연쇄 반복을 줄임. 1열로 찍히던 오류 위치를 그 줄의 이름 자리로 옮김. f 를 빼먹은 `"{x}"` 는 경고 W0001(실행은 막지 않음).

**새로 된 것:** `pass`, `%=`, `eprint`, `for x in json`, 튜플 `p.0`, `for (a, b) in 튜플들`, `x in xs` / `x not in xs`, 리스트·구조체·enum·튜플·사전·`?T` 의 `==`.

**문서:** GUIDE 에 서식(`{x:.2f}` 등), `inout self`, 값 복사, `x else 기본값`, 오류는 늘 Str, `match` 규칙, 튜플·제네릭·파일 나누기, `args()`·`input()`·`exit`, JSON→구조체 예제, 계약을 메서드에 쓰기, doctest 기대값 모양, 기준 두 개 정렬, 기호표 10줄 추가. 아레나 예제 고침. README 메서드 표 맞춤.

**시험:** `tests/agentfix.skn` 추가(run = build). 예제·realworld·tests 전부, 1차 시험 프로그램 8개 모두 run = build.

### 새 에이전트 4명으로 다시 시험 (`trial2/`)

| 과제 | 1차: 호출 / 첫 성공 실행 | 2차: 호출 / 첫 성공 실행 |
|---|---|---|
| 단어 빈도표 | 37 / 16번째 | 15 / **2번째** (실패 0번) |
| 은행 | 17 / 6번째 | 17 / **2번째** (실패 0번, 나머지는 일부러 해 본 시험) |
| 문서 없이 재고 | 26 / 19번째 | 19 / 9번째 (프로그램엔 11번) |
| 문서 없이 성적 | 25 / 16번째 | 18 / 13번째 |

4명 모두 run = build 가 바이트까지 같았고, 문서 없는 2명은 이번에도 문서를 한 번도 열지 않았습니다.
2차에서 새로 나온 것(위 목록의 T0068·T0069·E0143·W0001·`import fs`·`in`·파싱 도움말)도 모두 고쳤습니다.

### 아직 안 한 것
- **영어 메시지:** 모든 진단이 한국어뿐. 2차에서도 문서 없는 2명이 가장 큰 장벽으로 꼽음. 메시지 수백 개를 옮기는 큰 일이고 기본 언어를 정해야 해서 하루 결정이 필요.
- **오류 종류를 enum 으로 담기**(`catch e:` 후 `match e`): 지금은 글자 앞머리로 나눔. 언어 설계 변경.
- 계약 위반 위치가 부른 곳이 아니라 함수 머리를 가리킴. `match` 를 식으로 쓰기, `is_alpha` 같은 글자 판별 함수.

---

## 10. 하루 결정 반영 (2026-09-24, "다 진행해")

**1. 오류 메시지는 영어가 기본, 한국어는 옵션.** `siskin --lang ko ...` 또는 `SISKIN_LANG=ko`.
컴파일러·타입 검사·실행 중 오류·만든 프로그램의 런타임 오류·`siskin fmt`/`test`/`debug`/패키지/LSP 메시지까지 약 1000곳에 영어를 넣었고, 한국어 글은 한 글자도 바꾸지 않았습니다(한국어로 돌린 회귀 결과가 전과 똑같음).
`siskin build` 는 고른 언어를 실행 파일에 넣으므로 같은 언어의 `siskin run` 과 만든 프로그램의 오류 글이 같습니다. `siskin.lock` 머리 주석은 언어와 상관없이 영어로 고정(팀원마다 diff 가 생기지 않게).

**2. 오류 종류를 enum 으로 — `E!T`.**
```siskin
fn withdraw(inout self, amount: Int) -> BankError!Unit:
    if amount > self.balance:
        return error(NoFunds(need: amount - self.balance))
...
acc.withdraw(500) catch e:     # e 는 BankError
    match e:                   # 빠진 종류는 컴파일러가 알려 줌
        case NoFunds(need): ...
```
`!T` 는 그대로 `Str!T`. `try` 는 같은 오류 타입끼리 올리고, enum 오류를 `!T` 함수로 올리면 `NoFunds(430)` 같은 글자로 바꿉니다. 새 검사: T0071(오류 타입은 enum), T0072(다른 오류 타입은 try 로 못 올림), T0073(`error()` 값이 함수의 오류 타입과 다름). `main -> E!Unit` 도 됨. run = build 확인(`tests/errenum.skn`).

**시험:** 예제·realworld·tests 전부 영어·한국어 둘 다 run = build, 1차 시험 프로그램 8개도 두 언어 모두 같음, tools 7/7.

## 11. 남은 빈 곳 채우기 (2026-09-24, 하루 "빈곳 계속 채워")

1. **구조체 필드 좁히기.** `if t.due != none:` 안에서 `t.due` 가 바로 `Str`. 가드(`if t.due == none: return`), `and`/`or` 도 됨. 필드나 그 위를 다시 대입하거나, `inout` 으로 넘기거나, 반복문 안에서 바꾸면 좁히기가 풀림. 틀렸을 때 "`?Str` 이라 먼저 확인하라"는 안내(T0017/T0018). `tests/fieldnarrow.skn`.
2. **`?구조체`/`!구조체` 필드 네이티브.** 자기 자신을 가리키는 필드(`next: ?Node`)는 힙 상자에 담아 복사도 깊게. 덤으로 "구조체를 쓰는 쪽보다 뒤에 선언"·"튜플 필드" 가 C 컴파일 오류 나던 것도 고침(C 타입을 의존 순서대로 냄). `tests/optstruct.skn`, `recstruct.skn`, `structfields.skn`, `structorder.skn`.
3. **동시성.** `spawn f(x)` → `Task[T]`(`wait`, `done`), `channel[T]()`/`channel[T](n)`(`send`, `recv -> ?T`, `close`, `for x in ch`). 넘기는 값은 복사본(공유 메모리 없음 → 데이터 경쟁 없음). 교착은 실행 오류(E0260). build 는 pthread 진짜 병렬(4코어 3.5배), run 도 진짜 병렬(12장). std.net 을 스레드 안전하게 고침 → 연결마다 `spawn` 하는 서버 가능(`tests/net/net_spawn.skn`). `tests/conc.skn`, `examples/15_concurrency.skn`. 새 코드: T0074, T0075, E0162, E0260~E0263.
4. **이름공간.** `import a` → `a.f()`, `a.Point`, `case a.Circle(r)`. `import a as b`, `from a import f as g`, `_이름` 은 파일 밖에서 못 씀(E0146). 두 파일·두 패키지의 같은 이름이 부딪히지 않음. 모듈 이름 없이 쓰면 한 곳에만 있을 때 경고(W0002), 여러 곳이면 오류(E0147). 없는 이름 E0145, 겹치는 가져오기 E0148, 표준 모듈에 `as` E0149. 값을 찍을 때는 모듈 이름 없이(`Point(x: 1)`) 두 백엔드 같음. `tests/ns/`, `examples/16_modules.skn`.
5. **디버거.** C 라이브러리·std.net·`spawn` 프로그램은 멈출 자리를 넣어 네이티브로 컴파일해서 같은 명령(n s o c b d p v l w q)으로 따라감(`src/rt_dbg.c`, 파이프로 주고받음). 작업 안에서도 멈춤(`(task 2)`). import 한 파일 안에서도 멈추고 `b util.skn:5`. gdb 필요 없음.
6. **덤: 파일 맨 위 상수.** `let PI = 3.14` 가 모든 함수에서 보임(두 백엔드). 최상위 `var`(T0076)와 main 밖의 실행 문장(T0077)은 검사에서 막음 — 예전에는 `siskin run` 만 받아 주고 build 는 실패하던 곳. `siskin run x | head` 가 패닉 글을 내던 것도 고침.

**시험:** 예제 16개·realworld·tests·1·2차 시험 프로그램 전부 run = build(56개), tools 11/11(패키지 이름공간, 네이티브 디버거, 작업 안 디버거 추가).

**남은 한계:** 네이티브 디버거의 `p 식` 은 멈춘 순간 값의 복사본으로 계산(C 함수는 `p` 안에서 못 부름, `p` 안에서 `모듈.이름` 은 아직). 패키지 중앙 저장소 없음.

## 12. `siskin run` 도 진짜 병렬 (2026-09-24, 하루 "진짜 병렬 만들어")

- 예전: `siskin run` 의 `spawn` 은 큰 자물쇠(GIL) 하나로 한 번에 한 작업씩 돌아서 결과만 같고 빨라지지 않았음.
- 지금: 작업 하나 = 운영체제 스레드 하나 + 인터프리터 하나, 큰 자물쇠 없음. 4코어에서 소수 세기 4개 작업이 2.59초 → 0.81초(약 3.2배, build 는 3.5배).
- 메모리 안전: 인터프리터 값(`Rc`)은 스레드끼리 나누지 않음. 작업에 넘기는 값·작업 결과·통로로 보내는 값은 보내는 쪽 스레드가 `Value::detach()` 로 통째로 새로 만들어 자물쇠(Mutex)로 건넴. 함수·구조체 선언(문법 나무)은 `Arc` 로 바꿔 같이 읽음(컴파일 때 Send+Sync 검사). 교착 검사(E0260)는 그대로.
- 덤: `for i in range(a, b)` 가 리스트를 만들지 않고 바로 셈 → 한 작업만 돌 때도 약 30% 빨라짐(여러 작업이 메모리 할당으로 서로 막던 것도 풀림).
- 시험: 프로그램 56개 run = build, tools 11/11, `tests/conc.skn` 40번 반복 같은 결과, 구조체·열거형·사전·클로저를 통로로 주고받는 부하 시험 20번 반복 같은 결과, 교착 5가지 두 백엔드 같음.

## 13. https 서버와 패키지 목록 (2026-09-24, 하루 "https서버 만들수있게 하고 패키지 중앙 저장소는...")

- **https 서버.** `listen_tls(포트, 인증서, 비밀열쇠)` / `listen_tls_on(호스트, ...)`. 받은 연결은 이미 보안 연결이라 `send`/`recv` 그대로. OpenSSL 서버 쪽 함수(`TLS_server_method`, `SSL_accept` 등)도 실행 중에 찾아 씀(`rt_net.c`). 인증서가 없거나, PEM 이 아니거나, 열쇠와 짝이 아니면 두 언어로 알려 주고 시험용 인증서 만드는 openssl 명령을 보여 줌. 보안 연결을 못 맺은 손님(http 로 잘못 온 손님 등)은 건너뛰어 서버가 멈추지 않음.
- **HTTP 서버 도우미.** 예전엔 서버를 TCP 로 직접 짜야 했음. 이제 `server.next_request()` → `Request`(method, path, query, headers, body) → `req.respond(상태, 본문)` / `respond_with(상태, 헤더, 본문)`. `read_request(conn)`, `url_decode`, `Conn.recv_n(n)` 추가. 전부 `std/net.skn` 의 Siskin 코드.
- **패키지 목록(레지스트리).** 서버 없이 git 저장소 하나(`packages/이름.toml` 에 `git`, `description`). `siskin add 이름` 이 목록에서 주소를 찾아 예전 방식(`{ git = ... }`)으로 적음. `siskin search 단어`, `siskin publish`(목록에 더할 파일을 보여 주고, 목록이 폴더면 바로 적음). 목록 주소: `SISKIN_REGISTRY` > siskin.toml `[registry] url` > 기본 `https://github.com/Haru-neo/siskin-registry`(2026-09-24 공개). `~/.siskin/registry/` 에 받아 두고 매번 새로 받음, 인터넷이 안 되면 받아 둔 것을 씀. 틀린 이름은 비슷한 이름을 알려 줌. 목록 저장소 모양은 `registry-template/`.
- **덤: run≠build 하나 고침.** 메서드에 빈 사전·빈 리스트(`b.f({}, [])`)를 넘기면 build 만 "type _" 로 실패하던 것. cgen 이 메서드 인자에도 매개변수 타입을 알려 줌.
- **시험:** 프로그램 run = build 전부 전과 같음, tools 15/15(패키지 목록 search·add·비슷한 이름, https 서버 run = build 추가). curl 로도 https 서버에 GET·POST 확인.
