/* Function pointer types shaped like Vulkan's PFN_vkCmdBuildAccelerationStructuresKHR,
   whose last parameter is an array of pointers (`const T* const*`), for tests/fnptr.skn. */
#ifndef ACCEL_H
#define ACCEL_H
#include <stdint.h>

typedef struct BuildInfo { uint32_t geometryCount; int64_t dst; } BuildInfo;
typedef struct RangeInfo { uint32_t primitiveCount; uint32_t primitiveOffset; } RangeInfo;
typedef struct Cmd_T* Cmd;

typedef int64_t (*PFN_cmdBuild)(Cmd cmd, uint32_t infoCount, const BuildInfo* pInfos,
                                const RangeInfo* const* ppRanges);
typedef void (*PFN_noArgs)(void);
/* Returns a function pointer, like PFN_vkGetInstanceProcAddr. */
typedef PFN_noArgs (*PFN_getProc)(const char* name);
/* Not importable yet, but must say why instead of disappearing. */
typedef void (*PFN_byValue)(BuildInfo info);
typedef int (*PFN_printfLike)(const char* fmt, ...);

/* Address of a function with the PFN_cmdBuild shape (as vkGetDeviceProcAddr would give). */
void* get_proc(const char* name);
/* Address of a function with the PFN_getProc shape. */
void* get_loader(void);
#endif
