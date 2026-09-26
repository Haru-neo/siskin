#include <stdlib.h>
#include "counter.h"
struct Counter { int64_t v; };
Counter* counter_new(int64_t start) { Counter* c = malloc(sizeof *c); c->v = start; return c; }
void counter_add(Counter* c, int64_t n) { c->v += n; }
int64_t counter_get(const Counter* c) { return c->v; }
void counter_free(Counter* c) { free(c); }
double scale(double x, double k) { return x * k; }
const char* greet(void) { return "hello from C"; }
