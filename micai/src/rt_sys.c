/* ------- 운영체제와 이야기하는 부분: 날짜, 프로세스, 파일 폴더 (자동 생성) ------- */
#include <time.h>
#ifndef _WIN32
#include <unistd.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <dirent.h>
#include <poll.h>
#include <fcntl.h>
#include <signal.h>
#endif

/* ---- std.time ---- */
static void mi_sleep(double sec) {
    fflush(stdout);
    if (sec <= 0) return;
#ifdef _WIN32
    Sleep((DWORD)(sec * 1000.0 + 0.5));
#else
    struct timespec t;
    t.tv_sec = (time_t)sec;
    t.tv_nsec = (long)((sec - (double)t.tv_sec) * 1e9);
    while (nanosleep(&t, &t) != 0 && errno == EINTR) {}
#endif
}

/* [년, 월, 일, 시, 분, 초, 요일(1=월..7=일), UTC와의 차이(초)] */
static MiList mi_time_parts(double t, bool utc) {
    time_t tt = (time_t)floor(t);
    struct tm r;
#ifdef _WIN32
    /* 윈도우에는 tm_gmtoff 가 없어서, 현지 시각을 UTC 로 읽은 값과의 차이로 구합니다. */
    int64_t off = 0;
    if (utc) gmtime_s(&r, &tt);
    else { localtime_s(&r, &tt); struct tm c = r; off = (int64_t)(_mkgmtime(&c) - tt); }
#else
    if (utc) gmtime_r(&tt, &r); else localtime_r(&tt, &r);
    int64_t off = (int64_t)r.tm_gmtoff;
#endif
    int64_t v[8] = { r.tm_year + 1900, r.tm_mon + 1, r.tm_mday, r.tm_hour, r.tm_min, r.tm_sec,
                     r.tm_wday == 0 ? 7 : r.tm_wday, utc ? 0 : off };
    MiList l = mi_list_new((int64_t)sizeof(int64_t));
    for (int i = 0; i < 8; i++) mi_list_push(&l, &v[i]);
    return l;
}

/* 날짜 칸들을 초로. 범위를 넘는 값(13월, 32일)은 알아서 다음 달로 넘깁니다. */
static double mi_time_make(int64_t y, int64_t mo, int64_t d, int64_t h, int64_t mi, int64_t s, bool utc) {
    struct tm r;
    memset(&r, 0, sizeof r);
    r.tm_year = (int)(y - 1900); r.tm_mon = (int)(mo - 1); r.tm_mday = (int)d;
    r.tm_hour = (int)h; r.tm_min = (int)mi; r.tm_sec = (int)s; r.tm_isdst = -1;
#ifdef _WIN32
    return (double)(utc ? _mkgmtime(&r) : mktime(&r));
#else
    return (double)(utc ? timegm(&r) : mktime(&r));
#endif
}

/* ---- std.process ---- */
static MiStr mi_env_get_raw(const char* n) { const char* v = getenv(n); return v ? mi_str(v) : mi_mk(NULL, -1); }
#ifdef _WIN32
static void mi_set_env(MiStr n, MiStr v) { _putenv_s(mi_cstr(n), mi_cstr(v)); }
static MiStr mi_cwd(void) {
    wchar_t* w = _wgetcwd(NULL, 0);
    if (!w) return mi_str("");
    char* u = mi_narrow(w);
    free(w);
    return mi_str(u);
}
static MiRes_int64_t mi_set_cwd(MiStr p) {
    MiRes_int64_t r; r.val = 0; r.err = mi_str("");
    wchar_t* w = mi_wide(mi_cstr(p));
    int bad = _wchdir(w) != 0;
    free(w);
    if (bad) { r.ok = false; r.err = mi_errmsg(p, errno); return r; }
    r.ok = true; return r;
}
#else
static void mi_set_env(MiStr n, MiStr v) { setenv(mi_cstr(n), mi_cstr(v), 1); }
static MiStr mi_cwd(void) {
    char* b = mi_alloc(4096);
    if (!getcwd(b, 4096)) return mi_str("");
    return mi_str(b);
}
static MiRes_int64_t mi_set_cwd(MiStr p) {
    MiRes_int64_t r; r.val = 0; r.err = mi_str("");
    if (chdir(mi_cstr(p)) != 0) { r.ok = false; r.err = mi_errmsg(p, errno); return r; }
    r.ok = true; return r;
}
#endif

static MiStr mi_run_out_s, mi_run_err_s;

/* 버퍼에 이어 붙이기 */
typedef struct { char* p; int64_t len, cap; } MiBuf;
static void mi_buf_add(MiBuf* b, const char* s, int64_t n) {
    if (b->len + n + 1 > b->cap) {
        int64_t nc = b->cap ? b->cap * 2 : 4096;
        while (nc < b->len + n + 1) nc *= 2;
        b->p = (char*)realloc(b->p, (size_t)nc);
        if (!b->p) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
        b->cap = nc;
    }
    memcpy(b->p + b->len, s, (size_t)n);
    b->len += n;
    b->p[b->len] = 0;
}

#ifdef _WIN32
/* 명령줄 인자 하나를 윈도우 규칙대로 따옴표로 감쌉니다(Rust 의 Command 와 같은 규칙). */
static void mi_quote_arg(MiBuf* b, const char* a) {
    bool need = (*a == 0) || strpbrk(a, " \t\"") != NULL;
    if (!need) { mi_buf_add(b, a, (int64_t)strlen(a)); return; }
    mi_buf_add(b, "\"", 1);
    int64_t bs = 0;
    for (const char* c = a; ; c++) {
        if (*c == '\\') { bs++; continue; }
        if (*c == 0) { for (int64_t k = 0; k < bs; k++) mi_buf_add(b, "\\", 1); break; }
        if (*c == '"') { for (int64_t k = 0; k < bs * 2 + 1; k++) mi_buf_add(b, "\\", 1); }
        else { for (int64_t k = 0; k < bs; k++) mi_buf_add(b, "\\", 1); }
        bs = 0;
        mi_buf_add(b, c, 1);
    }
    mi_buf_add(b, "\"", 1);
}

/* 자식의 출력을 받을 임시 파일. 닫으면 저절로 지워집니다. */
static HANDLE mi_tmp_handle(void) {
    wchar_t dir[MAX_PATH + 1], name[MAX_PATH + 1];
    if (!GetTempPathW(MAX_PATH, dir) || !GetTempFileNameW(dir, L"skn", 0, name)) return INVALID_HANDLE_VALUE;
    SECURITY_ATTRIBUTES sa = { sizeof sa, NULL, TRUE };
    return CreateFileW(name, GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                       &sa, CREATE_ALWAYS, FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_DELETE_ON_CLOSE, NULL);
}

static MiStr mi_tmp_read(HANDLE h) {
    MiBuf b = {0};
    char tmp[65536];
    DWORD got = 0;
    SetFilePointer(h, 0, NULL, FILE_BEGIN);
    while (ReadFile(h, tmp, sizeof tmp, &got, NULL) && got > 0) mi_buf_add(&b, tmp, (int64_t)got);
    CloseHandle(h);
    return b.p ? mi_mk(b.p, b.len) : mi_str("");
}

/* 프로그램을 실행하고 끝날 때까지 기다립니다. 표준 출력과 표준 오류를 따로 모읍니다.
   shell 이면 `cmd /C 명령` 으로 돌립니다. 돌려주는 값은 끝난 코드. */
static int64_t mi_run(MiStr prog, MiList args, bool shell) {
    mi_run_out_s = mi_str(""); mi_run_err_s = mi_str("");
    MiBuf cl = {0};
    if (shell) {
        mi_buf_add(&cl, "cmd /C ", 7);
        mi_buf_add(&cl, prog.p, prog.len);
    } else {
        mi_quote_arg(&cl, mi_cstr(prog));
        for (int64_t i = 0; i < args.len; i++) {
            mi_buf_add(&cl, " ", 1);
            mi_quote_arg(&cl, mi_cstr(*(MiStr*)mi_list_at(&args, i)));
        }
    }
    HANDLE ho = mi_tmp_handle(), he = mi_tmp_handle();
    SECURITY_ATTRIBUTES sa = { sizeof sa, NULL, TRUE };
    HANDLE hn = CreateFileW(L"NUL", GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, &sa, OPEN_EXISTING, 0, NULL);
    if (ho == INVALID_HANDLE_VALUE || he == INVALID_HANDLE_VALUE) {
        mi_run_err_s = mi_str(MI_T("파이프를 만들 수 없습니다\n", "cannot create pipe\n"));
        return 127;
    }
    fflush(stdout); fflush(stderr);
    STARTUPINFOW si;
    PROCESS_INFORMATION pi;
    memset(&si, 0, sizeof si); memset(&pi, 0, sizeof pi);
    si.cb = sizeof si;
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = hn; si.hStdOutput = ho; si.hStdError = he;
    wchar_t* wcl = mi_wide(cl.p ? cl.p : "");
    BOOL ok = CreateProcessW(NULL, wcl, NULL, NULL, TRUE, 0, NULL, NULL, &si, &pi);
    free(wcl); free(cl.p);
    if (hn != INVALID_HANDLE_VALUE) CloseHandle(hn);
    if (!ok) {
        CloseHandle(ho); CloseHandle(he);
        mi_run_err_s = mi_cat(prog, mi_str(MI_T(": 실행할 수 없습니다\n", ": cannot execute\n")));
        return 127;
    }
    WaitForSingleObject(pi.hProcess, INFINITE);
    DWORD code = 0;
    GetExitCodeProcess(pi.hProcess, &code);
    CloseHandle(pi.hProcess); CloseHandle(pi.hThread);
    mi_run_out_s = mi_tmp_read(ho);
    mi_run_err_s = mi_tmp_read(he);
    return (int64_t)(int32_t)code;
}
#else
/* 프로그램을 실행하고 끝날 때까지 기다립니다. 표준 출력과 표준 오류를 따로 모읍니다.
   shell 이면 `sh -c 명령` 으로 돌립니다. 돌려주는 값은 끝난 코드. */
static int64_t mi_run(MiStr prog, MiList args, bool shell) {
    int po[2], pe[2];
    mi_run_out_s = mi_str(""); mi_run_err_s = mi_str("");
    if (pipe(po) != 0 || pipe(pe) != 0) { mi_run_err_s = mi_str(MI_T("파이프를 만들 수 없습니다\n", "cannot create pipe\n")); return 127; }
    fflush(stdout); fflush(stderr);
    pid_t pid = fork();
    if (pid < 0) { mi_run_err_s = mi_str(MI_T("프로세스를 만들 수 없습니다\n", "cannot create process\n")); return 127; }
    if (pid == 0) {
        int dn = open("/dev/null", O_RDONLY);
        if (dn >= 0) { dup2(dn, 0); close(dn); }
        dup2(po[1], 1); dup2(pe[1], 2);
        close(po[0]); close(po[1]); close(pe[0]); close(pe[1]);
        int64_t n = args.len;
        char** av = (char**)calloc((size_t)(n + 4), sizeof(char*));
        int k = 0;
        if (shell) { av[k++] = "sh"; av[k++] = "-c"; av[k++] = (char*)mi_cstr(prog); }
        else {
            av[k++] = (char*)mi_cstr(prog);
            for (int64_t i = 0; i < n; i++) av[k++] = (char*)mi_cstr(*(MiStr*)mi_list_at(&args, i));
        }
        av[k] = NULL;
        execvp(shell ? "sh" : av[0], av);
        const char* msg = MI_T(": 실행할 수 없습니다\n", ": cannot execute\n");
        if (write(2, prog.p, (size_t)prog.len) < 0 || write(2, msg, strlen(msg)) < 0) {}
        _exit(127);
    }
    close(po[1]); close(pe[1]);
    MiBuf bo = {0}, be = {0};
    struct pollfd fds[2] = { { po[0], POLLIN, 0 }, { pe[0], POLLIN, 0 } };
    int open_n = 2;
    char tmp[65536];
    while (open_n > 0) {
        if (poll(fds, 2, -1) < 0) { if (errno == EINTR) continue; break; }
        for (int i = 0; i < 2; i++) {
            if (fds[i].fd < 0 || !(fds[i].revents & (POLLIN | POLLHUP | POLLERR))) continue;
            ssize_t got = read(fds[i].fd, tmp, sizeof tmp);
            if (got > 0) mi_buf_add(i == 0 ? &bo : &be, tmp, (int64_t)got);
            else { close(fds[i].fd); fds[i].fd = -1; open_n--; }
        }
    }
    int st = 0;
    while (waitpid(pid, &st, 0) < 0 && errno == EINTR) {}
    if (bo.p) mi_run_out_s = mi_mk(bo.p, bo.len);
    if (be.p) mi_run_err_s = mi_mk(be.p, be.len);
    if (WIFEXITED(st)) return WEXITSTATUS(st);
    if (WIFSIGNALED(st)) return 128 + WTERMSIG(st);
    return -1;
}

#endif

/* ---- std.fs 추가분 ---- */
#ifdef _WIN32
static bool mi_is_dir(MiStr p) {
    wchar_t* w = mi_wide(mi_cstr(p));
    DWORD a = GetFileAttributesW(w);
    free(w);
    return a != INVALID_FILE_ATTRIBUTES && (a & FILE_ATTRIBUTE_DIRECTORY);
}
#else
static bool mi_is_dir(MiStr p) { struct stat s; return stat(mi_cstr(p), &s) == 0 && S_ISDIR(s.st_mode); }
#endif

static int mi_cmp_str_p(const void* a, const void* b) { return mi_str_cmp(*(const MiStr*)a, *(const MiStr*)b); }

typedef struct { bool ok; MiList val; MiStr err; } MiRes_MiList_S;
static MiRes_MiList_S mi_list_dir(MiStr p) {
    MiRes_MiList_S r; r.val = mi_list_new((int64_t)sizeof(MiStr)); r.err = mi_str("");
#ifdef _WIN32
    const char* cp = mi_cstr(p);
    wchar_t* w = mi_wide(cp);
    DWORD attr = GetFileAttributesW(w);
    if (attr == INVALID_FILE_ATTRIBUTES) {
        DWORD le = GetLastError();
        free(w);
        r.ok = false;
        r.err = mi_errmsg(p, le == ERROR_ACCESS_DENIED ? EACCES : ENOENT);
        return r;
    }
    if (!(attr & FILE_ATTRIBUTE_DIRECTORY)) { free(w); r.ok = false; r.err = mi_errmsg(p, ENOTDIR); return r; }
    size_t wl = wcslen(w);
    wchar_t* pat = (wchar_t*)malloc(sizeof(wchar_t) * (wl + 3));
    if (!pat) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
    memcpy(pat, w, sizeof(wchar_t) * wl);
    if (wl > 0 && pat[wl - 1] != L'\\' && pat[wl - 1] != L'/') pat[wl++] = L'\\';
    pat[wl++] = L'*'; pat[wl] = 0;
    free(w);
    WIN32_FIND_DATAW fd;
    HANDLE h = FindFirstFileW(pat, &fd);
    free(pat);
    if (h == INVALID_HANDLE_VALUE) {
        if (GetLastError() != ERROR_FILE_NOT_FOUND) { r.ok = false; r.err = mi_errmsg(p, EACCES); return r; }
    } else {
        do {
            if (wcscmp(fd.cFileName, L".") == 0 || wcscmp(fd.cFileName, L"..") == 0) continue;
            char* u = mi_narrow(fd.cFileName);
            char* c = mi_alloc((int64_t)strlen(u));
            strcpy(c, u);
            free(u);
            MiStr s = mi_str(c);
            mi_list_push(&r.val, &s);
        } while (FindNextFileW(h, &fd));
        FindClose(h);
    }
#else
    DIR* d = opendir(mi_cstr(p));
    if (!d) { r.ok = false; r.err = mi_errmsg(p, errno); return r; }
    struct dirent* e;
    while ((e = readdir(d)) != NULL) {
        if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
        char* c = mi_alloc((int64_t)strlen(e->d_name));
        strcpy(c, e->d_name);
        MiStr s = mi_str(c);
        mi_list_push(&r.val, &s);
    }
    closedir(d);
#endif
    /* 운영체제가 주는 순서는 제각각이라 이름순으로 맞춥니다. */
    if (r.val.len > 1) qsort(r.val.data, (size_t)r.val.len, sizeof(MiStr), mi_cmp_str_p);
    r.ok = true;
    return r;
}

/* 중간 폴더까지 만듭니다 (`mkdir -p`). 이미 있으면 괜찮습니다. */
static MiRes_int64_t mi_make_dir(MiStr p) {
    MiRes_int64_t r; r.val = 0; r.err = mi_str(""); r.ok = true;
    char* path = (char*)mi_cstr(p);
    char* cp = mi_alloc((int64_t)strlen(path));
    strcpy(cp, path);
    for (char* s = cp + 1; ; s++) {
        bool end = (*s == 0);
#ifdef _WIN32
        /* 윈도우는 `\` 도 경로 구분자이고, `C:` 같은 드라이브 이름은 만들 수 없으니 건너뜁니다. */
        if ((*s == '/' || *s == '\\' || end) && s[-1] != ':') {
            char keep = *s; *s = 0;
            wchar_t* w = mi_wide(cp);
            int bad = _wmkdir(w) != 0 && errno != EEXIST;
            free(w);
            if (bad) { r.ok = false; r.err = mi_errmsg(p, errno); return r; }
#else
        if (*s == '/' || end) {
            char keep = *s; *s = 0;
            if (mkdir(cp, 0777) != 0 && errno != EEXIST) { r.ok = false; r.err = mi_errmsg(p, errno); return r; }
#endif
            *s = keep;
            if (end) break;
        }
    }
    if (!mi_is_dir(p)) { r.ok = false; r.err = mi_errmsg(p, EEXIST); }
    return r;
}
