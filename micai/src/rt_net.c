/* ---- std.net: 인터넷 연결 (TCP, TLS, HTTP) ----
   이 조각은 프로그램이 std.net 을 쓸 때만 붙습니다.
   TLS(https)는 컴퓨터에 깔린 OpenSSL(libssl)을 실행 중에 찾아서 씁니다.
   그래서 빌드할 때 OpenSSL 헤더가 없어도 되고, https 를 안 쓰면 없어도 됩니다. */
#include <sys/socket.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <arpa/inet.h>
#include <netdb.h>
#include <dlfcn.h>
#include <strings.h>
#include <stdarg.h>
#include <signal.h>
#include <pthread.h>
#include <unistd.h>

static _Thread_local MiStr mi_net_err_s;
static double mi_net_timeout_s = 30.0;

static void mi_net_fail(const char* fmt, ...) {
    char tmp[1024];
    va_list ap; va_start(ap, fmt);
    vsnprintf(tmp, sizeof tmp, fmt, ap);
    va_end(ap);
    char* c = mi_alloc((int64_t)strlen(tmp));
    strcpy(c, tmp);
    mi_net_err_s = mi_str(c);
}
static void mi_net_ok(void) { mi_net_err_s = mi_str(""); }
static MiStr mi_net_error(void) { return mi_net_err_s.p ? mi_net_err_s : mi_str(""); }
static void mi_net_set_timeout(double s) { mi_net_timeout_s = s > 0 ? s : 0; }

/* ---- OpenSSL 을 실행 중에 불러오기 ---- */
typedef struct {
    bool tried, ok;
    const void* (*TLS_client_method)(void);
    void* (*SSL_CTX_new)(const void*);
    int (*SSL_CTX_set_default_verify_paths)(void*);
    void (*SSL_CTX_set_verify)(void*, int, void*);
    void* (*SSL_new)(void*);
    int (*SSL_set_fd)(void*, int);
    long (*SSL_ctrl)(void*, int, long, void*);
    int (*SSL_set1_host)(void*, const char*);
    int (*SSL_connect)(void*);
    int (*SSL_read)(void*, void*, int);
    int (*SSL_write)(void*, const void*, int);
    int (*SSL_shutdown)(void*);
    void (*SSL_free)(void*);
    long (*SSL_get_verify_result)(const void*);
    int (*SSL_get_error)(const void*, int);
    unsigned long (*ERR_get_error)(void);
    void (*ERR_error_string_n)(unsigned long, char*, size_t);
    const char* (*X509_verify_cert_error_string)(long);
    /* 서버 쪽(https 서버) */
    const void* (*TLS_server_method)(void);
    int (*SSL_CTX_use_certificate_chain_file)(void*, const char*);
    int (*SSL_CTX_use_PrivateKey_file)(void*, const char*, int);
    int (*SSL_CTX_check_private_key)(const void*);
    void (*SSL_CTX_free)(void*);
    int (*SSL_accept)(void*);
    void* ctx;
} MiSSL;
static MiSSL mi_ssl;

static pthread_mutex_t mi_ssl_mu = PTHREAD_MUTEX_INITIALIZER;
static bool mi_ssl_load_once(void);
/* 여러 작업이 동시에 https 를 처음 쓸 수 있어 한 번만 불러오게 자물쇠를 겁니다. */
static bool mi_ssl_load(void) {
    pthread_mutex_lock(&mi_ssl_mu);
    bool ok = mi_ssl_load_once();
    pthread_mutex_unlock(&mi_ssl_mu);
    return ok;
}
static bool mi_ssl_load_once(void) {
    if (mi_ssl.tried) return mi_ssl.ok;
    mi_ssl.tried = true;
#ifdef __APPLE__
    /* 맥의 /usr/lib/libssl.dylib 는 불러오면 프로그램을 멈춰 버리는 껍데기라서, Homebrew 의 OpenSSL 을 씁니다. */
    const char* names[] = { "/opt/homebrew/opt/openssl@3/lib/libssl.3.dylib", "/usr/local/opt/openssl@3/lib/libssl.3.dylib",
                            "/opt/homebrew/lib/libssl.3.dylib", "/usr/local/lib/libssl.3.dylib", "libssl.3.dylib", NULL };
#else
    const char* names[] = { "libssl.so.3", "libssl.so", "libssl.so.1.1", NULL };
#endif
    void* h = NULL;
    for (int i = 0; names[i] && !h; i++) h = dlopen(names[i], RTLD_NOW | RTLD_GLOBAL);
#ifdef __APPLE__
    if (!h) { mi_net_fail(MI_T("https 를 쓰려면 OpenSSL(libssl)이 필요합니다. 예: `brew install openssl@3`", "https requires OpenSSL (libssl), e.g. `brew install openssl@3`")); return false; }
#else
    if (!h) { mi_net_fail(MI_T("https 를 쓰려면 OpenSSL(libssl)이 필요합니다. 예: `apt install libssl3`", "https requires OpenSSL (libssl), e.g. `apt install libssl3`")); return false; }
#endif
#define MI_SSL_SYM(n) do { *(void**)(&mi_ssl.n) = dlsym(h, #n); if (!mi_ssl.n) { mi_net_fail(MI_T("OpenSSL 이 너무 오래되었습니다 (%s 없음). 1.1.1 이상이 필요합니다", "OpenSSL is too old (missing %s); version 1.1.1 or later is required"), #n); return false; } } while (0)
    MI_SSL_SYM(TLS_client_method); MI_SSL_SYM(SSL_CTX_new); MI_SSL_SYM(SSL_CTX_set_default_verify_paths);
    MI_SSL_SYM(SSL_CTX_set_verify); MI_SSL_SYM(SSL_new); MI_SSL_SYM(SSL_set_fd); MI_SSL_SYM(SSL_ctrl);
    MI_SSL_SYM(SSL_set1_host); MI_SSL_SYM(SSL_connect); MI_SSL_SYM(SSL_read); MI_SSL_SYM(SSL_write);
    MI_SSL_SYM(SSL_shutdown); MI_SSL_SYM(SSL_free); MI_SSL_SYM(SSL_get_verify_result); MI_SSL_SYM(SSL_get_error);
    MI_SSL_SYM(ERR_get_error); MI_SSL_SYM(ERR_error_string_n); MI_SSL_SYM(X509_verify_cert_error_string);
    MI_SSL_SYM(TLS_server_method); MI_SSL_SYM(SSL_CTX_use_certificate_chain_file); MI_SSL_SYM(SSL_CTX_use_PrivateKey_file);
    MI_SSL_SYM(SSL_CTX_check_private_key); MI_SSL_SYM(SSL_CTX_free); MI_SSL_SYM(SSL_accept);
#undef MI_SSL_SYM
    mi_ssl.ctx = mi_ssl.SSL_CTX_new(mi_ssl.TLS_client_method());
    if (!mi_ssl.ctx) { mi_net_fail(MI_T("TLS 를 준비할 수 없습니다", "cannot initialize TLS")); return false; }
    /* 운영체제의 인증서 목록을 씁니다. SSL_CERT_FILE / SSL_CERT_DIR 환경 변수도 따릅니다. */
    mi_ssl.SSL_CTX_set_default_verify_paths(mi_ssl.ctx);
    mi_ssl.SSL_CTX_set_verify(mi_ssl.ctx, 1 /* SSL_VERIFY_PEER */, NULL);
    mi_ssl.ok = true;
    return true;
}

/* ---- 연결 하나 ---- */
typedef struct {
    int fd;
    void* ssl;
    void* sctx;   /* https 서버로 연 포트면 그 인증서가 든 TLS 준비물 */
    bool used, listening;
    char* buf; int64_t blen, bcap, bpos;
    /* 여러 작업(spawn)이 같은 연결을 쓰더라도 한 번에 하나씩만 만지게 합니다. */
    pthread_mutex_t mu;
} MiConn;
/* 연결 표. 칸마다 따로 할당해서, 표가 커져도 이미 받은 연결 주소는 그대로입니다. */
static MiConn** mi_conns;
static int64_t mi_nconns;
static pthread_mutex_t mi_conn_mu = PTHREAD_MUTEX_INITIALIZER;

static int64_t mi_conn_new(int fd) {
    pthread_mutex_lock(&mi_conn_mu);
    int64_t i = 0;
    while (i < mi_nconns && mi_conns[i]->used) i++;
    if (i == mi_nconns) {
        int64_t n = mi_nconns ? mi_nconns * 2 : 8;
        mi_conns = (MiConn**)realloc(mi_conns, (size_t)n * sizeof(MiConn*));
        for (int64_t k = mi_nconns; k < n; k++) {
            mi_conns[k] = (MiConn*)calloc(1, sizeof(MiConn));
            pthread_mutex_init(&mi_conns[k]->mu, NULL);
        }
        mi_nconns = n;
    }
    MiConn* c = mi_conns[i];
    c->fd = fd; c->ssl = NULL; c->sctx = NULL; c->listening = false;
    c->buf = NULL; c->blen = c->bcap = c->bpos = 0;
    c->used = true;
    pthread_mutex_unlock(&mi_conn_mu);
    return i;
}
/* 손잡이로 연결을 찾습니다. 닫혔거나 없으면 NULL. */
static MiConn* mi_conn_at(int64_t h) {
    pthread_mutex_lock(&mi_conn_mu);
    MiConn* c = (h >= 0 && h < mi_nconns && mi_conns[h]->used) ? mi_conns[h] : NULL;
    pthread_mutex_unlock(&mi_conn_mu);
    return c;
}
static MiConn* mi_conn_get(int64_t h) {
    MiConn* c = mi_conn_at(h);
    if (!c) { mi_net_fail(MI_T("이미 닫힌 연결입니다", "connection is already closed")); return NULL; }
    return c;
}

static void mi_net_timeouts(int fd) {
    struct timeval tv;
    tv.tv_sec = (time_t)mi_net_timeout_s;
    tv.tv_usec = (suseconds_t)((mi_net_timeout_s - (double)tv.tv_sec) * 1e6);
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof tv);
}

/* 주소를 찾아서 TCP 로 붙습니다. 실패하면 -1 과 오류 글. */
static int mi_tcp_dial(const char* host, int64_t port) {
    static bool sig_done = false;
    if (!sig_done) { signal(SIGPIPE, SIG_IGN); sig_done = true; }
    if (port <= 0 || port > 65535) { mi_net_fail(MI_T("포트 번호 %lld 은(는) 쓸 수 없습니다 (1~65535)", "invalid port number %lld (must be 1-65535)"), (long long)port); return -1; }
    char ps[16]; snprintf(ps, sizeof ps, "%lld", (long long)port);
    struct addrinfo hints, *res = NULL;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    int gr = getaddrinfo(host, ps, &hints, &res);
    if (gr != 0 || !res) { mi_net_fail(MI_T("%s: 주소를 찾을 수 없습니다", "%s: cannot resolve address"), host); return -1; }
    int fd = -1; int last = 0;
    for (struct addrinfo* a = res; a; a = a->ai_next) {
        fd = socket(a->ai_family, a->ai_socktype, a->ai_protocol);
        if (fd < 0) { last = errno; continue; }
        int fl = fcntl(fd, F_GETFL, 0);
        fcntl(fd, F_SETFL, fl | O_NONBLOCK);
        int r = connect(fd, a->ai_addr, a->ai_addrlen);
        if (r != 0 && errno == EINPROGRESS) {
            struct pollfd p = { fd, POLLOUT, 0 };
            int ms = mi_net_timeout_s > 0 ? (int)(mi_net_timeout_s * 1000) : -1;
            int pr;
            do { pr = poll(&p, 1, ms); } while (pr < 0 && errno == EINTR);
            if (pr == 0) { last = ETIMEDOUT; close(fd); fd = -1; continue; }
            int e = 0; socklen_t el = sizeof e;
            getsockopt(fd, SOL_SOCKET, SO_ERROR, &e, &el);
            if (e != 0) { last = e; close(fd); fd = -1; continue; }
        } else if (r != 0) { last = errno; close(fd); fd = -1; continue; }
        fcntl(fd, F_SETFL, fl);
        break;
    }
    freeaddrinfo(res);
    if (fd < 0) {
        const char* why = last == ECONNREFUSED ? MI_T("상대가 받지 않습니다", "connection refused")
                        : last == ETIMEDOUT ? MI_T("시간이 지났습니다", "timed out")
                        : last == ENETUNREACH || last == EHOSTUNREACH ? MI_T("그곳으로 가는 길이 없습니다", "no route to host")
                        : MI_T("연결할 수 없습니다", "connection failed");
        mi_net_fail(MI_T("%s:%lld 에 연결할 수 없습니다 (%s)", "cannot connect to %s:%lld (%s)"), host, (long long)port, why);
        return -1;
    }
    int one = 1;
    setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof one);
    mi_net_timeouts(fd);
    return fd;
}

static bool mi_is_ip(const char* h) {
    unsigned char b[16];
    return inet_pton(AF_INET, h, b) == 1 || inet_pton(AF_INET6, h, b) == 1;
}

/* 이미 붙은 소켓 위에 TLS 를 올립니다. */
static bool mi_tls_start(MiConn* c, const char* host) {
    if (!mi_ssl_load()) return false;
    void* s = mi_ssl.SSL_new(mi_ssl.ctx);
    mi_ssl.SSL_set_fd(s, c->fd);
    if (!mi_is_ip(host)) {
        mi_ssl.SSL_ctrl(s, 55 /* SSL_CTRL_SET_TLSEXT_HOSTNAME */, 0, (void*)host);
        mi_ssl.SSL_set1_host(s, host);
    }
    if (mi_ssl.SSL_connect(s) != 1) {
        long v = mi_ssl.SSL_get_verify_result(s);
        if (v != 0) mi_net_fail(MI_T("%s: 인증서를 믿을 수 없습니다 (%s)", "%s: untrusted certificate (%s)"), host, mi_ssl.X509_verify_cert_error_string(v));
        else {
            char eb[256];
            snprintf(eb, sizeof eb, "%s", MI_T("알 수 없는 이유", "unknown reason"));
            unsigned long e = mi_ssl.ERR_get_error();
            if (e) mi_ssl.ERR_error_string_n(e, eb, sizeof eb);
            mi_net_fail(MI_T("%s: 보안 연결(TLS)을 맺을 수 없습니다 (%s)", "%s: TLS handshake failed (%s)"), host, eb);
        }
        mi_ssl.SSL_free(s);
        return false;
    }
    c->ssl = s;
    return true;
}

static int64_t mi_conn_write(MiConn* c, const char* p, int64_t n) {
    int64_t done = 0;
    while (done < n) {
        int64_t w;
        if (c->ssl) w = mi_ssl.SSL_write(c->ssl, p + done, (int)(n - done > (1 << 30) ? (1 << 30) : n - done));
        else w = (int64_t)send(c->fd, p + done, (size_t)(n - done), MSG_NOSIGNAL);
        if (w <= 0) {
            if (!c->ssl && w < 0 && errno == EINTR) continue;
            mi_net_fail(!c->ssl && (errno == EAGAIN || errno == EWOULDBLOCK) ? MI_T("보내다가 시간이 지났습니다", "timed out while sending") : MI_T("보내는 중에 연결이 끊겼습니다", "connection lost while sending"));
            return -1;
        }
        done += w;
    }
    return done;
}

/* 버퍼에 더 받아 둡니다. 받은 만큼(>0), 끝(0), 오류(-1). */
static int64_t mi_conn_fill(MiConn* c) {
    if (c->bpos > 0 && c->bpos == c->blen) { c->bpos = c->blen = 0; }
    if (c->blen + 65536 + 1 > c->bcap) {
        if (c->bpos > 0) { memmove(c->buf, c->buf + c->bpos, (size_t)(c->blen - c->bpos)); c->blen -= c->bpos; c->bpos = 0; }
        if (c->blen + 65536 + 1 > c->bcap) {
            int64_t nc = c->bcap ? c->bcap * 2 : 65536 * 2;
            while (nc < c->blen + 65536 + 1) nc *= 2;
            c->buf = (char*)realloc(c->buf, (size_t)nc);
            if (!c->buf) mi_panic(MI_T("메모리가 부족합니다", "out of memory"));
            c->bcap = nc;
        }
    }
    for (;;) {
        int64_t r;
        if (c->ssl) {
            r = mi_ssl.SSL_read(c->ssl, c->buf + c->blen, 65536);
            if (r <= 0) {
                int e = mi_ssl.SSL_get_error(c->ssl, (int)r);
                if (e == 6 /* SSL_ERROR_ZERO_RETURN */) return 0;
                if (e == 5 /* SSL_ERROR_SYSCALL */ && (errno == 0 || r == 0)) return 0;
                if (e == 5 && (errno == EAGAIN || errno == EWOULDBLOCK)) { mi_net_fail(MI_T("응답을 기다리다 시간이 지났습니다", "timed out waiting for a response")); return -1; }
                mi_net_fail(MI_T("받는 중에 보안 연결이 끊겼습니다", "TLS connection lost while receiving"));
                return -1;
            }
        } else {
            r = (int64_t)recv(c->fd, c->buf + c->blen, 65536, 0);
            if (r < 0) {
                if (errno == EINTR) continue;
                if (errno == EAGAIN || errno == EWOULDBLOCK) mi_net_fail(MI_T("응답을 기다리다 시간이 지났습니다", "timed out waiting for a response"));
                else if (errno == ECONNRESET) mi_net_fail(MI_T("상대가 연결을 끊었습니다", "connection reset by peer"));
                else mi_net_fail(MI_T("받는 중에 오류가 났습니다", "error while receiving"));
                return -1;
            }
            if (r == 0) return 0;
        }
        c->blen += r;
        return r;
    }
}

static MiStr mi_conn_take(MiConn* c, int64_t n) {
    char* p = mi_alloc(n);
    memcpy(p, c->buf + c->bpos, (size_t)n);
    p[n] = 0;
    c->bpos += n;
    return mi_mk(p, n);
}

/* 한 줄 (끝의 \n 포함). 끝이면 "" */
static MiStr mi_conn_line(MiConn* c, bool* failed) {
    int64_t scan = c->bpos;
    for (;;) {
        char* nl = memchr(c->buf + scan, '\n', (size_t)(c->blen - scan));
        if (c->buf && nl) return mi_conn_take(c, (int64_t)(nl - (c->buf + c->bpos)) + 1);
        scan = c->blen - c->bpos;
        int64_t r = mi_conn_fill(c);
        scan += c->bpos;
        if (r < 0) { *failed = true; return mi_str(""); }
        if (r == 0) return mi_conn_take(c, c->blen - c->bpos);
    }
}

/* ---- 프록시 (회사·학교 망에서 HTTPS_PROXY 를 쓰는 경우) ---- */
static bool mi_no_proxy(const char* host) {
    const char* np = getenv("NO_PROXY");
    if (!np) np = getenv("no_proxy");
    if (!np) return false;
    size_t hl = strlen(host);
    const char* p = np;
    while (*p) {
        while (*p == ',' || *p == ' ') p++;
        const char* e = p;
        while (*e && *e != ',') e++;
        size_t n = (size_t)(e - p);
        while (n > 0 && p[n - 1] == ' ') n--;
        if (n > 0) {
            const char* q = p; size_t qn = n;
            if (qn == 1 && q[0] == '*') return true;
            if (q[0] == '*') { q++; qn--; }
            if (q[0] == '.') { q++; qn--; }
            if (hl == qn && strncasecmp(host, q, qn) == 0) return true;
            if (hl > qn && host[hl - qn - 1] == '.' && strncasecmp(host + hl - qn, q, qn) == 0) return true;
            /* 127.0.0.0/8 같은 IPv4 범위 */
            const char* sl = memchr(q, '/', qn);
            if (sl) {
                char net[64]; size_t nn = (size_t)(sl - q);
                if (nn < sizeof net) {
                    memcpy(net, q, nn); net[nn] = 0;
                    int bits = atoi(sl + 1);
                    struct in_addr a, b;
                    if (bits >= 0 && bits <= 32 && inet_pton(AF_INET, net, &a) == 1 && inet_pton(AF_INET, host, &b) == 1) {
                        uint32_t m = bits == 0 ? 0 : htonl(0xffffffffu << (32 - bits));
                        if ((a.s_addr & m) == (b.s_addr & m)) return true;
                    }
                }
            }
        }
        p = e;
    }
    return false;
}

static const char* mi_b64(const char* s) {
    static const char* t = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    size_t n = strlen(s);
    char* o = mi_alloc((int64_t)(n * 4 / 3 + 8));
    size_t j = 0;
    for (size_t i = 0; i < n; i += 3) {
        uint32_t v = (uint32_t)(unsigned char)s[i] << 16;
        if (i + 1 < n) v |= (uint32_t)(unsigned char)s[i + 1] << 8;
        if (i + 2 < n) v |= (uint32_t)(unsigned char)s[i + 2];
        o[j++] = t[(v >> 18) & 63]; o[j++] = t[(v >> 12) & 63];
        o[j++] = i + 1 < n ? t[(v >> 6) & 63] : '=';
        o[j++] = i + 2 < n ? t[v & 63] : '=';
    }
    o[j] = 0;
    return o;
}

typedef struct { char host[256]; int64_t port; char auth[512]; bool has; } MiProxy;

static MiProxy mi_proxy_for(const char* host, bool tls) {
    MiProxy px; memset(&px, 0, sizeof px);
    const char* v = tls ? (getenv("HTTPS_PROXY") ? getenv("HTTPS_PROXY") : getenv("https_proxy"))
                        : (getenv("HTTP_PROXY") ? getenv("HTTP_PROXY") : getenv("http_proxy"));
    if (!v || !*v || mi_no_proxy(host)) return px;
    const char* p = strstr(v, "://");
    p = p ? p + 3 : v;
    const char* at = strchr(p, '@');
    const char* slash = strchr(p, '/');
    if (at && (!slash || at < slash)) {
        size_t n = (size_t)(at - p);
        char cred[400];
        if (n >= sizeof cred) n = sizeof cred - 1;
        memcpy(cred, p, n); cred[n] = 0;
        snprintf(px.auth, sizeof px.auth, "%s", mi_b64(cred));
        p = at + 1;
    }
    size_t i = 0;
    if (*p == '[') { p++; while (*p && *p != ']' && i < 255) px.host[i++] = *p++; if (*p == ']') p++; }
    else while (*p && *p != ':' && *p != '/' && i < 255) px.host[i++] = *p++;
    px.host[i] = 0;
    px.port = *p == ':' ? atoll(p + 1) : 80;
    px.has = px.host[0] != 0;
    return px;
}

/* 프록시에 CONNECT 를 보내 길을 뚫습니다. */
static bool mi_proxy_connect(MiConn* c, MiProxy* px, const char* host, int64_t port) {
    char req[1024];
    int n = snprintf(req, sizeof req, "CONNECT %s:%lld HTTP/1.1\r\nHost: %s:%lld\r\n%s%s%s\r\n",
                     host, (long long)port, host, (long long)port,
                     px->auth[0] ? "Proxy-Authorization: Basic " : "", px->auth, px->auth[0] ? "\r\n" : "");
    if (mi_conn_write(c, req, n) < 0) return false;
    bool failed = false;
    MiStr st = mi_conn_line(c, &failed);
    if (failed) return false;
    int code = 0;
    if (st.len > 12) code = atoi(st.p + 9);
    for (;;) {
        MiStr l = mi_conn_line(c, &failed);
        if (failed) return false;
        if (l.len == 0 || (l.len <= 2 && (l.p[0] == '\r' || l.p[0] == '\n'))) break;
    }
    if (code != 200) { mi_net_fail(MI_T("프록시(%s)가 %s 로 가는 길을 열어 주지 않았습니다 (%d)", "proxy (%s) refused to open a tunnel to %s (%d)"), px->host, host, code); return false; }
    return true;
}

/* 연결을 엽니다. tls 면 보안 연결. use_proxy 면 HTTPS_PROXY 를 따릅니다. */
static int64_t mi_net_dial(const char* host, int64_t port, bool tls, bool use_proxy) {
    MiProxy px; memset(&px, 0, sizeof px);
    if (use_proxy && tls) px = mi_proxy_for(host, true);
    int fd = px.has ? mi_tcp_dial(px.host, px.port) : mi_tcp_dial(host, port);
    if (fd < 0) return -1;
    int64_t h = mi_conn_new(fd);
    MiConn* c = mi_conn_at(h);
    if ((px.has && !mi_proxy_connect(c, &px, host, port)) || (tls && !mi_tls_start(c, host))) {
        close(fd); free(c->buf); c->buf = NULL;
        pthread_mutex_lock(&mi_conn_mu); c->used = false; pthread_mutex_unlock(&mi_conn_mu);
        return -1;
    }
    mi_net_ok();
    return h;
}

static int64_t mi_net_open(MiStr host, int64_t port, bool tls) {
    return mi_net_dial(mi_cstr(host), port, tls, true);
}

static int64_t mi_net_send(int64_t h, MiStr data) {
    MiConn* c = mi_conn_get(h);
    if (!c) return -1;
    pthread_mutex_lock(&c->mu);
    int64_t r = mi_conn_write(c, data.p, data.len);
    pthread_mutex_unlock(&c->mu);
    if (r < 0) return -1;
    mi_net_ok();
    return 0;
}

static MiStr mi_net_recv(int64_t h, int64_t max) {
    MiConn* c = mi_conn_get(h);
    if (!c) return mi_str("");
    pthread_mutex_lock(&c->mu);
    if (c->blen - c->bpos == 0) {
        int64_t r = mi_conn_fill(c);
        if (r < 0) { pthread_mutex_unlock(&c->mu); return mi_str(""); }
    }
    int64_t n = c->blen - c->bpos;
    if (max > 0 && n > max) n = max;
    mi_net_ok();
    MiStr out = n > 0 ? mi_conn_take(c, n) : mi_str("");
    pthread_mutex_unlock(&c->mu);
    return out;
}

static MiStr mi_net_recv_line(int64_t h) {
    MiConn* c = mi_conn_get(h);
    if (!c) return mi_str("");
    bool failed = false;
    pthread_mutex_lock(&c->mu);
    MiStr s = mi_conn_line(c, &failed);
    pthread_mutex_unlock(&c->mu);
    if (!failed) mi_net_ok();
    return s;
}

static void mi_net_close(int64_t h) {
    MiConn* c = mi_conn_at(h);
    if (!c) return;
    pthread_mutex_lock(&c->mu);
    if (c->used) {
        if (c->ssl) { mi_ssl.SSL_shutdown(c->ssl); mi_ssl.SSL_free(c->ssl); }
        if (c->sctx) mi_ssl.SSL_CTX_free(c->sctx);
        c->sctx = NULL;
        close(c->fd);
        free(c->buf);
        c->ssl = NULL; c->buf = NULL; c->blen = c->bcap = c->bpos = 0;
        pthread_mutex_lock(&mi_conn_mu);
        c->used = false;
        pthread_mutex_unlock(&mi_conn_mu);
    }
    pthread_mutex_unlock(&c->mu);
}

/* ---- 서버: 들어오는 연결 받기 ---- */
static int64_t mi_net_listen(MiStr host, int64_t port) {
    static bool sig_done = false;
    if (!sig_done) { signal(SIGPIPE, SIG_IGN); sig_done = true; }
    const char* hs = host.len ? mi_cstr(host) : NULL;
    char ps[16]; snprintf(ps, sizeof ps, "%lld", (long long)port);
    struct addrinfo hints, *res = NULL;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = hs ? AF_UNSPEC : AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_flags = AI_PASSIVE;
    if (port < 0 || port > 65535) { mi_net_fail(MI_T("포트 번호 %lld 은(는) 쓸 수 없습니다 (0~65535)", "invalid port number %lld (must be 0-65535)"), (long long)port); return -1; }
    if (getaddrinfo(hs, ps, &hints, &res) != 0 || !res) { mi_net_fail(MI_T("%s: 주소를 찾을 수 없습니다", "%s: cannot resolve address"), hs ? hs : "?"); return -1; }
    int fd = socket(res->ai_family, res->ai_socktype, res->ai_protocol);
    int one = 1;
    if (fd >= 0) setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    if (fd < 0 || bind(fd, res->ai_addr, res->ai_addrlen) != 0 || listen(fd, 64) != 0) {
        int e = errno;
        if (fd >= 0) close(fd);
        freeaddrinfo(res);
        mi_net_fail(e == EADDRINUSE ? MI_T("포트 %lld 은(는) 이미 쓰이고 있습니다", "port %lld is already in use")
                    : e == EACCES ? MI_T("포트 %lld 을(를) 열 권한이 없습니다 (1024 아래는 관리자 권한이 필요합니다)", "no permission to open port %lld (ports below 1024 require root)")
                    : MI_T("포트 %lld 을(를) 열 수 없습니다", "cannot open port %lld"), (long long)port);
        return -1;
    }
    freeaddrinfo(res);
    int64_t h = mi_conn_new(fd);
    mi_conn_at(h)->listening = true;
    mi_net_ok();
    return h;
}

/* https 서버: 인증서와 비밀 열쇠(PEM 파일)를 읽어 포트를 엽니다. */
static int64_t mi_net_listen_tls(MiStr host, int64_t port, MiStr cert, MiStr key) {
    if (!mi_ssl_load()) return -1;
    void* ctx = mi_ssl.SSL_CTX_new(mi_ssl.TLS_server_method());
    if (!ctx) { mi_net_fail(MI_T("TLS 를 준비할 수 없습니다", "cannot initialize TLS")); return -1; }
    const char* cf = mi_cstr(cert);
    const char* kf = mi_cstr(key);
    const char* how = MI_T("시험용 인증서는 이렇게 만들 수 있습니다: openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 365 -subj /CN=localhost",
                           "for testing, create one with: openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 365 -subj /CN=localhost");
    if (access(cf, R_OK) != 0) { mi_net_fail(MI_T("인증서 파일 `%s` 을(를) 읽을 수 없습니다. %s", "cannot read certificate file `%s`. %s"), cf, how); mi_ssl.SSL_CTX_free(ctx); return -1; }
    if (access(kf, R_OK) != 0) { mi_net_fail(MI_T("비밀 열쇠 파일 `%s` 을(를) 읽을 수 없습니다. %s", "cannot read private key file `%s`. %s"), kf, how); mi_ssl.SSL_CTX_free(ctx); return -1; }
    if (mi_ssl.SSL_CTX_use_certificate_chain_file(ctx, cf) != 1) {
        mi_net_fail(MI_T("`%s` 은(는) 인증서(PEM) 파일이 아닙니다", "`%s` is not a certificate (PEM) file"), cf);
        mi_ssl.SSL_CTX_free(ctx); return -1;
    }
    if (mi_ssl.SSL_CTX_use_PrivateKey_file(ctx, kf, 1 /* SSL_FILETYPE_PEM */) != 1) {
        char eb[256] = "";
        unsigned long e = mi_ssl.ERR_get_error();
        if (e) mi_ssl.ERR_error_string_n(e, eb, sizeof eb);
        while (mi_ssl.ERR_get_error()) {}
        /* OpenSSL 3 은 열쇠를 읽을 때 인증서와 짝인지도 봅니다. */
        if (strstr(eb, "mismatch")) mi_net_fail(MI_T("비밀 열쇠 `%s` 이(가) 인증서 `%s` 와 짝이 아닙니다", "private key `%s` does not match certificate `%s`"), kf, cf);
        else mi_net_fail(MI_T("`%s` 은(는) 비밀 열쇠(PEM) 파일이 아닙니다", "`%s` is not a private key (PEM) file"), kf);
        mi_ssl.SSL_CTX_free(ctx); return -1;
    }
    if (mi_ssl.SSL_CTX_check_private_key(ctx) != 1) {
        mi_net_fail(MI_T("비밀 열쇠 `%s` 이(가) 인증서 `%s` 와 짝이 아닙니다", "private key `%s` does not match certificate `%s`"), kf, cf);
        mi_ssl.SSL_CTX_free(ctx); return -1;
    }
    int64_t h = mi_net_listen(host, port);
    if (h < 0) { mi_ssl.SSL_CTX_free(ctx); return -1; }
    mi_conn_at(h)->sctx = ctx;
    return h;
}

static int64_t mi_net_accept(int64_t h) {
    MiConn* c = mi_conn_get(h);
    if (!c) return -1;
    fflush(stdout);
    for (;;) {
        int fd;
        do { fd = accept(c->fd, NULL, NULL); } while (fd < 0 && errno == EINTR);
        if (fd < 0) { mi_net_fail(MI_T("연결을 받을 수 없습니다", "cannot accept connection")); return -1; }
        mi_net_timeouts(fd);
        if (!c->sctx) { mi_net_ok(); return mi_conn_new(fd); }
        /* https 서버: 보안 연결을 맺습니다. 손님 한 명이 실패해도(가짜 손님, 인증서를 안 믿는 브라우저)
           서버는 멈추지 않고 다음 손님을 기다립니다. */
        void* s = mi_ssl.SSL_new(c->sctx);
        mi_ssl.SSL_set_fd(s, fd);
        if (mi_ssl.SSL_accept(s) != 1) {
            while (mi_ssl.ERR_get_error()) {}
            mi_ssl.SSL_free(s);
            close(fd);
            continue;
        }
        int64_t k = mi_conn_new(fd);
        mi_conn_at(k)->ssl = s;
        mi_net_ok();
        return k;
    }
}

/* 정확히 n 바이트를 받습니다(상대가 먼저 끊으면 받은 만큼). HTTP 본문 읽기에 씁니다. */
static MiStr mi_net_recv_n(int64_t h, int64_t n) {
    MiConn* c = mi_conn_get(h);
    if (!c) return mi_str("");
    if (n <= 0) { mi_net_ok(); return mi_str(""); }
    pthread_mutex_lock(&c->mu);
    while (c->blen - c->bpos < n) {
        int64_t r = mi_conn_fill(c);
        if (r < 0) { pthread_mutex_unlock(&c->mu); return mi_str(""); }
        if (r == 0) break;
    }
    int64_t have = c->blen - c->bpos;
    MiStr out = mi_conn_take(c, have < n ? have : n);
    pthread_mutex_unlock(&c->mu);
    mi_net_ok();
    return out;
}

static int64_t mi_net_byte_len(MiStr s) { return s.len; }

/* 주소의 %XX 와 + 를 원래 글자로 되돌립니다. `url_encode` 의 반대. */
static MiStr mi_net_url_decode(MiStr s) {
    char* o = mi_alloc(s.len);
    int64_t j = 0;
    for (int64_t i = 0; i < s.len; i++) {
        char ch = s.p[i];
        if (ch == '+') { o[j++] = ' '; continue; }
        if (ch == '%' && i + 2 < s.len) {
            char a = s.p[i + 1], b = s.p[i + 2];
            int hi = (a >= '0' && a <= '9') ? a - '0' : (a >= 'a' && a <= 'f') ? a - 'a' + 10 : (a >= 'A' && a <= 'F') ? a - 'A' + 10 : -1;
            int lo = (b >= '0' && b <= '9') ? b - '0' : (b >= 'a' && b <= 'f') ? b - 'a' + 10 : (b >= 'A' && b <= 'F') ? b - 'A' + 10 : -1;
            if (hi >= 0 && lo >= 0) { o[j++] = (char)(hi * 16 + lo); i += 2; continue; }
        }
        o[j++] = ch;
    }
    o[j] = 0;
    return mi_mk(o, j);
}

static int64_t mi_net_port(int64_t h) {
    MiConn* c = mi_conn_get(h);
    if (!c) return 0;
    struct sockaddr_storage a; socklen_t al = sizeof a;
    if (getsockname(c->fd, (struct sockaddr*)&a, &al) != 0) return 0;
    if (a.ss_family == AF_INET) return ntohs(((struct sockaddr_in*)&a)->sin_port);
    if (a.ss_family == AF_INET6) return ntohs(((struct sockaddr_in6*)&a)->sin6_port);
    return 0;
}

static MiStr mi_net_peer(int64_t h) {
    MiConn* c = mi_conn_get(h);
    if (!c) return mi_str("");
    struct sockaddr_storage a; socklen_t al = sizeof a;
    if (getpeername(c->fd, (struct sockaddr*)&a, &al) != 0) return mi_str("");
    char ip[64] = "";
    int port = 0;
    if (a.ss_family == AF_INET) { inet_ntop(AF_INET, &((struct sockaddr_in*)&a)->sin_addr, ip, sizeof ip); port = ntohs(((struct sockaddr_in*)&a)->sin_port); }
    else if (a.ss_family == AF_INET6) { inet_ntop(AF_INET6, &((struct sockaddr_in6*)&a)->sin6_addr, ip, sizeof ip); port = ntohs(((struct sockaddr_in6*)&a)->sin6_port); }
    char* o = mi_alloc(80);
    snprintf(o, 80, "%s:%d", ip, port);
    return mi_str(o);
}

/* ---- HTTP ---- */
typedef struct { bool tls; char host[256]; int64_t port; char* path; } MiUrl;

static bool mi_url_parse(const char* u, MiUrl* o) {
    memset(o, 0, sizeof *o);
    const char* p;
    if (strncasecmp(u, "https://", 8) == 0) { o->tls = true; p = u + 8; o->port = 443; }
    else if (strncasecmp(u, "http://", 7) == 0) { p = u + 7; o->port = 80; }
    else { mi_net_fail(MI_T("`%s`: 주소는 http:// 나 https:// 로 시작해야 합니다", "`%s`: URL must start with http:// or https://"), u); return false; }
    const char* at = strchr(p, '@');
    const char* slash = strpbrk(p, "/?#");
    if (at && (!slash || at < slash)) p = at + 1; /* 주소 속 아이디:비밀번호 는 무시 */
    size_t i = 0;
    if (*p == '[') { p++; while (*p && *p != ']' && i < 255) o->host[i++] = *p++; if (*p == ']') p++; }
    else while (*p && *p != ':' && *p != '/' && *p != '?' && *p != '#' && i < 255) o->host[i++] = *p++;
    o->host[i] = 0;
    if (i == 0) { mi_net_fail(MI_T("`%s`: 주소에 서버 이름이 없습니다", "`%s`: URL has no host name"), u); return false; }
    if (*p == ':') {
        p++;
        char* e;
        long long v = strtoll(p, &e, 10);
        if (e == p || v <= 0 || v > 65535) { mi_net_fail(MI_T("`%s`: 포트 번호가 이상합니다", "`%s`: invalid port number"), u); return false; }
        o->port = v; p = e;
    }
    const char* hash = strchr(p, '#');
    size_t n = hash ? (size_t)(hash - p) : strlen(p);
    char* path = mi_alloc((int64_t)n + 1);
    if (n == 0 || p[0] != '/') { path[0] = '/'; memcpy(path + 1, p, n); path[n + 1] = 0; }
    else { memcpy(path, p, n); path[n] = 0; }
    o->path = path;
    return true;
}

/* 마지막 http 응답은 작업(스레드)마다 따로 기억합니다. */
static _Thread_local MiList mi_http_headers_l;
static _Thread_local MiStr mi_http_body_s;
static _Thread_local bool mi_http_init_done;

static void mi_http_reset(void) {
    mi_http_headers_l = mi_list_new((int64_t)sizeof(MiStr));
    mi_http_body_s = mi_str("");
}

static MiStr mi_str_trim_c(const char* p, int64_t n) {
    while (n > 0 && (*p == ' ' || *p == '\t')) { p++; n--; }
    while (n > 0 && (p[n - 1] == ' ' || p[n - 1] == '\t' || p[n - 1] == '\r' || p[n - 1] == '\n')) n--;
    char* o = mi_alloc(n);
    memcpy(o, p, (size_t)n); o[n] = 0;
    return mi_mk(o, n);
}

/* 요청 하나를 보내고 응답을 받습니다. 넘겨주기(3xx)는 여기서 처리하지 않습니다. */
static int64_t mi_http_once(const char* method, MiUrl* u, MiList* hdrs, MiStr body, char** location) {
    *location = NULL;
    mi_http_reset();
    bool plain_proxy = false;
    MiProxy px; memset(&px, 0, sizeof px);
    if (!u->tls) { px = mi_proxy_for(u->host, false); plain_proxy = px.has; }
    int64_t h = plain_proxy ? mi_net_dial(px.host, px.port, false, false) : mi_net_dial(u->host, u->port, u->tls, true);
    if (h < 0) return -1;
    MiConn* c = mi_conn_at(h);
    MiBuf rq = {0};
    char line[2048];
    bool dflt_port = (u->tls && u->port == 443) || (!u->tls && u->port == 80);
    bool v6 = strchr(u->host, ':') != NULL;
    char hostf[300];
    snprintf(hostf, sizeof hostf, v6 ? "[%s]" : "%s", u->host);
    if (plain_proxy) {
        int n = snprintf(line, sizeof line, "%s http://%s:%lld", method, hostf, (long long)u->port);
        mi_buf_add(&rq, line, n);
    } else {
        int n = snprintf(line, sizeof line, "%s ", method);
        mi_buf_add(&rq, line, n);
    }
    mi_buf_add(&rq, u->path, (int64_t)strlen(u->path));
    mi_buf_add(&rq, " HTTP/1.1\r\n", 11);
    int n = dflt_port ? snprintf(line, sizeof line, "Host: %s\r\n", hostf)
                      : snprintf(line, sizeof line, "Host: %s:%lld\r\n", hostf, (long long)u->port);
    mi_buf_add(&rq, line, n);
    bool has_ua = false, has_accept = false, has_ctype = false;
    for (int64_t i = 0; i + 1 < hdrs->len; i += 2) {
        MiStr k = *(MiStr*)mi_list_at(hdrs, i), v = *(MiStr*)mi_list_at(hdrs, i + 1);
        if (k.len == 10 && strncasecmp(k.p, "user-agent", 10) == 0) has_ua = true;
        if (k.len == 6 && strncasecmp(k.p, "accept", 6) == 0) has_accept = true;
        if (k.len == 12 && strncasecmp(k.p, "content-type", 12) == 0) has_ctype = true;
        if ((k.len == 14 && strncasecmp(k.p, "content-length", 14) == 0) || (k.len == 4 && strncasecmp(k.p, "host", 4) == 0)
            || (k.len == 10 && strncasecmp(k.p, "connection", 10) == 0)) continue;
        mi_buf_add(&rq, k.p, k.len); mi_buf_add(&rq, ": ", 2); mi_buf_add(&rq, v.p, v.len); mi_buf_add(&rq, "\r\n", 2);
    }
    if (!has_ua) mi_buf_add(&rq, "User-Agent: siskin/0.1\r\n", 22);
    if (!has_accept) mi_buf_add(&rq, "Accept: */*\r\n", 13);
    if (plain_proxy && px.auth[0]) { n = snprintf(line, sizeof line, "Proxy-Authorization: Basic %s\r\n", px.auth); mi_buf_add(&rq, line, n); }
    bool send_body = body.len > 0 || strcmp(method, "POST") == 0 || strcmp(method, "PUT") == 0 || strcmp(method, "PATCH") == 0;
    if (send_body) {
        if (!has_ctype && body.len > 0) {
            const char* ct = (body.p[0] == '{' || body.p[0] == '[') ? "application/json" : "text/plain; charset=utf-8";
            n = snprintf(line, sizeof line, "Content-Type: %s\r\n", ct);
            mi_buf_add(&rq, line, n);
        }
        n = snprintf(line, sizeof line, "Content-Length: %lld\r\n", (long long)body.len);
        mi_buf_add(&rq, line, n);
    }
    mi_buf_add(&rq, "Accept-Encoding: identity\r\nConnection: close\r\n\r\n", 48);
    if (send_body && body.len) mi_buf_add(&rq, body.p, body.len);
    int64_t w = mi_conn_write(c, rq.p, rq.len);
    free(rq.p);
    if (w < 0) { mi_net_close(h); return -1; }

    bool failed = false;
    MiStr st = mi_conn_line(c, &failed);
    if (failed) { mi_net_close(h); return -1; }
    if (st.len < 12 || strncmp(st.p, "HTTP/", 5) != 0) { mi_net_close(h); mi_net_fail(MI_T("%s: HTTP 응답이 아닙니다", "%s: not an HTTP response"), u->host); return -1; }
    const char* sp = memchr(st.p, ' ', (size_t)st.len);
    int64_t code = sp ? atoll(sp + 1) : 0;
    /* 100 Continue 같은 중간 응답은 건너뜁니다. */
    int64_t clen = -1;
    bool chunked = false;
    for (;;) {
        MiStr l = mi_conn_line(c, &failed);
        if (failed) { mi_net_close(h); return -1; }
        if (l.len == 0 || (l.len <= 2 && (l.p[0] == '\r' || l.p[0] == '\n'))) {
            if (code >= 100 && code < 200) {
                st = mi_conn_line(c, &failed);
                if (failed) { mi_net_close(h); return -1; }
                sp = memchr(st.p, ' ', (size_t)st.len);
                code = sp ? atoll(sp + 1) : 0;
                mi_http_reset();
                continue;
            }
            break;
        }
        const char* colon = memchr(l.p, ':', (size_t)l.len);
        if (!colon) continue;
        int64_t kn = (int64_t)(colon - l.p);
        char* k = mi_alloc(kn);
        for (int64_t i = 0; i < kn; i++) k[i] = (char)tolower((unsigned char)l.p[i]);
        k[kn] = 0;
        MiStr ks = mi_mk(k, kn);
        MiStr vs = mi_str_trim_c(colon + 1, l.len - kn - 1);
        mi_list_push(&mi_http_headers_l, &ks);
        mi_list_push(&mi_http_headers_l, &vs);
        if (strcmp(k, "content-length") == 0) clen = atoll(vs.p);
        if (strcmp(k, "transfer-encoding") == 0 && strstr(vs.p, "chunked")) chunked = true;
        if (strcmp(k, "location") == 0) *location = (char*)vs.p;
    }
    bool no_body = strcmp(method, "HEAD") == 0 || code == 204 || code == 304;
    MiBuf bb = {0};
    if (!no_body) {
        if (chunked) {
            for (;;) {
                MiStr sz = mi_conn_line(c, &failed);
                if (failed) { mi_net_close(h); free(bb.p); return -1; }
                if (sz.len == 0) break;
                int64_t k = (int64_t)strtoll(sz.p, NULL, 16);
                if (k <= 0) {
                    for (;;) { MiStr t = mi_conn_line(c, &failed); if (failed || t.len <= 2) break; }
                    break;
                }
                while (c->blen - c->bpos < k + 2) {
                    int64_t r = mi_conn_fill(c);
                    if (r < 0) { mi_net_close(h); free(bb.p); return -1; }
                    if (r == 0) break;
                }
                int64_t have = c->blen - c->bpos;
                int64_t take = have < k ? have : k;
                mi_buf_add(&bb, c->buf + c->bpos, take);
                c->bpos += take;
                if (take < k) break;
                if (c->blen - c->bpos >= 2) c->bpos += 2;
            }
        } else {
            for (;;) {
                int64_t have = c->blen - c->bpos;
                if (clen >= 0 && have >= clen) break;
                int64_t r = mi_conn_fill(c);
                if (r < 0) { mi_net_close(h); free(bb.p); return -1; }
                if (r == 0) {
                    if (clen >= 0 && c->blen - c->bpos < clen) { mi_net_close(h); mi_net_fail(MI_T("%s: 응답이 중간에 끊겼습니다", "%s: response was cut off"), u->host); return -1; }
                    break;
                }
            }
            int64_t have = c->blen - c->bpos;
            if (clen >= 0 && have > clen) have = clen;
            if (have > 0) mi_buf_add(&bb, c->buf + c->bpos, have);
        }
    }
    mi_net_close(h);
    if (bb.p) mi_http_body_s = mi_mk(bb.p, bb.len);
    mi_net_ok();
    return code;
}

/* 넘겨받은 주소(Location)를 지금 주소 기준으로 풉니다. */
static char* mi_url_join(MiUrl* base, const char* loc) {
    if (strncasecmp(loc, "http://", 7) == 0 || strncasecmp(loc, "https://", 8) == 0) {
        char* o = mi_alloc((int64_t)strlen(loc)); strcpy(o, loc); return o;
    }
    size_t cap = strlen(loc) + strlen(base->path) + 600;
    char* o = mi_alloc((int64_t)cap);
    bool v6 = strchr(base->host, ':') != NULL;
    int n = snprintf(o, cap, v6 ? "%s://[%s]:%lld" : "%s://%s:%lld", base->tls ? "https" : "http", base->host, (long long)base->port);
    if (loc[0] == '/' && loc[1] == '/') { snprintf(o, cap, "%s:%s", base->tls ? "https" : "http", loc); return o; }
    if (loc[0] == '/') { snprintf(o + n, cap - (size_t)n, "%s", loc); return o; }
    const char* q = strchr(base->path, '?');
    size_t plen = q ? (size_t)(q - base->path) : strlen(base->path);
    while (plen > 0 && base->path[plen - 1] != '/') plen--;
    snprintf(o + n, cap - (size_t)n, "%.*s%s", (int)plen, base->path, loc);
    return o;
}

static int64_t mi_http(MiStr method, MiStr url, MiList headers, MiStr body) {
    if (!mi_http_init_done) { mi_http_reset(); mi_http_init_done = true; }
    char* m = mi_alloc(method.len);
    for (int64_t i = 0; i < method.len; i++) m[i] = (char)toupper((unsigned char)method.p[i]);
    m[method.len] = 0;
    const char* cur = mi_cstr(url);
    for (int hop = 0; hop < 10; hop++) {
        MiUrl u;
        if (!mi_url_parse(cur, &u)) { mi_http_reset(); return -1; }
        char* loc = NULL;
        int64_t code = mi_http_once(m, &u, &headers, body, &loc);
        if (code < 0) { mi_http_reset(); return -1; }
        bool redirect = (code == 301 || code == 302 || code == 303 || code == 307 || code == 308) && loc && *loc;
        if (!redirect) return code;
        if (code == 303 || ((code == 301 || code == 302) && strcmp(m, "POST") == 0)) { m = "GET"; body = mi_str(""); }
        /* 다른 서버로 넘어가면 비밀 헤더(Authorization, Cookie)는 따라가지 않습니다. */
        char* next = mi_url_join(&u, loc);
        MiUrl nu;
        if (mi_url_parse(next, &nu) && (strcasecmp(nu.host, u.host) != 0)) {
            MiList kept = mi_list_new((int64_t)sizeof(MiStr));
            for (int64_t i = 0; i + 1 < headers.len; i += 2) {
                MiStr k = *(MiStr*)mi_list_at(&headers, i);
                if ((k.len == 13 && strncasecmp(k.p, "authorization", 13) == 0) || (k.len == 6 && strncasecmp(k.p, "cookie", 6) == 0)) continue;
                mi_list_push(&kept, mi_list_at(&headers, i));
                mi_list_push(&kept, mi_list_at(&headers, i + 1));
            }
            headers = kept;
        }
        cur = next;
    }
    mi_http_reset();
    mi_net_fail(MI_T("`%s`: 다른 주소로 넘기기가 너무 많습니다 (10번 넘음)", "`%s`: too many redirects (more than 10)"), mi_cstr(url));
    return -1;
}

static MiList mi_http_headers(void) {
    if (!mi_http_init_done) { mi_http_reset(); mi_http_init_done = true; }
    return mi_http_headers_l;
}
static MiStr mi_http_body(void) { return mi_http_body_s.p ? mi_http_body_s : mi_str(""); }

/* 주소에 넣을 수 있게 글자를 바꿉니다 (`a b&c` → `a%20b%26c`). */
static MiStr mi_url_encode(MiStr s) {
    char* o = mi_alloc(s.len * 3);
    int64_t j = 0;
    for (int64_t i = 0; i < s.len; i++) {
        unsigned char ch = (unsigned char)s.p[i];
        if (isalnum(ch) || ch == '-' || ch == '_' || ch == '.' || ch == '~') o[j++] = (char)ch;
        else { static const char* hx = "0123456789ABCDEF"; o[j++] = '%'; o[j++] = hx[ch >> 4]; o[j++] = hx[ch & 15]; }
    }
    o[j] = 0;
    return mi_mk(o, j);
}
