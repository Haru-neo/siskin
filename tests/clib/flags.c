#include <stdlib.h>
#include "flags.h"

static float buf[16];

int map_floats(int n, void** out) {
    for (int i = 0; i < n && i < 16; i++) buf[i] = (float)i * 0.5f;
    *out = buf;
    return 0;
}

double sum_floats(void* p, int n) {
    double s = 0;
    for (int i = 0; i < n; i++) s += ((float*)p)[i];
    return s;
}
