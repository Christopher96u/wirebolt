#ifndef WIREBOLT_STREAM_H
#define WIREBOLT_STREAM_H

#include <stdint.h>

typedef struct wirebolt_run_session wirebolt_run_session;

typedef void (*wirebolt_on_head_fn)(void *context, const uint8_t *json, uintptr_t length);
typedef uint8_t (*wirebolt_on_chunk_fn)(void *context, const uint8_t *bytes, uintptr_t length);
typedef void (*wirebolt_on_complete_fn)(void *context, const uint8_t *json, uintptr_t length);
typedef void (*wirebolt_on_error_fn)(void *context, const uint8_t *json, uintptr_t length);

typedef struct wirebolt_run_callbacks {
    wirebolt_on_head_fn on_head;
    wirebolt_on_chunk_fn on_chunk;
    wirebolt_on_complete_fn on_complete;
    wirebolt_on_error_fn on_error;
} wirebolt_run_callbacks;

uint32_t wirebolt_stream_abi_version(void);

/*
 * Starts the shared runtime and warms the direct and system HTTP engines in
 * the background. Returns 1 when the runtime is available.
 *
 * Dropping the pooled engines after a network change is a control
 * operation and lives on the UniFFI surface as `resetHttpEngines()`.
 */
uint8_t wirebolt_runtime_warmup(void);

/*
 * Starts one request on a dedicated worker. Callback buffers are borrowed and
 * remain valid only for the duration of each callback. Callbacks are serialized
 * in head -> chunks -> complete/error order. Do not free the session from a
 * callback; schedule cleanup after the terminal callback returns.
 */
wirebolt_run_session *wirebolt_run_start(
    const uint8_t *input_json,
    uintptr_t input_length,
    wirebolt_run_callbacks callbacks,
    void *context
);

void wirebolt_run_cancel(wirebolt_run_session *session);
void wirebolt_run_free(wirebolt_run_session *session);

#endif
