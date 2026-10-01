#include <string.h>
#include "accel.h"

/* Sums dst + primitiveCount * 10 + primitiveOffset over every range of every info. */
static int64_t cmd_build(Cmd cmd, uint32_t infoCount, const BuildInfo* pInfos, const RangeInfo* const* ppRanges) {
    int64_t total = cmd ? 1000 : 0;
    for (uint32_t i = 0; i < infoCount; i++) {
        total += pInfos[i].dst;
        for (uint32_t g = 0; g < pInfos[i].geometryCount; g++)
            total += ppRanges[i][g].primitiveCount * 10 + ppRanges[i][g].primitiveOffset;
    }
    return total;
}

void* get_proc(const char* name) {
    if (strcmp(name, "cmdBuild") == 0) return (void*)cmd_build;
    return 0;
}

static PFN_noArgs lookup(const char* name) { return (PFN_noArgs)get_proc(name); }
void* get_loader(void) { return (void*)lookup; }
