/* ---------------- siskin debug (네이티브) ----------------
   C 라이브러리·std.net·spawn 을 쓰는 프로그램도 한 줄씩 따라가려고, `siskin debug` 는
   프로그램을 이 부분을 넣어 컴파일한 뒤 자식 프로세스로 돌립니다.
   문장마다 MI_DBG(줄, 변수들) 이 "여기서 멈출까" 를 묻고, 멈추면 지금 줄·호출 경로·
   변수 값(글자로)을 파이프로 `siskin debug` 에 보내고 다음 명령을 기다립니다.
   화면에 보이는 명령 처리(b, p, l …)는 모두 `siskin debug` 쪽이 합니다.

   주고받는 말 (한 줄씩):
     프로그램 → siskin:  STOP 줄 깊이 작업번호 / F 함수\t줄 / V 이름\t값 / .
     siskin → 프로그램:  B 줄 줄 ... (멈출 곳 전체) / G 방식 깊이 (s n o c) / Q */
#include <pthread.h>

typedef struct { const char* name; void* p; MiStr (*repr)(void*); } MiDbgVar;
typedef struct { const char* fn; int64_t line; } MiDbgFrame;

#define MI_DBG_MAXF 4096
static FILE* mi_dbg_inf = NULL;
static FILE* mi_dbg_outf = NULL;
static pthread_mutex_t mi_dbg_mu = PTHREAD_MUTEX_INITIALIZER;
static volatile int mi_dbg_nbp = 0;
static int64_t mi_dbg_bps[1024];
static volatile int mi_dbg_mode = 's';
static volatile int64_t mi_dbg_mdepth = 0;
static volatile int mi_dbg_owner = 0;
static int mi_dbg_ntasks = 0;
static _Thread_local MiDbgFrame mi_dbg_fr[MI_DBG_MAXF];
static _Thread_local int mi_dbg_depth = 0;
static _Thread_local int mi_dbg_task = 0;

static void mi_dbg_init(void) {
    const char* e = getenv("SISKIN_DBG_FDS");
    int a, b;
    if (!e || sscanf(e, "%d,%d", &a, &b) != 2) return;
    mi_dbg_inf = fdopen(a, "r");
    mi_dbg_outf = fdopen(b, "w");
    if (!mi_dbg_inf || !mi_dbg_outf) mi_dbg_inf = mi_dbg_outf = NULL;
}

static int mi_dbg_enter(const char* fn) {
    if (mi_dbg_depth < MI_DBG_MAXF) {
        mi_dbg_fr[mi_dbg_depth].fn = fn;
        mi_dbg_fr[mi_dbg_depth].line = 0;
    }
    return ++mi_dbg_depth;
}

static void mi_dbg_leave(int* d) { mi_dbg_depth = *d - 1; }

#define MI_DBG_FN(name) int _mi_dbg_g __attribute__((cleanup(mi_dbg_leave))) = mi_dbg_enter(name)

static void mi_dbg_task_start(void) {
    mi_dbg_task = __atomic_add_fetch(&mi_dbg_ntasks, 1, __ATOMIC_SEQ_CST);
    mi_dbg_depth = 0;
}

static inline bool mi_dbg_hit(int64_t line) {
    if (mi_dbg_depth > 0 && mi_dbg_depth <= MI_DBG_MAXF) mi_dbg_fr[mi_dbg_depth - 1].line = line;
    if (!mi_dbg_outf) return false;
    for (int i = 0; i < mi_dbg_nbp; i++)
        if (mi_dbg_bps[i] == line) return true;
    if (mi_dbg_task != mi_dbg_owner) return false;
    switch (mi_dbg_mode) {
    case 's': return true;
    case 'n': return mi_dbg_depth <= mi_dbg_mdepth;
    case 'o': return mi_dbg_depth < mi_dbg_mdepth;
    default: return false;
    }
}

static void mi_dbg_put(const char* p, int64_t n) {
    for (int64_t i = 0; i < n; i++) {
        char c = p[i];
        if (c == '\\') fputs("\\\\", mi_dbg_outf);
        else if (c == '\n') fputs("\\n", mi_dbg_outf);
        else if (c == '\t') fputs("\\t", mi_dbg_outf);
        else fputc(c, mi_dbg_outf);
    }
}

static void mi_dbg_stop(int64_t line, MiDbgVar* vs, int n) {
    fflush(stdout);
    pthread_mutex_lock(&mi_dbg_mu);
    if (!mi_dbg_outf) {
        pthread_mutex_unlock(&mi_dbg_mu);
        return;
    }
    fprintf(mi_dbg_outf, "STOP %lld %d %d\n", (long long)line, mi_dbg_depth, mi_dbg_task);
    int top = mi_dbg_depth < MI_DBG_MAXF ? mi_dbg_depth : MI_DBG_MAXF;
    for (int i = 0; i < top; i++)
        fprintf(mi_dbg_outf, "F %s\t%lld\n", mi_dbg_fr[i].fn, (long long)mi_dbg_fr[i].line);
    for (int i = 0; i < n; i++) {
        MiStr r = vs[i].repr(vs[i].p);
        fprintf(mi_dbg_outf, "V %s\t", vs[i].name);
        mi_dbg_put(r.p, r.len);
        fputc('\n', mi_dbg_outf);
    }
    fputs(".\n", mi_dbg_outf);
    fflush(mi_dbg_outf);
    char buf[16384];
    for (;;) {
        if (!fgets(buf, sizeof buf, mi_dbg_inf)) {
            /* siskin debug 가 먼저 끝났습니다. 멈추지 않고 끝까지 돕니다. */
            mi_dbg_outf = NULL;
            break;
        }
        if (buf[0] == 'Q') exit(0);
        if (buf[0] == 'B') {
            int k = 0;
            char* s = buf + 1;
            while (k < 1024) {
                char* end;
                long long v = strtoll(s, &end, 10);
                if (end == s) break;
                mi_dbg_bps[k++] = v;
                s = end;
            }
            mi_dbg_nbp = k;
            continue;
        }
        if (buf[0] == 'G') {
            char m = 'c';
            long long d = 0;
            sscanf(buf + 1, " %c %lld", &m, &d);
            mi_dbg_mode = m;
            mi_dbg_mdepth = d;
            mi_dbg_owner = mi_dbg_task;
            break;
        }
    }
    pthread_mutex_unlock(&mi_dbg_mu);
}

/* 멈출 때만 변수 목록을 만듭니다. */
#define MI_DBG(line, ...) do { if (mi_dbg_hit(line)) { MiDbgVar _mi_dv[] = { { "", NULL, NULL }, __VA_ARGS__ }; \
    mi_dbg_stop(line, _mi_dv + 1, (int)(sizeof _mi_dv / sizeof _mi_dv[0]) - 1); } } while (0)
