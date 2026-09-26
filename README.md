# Siskin

파이썬처럼 읽히고, C만큼 빠르며, C++처럼 메모리를 직접 다룰 수 있고,
컴파일러가 사람뿐 아니라 기계(AI)에게도 말을 거는 정적 타입 언어입니다.

*A statically typed language that reads like Python, runs as fast as C, and lets you manage memory like C++.
Programs run in an interpreter (`siskin run`) or compile to a native binary through C (`siskin build`), with the same output either way.
Error messages are in English by default; the documentation is currently in Korean.*

```
fn main():
    let names = ["하루", "Siskin"]
    for n in names:
        print(f"안녕, {n}!\n")
```

## 설치

필요한 것:

- **Rust** (컴파일러를 만들 때만): https://rustup.rs 에서 설치합니다.
- **C 컴파일러** (`siskin build` 가 씁니다). 리눅스는 `gcc`, 맥은 `xcode-select --install`,
  윈도우는 MinGW-w64 의 `gcc` (예: `winget install BrechtSanders.WinLibs.POSIX.UCRT` 뒤 새 터미널). 다른 컴파일러는 환경변수 `CC` 로 고릅니다.
- 있으면 좋은 것: C++ 라이브러리를 쓰려면 `c++`, https 를 쓰려면 OpenSSL(리눅스에는 대개 있습니다).

```
git clone https://github.com/Haru-neo/siskin
cd siskin
cargo install --path micai
```

`siskin` 명령이 `~/.cargo/bin` (윈도우는 `%USERPROFILE%\.cargo\bin`) 에 설치됩니다. 확인:

```
siskin run examples/01_hello.skn
```

> 리눅스·윈도우·macOS 에서 GitHub Actions 로 매번 시험합니다(`tests/same.sh`: run 과 build 의 결과가 같은지).
> 윈도우에서 아직 안 되는 것: `std.net`, C 라이브러리·std.net·spawn 을 쓰는 프로그램의 `siskin debug`.

## 써 보기

```
siskin new hello          # 새 프로젝트 (siskin.toml, main.skn)
cd hello
siskin run main.skn       # 바로 실행 (인터프리터)
siskin build main.skn     # 실행 파일로 컴파일 (C 를 거침, --release 로 더 빠르게)
siskin check main.skn     # 실행하지 않고 오류만 검사
siskin fmt main.skn       # 코드 모양 정리
```

오류 메시지는 영어가 기본입니다. 한국어로 보려면 `--lang ko` 를 붙이거나 `SISKIN_LANG=ko` 를 설정하세요.

에디터: VS Code 는 [`editors/vscode/siskin-0.1.0.vsix`](editors/vscode/) 를 설치하면 색칠, 오류 밑줄, 자동 완성, 실행 버튼이 생깁니다. Neovim·Helix 설정은 [editors/](editors/) 에 있습니다.

패키지: `siskin add 이름` 으로 받고 `siskin search` 로 찾습니다. 패키지 목록은 [siskin-registry](https://github.com/Haru-neo/siskin-registry) 저장소입니다.

## 문서

- [GUIDE.md](GUIDE.md): 사용 설명서. C++ 과 나란히 놓고 문법을 설명합니다. 처음이면 여기부터.
- [examples/](examples/): `01_hello.skn` 부터 번호 순서대로 읽으면 됩니다 (GUIDE 11장).
- [LIBS.md](LIBS.md): C·C++ 라이브러리 가져다 쓰기
- [DESIGN-v0.1.md](DESIGN-v0.1.md): 설계 문서
- [bench/](bench/): 속도 측정. C 대비 1.03~1.11배

## 폴더

| 폴더 | 내용 |
|---|---|
| `micai/` | 컴파일러와 인터프리터 (Rust) |
| `examples/` | 예제 프로그램, `examples/errors/` 는 일부러 틀린 예 |
| `tests/` | 시험 (`bash tests/same.sh siskin`, `sh tests/tools.sh siskin`) |
| `editors/` | VS Code 확장, Neovim·Helix 설정 |
| `bench/` | 같은 프로그램의 Siskin·C 판 속도 비교 |
| `realworld/` | 실제로 써 본 작은 프로그램들 |
| `registry-template/` | 패키지 목록 저장소의 모양 |
| `trial/`, `trial2/` | AI 에이전트들이 처음 써 본 기록 ([FEEDBACK-agents-v0.1.md](FEEDBACK-agents-v0.1.md)) |

## 명령

| 명령 | 하는 일 |
|---|---|
| `siskin run <파일>` | 프로그램 실행 |
| `siskin check <파일>` | 문법 검사 |
| `siskin check --json <파일>` | 기계가 읽는 진단 (설계 §9.2) |
| `siskin build <파일>` | 네이티브 실행 파일로 컴파일 (`--release`, `--emit-c`) |
| `siskin test <파일>` | docstring 안의 `>>>` 예제 실행 (설계 §9.6) |
| `siskin ffi <헤더.h>` | 그 라이브러리에서 뭘 쓸 수 있는지 (`--all`, `--cpp`, `--from`) |
| `siskin debug <파일>` | 한 줄씩 따라가며 값 보기 (`-b 줄` 로 멈출 곳) |
| `siskin fmt <파일/폴더>` | 코드 모양 정리 (`--check`, `--stdout`) |
| `siskin new <이름>` | 새 프로젝트 (siskin.toml, main.skn) |
| `siskin add <이름> <주소>` | 패키지 넣기 (git 주소나 폴더, `--rev 태그`) |
| `siskin install` / `update` / `remove` | 패키지 받기 / 새 판으로 / 빼기 |
| `siskin lsp` | 에디터용 언어 서버 ([editors/](editors/)) |
| `siskin tokens <파일>` | 토큰 덤프 (디버그) |

오류 메시지는 영어가 기본입니다. 한국어는 `--lang ko` 를 붙이거나 `SISKIN_LANG=ko` 를 설정하세요.

## 지금 되는 것 (실행)

함수 · 재귀 · `let`/`var` · `if`/`elif`/`else` · `while` · `for ... in` ·
`break`/`continue` · 리스트 · 딕셔너리 · f-string · 구조체와 메서드 ·
`interface` · `enum`과 `match` · `?T`와 `none` · `!T`와 `try`/`catch` · 오류 enum(`E!T`) ·
`requires`/`ensures` 계약 · doctest · 표준 라이브러리 ·
값 의미론(복사 후 수정해도 원본 불변) · 함수를 값으로 넘기기 ·
**클로저**(`fn(x): x * k` 익명 함수, 함수 안의 함수)

## 지금 되는 것 (타입 검사)

`siskin check`가 실행 전에 잡습니다. 타입 불일치, 인자 개수/타입, 없는 필드와 메서드,
`?T`를 벗기지 않고 쓴 것, `try`를 `!T` 아닌 함수에서 쓴 것,
**`match`에서 빠뜨린 변형**(설계 §4.5의 약속, 이제 컴파일 시점에 걸립니다).

## 지금 되는 것 (네이티브 컴파일)

`siskin build`가 C를 거쳐 실행 파일을 만듭니다. C 대비 1.0~1.2배 ([bench/](bench/)).

**예제 전부 네이티브로 컴파일되고, 인터프리터와 출력이 한 글자도 다르지 않습니다.**
(C 라이브러리를 부르는 예제만 `siskin build` 전용입니다.)

Int, Float, Bool, Str, 리스트, 구조체와 메서드, `enum`과 `match`,
`?T`/`none`/좁히기, `!T`/`try`/`catch`/`error`, 함수와 재귀, 모든 제어문,
f-string(리스트 출력 포함), 문자열 메서드, `requires`/`ensures` 계약, 경계 검사,
**메모리 3계층**(`with arena`, `unsafe`, 원시 포인터 `*T`, `alloc`/`free`, 포인터 산술),
**C 라이브러리 호출**(`extern "C"`), 표준 라이브러리 전체,
**사전(Dict)**, **정규식**, **JSON**.

생성된 C에는 `#line` 표시가 들어가서 C 컴파일러 오류와 디버거가
번역된 C가 아니라 원본 `.skn` 파일의 줄을 가리킵니다.

## 도구

- **코드 정리** `siskin fmt`: 들여쓰기(탭 포함)와 띄어쓰기를 한 가지 모양으로 맞춥니다.
  정리한 뒤 뜻이 한 토큰도 바뀌지 않았는지 스스로 확인하고, 다르면 파일을 건드리지 않습니다.
- **에디터**: VS Code 확장([editors/vscode/](editors/vscode/))으로 색칠, 오류 밑줄, 정리,
  정의로 이동, 자동 완성, 실행 버튼. Neovim·Helix 등은 `siskin lsp` 를 연결합니다 ([editors/](editors/)).
- **디버거** `siskin debug`: 한 줄씩(`n`), 함수 안으로(`s`), 멈출 곳까지(`c`), 값 보기(`p 식`, `v`).
  C 라이브러리·`std.net`·`spawn` 을 쓰는 프로그램과 import 한 파일 안에서도 같은 명령으로 따라갑니다.
- **패키지** `siskin add` / `siskin install`: git 저장소나 폴더를 패키지로 씁니다.
  받은 판은 `siskin.lock` 에 적혀서 어느 컴퓨터에서나 같은 판이 받아집니다.
  패키지 목록(git 저장소 하나)에 올라간 것은 `siskin add 이름` 처럼 이름만으로 받고, `siskin search` 로 찾습니다.
- **웹 서버** `std.net`: `listen(포트)` 는 http, `listen_tls(포트, 인증서, 열쇠)` 는 https 서버입니다.
  `server.next_request()` 로 요청을 받고 `req.respond(200, 본문)` 으로 대답합니다.

## 아직 안 되는 것

**std.net 에서 아직 안 되는 것**: HTTP/2, 한 연결로 여러 요청 받기(keep-alive).
여러 연결을 동시에 다루려면 연결마다 `spawn` 으로 작업을 띄웁니다.

**패키지 목록**: 기본 패키지 목록 저장소는 [`https://github.com/Haru-neo/siskin-registry`](https://github.com/Haru-neo/siskin-registry) 입니다(2026-09-24 공개).
아직 올라간 패키지는 없습니다. 패키지를 올리는 방법은 그 저장소의 README와 [registry-template/](registry-template/) 에 있습니다.

**라이브러리 쪽에서 아직 안 되는 것**: `printf` 처럼 인자 개수가 정해지지 않은 함수,
구조체를 통째로 주고받는 함수, C++ 템플릿을 통째로 미리 가져오기

## 라이브러리

새 언어에는 남이 만들어 둔 것이 없습니다. Siskin 의 답은 **C·C++ 세계를 그대로 쓰는 것**입니다.
C 로 번역된 뒤 컴파일되므로 변환 계층 없이 부릅니다.

헤더 파일(라이브러리 설명서) 이름만 적으면 그 안의 함수를 전부 가져옵니다.

```
import c "zlib.h" link "z"          # zlib 함수 80개가 열립니다
import c "sqlite3.h" link "sqlite3" # sqlite3 함수 283개가 열립니다
import cpp "shapes.hpp" also "shapes.cpp"
```

실제로 재 본 결과입니다.

| 라이브러리 | 바로 쓸 수 있는 함수 |
|---|---|
| zlib (압축) | 81개 중 80개 (98%) |
| sqlite3 (데이터베이스) | 291개 중 283개 (97%) |
| libpng (이미지) | 246개 중 246개 (100%) |
| curses (터미널 화면) | 456개 중 444개 (97%) |
| expat (XML) | 67개 중 66개 (98%) |

`siskin ffi <헤더>` 로 무엇이 열렸고 무엇이 왜 빠졌는지 볼 수 있습니다.
결과를 인자로 돌려주는 자리(`sqlite3_open` 의 두 번째 자리)와, 라이브러리에
내 함수를 넘겨주는 콜백도 됩니다. C++ 은 클래스·가상 함수·템플릿·`std::string`
까지 넘어옵니다. 아직 안 되는 것은 `printf` 처럼 인자 개수가 정해지지 않은 함수와
구조체를 통째로 주고받는 함수입니다.

예제는 [examples/08_cffi.skn](examples/08_cffi.skn)(C),
[examples/10_sqlite.skn](examples/10_sqlite.skn)(데이터베이스),
[examples/11_cpp.skn](examples/11_cpp.skn)(C++), 자세한 안내는 [LIBS.md](LIBS.md) 입니다.

표준 라이브러리는 좁게 갑니다.

| 모듈 | 있는 것 |
|---|---|
| (기본) | `print` `len` `range` `str` `int` `float` `abs` `min` `max` `sum` `assert` `error` |
| `std.math` | `sqrt` `sin` `cos` `tan` `log` `log10` `exp` `floor` `ceil` `round` `pow` `pi` `e` |
| `std.random` | `seed` `rand` `rand_int` |
| `std.time` | `now` `clock` `sleep` `today` `date` `local_time` `utc_time` `parse_time`, 날짜 `DateTime`(`format` `add_days` `days_until` ...) |
| `std.fs` | `read_text` `write_text` `append_text` `remove` `exists` `list_dir` `make_dir` `is_dir` |
| `std.process` | `run` `run_args` `env` `set_env` `cwd` `set_cwd` `pid` |
| `std.net` | `http_get` `http_post` `http_request` `url_encode` `set_timeout`, TCP `connect` `connect_tls` `listen` `listen_on` |
| `std.re` | `test` `find` `find_all` `groups` `replace` `split_re` |
| `std.json` | `parse` `stringify` `jnull` `jbool` `jint` `jfloat` `jstr` `jlist` `jdict` |
| 사전 | `len` `get(키, 기본값)` `set` `has` `keys`, `k in d`, `for k, v in d:`, `d[k]`는 없으면 `none` |
| 리스트 | `push` `pop` `len` `reverse` `contains` `join` `sort` `index_of` `slice` `clear`, 함수를 받는 `map` `filter` `any` `all` `sort_by` |
| 문자열 | `len` `split` `upper` `lower` `strip` `replace` `contains` `starts_with` `ends_with` `find` `repeat` `slice` `width` `pad_left` `pad_right` |

정규식은 바깥 라이브러리를 쓰지 않고 직접 만들었습니다. 같은 엔진을 인터프리터와
C 양쪽에 두어서, 어느 쪽으로 돌려도 결과가 같습니다. JSON도 마찬가지입니다.

정규식에는 역슬래시가 많으므로 **원시 문자열** `r"\d+"` 을 씁니다.
`r`을 빠뜨리면 컴파일이 알려 줍니다.

자세한 사용법은 [GUIDE.md](GUIDE.md) 9.7·9.9절, 예제는
[examples/07_stdlib.skn](examples/07_stdlib.skn)와
[examples/09_data.skn](examples/09_data.skn)입니다.

## 메모리 3계층

| | 쓰는 법 | 해제 | 실수하면 |
|---|---|---|---|
| Level 0 | 그냥 쓰기 | 자동 | — |
| Level 1 | `with arena a:` | 블록 끝에서 통째로 | 블록 밖으로 내보내면 **컴파일이 막음** |
| Level 2 | `unsafe:` + `alloc`/`free` | 직접 | 디버그 빌드가 **실행 중에 잡음** |

- 원시 포인터를 쓰려면 `unsafe:` 안이어야 합니다. 밖에서 쓰면 컴파일이 막습니다.
- 아레나 블록은 `return`으로 빠져나가도 반드시 해제됩니다.
- **디버그 빌드**는 해제 후 접근, 범위를 벗어난 접근, 두 번 해제를 잡아내고
  어디서 어떻게 틀렸는지 알려 줍니다. C에서는 조용히 넘어가는 것들입니다.
- **`--release`** 빌드에서는 그 검사가 통째로 빠집니다. 포인터가 그냥 C 포인터가
  되어 덧씌우는 비용이 0입니다. 아레나 속도는 손으로 쓴 C와 같습니다([bench/](bench/)).

예제는 [examples/06_memory.skn](examples/06_memory.skn),
막히는 예는 [examples/errors/](examples/errors/)의 `nounsafe.skn`, `arena_escape.skn`, `uaf.skn`입니다.

## 대소문자

정확히 쓴 이름이 있으면 그게 이기고, 없을 때만 대소문자만 다른 이름이
딱 하나 있으면 거기에 붙습니다. `siskin check`가 고친 자리를 알려줍니다.
자세한 건 [GUIDE.md](GUIDE.md) 2.7절.

## 만들면서 실제로 찾은 문제들

종이 위에서는 보이지 않던 것들입니다. P1을 먼저 만든 이유가 이것입니다.

1. **임포트가 함수 경계를 넘지 못함** — `from std.math import sqrt`를 파일 맨 위에 썼는데
   함수 안에서 `sqrt`가 안 보였습니다. 임포트는 프레임이 아니라 모듈 전역에 들어가야 합니다.
2. **f-string 안의 오류가 엉뚱한 줄을 가리킴** — `{...}` 안쪽은 별도 소스로 파싱되기 때문에
   줄 번호가 1로 잡혔습니다. 파싱 후 원래 위치로 다시 붙여야 합니다.
   AI 친화의 핵심이 진단 품질인데 이게 깨지면 §9.2가 무의미해집니다.
3. **사용자 함수 이름이 런타임 함수와 충돌** — 예제에 있던 `fn find`가 생성된 C의
   내부 함수 `mi_find`와 같은 이름이 됐습니다. 사용자 이름은 `mu_`, 런타임은 `mi_`로
   접두사를 갈라 해결했습니다.
4. **디버그에서 해제한 메모리를 진짜로 돌려주면 검사가 불가능** — 해제 후 접근을
   잡으려면 "이 메모리는 죽었다"는 표시를 읽어야 하는데, 메모리를 OS에 돌려주면
   그 표시를 읽는 것 자체가 위험한 접근이 됩니다. 디버그에서는 해제한 덩어리를
   돌려주지 않고 붙들어 두기로 했습니다. 그래서 디버그 빌드는 메모리를 더 씁니다.
   `--release`에서는 정상적으로 돌려줍니다.
5. **문자열 길이가 인터프리터와 네이티브에서 달랐음** — `"안녕하세요".len()`이
   인터프리터에서는 5, 네이티브에서는 15(바이트)였습니다. 한글 예제를 쓰기 전까지
   드러나지 않던 문제입니다. 네이티브 쪽을 글자 수로 맞췄습니다. C++의
   `std::string::size()`가 바이트를 세는 것과 같은 함정입니다.
6. **C 함수 이름이 헤더와 부딪힘** — `extern "C" fn strlen(...)`을 그대로 C로 내면
   `string.h`가 이미 선언한 `strlen`과 충돌합니다. 우리 쪽에 다른 이름을 두고
   진짜 심볼에 붙이는 방식으로 풀었습니다.
7. **잘라낸 문자열을 C에 넘기면 깨짐** — `s.slice(0, 2)`는 원본 중간을 가리키는 조각이라
   끝에 0이 없습니다. 이걸 C에 그대로 넘기면 뒤쪽까지 읽습니다. C로 넘길 때는 항상
   0으로 끝나는 사본을 만들도록 했습니다.
8. **큰 실수의 표기가 두 쪽에서 달랐음** — JSON에 아주 큰 수를 넣었더니
   인터프리터는 `100000000000000000000.0`, 컴파일한 쪽은 `1e+20`을 냈습니다.
   같은 프로그램이 다른 답을 내면 안 되므로 규칙을 맞췄습니다.
   JSON을 만들면서야 드러난 문제입니다.
9. **정규식 문자열에서 역슬래시가 조용히 사라짐** — `"\d+"` 라고 쓰면 `\`가 빠져
   `d+`가 됐습니다. 모르는 이스케이프를 그냥 넘기고 있었기 때문입니다.
   이제 오류로 잡고, 원시 문자열 `r"..."` 을 만들었습니다.
10. **실수 출력이 인터프리터와 달랐음** — C의 `%g`는 유효숫자 6자리에서 끊어서
   `12.56636`이 `12.5664`로 나왔습니다. 되돌려 읽어도 같은 값이 되는 가장 짧은 표기를
   찾도록 고쳤습니다. 같은 프로그램이 실행 방식에 따라 다른 답을 내면 안 됩니다.

## 라이선스

[Apache License 2.0](LICENSE). VS Code 확장(`editors/vscode/`)은 그 폴더의 MIT 라이선스를 따릅니다.
