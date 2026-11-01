#ifndef WIREBOLT_STREAM_H
#define WIREBOLT_STREAM_H

#include <stdint.h>

#define WIREBOLT_STREAM_ABI_VERSION 5

uint32_t wirebolt_stream_abi_version(void);
/* Starts the shared runtime and warms direct/system HTTP engines in the background. */
uint8_t wirebolt_runtime_warmup(void);

#endif
