//! P3 네이티브 백엔드: Siskin을 C로 번역합니다.
//!
//! 왜 C인가. 생성된 것이 진짜 C라서 gcc -O2가 그대로 최적화합니다.
//! "C만큼 빠르다"는 주장을 증명하는 가장 짧은 길이고, LLVM 연동보다
//! 훨씬 적은 코드로 됩니다.
//!
//! 출력 순서: 런타임 / 구조체·열거형 / ?T·!T 타입 / 함수 선언 / 함수 본문

use crate::ast::*;
use crate::error::SiskinError;
use crate::types::{Region, Ty, Types};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

const RUNTIME: &str = r##"/* ------- Siskin 런타임 (자동 생성) ------- */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <ctype.h>
#include <math.h>
#include <errno.h>
#ifdef _WIN32
/* 윈도우: 경로·명령줄 인자·화면 글자를 UTF-8 로 맞춥니다. 인터프리터(siskin run)가
   UTF-8 로 다루므로 이렇게 해야 두 방식의 결과가 같습니다. */
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0A00
#endif
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
#include <shellapi.h>
#include <io.h>
#include <fcntl.h>
#include <direct.h>
#include <process.h>
#include <sys/types.h>
#include <sys/stat.h>
static wchar_t* mi_wide(const char* s) {
    int n = MultiByteToWideChar(CP_UTF8, 0, s, -1, NULL, 0);
    wchar_t* w = (wchar_t*)malloc(sizeof(wchar_t) * (size_t)(n > 0 ? n : 1));
    if (!w) { fputs("out of memory\n", stderr); exit(1); }
    if (n <= 0) w[0] = 0; else MultiByteToWideChar(CP_UTF8, 0, s, -1, w, n);
    return w;
}
static char* mi_narrow(const wchar_t* w) {
    int n = WideCharToMultiByte(CP_UTF8, 0, w, -1, NULL, 0, NULL, NULL);
    char* s = (char*)malloc((size_t)(n > 0 ? n : 1));
    if (!s) { fputs("out of memory\n", stderr); exit(1); }
    if (n <= 0) s[0] = 0; else WideCharToMultiByte(CP_UTF8, 0, w, -1, s, n, NULL, NULL);
    return s;
}
static FILE* mi_fopen(const char* p, const char* m) {
    wchar_t* wp = mi_wide(p);
    wchar_t* wm = mi_wide(m);
    FILE* f = _wfopen(wp, wm);
    free(wp); free(wm);
    return f;
}
static int mi_unlink(const char* p) { wchar_t* w = mi_wide(p); int r = _wunlink(w); free(w); return r; }
#define getpid _getpid
/* main 맨 앞에서 부릅니다: 화면을 UTF-8 로, 줄바꿈을 그대로(\n), 인자를 UTF-8 로. */
static void mi_win_init(int* argc, char*** argv) {
    SetConsoleOutputCP(CP_UTF8);
    SetConsoleCP(CP_UTF8);
    _setmode(_fileno(stdout), _O_BINARY);
    _setmode(_fileno(stderr), _O_BINARY);
    int n = 0;
    wchar_t** wa = CommandLineToArgvW(GetCommandLineW(), &n);
    if (!wa) return;
    char** a = (char**)malloc(sizeof(char*) * (size_t)(n + 1));
    if (!a) return;
    for (int i = 0; i < n; i++) a[i] = mi_narrow(wa[i]);
    a[n] = NULL;
    LocalFree(wa);
    *argc = n; *argv = a;
}
/* 런타임(동시성·네트워크·디버거)이 쓰는 pthread 몇 개를 윈도우 스레드로 옮겨 둡니다.
   MSVC 용 clang 에는 pthread 가 없고, MinGW 에서도 따로 DLL 을 안 달고 다니게 됩니다. */
typedef HANDLE pthread_t;
typedef SRWLOCK pthread_mutex_t;
typedef CONDITION_VARIABLE pthread_cond_t;
typedef struct { size_t stack; } pthread_attr_t;
#define PTHREAD_MUTEX_INITIALIZER SRWLOCK_INIT
#define PTHREAD_COND_INITIALIZER CONDITION_VARIABLE_INIT
#define PTHREAD_CREATE_DETACHED 1
static int pthread_mutex_lock(pthread_mutex_t* m) { AcquireSRWLockExclusive(m); return 0; }
static int pthread_mutex_init(pthread_mutex_t* m, void* a) { (void)a; InitializeSRWLock(m); return 0; }
static int pthread_mutex_unlock(pthread_mutex_t* m) { ReleaseSRWLockExclusive(m); return 0; }
static int pthread_cond_wait(pthread_cond_t* c, pthread_mutex_t* m) { SleepConditionVariableSRW(c, m, INFINITE, 0); return 0; }
static int pthread_cond_broadcast(pthread_cond_t* c) { WakeAllConditionVariable(c); return 0; }
static int pthread_attr_init(pthread_attr_t* a) { a->stack = 0; return 0; }
static int pthread_attr_setstacksize(pthread_attr_t* a, size_t n) { a->stack = n; return 0; }
static int pthread_attr_setdetachstate(pthread_attr_t* a, int d) { (void)a; (void)d; return 0; }
static int pthread_attr_destroy(pthread_attr_t* a) { (void)a; return 0; }
typedef struct { void* (*f)(void*); void* arg; } MiThrStart;
static unsigned __stdcall mi_thr_tramp(void* p) {
    MiThrStart s = *(MiThrStart*)p;
    free(p);
    s.f(s.arg);
    return 0;
}
/* 늘 떼어 놓은(detached) 스레드로 만듭니다. 여기서는 그렇게만 씁니다. */
static int pthread_create(pthread_t* t, const pthread_attr_t* a, void* (*f)(void*), void* arg) {
    MiThrStart* s = (MiThrStart*)malloc(sizeof(MiThrStart));
    if (!s) return 1;
    s->f = f; s->arg = arg;
    uintptr_t h = _beginthreadex(NULL, (unsigned)(a ? a->stack : 0), mi_thr_tramp, s, STACK_SIZE_PARAM_IS_A_RESERVATION, NULL);
    if (!h) { free(s); return 1; }
    CloseHandle((HANDLE)h);
    *t = NULL;
    return 0;
}
#else
#include <unistd.h>
#define mi_fopen fopen
#define mi_unlink unlink
#endif

typedef struct { const char* p; int64_t len; } MiStr;
/* 함수 값. 함수 자리(fn)와 붙잡은 값 묶음(env). 이름 붙은 함수는 env 가 NULL 입니다. */
typedef struct { void* fn; void* env; } MiClo;
typedef struct MiArena MiArena;
typedef struct { void* data; int64_t len; int64_t cap; int64_t esz; MiArena* ar; } MiList;

typedef struct MiJson MiJson;
typedef MiJson* MiJsonp;
typedef struct MiTask MiTask;
typedef struct MiChan MiChan;

typedef struct { bool has; MiStr val; }    MiOpt_MiStr;
typedef struct { bool has; int64_t val; }  MiOpt_int64_t;
typedef struct { bool has; double val; }   MiOpt_double;
typedef struct { bool has; bool val; }     MiOpt_bool;
typedef struct { bool has; MiJson* val; }  MiOpt_MiJsonp;
typedef struct { bool ok; MiJson* val; MiStr err; } MiRes_MiJsonp;

typedef struct { bool ok; int64_t val; MiStr err; } MiRes_int64_t;
typedef struct { bool ok; double  val; MiStr err; } MiRes_double;
typedef struct { bool ok; MiStr   val; MiStr err; } MiRes_MiStr;

/* 디버그 빌드는 지금 실행 중인 .skn 줄을 기억해 두었다가 실행 오류에 함께 알립니다. */
static _Thread_local const char* mi_file = "";
static _Thread_local int64_t mi_line = 0;
#ifdef MI_RELEASE
  #define MI_LINE(f, n) ((void)0)
#else
  #define MI_LINE(f, n) (mi_file = (f), mi_line = (n))
#endif

static void mi_panic(const char* msg) {
    fflush(stdout);
    fprintf(stderr, MI_T("실행 오류: %s\n", "runtime error: %s\n"), msg);
    if (mi_line > 0) fprintf(stderr, "  --> %s:%lld\n", mi_file, (long long)mi_line);
    exit(1);
}

static MiStr mi_str(const char* s) { MiStr r; r.p = s; r.len = (int64_t)strlen(s); return r; }
static MiStr mi_mk(const char* p, int64_t n) { MiStr r; r.p = p; r.len = n; return r; }

/* 글자 수를 셉니다. UTF-8 이어붙임 바이트는 빼고 셉니다. */
static int64_t mi_utf8_len(const char* p, int64_t bytes) {
    int64_t n = 0;
    for (int64_t i = 0; i < bytes; i++) if (((unsigned char)p[i] & 0xC0) != 0x80) n++;
    return n;
}

/* i 바이트에서 시작하는 글자가 몇 바이트인지 (문자열 끝을 넘지 않게) */
static int64_t mi_utf8_step(MiStr s, int64_t i) {
    int64_t n = 1;
    while (i + n < s.len && (((unsigned char)s.p[i + n]) & 0xC0) == 0x80) n++;
    return n;
}

static int64_t mi_char_to_byte(MiStr s, int64_t at) {
    int64_t n = 0;
    for (int64_t i = 0; i < s.len; i++) {
        if (((unsigned char)s.p[i] & 0xC0) != 0x80) {
            if (n == at) return i;
            n++;
        }
    }
    return s.len;
}


static char* mi_alloc(int64_t n) {
    char* b = (char*)malloc((size_t)n + 1);
    if (!b) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    b[n] = 0;
    return b;
}

static MiStr mi_cat(MiStr a, MiStr b) {
    char* buf = mi_alloc(a.len + b.len);
    memcpy(buf, a.p, (size_t)a.len);
    memcpy(buf + a.len, b.p, (size_t)b.len);
    return mi_mk(buf, a.len + b.len);
}

static MiStr mi_from_i64(int64_t v) {
    char* buf = mi_alloc(24);
    int n = snprintf(buf, 25, "%lld", (long long)v);
    return mi_mk(buf, n);
}

/* NaN·무한대는 인터프리터(Rust)와 같은 글자로 씁니다. C의 printf는 `-nan` 처럼 달리 씁니다. */
static const char* mi_nonfinite(double v) {
    if (isnan(v)) return "NaN";
    if (isinf(v)) return v < 0 ? "-inf" : "inf";
    return NULL;
}

static MiStr mi_from_f64(double v) {
    if (mi_nonfinite(v)) return mi_str(mi_nonfinite(v));
    char* buf = mi_alloc(40);
    int n;
    /* 정수처럼 딱 떨어지면 `.0` 을 붙입니다. 인터프리터와 같은 규칙입니다.
       자릿수가 많을 수 있어 넉넉히 잡습니다. */
    if (isfinite(v) && floor(v) == v) {
        char* big = mi_alloc(360);
        n = snprintf(big, 361, "%.1f", v);
        return mi_mk(big, n);
    }
    /* 인터프리터와 같은 결과를 내려면 되돌려 읽어도 같아지는 가장 짧은 표기가 필요합니다. */
    n = snprintf(buf, 41, "%.17g", v);
    for (int p = 1; p <= 17; p++) {
        int m = snprintf(buf, 41, "%.*g", p, v);
        if (strtod(buf, NULL) == v) { n = m; break; }
    }
    return mi_mk(buf, n);
}

static MiStr mi_from_bool(bool b) { return mi_str(b ? "true" : "false"); }
static void mi_print(MiStr s) { fwrite(s.p, 1, (size_t)s.len, stdout); }

static bool mi_str_eq(MiStr a, MiStr b) {
    return a.len == b.len && memcmp(a.p, b.p, (size_t)a.len) == 0;
}

static int mi_str_cmp(MiStr a, MiStr b) {
    int64_t n = a.len < b.len ? a.len : b.len;
    int c = memcmp(a.p, b.p, (size_t)n);
    if (c != 0) return c;
    return a.len < b.len ? -1 : (a.len > b.len ? 1 : 0);
}

static int64_t mi_div_i64(int64_t a, int64_t b) {
    if (b == 0) mi_panic(MI_T("0으로 나눌 수 없습니다", "division by zero"));
    return a / b;
}

static int64_t mi_mod_i64(int64_t a, int64_t b) {
    if (b == 0) mi_panic(MI_T("0으로 나눌 수 없습니다", "division by zero"));
    return a % b;
}

/* 실수도 0으로 나누면 인터프리터처럼 멈춥니다 (inf 를 몰래 만들지 않음). */
static double mi_div_f64(double a, double b) {
    if (b == 0.0) mi_panic(MI_T("0으로 나눌 수 없습니다", "division by zero"));
    return a / b;
}

/* ---------------- 메모리 Level 1: 아레나 (범프 할당) ---------------- */
/* 한 덩어리를 크게 잡아 두고 포인터만 앞으로 밀면서 나눠 줍니다.
   블록이 끝나면 덩어리 전체를 한 번에 돌려줍니다.                     */
typedef struct MiChunk {
    struct MiChunk* next;
    size_t used, cap;
    char data[1];
} MiChunk;

struct MiArena { MiChunk* head; size_t total; };

#define MI_CHUNK0 (16 * 1024)

static MiArena mi_arena_new(void) { MiArena a; a.head = NULL; a.total = 0; return a; }

static void* mi_arena_bump(MiArena* a, size_t n) {
    n = (n + 15) & ~(size_t)15;              /* 16바이트 정렬 */
    if (!a->head || a->head->cap - a->head->used < n) {
        size_t cap = a->head ? a->head->cap * 2 : MI_CHUNK0;
        if (cap < n) cap = n;
        MiChunk* c = (MiChunk*)malloc(sizeof(MiChunk) + cap);
        if (!c) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
        c->next = a->head; c->used = 0; c->cap = cap;
        a->head = c;
    }
    void* p = a->head->data + a->head->used;
    a->head->used += n;
    a->total += n;
    return p;
}

#ifndef MI_RELEASE
static _Thread_local MiChunk* mi_quarantine = NULL;        /* 디버그: 해제한 덩어리를 붙들어 둡니다 */
#endif

static void mi_arena_drop(MiArena* a) {
    MiChunk* c = a->head;
    while (c) {
        MiChunk* n = c->next;
#ifdef MI_RELEASE
        free(c);
#else
        /* 디버그에서는 실제로 돌려주지 않고 격리합니다.
           블록을 벗어난 포인터를 쓰면 그 자리에서 잡아내기 위해서입니다. */
        memset(c->data, 0xDD, c->used);
        c->next = mi_quarantine;
        mi_quarantine = c;
#endif
        c = n;
    }
    a->head = NULL;
}

static void mi_arena_cleanup(MiArena* a) { mi_arena_drop(a); }

/* ---------------- 메모리 Level 2: 원시 포인터 ---------------- */
/* 릴리스에서는 그냥 C 포인터입니다. 덧씌우는 비용이 0입니다.
   디버그에서는 세대 번호를 들고 다녀서, 해제한 메모리를 다시 쓰면 잡힙니다. */
typedef struct { uint64_t gen; int64_t size; int64_t in_arena; } MiGuard;
typedef struct { MiGuard* g; int64_t off; uint64_t gen; } MiPtr;

static _Thread_local uint64_t mi_gen_counter = 0;

static MiPtr mi_ptr_alloc(int64_t bytes) {
    if (bytes < 0) mi_panic(MI_T("할당 크기가 음수입니다", "negative allocation size"));
    MiGuard* g = (MiGuard*)calloc(1, sizeof(MiGuard) + (size_t)bytes);
    if (!g) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    g->gen = ++mi_gen_counter;
    g->size = bytes;
    MiPtr p; p.g = g; p.off = 0; p.gen = g->gen; return p;
}

static MiPtr mi_ptr_arena(MiArena* a, int64_t bytes) {
    MiGuard* g = (MiGuard*)mi_arena_bump(a, sizeof(MiGuard) + (size_t)bytes);
    memset(g, 0, sizeof(MiGuard) + (size_t)bytes);
    g->gen = ++mi_gen_counter;
    g->size = bytes;
    g->in_arena = 1;
    MiPtr p; p.g = g; p.off = 0; p.gen = g->gen; return p;
}

static MiPtr mi_ptr_null(void) { MiPtr p; p.g = NULL; p.off = 0; p.gen = 0; return p; }

static void mi_ptr_free(MiPtr p) {
    if (!p.g) mi_panic(MI_T("빈 포인터를 해제했습니다", "freed a null pointer"));
    if (p.g->in_arena) mi_panic(MI_T("아레나 메모리는 따로 해제하지 않습니다 (블록이 끝나면 한꺼번에 사라집니다)", "arena memory is not freed individually (it is all released when the block ends)"));
    if (p.off != 0) mi_panic(MI_T("포인터 중간을 해제할 수 없습니다", "cannot free a pointer into the middle of an allocation"));
    if (p.g->gen == 0) mi_panic(MI_T("이미 해제한 메모리를 또 해제했습니다", "double free: this memory was already freed"));
    if (p.g->gen != p.gen) mi_panic(MI_T("이 포인터는 더 이상 이 메모리를 가리키지 않습니다", "this pointer no longer refers to this memory"));
    p.g->gen = 0;                /* 세대를 지웁니다 */
    memset(p.g + 1, 0xDD, (size_t)p.g->size);
    /* 메모리 자체는 돌려주지 않고 격리합니다. 해제 후 접근을 잡기 위해서입니다. */
}

static void* mi_ptr_at(MiPtr p, int64_t byteoff, int64_t size) {
    if (!p.g) mi_panic(MI_T("빈 포인터에 접근했습니다", "null pointer access"));
    if (p.g->gen != p.gen) mi_panic(MI_T("이미 해제된 메모리에 접근했습니다 (use-after-free)", "access to freed memory (use-after-free)"));
    int64_t at = p.off + byteoff;
    if (at < 0 || at + size > p.g->size) {
        char buf[128];
        snprintf(buf, sizeof buf,
                 MI_T("포인터 접근이 범위를 벗어납니다 (위치 %lld, 크기 %lld바이트)", "pointer access out of bounds (offset %lld, size %lld bytes)"),
                 (long long)at, (long long)p.g->size);
        mi_panic(buf);
    }
    return (char*)(p.g + 1) + at;
}

static MiPtr mi_ptr_add(MiPtr p, int64_t byteoff) { p.off += byteoff; return p; }

static void* mi_raw_alloc(int64_t bytes) {
    void* p = calloc(1, (size_t)(bytes > 0 ? bytes : 1));
    if (!p) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    return p;
}

#ifdef MI_RELEASE
  #define MI_PTR(T)        T*
  #define MI_ALLOC(T, n)   ((T*)mi_raw_alloc((int64_t)(n) * (int64_t)sizeof(T)))
  #define MI_APTR(T, a, n) ((T*)mi_arena_bump(&(a), (size_t)((n) * (int64_t)sizeof(T))))
  #define MI_FREE(p)       free((void*)(p))
  #define MI_AT(T, p, i)   (((T*)(p))[(i)])
  #define MI_ADD(T, p, n)  ((T*)(p) + (n))
  #define MI_NULL(T)       ((T*)0)
#else
  #define MI_PTR(T)        MiPtr
  #define MI_ALLOC(T, n)   mi_ptr_alloc((int64_t)(n) * (int64_t)sizeof(T))
  #define MI_APTR(T, a, n) mi_ptr_arena(&(a), (int64_t)(n) * (int64_t)sizeof(T))
  #define MI_FREE(p)       mi_ptr_free(p)
  #define MI_AT(T, p, i)   (*(T*)mi_ptr_at((p), (int64_t)(i) * (int64_t)sizeof(T), (int64_t)sizeof(T)))
  #define MI_ADD(T, p, n)  mi_ptr_add((p), (int64_t)(n) * (int64_t)sizeof(T))
  #define MI_NULL(T)       mi_ptr_null()
#endif

/* ---------------- 표준 라이브러리 ---------------- */
#include <time.h>

/* 난수. 인터프리터(siskin run)와 한 글자도 다르지 않은 값을 내야 하므로
   양쪽에 똑같은 계산식을 둡니다 (xorshift64*). */
static _Thread_local uint64_t mi_rng = 0x853C49E6748FEA9Bull;

static uint64_t mi_next_rand(void) {
    uint64_t x = mi_rng;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    mi_rng = x;
    return x * 0x2545F4914F6CDD1Dull;
}

static void mi_seed(int64_t n) {
    mi_rng = (uint64_t)n ^ 0x9E3779B97F4A7C15ull;
    if (mi_rng == 0) mi_rng = 0x853C49E6748FEA9Bull;
}

static double mi_rand(void) {
    return (double)(mi_next_rand() >> 11) / 9007199254740992.0;
}

static int64_t mi_rand_int(int64_t lo, int64_t hi) {
    if (hi <= lo) mi_panic(MI_T("rand_int: 뒤 숫자가 더 커야 합니다", "rand_int: the upper bound must be greater than the lower bound"));
    return lo + (int64_t)(mi_next_rand() % (uint64_t)(hi - lo));
}

#ifdef _WIN32
static double mi_now(void) {
    FILETIME ft;
    GetSystemTimePreciseAsFileTime(&ft);
    uint64_t v = ((uint64_t)ft.dwHighDateTime << 32) | ft.dwLowDateTime;   /* 1601년부터 100ns 단위 */
    return (double)(v - 116444736000000000ull) / 1e7;
}

static double mi_clock(void) {
    static LARGE_INTEGER f, t0;
    static int started = 0;
    LARGE_INTEGER t;
    QueryPerformanceCounter(&t);
    if (!started) { QueryPerformanceFrequency(&f); t0 = t; started = 1; }
    return (double)(t.QuadPart - t0.QuadPart) / (double)f.QuadPart;
}
#else
static double mi_now(void) {
    struct timespec t;
    clock_gettime(CLOCK_REALTIME, &t);
    return (double)t.tv_sec + (double)t.tv_nsec / 1e9;
}

static double mi_clock(void) {
    static int started = 0;
    static struct timespec t0;
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    if (!started) { t0 = t; started = 1; }
    return (double)(t.tv_sec - t0.tv_sec) + (double)(t.tv_nsec - t0.tv_nsec) / 1e9;
}
#endif

/* Siskin 문자열을 C가 쓰는 "0으로 끝나는 문자열"로 바꿉니다.
   잘라낸 문자열은 끝에 0이 없을 수 있어 항상 복사합니다. */
static const char* mi_cstr(MiStr s) {
    char* b = mi_alloc(s.len);
    memcpy(b, s.p, (size_t)s.len);
    return b;                      /* mi_alloc 이 끝에 0을 붙입니다 */
}

static bool mi_exists(MiStr p) {
    FILE* f = mi_fopen(mi_cstr(p), "rb");
    if (!f) return false;
    fclose(f);
    return true;
}

/* ---------------- C 라이브러리 연결 (FFI) ---------------- */
/* Level 2 포인터에서 진짜 C 주소를 꺼냅니다.
   디버그에서는 여기서 한 번 살아 있는지 확인하고 넘깁니다. */
#ifdef MI_RELEASE
  #define MI_RAW(T, p)   ((T*)(p))
#else
  #define MI_RAW(T, p)   ((T*)mi_ptr_at((p), 0, 0))
#endif

/* C가 돌려준 문자열을 Siskin 문자열로. NULL이면 빈 문자열입니다. */
static MiStr mi_from_c(const char* p) {
    if (!p) return mi_mk("", 0);
    return mi_mk(p, (int64_t)strlen(p));
}

static MiList mi_list_new(int64_t esz) {
    MiList l; l.data = NULL; l.len = 0; l.cap = 0; l.esz = esz; l.ar = NULL; return l;
}

/* 리스트 정렬. 원소 타입마다 비교 방법이 달라 코드가 타입을 넘겨 줍니다. */
static int mi_cmp_i64(const void* a, const void* b) {
    int64_t x = *(const int64_t*)a, y = *(const int64_t*)b;
    return (x > y) - (x < y);
}
static int mi_cmp_f64(const void* a, const void* b) {
    double x = *(const double*)a, y = *(const double*)b;
    return (x > y) - (x < y);
}
static int mi_cmp_bool(const void* a, const void* b) {
    return (int)*(const bool*)a - (int)*(const bool*)b;
}

static int mi_cmp_str(const void* a, const void* b) {
    return mi_str_cmp(*(const MiStr*)a, *(const MiStr*)b);
}

static MiList mi_list_arena(MiArena* a, int64_t esz) {
    MiList l = mi_list_new(esz); l.ar = a; return l;
}

static void mi_list_push(MiList* l, const void* v) {
    if (l->len == l->cap) {
        int64_t ncap = l->cap ? l->cap * 2 : 8;
        if (l->ar) {
            /* 아레나 리스트: 새 자리를 범프로 받고 옮겨 담습니다.
               개별 free가 없으니 realloc보다 단순하고 빠릅니다. */
            void* nd = mi_arena_bump(l->ar, (size_t)(ncap * l->esz));
            if (l->len) memcpy(nd, l->data, (size_t)(l->len * l->esz));
            l->data = nd;
        } else {
            l->data = realloc(l->data, (size_t)(ncap * l->esz));
            if (!l->data) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
        }
        l->cap = ncap;
    }
    memcpy((char*)l->data + l->len * l->esz, v, (size_t)l->esz);
    l->len++;
}

static void* mi_list_at(MiList* l, int64_t i) {
    if (i < 0 || i >= l->len) {
        char buf[96];
        snprintf(buf, sizeof buf, MI_T("인덱스 %lld이(가) 범위를 벗어납니다 (길이 %lld)", "index %lld out of range (length %lld)"),
                 (long long)i, (long long)l->len);
        mi_panic(buf);
    }
    return (char*)l->data + i * l->esz;
}

static void mi_list_sort(MiList* l, int kind) {
    if (l->len < 2) return;
    int (*cmp)(const void*, const void*) =
        kind == 1 ? mi_cmp_f64 : kind == 2 ? mi_cmp_str : kind == 3 ? mi_cmp_bool : mi_cmp_i64;
    qsort(l->data, (size_t)l->len, (size_t)l->esz, cmp);
}

static MiList mi_list_slice(MiList l, int64_t a, int64_t b) {
    if (a < 0) a = 0;
    if (a > l.len) a = l.len;
    if (b < a) b = a;
    if (b > l.len) b = l.len;
    MiList out = mi_list_new(l.esz);
    for (int64_t i = a; i < b; i++) mi_list_push(&out, (char*)l.data + i * l.esz);
    return out;
}

static void mi_list_clear(MiList* l) { l->len = 0; }

static void mi_list_reverse(MiList* l) {
    char tmp[256];
    char* big = l->esz > (int64_t)sizeof tmp ? (char*)malloc((size_t)l->esz) : tmp;
    for (int64_t i = 0, j = l->len - 1; i < j; i++, j--) {
        char* a = (char*)l->data + i * l->esz;
        char* b = (char*)l->data + j * l->esz;
        memcpy(big, a, (size_t)l->esz); memcpy(a, b, (size_t)l->esz); memcpy(b, big, (size_t)l->esz);
    }
    if (big != tmp) free(big);
}

/* sort_by: 미리 구한 기준값(keys)으로 안정 정렬합니다(같으면 원래 순서).
   kind 0 = Int, 1 = Float, 2 = Str. 인터프리터와 같은 순서가 나와야 합니다. */
static int mi_key_cmp(const void* keys, int kind, int64_t a, int64_t b) {
    if (kind == 1) { double x = ((const double*)keys)[a], y = ((const double*)keys)[b]; return x < y ? -1 : (x > y ? 1 : 0); }
    if (kind == 2) return mi_str_cmp(((const MiStr*)keys)[a], ((const MiStr*)keys)[b]);
    int64_t x = ((const int64_t*)keys)[a], y = ((const int64_t*)keys)[b]; return x < y ? -1 : (x > y ? 1 : 0);
}

static void mi_list_sort_keyed(MiList* l, const void* keys, int kind) {
    int64_t n = l->len;
    if (n < 2) return;
    int64_t* idx = (int64_t*)malloc((size_t)n * sizeof(int64_t));
    int64_t* tmp = (int64_t*)malloc((size_t)n * sizeof(int64_t));
    if (!idx || !tmp) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    for (int64_t i = 0; i < n; i++) idx[i] = i;
    for (int64_t w = 1; w < n; w *= 2) {            /* 아래에서 위로 합치는 병합 정렬 */
        for (int64_t lo = 0; lo < n; lo += 2 * w) {
            int64_t mid = lo + w < n ? lo + w : n, hi = lo + 2 * w < n ? lo + 2 * w : n;
            int64_t i = lo, j = mid, k = lo;
            while (i < mid && j < hi) tmp[k++] = mi_key_cmp(keys, kind, idx[j], idx[i]) < 0 ? idx[j++] : idx[i++];
            while (i < mid) tmp[k++] = idx[i++];
            while (j < hi) tmp[k++] = idx[j++];
        }
        int64_t* t = idx; idx = tmp; tmp = t;
    }
    char* nd = (char*)malloc((size_t)(n * l->esz));
    if (!nd) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    for (int64_t i = 0; i < n; i++) memcpy(nd + i * l->esz, (char*)l->data + idx[i] * l->esz, (size_t)l->esz);
    memcpy(l->data, nd, (size_t)(n * l->esz));
    free(nd); free(idx); free(tmp);
}

static MiStr mi_str_repeat(MiStr s, int64_t n) {
    if (n <= 0) return mi_mk("", 0);
    char* buf = mi_alloc(s.len * n);
    for (int64_t i = 0; i < n; i++) memcpy(buf + i * s.len, s.p, (size_t)s.len);
    return mi_mk(buf, s.len * n);
}

/* f-string 서식: 소수 자릿수 / 16진수 / 폭·정렬 채우기. 인터프리터와 같은 규칙. */
static MiStr mi_fmt_float(double v, int64_t prec) {
    if (mi_nonfinite(v)) return mi_str(mi_nonfinite(v));
    char* buf = mi_alloc(400);
    int n = snprintf(buf, 401, "%.*f", (int)prec, v);
    return mi_mk(buf, n);
}
static MiStr mi_fmt_hex(int64_t v, int upper) {
    char* buf = mi_alloc(24);
    int n = snprintf(buf, 25, upper ? "%llX" : "%llx", (long long)v);
    return mi_mk(buf, n);
}
static MiStr mi_fmt_pad(MiStr s, int64_t width, char align, char fill) {
    int64_t len = mi_utf8_len(s.p, s.len);       /* 글자 수 기준 (Str.len과 같음) */
    if (len >= width) return s;
    int64_t pad = width - len;
    int64_t total = s.len + pad;                 /* fill 은 ASCII 한 바이트로 봅니다 */
    char* buf = mi_alloc(total);
    int64_t left, right;
    if (align == '<') { left = 0; right = pad; }
    else if (align == '^') { left = pad / 2; right = pad - left; }
    else { left = pad; right = 0; }
    int64_t o = 0;
    for (int64_t i = 0; i < left; i++) buf[o++] = fill;
    memcpy(buf + o, s.p, (size_t)s.len); o += s.len;
    for (int64_t i = 0; i < right; i++) buf[o++] = fill;
    return mi_mk(buf, total);
}

/* 화면 폭: 한글·CJK·전각·이모지는 두 칸으로 셉니다. 인터프리터와 같은 규칙. */
static int mi_char_width(uint32_t u) {
    if ((u >= 0x1100 && u <= 0x115F) || (u >= 0x2E80 && u <= 0xA4CF) ||
        (u >= 0xAC00 && u <= 0xD7A3) || (u >= 0xF900 && u <= 0xFAFF) ||
        (u >= 0xFE30 && u <= 0xFE4F) || (u >= 0xFF00 && u <= 0xFF60) ||
        (u >= 0xFFE0 && u <= 0xFFE6) || (u >= 0x1F300 && u <= 0x1FAFF) ||
        (u >= 0x20000 && u <= 0x3FFFD))
        return 2;
    return 1;
}
static int64_t mi_disp_width(MiStr s) {
    int64_t w = 0, i = 0;
    while (i < s.len) {
        unsigned char c = (unsigned char)s.p[i];
        uint32_t u; int n;
        if (c < 0x80) { u = c; n = 1; }
        else if ((c >> 5) == 0x6) { u = c & 0x1F; n = 2; }
        else if ((c >> 4) == 0xE) { u = c & 0x0F; n = 3; }
        else if ((c >> 3) == 0x1E) { u = c & 0x07; n = 4; }
        else { u = c; n = 1; }
        for (int k = 1; k < n && i + k < s.len; k++) u = (u << 6) | (s.p[i + k] & 0x3F);
        w += mi_char_width(u);
        i += n;
    }
    return w;
}
/* pad_right: 왼쪽 정렬(오른쪽에 공백). pad_left: 오른쪽 정렬(왼쪽에 공백). 화면 폭 기준. */
static MiStr mi_pad_right(MiStr s, int64_t width) {
    int64_t w = mi_disp_width(s);
    if (w >= width) return s;
    int64_t pad = width - w;
    char* buf = mi_alloc(s.len + pad);
    memcpy(buf, s.p, (size_t)s.len);
    for (int64_t i = 0; i < pad; i++) buf[s.len + i] = ' ';
    return mi_mk(buf, s.len + pad);
}
static MiStr mi_pad_left(MiStr s, int64_t width) {
    int64_t w = mi_disp_width(s);
    if (w >= width) return s;
    int64_t pad = width - w;
    char* buf = mi_alloc(s.len + pad);
    for (int64_t i = 0; i < pad; i++) buf[i] = ' ';
    memcpy(buf + pad, s.p, (size_t)s.len);
    return mi_mk(buf, s.len + pad);
}

/* input() / args(): 키보드 한 줄 읽기와 명령행 인자. */
static MiList mi_prog_args;   /* main에서 채웁니다. */
/* 한 줄 읽기. 입력이 끝났으면(EOF) none 입니다. 빈 줄은 "" 입니다. */
static MiOpt_MiStr mi_input(void) {
    MiOpt_MiStr r;
    size_t cap = 64, len = 0;
    char* buf = (char*)malloc(cap);
    int ch;
    fflush(stdout);
    while ((ch = getchar()) != EOF && ch != '\n') {
        if (len + 2 >= cap) { cap *= 2; buf = (char*)realloc(buf, cap); }
        buf[len++] = (char)ch;
    }
    if (ch == EOF && len == 0) { free(buf); r.has = false; r.val = mi_mk("", 0); return r; }
    while (len > 0 && (buf[len - 1] == '\r' || buf[len - 1] == '\n')) len--;
    buf[len] = 0;
    r.has = true; r.val = mi_mk(buf, (int64_t)len);
    return r;
}

static int64_t mi_str_find(MiStr s, MiStr sub) {
    if (sub.len == 0) return 0;
    for (int64_t i = 0; i + sub.len <= s.len; i++) {
        if (memcmp(s.p + i, sub.p, (size_t)sub.len) == 0) return mi_utf8_len(s.p, i);
    }
    return -1;
}

static MiStr mi_str_slice(MiStr s, int64_t a, int64_t b) {
    int64_t n = mi_utf8_len(s.p, s.len);
    if (a < 0) a = 0;
    if (a > n) a = n;
    if (b < a) b = a;
    if (b > n) b = n;
    int64_t ba = mi_char_to_byte(s, a), bb = mi_char_to_byte(s, b);
    return mi_mk(s.p + ba, bb - ba);
}

/* ---------------- 사전(Dict) ---------------- */
/* 넣은 순서를 기억하는 해시 표입니다.
   entries 에 들어온 순서대로 쌓고, idx 는 빠르게 찾기 위한 색인입니다. */
typedef struct {
    char* keys;      /* 넣은 순서대로 */
    char* vals;
    int64_t* idx;    /* 해시 -> entries 위치. -1이면 빈 칸 */
    int64_t len, cap, islots;
    int64_t ksz, vsz;
    int kkind;       /* 0 = 숫자/불리언(바이트 비교), 1 = 문자열 */
} MiDict;

static MiDict mi_dict_new(int64_t ksz, int64_t vsz, int kkind) {
    MiDict d;
    d.keys = NULL; d.vals = NULL; d.idx = NULL;
    d.len = 0; d.cap = 0; d.islots = 0;
    d.ksz = ksz; d.vsz = vsz; d.kkind = kkind;
    return d;
}

static uint64_t mi_hash_bytes(const void* p, int64_t n) {
    const unsigned char* b = (const unsigned char*)p;
    uint64_t h = 1469598103934665603ull;          /* FNV-1a */
    for (int64_t i = 0; i < n; i++) { h ^= b[i]; h *= 1099511628211ull; }
    return h;
}

static uint64_t mi_dict_hash(const MiDict* d, const void* k) {
    if (d->kkind == 1) {
        const MiStr* s = (const MiStr*)k;
        return mi_hash_bytes(s->p, s->len);
    }
    return mi_hash_bytes(k, d->ksz);
}

static bool mi_dict_keq(const MiDict* d, const void* a, const void* b) {
    if (d->kkind == 1) return mi_str_eq(*(const MiStr*)a, *(const MiStr*)b);
    return memcmp(a, b, (size_t)d->ksz) == 0;
}

static void mi_dict_reindex(MiDict* d, int64_t slots) {
    d->idx = (int64_t*)realloc(d->idx, (size_t)slots * sizeof(int64_t));
    if (!d->idx) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    for (int64_t i = 0; i < slots; i++) d->idx[i] = -1;
    d->islots = slots;
    for (int64_t e = 0; e < d->len; e++) {
        uint64_t h = mi_dict_hash(d, d->keys + e * d->ksz);
        int64_t at = (int64_t)(h & (uint64_t)(slots - 1));
        while (d->idx[at] != -1) at = (at + 1) & (slots - 1);
        d->idx[at] = e;
    }
}

/* 키가 있으면 그 자리를, 없으면 -1 을 냅니다. */
static int64_t mi_dict_find(const MiDict* d, const void* k) {
    if (d->islots == 0) return -1;
    uint64_t h = mi_dict_hash(d, k);
    int64_t at = (int64_t)(h & (uint64_t)(d->islots - 1));
    while (d->idx[at] != -1) {
        int64_t e = d->idx[at];
        if (mi_dict_keq(d, d->keys + e * d->ksz, k)) return e;
        at = (at + 1) & (d->islots - 1);
    }
    return -1;
}

static void mi_dict_set(MiDict* d, const void* k, const void* v) {
    int64_t e = mi_dict_find(d, k);
    if (e >= 0) {                                  /* 이미 있으면 값만 바꿉니다 */
        memcpy(d->vals + e * d->vsz, v, (size_t)d->vsz);
        return;
    }
    if (d->len == d->cap) {
        d->cap = d->cap ? d->cap * 2 : 8;
        d->keys = (char*)realloc(d->keys, (size_t)(d->cap * d->ksz));
        d->vals = (char*)realloc(d->vals, (size_t)(d->cap * d->vsz));
        if (!d->keys || !d->vals) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    }
    memcpy(d->keys + d->len * d->ksz, k, (size_t)d->ksz);
    memcpy(d->vals + d->len * d->vsz, v, (size_t)d->vsz);
    d->len++;
    /* 절반 넘게 차면 색인을 넓힙니다. */
    if (d->islots == 0 || d->len * 2 > d->islots) {
        int64_t slots = d->islots ? d->islots * 2 : 16;
        while (d->len * 2 > slots) slots *= 2;
        mi_dict_reindex(d, slots);
    } else {
        uint64_t h = mi_dict_hash(d, d->keys + (d->len - 1) * d->ksz);
        int64_t at = (int64_t)(h & (uint64_t)(d->islots - 1));
        while (d->idx[at] != -1) at = (at + 1) & (d->islots - 1);
        d->idx[at] = d->len - 1;
    }
}

static bool mi_dict_has(const MiDict* d, const void* k) { return mi_dict_find(d, k) >= 0; }

/* 찾으면 값의 주소, 없으면 NULL. */
static void* mi_dict_at(const MiDict* d, const void* k) {
    int64_t e = mi_dict_find(d, k);
    return e < 0 ? NULL : (void*)(d->vals + e * d->vsz);
}

static MiList mi_dict_keys(const MiDict* d) {
    MiList out = mi_list_new(d->ksz);
    for (int64_t i = 0; i < d->len; i++) mi_list_push(&out, d->keys + i * d->ksz);
    return out;
}

/* 값 의미론: 복사본을 고쳐도 원본이 그대로이도록 속까지 새로 만듭니다. */
static MiList mi_list_copy(MiList l) {
    MiList r = mi_list_new(l.esz);
    if (l.len > 0) {
        r.data = malloc((size_t)(l.len * l.esz));
        if (!r.data) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
        memcpy(r.data, l.data, (size_t)(l.len * l.esz));
        r.len = r.cap = l.len;
    }
    return r;
}

static MiDict mi_dict_copy(MiDict d) {
    MiDict r = d;
    if (d.cap > 0) {
        r.keys = (char*)malloc((size_t)(d.cap * d.ksz));
        r.vals = (char*)malloc((size_t)(d.cap * d.vsz));
        if (!r.keys || !r.vals) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
        memcpy(r.keys, d.keys, (size_t)(d.len * d.ksz));
        memcpy(r.vals, d.vals, (size_t)(d.len * d.vsz));
    }
    if (d.islots > 0) {
        r.idx = (int64_t*)malloc((size_t)d.islots * sizeof(int64_t));
        if (!r.idx) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
        memcpy(r.idx, d.idx, (size_t)d.islots * sizeof(int64_t));
    }
    return r;
}

/* 따옴표를 붙이고 \\ 와 " 를 이스케이프한 문자열. 인터프리터의 repr 과 같습니다. */
static MiStr mi_str_repr(MiStr s) {
    int64_t extra = 2;
    for (int64_t i = 0; i < s.len; i++) if (s.p[i] == '"' || s.p[i] == '\\') extra++;
    char* b = mi_alloc(s.len + extra);
    int64_t n = 0;
    b[n++] = '"';
    for (int64_t i = 0; i < s.len; i++) {
        if (s.p[i] == '"' || s.p[i] == '\\') b[n++] = '\\';
        b[n++] = s.p[i];
    }
    b[n++] = '"';
    return mi_mk(b, n);
}

static MiList mi_range(int64_t from, int64_t to) {
    MiList l = mi_list_new((int64_t)sizeof(int64_t));
    for (int64_t i = from; i < to; i++) mi_list_push(&l, &i);
    return l;
}

static void mi_contract_fail(const char* rendered) {
    fflush(stdout);
    fputs(rendered, stderr);
    exit(1);
}

/* i번째 "글자"를 냅니다. 바이트가 아닙니다. */
static MiStr mi_str_index(MiStr s, int64_t i) {
    int64_t n = mi_utf8_len(s.p, s.len);
    if (i < 0 || i >= n) {
        char b[96];
        snprintf(b, sizeof b, MI_T("문자열 인덱스 %lld이(가) 범위를 벗어납니다 (길이 %lld)", "string index %lld out of range (length %lld)"),
                 (long long)i, (long long)n);
        mi_panic(b);
    }
    int64_t a = mi_char_to_byte(s, i), z = mi_char_to_byte(s, i + 1);
    char* buf = mi_alloc(z - a);
    memcpy(buf, s.p + a, (size_t)(z - a));
    return mi_mk(buf, z - a);
}

/* 문자열 메서드 */
static MiList mi_split(MiStr s, MiStr sep) {
    MiList out = mi_list_new((int64_t)sizeof(MiStr));
    if (sep.len == 0) { mi_list_push(&out, &s); return out; }
    int64_t start = 0, i = 0;
    while (i + sep.len <= s.len) {
        if (memcmp(s.p + i, sep.p, (size_t)sep.len) == 0) {
            MiStr piece = mi_mk(s.p + start, i - start);
            mi_list_push(&out, &piece);
            i += sep.len; start = i;
        } else i++;
    }
    MiStr piece = mi_mk(s.p + start, s.len - start);
    mi_list_push(&out, &piece);
    return out;
}

static MiStr mi_join(MiList parts, MiStr sep) {
    int64_t total = 0;
    for (int64_t i = 0; i < parts.len; i++)
        total += ((MiStr*)parts.data)[i].len + (i ? sep.len : 0);
    char* buf = mi_alloc(total);
    int64_t at = 0;
    for (int64_t i = 0; i < parts.len; i++) {
        if (i) { memcpy(buf + at, sep.p, (size_t)sep.len); at += sep.len; }
        MiStr e = ((MiStr*)parts.data)[i];
        memcpy(buf + at, e.p, (size_t)e.len); at += e.len;
    }
    return mi_mk(buf, total);
}


static MiStr mi_strip(MiStr s) {
    int64_t a = 0, b = s.len;
    while (a < b && isspace((unsigned char)s.p[a])) a++;
    while (b > a && isspace((unsigned char)s.p[b-1])) b--;
    return mi_mk(s.p + a, b - a);
}

static bool mi_find(MiStr s, MiStr t, int64_t from, int64_t* at) {
    if (t.len == 0) { *at = from; return from <= s.len; }
    for (int64_t i = from; i + t.len <= s.len; i++)
        if (memcmp(s.p + i, t.p, (size_t)t.len) == 0) { *at = i; return true; }
    return false;
}

static bool mi_contains(MiStr s, MiStr t) { int64_t at; return mi_find(s, t, 0, &at); }
static bool mi_starts_with(MiStr s, MiStr t) {
    return t.len <= s.len && memcmp(s.p, t.p, (size_t)t.len) == 0;
}
static bool mi_ends_with(MiStr s, MiStr t) {
    return t.len <= s.len && memcmp(s.p + s.len - t.len, t.p, (size_t)t.len) == 0;
}

static MiStr mi_replace(MiStr s, MiStr from, MiStr to) {
    if (from.len == 0) return s;
    int64_t count = 0, i = 0, at;
    while (mi_find(s, from, i, &at)) { count++; i = at + from.len; }
    int64_t total = s.len + count * (to.len - from.len);
    char* buf = mi_alloc(total);
    int64_t src = 0, dst = 0;
    while (mi_find(s, from, src, &at)) {
        memcpy(buf + dst, s.p + src, (size_t)(at - src)); dst += at - src;
        memcpy(buf + dst, to.p, (size_t)to.len); dst += to.len;
        src = at + from.len;
    }
    memcpy(buf + dst, s.p + src, (size_t)(s.len - src));
    return mi_mk(buf, total);
}

static MiRes_int64_t mi_parse_int(MiStr s) {
    MiRes_int64_t r;
    MiStr t = mi_strip(s);
    char buf[64];
    if (t.len == 0 || t.len >= 63) {
        r.ok = false; r.val = 0;
        r.err = mi_cat(mi_cat(mi_str(MI_T("`", "cannot parse `")), s), mi_str(MI_T("`을(를) Int로 읽을 수 없습니다", "` as Int")));
        return r;
    }
    memcpy(buf, t.p, (size_t)t.len); buf[t.len] = 0;
    char* end = NULL;
    long long v = strtoll(buf, &end, 10);
    if (end == buf || *end != 0) {
        r.ok = false; r.val = 0;
        r.err = mi_cat(mi_cat(mi_str(MI_T("`", "cannot parse `")), s), mi_str(MI_T("`을(를) Int로 읽을 수 없습니다", "` as Int")));
        return r;
    }
    r.ok = true; r.val = (int64_t)v; r.err = mi_str("");
    return r;
}

static MiRes_double mi_parse_float(MiStr s) {
    MiRes_double r;
    MiStr t = mi_strip(s);
    char buf[64];
    if (t.len == 0 || t.len >= 63) {
        r.ok = false; r.val = 0;
        r.err = mi_cat(mi_cat(mi_str(MI_T("`", "cannot parse `")), s), mi_str(MI_T("`을(를) Float로 읽을 수 없습니다", "` as Float")));
        return r;
    }
    memcpy(buf, t.p, (size_t)t.len); buf[t.len] = 0;
    char* end = NULL;
    double v = strtod(buf, &end);
    if (end == buf || *end != 0) {
        r.ok = false; r.val = 0;
        r.err = mi_cat(mi_cat(mi_str(MI_T("`", "cannot parse `")), s), mi_str(MI_T("`을(를) Float로 읽을 수 없습니다", "` as Float")));
        return r;
    }
    r.ok = true; r.val = v; r.err = mi_str("");
    return r;
}

/* errno 를 사람이 읽는 까닭으로. 인터프리터(sys.rs)와 글자까지 같습니다. */
static MiStr mi_errmsg(MiStr path, int e) {
    const char* why;
    char buf[64];
    switch (e) {
    case ENOENT: why = MI_T("없는 경로입니다", "no such file or directory"); break;
    case EACCES: case EPERM: why = MI_T("권한이 없습니다", "permission denied"); break;
    case EEXIST: why = MI_T("이미 있습니다", "already exists"); break;
    case ENOTDIR: why = MI_T("폴더가 아닙니다", "not a directory"); break;
    case EISDIR: why = MI_T("폴더입니다", "is a directory"); break;
    case ENOTEMPTY: why = MI_T("폴더가 비어 있지 않습니다", "directory not empty"); break;
    default: snprintf(buf, sizeof buf, MI_T("할 수 없습니다 (errno %d)", "operation failed (errno %d)"), e); why = buf; break;
    }
    return mi_cat(mi_cat(path, mi_str(": ")), mi_str(why));
}

/* `!Unit` 은 내부적으로 MiRes_int64_t 로 표현됩니다. */
static MiRes_int64_t mi_write_text(MiStr path, MiStr text, int append) {
    MiRes_int64_t r; r.val = 0;
    FILE* f = mi_fopen(mi_cstr(path), append ? "ab" : "wb");
    if (!f) {
        r.ok = false;
        r.err = mi_errmsg(path, errno);
        return r;
    }
    fwrite(text.p, 1, (size_t)text.len, f);
    fclose(f);
    r.ok = true; r.err = mi_str("");
    return r;
}

static MiRes_int64_t mi_remove(MiStr path) {
    MiRes_int64_t r; r.val = 0;
    if (mi_unlink(mi_cstr(path)) != 0) {
        r.ok = false;
        r.err = mi_errmsg(path, errno);
        return r;
    }
    r.ok = true; r.err = mi_str("");
    return r;
}

static MiRes_MiStr mi_read_text(MiStr path) {
    MiRes_MiStr r;
    FILE* f = mi_fopen(mi_cstr(path), "rb");
    if (!f) {
        r.ok = false; r.val = mi_str("");
        r.err = mi_errmsg(path, errno);
        return r;
    }
    /* 크기를 미리 묻지 않고 끝까지 읽습니다. /proc 파일이나 /dev/stdin 은 크기가 0으로 보입니다. */
    int64_t cap = 4096, got = 0;
    char* data = (char*)malloc((size_t)cap);
    if (!data) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    for (;;) {
        if (got == cap) {
            cap *= 2;
            data = (char*)realloc(data, (size_t)cap);
            if (!data) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
        }
        size_t n = fread(data + got, 1, (size_t)(cap - got), f);
        if (n == 0) break;
        got += (int64_t)n;
    }
    fclose(f);
    data = (char*)realloc(data, (size_t)got + 1);
    data[got] = 0;
    r.ok = true; r.val = mi_mk(data, (int64_t)got); r.err = mi_str("");
    return r;
}
/* ------- 런타임 끝 ------- */

"##;

pub struct CGen {
    ty: Types,
    /// 앞쪽 선언: `struct mu_X;`, `typedef struct MiOpt_.. MiOpt_..;`, 함수 포인터 typedef.
    /// 포인터로만 쓰는 곳은 이것만 있으면 됩니다.
    decls: String,
    /// 값으로 품는 타입 정의들(구조체·열거형·MiOpt_·MiRes_·MiTup_·클로저 묶음).
    /// C 는 값으로 품는 타입이 먼저 완성돼 있어야 하므로, 끝에서 의존 순서대로 냅니다.
    tdefs: Vec<(String, String, Vec<String>)>,
    /// 자기 자신을 (돌고 돌아) 품는 `?구조체`/`!구조체` 필드: (구조체, 필드).
    /// 값으로 품으면 크기가 무한이라, 힙 상자(포인터)에 담고 복사할 때 상자도 새로 만듭니다.
    boxed: HashSet<(String, String)>,
    protos: String,
    /// 헤더에서 가져온 C 함수의 껍데기. `#include` 와 타입을 맞춘 함수들.
    ffi: String,
    /// 실제로 불린 C 함수 이름. 안 쓴 함수는 껍데기를 안 냅니다.
    used_externs: HashSet<String>,
    /// C++ 껍데기. 따로 파일로 빠져 C++ 컴파일러로 컴파일됩니다.
    cpp_ffi: String,
    /// 라이브러리에 넘겨주는 내 함수의 다리. protos 뒤에 놓입니다.
    cb_ffi: String,
    cb_done: HashSet<String>,
    body: String,
    indent: usize,
    errors: Vec<SiskinError>,
    cur_fn: String,
    cur_ret: Ty,
    expect: Option<Ty>,
    /// 이름 -> C 식. `?T` 좁히기(`if x != none:`)에 씁니다.
    renames: Vec<HashMap<String, String>>,
    /// 미리 계산해 둔 하위 식 (식 주소 → C 임시 변수). 계산 순서를 왼쪽→오른쪽으로 고정할 때 씁니다.
    pre: HashMap<usize, String>,
    opt_types: HashSet<String>,
    res_types: HashSet<String>,
    tup_types: HashSet<String>,
    fn_ptr_types: HashSet<String>,
    /// 지금 만들고 있는 제네릭 특수화의 이름 꼬리표(`__int64_t` 등). 보통은 빈 문자열.
    mono_suffix: String,
    /// 아직 만들지 않은 제네릭 특수화 대기열: (선언, 타입 묶음, 이름 꼬리표).
    mono_queue: Vec<(crate::ast::Shared<FnDecl>, HashMap<String, Ty>, String)>,
    /// 이미 만든 특수화의 C 이름들(중복 생성 방지).
    mono_done: HashSet<String>,
    tmp: usize,
    /// `#line` 표시에 쓸 원본 경로. 오류와 디버거가 .skn 파일을 가리키게 합니다.
    src_path: String,
    last_line: usize,
    /// 클로저 본문을 끌어올린 C 함수들과, 이름 붙은 함수를 값으로 쓸 때의 다리들.
    lifted: String,
    thunks_done: HashSet<String>,
    lam_count: usize,
    /// 구조체 이름 -> 복사할 때 속까지 새로 만들어야 하는가(리스트·사전을 품었는가).
    copy_needed: HashMap<String, bool>,
    copy_done: HashSet<String>,
    repr_done: HashSet<String>,
    /// std.net 을 썼는가. 썼을 때만 네트워크 런타임을 붙입니다.
    uses_net: bool,
    /// `spawn`·`channel` 을 쓰면 동시성 런타임(rt_conc.c)을 넣고 pthread 를 링크합니다.
    uses_conc: bool,
    /// 작업 결과 타입마다 하나씩 만드는 "작업 돌리개" 함수 이름들.
    task_runners: HashSet<String>,
    uses_case: bool,
    /// `siskin debug` 용: 문장마다 멈출 자리(MI_DBG)를 넣습니다.
    dbg: bool,
    /// 지금 함수의 지역 변수가 시작하는 타입 검사기 스코프 번호 (디버거용)
    dbg_base: usize,
    /// 최상위 상수 (전역 변수 선언, 채울 문장)
    gvars: String,
    globals: Vec<Stmt>,
    has_globals: bool,
}

fn cerr(code: &'static str, msg: impl Into<String>, line: usize, col: usize) -> SiskinError {
    SiskinError::new(code, msg, line, col)
}

/// 사용자 이름은 `mu_`, 런타임 내부 함수는 `mi_`.
/// 이렇게 나눠야 사용자가 `fn find`를 만들어도 런타임 `mi_find`와 안 부딪힙니다.
fn mangle(name: &str) -> String {
    format!("mu_{}", name)
}

fn local(name: &str) -> String {
    format!("v_{}", name)
}

/// 필드의 C 이름. 튜플의 `.0` 은 `f0`, 구조체 필드는 `v_이름`.
fn field_c(name: &str) -> String {
    if name.chars().all(|ch| ch.is_ascii_digit()) {
        format!("f{}", name)
    } else {
        local(name)
    }
}

/// 타입마다 다른 C 이름 조각. `[[Int]]` 와 `[Str]` 은 C 로는 둘 다 MiList 라서
/// 타입별로 만드는 함수(복사, 글자로 바꾸기)의 이름은 이걸로 가릅니다.
fn ty_key(t: &Ty) -> String {
    match t {
        Ty::Int => "I".into(),
        Ty::Float => "F".into(),
        Ty::Bool => "B".into(),
        Ty::Str => "S".into(),
        Ty::Unit => "U".into(),
        Ty::Json => "J".into(),
        Ty::List(a) => format!("L{}_", ty_key(a)),
        Ty::Dict(a, b) => format!("D{}{}_", ty_key(a), ty_key(b)),
        Ty::Optional(a) => format!("O{}_", ty_key(a)),
        Ty::Fallible(a, e) if **e == Ty::Str || **e == Ty::Unknown => format!("R{}_", ty_key(a)),
        Ty::Fallible(a, e) => format!("R{}E{}_", ty_key(a), ty_key(e)),
        Ty::Raw(a) => format!("P{}_", ty_key(a)),
        Ty::Tuple(ts) => format!("T{}_", ts.iter().map(ty_key).collect::<Vec<_>>().join("")),
        Ty::Fn(ps, r) => format!("Fn{}{}_", ps.iter().map(ty_key).collect::<Vec<_>>().join(""), ty_key(r)),
        Ty::Struct(n) | Ty::Enum(n) => format!("N{}{}", n.len(), n),
        other => format!("X{}", other.to_string().len()),
    }
}

/// 디버거가 값을 보여 줄 수 있는 타입인가 (C 로 만든 변수가 실제로 있는 것).
fn dbg_showable(t: &Ty) -> bool {
    match t {
        Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Json | Ty::Task(_) | Ty::Chan(_) | Ty::Struct(_) | Ty::Enum(_) => true,
        Ty::List(a) | Ty::Optional(a) => dbg_showable(a),
        Ty::Dict(a, b) => dbg_showable(a) && dbg_showable(b),
        Ty::Tuple(ts) => ts.iter().all(dbg_showable),
        _ => false,
    }
}

fn sanitize(ctype: &str) -> String {
    ctype.replace("struct ", "").replace(' ', "_").replace('*', "p")
}

fn c_string(s: &str) -> String {
    let mut out = String::from("\"");
    for b in s.bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            0x20..=0x7e => out.push(b as char),
            other => out.push_str(&format!("\\{:03o}", other)),
        }
    }
    out.push('"');
    out
}

/// C 소스와, 함께 링크해야 할 라이브러리 이름 목록을 냅니다.
/// `upper()`/`lower()` 의 C 쪽. 인터프리터가 쓰는 Rust 표준 라이브러리의 대소문자 표를
/// 그대로 C 배열로 옮겨서, 두 실행 방식의 결과가 글자 하나까지 같게 합니다.
fn case_runtime() -> String {
    fn table(name: &str, upper: bool) -> String {
        let mut rows = Vec::new();
        for cp in 0u32..=0x10FFFF {
            let Some(c) = char::from_u32(cp) else { continue };
            let m: Vec<char> = if upper { c.to_uppercase().collect() } else { c.to_lowercase().collect() };
            if m.len() == 1 && m[0] == c {
                continue;
            }
            let mut v = [0u32; 3];
            for (i, x) in m.iter().enumerate().take(3) {
                v[i] = *x as u32;
            }
            rows.push(format!("{{{},{},{},{}}}", cp, v[0], v[1], v[2]));
        }
        format!(
            "static const uint32_t {}[{}][4] = {{{}}};\n",
            name,
            rows.len(),
            rows.join(",")
        )
    }
    let mut s = String::new();
    s.push_str(&table("mi_up_tab", true));
    s.push_str(&table("mi_lo_tab", false));
    s.push_str(r#"
static const uint32_t* mi_case_find(const uint32_t (*t)[4], int64_t n, uint32_t u) {
    int64_t lo = 0, hi = n - 1;
    while (lo <= hi) {
        int64_t mid = (lo + hi) / 2;
        if (t[mid][0] == u) return t[mid];
        if (t[mid][0] < u) lo = mid + 1; else hi = mid - 1;
    }
    return NULL;
}
static int64_t mi_utf8_dec(MiStr s, int64_t i, uint32_t* out) {
    unsigned char c = (unsigned char)s.p[i];
    int64_t n = c < 0x80 ? 1 : c < 0xE0 ? 2 : c < 0xF0 ? 3 : 4;
    if (i + n > s.len) n = s.len - i;
    uint32_t u = n == 1 ? c : n == 2 ? (c & 0x1F) : n == 3 ? (c & 0x0F) : (c & 0x07);
    for (int64_t k = 1; k < n; k++) u = (u << 6) | ((unsigned char)s.p[i + k] & 0x3F);
    *out = u;
    return n;
}
static int mi_utf8_enc(uint32_t u, char* b) {
    if (u < 0x80) { b[0] = (char)u; return 1; }
    if (u < 0x800) { b[0] = (char)(0xC0 | (u >> 6)); b[1] = (char)(0x80 | (u & 0x3F)); return 2; }
    if (u < 0x10000) { b[0] = (char)(0xE0 | (u >> 12)); b[1] = (char)(0x80 | ((u >> 6) & 0x3F));
        b[2] = (char)(0x80 | (u & 0x3F)); return 3; }
    b[0] = (char)(0xF0 | (u >> 18)); b[1] = (char)(0x80 | ((u >> 12) & 0x3F));
    b[2] = (char)(0x80 | ((u >> 6) & 0x3F)); b[3] = (char)(0x80 | (u & 0x3F)); return 4;
}
#define MI_TAB_N(t) ((int64_t)(sizeof(t) / sizeof(t[0])))
static bool mi_is_cased(uint32_t u) {
    return mi_case_find(mi_up_tab, MI_TAB_N(mi_up_tab), u) || mi_case_find(mi_lo_tab, MI_TAB_N(mi_lo_tab), u);
}
static MiStr mi_casemap(MiStr s, int upper) {
    char* buf = mi_alloc(s.len * 3 + 4);
    int64_t at = 0, i = 0;
    bool prev_cased = false;
    while (i < s.len) {
        uint32_t u;
        int64_t n = mi_utf8_dec(s, i, &u);
        const uint32_t* row = upper ? mi_case_find(mi_up_tab, MI_TAB_N(mi_up_tab), u)
                                    : mi_case_find(mi_lo_tab, MI_TAB_N(mi_lo_tab), u);
        if (!upper && u == 0x3A3) {
            /* 그리스어 시그마: 낱말 끝이면 ς (Rust 와 같은 규칙) */
            bool next_cased = false;
            if (i + n < s.len) { uint32_t v; mi_utf8_dec(s, i + n, &v); next_cased = mi_is_cased(v); }
            at += mi_utf8_enc(prev_cased && !next_cased ? 0x3C2 : 0x3C3, buf + at);
        } else if (row) {
            for (int k = 1; k < 4 && row[k]; k++) at += mi_utf8_enc(row[k], buf + at);
        } else {
            memcpy(buf + at, s.p + i, (size_t)n); at += n;
        }
        prev_cased = mi_is_cased(u);
        i += n;
    }
    return mi_mk(buf, at);
}
static MiStr mi_upper(MiStr s) { return mi_casemap(s, 1); }
static MiStr mi_lower(MiStr s) { return mi_casemap(s, 0); }
"#);
    s
}

pub fn generate(prog: &Program, src_path: &str) -> Result<(String, Vec<String>, Option<String>), Vec<SiskinError>> {
    generate_opts(prog, src_path, false)
}

/// `siskin debug` 용: 문장마다 멈출 자리를 넣은 C 를 만듭니다(`rt_dbg.c`).
pub fn generate_debug(prog: &Program, src_path: &str) -> Result<(String, Vec<String>, Option<String>), Vec<SiskinError>> {
    generate_opts(prog, src_path, true)
}

fn generate_opts(prog: &Program, src_path: &str, dbg: bool) -> Result<(String, Vec<String>, Option<String>), Vec<SiskinError>> {
    let mut ty = Types::new(prog);
    ty.errors.clear();
    // 익명 함수의 인자 타입(적지 않은 것)을 알려면 타입 검사를 한 번 돌려야 합니다.
    {
        let mut chk = Types::new(prog);
        chk.check_program(prog);
        ty.lambda_sigs = chk.lambda_sigs;
    }
    let mut res = HashSet::new();
    res.insert("MiRes_int64_t".to_string());
    res.insert("MiRes_double".to_string());
    res.insert("MiRes_MiStr".to_string());
    res.insert("MiRes_MiJsonp".to_string());
    let mut g = CGen {
        ty,
        decls: String::new(),
        tdefs: Vec::new(),
        boxed: HashSet::new(),
        protos: String::new(),
        ffi: String::new(),
        used_externs: HashSet::new(),
        cpp_ffi: String::new(),
        cb_ffi: String::new(),
        cb_done: HashSet::new(),
        body: String::new(),
        indent: 0,
        errors: Vec::new(),
        cur_fn: String::new(),
        cur_ret: Ty::Unit,
        expect: None,
        renames: vec![HashMap::new()],
        pre: HashMap::new(),
        opt_types: {
            let mut o = HashSet::new();
            for n in ["MiOpt_MiStr", "MiOpt_int64_t", "MiOpt_double", "MiOpt_bool", "MiOpt_MiJsonp"] {
                o.insert(n.to_string());
            }
            o
        },
        res_types: res,
        tup_types: HashSet::new(),
        fn_ptr_types: HashSet::new(),
        mono_suffix: String::new(),
        mono_queue: Vec::new(),
        mono_done: HashSet::new(),
        tmp: 0,
        src_path: src_path.to_string(),
        last_line: 0,
        lifted: String::new(),
        thunks_done: HashSet::new(),
        lam_count: 0,
        copy_needed: HashMap::new(),
        copy_done: HashSet::new(),
        repr_done: HashSet::new(),
        uses_net: false,
        uses_conc: false,
        task_runners: HashSet::new(),
        uses_case: false,
        dbg,
        dbg_base: 0,
        gvars: String::new(),
        globals: Vec::new(),
        has_globals: false,
    };
    g.run(prog);
    g.emit_ffi_shims(prog);
    // `fn main() -> !Unit` 이면 실패했을 때 오류를 알리고 1로 끝냅니다.
    let main_call = match g.ty.fns.get("main").map(|m| m.ret.clone()) {
        Some(Ty::Fallible(inner, et)) => {
            let rn = g.res_name(&inner, &et, 0);
            let shown = if matches!(*et, Ty::Enum(_)) { g.repr_of(&et, "r.err".into(), 0) } else { "r.err".into() };
            format!(
                "{{ {} r = mu_main(); if (!r.ok) {{ MiStr m = {}; fflush(stdout); fprintf(stderr, \"{}: %.*s\\n\", (int)m.len, m.p); return 1; }} }}",
                rn, shown, tr!("오류", "error")
            )
        }
        _ => "mu_main();".to_string(),
    };
    // 동시성을 쓰면 main 이 끝난 뒤 남은 작업을 모두 기다립니다(인터프리터와 같게).
    let main_call = if g.uses_conc { format!("{} mi_tasks_finish();", main_call) } else { main_call };
    if g.errors.is_empty() {
        // 진단 언어를 굳혀 넣습니다. 런타임의 `MI_T(한국어, 영어)` 가 이걸 봅니다.
        let mut out = format!("#define MI_KO {}\n#define MI_T(ko, en) (MI_KO ? (ko) : (en))\n", if crate::lang::ko() { 1 } else { 0 });
        out.push_str(RUNTIME);
        out.push_str(include_str!("rt_sys.c"));
        if g.dbg {
            out.push_str("#define MI_DBG_ON 1\n");
            out.push_str(include_str!("rt_dbg.c"));
        }
        if g.uses_net {
            out.push_str(include_str!("rt_net.c"));
        }
        if g.uses_conc {
            out.push_str(include_str!("rt_conc.c"));
        }
        if g.uses_case {
            out.push_str(&case_runtime());
        }
        out.push_str(RUNTIME_RE);
        out.push_str(RUNTIME_JSON);
        out.push_str(&g.decls);
        out.push('\n');
        out.push_str(&g.ordered_tdefs());
        out.push('\n');
        out.push_str(&g.ffi);
        out.push('\n');
        out.push_str(&g.protos);
        out.push('\n');
        out.push_str(&g.gvars);
        out.push('\n');
        out.push_str(&g.cb_ffi);
        out.push('\n');
        out.push_str(&g.lifted);
        out.push('\n');
        out.push_str(&g.body);
        out.push_str(&
            "\nint main(int argc, char** argv) {\n\
             #ifdef _WIN32\n\
             \x20   mi_win_init(&argc, &argv);\n\
             #endif\n\
             \x20   mi_prog_args = mi_list_new((int64_t)sizeof(MiStr));\n\
             \x20   for (int i = 1; i < argc; i++) { MiStr s = mi_str(argv[i]); mi_list_push(&mi_prog_args, &s); }\n\
             \x20   DBG_INIT GLOBALS_INIT MAIN_CALL\n\
             \x20   return 0;\n\
             }\n"
            .replace("MAIN_CALL", &main_call)
            .replace("DBG_INIT ", if g.dbg { "mi_dbg_init(); " } else { "" })
            .replace("GLOBALS_INIT ", if g.has_globals { "mi_init_globals(); " } else { "" }),
        );
        let mut links = g.ty.links.clone();
        if g.uses_net && cfg!(windows) {
            // 윈도우: 소켓(ws2_32)과 인증서 목록(crypt32).
            links.push("ws2_32".into());
            links.push("crypt32".into());
        } else if g.uses_net && !links.iter().any(|l| l == "dl") {
            links.push("dl".into());
        }
        if (g.uses_conc || g.uses_net || g.dbg) && !cfg!(windows) && !links.iter().any(|l| l == "pthread") {
            links.push("pthread".into());
        }
        let cpp = if g.cpp_ffi.is_empty() { None } else { Some(g.cpp_ffi.clone()) };
        Ok((out, links, cpp))
    } else {
        Err(g.errors)
    }
}

impl CGen {
    fn w(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.body.push_str("    ");
        }
        self.body.push_str(s);
        self.body.push('\n');
    }

    /// 생성된 C의 이 지점이 원본 .skn 몇 번째 줄인지 알려 줍니다.
    /// 이게 있어야 C 컴파일러 오류와 디버거가 .skn 파일을 가리킵니다.
    fn line_mark(&mut self, line: usize) {
        if line == 0 || line == self.last_line {
            return;
        }
        self.last_line = line;
        let (file, l) = self.src_of(line);
        let path = file.replace('\\', "\\\\").replace('"', "\\\"");
        let _ = writeln!(self.body, "#line {} \"{}\"", l, path);
    }

    /// 합친 파일의 줄 번호를 (파일 이름, 그 파일 안의 줄) 로.
    fn src_of(&self, line: usize) -> (String, usize) {
        match crate::error::locate(line) {
            Some((f, _, l)) => (f, l),
            None => (self.src_path.clone(), line),
        }
    }

    fn next_tmp(&mut self) -> String {
        self.tmp += 1;
        format!("_t{}", self.tmp)
    }

    fn push_scope(&mut self) {
        self.renames.push(HashMap::new());
        self.ty.push_scope();
    }

    fn pop_scope(&mut self) {
        self.renames.pop();
        self.ty.pop_scope();
    }

    /// 이름을 실제 C 식으로. 좁혀진 `?T` 변수는 `.val`이 붙습니다.
    fn cname(&self, n: &str) -> String {
        for s in self.renames.iter().rev() {
            if let Some(c) = s.get(n) {
                return c.clone();
            }
        }
        local(n)
    }

    fn infer(&mut self, e: &Expr) -> Ty {
        let t = self.ty.infer(e);
        self.ty.errors.clear();
        t
    }

    // ------------------------------------------------------------- 타입 이름

    fn ctype(&mut self, t: &Ty, line: usize) -> String {
        match t {
            Ty::Int => "int64_t".into(),
            Ty::Float => "double".into(),
            Ty::Bool => "bool".into(),
            Ty::Str => "MiStr".into(),
            Ty::Unit => "void".into(),
            Ty::List(_) => "MiList".into(),
            Ty::Dict(_, _) => "MiDict".into(),
            Ty::Struct(n) => format!("struct {}", mangle(n)),
            Ty::Enum(n) => format!("struct {}", mangle(n)),
            Ty::Raw(inner) => {
                let c = self.ctype(inner, line);
                format!("MI_PTR({})", c)
            }
            Ty::Arena => "MiArena".into(),
            Ty::Json => "MiJson*".into(),
            Ty::Optional(inner) => self.opt_name(inner, line),
            Ty::Fallible(inner, e) => self.res_name(inner, e, line),
            Ty::Tuple(elems) => self.tup_name(elems, line),
            Ty::Fn(params, ret) => {
                self.fn_ptr_name(params, ret, line);
                "MiClo".into()
            }
            Ty::Task(_) => {
                self.uses_conc = true;
                "MiTask*".into()
            }
            Ty::Chan(_) => {
                self.uses_conc = true;
                "MiChan*".into()
            }
            Ty::NoneTy => {
                self.errors.push(
                    cerr("C0011", tr!("`none`만으로는 타입을 정할 수 없습니다", "cannot infer a type from `none` alone"), line, 1)
                        .with_fix(tr!("`?T` 타입을 명시하세요", "annotate the optional type `?T`")),
                );
                "int64_t".into()
            }
            other => {
                self.errors.push(
                    cerr("C0001", tr!(format!("{} 타입은 아직 네이티브 컴파일이 안 됩니다", other), format!("type {} is not supported by the native build yet", other)), line, 1)
                        .with_fix(tr!("`siskin run`(인터프리터)에서는 동작할 수 있습니다", "it may work under `siskin run` (the interpreter)")),
                );
                "int64_t".into()
            }
        }
    }

    /// C 쪽 시그니처에 쓸 타입. Str은 `const char*`, `*T`는 진짜 `T*`입니다.
    /// 내 Siskin 함수를 C 라이브러리가 부를 수 있는 모양으로 감쌉니다.
    ///
    /// C 라이브러리는 자기가 정한 생김새(`int (*)(void*, int)`)로 부릅니다.
    /// Siskin 쪽은 모든 정수를 64비트로 다루므로 그대로는 안 맞습니다.
    /// 그래서 가운데 다리를 하나 놓고, 거기서 값을 옮겨 담아 부릅니다.
    fn make_callback(
        &mut self,
        fname: &str,
        cps: &[String],
        cret: &str,
        line: usize,
    ) -> Option<String> {
        let sig = self.ty.fns.get(fname).cloned()?;
        if sig.decl.is_extern || !sig.decl.generics.is_empty() {
            return None;
        }
        if sig.params.len() != cps.len() {
            return None;
        }
        let tramp = format!("mi_cb_{}", mangle(fname));
        if self.cb_done.contains(&tramp) {
            return Some(tramp);
        }
        self.cb_done.insert(tramp.clone());

        let mut ps: Vec<String> = Vec::new();
        let mut args: Vec<String> = Vec::new();
        for (i, ct) in cps.iter().enumerate() {
            ps.push(format!("{} a{}", ct, i));
            let want = sig.params.get(i).map(|p| p.1.clone()).unwrap_or(Ty::Int);
            args.push(match want {
                Ty::Str => format!("mi_from_c((const char*)a{})", i),
                Ty::Float => format!("(double)a{}", i),
                Ty::Bool => format!("(bool)a{}", i),
                _ => {
                    if ct.contains('*') {
                        format!("(int64_t)(void*)a{}", i)
                    } else {
                        format!("(int64_t)a{}", i)
                    }
                }
            });
        }
        let rname = self.fn_cname(&sig.decl, &None);
        let call = format!("{}({})", rname, args.join(", "));
        let body = if cret.trim() == "void" || sig.ret == Ty::Unit {
            format!("    {};\n", call)
        } else if sig.ret == Ty::Str {
            format!("    return ({})mi_cstr({});\n", cret, call)
        } else {
            format!("    return ({}){};\n", cret, call)
        };
        let plist = if ps.is_empty() { "void".to_string() } else { ps.join(", ") };
        let _ = write!(
            self.cb_ffi,
            "/* {} 를 C 쪽에 넘기기 위한 다리 */\nstatic {} {}({}) {{\n{}}}\n",
            fname,
            if cret.trim().is_empty() { "void" } else { cret },
            tramp,
            plist,
            body
        );
        let _ = line;
        Some(tramp)
    }

    /// 헤더에서 가져온 C 함수의 껍데기를 냅니다.
    ///
    /// 예전에는 `extern int64_t mx_crc32(...) __asm__("crc32")` 처럼 심볼에
    /// 바로 붙였는데, 그러면 C 함수가 32비트 `int` 를 돌려줄 때 위쪽 32비트가
    /// 쓰레기값이 됩니다. 헤더를 include 하고 진짜 타입으로 부른 뒤 옮겨 담으면
    /// C 컴파일러가 타입을 대신 검사해 주기도 합니다.
    fn emit_ffi_shims(&mut self, prog: &Program) {
        let mut decls: Vec<crate::ast::Shared<FnDecl>> = Vec::new();
        for s in &prog.stmts {
            if let Stmt::Fn(f) = s {
                if f.is_extern && f.c_sig.is_some() && self.used_externs.contains(&f.name) {
                    decls.push(f.clone());
                }
            }
        }
        if decls.is_empty() {
            return;
        }
        let mut seen: Vec<String> = Vec::new();
        let mut seen_cpp: Vec<String> = Vec::new();
        for f in &decls {
            let cs = f.c_sig.as_ref().unwrap();
            let h = cs.header.clone();
            let inc = if h.starts_with('.') || h.starts_with('/') || std::path::Path::new(h.as_str()).is_absolute() {
                format!("#include \"{}\"\n", h)
            } else {
                format!("#include <{}>\n", h)
            };
            if cs.cpp {
                if !seen_cpp.contains(&h) {
                    seen_cpp.push(h.clone());
                    self.cpp_ffi.push_str(&inc);
                }
            } else if !seen.contains(&h) {
                seen.push(h.clone());
                self.ffi.push_str(&inc);
            }
        }
        if !self.cpp_ffi.is_empty() {
            self.cpp_ffi.insert_str(
                0,
                "// Siskin이 자동으로 만든 C++ 다리 파일입니다. 고치지 마세요.\n\
                 #include <cstdint>\n#include <string>\n",
            );
            self.cpp_ffi.push_str(
                "\n// std::string 을 글자열로 넘길 때 잠깐 담아 두는 칸입니다.\n\
                 static const char* mi_cpp_hold(const std::string& s) {\n\
                 \x20   static std::string pool[16];\n\
                 \x20   static int at = 0;\n\
                 \x20   at = (at + 1) & 15;\n\
                 \x20   pool[at] = s;\n\
                 \x20   return pool[at].c_str();\n\
                 }\n\n",
            );
        }
        self.ffi.push('\n');
        for f in &decls {
            let cs = f.c_sig.as_ref().unwrap().clone();
            // C++ 은 껍데기를 이미 만들어 뒀습니다. C 쪽에는 "있다"고만 알립니다.
            if let Some(shim) = &cs.shim {
                self.cpp_ffi.push_str(shim);
                self.cpp_ffi.push('\n');
                let ret_ty = match &f.ret {
                    Some(te) => self.ty.resolve(te, f.line),
                    None => Ty::Unit,
                };
                let rt = self.cty_ffi(&ret_ty, f.line);
                let mut ps: Vec<String> = Vec::new();
                for p in &f.params {
                    let t = match &p.ty {
                        Some(te) => self.ty.resolve(te, f.line),
                        None => Ty::Int,
                    };
                    ps.push(self.cty_ffi(&t, f.line));
                }
                let plist = if ps.is_empty() { "void".to_string() } else { ps.join(", ") };
                let _ = writeln!(self.ffi, "extern {} mx_{}({});", rt, f.name, plist);
                continue;
            }
            let ret_ty = match &f.ret {
                Some(te) => self.ty.resolve(te, f.line),
                None => Ty::Unit,
            };
            let rt = self.cty_ffi(&ret_ty, f.line);
            let mut ps: Vec<String> = Vec::new();
            let mut args: Vec<String> = Vec::new();
            let mut pre: Vec<String> = Vec::new();
            let mut post: Vec<String> = Vec::new();
            for (i, p) in f.params.iter().enumerate() {
                let t = match &p.ty {
                    Some(te) => self.ty.resolve(te, f.line),
                    None => Ty::Int,
                };
                let c = self.cty_ffi(&t, f.line);
                if p.conv == Convention::Inout {
                    // `T**` 자리. 진짜 C 변수를 하나 두고, 부르기 전에 채워 넣고
                    // 부른 뒤에 Siskin 쪽 변수로 옮겨 담습니다.
                    let ct = cs.params.get(i).cloned().unwrap_or_else(|| "void**".into());
                    let inner = ct.trim_end().strip_suffix('*').unwrap_or("void*").trim().to_string();
                    ps.push(format!("{}* a{}", c, i));
                    pre.push(format!("    {} t{} = ({})(void*)(*a{});", inner, i, inner, i));
                    post.push(format!("    *a{} = (int64_t)(void*)t{};", i, i));
                    args.push(format!("&t{}", i));
                    continue;
                }
                ps.push(format!("{} a{}", c, i));
                // 원래 C 타입으로 되돌려서 넘깁니다.
                match cs.params.get(i) {
                    Some(ct) => args.push(format!("({})a{}", ct, i)),
                    None => args.push(format!("a{}", i)),
                }
            }
            let plist = if ps.is_empty() { "void".to_string() } else { ps.join(", ") };
            let call = format!("{}({})", cs.call, args.join(", "));
            let mut body = String::new();
            for l in &pre {
                body.push_str(l);
                body.push('\n');
            }
            if ret_ty == Ty::Unit {
                body.push_str(&format!("    {};\n", call));
                for l in &post {
                    body.push_str(l);
                    body.push('\n');
                }
            } else if post.is_empty() {
                body.push_str(&format!("    return ({}){};\n", rt, call));
            } else {
                body.push_str(&format!("    {} r = ({}){};\n", rt, rt, call));
                for l in &post {
                    body.push_str(l);
                    body.push('\n');
                }
                body.push_str("    return r;\n");
            }
            let _ = write!(self.ffi, "static {} mx_{}({}) {{\n{}}}\n", rt, f.name, plist, body);
        }
        self.ffi.push('\n');
    }

    fn cty_ffi(&mut self, t: &Ty, line: usize) -> String {
        match t {
            Ty::Str => "const char*".into(),
            Ty::Raw(inner) => {
                let c = self.ctype(inner, line);
                format!("{}*", c)
            }
            // 라이브러리에 넘기는 함수 자리: 다리 함수(mi_cb_*)의 주소가 들어옵니다.
            Ty::Fn(..) => "void*".into(),
            other => self.ctype(other, line),
        }
    }

    /// 값으로 품는 타입 정의를 모아 둡니다. `deps` 는 속에 값으로 든 C 타입들입니다.
    /// typedef 이름(MiOpt_ 등)은 앞쪽에 `typedef struct 이름 이름;` 을 먼저 냅니다.
    fn add_tdef(&mut self, name: &str, text: String, deps: &[String]) {
        if !name.starts_with("struct ") {
            let _ = writeln!(self.decls, "typedef struct {} {};", name, name);
        }
        let deps = deps.iter().filter(|d| !d.ends_with('*')).cloned().collect();
        self.tdefs.push((name.to_string(), text, deps));
    }

    /// 모아 둔 타입 정의를 "속에 든 것 먼저" 순서로 냅니다.
    fn ordered_tdefs(&self) -> String {
        let idx: HashMap<&str, usize> = self.tdefs.iter().enumerate().map(|(i, t)| (t.0.as_str(), i)).collect();
        let mut state = vec![0u8; self.tdefs.len()];
        let mut out = String::new();
        fn visit(i: usize, g: &CGen, idx: &HashMap<&str, usize>, state: &mut Vec<u8>, out: &mut String) {
            if state[i] != 0 {
                return;
            }
            state[i] = 1;
            for d in &g.tdefs[i].2 {
                if let Some(&j) = idx.get(d.as_str()) {
                    visit(j, g, idx, state, out);
                }
            }
            state[i] = 2;
            out.push_str(&g.tdefs[i].1);
            if !g.tdefs[i].1.ends_with('\n') {
                out.push('\n');
            }
        }
        for i in 0..self.tdefs.len() {
            visit(i, self, &idx, &mut state, &mut out);
        }
        out
    }

    /// 이 타입의 값이 (포인터를 거치지 않고) 구조체 `target` 을 품을 수 있는가.
    fn reaches_by_value(&mut self, t: &Ty, target: &str, seen: &mut HashSet<String>) -> bool {
        match t {
            Ty::Struct(n) => {
                if n == target {
                    return true;
                }
                if !seen.insert(n.clone()) {
                    return false;
                }
                let sd = match self.ty.structs.get(n).cloned() {
                    Some(sd) => sd,
                    None => return false,
                };
                if !sd.generics.is_empty() {
                    return false;
                }
                sd.fields.iter().any(|f| match &f.ty {
                    Some(te) => {
                        let ft = self.ty.resolve(te, sd.line);
                        !self.boxed.contains(&(n.clone(), f.name.clone())) && self.reaches_by_value(&ft, target, seen)
                    }
                    None => false,
                })
            }
            Ty::Enum(n) => {
                if !seen.insert(format!("enum {}", n)) {
                    return false;
                }
                let ed = match self.ty.enums.get(n).cloned() {
                    Some(ed) => ed,
                    None => return false,
                };
                ed.variants.iter().any(|v| {
                    v.fields.iter().any(|f| match &f.ty {
                        Some(te) => {
                            let ft = self.ty.resolve(te, ed.line);
                            // 열거형 필드는 원래 포인터로 담습니다.
                            !matches!(ft, Ty::Enum(_)) && self.reaches_by_value(&ft, target, seen)
                        }
                        None => false,
                    })
                })
            }
            Ty::Optional(i) => self.reaches_by_value(i, target, seen),
            Ty::Fallible(i, e) => self.reaches_by_value(i, target, seen) || self.reaches_by_value(e, target, seen),
            Ty::Tuple(ts) => ts.iter().any(|x| self.reaches_by_value(x, target, seen)),
            _ => false,
        }
    }

    /// 식 `obj.field` 의 필드가 힙 상자에 담긴 필드인가.
    fn field_boxed(&mut self, obj: &Expr, field: &str) -> bool {
        if self.boxed.is_empty() {
            return false;
        }
        match self.infer(obj) {
            Ty::Struct(sn) => self.boxed.contains(&(sn, field.to_string())),
            _ => false,
        }
    }

    /// 결과 타입이 `rt` 인 작업을 돌리는 C 함수. 클로저를 부르고 결과를 작업 칸에 담습니다.
    fn task_runner(&mut self, rt: &Ty, line: usize) -> String {
        let name = format!("mi_trun_{}", ty_key(rt));
        if self.task_runners.insert(name.clone()) {
            let fp = self.fn_ptr_name(&[], rt, line);
            let _ = writeln!(self.protos, "static void {}(MiTask* t);", name);
            let body = if matches!(rt, Ty::Unit) {
                format!("    (({})t->clo.fn)(t->clo.env);\n", fp)
            } else {
                let c = self.ctype(rt, line);
                format!("    {c} r = (({fp})t->clo.fn)(t->clo.env);\n    memcpy(t->res, &r, sizeof({c}));\n", c = c, fp = fp)
            };
            let _ = write!(self.lifted, "static void {}(MiTask* t) {{\n{}}}\n\n", name, body);
        }
        name
    }

    fn opt_name(&mut self, inner: &Ty, line: usize) -> String {
        let c = self.ctype(inner, line);
        let name = format!("MiOpt_{}", sanitize(&c));
        if !self.opt_types.contains(&name) {
            self.opt_types.insert(name.clone());
            self.add_tdef(&name, format!("struct {} {{ bool has; {} val; }};", name, c), &[c.clone()]);
        }
        name
    }

    /// 결과 구조체 이름. 오류 타입이 Str 이면 예전과 같은 `MiRes_T`,
    /// enum 이면 `MiRes_T_E_이름` 이고 `err` 칸에 그 enum 값이 들어갑니다.
    fn res_name(&mut self, inner: &Ty, err: &Ty, line: usize) -> String {
        let c = if *inner == Ty::Unit || *inner == Ty::Unknown {
            "int64_t".to_string()
        } else {
            self.ctype(inner, line)
        };
        let ec = self.err_ctype(err, line);
        let name = if ec == "MiStr" {
            format!("MiRes_{}", sanitize(&c))
        } else {
            format!("MiRes_{}_E_{}", sanitize(&c), sanitize(&ec))
        };
        if !self.res_types.contains(&name) {
            self.res_types.insert(name.clone());
            self.add_tdef(&name, format!("struct {} {{ bool ok; {} val; {} err; }};", name, c, ec), &[c.clone(), ec.clone()]);
        }
        name
    }

    fn err_ctype(&mut self, err: &Ty, line: usize) -> String {
        match err {
            Ty::Enum(_) => self.ctype(err, line),
            _ => "MiStr".to_string(),
        }
    }

    /// 튜플 C 구조체 이름. 원소 C타입들을 이어 붙여 이름을 만들고,
    /// 없으면 aux에 typedef를 냅니다(구조체 선언 뒤에 옵니다).
    fn tup_name(&mut self, elems: &[Ty], line: usize) -> String {
        let ctypes: Vec<String> = elems.iter().map(|e| self.ctype(e, line)).collect();
        let sanitized: Vec<String> = ctypes.iter().map(|c| sanitize(c)).collect();
        let name = format!("MiTup_{}", sanitized.join("_"));
        if !self.tup_types.contains(&name) {
            self.tup_types.insert(name.clone());
            let mut fields = String::new();
            for (i, c) in ctypes.iter().enumerate() {
                let _ = write!(fields, "{} f{}; ", c, i);
            }
            self.add_tdef(&name, format!("struct {} {{ {}}};", name, fields), &ctypes);
        }
        name
    }

    /// 함수 값의 C 타입(함수 포인터). 이름이 가운데 들어가는 C 문법 때문에
    /// typedef로 만들어 이름 하나로 씁니다: `typedef R (*MiFn_...)(A, B);`
    fn fn_ptr_name(&mut self, params: &[Ty], ret: &Ty, line: usize) -> String {
        let pcs: Vec<String> = params.iter().map(|p| self.ctype(p, line)).collect();
        let rc = self.ctype(ret, line);
        let mut key = sanitize(&rc);
        for p in &pcs {
            key.push('_');
            key.push_str(&sanitize(p));
        }
        let name = format!("MiFn_{}", key);
        if !self.fn_ptr_types.contains(&name) {
            self.fn_ptr_types.insert(name.clone());
            // 첫 자리는 언제나 붙잡은 값 묶음(env)입니다.
            let mut all = vec!["void*".to_string()];
            all.extend(pcs);
            let _ = writeln!(self.decls, "typedef {} (*{})({});", rc, name, all.join(", "));
        }
        name
    }

    /// 사전의 키/값 C 타입과 키 종류(0=바이트 비교, 1=문자열)를 냅니다.
    fn dict_parts(&mut self, t: &Ty, line: usize) -> (String, String, i32) {
        match t {
            Ty::Dict(k, v) => {
                let kc = self.ctype(k, line);
                let vc = self.ctype(v, line);
                let kind = if **k == Ty::Str { 1 } else { 0 };
                (kc, vc, kind)
            }
            _ => ("MiStr".into(), "int64_t".into(), 1),
        }
    }

    fn elem_ctype(&mut self, list_ty: &Ty, line: usize) -> String {
        match list_ty {
            Ty::List(inner) => self.ctype(inner, line),
            _ => "int64_t".into(),
        }
    }

    // -------------------------------------------------------------- 최상위

    fn run(&mut self, prog: &Program) {
        // 모든 구조체·열거형을 먼저 알려 둡니다(포인터·함수 원형에서 쓸 수 있게).
        for s in &prog.stmts {
            match s {
                Stmt::Struct(sd) => {
                    let _ = writeln!(self.decls, "struct {};", mangle(&sd.name));
                }
                Stmt::Enum(ed) => {
                    let _ = writeln!(self.decls, "struct {};", mangle(&ed.name));
                }
                _ => {}
            }
        }
        // 자기 자신을 품는 `?구조체`/`!구조체` 필드는 상자에 담습니다(`next: ?Node`).
        for s in &prog.stmts {
            if let Stmt::Struct(sd) = s {
                if !sd.generics.is_empty() {
                    continue;
                }
                for f in &sd.fields {
                    if let Some(te) = &f.ty {
                        let ft = self.ty.resolve(te, sd.line);
                        if matches!(ft, Ty::Optional(_) | Ty::Fallible(..)) {
                            let mut seen = HashSet::new();
                            if self.reaches_by_value(&ft, &sd.name, &mut seen) {
                                self.boxed.insert((sd.name.clone(), f.name.clone()));
                            }
                        }
                    }
                }
            }
        }
        for s in &prog.stmts {
            match s {
                Stmt::Struct(sd) => {
                    let mut d = format!("struct {} {{\n", mangle(&sd.name));
                    let mut deps = Vec::new();
                    for f in &sd.fields {
                        let t = match &f.ty {
                            Some(te) => {
                                let r = self.ty.resolve(te, sd.line);
                                let c = self.ctype(&r, sd.line);
                                if self.boxed.contains(&(sd.name.clone(), f.name.clone())) {
                                    format!("{}*", c)
                                } else {
                                    c
                                }
                            }
                            None => "int64_t".into(),
                        };
                        let _ = writeln!(d, "    {} {};", t, local(&f.name));
                        deps.push(t);
                    }
                    d.push_str("};\n");
                    self.add_tdef(&format!("struct {}", mangle(&sd.name)), d, &deps);
                }
                Stmt::Enum(ed) => {
                    // 태그 + 공용체. 변형마다 자기 데이터를 가집니다.
                    let mut d = String::new();
                    for (i, v) in ed.variants.iter().enumerate() {
                        let _ = writeln!(self.decls, "#define {}_{} {}", mangle(&ed.name), v.name, i);
                    }
                    let mut deps = Vec::new();
                    let _ = writeln!(d, "struct {} {{", mangle(&ed.name));
                    d.push_str("    int32_t tag;\n    union {\n");
                    for v in &ed.variants {
                        if v.fields.is_empty() {
                            let _ = writeln!(d, "        struct {{ char _empty; }} {};", v.name);
                            continue;
                        }
                        let _ = write!(d, "        struct {{ ");
                        for f in &v.fields {
                            let r = match &f.ty {
                                Some(te) => self.ty.resolve(te, ed.line),
                                None => Ty::Int,
                            };
                            // 재귀 가능성이 있는 열거형 필드는 포인터로 담습니다.
                            // 값으로 담으면 자기 자신을 품어 크기가 무한이 됩니다.
                            let t = if matches!(r, Ty::Enum(_)) {
                                format!("{}*", self.ctype(&r, ed.line))
                            } else {
                                self.ctype(&r, ed.line)
                            };
                            let _ = write!(d, "{} {}; ", t, local(&f.name));
                            deps.push(t);
                        }
                        let _ = writeln!(d, "}} {};", v.name);
                    }
                    d.push_str("    } u;\n};\n");
                    self.add_tdef(&format!("struct {}", mangle(&ed.name)), d, &deps);
                }
                _ => {}
            }
        }

        // 최상위 `let` 상수: C 전역 변수로 두고, main 전에 한 번 채웁니다(`mi_init_globals`).
        for s in &prog.stmts {
            if let Stmt::Let { name, ty, value, line, .. } = s {
                let target = match ty {
                    Some(t) => self.ty.resolve(t, *line),
                    None => self.infer(value),
                };
                let c = self.ctype(&target, *line);
                let _ = writeln!(self.gvars, "static {} {};", c, local(name));
                self.ty.declare(name, target);
                self.globals.push(s.clone());
            }
        }
        for s in &prog.stmts {
            match s {
                Stmt::Fn(f) => {
                    // 제네릭 함수는 지금 원형을 못 냅니다(C에 `T`가 없음).
                    // 호출될 때 실제 타입으로 특수화해서 냅니다.
                    if !f.generics.is_empty() {
                        continue;
                    }
                    if f.is_extern {
                        // 헤더에서 자동으로 가져온 함수는 껍데기를 따로 냅니다
                        // (호출된 것만, 아래 `emit_ffi_shims` 에서).
                        if f.c_sig.is_some() {
                            continue;
                        }
                        // 손으로 적은 `extern "C" fn` 은 예전처럼 심볼에 바로 붙입니다.
                        let rt = match &f.ret {
                            Some(te) => {
                                let r = self.ty.resolve(te, f.line);
                                self.cty_ffi(&r, f.line)
                            }
                            None => "void".into(),
                        };
                        let mut ps: Vec<String> = Vec::new();
                        for p in &f.params {
                            let t = match &p.ty {
                                Some(te) => self.ty.resolve(te, f.line),
                                None => Ty::Int,
                            };
                            ps.push(self.cty_ffi(&t, f.line));
                        }
                        let args = if ps.is_empty() { "void".to_string() } else { ps.join(", ") };
                        let _ = writeln!(
                            self.protos,
                            "extern {} mx_{}({}) __asm__(\"{}\");",
                            rt, f.name, args, f.name
                        );
                        continue;
                    }
                    if !f.ensures.is_empty() {
                        let p = self.proto_named(f, None, "__body");
                        let _ = writeln!(self.protos, "{};", p);
                    }
                    let p = self.proto(f, None);
                    let _ = writeln!(self.protos, "{};", p);
                }
                Stmt::Struct(sd) => {
                    for m in &sd.methods {
                        if !m.ensures.is_empty() {
                            let p = self.proto_named(m, Some(sd.name.clone()), "__body");
                            let _ = writeln!(self.protos, "{};", p);
                        }
                        let p = self.proto(m, Some(sd.name.clone()));
                        let _ = writeln!(self.protos, "{};", p);
                    }
                }
                Stmt::Enum(ed) => {
                    for m in &ed.methods {
                        let p = self.proto(m, Some(ed.name.clone()));
                        let _ = writeln!(self.protos, "{};", p);
                    }
                }
                _ => {}
            }
        }

        for s in &prog.stmts {
            match s {
                Stmt::Fn(f) => {
                    // 제네릭 함수 본문은 호출될 때 특수화해서 냅니다(아래 대기열).
                    if !f.is_extern && f.generics.is_empty() {
                        self.gen_fn(f, None);
                    }
                }
                Stmt::Struct(sd) => {
                    let sd = sd.clone();
                    for m in &sd.methods {
                        self.gen_fn(m, Some(sd.name.clone()));
                    }
                }
                Stmt::Enum(ed) => {
                    let ed = ed.clone();
                    for m in &ed.methods {
                        self.gen_fn(m, Some(ed.name.clone()));
                    }
                }
                Stmt::Interface(_) | Stmt::Import { .. } | Stmt::Link(_, _) | Stmt::CHeader { .. } | Stmt::Let { .. } => {}
                _ => {
                    self.errors.push(
                        cerr("C0003", tr!("최상위에는 함수와 타입 선언만 올 수 있습니다", "only function and type declarations are allowed at the top level"), 1, 1)
                            .with_fix(tr!("실행할 코드는 `fn main():` 안에 넣으세요", "put code to run inside `fn main():`")),
                    );
                }
            }
        }

        if !self.globals.is_empty() {
            self.gen_globals_init();
        }

        // 제네릭 특수화 대기열을 비웁니다. 특수화 본문을 내는 동안 또 다른
        // 특수화가 필요해질 수 있으므로(중첩 제네릭 호출), 빌 때까지 돕니다.
        while let Some((decl, subst, suffix)) = self.mono_queue.pop() {
            self.mono_suffix = suffix;
            self.ty.enter_mono(&decl.generics, subst);
            let p = self.proto(&decl, None);
            let _ = writeln!(self.protos, "{};", p);
            self.gen_fn(&decl, None);
            self.ty.exit_mono();
            self.mono_suffix = String::new();
        }
    }

    fn fn_cname(&self, f: &FnDecl, owner: &Option<String>) -> String {
        let base = match owner {
            Some(o) => format!("mu_{}_{}", o, f.name),
            None => mangle(&f.name),
        };
        // 제네릭 함수는 특수화마다 이름 꼬리표를 붙입니다(mu_first__int64_t 등).
        if f.generics.is_empty() {
            base
        } else {
            format!("{}{}", base, self.mono_suffix)
        }
    }

    /// 제네릭 특수화의 이름 꼬리표를 타입 묶음에서 만듭니다.
    /// 예: `first`를 T=Int로 부르면 `__int64_t`.
    fn mono_suffix_of(&mut self, decl: &FnDecl, subst: &HashMap<String, Ty>) -> String {
        let mut parts = Vec::new();
        for g in &decl.generics {
            let ty = subst.get(g).cloned().unwrap_or(Ty::Unknown);
            let c = self.ctype(&ty, decl.line);
            parts.push(sanitize(&c));
        }
        format!("__{}", parts.join("_"))
    }

    fn proto(&mut self, f: &crate::ast::Shared<FnDecl>, owner: Option<String>) -> String {
        self.proto_named(f, owner, "")
    }

    fn proto_named(&mut self, f: &crate::ast::Shared<FnDecl>, owner: Option<String>, suffix: &str) -> String {
        let ret_ty = match &f.ret {
            Some(te) => self.ty.resolve(te, f.line),
            None => Ty::Unit,
        };
        let ret = self.ctype(&ret_ty, f.line);
        let name = format!("{}{}", self.fn_cname(f, &owner), suffix);
        let mut params = Vec::new();
        for p in &f.params {
            // `inout` 인자는 포인터로 받습니다. 그래야 호출한 쪽 변수를 바꿀 수 있습니다.
            let star = if p.conv == Convention::Inout { "*" } else { "" };
            if p.is_self {
                let o = owner.clone().unwrap_or_default();
                params.push(format!("struct {}{} {}", mangle(&o), star, local("self")));
                continue;
            }
            let t = match &p.ty {
                Some(te) => self.ty.resolve(te, f.line),
                None => Ty::Unknown,
            };
            let c = self.ctype(&t, f.line);
            params.push(format!("{}{} {}", c, star, local(&p.name)));
        }
        let plist = if params.is_empty() { "void".to_string() } else { params.join(", ") };
        format!("static {} {}({})", ret, name, plist)
    }

    fn declare_params(&mut self, f: &crate::ast::Shared<FnDecl>, owner: &Option<String>) {
        for p in &f.params {
            if p.is_self {
                let o = owner.clone().unwrap_or_default();
                let t = if self.ty.enums.contains_key(&o) {
                    Ty::Enum(o)
                } else {
                    Ty::Struct(o)
                };
                self.ty.declare("self", t);
                if p.conv == Convention::Inout {
                    // `inout self`는 포인터로 들어오므로 본문에서 쓸 때 역참조합니다.
                    self.renames
                        .last_mut()
                        .unwrap()
                        .insert("self".into(), format!("(*{})", local("self")));
                }
                continue;
            }
            let t = match &p.ty {
                Some(te) => self.ty.resolve(te, f.line),
                None => Ty::Unknown,
            };
            self.ty.declare(&p.name, t);
            if p.conv == Convention::Inout {
                self.renames
                    .last_mut()
                    .unwrap()
                    .insert(p.name.clone(), format!("(*{})", local(&p.name)));
            }
        }
    }

    fn gen_fn(&mut self, f: &crate::ast::Shared<FnDecl>, owner: Option<String>) {
        let ret_ty = match &f.ret {
            Some(te) => self.ty.resolve(te, f.line),
            None => Ty::Unit,
        };
        self.cur_fn = f.name.clone();
        self.cur_ret = ret_ty.clone();
        self.ty.cur_decl = Some(f.clone());

        let has_ensures = !f.ensures.is_empty();
        let body_proto = if has_ensures {
            self.proto_named(f, owner.clone(), "__body")
        } else {
            self.proto(f, owner.clone())
        };

        // --- 본문 ---
        self.body.push_str(&body_proto);
        self.body.push_str(" {\n");
        self.indent = 1;
        self.dbg_base = self.ty.scope_depth();
        self.push_scope();
        self.declare_params(f, &owner);
        if self.dbg {
            self.w(&format!("MI_DBG_FN({});", c_string(&crate::ns::shown(&f.name))));
        }
        // `owned` 인자는 받은 쪽이 자기 복사본을 가집니다(값 의미론).
        for p in &f.params {
            if p.conv == Convention::Owned && !p.is_self {
                if let Some(te) = &p.ty {
                    let t = self.ty.resolve(te, f.line);
                    let cp = self.copy_value(local(&p.name), &t, f.line);
                    if cp != local(&p.name) {
                        self.w(&format!("{} = {};", local(&p.name), cp));
                    }
                }
            }
        }

        if !has_ensures && !f.requires.is_empty() {
            self.emit_requires(f);
        }
        for s in &f.body {
            self.stmt(s);
        }
        // `!T` 함수가 명시적 return 없이 끝나면 성공 결과를 채웁니다.
        // C에서 구조체를 돌려주는 함수가 그냥 끝나면 .ok가 쓰레기값이 됩니다.
        if let Ty::Fallible(inner, et) = self.cur_ret.clone() {
            let rn = self.res_name(&inner, &et, f.line);
            self.w(&format!("return ({}){{ .ok = true }};", rn));
        }
        self.pop_scope();
        self.indent = 0;
        self.body.push_str("}\n\n");

        // --- 계약 검사용 겉껍데기 ---
        if has_ensures {
            let wrapper = self.proto(f, owner.clone());
            self.body.push_str(&wrapper);
            self.body.push_str(" {\n");
            self.indent = 1;
            self.push_scope();
            self.declare_params(f, &owner);
            if !f.requires.is_empty() {
                self.emit_requires(f);
            }
            let args: Vec<String> = f
                .params
                .iter()
                .map(|p| if p.is_self { local("self") } else { local(&p.name) })
                .collect();
            let callee = format!("{}__body", self.fn_cname(f, &owner));
            let call = format!("{}({})", callee, args.join(", "));
            if ret_ty == Ty::Unit {
                self.w(&format!("{};", call));
            } else {
                let rc = self.ctype(&ret_ty, f.line);
                self.w(&format!("{} v_result = {};", rc, call));
                self.ty.declare("result", ret_ty.clone());
            }
            self.body.push_str("#ifndef MI_RELEASE\n");
            for en in &f.ensures {
                let c = self.expr(en);
                let text = crate::interp::render_expr(en);
                let fname = f.name.clone();
                let fl = f.line;
                let path = self.src_path.clone();
                let rendered = self.contract_message(&fname, "사후", &text, fl, &path);
                self.w(&format!("if (!({})) mi_contract_fail({});", c, c_string(&rendered)));
            }
            self.body.push_str("#endif\n");
            if ret_ty != Ty::Unit {
                self.w("return v_result;");
            }
            self.pop_scope();
            self.indent = 0;
            self.body.push_str("}\n\n");
        }
    }

    /// 계약이 깨졌을 때 보여 줄 문구를 컴파일 시점에 통째로 만들어 둡니다.
    /// 인터프리터(`siskin run`)와 한 글자도 다르지 않아야 합니다.
    fn contract_message(
        &self,
        fname: &str,
        kind: &str,
        cond: &str,
        line: usize,
        path: &str,
    ) -> String {
        let (code, kw, fix) = if kind == "사전" {
            ("E0229", "requires", tr!("호출하는 쪽에서 이 조건을 먼저 확인하세요", "check this condition at the call site first"))
        } else {
            ("E0231", "ensures", tr!("함수 본문이 약속한 결과를 내지 못했습니다", "the function body did not produce the promised result"))
        };
        let e = cerr(
            code,
            tr!(
                format!("`{}`의 {} 조건이 깨졌습니다: {} {}", fname, kind, kw, cond),
                format!(
                    "{} of `{}` violated: {} {}",
                    if kind == "사전" { "precondition" } else { "postcondition" },
                    fname,
                    kw,
                    cond
                )
            ),
            line,
            1,
        )
        .with_fix(fix);
        let src = std::fs::read_to_string(path).unwrap_or_default();
        e.render(path, &src)
    }

    fn emit_requires(&mut self, f: &crate::ast::Shared<FnDecl>) {
        self.body.push_str("#ifndef MI_RELEASE\n");
        for r in &f.requires {
            let c = self.expr(r);
            let text = crate::interp::render_expr(r);
            let fname = f.name.clone();
            let fl = f.line;
            let path = self.src_path.clone();
            let rendered = self.contract_message(&fname, "사전", &text, fl, &path);
            self.w(&format!("if (!({})) mi_contract_fail({});", c, c_string(&rendered)));
        }
        self.body.push_str("#endif\n");
    }

    // ------------------------------------------------------------- 값 맞추기

    /// `from` 타입의 C 식을 `to` 타입 자리에 넣을 수 있게 감쌉니다.
    fn coerce(&mut self, code: String, from: &Ty, to: &Ty, line: usize) -> String {
        if from == to {
            return code;
        }
        match to {
            Ty::Optional(inner) => {
                let name = self.opt_name(inner, line);
                match from {
                    Ty::NoneTy => format!("(({}){{ .has = false }})", name),
                    Ty::Optional(_) => code,
                    _ => format!("(({}){{ .has = true, .val = {} }})", name, code),
                }
            }
            Ty::Fallible(inner, et) => {
                let name = self.res_name(inner, et, line);
                match from {
                    Ty::Fallible(..) => code,
                    _ => format!("(({}){{ .ok = true, .val = {} }})", name, code),
                }
            }
            _ => code,
        }
    }

    /// `if x != none:` 의 참쪽에서 x를 벗겨진 값으로 보게 합니다.
    /// 무엇을 좁힐지는 타입 검사기와 같은 규칙(`Types::narrowing`)으로 정합니다.
    /// 변수는 `(x).val` 로 이름을 바꾸고, 구조체 필드(`t.due`)는 식을 만들 때
    /// `narrowed_expr` 로 알아보고 `.val` 을 붙입니다.
    fn narrow(&mut self, cond: &Expr, positive: bool, region: &Region) -> Vec<(String, String, Ty)> {
        let found = self.ty.narrowing(cond, positive, region);
        found
            .into_iter()
            .map(|(name, t)| {
                let code = if name.contains('.') { String::new() } else { format!("({}).val", self.cname(&name)) };
                (name, code, t)
            })
            .collect()
    }

    fn apply_narrow(&mut self, n: Vec<(String, String, Ty)>) {
        for (name, code, t) in n {
            if !name.contains('.') {
                self.renames.last_mut().unwrap().insert(name.clone(), code);
            }
            self.ty.declare(&name, t);
        }
    }

    // ------------------------------------------------------------------ 문장

    fn stmt(&mut self, s: &Stmt) {
        // 필드 좁히기는 대입을 따라 풀립니다(타입 검사기와 같은 자리에서).
        if let Stmt::Assign { target, value, .. } = s {
            self.ty.before_assign(target, value);
            self.stmt_body(s);
            self.ty.after_assign(target);
        } else {
            self.stmt_body(s);
        }
    }

    fn stmt_body(&mut self, s: &Stmt) {
        let line = match s {
            Stmt::Let { line, .. } => *line,
            Stmt::LetTuple { line, .. } => *line,
            Stmt::Assign { line, .. } => *line,
            Stmt::For { line, .. } => *line,
            Stmt::Match { line, .. } => *line,
            Stmt::Return(_, l, _) => *l,
            Stmt::Break(l, _) | Stmt::Continue(l, _) => *l,
            Stmt::Expr(e, _) => e.pos().0,
            Stmt::If { arms, .. } => arms.first().map(|(c, _)| c.pos().0).unwrap_or(0),
            Stmt::While { cond, .. } => cond.pos().0,
            _ => 0,
        };
        self.line_mark(line);
        if line > 0 && !matches!(s, Stmt::Fn(_) | Stmt::Struct(_) | Stmt::Enum(_)) {
            let (f, l) = self.src_of(line);
            self.w(&format!("MI_LINE({}, {});", c_string(&f), l));
            if self.dbg && !f.starts_with("<std.") {
                let hook = self.dbg_hook(line);
                self.w(&hook);
            }
        }
        match s {
            Stmt::Link(_, _) | Stmt::CHeader { .. } => {}

            Stmt::Arena { name, body, line } => {
                // 블록을 어떻게 빠져나가든(return, break 포함) 아레나는 통째로 풀립니다.
                self.w("{");
                self.indent += 1;
                self.push_scope();
                self.ty.declare(name, Ty::Arena);
                self.w(&format!(
                    "MiArena {} __attribute__((cleanup(mi_arena_cleanup))) = mi_arena_new();",
                    self.cname(name)
                ));
                let _ = line;
                for st in body {
                    self.stmt(st);
                }
                self.pop_scope();
                self.indent -= 1;
                self.w("}");
            }

            Stmt::Unsafe { body, .. } => {
                self.w("{");
                self.indent += 1;
                self.push_scope();
                for st in body {
                    self.stmt(st);
                }
                self.pop_scope();
                self.indent -= 1;
                self.w("}");
            }

            Stmt::Let { name, ty, value, line, catch, .. } => {
                let declared = ty.as_ref().map(|t| self.ty.resolve(t, *line));
                let vt_raw = self.infer(value);

                if let Some(c) = catch {
                    // `let x = <!T 식> catch e:` 형태
                    let (inner, err_ty) = match &vt_raw {
                        Ty::Fallible(i, e) => ((**i).clone(), (**e).clone()),
                        other => (other.clone(), Ty::Str),
                    };
                    let res_name = self.res_name(&inner, &err_ty, *line);
                    let target = declared.clone().unwrap_or_else(|| inner.clone());
                    let ic = self.ctype(&target, *line);
                    let tmp = self.next_tmp();
                    self.expect = Some(vt_raw.clone());
                    let v = self.expr(value);
                    self.expect = None;
                    self.w(&format!("{} {} = {};", res_name, tmp, v));
                    self.w(&format!("{} {} = ({}){{0}};", ic, local(name), ic));
                    self.w(&format!("if (!{}.ok) {{", tmp));
                    self.indent += 1;
                    self.push_scope();
                    let ec = self.err_ctype(&err_ty, *line);
                    self.w(&format!("{} {} = {}.err;", ec, local(&c.name), tmp));
                    self.w(&format!("(void){};", local(&c.name)));
                    self.ty.declare(&c.name, err_ty.clone());
                    // 마지막 줄이 값이면(`catch e:` 아래 `0` 같은) 실패했을 때 그 값을 넣습니다.
                    let fallback = crate::ast::catch_fallback(c)
                        .filter(|fe| !matches!(self.ty.infer_quiet_pub(fe), Ty::Unit));
                    let n = c.body.len() - if fallback.is_some() { 1 } else { 0 };
                    for s in &c.body[..n] {
                        self.stmt(s);
                    }
                    if let Some(fe) = fallback {
                        let ft = self.ty.infer_quiet_pub(fe);
                        self.expect = Some(target.clone());
                        let fv = self.expr(fe);
                        self.expect = None;
                        let fv = self.coerce(fv, &ft, &target, *line);
                        let fv = self.bind_copy(fv, fe, &target, *line);
                        self.w(&format!("{} = {};", local(name), fv));
                    }
                    self.pop_scope();
                    self.indent -= 1;
                    self.w("} else {");
                    self.indent += 1;
                    let okv = self.coerce(format!("{}.val", tmp), &inner, &target, *line);
                    self.w(&format!("{} = {};", local(name), okv));
                    self.indent -= 1;
                    self.w("}");
                    self.ty.declare(name, target);
                    return;
                }

                let target = declared.clone().unwrap_or_else(|| vt_raw.clone());
                let c = self.ctype(&target, *line);
                self.expect = Some(target.clone());
                let v = self.expr(value);
                self.expect = None;
                let v = self.coerce(v, &vt_raw, &target, *line);
                let v = self.bind_copy(v, value, &target, *line);
                self.w(&format!("{} {} = {};", c, local(name), v));
                self.ty.declare(name, target);
            }

            Stmt::LetTuple { names, value, line, .. } => {
                let vt = self.infer(value);
                let elems: Vec<Ty> = match &vt {
                    Ty::Tuple(es) => es.clone(),
                    _ => {
                        self.errors.push(cerr(
                            "C0019",
                            tr!("튜플로 풀어 받으려면 오른쪽이 튜플이어야 합니다", "tuple destructuring needs a tuple on the right"),
                            *line,
                            1,
                        ));
                        return;
                    }
                };
                let tc = self.ctype(&vt, *line);
                let tmp = self.next_tmp();
                self.expect = Some(vt.clone());
                let v = self.expr(value);
                self.expect = None;
                self.w(&format!("{} {} = {};", tc, tmp, v));
                for (i, name) in names.iter().enumerate() {
                    if let Some(ety) = elems.get(i) {
                        let ec = self.ctype(ety, *line);
                        let src = self.copy_value(format!("{}.f{}", tmp, i), ety, *line);
                        self.w(&format!("{} {} = {};", ec, local(name), src));
                        self.ty.declare(name, ety.clone());
                    }
                }
            }

            Stmt::Assign { target, op, value, line, catch, .. } => {
                // 사전 대입은 자리에 쓰는 게 아니라 넣는 함수를 부릅니다.
                if let (Expr::Index(o, k, il, _), None, None) = (target, op, catch) {
                    let dt = self.infer(o);
                    if let Ty::Dict(_, v) = dt.clone() {
                        let (kc, vc, _) = self.dict_parts(&dt, *il);
                        let lv = self.lvalue(o);
                        let kk = self.expr(k);
                        self.expect = Some((*v).clone());
                        let vv = self.expr(value);
                        self.expect = None;
                        let vv = self.bind_copy(vv, value, &v, *il);
                        self.w(&format!(
                            "{{ {} _k = {}; {} _v = {}; mi_dict_set(&{}, &_k, &_v); }}",
                            kc, kk, vc, vv, lv
                        ));
                        return;
                    }
                }
                if let Some(c) = catch {
                    // `x = f() catch e: ...` 는 `let 임시 = f() catch e: ...` 다음 `x = 임시` 로 풉니다.
                    let (l, col) = target.pos();
                    let tmpname = format!("__cv{}", self.next_tmp());
                    self.stmt(&Stmt::Let {
                        name: tmpname.clone(),
                        ty: None,
                        value: value.clone(),
                        mutable: false,
                        catch: Some(c.clone()),
                        line: *line,
                        col: 1,
                    });
                    self.stmt(&Stmt::Assign {
                        target: target.clone(),
                        op: op.clone(),
                        value: Expr::Ident(tmpname, l, col),
                        catch: None,
                        line: *line,
                        col: 1,
                    });
                    return;
                }
                let tt = self.infer(target);
                let vt = self.infer(value);
                let t = self.lvalue(target);
                self.expect = Some(tt.clone());
                let v = self.expr(value);
                self.expect = None;
                let v = self.coerce(v, &vt, &tt, *line);
                match op {
                    None => {
                        let v = self.bind_copy(v, value, &tt, *line);
                        self.w(&format!("{} = {};", t, v));
                    }
                    Some(o) => match (o, &tt) {
                        (BinOp::Div, Ty::Int) => self.w(&format!("{} = mi_div_i64({}, {});", t, t, v)),
                        (BinOp::Mod, Ty::Int) => self.w(&format!("{} = mi_mod_i64({}, {});", t, t, v)),
                        (BinOp::Div, Ty::Float) => self.w(&format!("{} = mi_div_f64({}, {});", t, t, v)),
                        (BinOp::Mod, Ty::Float) => self.w(&format!("{} = fmod({}, {});", t, t, v)),
                        (BinOp::Add, Ty::Str) => self.w(&format!("{} = mi_cat({}, {});", t, t, v)),
                        _ => {
                            let sym = match o {
                                BinOp::Add => "+=",
                                BinOp::Sub => "-=",
                                BinOp::Mul => "*=",
                                BinOp::Div => "/=",
                                _ => "+=",
                            };
                            self.w(&format!("{} {} {};", t, sym, v));
                        }
                    },
                }
            }

            Stmt::Expr(e, catch) => {
                if let Some(c) = catch {
                    let vt = self.infer(e);
                    let (inner, err_ty) = match &vt {
                        Ty::Fallible(i, e) => ((**i).clone(), (**e).clone()),
                        other => (other.clone(), Ty::Str),
                    };
                    let res_name = self.res_name(&inner, &err_ty, 1);
                    let tmp = self.next_tmp();
                    let v = self.expr(e);
                    self.w(&format!("{} {} = {};", res_name, tmp, v));
                    self.w(&format!("if (!{}.ok) {{", tmp));
                    self.indent += 1;
                    self.push_scope();
                    let ec = self.err_ctype(&err_ty, 1);
                    self.w(&format!("{} {} = {}.err;", ec, local(&c.name), tmp));
                    self.w(&format!("(void){};", local(&c.name)));
                    self.ty.declare(&c.name, err_ty.clone());
                    for s in &c.body {
                        self.stmt(s);
                    }
                    self.pop_scope();
                    self.indent -= 1;
                    self.w("}");
                    return;
                }
                let c = self.expr(e);
                self.w(&format!("{};", c));
            }

            Stmt::If { arms, els } => {
                for (i, (cond, body)) in arms.iter().enumerate() {
                    let n = self.narrow(cond, true, &Region::Block(body));
                    let c = self.expr(cond);
                    if i == 0 {
                        self.w(&format!("if ({}) {{", c));
                    } else {
                        self.w(&format!("}} else if ({}) {{", c));
                    }
                    self.indent += 1;
                    self.push_scope();
                    self.apply_narrow(n);
                    for s in body {
                        self.stmt(s);
                    }
                    self.pop_scope();
                    self.indent -= 1;
                }
                if let Some(b) = els {
                    let n = if arms.len() == 1 { self.narrow(&arms[0].0, false, &Region::Block(b)) } else { Vec::new() };
                    self.w("} else {");
                    self.indent += 1;
                    self.push_scope();
                    self.apply_narrow(n);
                    for s in b {
                        self.stmt(s);
                    }
                    self.pop_scope();
                    self.indent -= 1;
                }
                self.w("}");
                // 가드 절: 유일한 arm이 반드시 빠져나가면 그 뒤를 반대로 좁힙니다.
                if els.is_none() && arms.len() == 1 && block_diverges(&arms[0].1) {
                    let line = arms[0].0.pos().0;
                    let n = self.narrow(&arms[0].0, false, &Region::After(line));
                    self.apply_narrow(n);
                }
            }

            Stmt::While { cond, body } => {
                self.ty.loop_forget(body);
                let c = self.expr(cond);
                self.w(&format!("while ({}) {{", c));
                self.indent += 1;
                self.push_scope();
                for s in body {
                    self.stmt(s);
                }
                self.pop_scope();
                self.indent -= 1;
                self.w("}");
            }

            Stmt::For { var, var2, iter, body, line } => {
                self.ty.loop_forget(body);
                // `for k, v in d:` — 사전을 넣은 순서대로 돕니다.
                if let Some(v2) = var2 {
                    let it = self.infer(iter);
                    let (kc, vc) = match &it {
                        Ty::Dict(k, v) => (self.ctype(k, *line), self.ctype(v, *line)),
                        _ => {
                            self.errors.push(cerr(
                                "C0007",
                                tr!(format!("`for 키, 값 in ...`은 사전만 되는데 {}입니다", it), format!("`for key, value in ...` only works on dictionaries, found {}", it)),
                                *line,
                                1,
                            ));
                            return;
                        }
                    };
                    let src = self.expr(iter);
                    let dv = self.next_tmp();
                    let iv = self.next_tmp();
                    self.w("{");
                    self.indent += 1;
                    self.w(&format!("MiDict {} = {};", dv, src));
                    self.w(&format!("for (int64_t {} = 0; {} < {}.len; {}++) {{", iv, iv, dv, iv));
                    self.indent += 1;
                    self.w(&format!(
                        "{kc} {k} = *({kc}*)({d}.keys + {i} * {d}.ksz);",
                        kc = kc, k = local(var), d = dv, i = iv
                    ));
                    self.w(&format!(
                        "{vc} {v} = *({vc}*)({d}.vals + {i} * {d}.vsz);",
                        vc = vc, v = local(v2), d = dv, i = iv
                    ));
                    self.push_scope();
                    if let Ty::Dict(k, v) = &it {
                        self.ty.declare(var, (**k).clone());
                        self.ty.declare(v2, (**v).clone());
                    }
                    for s in body {
                        self.stmt(s);
                    }
                    self.pop_scope();
                    self.indent -= 1;
                    self.w("}");
                    self.indent -= 1;
                    self.w("}");
                    return;
                }
                // `for x in range(a, b):` 는 리스트를 만들지 않고 바로 C 루프로 냅니다.
                if let Expr::Call { callee, args, .. } = iter {
                    if let Expr::Ident(n, _, _) = callee.as_ref() {
                        if n == "range" && !args.is_empty() && args.len() <= 2 {
                            let (from, to) = if args.len() == 1 {
                                ("0".to_string(), self.expr(&args[0].value))
                            } else {
                                (self.expr(&args[0].value), self.expr(&args[1].value))
                            };
                            let tmp = self.next_tmp();
                            self.w(&format!("{{ const int64_t {} = {};", tmp, to));
                            self.indent += 1;
                            self.w(&format!(
                                "for (int64_t {} = {}; {} < {}; {}++) {{",
                                local(var),
                                from,
                                local(var),
                                tmp,
                                local(var)
                            ));
                            self.indent += 1;
                            self.push_scope();
                            self.ty.declare(var, Ty::Int);
                            for s in body {
                                self.stmt(s);
                            }
                            self.pop_scope();
                            self.indent -= 1;
                            self.w("}");
                            self.indent -= 1;
                            self.w("}");
                            return;
                        }
                    }
                }
                let it = self.infer(iter);
                // `for x in ch:` — 통로가 닫히고 빌 때까지 하나씩 받습니다.
                if let Ty::Chan(et) = &it {
                    let ec = self.ctype(et, *line);
                    let src = self.expr(iter);
                    let cv = self.next_tmp();
                    self.w(&format!("{{ MiChan* {} = {};", cv, src));
                    self.indent += 1;
                    self.w("for (;;) {");
                    self.indent += 1;
                    self.w(&format!("{} {}; if (!mi_chan_recv({}, &{})) break;", ec, local(var), cv, local(var)));
                    self.push_scope();
                    self.ty.declare(var, (**et).clone());
                    for s in body {
                        self.stmt(s);
                    }
                    self.pop_scope();
                    self.indent -= 1;
                    self.w("}");
                    self.indent -= 1;
                    self.w("}");
                    return;
                }
                let elem = match &it {
                    Ty::List(t) => (**t).clone(),
                    Ty::Str => Ty::Str,
                    Ty::Json => Ty::Json,
                    _ => {
                        self.errors.push(cerr(
                            "C0007",
                            tr!(format!("{} 값은 아직 네이티브에서 반복할 수 없습니다", it), format!("iterating over a {} value is not supported by the native build yet", it)),
                            *line,
                            1,
                        ));
                        return;
                    }
                };
                let ec = self.ctype(&elem, *line);
                let src = self.expr(iter);
                let lv = self.next_tmp();
                let iv = self.next_tmp();
                self.w("{");
                self.indent += 1;
                if it == Ty::Json {
                    self.w(&format!("MiJson* {} = {};", lv, src));
                    self.w(&format!(
                        "for (int64_t {i} = 0; {i} < ({l}->kind == 5 ? {l}->n : 0); {i}++) {{",
                        i = iv, l = lv
                    ));
                    self.indent += 1;
                    self.w(&format!("MiJson* {} = {}->items[{}];", local(var), lv, iv));
                } else if it == Ty::Str {
                    // 한 글자씩(UTF-8) 돕니다. 바이트 단위로 돌면 한글에서 범위를 넘습니다.
                    let nx = self.next_tmp();
                    self.w(&format!("MiStr {} = {};", lv, src));
                    self.w(&format!(
                        "for (int64_t {i} = 0, {n} = 0; {i} < {s}.len; {i} = {n}) {{",
                        i = iv, n = nx, s = lv
                    ));
                    self.indent += 1;
                    self.w(&format!("{n} = {i} + mi_utf8_step({s}, {i});", n = nx, i = iv, s = lv));
                    self.w(&format!("MiStr {} = mi_mk({}.p + {}, {} - {});", local(var), lv, iv, nx, iv));
                } else {
                    self.w(&format!("MiList {} = {};", lv, src));
                    self.w(&format!("for (int64_t {} = 0; {} < {}.len; {}++) {{", iv, iv, lv, iv));
                    self.indent += 1;
                    self.w(&format!(
                        "{} {} = *({}*)mi_list_at(&{}, {});",
                        ec,
                        local(var),
                        ec,
                        lv,
                        iv
                    ));
                }
                self.push_scope();
                self.ty.declare(var, elem);
                for s in body {
                    self.stmt(s);
                }
                self.pop_scope();
                self.indent -= 1;
                self.w("}");
                self.indent -= 1;
                self.w("}");
            }

            Stmt::Match { subject, cases, line } => self.gen_match(subject, cases, *line),

            Stmt::Return(v, l, _) => match v {
                Some(e) => {
                    let vt = self.infer(e);
                    let want = self.cur_ret.clone();
                    self.expect = Some(want.clone());
                    let c = self.expr(e);
                    self.expect = None;
                    let c = self.coerce(c, &vt, &want, *l);
                    self.w(&format!("return {};", c));
                }
                None => match self.cur_ret.clone() {
                    // `!Unit`(그리고 모든 `!T`)는 결과 구조체를 돌려주므로
                    // 값 없는 `return`도 성공 결과를 채워야 합니다. 안 그러면
                    // 구조체 반환 함수가 아무것도 안 돌려줘 .ok가 쓰레기값이 됩니다.
                    Ty::Fallible(inner, et) => {
                        let rn = self.res_name(&inner, &et, *l);
                        self.w(&format!("return ({}){{ .ok = true }};", rn));
                    }
                    _ => self.w("return;"),
                },
            },

            Stmt::Break(_, _) => self.w("break;"),
            Stmt::Continue(_, _) => self.w("continue;"),

            Stmt::Fn(f) => {
                // 함수 안의 `fn` 은 지역 클로저 변수가 됩니다.
                let sig_ps: Vec<Ty> = f
                    .params
                    .iter()
                    .map(|p| match &p.ty {
                        Some(te) => self.ty.resolve(te, f.line),
                        None => Ty::Unknown,
                    })
                    .collect();
                let ret = match &f.ret {
                    Some(te) => self.ty.resolve(te, f.line),
                    None => Ty::Unit,
                };
                let fty = Ty::Fn(sig_ps, Box::new(ret));
                self.ctype(&fty, f.line);
                let v = self.gen_closure(f, f.line);
                self.w(&format!("MiClo {} = {};", local(&f.name), v));
                self.ty.declare(&f.name, fty);
            }
            Stmt::Struct(_) | Stmt::Enum(_) | Stmt::Interface(_) | Stmt::Import { .. } => {}
        }
    }

    /// match는 switch가 아니라 if/else 사슬로 냅니다.
    /// switch를 쓰면 case 안의 `break`가 바깥 반복문이 아니라 switch를 빠져나가
    /// 뜻이 달라집니다. gcc는 어차피 이 사슬을 점프 테이블로 바꿉니다.
    fn gen_match(&mut self, subject: &Expr, cases: &[MatchCase], line: usize) {
        let st = self.infer(subject);
        let sc = self.ctype(&st, line);
        let tmp = self.next_tmp();
        let src = self.expr(subject);
        self.w("{");
        self.indent += 1;
        self.w(&format!("{} {} = {};", sc, tmp, src));

        let mut first = true;
        let mut closed = false;
        for c in cases {
            let test = match &c.pattern {
                Pattern::Wildcard | Pattern::Bind(_) => None,
                Pattern::Variant(name, _) => match &st {
                    Ty::Enum(en) => Some(format!("{}.tag == {}_{}", tmp, mangle(en), name)),
                    _ => {
                        self.errors.push(cerr(
                            "C0019",
                            tr!(format!("{} 값에는 변형 패턴을 쓸 수 없습니다", st), format!("cannot use a variant pattern on a {} value", st)),
                            c.line,
                            1,
                        ));
                        None
                    }
                },
                Pattern::Literal(e) => {
                    let lc = self.expr(e);
                    if st == Ty::Str {
                        Some(format!("mi_str_eq({}, {})", tmp, lc))
                    } else {
                        Some(format!("{} == {}", tmp, lc))
                    }
                }
            };

            match test {
                Some(t) => {
                    if first {
                        self.w(&format!("if ({}) {{", t));
                        first = false;
                    } else {
                        self.w(&format!("}} else if ({}) {{", t));
                    }
                }
                None => {
                    if first {
                        self.w("{");
                        first = false;
                        closed = true;
                    } else {
                        self.w("} else {");
                    }
                }
            }

            self.indent += 1;
            self.push_scope();
            match &c.pattern {
                Pattern::Variant(name, binds) => {
                    if let Ty::Enum(en) = &st {
                        let ed = self.ty.enums.get(en).cloned();
                        if let Some(ed) = ed {
                            if let Some(vd) = ed.variants.iter().find(|v| &v.name == name) {
                                for (i, b) in binds.iter().enumerate() {
                                    if let Some(f) = vd.fields.get(i) {
                                        let ft = match &f.ty {
                                            Some(te) => self.ty.resolve(te, c.line),
                                            None => Ty::Unknown,
                                        };
                                        let fc = self.ctype(&ft, c.line);
                                        // 열거형 필드는 포인터로 담겨 있으니 꺼낼 때 역참조합니다.
                                        let rhs = if matches!(ft, Ty::Enum(_)) {
                                            format!("*({}.u.{}.{})", tmp, name, local(&f.name))
                                        } else {
                                            format!("{}.u.{}.{}", tmp, name, local(&f.name))
                                        };
                                        self.w(&format!("{} {} = {};", fc, local(b), rhs));
                                        self.ty.declare(b, ft);
                                    }
                                }
                            }
                        }
                    }
                }
                Pattern::Bind(n) => {
                    self.w(&format!("{} {} = {};", sc, local(n), tmp));
                    self.w(&format!("(void){};", local(n)));
                    self.ty.declare(n, st.clone());
                }
                _ => {}
            }
            for s in &c.body {
                self.stmt(s);
            }
            self.pop_scope();
            self.indent -= 1;
            if closed {
                break;
            }
        }
        if !first {
            self.w("}");
        }
        self.indent -= 1;
        self.w("}");
    }

    /// `inout` 자리에 넘길 주소. 변수·필드·원소 사슬이면 그 자리를 그대로 가리킵니다
    /// (`self.xs[0].bump()` 처럼 필드 안 리스트 원소도).
    fn inout_ref(&mut self, e: &Expr) -> String {
        fn rooted(e: &Expr) -> bool {
            match e {
                Expr::Ident(..) => true,
                Expr::Field(o, ..) | Expr::Index(o, ..) => rooted(o),
                _ => false,
            }
        }
        if rooted(e) {
            let lv = self.lvalue(e);
            format!("&({})", lv)
        } else {
            let c = self.expr(e);
            format!("&({})", c)
        }
    }

    fn lvalue(&mut self, e: &Expr) -> String {
        match e {
            Expr::Ident(n, _, _) => self.cname(n),
            Expr::Field(o, n, _, _) => {
                let base = self.lvalue(o);
                let val = if self.ty.narrowed_expr(e).is_some() { ".val" } else { "" };
                if self.field_boxed(o, n) {
                    return format!("(*{}.{}){}", base, field_c(n), val);
                }
                format!("{}.{}{}", base, field_c(n), val)
            }
            Expr::Index(o, i, line, _) => {
                let ot = self.infer(o);
                let base = self.lvalue(o);
                let idx = self.expr(i);
                match &ot {
                    Ty::Raw(t) => {
                        let c = self.ctype(t, *line);
                        format!("MI_AT({}, {}, {})", c, base, idx)
                    }
                    Ty::List(t) => {
                        let c = self.ctype(t, *line);
                        format!("(*({}*)mi_list_at(&{}, {}))", c, base, idx)
                    }
                    _ => {
                        self.errors.push(cerr("C0009", tr!("여기에는 대입할 수 없습니다", "cannot assign here"), *line, 1));
                        "0".into()
                    }
                }
            }
            _ => {
                self.errors.push(cerr("C0009", tr!("여기에는 대입할 수 없습니다", "cannot assign here"), 1, 1));
                "0".into()
            }
        }
    }

    // ---------------------------------------------------------------- 표현식

    /// 값 하나를 보기 좋은 문자열로 바꾸는 C 식.
    fn piece_of(&mut self, t: &Ty, item: &str, line: usize) -> String {
        self.repr_of(t, item.to_string(), line)
    }

    /// f-string 서식 조각(`{x:.2f}`)을 MiStr를 만드는 C 식으로 바꿉니다.
    /// 스펙은 컴파일 시점에 이미 알고 있어 정적으로 풀어냅니다.
    fn fmt_piece(&mut self, inner: &Expr, spec: &str) -> String {
        let fs = crate::value::parse_spec(spec);
        let t = self.infer(inner);
        let numeric = matches!(t, Ty::Int | Ty::Float);
        let want_prec = fs.ty == Some('f') || (fs.prec.is_some() && numeric);
        let base = if want_prec {
            let cexpr = self.expr(inner);
            format!("mi_fmt_float((double)({}), {})", cexpr, fs.prec.unwrap_or(6))
        } else if matches!(fs.ty, Some('x') | Some('X')) && matches!(t, Ty::Int) {
            let cexpr = self.expr(inner);
            let upper = if fs.ty == Some('X') { 1 } else { 0 };
            format!("mi_fmt_hex((int64_t)({}), {})", cexpr, upper)
        } else {
            self.to_str(inner)
        };
        if fs.width > 0 {
            let align = fs.align.unwrap_or(if numeric { '>' } else { '<' });
            let fill = if (fs.fill as u32) < 128 { fs.fill } else { ' ' };
            format!("mi_fmt_pad({}, {}, {}, {})", base, fs.width, align as u32, fill as u32)
        } else {
            base
        }
    }

    fn to_str(&mut self, e: &Expr) -> String {
        let t = self.infer(e);
        let c = self.expr(e);
        let (l, _) = e.pos();
        match t {
            Ty::Str => c,
            Ty::Int => format!("mi_from_i64({})", c),
            Ty::Float => format!("mi_from_f64({})", c),
            Ty::Bool => format!("mi_from_bool({})", c),
            Ty::Json => format!("mi_json_write({})", c),
            other => self.repr_of(&other, c, l),
        }
    }

    /// 값을 인터프리터의 repr 과 똑같은 글자로 바꾸는 C 식.
    /// 리스트 안의 리스트, 구조체, 열거형처럼 겹친 값은 타입마다 함수를 만들어 씁니다.
    /// `static void mi_init_globals(void)` — 최상위 상수를 적힌 순서대로 채웁니다.
    fn gen_globals_init(&mut self) {
        let lets = std::mem::take(&mut self.globals);
        self.cur_fn = String::new();
        self.cur_ret = Ty::Unit;
        self.body.push_str("static void mi_init_globals(void) {\n");
        self.indent = 1;
        self.dbg_base = self.ty.scope_depth();
        self.push_scope();
        for s in &lets {
            if let Stmt::Let { name, value, line, .. } = s {
                let target = self.ty.lookup(name).unwrap_or(Ty::Unknown);
                let vt_raw = self.infer(value);
                self.line_mark(*line);
                let (f, l) = self.src_of(*line);
                self.w(&format!("MI_LINE({}, {});", c_string(&f), l));
                self.expect = Some(target.clone());
                let v = self.expr(value);
                self.expect = None;
                let v = self.coerce(v, &vt_raw, &target, *line);
                let v = self.bind_copy(v, value, &target, *line);
                self.w(&format!("{} = {};", local(name), v));
            }
        }
        self.pop_scope();
        self.indent = 0;
        self.body.push_str("}\n\n");
        self.has_globals = true;
    }

    /// `MI_DBG(줄, {"x", &v_x, 글자로}, ...)` — 멈추면 보여 줄 지역 변수들과 함께.
    fn dbg_hook(&mut self, line: usize) -> String {
        let mut vars = Vec::new();
        for (n, t) in self.ty.locals_from(self.dbg_base) {
            if !dbg_showable(&t) {
                continue;
            }
            let f = self.dbg_repr_fn(&t, line);
            vars.push(format!("{{ {}, (void*)&({}), {} }}", c_string(&n), self.cname(&n), f));
        }
        format!("MI_DBG({}, {});", line, vars.join(", "))
    }

    /// 디버거가 변수 값을 글자로 바꿀 때 부르는 함수 `MiStr f(void* p)`.
    fn dbg_repr_fn(&mut self, t: &Ty, line: usize) -> String {
        let name = format!("mi_dbgr_{}", ty_key(t));
        if self.repr_done.contains(&name) {
            return name;
        }
        self.repr_done.insert(name.clone());
        let c = self.ctype(t, line);
        let before = self.errors.len();
        let r = self.repr_of(t, format!("(*({}*)p)", c), line);
        // 글자로 못 바꾸는 값(함수 등)은 조용히 `?` 로 보여 줍니다.
        let r = if self.errors.len() > before {
            self.errors.truncate(before);
            "mi_str(\"?\")".to_string()
        } else {
            r
        };
        let _ = writeln!(self.protos, "static MiStr {}(void* p);", name);
        let _ = writeln!(self.lifted, "static MiStr {}(void* p) {{ (void)p; return {}; }}", name, r);
        name
    }

    fn repr_of(&mut self, t: &Ty, c: String, line: usize) -> String {
        match t {
            Ty::Str => format!("mi_str_repr({})", c),
            Ty::Int => format!("mi_from_i64({})", c),
            Ty::Float => format!("mi_from_f64({})", c),
            Ty::Bool => format!("mi_from_bool({})", c),
            Ty::Json => format!("mi_json_write({})", c),
            Ty::Task(_) => format!("((void)({}), mi_str(MI_T(\"<작업>\", \"<task>\")))", c),
            Ty::Chan(_) => format!("((void)({}), mi_str(MI_T(\"<통로>\", \"<channel>\")))", c),
            Ty::List(_) | Ty::Tuple(_) | Ty::Dict(_, _) | Ty::Struct(_) | Ty::Enum(_) | Ty::Optional(_) => {
                match self.repr_fn(t, line) {
                    Some(f) => format!("{}({})", f, c),
                    None => "mi_str(\"?\")".into(),
                }
            }
            other => {
                self.errors.push(
                    cerr("C0010", tr!(format!("{} 값은 아직 문자열로 바꿀 수 없습니다", other), format!("cannot convert a {} value to a string yet", other)), line, 1)
                        .with_fix(tr!("함수 값은 글자로 바꿀 수 없습니다", "function values cannot be converted to strings")),
                );
                "mi_str(\"?\")".into()
            }
        }
    }

    /// 두 값이 같은지 보는 C 식 (bool).
    fn eq_of(&mut self, t: &Ty, a: String, b: String, line: usize) -> String {
        match t {
            Ty::Int | Ty::Float | Ty::Bool => format!("(({}) == ({}))", a, b),
            Ty::Str => format!("mi_str_eq({}, {})", a, b),
            Ty::NoneTy | Ty::Unit => "true".into(),
            Ty::Json => format!("mi_str_eq(mi_json_write({}), mi_json_write({}))", a, b),
            Ty::List(_) | Ty::Tuple(_) | Ty::Dict(_, _) | Ty::Struct(_) | Ty::Enum(_) | Ty::Optional(_) => {
                let f = self.eq_fn(t, line);
                format!("{}({}, {})", f, a, b)
            }
            other => {
                self.errors.push(cerr("C0012", tr!(format!("{} 값은 `==`로 비교할 수 없습니다", other), format!("cannot compare {} values with `==`", other)), line, 1));
                "false".into()
            }
        }
    }

    fn eq_fn(&mut self, t: &Ty, line: usize) -> String {
        let c = self.ctype(t, line);
        let name = format!("mi_eq_{}", ty_key(t));
        if self.repr_done.contains(&name) {
            return name;
        }
        self.repr_done.insert(name.clone());
        let _ = writeln!(self.protos, "static bool {}({} a, {} b);", name, c, c);
        let mut body = String::new();
        match t {
            Ty::List(inner) => {
                let ic = self.ctype(inner, line);
                let e = self.eq_of(
                    inner,
                    format!("*({}*)((char*)a.data + i * a.esz)", ic),
                    format!("*({}*)((char*)b.data + i * b.esz)", ic),
                    line,
                );
                let _ = writeln!(
                    body,
                    "    if (a.len != b.len) return false;\n    for (int64_t i = 0; i < a.len; i++) if (!{}) return false;\n    return true;",
                    e
                );
            }
            Ty::Tuple(ts) => {
                for (i, et) in ts.iter().enumerate() {
                    let e = self.eq_of(et, format!("a.f{}", i), format!("b.f{}", i), line);
                    let _ = writeln!(body, "    if (!{}) return false;", e);
                }
                body.push_str("    return true;\n");
            }
            Ty::Optional(inner) => {
                let e = self.eq_of(inner, "a.val".into(), "b.val".into(), line);
                let _ = writeln!(body, "    if (a.has != b.has) return false;\n    return !a.has || {};", e);
            }
            Ty::Dict(_, vt) => {
                // 순서와 상관없이: 개수가 같고, a 의 모든 키가 b 에 같은 값으로 있으면 같습니다.
                let (kc, vc, _) = self.dict_parts(t, line);
                let e = self.eq_of(vt, format!("*({}*)(a.vals + i * a.vsz)", vc), format!("*({}*)bv", vc), line);
                let _ = writeln!(
                    body,
                    "    if (a.len != b.len) return false;\n    for (int64_t i = 0; i < a.len; i++) {{\n        void* bv = mi_dict_at(&b, (const {}*)(a.keys + i * a.ksz));\n        if (!bv || !{}) return false;\n    }}\n    return true;",
                    kc, e
                );
            }
            Ty::Struct(n) => {
                if let Some(sd) = self.ty.structs.get(n).cloned() {
                    for f in &sd.fields {
                        let ft = match &f.ty {
                            Some(te) => self.ty.resolve(te, sd.line),
                            None => Ty::Int,
                        };
                        let (x, y) = if self.boxed.contains(&(n.clone(), f.name.clone())) {
                            (format!("(*a.{})", local(&f.name)), format!("(*b.{})", local(&f.name)))
                        } else {
                            (format!("a.{}", local(&f.name)), format!("b.{}", local(&f.name)))
                        };
                        let e = self.eq_of(&ft, x, y, line);
                        let _ = writeln!(body, "    if (!{}) return false;", e);
                    }
                }
                body.push_str("    return true;\n");
            }
            Ty::Enum(n) => {
                if let Some(ed) = self.ty.enums.get(n).cloned() {
                    body.push_str("    if (a.tag != b.tag) return false;\n    switch (a.tag) {\n");
                    for vd in &ed.variants {
                        let _ = writeln!(body, "    case {}_{}:", mangle(n), vd.name);
                        for f in &vd.fields {
                            let ft = f.ty.as_ref().map(|te| self.ty.resolve(te, ed.line)).unwrap_or(Ty::Int);
                            let (x, y) = if matches!(ft, Ty::Enum(_)) {
                                (
                                    format!("(*a.u.{}.{})", vd.name, local(&f.name)),
                                    format!("(*b.u.{}.{})", vd.name, local(&f.name)),
                                )
                            } else {
                                (
                                    format!("a.u.{}.{}", vd.name, local(&f.name)),
                                    format!("b.u.{}.{}", vd.name, local(&f.name)),
                                )
                            };
                            let e = self.eq_of(&ft, x, y, line);
                            let _ = writeln!(body, "        if (!{}) return false;", e);
                        }
                        body.push_str("        return true;\n");
                    }
                    body.push_str("    }\n    return true;\n");
                } else {
                    body.push_str("    return false;\n");
                }
            }
            _ => body.push_str("    (void)a; (void)b; return false;\n"),
        }
        let _ = write!(self.lifted, "static bool {}({} a, {} b) {{\n{}}}\n\n", name, c, c, body);
        name
    }

    fn repr_fn(&mut self, t: &Ty, line: usize) -> Option<String> {
        let c = self.ctype(t, line);
        let name = format!("mi_repr_{}", ty_key(t));
        if self.repr_done.contains(&name) {
            return Some(name);
        }
        self.repr_done.insert(name.clone());
        let _ = writeln!(self.protos, "static MiStr {}({} v);", name, c);
        let mut b = String::new();
        let cat = |b: &mut String, piece: &str| {
            let _ = writeln!(b, "    s = mi_cat(s, {});", piece);
        };
        match t {
            Ty::List(inner) => {
                let ic = self.ctype(inner, line);
                let item = self.repr_of(inner, format!("*({}*)((char*)v.data + i * v.esz)", ic), line);
                let _ = writeln!(
                    b,
                    "    MiStr s = mi_str(\"[\");\n    for (int64_t i = 0; i < v.len; i++) {{ if (i) s = mi_cat(s, mi_str(\", \")); s = mi_cat(s, {}); }}\n    return mi_cat(s, mi_str(\"]\"));",
                    item
                );
            }
            Ty::Dict(kt, vt) => {
                let (kc, vc, _) = self.dict_parts(t, line);
                let k = self.repr_of(kt, format!("*({}*)(v.keys + i * v.ksz)", kc), line);
                let val = self.repr_of(vt, format!("*({}*)(v.vals + i * v.vsz)", vc), line);
                let _ = writeln!(
                    b,
                    "    MiStr s = mi_str(\"{{\");\n    for (int64_t i = 0; i < v.len; i++) {{ if (i) s = mi_cat(s, mi_str(\", \")); s = mi_cat(s, {}); s = mi_cat(s, mi_str(\": \")); s = mi_cat(s, {}); }}\n    return mi_cat(s, mi_str(\"}}\"));",
                    k, val
                );
            }
            Ty::Tuple(ts) => {
                b.push_str("    MiStr s = mi_str(\"(\");\n");
                for (i, et) in ts.iter().enumerate() {
                    if i > 0 {
                        cat(&mut b, "mi_str(\", \")");
                    }
                    let p = self.repr_of(et, format!("v.f{}", i), line);
                    cat(&mut b, &p);
                }
                b.push_str("    return mi_cat(s, mi_str(\")\"));\n");
            }
            Ty::Optional(inner) => {
                let p = self.repr_of(inner, "v.val".into(), line);
                let _ = writeln!(b, "    return v.has ? {} : mi_str(\"none\");", p);
            }
            Ty::Struct(n) => {
                let sd = self.ty.structs.get(n).cloned()?;
                let _ = writeln!(b, "    MiStr s = mi_str({});", c_string(&format!("{}(", crate::ns::plain(n))));
                for (i, f) in sd.fields.iter().enumerate() {
                    let ft = match &f.ty {
                        Some(te) => self.ty.resolve(te, sd.line),
                        None => Ty::Int,
                    };
                    let lead = if i > 0 { format!(", {}: ", f.name) } else { format!("{}: ", f.name) };
                    cat(&mut b, &format!("mi_str({})", c_string(&lead)));
                    let fe = if self.boxed.contains(&(n.clone(), f.name.clone())) {
                        format!("(*v.{})", local(&f.name))
                    } else {
                        format!("v.{}", local(&f.name))
                    };
                    let p = self.repr_of(&ft, fe, line);
                    cat(&mut b, &p);
                }
                b.push_str("    return mi_cat(s, mi_str(\")\"));\n");
            }
            Ty::Enum(n) => {
                let ed = self.ty.enums.get(n).cloned()?;
                b.push_str("    MiStr s = mi_str(\"\");\n    switch (v.tag) {\n");
                for vd in &ed.variants {
                    let _ = writeln!(b, "    case {}_{}:", mangle(n), vd.name);
                    if vd.fields.is_empty() {
                        let _ = writeln!(b, "        return mi_str({});", c_string(crate::ns::plain(&vd.name)));
                        continue;
                    }
                    let _ = writeln!(b, "        s = mi_str({});", c_string(&format!("{}(", crate::ns::plain(&vd.name))));
                    for (i, f) in vd.fields.iter().enumerate() {
                        let ft = f.ty.as_ref().map(|te| self.ty.resolve(te, ed.line)).unwrap_or(Ty::Int);
                        if i > 0 {
                            b.push_str("        s = mi_cat(s, mi_str(\", \"));\n");
                        }
                        let fe = if matches!(ft, Ty::Enum(_)) {
                            format!("(*v.u.{}.{})", vd.name, local(&f.name))
                        } else {
                            format!("v.u.{}.{}", vd.name, local(&f.name))
                        };
                        let p = self.repr_of(&ft, fe, line);
                        let _ = writeln!(b, "        s = mi_cat(s, {});", p);
                    }
                    b.push_str("        return mi_cat(s, mi_str(\")\"));\n");
                }
                b.push_str("    }\n    return s;\n");
            }
            _ => b.push_str("    (void)v; return mi_str(\"?\");\n"),
        }
        let _ = write!(self.lifted, "static MiStr {}({} v) {{\n{}}}\n\n", name, c, b);
        Some(name)
    }

    /// C 는 함수 인자·연산자 양쪽을 어떤 순서로 계산할지 정하지 않습니다(gcc 는 대개 오른쪽부터).
    /// 인터프리터는 왼쪽부터 계산하므로, 부작용이 있을 수 있는 하위 식(함수 호출을 품은 것)이
    /// 둘 이상이면 왼쪽부터 임시 변수에 담아 두고 원래 식을 만듭니다.
    fn expr(&mut self, e: &Expr) -> String {
        if let Some(t) = self.pre.get(&(e as *const Expr as usize)) {
            return t.clone();
        }
        fn has_call(e: &Expr) -> bool {
            match e {
                Expr::Call { .. } | Expr::Spawn(..) => true,
                Expr::Lambda(..) => false,
                Expr::FString(ps) => ps.iter().any(|p| matches!(p, FStrPart::Expr(x, _) if has_call(x))),
                Expr::List(xs) | Expr::Tuple(xs) => xs.iter().any(has_call),
                Expr::Dict(kv) => kv.iter().any(|(k, v)| has_call(k) || has_call(v)),
                Expr::Unary(_, a, ..) | Expr::Try(a, ..) | Expr::Field(a, ..) => has_call(a),
                Expr::Binary(_, a, b, ..) | Expr::Index(a, b, ..) | Expr::OrElse(a, b, ..) => has_call(a) || has_call(b),
                Expr::IfExpr { cond, then, els } => has_call(cond) || has_call(then) || has_call(els),
                _ => false,
            }
        }
        let subs: Vec<&Expr> = match e {
            Expr::Call { args, .. } => args.iter().map(|a| &a.value).collect(),
            Expr::Binary(op, a, b, ..) if !matches!(op, BinOp::And | BinOp::Or) => vec![a.as_ref(), b.as_ref()],
            Expr::FString(ps) => ps
                .iter()
                .filter_map(|p| if let FStrPart::Expr(x, _) = p { Some(x.as_ref()) } else { None })
                .collect(),
            Expr::List(xs) | Expr::Tuple(xs) => xs.iter().collect(),
            Expr::Dict(kv) => kv.iter().flat_map(|(k, v)| [k, v]).collect(),
            _ => Vec::new(),
        };
        let hoist: Vec<&Expr> = subs
            .into_iter()
            .filter(|x| has_call(x) && !matches!(x, Expr::Ident(..) | Expr::Field(..) | Expr::Index(..)))
            .collect();
        if hoist.len() >= 2 {
            let mut tys = Vec::new();
            let mut ok = true;
            for h in &hoist {
                let t = self.infer(h);
                fn concrete(t: &Ty) -> bool {
                    match t {
                        Ty::Unknown | Ty::Var(_) | Ty::Unit | Ty::NoneTy | Ty::Arena => false,
                        Ty::List(a) | Ty::Optional(a) | Ty::Fallible(a, _) | Ty::Raw(a) => concrete(a),
                        Ty::Dict(a, b) => concrete(a) && concrete(b),
                        Ty::Tuple(ts) => ts.iter().all(concrete),
                        Ty::Fn(ps, r) => ps.iter().all(concrete) && concrete(r),
                        _ => true,
                    }
                }
                if !concrete(&t) {
                    ok = false;
                    break;
                }
                tys.push(t);
            }
            if ok {
                let mut decls = String::new();
                let mut keys = Vec::new();
                for (h, t) in hoist.iter().zip(tys.iter()) {
                    let ct = self.ctype(t, 0);
                    let saved = self.expect.take();
                    let code = self.expr(h);
                    self.expect = saved;
                    let tmp = self.next_tmp();
                    let _ = write!(decls, "{} {} = {}; ", ct, tmp, code);
                    let key = *h as *const Expr as usize;
                    self.pre.insert(key, tmp);
                    keys.push(key);
                }
                let r = self.expr_inner(e);
                for k in keys {
                    self.pre.remove(&k);
                }
                return format!("({{ {}{}; }})", decls, r);
            }
        }
        self.expr_inner(e)
    }

    fn expr_inner(&mut self, e: &Expr) -> String {
        match e {
            Expr::Lambda(f, l, _) => self.gen_closure(f, *l),
            Expr::Spawn(f, l, _) => {
                // 붙잡은 값을 복사한 클로저를 만들고, 결과 타입에 맞는 돌리개로 새 스레드에서 부릅니다.
                self.uses_conc = true;
                let rt = match self.infer(e) {
                    Ty::Task(r) => *r,
                    _ => Ty::Unknown,
                };
                let saved = self.expect.take();
                let clo = self.gen_closure(f, *l);
                self.expect = saved;
                let runner = self.task_runner(&rt, *l);
                let size = if matches!(rt, Ty::Unit) { "1".to_string() } else { format!("(int64_t)sizeof({})", self.ctype(&rt, *l)) };
                format!("mi_task_spawn({}, {}, {})", clo, size, runner)
            }
            Expr::Int(n) => format!("INT64_C({})", n),
            Expr::Float(f) => {
                if f.fract() == 0.0 {
                    format!("{:.1}", f)
                } else {
                    format!("{}", f)
                }
            }
            Expr::Bool(b) => if *b { "true".into() } else { "false".into() },
            Expr::Str(s) => format!("mi_str({})", c_string(s)),

            Expr::NoneLit => {
                let want = self.expect.clone();
                match want {
                    Some(Ty::Optional(inner)) => {
                        let n = self.opt_name(&inner, 1);
                        format!("(({}){{ .has = false }})", n)
                    }
                    _ => {
                        self.errors.push(
                            cerr("C0011", tr!("`none`이 어떤 `?T`인지 알 수 없습니다", "cannot tell which optional type `?T` this `none` is"), 1, 1)
                                .with_fix(tr!("반환 타입이나 변수 타입에 `?T`를 적어 주세요", "annotate the return type or variable type with `?T`")),
                        );
                        "0".into()
                    }
                }
            }

            Expr::Ident(n, _, _) => {
                // 값을 담지 않는 열거형 변형은 괄호 없이 그냥 써서 만듭니다.
                if let Some(ename) = self.ty.variant_of.get(n).cloned() {
                    let ed = self.ty.enums.get(&ename).cloned().unwrap();
                    if let Some(vd) = ed.variants.iter().find(|v| &v.name == n) {
                        if vd.fields.is_empty() {
                            return format!(
                                "((struct {}){{ .tag = {}_{} }})",
                                mangle(&ename),
                                mangle(&ename),
                                vd.name
                            );
                        }
                    }
                }
                // 최상위 함수 이름을 값으로 쓰면 그 함수 포인터입니다.
                if self.ty.lookup(n).is_none() && self.ty.fns.contains_key(n) {
                    let (l, _) = e.pos();
                    return self.fn_value(n, l);
                }
                self.cname(n)
            }

            Expr::FString(parts) => {
                let mut acc: Option<String> = None;
                let saved = self.expect.take();
                for p in parts {
                    let piece = match p {
                        FStrPart::Lit(s) => format!("mi_str({})", c_string(s)),
                        FStrPart::Expr(inner, spec) if spec.is_empty() => self.to_str(inner),
                        FStrPart::Expr(inner, spec) => self.fmt_piece(inner, spec),
                    };
                    acc = Some(match acc {
                        None => piece,
                        Some(a) => format!("mi_cat({}, {})", a, piece),
                    });
                }
                self.expect = saved;
                acc.unwrap_or_else(|| "mi_str(\"\")".into())
            }

            Expr::List(items) => {
                let inferred = self.infer(e);
                let t = match (&inferred, self.expect.clone()) {
                    (Ty::List(inner), Some(exp)) if **inner == Ty::Unknown => exp,
                    _ => inferred,
                };
                let (l, _) = e.pos();
                let ec = self.elem_ctype(&t, l);
                let saved = self.expect.take();
                let et = match &t {
                    Ty::List(inner) => Some((**inner).clone()),
                    _ => None,
                };
                let mut s = format!("({{ MiList _t = mi_list_new((int64_t)sizeof({})); ", ec);
                for it in items {
                    // 원소 자리의 타입을 알려 줘야 `[]`, `none`, 익명 함수 원소가 제 타입을 압니다.
                    self.expect = et.clone();
                    let vt = self.infer(it);
                    let v = self.expr(it);
                    let v = match &et {
                        Some(w) => self.coerce(v, &vt, w, l),
                        None => v,
                    };
                    let _ = write!(s, "{{ {} _v = {}; mi_list_push(&_t, &_v); }} ", ec, v);
                }
                s.push_str("_t; })");
                self.expect = saved;
                s
            }

            Expr::Tuple(items) => {
                let inferred = self.infer(e);
                let exp = self.expect.clone();
                // 반환/대입에서 기대 타입이 튜플이면 그쪽 원소 타입을 씁니다
                // (none 같은 원소의 타입을 정확히 잡기 위해서).
                let elems: Vec<Ty> = match (&inferred, &exp) {
                    (_, Some(Ty::Tuple(es))) => es.clone(),
                    (Ty::Tuple(es), _) => es.clone(),
                    _ => items.iter().map(|it| self.infer(it)).collect(),
                };
                let (l, _) = e.pos();
                let name = self.tup_name(&elems, l);
                let saved = self.expect.take();
                let mut s = format!("({{ {} _t; ", name);
                for (i, it) in items.iter().enumerate() {
                    self.expect = elems.get(i).cloned();
                    let v = self.expr(it);
                    let _ = write!(s, "_t.f{} = {}; ", i, v);
                }
                s.push_str("_t; })");
                self.expect = saved;
                s
            }

            Expr::Unary(op, inner, _, _) => {
                let c = self.expr(inner);
                match op {
                    UnOp::Neg => format!("(-{})", c),
                    UnOp::Not => format!("(!{})", c),
                }
            }

            Expr::Binary(op, a, b, line, _) => {
                // `x != none` / `x == none`
                let cmp_none = matches!(op, BinOp::Eq | BinOp::Ne)
                    && (matches!(b.as_ref(), Expr::NoneLit) || matches!(a.as_ref(), Expr::NoneLit));
                if cmp_none {
                    let target = if matches!(b.as_ref(), Expr::NoneLit) { a } else { b };
                    let tt = self.infer(target);
                    if let Ty::Optional(_) = tt {
                        // 좁히기 이름이 걸려 있으면 원래 변수를 봐야 합니다.
                        let c = match target.as_ref() {
                            Expr::Ident(n, _, _) => local(n),
                            other => self.expr(other),
                        };
                        return if *op == BinOp::Ne {
                            format!("({}).has", c)
                        } else {
                            format!("(!({}).has)", c)
                        };
                    }
                }

                let at = self.infer(a);
                let saved = self.expect.take();
                let ca = self.expr(a);
                // `a and b`: 오른쪽은 왼쪽이 참일 때만 평가되므로 좁혀서 냅니다.
                let cb = if *op == BinOp::And {
                    let n = self.narrow(a, true, &Region::Expr(b));
                    self.push_scope();
                    self.apply_narrow(n);
                    let cb = self.expr(b);
                    self.pop_scope();
                    cb
                } else {
                    self.expr(b)
                };
                self.expect = saved;
                use BinOp::*;
                // 포인터 산술: `p + 1` 은 한 칸 뒤를 가리킵니다.
                if let Ty::Raw(inner) = &at {
                    let ec = self.ctype(inner, *line);
                    return match op {
                        Add => format!("MI_ADD({}, {}, {})", ec, ca, cb),
                        Sub => format!("MI_ADD({}, {}, -({}))", ec, ca, cb),
                        _ => {
                            self.errors.push(cerr(
                                "C0021",
                                tr!(format!("포인터에 `{}`은(는) 쓸 수 없습니다", op.symbol()), format!("cannot use `{}` on pointers", op.symbol())),
                                *line,
                                1,
                            ));
                            "0".into()
                        }
                    };
                }
                if at == Ty::Str {
                    return match op {
                        Add => format!("mi_cat({}, {})", ca, cb),
                        Eq => format!("mi_str_eq({}, {})", ca, cb),
                        Ne => format!("(!mi_str_eq({}, {}))", ca, cb),
                        Lt => format!("(mi_str_cmp({}, {}) < 0)", ca, cb),
                        Le => format!("(mi_str_cmp({}, {}) <= 0)", ca, cb),
                        Gt => format!("(mi_str_cmp({}, {}) > 0)", ca, cb),
                        Ge => format!("(mi_str_cmp({}, {}) >= 0)", ca, cb),
                        _ => {
                            self.errors.push(cerr(
                                "C0012",
                                tr!(format!("문자열에 `{}`은(는) 쓸 수 없습니다", op.symbol()), format!("cannot use `{}` on strings", op.symbol())),
                                *line,
                                1,
                            ));
                            "0".into()
                        }
                    };
                }
                // 리스트·구조체·enum·튜플·사전·`?T` 의 `==` 는 안쪽까지 비교합니다 (인터프리터와 같게).
                if matches!(op, Eq | Ne) {
                    let bt = self.infer(b);
                    let deep = |t: &Ty| {
                        matches!(
                            t,
                            Ty::List(_) | Ty::Tuple(_) | Ty::Dict(_, _) | Ty::Struct(_) | Ty::Enum(_)
                                | Ty::Optional(_) | Ty::Json
                        )
                    };
                    if deep(&at) || deep(&bt) {
                        let t = if matches!(bt, Ty::Optional(_)) { bt.clone() } else { at.clone() };
                        let ca = self.coerce(ca, &at, &t, *line);
                        let cb = self.coerce(cb, &bt, &t, *line);
                        let e = self.eq_of(&t, ca, cb, *line);
                        return if *op == Eq { e } else { format!("(!{})", e) };
                    }
                }
                if at == Ty::Int && *op == Div {
                    return format!("mi_div_i64({}, {})", ca, cb);
                }
                if at == Ty::Int && *op == Mod {
                    return format!("mi_mod_i64({}, {})", ca, cb);
                }
                if at == Ty::Float && *op == Div {
                    return format!("mi_div_f64({}, {})", ca, cb);
                }
                if at == Ty::Float && *op == Mod {
                    return format!("fmod({}, {})", ca, cb);
                }
                let sym = match op {
                    Add => "+",
                    Sub => "-",
                    Mul => "*",
                    Div => "/",
                    Mod => "%",
                    Eq => "==",
                    Ne => "!=",
                    Lt => "<",
                    Le => "<=",
                    Gt => ">",
                    Ge => ">=",
                    And => "&&",
                    Or => "||",
                };
                format!("({} {} {})", ca, sym, cb)
            }

            Expr::IfExpr { cond, then, els } => {
                let c = self.expr(cond);
                let t = self.expr(then);
                let e2 = self.expr(els);
                format!("({} ? {} : {})", c, t, e2)
            }

            Expr::Index(o, i, line, _) => {
                let ot = self.infer(o);
                let saved = self.expect.take();
                let idx = self.expr(i);
                let r = match &ot {
                    Ty::Dict(_, v) => {
                        // 없는 키를 물으면 `none`이 나옵니다. 그래서 `?V` 입니다.
                        let (kc, vc, _) = self.dict_parts(&ot, *line);
                        let on = self.opt_name(v, *line);
                        let base = self.expr(o);
                        let dt = self.next_tmp();
                        let kt = self.next_tmp();
                        let pt = self.next_tmp();
                        format!(
                            "({{ MiDict {} = {}; {} {} = {}; {}* {} = ({}*)mi_dict_at(&{}, &{}); {} ? (({}){{ .has = true, .val = *{} }}) : (({}){{ .has = false }}); }})",
                            dt, base, kc, kt, idx, vc, pt, vc, dt, kt, pt, on, pt, on
                        )
                    }
                    Ty::Raw(t) => {
                        let c = self.ctype(t, *line);
                        let base = self.expr(o);
                        format!("MI_AT({}, {}, {})", c, base, idx)
                    }
                    Ty::List(t) => {
                        let c = self.ctype(t, *line);
                        match o.as_ref() {
                            Expr::Ident(n, _, _) => {
                                format!("(*({}*)mi_list_at(&{}, {}))", c, self.cname(n), idx)
                            }
                            other => {
                                let base = self.expr(other);
                                let tmp = self.next_tmp();
                                format!(
                                    "({{ MiList {} = {}; *({}*)mi_list_at(&{}, {}); }})",
                                    tmp, base, c, tmp, idx
                                )
                            }
                        }
                    }
                    Ty::Str => {
                        let base = self.expr(o);
                        format!("mi_str_index({}, {})", base, idx)
                    }
                    other => {
                        self.errors.push(cerr(
                            "C0013",
                            tr!(format!("{} 값은 아직 네이티브에서 인덱싱할 수 없습니다", other), format!("indexing a {} value is not supported by the native build yet", other)),
                            *line,
                            1,
                        ));
                        "0".into()
                    }
                };
                self.expect = saved;
                r
            }

            Expr::Field(o, n, _, _) => {
                let base = self.expr(o);
                // 좁혀진 `?T` 필드(`if t.due != none:` 안)는 값 쪽을 꺼냅니다.
                let val = if self.ty.narrowed_expr(e).is_some() { ".val" } else { "" };
                if self.field_boxed(o, n) {
                    return format!("(*({}).{}){}", base, field_c(n), val);
                }
                format!("({}).{}{}", base, field_c(n), val)
            }

            Expr::Try(inner, l, _) => {
                // 실패하면 현재 함수에서 그 에러를 그대로 반환합니다.
                let it = self.infer(inner);
                let (ok_ty, err_in) = match &it {
                    Ty::Fallible(x, e) => ((**x).clone(), (**e).clone()),
                    other => (other.clone(), Ty::Str),
                };
                let res_in = self.res_name(&ok_ty, &err_in, *l);
                let mut err_out = Ty::Str;
                let out_inner = match self.cur_ret.clone() {
                    Ty::Fallible(x, e) => {
                        err_out = *e;
                        (*x).clone()
                    }
                    other => {
                        self.errors.push(
                            cerr("C0020", tr!("`try`는 `!T`를 반환하는 함수 안에서만 쓸 수 있습니다", "`try` can only be used in a function returning a fallible `!T`"), *l, 1)
                                .with_fix(tr!("반환 타입에 `!`를 붙이거나 `catch`로 받으세요", "add `!` to the return type or handle it with `catch`")),
                        );
                        other
                    }
                };
                let res_out = self.res_name(&out_inner, &err_out, *l);
                let saved = self.expect.take();
                let c = self.expr(inner);
                self.expect = saved;
                let tmp = self.next_tmp();
                // enum 오류를 글자 오류 함수로 올릴 때는 `NoFunds(need: 5)` 같은 글자로 바꿉니다.
                let err_val = if matches!(err_in, Ty::Enum(_)) && !matches!(err_out, Ty::Enum(_)) {
                    self.repr_of(&err_in, format!("{}.err", tmp), *l)
                } else {
                    format!("{}.err", tmp)
                };
                format!(
                    "({{ {} {} = {}; if (!{}.ok) return ({}){{ .ok = false, .err = {} }}; {}.val; }})",
                    res_in, tmp, c, tmp, res_out, err_val, tmp
                )
            }

            // `a else 기본값` — a가 none이면 기본값을 냅니다.
            Expr::OrElse(a, b, l, _) => {
                let at = self.infer(a);
                let inner = match &at {
                    Ty::Optional(x) => (**x).clone(),
                    other => other.clone(),
                };
                let on = self.opt_name(&inner, *l);
                let saved = self.expect.take();
                self.expect = Some(at.clone());
                let ca = self.expr(a);
                self.expect = Some(inner.clone());
                let cb = self.expr(b);
                self.expect = saved;
                let tmp = self.next_tmp();
                format!(
                    "({{ {on} {t} = {ca}; {t}.has ? {t}.val : ({cb}); }})",
                    on = on, t = tmp, ca = ca, cb = cb
                )
            }

            Expr::Call { callee, targs, args, line, col } => {
                let targs = targs.clone();
                self.call(callee, &targs, args, *line, *col)
            }

            Expr::Dict(pairs) => {
                // 빈 사전 `{}` 은 그 자체로는 타입을 알 수 없으니
                // 받는 쪽에 적힌 타입을 씁니다.
                let t = match (self.infer(e), self.expect.clone()) {
                    (Ty::Dict(k, v), Some(Ty::Dict(ek, ev)))
                        if *k == Ty::Unknown || *v == Ty::Unknown =>
                    {
                        let _ = (k, v);
                        Ty::Dict(ek, ev)
                    }
                    (got, _) => got,
                };
                let (kc, vc, kind) = self.dict_parts(&t, 1);
                let d = self.next_tmp();
                let mut out = format!(
                    "({{ MiDict {} = mi_dict_new((int64_t)sizeof({}), (int64_t)sizeof({}), {}); ",
                    d, kc, vc, kind
                );
                let saved = self.expect.take();
                for (k, v) in pairs {
                    let kk = self.expr(k);
                    let vv = self.expr(v);
                    let _ = write!(
                        out,
                        "{{ {} _k = {}; {} _v = {}; mi_dict_set(&{}, &_k, &_v); }} ",
                        kc, kk, vc, vv, d
                    );
                }
                self.expect = saved;
                let _ = write!(out, "{}; }})", d);
                out
            }
        }
    }

    /// 함수 값(함수 포인터)을 부르는 코드를 냅니다. 인자를 매개변수 타입에 맞춰
    /// 냅니다. inout은 함수 타입에 없으므로 모두 값으로 넘깁니다.
    fn emit_indirect_call(&mut self, cexpr: String, params: &[Ty], ret: &Ty, args: &[Arg], l: usize) -> String {
        let fp = self.fn_ptr_name(params, ret, l);
        let saved = self.expect.take();
        let mut out = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let vt = self.infer(&a.value);
            let wt = params.get(i).cloned();
            self.expect = wt.clone();
            let c = self.expr(&a.value);
            self.expect = None;
            let c = match &wt {
                Some(w) => self.coerce(c, &vt, w, l),
                None => c,
            };
            out.push(c);
        }
        self.expect = saved;
        let t = self.next_tmp();
        let mut all = vec![format!("{}.env", t)];
        all.extend(out);
        format!("({{ MiClo {} = {}; (({}){}.fn)({}); }})", t, cexpr, fp, t, all.join(", "))
    }

    /// 이름 붙은 최상위 함수를 값으로 쓸 때의 다리. 첫 자리(env)를 받고 버립니다.
    fn fn_value(&mut self, name: &str, line: usize) -> String {
        let sig = match self.ty.fns.get(name).cloned() {
            Some(s) => s,
            None => return "((MiClo){0})".into(),
        };
        if !sig.decl.generics.is_empty() {
            self.errors.push(
                cerr("C0023", tr!(format!("제네릭 함수 `{}`은(는) 아직 값으로 넘길 수 없습니다", name), format!("generic function `{}` cannot be passed as a value yet", name)), line, 1)
                    .with_fix(tr!("`fn(x: Int): 이름(x)` 처럼 익명 함수로 감싸서 넘기세요", "wrap it in an anonymous function, like `fn(x: Int): name(x)`")),
            );
            return "((MiClo){0})".into();
        }
        let th = format!("mi_th_{}", name);
        if !self.thunks_done.contains(&th) {
            self.thunks_done.insert(th.clone());
            let rc = self.ctype(&sig.ret, line);
            let mut ps = vec!["void* _e".to_string()];
            let mut call = Vec::new();
            for (i, (pn, pt)) in sig.params.iter().enumerate() {
                if pn == "self" {
                    continue;
                }
                let c = self.ctype(pt, line);
                ps.push(format!("{} a{}", c, i));
                let inout = sig.decl.params.get(i).map(|p| p.conv == Convention::Inout).unwrap_or(false);
                call.push(if inout { format!("&a{}", i) } else { format!("a{}", i) });
            }
            let callc = format!("{}({})", mangle(name), call.join(", "));
            let body = if sig.ret == Ty::Unit { format!("{};", callc) } else { format!("return {};", callc) };
            let _ = writeln!(self.protos, "static {} {}({});", rc, th, ps.join(", "));
            let _ = writeln!(self.lifted, "static {} {}({}) {{ (void)_e; {} }}", rc, th, ps.join(", "), body);
        }
        format!("((MiClo){{ (void*){}, NULL }})", th)
    }

    /// 익명 함수·중첩 함수를 C로 냅니다. 본문은 최상위 C 함수로 끌어올리고,
    /// 붙잡은 값은 힙에 복사해 둔 묶음(env)으로 넘깁니다. 결과는 MiClo 값 식입니다.
    fn gen_closure(&mut self, f: &crate::ast::Shared<FnDecl>, line: usize) -> String {
        self.lam_count += 1;
        let k = self.lam_count;
        let hint = match self.expect.clone() {
            Some(Ty::Fn(ps, _)) if ps.len() == f.params.len() => Some(ps),
            _ => None,
        };
        let table = self.ty.lambda_sigs.get(&(crate::ast::Shared::as_ptr(f) as usize)).cloned();
        let mut ptys = Vec::new();
        for (i, p) in f.params.iter().enumerate() {
            let t = match &p.ty {
                Some(te) => self.ty.resolve(te, f.line),
                None => {
                    let from_hint = hint.as_ref().and_then(|h| h.get(i).cloned());
                    let from_table = table.as_ref().and_then(|(ps, _)| ps.get(i).cloned());
                    match (from_hint, from_table) {
                        (Some(h), _) if h != Ty::Unknown => h,
                        (_, Some(t)) => {
                            let subst = self.ty.mono_subst.clone();
                            if subst.is_empty() { t } else { self.ty.substitute(&t, &subst) }
                        }
                        _ => Ty::Unknown,
                    }
                }
            };
            ptys.push(t);
        }

        // 무엇을 붙잡는가: 본문이 쓰는 바깥 이름 중 지금 보이는 지역 변수.
        let mut caps: Vec<(String, Ty, String)> = Vec::new();
        for n in free_vars(f) {
            if let Some(t) = self.ty.lookup(&n) {
                let src = self.cname(&n);
                caps.push((n, t, src));
            }
        }

        // 반환 타입. 적지 않은 익명 함수는 본문 식에서 추론합니다.
        self.ty.push_scope();
        for (p, t) in f.params.iter().zip(&ptys) {
            self.ty.declare(&p.name, t.clone());
        }
        let ret = match (&f.ret, f.is_lambda(), f.body.first()) {
            (Some(te), _, _) => self.ty.resolve(te, f.line),
            (None, true, Some(Stmt::Return(Some(e), _, _))) => self.infer(e),
            _ => Ty::Unit,
        };
        self.ty.pop_scope();

        let fname = format!("mi_lam_{}", k);
        let env_name = format!("mi_env_{}", k);
        if !caps.is_empty() {
            let mut d = format!("struct {} {{ ", env_name);
            for (n, t, _) in &caps {
                let c = self.ctype(t, line);
                let _ = write!(d, "{} {}; ", c, local(n));
            }
            d.push_str("};\n");
            let deps: Vec<String> = caps.iter().map(|(_, t, _)| self.ctype(t, line)).collect();
            self.add_tdef(&format!("struct {}", env_name), d, &deps);
        }

        // --- 끌어올린 함수 본문 ---
        let rc = self.ctype(&ret, line);
        let mut plist = vec!["void* _envp".to_string()];
        for (p, t) in f.params.iter().zip(&ptys) {
            let c = self.ctype(t, line);
            plist.push(format!("{} {}", c, local(&p.name)));
        }
        let proto = format!("static {} {}({})", rc, fname, plist.join(", "));
        let _ = writeln!(self.protos, "{};", proto);

        let saved_body = std::mem::take(&mut self.body);
        let saved_indent = self.indent;
        let saved_fn = std::mem::replace(&mut self.cur_fn, f.shown_name());
        let saved_decl = std::mem::replace(&mut self.ty.cur_decl, Some(f.clone()));
        let saved_ret = std::mem::replace(&mut self.cur_ret, ret.clone());
        let saved_renames = std::mem::replace(&mut self.renames, vec![HashMap::new()]);
        let saved_line = self.last_line;
        let saved_expect = self.expect.take();
        self.last_line = 0;

        self.body.push_str(&proto);
        self.body.push_str(" {\n");
        self.indent = 1;
        let saved_base = std::mem::replace(&mut self.dbg_base, self.ty.scope_depth());
        self.ty.push_scope();
        // `spawn f(x)` 을 감싼 익명 함수는 호출 경로에 보이지 않게 합니다(작업은 f 부터 보임).
        if self.dbg && !f.name.starts_with("λspawn") {
            self.w(&format!("MI_DBG_FN({});", c_string(&f.shown_name())));
        }
        if caps.is_empty() {
            self.w("(void)_envp;");
        } else {
            self.w(&format!("struct {}* _env = (struct {}*)_envp;", env_name, env_name));
            for (n, t, _) in &caps {
                let c = self.ctype(t, line);
                self.w(&format!("{} {} = _env->{}; (void){};", c, local(n), local(n), local(n)));
                self.ty.declare(n, t.clone());
            }
        }
        if !f.is_lambda() {
            // 자기 이름으로 자기를 부를 수 있게(재귀).
            self.w(&format!(
                "MiClo {} = {{ (void*){}, _envp }}; (void){};",
                local(&f.name),
                fname,
                local(&f.name)
            ));
            self.ty.declare(&f.name, Ty::Fn(ptys.clone(), Box::new(ret.clone())));
        }
        for (p, t) in f.params.iter().zip(&ptys) {
            self.ty.declare(&p.name, t.clone());
        }
        if !f.requires.is_empty() {
            self.emit_requires(f);
        }
        match (f.is_lambda(), f.body.first(), &ret) {
            (true, Some(Stmt::Return(Some(e), l, _)), Ty::Unit) => {
                self.line_mark(*l);
                let c = self.expr(e);
                self.w(&format!("{};", c));
            }
            _ => {
                for st in &f.body {
                    self.stmt(st);
                }
            }
        }
        if let Ty::Fallible(inner, et) = ret.clone() {
            let rn = self.res_name(&inner, &et, f.line);
            self.w(&format!("return ({}){{ .ok = true }};", rn));
        }
        self.ty.pop_scope();
        self.dbg_base = saved_base;
        self.body.push_str("}\n\n");
        let text = std::mem::replace(&mut self.body, saved_body);
        self.lifted.push_str(&text);
        self.indent = saved_indent;
        self.cur_fn = saved_fn;
        self.ty.cur_decl = saved_decl;
        self.cur_ret = saved_ret;
        self.renames = saved_renames;
        self.expect = saved_expect;
        // 바깥 함수로 돌아왔으니 다음 문장에서 `#line` 을 다시 찍게 합니다.
        self.last_line = if saved_line == 0 { 0 } else { usize::MAX };
        if !f.ensures.is_empty() {
            self.errors.push(
                cerr("C0024", tr!("클로저에는 아직 `ensures` 를 쓸 수 없습니다", "closures cannot use `ensures` yet"), f.line, 1)
                    .with_fix(tr!("`requires` 는 됩니다. `ensures` 가 필요하면 최상위 함수로 옮기세요", "`requires` works; move it to a top-level function if you need `ensures`")),
            );
        }

        // --- 만드는 자리 ---
        if caps.is_empty() {
            return format!("((MiClo){{ (void*){}, NULL }})", fname);
        }
        let mut init = String::new();
        for (n, t, src) in &caps {
            let cp = self.copy_value(src.clone(), t, line);
            let _ = write!(init, "_e->{} = {}; ", local(n), cp);
        }
        format!(
            "({{ struct {}* _e = (struct {}*)mi_alloc((int64_t)sizeof(struct {})); {}(MiClo){{ (void*){}, _e }}; }})",
            env_name, env_name, env_name, init, fname
        )
    }

    /// 값 의미론을 지키기 위한 복사. 리스트·사전처럼 속을 가리키는 값은
    /// 속까지 새로 만들어야 복사본을 고쳐도 원본이 그대로입니다.
    fn copy_value(&mut self, code: String, t: &Ty, line: usize) -> String {
        if !self.needs_copy(t) {
            return code;
        }
        match t {
            Ty::List(inner) if !self.needs_copy(inner) => format!("mi_list_copy({})", code),
            Ty::Dict(_, v) if !self.needs_copy(v) => format!("mi_dict_copy({})", code),
            _ => {
                let f = self.copy_fn(t, line);
                format!("{}({})", f, code)
            }
        }
    }

    /// 이 타입의 값을 복사할 때 속까지 새로 만들어야 하는가.
    fn needs_copy(&mut self, t: &Ty) -> bool {
        match t {
            Ty::List(_) | Ty::Dict(_, _) => true,
            Ty::Tuple(ts) => ts.iter().any(|x| self.needs_copy(x)),
            Ty::Optional(i) | Ty::Fallible(i, _) => self.needs_copy(i),
            Ty::Struct(n) => {
                if let Some(b) = self.copy_needed.get(n) {
                    return *b;
                }
                // 자기 자신을 품는 구조체를 위해 먼저 false 로 적어 둡니다.
                self.copy_needed.insert(n.clone(), false);
                let sd = match self.ty.structs.get(n).cloned() {
                    Some(sd) => sd,
                    None => return false,
                };
                let mut any = false;
                for f in &sd.fields {
                    if self.boxed.contains(&(n.clone(), f.name.clone())) {
                        any = true;
                    }
                    if let Some(te) = &f.ty {
                        let ft = self.ty.resolve(te, sd.line);
                        if self.needs_copy(&ft) {
                            any = true;
                        }
                    }
                }
                self.copy_needed.insert(n.clone(), any);
                any
            }
            _ => false,
        }
    }

    /// 속까지 복사하는 C 함수를 (한 번만) 만들고 그 이름을 냅니다.
    fn copy_fn(&mut self, t: &Ty, line: usize) -> String {
        let c = self.ctype(t, line);
        let name = format!("mi_cp_{}", ty_key(t));
        if self.copy_done.contains(&name) {
            return name;
        }
        self.copy_done.insert(name.clone());
        let _ = writeln!(self.protos, "static {} {}({} v);", c, name, c);
        let mut body = String::new();
        match t {
            Ty::List(inner) => {
                let ic = self.ctype(inner, line);
                let e = self.copy_value("(*p)".into(), inner, line);
                let _ = write!(
                    body,
                    "    MiList r = mi_list_copy(v);\n    for (int64_t i = 0; i < r.len; i++) {{ {}* p = ({}*)((char*)r.data + i * r.esz); *p = {}; }}\n    return r;\n",
                    ic, ic, e
                );
            }
            Ty::Dict(_, vt) => {
                let vc = self.ctype(vt, line);
                let e = self.copy_value("(*p)".into(), vt, line);
                let _ = write!(
                    body,
                    "    MiDict r = mi_dict_copy(v);\n    for (int64_t i = 0; i < r.len; i++) {{ {}* p = ({}*)(r.vals + i * r.vsz); *p = {}; }}\n    return r;\n",
                    vc, vc, e
                );
            }
            Ty::Tuple(ts) => {
                for (i, et) in ts.iter().enumerate() {
                    if self.needs_copy(et) {
                        let e = self.copy_value(format!("v.f{}", i), et, line);
                        let _ = writeln!(body, "    v.f{} = {};", i, e);
                    }
                }
                body.push_str("    return v;\n");
            }
            Ty::Optional(inner) => {
                let e = self.copy_value("v.val".into(), inner, line);
                let _ = writeln!(body, "    if (v.has) v.val = {};\n    return v;", e);
            }
            Ty::Fallible(inner, _) => {
                let e = self.copy_value("v.val".into(), inner, line);
                let _ = writeln!(body, "    if (v.ok) v.val = {};\n    return v;", e);
            }
            Ty::Struct(n) => {
                if let Some(sd) = self.ty.structs.get(n).cloned() {
                    for f in &sd.fields {
                        if let Some(te) = &f.ty {
                            let ft = self.ty.resolve(te, sd.line);
                            if self.boxed.contains(&(n.clone(), f.name.clone())) {
                                // 상자에 든 필드: 새 상자를 만들어 속까지 복사합니다(값 의미론).
                                let fc = self.ctype(&ft, line);
                                let e = self.copy_value(format!("(*v.{})", local(&f.name)), &ft, line);
                                let _ = writeln!(
                                    body,
                                    "    if (v.{f}) {{ {c}* nb = ({c}*)mi_alloc((int64_t)sizeof({c})); *nb = {e}; v.{f} = nb; }}",
                                    f = local(&f.name),
                                    c = fc,
                                    e = e
                                );
                                continue;
                            }
                            if self.needs_copy(&ft) {
                                let fe = format!("v.{}", local(&f.name));
                                let e = self.copy_value(fe.clone(), &ft, line);
                                let _ = writeln!(body, "    {} = {};", fe, e);
                            }
                        }
                    }
                }
                body.push_str("    return v;\n");
            }
            _ => body.push_str("    return v;\n"),
        }
        let _ = write!(self.lifted, "static {} {}({} v) {{\n{}}}\n\n", c, name, c, body);
        name
    }

    /// 새로 만든 값이라 복사할 필요가 없는가(원소가 리스트가 아닌 리스트·사전 리터럴).
    fn is_fresh(&mut self, e: &Expr, t: &Ty) -> bool {
        match (e, t) {
            (Expr::List(_), Ty::List(inner)) => !self.needs_copy(inner),
            (Expr::Dict(_), Ty::Dict(_, v)) => !self.needs_copy(v),
            // 언제나 새 리스트를 만들어 돌려주는 내장 함수들. 아레나 리스트는
            // 복사하면 아레나를 떠나 버리므로 특히 복사하면 안 됩니다.
            (Expr::Call { callee, .. }, _) => match callee.as_ref() {
                Expr::Field(o, m, _, _) => {
                    (m == "list" && self.infer(o) == Ty::Arena)
                        || matches!(m.as_str(), "split" | "keys" | "find_all" | "groups" | "split_re")
                }
                Expr::Ident(n, _, _) => {
                    matches!(n.as_str(), "range" | "args") && self.ty.lookup(n).is_none() && !self.ty.fns.contains_key(n)
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// 대입·바인딩 자리에 들어가는 값을 값 의미론대로 복사합니다.
    fn bind_copy(&mut self, code: String, e: &Expr, t: &Ty, line: usize) -> String {
        if self.is_fresh(e, t) {
            code
        } else {
            self.copy_value(code, t, line)
        }
    }

    fn call(&mut self, callee: &Expr, targs: &[TypeExpr], args: &[Arg], line: usize, _col: usize) -> String {
        if let Expr::Field(obj, mname, l, _) = callee {
            if let Expr::Ident(m, _, _) = obj.as_ref() {
                if self.ty.lookup(m).is_none() && crate::types::is_std_module(m) {
                    if self.ty.fns.contains_key(mname.as_str()) {
                        let callee = Expr::Ident(mname.clone(), *l, 1);
                        return self.call(&callee, targs, args, line, _col);
                    }
                    return self.builtin(mname, args, *l);
                }
            }
            let ot = self.infer(obj);
            if let Some(Ty::Fn(ps, r)) = self.ty.fn_field(&ot, mname) {
                let o = self.expr(obj);
                return self.emit_indirect_call(format!("({}).{}", o, local(mname)), &ps, &r, args, *l);
            }
            if ot == Ty::Arena {
                let elem = match targs.first() {
                    Some(te) => self.ty.resolve(te, *l),
                    None => Ty::Int,
                };
                let ec = self.ctype(&elem, *l);
                let aname = self.expr(obj);
                return match mname.as_str() {
                    "list" => format!("mi_list_arena(&{}, (int64_t)sizeof({}))", aname, ec),
                    "alloc" => {
                        let n = match args.first() {
                            Some(a) => self.expr(&a.value),
                            None => "0".into(),
                        };
                        format!("MI_APTR({}, {}, {})", ec, aname, n)
                    }
                    other => {
                        self.errors.push(cerr(
                            "C0020",
                            tr!(format!("아레나에 `{}` 메서드가 없습니다", other), format!("arena has no method `{}`", other)),
                            *l,
                            1,
                        ));
                        "0".into()
                    }
                };
            }
            let owner = match &ot {
                Ty::Struct(n) | Ty::Enum(n) => Some(n.clone()),
                _ => None,
            };
            if let Some(o) = owner {
                let key = format!("{}.{}", o, mname);
                if let Some(sig) = self.ty.methods.get(&key).cloned() {
                    let decl = sig.decl.clone();
                    let self_inout = decl
                        .params
                        .iter()
                        .find(|p| p.is_self)
                        .map(|p| p.conv == Convention::Inout)
                        .unwrap_or(false);
                    let non_self: Vec<Convention> =
                        decl.params.iter().filter(|p| !p.is_self).map(|p| p.conv).collect();
                    let saved = self.expect.take();
                    // `inout self` 메서드는 수신자를 포인터로 넘깁니다.
                    let recv = if self_inout { self.inout_ref(obj) } else { self.expr(obj) };
                    let mut a = vec![recv];
                    // 매개변수 타입을 알려 줘야 `{}` 나 `[]` 같은 빈 값도 제 타입으로 만들어집니다.
                    let want: Vec<Ty> =
                        sig.params.iter().filter(|(n, _)| n != "self").map(|(_, t)| t.clone()).collect();
                    for (i, x) in args.iter().enumerate() {
                        let inout =
                            non_self.get(i).map(|cv| *cv == Convention::Inout).unwrap_or(false);
                        if inout {
                            a.push(self.inout_ref(&x.value));
                        } else if let Some(wt) = want.get(i).filter(|t| !matches!(t, Ty::Unknown)) {
                            let vt = self.infer(&x.value);
                            self.expect = Some(wt.clone());
                            let c = self.expr(&x.value);
                            self.expect = None;
                            a.push(self.coerce(c, &vt, wt, *l));
                        } else {
                            a.push(self.expr(&x.value));
                        }
                    }
                    self.expect = saved;
                    return format!("mu_{}_{}({})", o, mname, a.join(", "));
                }
            }
            return self.method(&ot, mname, obj, args, *l);
        }

        if let Expr::Ident(name, l, _) = callee {
            // 함수 값(지역 변수·인자로 받은 함수)을 부르면 간접 호출입니다.
            if self.ty.fns.get(name).is_none() {
                if let Some(Ty::Fn(params, ret)) = self.ty.lookup(name) {
                    let cexpr = self.cname(name);
                    return self.emit_indirect_call(cexpr, &params, &ret, args, *l);
                }
            }
            // `channel[T]()` / `channel[T](크기)`
            if name == "channel" && self.ty.fns.get(name).is_none() {
                self.uses_conc = true;
                let et = match self.infer(&Expr::Call { callee: Box::new(callee.clone()), targs: targs.to_vec(), args: args.to_vec(), line, col: _col }) {
                    Ty::Chan(t) => *t,
                    _ => Ty::Int,
                };
                let c = self.ctype(&et, *l);
                return match args.first() {
                    Some(a) => {
                        let n = self.expr(&a.value);
                        format!("mi_chan_new((int64_t)sizeof({}), {}, true)", c, n)
                    }
                    None => format!("mi_chan_new((int64_t)sizeof({}), 0, false)", c),
                };
            }
            // 메모리 Level 2 내장 함수
            if name == "alloc" {
                let elem = match targs.first() {
                    Some(te) => self.ty.resolve(te, *l),
                    None => Ty::Int,
                };
                let ec = self.ctype(&elem, *l);
                let n = match args.first() {
                    Some(a) => {
                        let saved = self.expect.take();
                        let v = self.expr(&a.value);
                        self.expect = saved;
                        v
                    }
                    None => "0".into(),
                };
                return format!("MI_ALLOC({}, {})", ec, n);
            }
            if name == "cstr" || name == "ptr_get" {
                let saved = self.expect.take();
                let vals: Vec<String> = args
                    .iter()
                    .map(|a| self.expr(&a.value))
                    .collect();
                self.expect = saved;
                if name == "cstr" {
                    return format!("mi_from_c((const char*)(intptr_t)({}))", vals.first().cloned().unwrap_or_else(|| "0".into()));
                }
                return format!(
                    "((int64_t*)(intptr_t)({}))[{}]",
                    vals.first().cloned().unwrap_or_else(|| "0".into()),
                    vals.get(1).cloned().unwrap_or_else(|| "0".into())
                );
            }
            if name == "free" {
                let p = match args.first() {
                    Some(a) => {
                        let saved = self.expect.take();
                        let v = self.expr(&a.value);
                        self.expect = saved;
                        v
                    }
                    None => "0".into(),
                };
                return format!("MI_FREE({})", p);
            }
            // 구조체 생성
            if let Some(sd) = self.ty.structs.get(name).cloned() {
                let mut inits = Vec::new();
                let mut pi = 0usize;
                let saved = self.expect.take();
                for f in &sd.fields {
                    let val = args
                        .iter()
                        .find(|a| a.name.as_deref() == Some(f.name.as_str()))
                        .map(|a| &a.value)
                        .or_else(|| {
                            let v = args.iter().filter(|a| a.name.is_none()).nth(pi).map(|a| &a.value);
                            pi += 1;
                            v
                        })
                        .or(f.default.as_ref());
                    if let Some(v) = val {
                        let ft = match &f.ty {
                            Some(te) => self.ty.resolve(te, *l),
                            None => Ty::Unknown,
                        };
                        let vt = self.infer(v);
                        self.expect = Some(ft.clone());
                        let c = self.expr(v);
                        self.expect = None;
                        let c = self.coerce(c, &vt, &ft, *l);
                        let c = self.bind_copy(c, v, &ft, *l);
                        let c = if self.boxed.contains(&(sd.name.clone(), f.name.clone())) {
                            let fc = self.ctype(&ft, *l);
                            format!("({{ {fc}* _b = ({fc}*)mi_alloc((int64_t)sizeof({fc})); *_b = {c}; _b; }})", fc = fc, c = c)
                        } else {
                            c
                        };
                        inits.push(format!(".{} = {}", local(&f.name), c));
                    } else if self.boxed.contains(&(sd.name.clone(), f.name.clone())) {
                        // 적지 않은 상자 필드도 빈 상자(none)를 달아 둡니다.
                        let ft = f.ty.as_ref().map(|te| self.ty.resolve(te, *l)).unwrap_or(Ty::Unknown);
                        let fc = self.ctype(&ft, *l);
                        inits.push(format!(
                            ".{} = ({fc}*)memset(mi_alloc((int64_t)sizeof({fc})), 0, sizeof({fc}))",
                            local(&f.name),
                            fc = fc
                        ));
                    }
                }
                self.expect = saved;
                return format!("((struct {}){{ {} }})", mangle(&sd.name), inits.join(", "));
            }
            // 열거형 변형 생성
            if let Some(ename) = self.ty.variant_of.get(name).cloned() {
                let ed = self.ty.enums.get(&ename).cloned().unwrap();
                let vd = ed.variants.iter().find(|v| &v.name == name).unwrap().clone();
                let mut inits = Vec::new();
                let mut pi = 0usize;
                let saved = self.expect.take();
                for f in &vd.fields {
                    let val = args
                        .iter()
                        .find(|a| a.name.as_deref() == Some(f.name.as_str()))
                        .map(|a| &a.value)
                        .or_else(|| {
                            let v = args.iter().filter(|a| a.name.is_none()).nth(pi).map(|a| &a.value);
                            pi += 1;
                            v
                        });
                    if let Some(v) = val {
                        let ft = f.ty.as_ref().map(|te| self.ty.resolve(te, *l)).unwrap_or(Ty::Int);
                        self.expect = Some(ft.clone());
                        let c = self.expr(v);
                        let c = self.bind_copy(c, v, &ft, *l);
                        // 열거형 필드는 포인터로 담으므로 힙에 올려 주소를 넣습니다.
                        let c = if let Ty::Enum(en2) = &ft {
                            let m = mangle(en2);
                            format!(
                                "({{ struct {}* _b = (struct {}*)mi_alloc((int64_t)sizeof(struct {})); *_b = {}; _b; }})",
                                m, m, m, c
                            )
                        } else {
                            c
                        };
                        inits.push(format!(".{} = {}", local(&f.name), c));
                    }
                }
                self.expect = saved;
                let payload = if vd.fields.is_empty() {
                    String::new()
                } else {
                    format!(", .u.{} = {{ {} }}", vd.name, inits.join(", "))
                };
                return format!(
                    "((struct {}){{ .tag = {}_{}{} }})",
                    mangle(&ename),
                    mangle(&ename),
                    vd.name,
                    payload
                );
            }
            // C 함수
            if self.ty.externs.contains(name) {
                let sig = self.ty.fns.get(name).cloned().unwrap();
                let saved = self.expect.take();
                let convs: Vec<Convention> = sig.decl.params.iter().map(|p| p.conv).collect();
                let cbs = sig.decl.c_sig.as_ref().map(|c| c.cbs.clone()).unwrap_or_default();
                let mut out: Vec<String> = Vec::new();
                for (i, (_, wt)) in sig.params.iter().enumerate() {
                    let v = match args.get(i) {
                        Some(a) => &a.value,
                        None => continue,
                    };
                    // `inout` 자리는 변수의 자리를 넘겨야 C가 거기에 써 넣습니다.
                    if convs.get(i) == Some(&Convention::Inout) {
                        out.push(self.inout_ref(v));
                        continue;
                    }
                    let c = self.expr(v);
                    // 내 함수를 라이브러리에 넘겨주는 자리.
                    if let Some(Some((cps, cret))) = cbs.get(i) {
                        if let Expr::Ident(fname, fl, _) = &v {
                            if let Some(t) = self.make_callback(fname, cps, cret, *fl) {
                                out.push(t);
                                continue;
                            }
                        }
                        self.errors.push(
                            SiskinError::new(
                                "C0020",
                                tr!("라이브러리에 넘길 함수는 이름 붙은 함수여야 합니다", "a function passed to a library must be a named function"),
                                *l,
                                1,
                            )
                            .with_fix(tr!(
                                "`fn 이름(...)` 으로 함수를 만들고 그 이름을 그대로 넘기세요. \
                                 바깥 변수를 붙잡는 익명 함수는 C로 넘길 수 없습니다",
                                "define it with `fn name(...)` and pass that name directly; \
                                 anonymous functions that capture outer variables cannot be passed to C",
                            )),
                        );
                        out.push("0".into());
                        continue;
                    }
                    out.push(match wt {
                        // C는 "0으로 끝나는 문자열"을 기대하므로 맞춰 줍니다.
                        Ty::Str => format!("mi_cstr({})", c),
                        Ty::Raw(inner) => {
                            let ec = self.ctype(inner, *l);
                            format!("MI_RAW({}, {})", ec, c)
                        }
                        _ => c,
                    });
                }
                self.expect = saved;
                self.used_externs.insert(name.clone());
                let call = format!("mx_{}({})", name, out.join(", "));
                return match &sig.ret {
                    Ty::Str => format!("mi_from_c({})", call),
                    _ => call,
                };
            }
            // 사용자 함수
            if self.ty.fns.contains_key(name) {
                let sig = self.ty.fns.get(name).cloned().unwrap();
                let want: Vec<(String, Ty)> =
                    sig.params.iter().filter(|(n, _)| n != "self").cloned().collect();

                // 제네릭 함수면 타입 매개변수를 채웁니다. 명시한 타입(`f[Int](...)`)이
                // 있으면 그걸 쓰고, 없으면 인자 타입에서 알아냅니다.
                let is_generic = !sig.decl.generics.is_empty();
                let mut subst: HashMap<String, Ty> = HashMap::new();
                if is_generic {
                    if !targs.is_empty() {
                        for (g, te) in sig.decl.generics.iter().zip(targs) {
                            let t = self.ty.resolve(te, *l);
                            subst.insert(g.clone(), t);
                        }
                    } else {
                        let mut gpi = 0usize;
                        for a in args {
                            let at = self.infer(&a.value);
                            let pty = match &a.name {
                                Some(n) => {
                                    want.iter().find(|(wn, _)| wn == n).map(|(_, t)| t.clone())
                                }
                                None => {
                                    let t = want.get(gpi).map(|(_, t)| t.clone());
                                    gpi += 1;
                                    t
                                }
                            };
                            if let Some(pty) = pty {
                                self.ty.unify(&pty, &at, &mut subst);
                            }
                        }
                    }
                }

                // 제네릭이면 특수화 이름을 쓰고 대기열에 올립니다.
                let cname = if is_generic {
                    let suffix = self.mono_suffix_of(&sig.decl, &subst);
                    let full = format!("{}{}", mangle(name), suffix);
                    if !self.mono_done.contains(&full) {
                        self.mono_done.insert(full.clone());
                        self.mono_queue.push((sig.decl.clone(), subst.clone(), suffix));
                    }
                    full
                } else {
                    mangle(name)
                };

                let mut out: Vec<String> = Vec::new();
                let mut used: HashSet<usize> = HashSet::new();
                let positional: Vec<&Arg> = args.iter().filter(|a| a.name.is_none()).collect();
                let mut pi = 0usize;
                let saved = self.expect.take();
                for (wn, wt) in &want {
                    // 제네릭 매개변수 타입(T 등)은 실제 타입으로 바꿉니다.
                    let wt: Ty = if is_generic { self.ty.substitute(wt, &subst) } else { wt.clone() };
                    let v = args
                        .iter()
                        .find(|a| a.name.as_deref() == Some(wn.as_str()))
                        .map(|a| &a.value)
                        .or_else(|| {
                            let v = positional.get(pi).map(|a| &a.value);
                            used.insert(pi);
                            pi += 1;
                            v
                        });
                    let is_inout = sig
                        .decl
                        .params
                        .iter()
                        .find(|p| &p.name == wn)
                        .map(|p| p.conv == Convention::Inout)
                        .unwrap_or(false);
                    match v {
                        Some(v) => {
                            if is_inout {
                                // 포인터로 넘깁니다. 인자가 lvalue임은 타입검사기가 보장합니다.
                                self.expect = None;
                                out.push(self.inout_ref(v));
                            } else {
                                let vt = self.infer(v);
                                self.expect = Some(wt.clone());
                                let c = self.expr(v);
                                self.expect = None;
                                let c = self.coerce(c, &vt, &wt, *l);
                                out.push(c);
                            }
                        }
                        None => out.push("0".into()),
                    }
                }
                self.expect = saved;
                return format!("{}({})", cname, out.join(", "));
            }
            return self.builtin(name, args, *l);
        }

        // 함수 값을 돌려주는 식을 바로 부르기: `구하기()(x)` 등.
        if let Ty::Fn(params, ret) = self.infer(callee) {
            let cexpr = self.expr(callee);
            return self.emit_indirect_call(cexpr, &params, &ret, args, line);
        }

        self.errors.push(cerr("C0015", tr!("호출할 수 없는 대상입니다", "this value is not callable"), line, 1));
        "0".into()
    }

    fn method(&mut self, recv: &Ty, name: &str, obj: &Expr, args: &[Arg], line: usize) -> String {
        let saved = self.expect.take();
        let r = match (recv, name) {
            (Ty::Task(t), "wait") => {
                let b = self.expr(obj);
                if matches!(**t, Ty::Unit) {
                    format!("((void)mi_task_wait({}))", b)
                } else {
                    // 결과는 복사해서 줍니다(여러 번 기다려도 서로 다른 값).
                    let c = self.ctype(t, line);
                    let v = format!("(*({}*)mi_task_wait({}))", c, b);
                    self.copy_value(v, t, line)
                }
            }
            (Ty::Task(_), "done") => {
                let b = self.expr(obj);
                format!("mi_task_done({})", b)
            }
            (Ty::Chan(t), "send") => {
                let b = self.expr(obj);
                let c = self.ctype(t, line);
                let vt = self.infer(&args[0].value);
                self.expect = Some((**t).clone());
                let v = self.expr(&args[0].value);
                self.expect = None;
                let v = self.coerce(v, &vt, t, line);
                let v = self.bind_copy(v, &args[0].value, t, line);
                format!("({{ {} _v = {}; mi_chan_send({}, &_v); }})", c, v, b)
            }
            (Ty::Chan(t), "recv") => {
                let b = self.expr(obj);
                let on = self.opt_name(t, line);
                format!("({{ {} _o; _o.has = mi_chan_recv({}, &_o.val); _o; }})", on, b)
            }
            (Ty::Chan(_), "close") => {
                let b = self.expr(obj);
                format!("mi_chan_close({})", b)
            }
            (Ty::List(_), "len") => {
                let b = self.expr(obj);
                format!("(({}).len)", b)
            }
            (Ty::Str, "len") => {
                // 글자 수입니다. 바이트 수가 아닙니다.
                let b = self.expr(obj);
                let t = self.next_tmp();
                format!("({{ MiStr {} = {}; mi_utf8_len({}.p, {}.len); }})", t, b, t, t)
            }
            (Ty::List(t), "push") => {
                let lv = self.lvalue(obj);
                let c = self.ctype(t, line);
                let v = self.expr(&args[0].value);
                let v = self.bind_copy(v, &args[0].value, t, line);
                format!("({{ {} _v = {}; mi_list_push(&{}, &_v); }})", c, v, lv)
            }
            (Ty::List(t), "pop") => {
                let lv = self.lvalue(obj);
                let c = self.ctype(t, line);
                let on = self.opt_name(t, line);
                format!(
                    "({{ {} _o; if ({}.len > 0) {{ {}.len--; _o.has = true; _o.val = *({}*)((char*){}.data + {}.len * {}.esz); }} else _o.has = false; _o; }})",
                    on, lv, lv, c, lv, lv, lv
                )
            }
            (Ty::Json, _) => {
                let b = self.expr(obj);
                match name {
                    "kind" => format!("mi_json_kind({})", b),
                    "len" => format!("mi_json_len({})", b),
                    "keys" => format!("mi_json_keys({})", b),
                    "as_int" => format!("mi_json_as_int({})", b),
                    "as_float" => format!("mi_json_as_float({})", b),
                    "as_str" => format!("mi_json_as_str({})", b),
                    "as_bool" => format!("mi_json_as_bool({})", b),
                    "get" => {
                        let k = self.expr(&args[0].value);
                        format!("mi_json_get({}, {})", b, k)
                    }
                    "at" => {
                        let i = self.expr(&args[0].value);
                        format!("mi_json_at({}, {})", b, i)
                    }
                    "set" => {
                        let k = self.expr(&args[0].value);
                        let v = self.expr(&args[1].value);
                        format!("mi_json_set({}, {}, {})", b, k, v)
                    }
                    "push" => {
                        let v = self.expr(&args[0].value);
                        format!("mi_json_push({}, {})", b, v)
                    }
                    other => {
                        self.errors.push(cerr(
                            "C0022",
                            tr!(format!("Json에 `{}` 메서드가 없습니다", other), format!("Json has no method `{}`", other)),
                            line,
                            1,
                        ));
                        "0".into()
                    }
                }
            }
            (Ty::Dict(_, _), "len") => {
                let b = self.expr(obj);
                format!("(({}).len)", b)
            }
            (Ty::Dict(_, _), "set") => {
                let dt = recv.clone();
                let (kc, vc, _) = self.dict_parts(&dt, line);
                let lv = self.lvalue(obj);
                let kk = self.expr(&args[0].value);
                let vv = self.expr(&args[1].value);
                format!(
                    "({{ {} _k = {}; {} _v = {}; mi_dict_set(&{}, &_k, &_v); }})",
                    kc, kk, vc, vv, lv
                )
            }
            (Ty::Dict(_, _), "has" | "contains") => {
                let dt = recv.clone();
                let (kc, _, _) = self.dict_parts(&dt, line);
                let b = self.expr(obj);
                let kk = self.expr(&args[0].value);
                let dv = self.next_tmp();
                let kv = self.next_tmp();
                format!(
                    "({{ MiDict {} = {}; {} {} = {}; mi_dict_has(&{}, &{}); }})",
                    dv, b, kc, kv, kk, dv, kv
                )
            }
            (Ty::Dict(_, _), "keys") => {
                let b = self.expr(obj);
                let dv = self.next_tmp();
                format!("({{ MiDict {} = {}; mi_dict_keys(&{}); }})", dv, b, dv)
            }
            (Ty::Dict(_, _), "get") => {
                let dt = recv.clone();
                let (kc, vc, _) = self.dict_parts(&dt, line);
                let b = self.expr(obj);
                let kk = self.expr(&args[0].value);
                let def = self.expr(&args[1].value);
                let dv = self.next_tmp();
                let kv = self.next_tmp();
                let pv = self.next_tmp();
                // 키가 있으면 그 값을, 없으면 기본값을 냅니다.
                format!(
                    "({{ MiDict {d} = {b}; {kc} {kv} = {kk}; {vc}* {pv} = ({vc}*)mi_dict_at(&{d}, &{kv}); {pv} ? *{pv} : ({def}); }})",
                    d = dv, b = b, kc = kc, kv = kv, kk = kk, vc = vc, pv = pv, def = def
                )
            }
            (Ty::List(t), "map" | "filter" | "any" | "all" | "sort_by") => {
                let ec = self.ctype(t, line);
                let hint = Ty::Fn(vec![(**t).clone()], Box::new(Ty::Unknown));
                let ft = {
                    if matches!(args[0].value, Expr::Lambda(..)) {
                        self.ty.lambda_hint_set(Some(hint.clone()));
                    }
                    self.infer(&args[0].value)
                };
                let (fps, fr) = match &ft {
                    Ty::Fn(ps, r) => (ps.clone(), (**r).clone()),
                    _ => (vec![(**t).clone()], Ty::Unknown),
                };
                let fp = self.fn_ptr_name(&fps, &fr, line);
                self.expect = Some(hint);
                let fc = self.expr(&args[0].value);
                self.expect = None;
                let (lv, fv, iv, xv) = (self.next_tmp(), self.next_tmp(), self.next_tmp(), self.next_tmp());
                let src = if name == "sort_by" { format!("&{}", self.lvalue(obj)) } else { self.expr(obj) };
                let head = if name == "sort_by" {
                    format!("MiList* {lv} = {src}; MiClo {fv} = {fc};", lv = lv, src = src, fv = fv, fc = fc)
                } else {
                    format!("MiList {lv}_v = {src}; MiList* {lv} = &{lv}_v; MiClo {fv} = {fc};", lv = lv, src = src, fv = fv, fc = fc)
                };
                let call = format!("(({fp}){fv}.fn)({fv}.env, {xv})", fp = fp, fv = fv, xv = xv);
                let each = format!(
                    "for (int64_t {iv} = 0; {iv} < {lv}->len; {iv}++) {{ {ec} {xv} = *({ec}*)mi_list_at({lv}, {iv});",
                    iv = iv, lv = lv, ec = ec, xv = xv
                );
                match name {
                    "map" => {
                        let rc = self.ctype(&fr, line);
                        let cp = self.copy_value("_y".into(), &fr, line);
                        format!(
                            "({{ {head} MiList _o = mi_list_new((int64_t)sizeof({rc})); {each} {rc} _y = {call}; _y = {cp}; mi_list_push(&_o, &_y); }} _o; }})",
                            head = head, rc = rc, each = each, call = call, cp = cp
                        )
                    }
                    "filter" => {
                        let cp = self.copy_value(xv.clone(), t, line);
                        format!(
                            "({{ {head} MiList _o = mi_list_new((int64_t)sizeof({ec})); {each} if ({call}) {{ {ec} _y = {cp}; mi_list_push(&_o, &_y); }} }} _o; }})",
                            head = head, ec = ec, each = each, call = call, cp = cp
                        )
                    }
                    "any" | "all" => {
                        let (init, test) = if name == "any" { ("false", "") } else { ("true", "!") };
                        format!(
                            "({{ {head} bool _r = {init}; {each} if ({test}{call}) {{ _r = !{init}; break; }} }} _r; }})",
                            head = head, init = init, each = each, test = test, call = call
                        )
                    }
                    _ => {
                        let (kc, kind) = match fr {
                            Ty::Float => ("double", 1),
                            Ty::Str => ("MiStr", 2),
                            _ => ("int64_t", 0),
                        };
                        format!(
                            "({{ {head} {kc}* _k = ({kc}*)malloc((size_t)({lv}->len + 1) * sizeof({kc})); {each} _k[{iv}] = {call}; }} mi_list_sort_keyed({lv}, _k, {kind}); free(_k); }})",
                            head = head, kc = kc, lv = lv, each = each, iv = iv, call = call, kind = kind
                        )
                    }
                }
            }
            (Ty::List(t), "sort") => {
                let kind = match &**t {
                    Ty::Float => 1,
                    Ty::Str => 2,
                    Ty::Bool => 3,
                    _ => 0,
                };
                let lv = self.lvalue(obj);
                format!("mi_list_sort(&{}, {})", lv, kind)
            }
            (Ty::List(t), "index_of" | "contains") => {
                let ec = self.ctype(t, line);
                let b = self.expr(obj);
                let v = self.expr(&args[0].value);
                let lt = self.next_tmp();
                let vt = self.next_tmp();
                let it = self.next_tmp();
                let rt = self.next_tmp();
                // 구조체·리스트 원소도 run 과 같게 깊이 비교합니다.
                let eq = self.eq_of(t, format!("(*({}*)mi_list_at(&{}, {}))", ec, lt, it), vt.clone(), line);
                let found = format!(
                    "({{ MiList {} = {}; {} {} = {}; int64_t {} = -1; for (int64_t {} = 0; {} < {}.len; {}++) {{ if ({}) {{ {} = {}; break; }} }} {}; }})",
                    lt, b, ec, vt, v, rt, it, it, lt, it, eq, rt, it, rt
                );
                if name == "contains" {
                    format!("({} >= 0)", found)
                } else {
                    found
                }
            }
            (Ty::List(_), "slice") => {
                let b = self.expr(obj);
                let a0 = self.expr(&args[0].value);
                let a1 = self.expr(&args[1].value);
                format!("mi_list_slice({}, {}, {})", b, a0, a1)
            }
            (Ty::List(_), "clear") => {
                let lv = self.lvalue(obj);
                format!("mi_list_clear(&{})", lv)
            }
            (Ty::List(_), "reverse") => {
                let lv = self.lvalue(obj);
                format!("mi_list_reverse(&{})", lv)
            }
            (Ty::Str, "find") => {
                let b = self.expr(obj);
                let v = self.expr(&args[0].value);
                format!("mi_str_find({}, {})", b, v)
            }
            (Ty::Str, "repeat") => {
                let b = self.expr(obj);
                let n = self.expr(&args[0].value);
                format!("mi_str_repeat({}, {})", b, n)
            }
            (Ty::Str, "slice") => {
                let b = self.expr(obj);
                let a0 = self.expr(&args[0].value);
                let a1 = self.expr(&args[1].value);
                format!("mi_str_slice({}, {}, {})", b, a0, a1)
            }
            (Ty::Str, "pad_right") => {
                let b = self.expr(obj);
                let w = self.expr(&args[0].value);
                format!("mi_pad_right({}, {})", b, w)
            }
            (Ty::Str, "pad_left") => {
                let b = self.expr(obj);
                let w = self.expr(&args[0].value);
                format!("mi_pad_left({}, {})", b, w)
            }
            (Ty::Str, "width") => {
                let b = self.expr(obj);
                let t = self.next_tmp();
                format!("({{ MiStr {} = {}; mi_disp_width({}); }})", t, b, t)
            }
            (Ty::List(_), "join") => {
                let b = self.expr(obj);
                let sep = self.expr(&args[0].value);
                format!("mi_join({}, {})", b, sep)
            }
            (Ty::Str, "split") => {
                let b = self.expr(obj);
                let sep = self.expr(&args[0].value);
                format!("mi_split({}, {})", b, sep)
            }
            (Ty::Str, "upper") => {
                let b = self.expr(obj);
                self.uses_case = true;
                format!("mi_upper({})", b)
            }
            (Ty::Str, "lower") => {
                let b = self.expr(obj);
                self.uses_case = true;
                format!("mi_lower({})", b)
            }
            (Ty::Str, "strip") => {
                let b = self.expr(obj);
                format!("mi_strip({})", b)
            }
            (Ty::Str, "contains") => {
                let b = self.expr(obj);
                let t = self.expr(&args[0].value);
                format!("mi_contains({}, {})", b, t)
            }
            (Ty::Str, "starts_with") => {
                let b = self.expr(obj);
                let t = self.expr(&args[0].value);
                format!("mi_starts_with({}, {})", b, t)
            }
            (Ty::Str, "ends_with") => {
                let b = self.expr(obj);
                let t = self.expr(&args[0].value);
                format!("mi_ends_with({}, {})", b, t)
            }
            (Ty::Str, "replace") => {
                let b = self.expr(obj);
                let f = self.expr(&args[0].value);
                let t = self.expr(&args[1].value);
                format!("mi_replace({}, {}, {})", b, f, t)
            }
            (other, _) => {
                self.errors.push(
                    cerr(
                        "C0016",
                        tr!(format!("{}의 `{}` 메서드는 아직 네이티브 컴파일이 안 됩니다", other, name), format!("method `{1}` of {0} is not supported by the native build yet", other, name)),
                        line,
                        1,
                    )
                    .with_fix(tr!("`siskin run`에서는 동작합니다", "it works under `siskin run`")),
                );
                "0".into()
            }
        };
        self.expect = saved;
        r
    }

    fn builtin(&mut self, name: &str, args: &[Arg], line: usize) -> String {
        let saved = self.expect.take();
        let r = match name {
            "eprint" => {
                let mut parts = vec!["fflush(stdout)".to_string()];
                for a in args {
                    let s = self.to_str(&a.value);
                    parts.push(format!("({{ MiStr _e = {}; fwrite(_e.p, 1, (size_t)_e.len, stderr); }})", s));
                }
                format!("({})", parts.join(", "))
            }
            "print" => {
                let mut parts = Vec::new();
                for a in args {
                    let s = self.to_str(&a.value);
                    parts.push(format!("mi_print({})", s));
                }
                format!("({})", parts.join(", "))
            }
            "len" => {
                let at = self.infer(&args[0].value);
                let a = self.expr(&args[0].value);
                if at == Ty::Str {
                    // 글자 수입니다. 바이트 수가 아닙니다.
                    let t = self.next_tmp();
                    format!("({{ MiStr {} = {}; mi_utf8_len({}.p, {}.len); }})", t, a, t, t)
                } else {
                    format!("(({}).len)", a)
                }
            }
            "range" => {
                if args.len() == 1 {
                    let b = self.expr(&args[0].value);
                    format!("mi_range(0, {})", b)
                } else {
                    let a = self.expr(&args[0].value);
                    let b = self.expr(&args[1].value);
                    format!("mi_range({}, {})", a, b)
                }
            }
            "str" => self.to_str(&args[0].value),
            "input" => {
                if let Some(a) = args.first() {
                    let p = self.to_str(&a.value);
                    format!("({{ mi_print({}); mi_input(); }})", p)
                } else {
                    "mi_input()".into()
                }
            }
            "args" => "mi_prog_args".into(),
            "exit" => {
                let code = args
                    .first()
                    .map(|a| self.expr(&a.value))
                    .unwrap_or_else(|| "0".into());
                format!("exit((int)({}))", code)
            }
            "error" => {
                let at = self.infer(&args[0].value);
                let msg = if matches!(at, Ty::Enum(_)) {
                    // enum 오류는 값을 그대로 담습니다.
                    let v = self.expr(&args[0].value);
                    self.copy_value(v, &at, line)
                } else {
                    self.to_str(&args[0].value)
                };
                let (inner, et) = match saved.clone() {
                    Some(Ty::Fallible(x, e)) if *x != Ty::Unknown => ((*x).clone(), (*e).clone()),
                    _ => match self.cur_ret.clone() {
                        Ty::Fallible(x, e) => ((*x).clone(), (*e).clone()),
                        _ => {
                            self.errors.push(
                                cerr("C0021", tr!("`error(...)`가 어떤 `!T`인지 알 수 없습니다", "cannot tell which fallible type `!T` this `error(...)` is"), line, 1)
                                    .with_fix(tr!("함수 반환 타입에 `!T`를 적어 주세요", "annotate the function's return type with `!T`")),
                            );
                            (Ty::Int, Ty::Str)
                        }
                    },
                };
                let rn = self.res_name(&inner, &et, line);
                format!("(({}){{ .ok = false, .err = {} }})", rn, msg)
            }
            "int" => {
                let t = self.infer(&args[0].value);
                let a = self.expr(&args[0].value);
                if t == Ty::Str {
                    format!("mi_parse_int({})", a)
                } else {
                    format!("((int64_t)({}))", a)
                }
            }
            "float" => {
                let t = self.infer(&args[0].value);
                let a = self.expr(&args[0].value);
                if t == Ty::Str {
                    format!("mi_parse_float({})", a)
                } else {
                    format!("((double)({}))", a)
                }
            }
            // ---- std.math ----
            "sin" | "cos" | "tan" | "exp" | "log10" => {
                let a = self.expr(&args[0].value);
                let f = if name == "log10" { "log10" } else { name };
                format!("{}({})", f, a)
            }
            "log" => {
                let a = self.expr(&args[0].value);
                format!("log({})", a)
            }
            "round" => {
                let a = self.expr(&args[0].value);
                format!("((int64_t)round({}))", a)
            }
            "pi" => "3.14159265358979311600".into(),
            "e" => "2.71828182845904509080".into(),

            // ---- std.time ----
            "now" => "mi_now()".into(),
            "sleep" => {
                let a = self.expr(&args[0].value);
                format!("mi_sleep({})", a)
            }
            "__time_parts" => {
                let a = self.expr(&args[0].value);
                let b = self.expr(&args[1].value);
                format!("mi_time_parts({}, {})", a, b)
            }
            "__time_make" => {
                let v: Vec<String> = args.iter().map(|a| self.expr(&a.value)).collect();
                format!("mi_time_make({})", v.join(", "))
            }
            "env" => {
                let a = self.expr(&args[0].value);
                let on = self.opt_name(&Ty::Str, line);
                format!(
                    "({{ const char* _e = getenv(mi_cstr({})); _e ? (({}){{ .has = true, .val = mi_from_c(_e) }}) : (({}){{ .has = false }}); }})",
                    a, on, on
                )
            }
            "set_env" => {
                let a = self.expr(&args[0].value);
                let b = self.expr(&args[1].value);
                format!("mi_set_env({}, {})", a, b)
            }
            "cwd" => "mi_cwd()".into(),
            "set_cwd" => {
                let a = self.expr(&args[0].value);
                format!("mi_set_cwd({})", a)
            }
            "pid" => "((int64_t)getpid())".into(),
            // ---- std.net (네이티브 전용) ----
            n if n.starts_with("__net_") || n.starts_with("__http") || n == "__url_encode" => {
                self.uses_net = true;
                let mut v: Vec<String> = Vec::new();
                for (i, a) in args.iter().enumerate() {
                    if n == "__http" && i == 2 {
                        self.expect = Some(Ty::List(Box::new(Ty::Str)));
                    }
                    v.push(self.expr(&a.value));
                    self.expect = None;
                }
                let cname = match n {
                    "__net_timeout" => "mi_net_set_timeout".to_string(),
                    other => format!("mi_{}", &other[2..]),
                };
                format!("{}({})", cname, v.join(", "))
            }
            "__run" => {
                let a = self.expr(&args[0].value);
                self.expect = Some(Ty::List(Box::new(Ty::Str)));
                let b = self.expr(&args[1].value);
                self.expect = None;
                let c = self.expr(&args[2].value);
                format!("mi_run({}, {}, {})", a, b, c)
            }
            "__run_out" => "mi_run_out_s".into(),
            "__ko" => "((bool)MI_KO)".into(),
            "__run_err" => "mi_run_err_s".into(),
            "list_dir" => {
                let a = self.expr(&args[0].value);
                let rn = self.res_name(&Ty::List(Box::new(Ty::Str)), &Ty::Str, line);
                format!(
                    "({{ MiRes_MiList_S _r = mi_list_dir({}); (({}){{ .ok = _r.ok, .val = _r.val, .err = _r.err }}); }})",
                    a, rn
                )
            }
            "make_dir" => {
                let a = self.expr(&args[0].value);
                format!("mi_make_dir({})", a)
            }
            "is_dir" => {
                let a = self.expr(&args[0].value);
                format!("mi_is_dir({})", a)
            }
            "clock" => "mi_clock()".into(),

            // ---- std.random ----
            "seed" => {
                let a = self.expr(&args[0].value);
                format!("mi_seed({})", a)
            }
            "rand" => "mi_rand()".into(),
            "rand_int" => {
                let a = self.expr(&args[0].value);
                let b = self.expr(&args[1].value);
                format!("mi_rand_int({}, {})", a, b)
            }

            // ---- std.json ----
            "parse" => {
                let a = self.expr(&args[0].value);
                format!("mi_json_parse({})", a)
            }
            "stringify" => {
                let a = self.expr(&args[0].value);
                format!("mi_json_write({})", a)
            }
            "jnull" => "mi_json_null()".into(),
            "jlist" => "mi_json_list()".into(),
            "jdict" => "mi_json_dict()".into(),
            "jbool" | "jint" | "jfloat" | "jstr" => {
                let a = self.expr(&args[0].value);
                let f = match name {
                    "jbool" => "mi_json_bool",
                    "jint" => "mi_json_int",
                    "jfloat" => "mi_json_float",
                    _ => "mi_json_str",
                };
                format!("{}({})", f, a)
            }

            // ---- std.re ----
            "test" | "find_all" | "groups" | "split_re" => {
                let a = self.expr(&args[0].value);
                let b = self.expr(&args[1].value);
                let f = match name {
                    "test" => "mi_re_test",
                    "find_all" => "mi_re_find_all",
                    "groups" => "mi_re_groups",
                    _ => "mi_re_split",
                };
                format!("{}({}, {})", f, a, b)
            }
            "find" if args.len() == 2 => {
                let a = self.expr(&args[0].value);
                let b = self.expr(&args[1].value);
                format!("mi_re_find({}, {})", a, b)
            }
            "replace" if args.len() == 3 => {
                let a = self.expr(&args[0].value);
                let b = self.expr(&args[1].value);
                let c = self.expr(&args[2].value);
                format!("mi_re_replace({}, {}, {})", a, b, c)
            }

            // ---- std.fs ----
            "write_text" | "append_text" => {
                let a = self.expr(&args[0].value);
                let b = self.to_str(&args[1].value);
                let ap = if name == "append_text" { 1 } else { 0 };
                format!("mi_write_text({}, {}, {})", a, b, ap)
            }
            "exists" => {
                let a = self.expr(&args[0].value);
                format!("mi_exists({})", a)
            }
            "remove" => {
                let a = self.expr(&args[0].value);
                format!("mi_remove({})", a)
            }

            // ---- 프렐류드 ----
            "sum" => {
                let lt = self.infer(&args[0].value);
                let ec = self.elem_ctype(&lt, line);
                let a = self.expr(&args[0].value);
                let t = self.next_tmp();
                let i = self.next_tmp();
                let acc = self.next_tmp();
                format!(
                    "({{ MiList {} = {}; {} {} = 0; for (int64_t {} = 0; {} < {}.len; {}++) {} += *({}*)mi_list_at(&{}, {}); {}; }})",
                    t, a, ec, acc, i, i, t, i, acc, ec, t, i, acc
                )
            }

            "read_text" => {
                let a = self.expr(&args[0].value);
                format!("mi_read_text({})", a)
            }
            "abs" => {
                let t = self.infer(&args[0].value);
                let a = self.expr(&args[0].value);
                if t == Ty::Float {
                    format!("fabs({})", a)
                } else {
                    format!("llabs({})", a)
                }
            }
            "sqrt" => {
                let a = self.expr(&args[0].value);
                format!("sqrt({})", a)
            }
            "floor" => {
                let a = self.expr(&args[0].value);
                format!("((int64_t)floor({}))", a)
            }
            "ceil" => {
                let a = self.expr(&args[0].value);
                format!("((int64_t)ceil({}))", a)
            }
            "pow" => {
                let a = self.expr(&args[0].value);
                let b = self.expr(&args[1].value);
                format!("pow({}, {})", a, b)
            }
            "min" | "max" => {
                let a = self.expr(&args[0].value);
                let b = self.expr(&args[1].value);
                // 인자를 한 번씩만 계산합니다 (부작용이 두 번 일어나지 않게). 인터프리터와 같은 규칙:
                // min 은 a < b 면 a, max 는 a < b 가 아니면 a.
                let ct = if self.infer(&args[0].value) == Ty::Float { "double" } else { "int64_t" };
                let (ta, tb) = (self.next_tmp(), self.next_tmp());
                let cond = if name == "min" {
                    format!("{} < {}", ta, tb)
                } else {
                    format!("!({} < {})", ta, tb)
                };
                format!("({{ {ct} {ta} = {a}; {ct} {tb} = {b}; ({cond}) ? {ta} : {tb}; }})", ct = ct, ta = ta, tb = tb, a = a, b = b, cond = cond)
            }
            "assert" => {
                let a = self.expr(&args[0].value);
                format!("((void)(({}) ? 0 : (mi_panic(MI_T(\"단언 실패\", \"assertion failed\")), 0)))", a)
            }
            other => {
                self.errors.push(
                    cerr("C0017", tr!(format!("`{}`은(는) 아직 네이티브 컴파일이 안 됩니다", other), format!("`{}` is not supported by the native build yet", other)), line, 1)
                        .with_fix(tr!("`siskin run`(인터프리터)에서는 동작합니다", "it works under `siskin run` (the interpreter)")),
                );
                "0".into()
            }
        };
        self.expect = saved;
        r
    }
}

/// 정규식 엔진의 C판. `src/regex.rs` 와 같은 알고리즘이어야 합니다.
/// 그래야 `siskin run` 과 `siskin build` 가 같은 답을 냅니다.
const RUNTIME_RE: &str = r##"
/* ---------------- 정규식 ---------------- */
#define MI_RE_RANGES 64
#define MI_RE_GROUPS 10
#define MI_RE_STEPS  2000000

typedef struct { int neg; int n; uint32_t lo[MI_RE_RANGES], hi[MI_RE_RANGES]; } MiClass;

/* op: 0 Char, 1 Any, 2 Class, 3 Match, 4 Jmp, 5 Split, 6 Save, 7 Bol, 8 Eol */
typedef struct { int op; int64_t a, b; } MiInst;

typedef struct {
    MiInst* insts; int64_t ni, capi;
    MiClass* cls;  int64_t nc, capc;
    int ngroups;
    int ok;
} MiProg;

typedef struct {
    const uint32_t* src; int64_t len, pos;
    MiProg* p;
    int failed;
    char msg[160];
} MiReP;

static void mi_re_emit(MiProg* p, int op, int64_t a, int64_t b) {
    if (p->ni == p->capi) {
        p->capi = p->capi ? p->capi * 2 : 32;
        p->insts = (MiInst*)realloc(p->insts, (size_t)p->capi * sizeof(MiInst));
        if (!p->insts) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    }
    p->insts[p->ni].op = op; p->insts[p->ni].a = a; p->insts[p->ni].b = b;
    p->ni++;
}

static int64_t mi_re_addcls(MiProg* p, MiClass c) {
    if (p->nc == p->capc) {
        p->capc = p->capc ? p->capc * 2 : 8;
        p->cls = (MiClass*)realloc(p->cls, (size_t)p->capc * sizeof(MiClass));
        if (!p->cls) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    }
    p->cls[p->nc] = c;
    return p->nc++;
}

static void mi_re_err(MiReP* q, const char* m) {
    if (!q->failed) { q->failed = 1; snprintf(q->msg, sizeof q->msg, "%s", m); }
}

static uint32_t mi_re_at(MiReP* q) { return q->pos < q->len ? q->src[q->pos] : 0; }
static int mi_re_more(MiReP* q) { return q->pos < q->len; }

static uint32_t mi_re_escchar(uint32_t e) {
    switch (e) {
        case 'n': return '\n';
        case 't': return '\t';
        case 'r': return '\r';
        case '0': return 0;
        default:  return e;
    }
}

/* \d \w \s 와 그 대문자. 아니면 0을 냅니다. */
static int mi_re_escclass(uint32_t e, MiClass* out) {
    MiClass c; c.neg = 0; c.n = 0;
    int lower = (e == 'd' || e == 'w' || e == 's');
    int upper = (e == 'D' || e == 'W' || e == 'S');
    if (!lower && !upper) return 0;
    uint32_t k = lower ? e : (e + 32);
    if (k == 'd') {
        c.lo[c.n] = '0'; c.hi[c.n] = '9'; c.n++;
    } else if (k == 'w') {
        c.lo[c.n] = 'a'; c.hi[c.n] = 'z'; c.n++;
        c.lo[c.n] = 'A'; c.hi[c.n] = 'Z'; c.n++;
        c.lo[c.n] = '0'; c.hi[c.n] = '9'; c.n++;
        c.lo[c.n] = '_'; c.hi[c.n] = '_'; c.n++;
    } else {
        c.lo[c.n] = ' ';  c.hi[c.n] = ' ';  c.n++;
        c.lo[c.n] = '\t'; c.hi[c.n] = '\t'; c.n++;
        c.lo[c.n] = '\n'; c.hi[c.n] = '\n'; c.n++;
        c.lo[c.n] = '\r'; c.hi[c.n] = '\r'; c.n++;
    }
    c.neg = upper;
    *out = c;
    return 1;
}

static void mi_re_alt(MiReP* q);

/* Jmp/Split 의 목적지를 옮깁니다. */
static MiInst mi_re_shift(MiInst i, int64_t by) {
    if (i.op == 4) i.a += by;
    else if (i.op == 5) { i.a += by; i.b += by; }
    return i;
}

static void mi_re_class(MiReP* q) {
    MiClass c; c.neg = 0; c.n = 0;
    if (mi_re_more(q) && mi_re_at(q) == '^') { c.neg = 1; q->pos++; }
    int first = 1;
    for (;;) {
        if (!mi_re_more(q)) { mi_re_err(q, MI_T("`]` 가 없습니다", "missing `]`")); return; }
        uint32_t ch = mi_re_at(q);
        if (ch == ']' && !first) { q->pos++; break; }
        first = 0;
        q->pos++;
        if (ch == '\\') {
            if (!mi_re_more(q)) { mi_re_err(q, MI_T("`\\` 뒤에 글자가 없습니다", "missing character after `\\`")); return; }
            uint32_t e = mi_re_at(q); q->pos++;
            MiClass sub;
            if (mi_re_escclass(e, &sub)) {
                for (int i = 0; i < sub.n && c.n < MI_RE_RANGES; i++) {
                    c.lo[c.n] = sub.lo[i]; c.hi[c.n] = sub.hi[i]; c.n++;
                }
            } else if (c.n < MI_RE_RANGES) {
                uint32_t v = mi_re_escchar(e);
                c.lo[c.n] = v; c.hi[c.n] = v; c.n++;
            }
            continue;
        }
        if (mi_re_more(q) && mi_re_at(q) == '-' &&
            q->pos + 1 < q->len && q->src[q->pos + 1] != ']') {
            q->pos++;
            uint32_t hi = q->src[q->pos]; q->pos++;
            if (c.n < MI_RE_RANGES) { c.lo[c.n] = ch; c.hi[c.n] = hi; c.n++; }
        } else if (c.n < MI_RE_RANGES) {
            c.lo[c.n] = ch; c.hi[c.n] = ch; c.n++;
        }
    }
    int64_t id = mi_re_addcls(q->p, c);
    mi_re_emit(q->p, 2, id, 0);
}

static void mi_re_atom(MiReP* q) {
    if (q->failed) return;
    if (!mi_re_more(q)) { mi_re_err(q, MI_T("정규식이 갑자기 끝났습니다", "unexpected end of regex")); return; }
    uint32_t c = mi_re_at(q); q->pos++;
    if (c == '(') {
        int capture = !(mi_re_more(q) && mi_re_at(q) == '?' &&
                        q->pos + 1 < q->len && q->src[q->pos + 1] == ':');
        if (!capture) q->pos += 2;
        int g = 0;
        if (capture) {
            q->p->ngroups++;
            if (q->p->ngroups >= MI_RE_GROUPS) { mi_re_err(q, MI_T("괄호가 너무 많습니다", "too many groups")); return; }
            g = q->p->ngroups;
            mi_re_emit(q->p, 6, g * 2, 0);
        }
        mi_re_alt(q);
        if (q->failed) return;
        if (!(mi_re_more(q) && mi_re_at(q) == ')')) { mi_re_err(q, MI_T("`)` 가 없습니다", "missing `)`")); return; }
        q->pos++;
        if (capture) mi_re_emit(q->p, 6, g * 2 + 1, 0);
        return;
    }
    if (c == '[') { mi_re_class(q); return; }
    if (c == '.') { mi_re_emit(q->p, 1, 0, 0); return; }
    if (c == '^') { mi_re_emit(q->p, 7, 0, 0); return; }
    if (c == '$') { mi_re_emit(q->p, 8, 0, 0); return; }
    if (c == '\\') {
        if (!mi_re_more(q)) { mi_re_err(q, MI_T("`\\` 뒤에 글자가 없습니다", "missing character after `\\`")); return; }
        uint32_t e = mi_re_at(q); q->pos++;
        MiClass sub;
        if (mi_re_escclass(e, &sub)) {
            int64_t id = mi_re_addcls(q->p, sub);
            mi_re_emit(q->p, 2, id, 0);
        } else {
            mi_re_emit(q->p, 0, (int64_t)mi_re_escchar(e), 0);
        }
        return;
    }
    if (c == ')') { mi_re_err(q, MI_T("짝이 없는 `)` 입니다", "unmatched `)`")); return; }
    if (c == '*' || c == '+') { mi_re_err(q, MI_T("앞에 반복할 것이 없습니다", "nothing to repeat")); return; }
    mi_re_emit(q->p, 0, (int64_t)c, 0);
}

static void mi_re_repeat(MiReP* q) {
    int64_t start = q->p->ni;
    mi_re_atom(q);
    if (q->failed) return;
    for (;;) {
        if (!mi_re_more(q)) return;
        uint32_t c = mi_re_at(q);
        if (c != '*' && c != '+' && c != '?') return;
        q->pos++;
        int lazy = (mi_re_more(q) && mi_re_at(q) == '?');
        if (lazy) q->pos++;

        int64_t blen = q->p->ni - start;
        MiInst* body = (MiInst*)malloc((size_t)(blen > 0 ? blen : 1) * sizeof(MiInst));
        if (!body) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
        memcpy(body, q->p->insts + start, (size_t)blen * sizeof(MiInst));
        q->p->ni = start;

        if (c == '*') {
            mi_re_emit(q->p, 5, 0, 0);
            int64_t split_at = q->p->ni - 1;
            int64_t shift = q->p->ni - start;
            for (int64_t i = 0; i < blen; i++) {
                MiInst x = mi_re_shift(body[i], shift);
                mi_re_emit(q->p, x.op, x.a, x.b);
            }
            mi_re_emit(q->p, 4, split_at, 0);
            int64_t after = q->p->ni;
            q->p->insts[split_at].a = lazy ? after : split_at + 1;
            q->p->insts[split_at].b = lazy ? split_at + 1 : after;
        } else if (c == '+') {
            for (int64_t i = 0; i < blen; i++) mi_re_emit(q->p, body[i].op, body[i].a, body[i].b);
            int64_t split_at = q->p->ni;
            mi_re_emit(q->p, 5, 0, 0);
            int64_t after = q->p->ni;
            q->p->insts[split_at].a = lazy ? after : start;
            q->p->insts[split_at].b = lazy ? start : after;
        } else {
            mi_re_emit(q->p, 5, 0, 0);
            int64_t split_at = q->p->ni - 1;
            int64_t shift = q->p->ni - start;
            for (int64_t i = 0; i < blen; i++) {
                MiInst x = mi_re_shift(body[i], shift);
                mi_re_emit(q->p, x.op, x.a, x.b);
            }
            int64_t after = q->p->ni;
            q->p->insts[split_at].a = lazy ? after : split_at + 1;
            q->p->insts[split_at].b = lazy ? split_at + 1 : after;
        }
        free(body);
    }
}

static void mi_re_concat(MiReP* q) {
    while (!q->failed && mi_re_more(q)) {
        uint32_t c = mi_re_at(q);
        if (c == '|' || c == ')') break;
        mi_re_repeat(q);
    }
}

static void mi_re_alt(MiReP* q) {
    int64_t start = q->p->ni;
    mi_re_concat(q);
    while (!q->failed && mi_re_more(q) && mi_re_at(q) == '|') {
        q->pos++;
        int64_t blen = q->p->ni - start;
        MiInst* body = (MiInst*)malloc((size_t)(blen > 0 ? blen : 1) * sizeof(MiInst));
        if (!body) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
        memcpy(body, q->p->insts + start, (size_t)blen * sizeof(MiInst));
        q->p->ni = start;

        mi_re_emit(q->p, 5, 0, 0);
        int64_t split_at = q->p->ni - 1;
        int64_t shift = q->p->ni - start;
        for (int64_t i = 0; i < blen; i++) {
            MiInst x = mi_re_shift(body[i], shift);
            mi_re_emit(q->p, x.op, x.a, x.b);
        }
        free(body);
        mi_re_emit(q->p, 4, 0, 0);
        int64_t jmp_at = q->p->ni - 1;
        int64_t right = q->p->ni;
        mi_re_concat(q);
        int64_t end = q->p->ni;
        q->p->insts[split_at].a = split_at + 1;
        q->p->insts[split_at].b = right;
        q->p->insts[jmp_at].a = end;
    }
}

/* MiStr 을 코드포인트 배열로. 한글도 한 글자로 셉니다. */
static int64_t mi_utf8_decode(MiStr s, uint32_t** out) {
    int64_t n = mi_utf8_len(s.p, s.len);
    uint32_t* buf = (uint32_t*)malloc((size_t)(n > 0 ? n : 1) * sizeof(uint32_t));
    if (!buf) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    int64_t k = 0;
    for (int64_t i = 0; i < s.len; ) {
        unsigned char c = (unsigned char)s.p[i];
        uint32_t cp; int width;
        if (c < 0x80)            { cp = c;          width = 1; }
        else if ((c & 0xE0) == 0xC0) { cp = c & 0x1F; width = 2; }
        else if ((c & 0xF0) == 0xE0) { cp = c & 0x0F; width = 3; }
        else                     { cp = c & 0x07; width = 4; }
        for (int j = 1; j < width && i + j < s.len; j++)
            cp = (cp << 6) | ((unsigned char)s.p[i + j] & 0x3F);
        buf[k++] = cp;
        i += width;
    }
    *out = buf;
    return k;
}

static MiStr mi_utf8_encode(const uint32_t* cp, int64_t a, int64_t b) {
    if (b < a) b = a;
    char* buf = mi_alloc((b - a) * 4);
    int64_t k = 0;
    for (int64_t i = a; i < b; i++) {
        uint32_t c = cp[i];
        if (c < 0x80) buf[k++] = (char)c;
        else if (c < 0x800) {
            buf[k++] = (char)(0xC0 | (c >> 6));
            buf[k++] = (char)(0x80 | (c & 0x3F));
        } else if (c < 0x10000) {
            buf[k++] = (char)(0xE0 | (c >> 12));
            buf[k++] = (char)(0x80 | ((c >> 6) & 0x3F));
            buf[k++] = (char)(0x80 | (c & 0x3F));
        } else {
            buf[k++] = (char)(0xF0 | (c >> 18));
            buf[k++] = (char)(0x80 | ((c >> 12) & 0x3F));
            buf[k++] = (char)(0x80 | ((c >> 6) & 0x3F));
            buf[k++] = (char)(0x80 | (c & 0x3F));
        }
    }
    buf[k] = 0;
    return mi_mk(buf, k);
}

static MiProg mi_re_compile(MiStr pattern) {
    MiProg p;
    p.insts = NULL; p.ni = 0; p.capi = 0;
    p.cls = NULL; p.nc = 0; p.capc = 0;
    p.ngroups = 0; p.ok = 1;
    uint32_t* src; int64_t n = mi_utf8_decode(pattern, &src);
    MiReP q; q.src = src; q.len = n; q.pos = 0; q.p = &p; q.failed = 0; q.msg[0] = 0;
    mi_re_emit(&p, 6, 0, 0);
    mi_re_alt(&q);
    if (!q.failed && q.pos < q.len) mi_re_err(&q, MI_T("정규식을 끝까지 읽지 못했습니다", "could not parse the whole regex"));
    if (q.failed) {
        char b[256];
        snprintf(b, sizeof b, MI_T("정규식 오류: %s", "regex error: %s"), q.msg);
        free(src);
        mi_panic(b);
    }
    mi_re_emit(&p, 6, 1, 0);
    mi_re_emit(&p, 3, 0, 0);
    free(src);
    return p;
}

static int mi_class_has(const MiClass* c, uint32_t ch) {
    int inside = 0;
    for (int i = 0; i < c->n; i++) if (ch >= c->lo[i] && ch <= c->hi[i]) { inside = 1; break; }
    return c->neg ? !inside : inside;
}

typedef struct { int64_t pc, sp; int64_t saves[MI_RE_GROUPS * 2]; } MiReFrame;

/* start 자리에서 시작하는 일치. 찾으면 1을 내고 saves 를 채웁니다. */
static int mi_re_match_at(const MiProg* p, const uint32_t* in, int64_t n,
                          int64_t start, int64_t* saves) {
    int64_t cur[MI_RE_GROUPS * 2];
    for (int i = 0; i < MI_RE_GROUPS * 2; i++) cur[i] = -1;
    int64_t cap = 256, top = 0;
    MiReFrame* st = (MiReFrame*)malloc((size_t)cap * sizeof(MiReFrame));
    if (!st) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    int64_t pc = 0, sp = start, steps = 0;
    int result = 0;
    for (;;) {
        if (++steps > MI_RE_STEPS) { result = 0; break; }
        int ok = 1;
        MiInst ins = p->insts[pc];
        if (ins.op == 0) {                          /* Char */
            if (sp < n && in[sp] == (uint32_t)ins.a) { sp++; pc++; } else ok = 0;
        } else if (ins.op == 1) {                   /* Any */
            if (sp < n) { sp++; pc++; } else ok = 0;
        } else if (ins.op == 2) {                   /* Class */
            if (sp < n && mi_class_has(&p->cls[ins.a], in[sp])) { sp++; pc++; } else ok = 0;
        } else if (ins.op == 7) {                   /* Bol */
            if (sp == 0) pc++; else ok = 0;
        } else if (ins.op == 8) {                   /* Eol */
            if (sp == n) pc++; else ok = 0;
        } else if (ins.op == 6) {                   /* Save */
            cur[ins.a] = sp; pc++;
        } else if (ins.op == 4) {                   /* Jmp */
            pc = ins.a;
        } else if (ins.op == 5) {                   /* Split */
            if (top == cap) {
                cap *= 2;
                st = (MiReFrame*)realloc(st, (size_t)cap * sizeof(MiReFrame));
                if (!st) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
            }
            st[top].pc = ins.b; st[top].sp = sp;
            memcpy(st[top].saves, cur, sizeof cur);
            top++;
            pc = ins.a;
        } else {                                    /* Match */
            memcpy(saves, cur, sizeof cur);
            result = 1;
            break;
        }
        if (!ok) {
            if (top == 0) { result = 0; break; }
            top--;
            pc = st[top].pc; sp = st[top].sp;
            memcpy(cur, st[top].saves, sizeof cur);
        }
    }
    free(st);
    return result;
}

static int mi_re_search(const MiProg* p, const uint32_t* in, int64_t n,
                        int64_t from, int64_t* saves) {
    for (int64_t at = from; ; at++) {
        if (mi_re_match_at(p, in, n, at, saves)) return 1;
        if (at >= n) return 0;
    }
}

static void mi_re_free(MiProg* p) { free(p->insts); free(p->cls); }

/* ---- 바깥에서 부르는 여섯 가지 ---- */

static bool mi_re_test(MiStr pat, MiStr text) {
    MiProg p = mi_re_compile(pat);
    uint32_t* in; int64_t n = mi_utf8_decode(text, &in);
    int64_t sv[MI_RE_GROUPS * 2];
    int r = mi_re_search(&p, in, n, 0, sv);
    free(in); mi_re_free(&p);
    return r != 0;
}

static MiOpt_MiStr mi_re_find(MiStr pat, MiStr text) {
    MiProg p = mi_re_compile(pat);
    uint32_t* in; int64_t n = mi_utf8_decode(text, &in);
    int64_t sv[MI_RE_GROUPS * 2];
    MiOpt_MiStr out;
    if (mi_re_search(&p, in, n, 0, sv)) {
        out.has = true; out.val = mi_utf8_encode(in, sv[0], sv[1]);
    } else {
        out.has = false; out.val = mi_str("");
    }
    free(in); mi_re_free(&p);
    return out;
}

static MiList mi_re_find_all(MiStr pat, MiStr text) {
    MiProg p = mi_re_compile(pat);
    uint32_t* in; int64_t n = mi_utf8_decode(text, &in);
    int64_t sv[MI_RE_GROUPS * 2];
    MiList out = mi_list_new((int64_t)sizeof(MiStr));
    int64_t at = 0;
    while (at <= n && mi_re_search(&p, in, n, at, sv)) {
        MiStr piece = mi_utf8_encode(in, sv[0], sv[1]);
        mi_list_push(&out, &piece);
        at = sv[1] > sv[0] ? sv[1] : sv[1] + 1;
    }
    free(in); mi_re_free(&p);
    return out;
}

static MiList mi_re_groups(MiStr pat, MiStr text) {
    MiProg p = mi_re_compile(pat);
    uint32_t* in; int64_t n = mi_utf8_decode(text, &in);
    int64_t sv[MI_RE_GROUPS * 2];
    MiList out = mi_list_new((int64_t)sizeof(MiStr));
    if (mi_re_search(&p, in, n, 0, sv)) {
        for (int g = 0; g <= p.ngroups; g++) {
            MiStr piece = (sv[g * 2] < 0 || sv[g * 2 + 1] < 0)
                ? mi_str("")
                : mi_utf8_encode(in, sv[g * 2], sv[g * 2 + 1]);
            mi_list_push(&out, &piece);
        }
    }
    free(in); mi_re_free(&p);
    return out;
}

static MiStr mi_re_replace(MiStr pat, MiStr text, MiStr repl) {
    MiProg p = mi_re_compile(pat);
    uint32_t* in; int64_t n = mi_utf8_decode(text, &in);
    int64_t sv[MI_RE_GROUPS * 2];
    MiStr out = mi_str("");
    int64_t at = 0, last = 0;
    while (at <= n && mi_re_search(&p, in, n, at, sv)) {
        out = mi_cat(out, mi_utf8_encode(in, last, sv[0]));
        out = mi_cat(out, repl);
        last = sv[1];
        at = sv[1] > sv[0] ? sv[1] : sv[1] + 1;
    }
    if (last > n) last = n;
    out = mi_cat(out, mi_utf8_encode(in, last, n));
    free(in); mi_re_free(&p);
    return out;
}

static MiList mi_re_split(MiStr pat, MiStr text) {
    MiProg p = mi_re_compile(pat);
    uint32_t* in; int64_t n = mi_utf8_decode(text, &in);
    int64_t sv[MI_RE_GROUPS * 2];
    MiList out = mi_list_new((int64_t)sizeof(MiStr));
    int64_t at = 0, last = 0;
    while (at <= n && mi_re_search(&p, in, n, at, sv)) {
        if (sv[1] == sv[0]) { at = sv[0] + 1; continue; }
        MiStr piece = mi_utf8_encode(in, last, sv[0]);
        mi_list_push(&out, &piece);
        last = sv[1];
        at = sv[1];
    }
    MiStr tail = mi_utf8_encode(in, last, n);
    mi_list_push(&out, &tail);
    free(in); mi_re_free(&p);
    return out;
}
"##;

/// JSON 의 C판. `src/json.rs` 와 같은 규칙이어야 합니다.
const RUNTIME_JSON: &str = r##"
/* ---------------- JSON ---------------- */
/* kind: 0 null, 1 bool, 2 int, 3 float, 4 str, 5 list, 6 dict */
struct MiJson {
    int kind;
    bool b;
    int64_t i;
    double f;
    MiStr s;
    struct MiJson** items;
    MiStr* keys;
    int64_t n, cap;
};

static MiJson* mi_json_new(int kind) {
    MiJson* j = (MiJson*)calloc(1, sizeof(MiJson));
    if (!j) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    j->kind = kind;
    j->s = mi_str("");
    return j;
}

static void mi_json_grow(MiJson* j) {
    if (j->n < j->cap) return;
    j->cap = j->cap ? j->cap * 2 : 8;
    j->items = (MiJson**)realloc(j->items, (size_t)j->cap * sizeof(MiJson*));
    j->keys = (MiStr*)realloc(j->keys, (size_t)j->cap * sizeof(MiStr));
    if (!j->items || !j->keys) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
}

static MiJson* mi_json_null(void)         { return mi_json_new(0); }
static MiJson* mi_json_bool(bool v)       { MiJson* j = mi_json_new(1); j->b = v; return j; }
static MiJson* mi_json_int(int64_t v)     { MiJson* j = mi_json_new(2); j->i = v; return j; }
static MiJson* mi_json_float(double v)    { MiJson* j = mi_json_new(3); j->f = v; return j; }
static MiJson* mi_json_str(MiStr v)       { MiJson* j = mi_json_new(4); j->s = v; return j; }
static MiJson* mi_json_list(void)         { return mi_json_new(5); }
static MiJson* mi_json_dict(void)         { return mi_json_new(6); }

static MiStr mi_json_kind(MiJson* j) {
    switch (j->kind) {
        case 0: return mi_str("null");
        case 1: return mi_str("bool");
        case 2: return mi_str("int");
        case 3: return mi_str("float");
        case 4: return mi_str("str");
        case 5: return mi_str("list");
        default: return mi_str("dict");
    }
}

static int64_t mi_json_len(MiJson* j) { return (j->kind == 5 || j->kind == 6) ? j->n : 0; }

static MiList mi_json_keys(MiJson* j) {
    MiList out = mi_list_new((int64_t)sizeof(MiStr));
    if (j->kind == 6) for (int64_t i = 0; i < j->n; i++) mi_list_push(&out, &j->keys[i]);
    return out;
}

static void mi_json_set(MiJson* j, MiStr k, MiJson* v) {
    if (j->kind != 6) mi_panic(MI_T("set()은 JSON 객체에만 씁니다", "set() only works on JSON objects"));
    for (int64_t i = 0; i < j->n; i++) {
        if (mi_str_eq(j->keys[i], k)) { j->items[i] = v; return; }
    }
    mi_json_grow(j);
    j->keys[j->n] = k;
    j->items[j->n] = v;
    j->n++;
}

static void mi_json_push(MiJson* j, MiJson* v) {
    if (j->kind != 5) mi_panic(MI_T("push()는 JSON 배열에만 씁니다", "push() only works on JSON arrays"));
    mi_json_grow(j);
    j->keys[j->n] = mi_str("");
    j->items[j->n] = v;
    j->n++;
}

static MiOpt_MiJsonp mi_json_get(MiJson* j, MiStr k) {
    MiOpt_MiJsonp o; o.has = false; o.val = NULL;
    if (j->kind == 6) {
        for (int64_t i = 0; i < j->n; i++) {
            if (mi_str_eq(j->keys[i], k)) { o.has = true; o.val = j->items[i]; return o; }
        }
    }
    return o;
}

static MiOpt_MiJsonp mi_json_at(MiJson* j, int64_t i) {
    MiOpt_MiJsonp o; o.has = false; o.val = NULL;
    if (j->kind == 5 && i >= 0 && i < j->n) { o.has = true; o.val = j->items[i]; }
    return o;
}

static MiOpt_int64_t mi_json_as_int(MiJson* j) {
    MiOpt_int64_t o; o.has = false; o.val = 0;
    if (j->kind == 2) { o.has = true; o.val = j->i; }
    return o;
}

static MiOpt_double mi_json_as_float(MiJson* j) {
    MiOpt_double o; o.has = false; o.val = 0;
    if (j->kind == 3) { o.has = true; o.val = j->f; }
    else if (j->kind == 2) { o.has = true; o.val = (double)j->i; }
    return o;
}

static MiOpt_MiStr mi_json_as_str(MiJson* j) {
    MiOpt_MiStr o; o.has = false; o.val = mi_str("");
    if (j->kind == 4) { o.has = true; o.val = j->s; }
    return o;
}

static MiOpt_bool mi_json_as_bool(MiJson* j) {
    MiOpt_bool o; o.has = false; o.val = false;
    if (j->kind == 1) { o.has = true; o.val = j->b; }
    return o;
}

/* ---- 쓰기 ---- */
static MiStr mi_json_escape(MiStr s) {
    /* 최악의 경우 한 글자가 여섯 글자(\u00XX)가 되고, 양 끝에 따옴표가 붙습니다. */
    char* buf = mi_alloc(s.len * 6 + 2);
    int64_t k = 0;
    buf[k++] = '"';
    for (int64_t i = 0; i < s.len; i++) {
        unsigned char c = (unsigned char)s.p[i];
        if (c == '"')       { buf[k++] = '\\'; buf[k++] = '"'; }
        else if (c == '\\') { buf[k++] = '\\'; buf[k++] = '\\'; }
        else if (c == '\n') { buf[k++] = '\\'; buf[k++] = 'n'; }
        else if (c == '\t') { buf[k++] = '\\'; buf[k++] = 't'; }
        else if (c == '\r') { buf[k++] = '\\'; buf[k++] = 'r'; }
        else if (c < 0x20)  { k += snprintf(buf + k, 7, "\\u%04x", c); }
        else                { buf[k++] = (char)c; }
    }
    buf[k++] = '"';
    buf[k] = 0;
    return mi_mk(buf, k);
}

static MiStr mi_json_write(MiJson* j) {
    switch (j->kind) {
        case 0: return mi_str("null");
        case 1: return mi_str(j->b ? "true" : "false");
        case 2: return mi_from_i64(j->i);
        case 3: return mi_from_f64(j->f);
        case 4: return mi_json_escape(j->s);
        case 5: {
            MiStr out = mi_str("[");
            for (int64_t i = 0; i < j->n; i++) {
                if (i) out = mi_cat(out, mi_str(","));
                out = mi_cat(out, mi_json_write(j->items[i]));
            }
            return mi_cat(out, mi_str("]"));
        }
        default: {
            MiStr out = mi_str("{");
            for (int64_t i = 0; i < j->n; i++) {
                if (i) out = mi_cat(out, mi_str(","));
                out = mi_cat(out, mi_json_escape(j->keys[i]));
                out = mi_cat(out, mi_str(":"));
                out = mi_cat(out, mi_json_write(j->items[i]));
            }
            return mi_cat(out, mi_str("}"));
        }
    }
}

/* ---- 읽기 ---- */
typedef struct {
    const uint32_t* s; int64_t n, i;
    int failed; char msg[160];
} MiJP;

static void mi_jp_err(MiJP* q, const char* m) {
    if (!q->failed) { q->failed = 1; snprintf(q->msg, sizeof q->msg, "%s", m); }
}
static void mi_jp_errat(MiJP* q, int64_t at, const char* m) {
    if (!q->failed) { q->failed = 1; snprintf(q->msg, sizeof q->msg, MI_T("%lld번째: %s", "at %lld: %s"), (long long)at, m); }
}
static void mi_jp_ws(MiJP* q) {
    while (q->i < q->n) {
        uint32_t c = q->s[q->i];
        if (c == ' ' || c == '\t' || c == '\n' || c == '\r') q->i++; else break;
    }
}
static int mi_jp_lit(MiJP* q, const char* w) {
    int64_t k = (int64_t)strlen(w);
    if (q->i + k > q->n) return 0;
    for (int64_t i = 0; i < k; i++) if (q->s[q->i + i] != (uint32_t)(unsigned char)w[i]) return 0;
    q->i += k;
    return 1;
}

static MiJson* mi_jp_value(MiJP* q);

static MiStr mi_jp_string(MiJP* q) {
    q->i++;                                  /* 여는 따옴표 */
    uint32_t* buf = (uint32_t*)malloc((size_t)(q->n + 1) * sizeof(uint32_t));
    if (!buf) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    int64_t k = 0;
    for (;;) {
        if (q->i >= q->n) { mi_jp_err(q, MI_T("문자열이 닫히지 않았습니다", "unterminated string")); free(buf); return mi_str(""); }
        uint32_t c = q->s[q->i++];
        if (c == '"') break;
        if (c != '\\') { buf[k++] = c; continue; }
        if (q->i >= q->n) { mi_jp_err(q, MI_T("문자열이 닫히지 않았습니다", "unterminated string")); free(buf); return mi_str(""); }
        uint32_t e = q->s[q->i++];
        if (e == 'n') buf[k++] = '\n';
        else if (e == 't') buf[k++] = '\t';
        else if (e == 'r') buf[k++] = '\r';
        else if (e == 'b') buf[k++] = 8;
        else if (e == 'f') buf[k++] = 12;
        else if (e == '/') buf[k++] = '/';
        else if (e == '"') buf[k++] = '"';
        else if (e == '\\') buf[k++] = '\\';
        else if (e == 'u') {
            uint32_t v = 0;
            for (int d = 0; d < 4; d++) {
                if (q->i >= q->n) { mi_jp_err(q, MI_T("`\\u` 뒤가 모자랍니다", "incomplete `\\u` escape")); free(buf); return mi_str(""); }
                uint32_t h = q->s[q->i++];
                int dv;
                if (h >= '0' && h <= '9') dv = (int)(h - '0');
                else if (h >= 'a' && h <= 'f') dv = (int)(h - 'a') + 10;
                else if (h >= 'A' && h <= 'F') dv = (int)(h - 'A') + 10;
                else { mi_jp_err(q, MI_T("`\\u` 뒤는 16진수 네 자리입니다", "`\\u` must be followed by four hex digits")); free(buf); return mi_str(""); }
                v = v * 16 + (uint32_t)dv;
            }
            buf[k++] = v;
        } else {
            char b[64]; snprintf(b, sizeof b, MI_T("JSON에 없는 표기입니다", "invalid JSON escape"));
            mi_jp_err(q, b); free(buf); return mi_str("");
        }
    }
    MiStr out = mi_utf8_encode(buf, 0, k);
    free(buf);
    return out;
}

static MiJson* mi_jp_number(MiJP* q) {
    int64_t start = q->i;
    if (q->i < q->n && q->s[q->i] == '-') q->i++;
    int isfloat = 0;
    while (q->i < q->n) {
        uint32_t c = q->s[q->i];
        if (c >= '0' && c <= '9') q->i++;
        else if (c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-') {
            if (c == '.' || c == 'e' || c == 'E') isfloat = 1;
            q->i++;
        } else break;
    }
    if (q->i == start || (q->i - start == 1 && q->s[start] == '-')) {
        mi_jp_errat(q, start, MI_T("숫자가 아닙니다", "expected a number"));
        return mi_json_null();
    }
    MiStr t = mi_utf8_encode(q->s, start, q->i);
    const char* cs = mi_cstr(t);
    if (isfloat) return mi_json_float(strtod(cs, NULL));
    errno = 0;
    long long v = strtoll(cs, NULL, 10);
    if (errno == ERANGE) return mi_json_float(strtod(cs, NULL));
    return mi_json_int((int64_t)v);
}

static MiJson* mi_jp_object(MiJP* q) {
    q->i++;                                  /* { */
    MiJson* o = mi_json_dict();
    mi_jp_ws(q);
    if (q->i < q->n && q->s[q->i] == '}') { q->i++; return o; }
    for (;;) {
        mi_jp_ws(q);
        if (!(q->i < q->n && q->s[q->i] == '"')) {
            mi_jp_errat(q, q->i, MI_T("이름은 따옴표로 감싸야 합니다", "object keys must be quoted"));
            return o;
        }
        MiStr k = mi_jp_string(q);
        if (q->failed) return o;
        mi_jp_ws(q);
        if (!(q->i < q->n && q->s[q->i] == ':')) { mi_jp_errat(q, q->i, MI_T("`:` 가 필요합니다", "expected `:`")); return o; }
        q->i++;
        MiJson* v = mi_jp_value(q);
        if (q->failed) return o;
        mi_json_set(o, k, v);
        mi_jp_ws(q);
        if (q->i < q->n && q->s[q->i] == ',') { q->i++; continue; }
        if (q->i < q->n && q->s[q->i] == '}') { q->i++; break; }
        mi_jp_errat(q, q->i, MI_T("`,` 나 `}` 가 필요합니다", "expected `,` or `}`"));
        return o;
    }
    return o;
}

static MiJson* mi_jp_array(MiJP* q) {
    q->i++;                                  /* [ */
    MiJson* o = mi_json_list();
    mi_jp_ws(q);
    if (q->i < q->n && q->s[q->i] == ']') { q->i++; return o; }
    for (;;) {
        MiJson* v = mi_jp_value(q);
        if (q->failed) return o;
        mi_json_push(o, v);
        mi_jp_ws(q);
        if (q->i < q->n && q->s[q->i] == ',') { q->i++; continue; }
        if (q->i < q->n && q->s[q->i] == ']') { q->i++; break; }
        mi_jp_errat(q, q->i, MI_T("`,` 나 `]` 가 필요합니다", "expected `,` or `]`"));
        return o;
    }
    return o;
}

static MiJson* mi_jp_value(MiJP* q) {
    mi_jp_ws(q);
    if (q->i >= q->n) { mi_jp_err(q, MI_T("값이 오기 전에 끝났습니다", "unexpected end of input")); return mi_json_null(); }
    uint32_t c = q->s[q->i];
    if (c == '{') return mi_jp_object(q);
    if (c == '[') return mi_jp_array(q);
    if (c == '"') { MiStr s = mi_jp_string(q); return mi_json_str(s); }
    if (c == 't') {
        if (mi_jp_lit(q, "true")) return mi_json_bool(true);
        mi_jp_errat(q, q->i, MI_T("`true` 가 아닙니다", "expected `true`")); return mi_json_null();
    }
    if (c == 'f') {
        if (mi_jp_lit(q, "false")) return mi_json_bool(false);
        mi_jp_errat(q, q->i, MI_T("`false` 가 아닙니다", "expected `false`")); return mi_json_null();
    }
    if (c == 'n') {
        if (mi_jp_lit(q, "null")) return mi_json_null();
        mi_jp_errat(q, q->i, MI_T("`null` 이 아닙니다", "expected `null`")); return mi_json_null();
    }
    return mi_jp_number(q);
}

static MiRes_MiJsonp mi_json_parse(MiStr text) {
    MiRes_MiJsonp r;
    uint32_t* in; int64_t n = mi_utf8_decode(text, &in);
    MiJP q; q.s = in; q.n = n; q.i = 0; q.failed = 0; q.msg[0] = 0;
    MiJson* v = mi_jp_value(&q);
    if (!q.failed) {
        mi_jp_ws(&q);
        if (q.i < q.n) {
            char b[96];
            snprintf(b, sizeof b, MI_T("%lld번째 글자 뒤에 남는 것이 있습니다", "unexpected trailing characters after position %lld"), (long long)q.i);
            mi_jp_err(&q, b);
        }
    }
    free(in);
    if (q.failed) {
        r.ok = false; r.val = mi_json_null();
        r.err = mi_cat(mi_str("JSON: "), mi_mk(q.msg, (int64_t)strlen(q.msg)));
        return r;
    }
    r.ok = true; r.val = v; r.err = mi_str("");
    return r;
}
"##;
