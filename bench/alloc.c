/* alloc.skn 와 같은 일을 C로. 범프 할당. */
#include <stdio.h>
#include <stdlib.h>
typedef struct Chunk { struct Chunk* next; size_t used, cap; char data[1]; } Chunk;
typedef struct { Chunk* head; } Arena;
static void* bump(Arena* a, size_t n) {
    n = (n + 15) & ~(size_t)15;
    if (!a->head || a->head->cap - a->head->used < n) {
        size_t cap = a->head ? a->head->cap * 2 : 16384;
        if (cap < n) cap = n;
        Chunk* c = (Chunk*)malloc(sizeof(Chunk) + cap);
        c->next = a->head; c->used = 0; c->cap = cap; a->head = c;
    }
    void* p = a->head->data + a->head->used; a->head->used += n; return p;
}
static void drop(Arena* a) { Chunk* c = a->head; while (c) { Chunk* n = c->next; free(c); c = n; } a->head = NULL; }
int main(void) {
    long long reqs = 2000, per = 1000, total = 0;
    for (long long r = 0; r < reqs; r++) {
        Arena a; a.head = NULL;
        for (long long i = 0; i < per; i++) {
            long long* p = (long long*)bump(&a, sizeof(long long) * 8);
            p[0] = i; p[7] = i + 1;
            total += p[0] + p[7];
        }
        drop(&a);
    }
    printf("%lld\n", total);
    return 0;
}
