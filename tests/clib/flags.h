/* `static const` values (the way Vulkan 1.3 declares 64-bit flags) and a C buffer read
   through `cast[*T](addr)`, for tests/cconst.skn. */
#ifndef FLAGS_H
#define FLAGS_H
#include <stdint.h>

typedef uint64_t Flags64;
typedef Flags64 StageFlags2;
enum { BASE_SHIFT = 3 };
static const StageFlags2 STAGE_2_NONE = 0ULL;
static const StageFlags2 STAGE_2_TOP_OF_PIPE_BIT = 0x00000001ULL;
static const StageFlags2 STAGE_2_COPY_BIT = 0x100000000ULL;
static const StageFlags2 STAGE_2_HIGH_BIT = 0x8000000000000000ULL;
static const StageFlags2 STAGE_2_MIXED = (1ULL << 40) | STAGE_2_TOP_OF_PIPE_BIT;
static const int SHIFTED = -(1 << BASE_SHIFT) * 2;
static const uint32_t ALL_U32 = (uint32_t)-1;
static const double RATIO = 1.0 / 4;
static const float HALF = 0.5f;
static const char* const LABEL = "flags";

/* Fills a C-owned float buffer with i * 0.5 and writes its address into *out. */
int map_floats(int n, void** out);
/* Sums n floats at p (memory written from Siskin). */
double sum_floats(void* p, int n);
#endif
