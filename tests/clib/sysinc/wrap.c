#include "wrap.h"
int wrap_version(void) { return WRAP_VERSION; }
int wrap_add(int a, int b) { return a + b; }
int wrap_scale(int a) { return a * WRAP_LIMIT; }
