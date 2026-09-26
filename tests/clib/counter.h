/* Siskin 시험용 작은 C 라이브러리 (import c ... also) */
#ifndef COUNTER_H
#define COUNTER_H
#include <stdint.h>
typedef struct Counter Counter;
Counter* counter_new(int64_t start);
void counter_add(Counter* c, int64_t n);
int64_t counter_get(const Counter* c);
void counter_free(Counter* c);
double scale(double x, double k);
const char* greet(void);
#endif
