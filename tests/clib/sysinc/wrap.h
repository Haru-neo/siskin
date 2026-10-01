/* A library header that pulls the rest of itself in with angle brackets,
   the way Windows' stdlib.h pulls malloc from <corecrt_malloc.h>. */
#ifndef WRAP_H
#define WRAP_H
#include <wrap_impl.h>

#define WRAP_VERSION 3
int wrap_version(void);
#endif
