#include <stdio.h>
#include <stdint.h>
int main(void) {
    double sum = 0.0;
    for (int64_t i = 0; i < 50000000; i++) {
        double x = (double)i * 0.0000001;
        sum += x * x - x;
    }
    printf("%g\n", sum);
    return 0;
}
