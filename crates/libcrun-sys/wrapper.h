// Wrapper header for libcrun
// This will include the actual libcrun headers when available

#ifdef __cplusplus
extern "C" {
#endif

// Try to include actual libcrun headers if available
#if __has_include(<libcrun/container.h>)
#include <libcrun/container.h>
#include <libcrun/context.h>
#include <libcrun/error.h>
#else
// Fallback: forward declarations when headers not available
// These match the actual libcrun API structure

typedef struct libcrun_container_s libcrun_container_t;
typedef struct libcrun_context_s libcrun_context_t;
typedef struct libcrun_error_s libcrun_error_t;

// Error handling
void libcrun_error_release(libcrun_error_t **err);

// Container operations
libcrun_container_t* libcrun_container_load_from_memory(
    const char *config_json,
    libcrun_error_t **err
);

int libcrun_container_create(
    libcrun_context_t *context,
    libcrun_container_t *container,
    const char *id,
    libcrun_error_t **err
);

int libcrun_container_start(
    libcrun_context_t *context,
    libcrun_container_t *container,
    const char *id,
    libcrun_error_t **err
);

int libcrun_container_kill(
    libcrun_context_t *context,
    libcrun_container_t *container,
    const char *id,
    int signal,
    libcrun_error_t **err
);

int libcrun_container_delete(
    libcrun_context_t *context,
    libcrun_container_t *container,
    const char *id,
    libcrun_error_t **err
);

int libcrun_container_state(
    libcrun_context_t *context,
    libcrun_container_t *container,
    const char *id,
    libcrun_error_t **err
);

void libcrun_container_free(libcrun_container_t *container);

// Context operations
libcrun_context_t* libcrun_context_new(libcrun_error_t **err);
void libcrun_context_free(libcrun_context_t *context);

#endif

#ifdef __cplusplus
}
#endif

