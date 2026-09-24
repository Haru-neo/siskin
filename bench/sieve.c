#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdbool.h>
static int64_t count_primes(int64_t limit) {
    bool* sieve = malloc((size_t)limit);
    for (int64_t i = 0; i < limit; i++) sieve[i] = true;
    int64_t count = 0;
    for (int64_t i = 2; i < limit; i++) {
        if (sieve[i]) {
            count++;
            for (int64_t j = i*i; j < limit; j += i) sieve[j] = false;
        }
    }
    return count;
}
int main(void) { printf("%lld\n", (long long)count_primes(20000000)); return 0; }
