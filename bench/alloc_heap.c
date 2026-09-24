/* alloc_heap.skn 와 같은 일을 C로. malloc/free. */
#include <stdio.h>
#include <stdlib.h>
int main(void) {
    long long reqs = 2000, per = 1000, total = 0;
    for (long long r = 0; r < reqs; r++) {
        for (long long i = 0; i < per; i++) {
            long long* p = (long long*)calloc(8, sizeof(long long));
            p[0] = i; p[7] = i + 1;
            total += p[0] + p[7];
            free(p);
        }
    }
    printf("%lld\n", total);
    return 0;
}
