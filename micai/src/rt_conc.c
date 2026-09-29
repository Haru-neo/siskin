/* ---------------- Concurrency: spawn and channel ----------------
   Each task (spawn) is one OS thread. Every value passed to a task is a copy, so
   tasks never touch the same memory. They communicate through channels.
   Channel and task state are protected by a single lock (mi_sync). If every task is waiting
   on something (deadlock), nobody can wake them, so it stops with a runtime error (same rule as the interpreter). */
#ifndef _WIN32
#include <pthread.h>
#endif

struct MiTask {
    pthread_t th;
    MiClo clo;
    void (*run)(struct MiTask*);
    void* res;
    bool done;
    uint64_t seed;
};

struct MiChan {
    char* buf;
    int64_t esz, cap, len, head, bcap;
    bool closed;
};

static pthread_mutex_t mi_sync = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t mi_sync_cv = PTHREAD_COND_INITIALIZER;
static int64_t mi_live = 1, mi_blocked = 0;
/* Incremented on every wake-all. At the moment of waking, the waiting count is reset to 0 (a woken
   task that has not yet grabbed the lock could otherwise be counted as "waiting" and mistaken for a deadlock). */
static uint64_t mi_epoch = 0;

static void mi_wake_all(void) {
    mi_blocked = 0;
    mi_epoch++;
    pthread_cond_broadcast(&mi_sync_cv);
}

static void mi_deadlock(void) {
    fflush(stdout);
    fprintf(stderr, MI_T("실행 오류: %s\n", "runtime error: %s\n"),
            MI_T("교착 상태: 모든 작업이 서로를 기다리고 있습니다(통로 받기·보내기 또는 작업 기다리기)",
                 "deadlock: every task is waiting (on a channel or on another task)"));
    if (mi_line > 0) fprintf(stderr, "  --> %s:%lld\n", mi_file, (long long)mi_line);
    fprintf(stderr, "  help: %s\n",
            MI_T("보내는 쪽이 다 보낸 뒤 `ch.close()` 를 부르는지, 기다리는 작업이 실제로 끝나는지 보세요",
                 "make sure the sender calls `ch.close()` when it is done, and that the task being waited on can finish"));
    exit(1);
}

/* Called while holding mi_sync. Waits until woken once. */
static void mi_block(void) {
    uint64_t e = mi_epoch;
    mi_blocked++;
    if (mi_blocked >= mi_live) mi_deadlock();
    pthread_cond_wait(&mi_sync_cv, &mi_sync);
    if (mi_epoch == e) mi_blocked--;
}

static void* mi_task_main(void* p) {
    MiTask* t = (MiTask*)p;
    mi_rng = t->seed;
#ifdef MI_DBG_ON
    mi_dbg_task_start();
#endif
    t->run(t);
    pthread_mutex_lock(&mi_sync);
    t->done = true;
    mi_live--;
    mi_wake_all();
    pthread_mutex_unlock(&mi_sync);
    return NULL;
}

static MiTask* mi_task_spawn(MiClo clo, int64_t rsz, void (*run)(MiTask*)) {
    MiTask* t = (MiTask*)calloc(1, sizeof(MiTask));
    if (!t) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    t->clo = clo;
    t->run = run;
    t->res = mi_alloc(rsz > 0 ? rsz : 1);
    /* Random seed: drawn once from the parent (same rule as the interpreter). */
    uint64_t s = mi_next_rand();
    t->seed = s ? s : 0x853C49E6748FEA9Bull;
    pthread_mutex_lock(&mi_sync);
    mi_live++;
    pthread_mutex_unlock(&mi_sync);
    pthread_attr_t at;
    pthread_attr_init(&at);
    pthread_attr_setstacksize(&at, (size_t)64 * 1024 * 1024);
    pthread_attr_setdetachstate(&at, PTHREAD_CREATE_DETACHED);
    if (pthread_create(&t->th, &at, mi_task_main, t) != 0)
        mi_panic(MI_T("새 작업(스레드)을 시작하지 못했습니다", "could not start a new task (thread)"));
    pthread_attr_destroy(&at);
    return t;
}

static void* mi_task_wait(MiTask* t) {
    pthread_mutex_lock(&mi_sync);
    while (!t->done) mi_block();
    pthread_mutex_unlock(&mi_sync);
    return t->res;
}

static bool mi_task_done(MiTask* t) {
    pthread_mutex_lock(&mi_sync);
    bool d = t->done;
    pthread_mutex_unlock(&mi_sync);
    return d;
}

/* When main ends: wait for all tasks still running. */
static void mi_tasks_finish(void) {
    pthread_mutex_lock(&mi_sync);
    while (mi_live > 1) mi_block();
    pthread_mutex_unlock(&mi_sync);
}

static MiChan* mi_chan_new(int64_t esz, int64_t cap, bool sized) {
    if (sized && cap < 1) {
        char m[160];
        snprintf(m, sizeof m, MI_T("통로 크기는 1 이상이어야 하는데 %lld입니다", "channel size must be at least 1, got %lld"), (long long)cap);
        mi_panic(m);
    }
    MiChan* c = (MiChan*)calloc(1, sizeof(MiChan));
    if (!c) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    c->esz = esz;
    c->cap = sized ? cap : 0;
    return c;
}

static void mi_chan_send(MiChan* c, const void* v) {
    pthread_mutex_lock(&mi_sync);
    while (!c->closed && c->cap > 0 && c->len >= c->cap) mi_block();
    if (c->closed) {
        pthread_mutex_unlock(&mi_sync);
        mi_panic(MI_T("닫힌 통로에 보냈습니다", "send on a closed channel"));
    }
    if (c->len == c->bcap) {
        int64_t nb = c->bcap ? c->bcap * 2 : 8;
        char* nbuf = mi_alloc(nb * c->esz);
        for (int64_t i = 0; i < c->len; i++)
            memcpy(nbuf + i * c->esz, c->buf + ((c->head + i) % c->bcap) * c->esz, (size_t)c->esz);
        free(c->buf);
        c->buf = nbuf;
        c->bcap = nb;
        c->head = 0;
    }
    memcpy(c->buf + ((c->head + c->len) % c->bcap) * c->esz, v, (size_t)c->esz);
    c->len++;
    mi_wake_all();
    pthread_mutex_unlock(&mi_sync);
}

/* Receive one value. false if closed and empty. */
static bool mi_chan_recv(MiChan* c, void* out) {
    pthread_mutex_lock(&mi_sync);
    while (c->len == 0 && !c->closed) mi_block();
    if (c->len == 0) {
        pthread_mutex_unlock(&mi_sync);
        return false;
    }
    memcpy(out, c->buf + c->head * c->esz, (size_t)c->esz);
    c->head = (c->head + 1) % c->bcap;
    c->len--;
    mi_wake_all();
    pthread_mutex_unlock(&mi_sync);
    return true;
}

static void mi_chan_close(MiChan* c) {
    pthread_mutex_lock(&mi_sync);
    c->closed = true;
    mi_wake_all();
    pthread_mutex_unlock(&mi_sync);
}
