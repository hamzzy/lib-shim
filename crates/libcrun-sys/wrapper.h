// Wrapper header for libcrun
// This will include the actual libcrun headers when available

#ifdef __cplusplus
extern "C" {
#endif

// Basic types and structures
// These are placeholders - in a real implementation, we'd include
// the actual libcrun headers: #include <libcrun/container.h>

typedef struct crun_container_s crun_container_t;
typedef struct crun_runtime_s crun_runtime_t;

// Function declarations (stubs for now)
// In a real implementation, these would come from libcrun headers

int crun_container_create(crun_container_t *container, const char *id);
int crun_container_start(crun_container_t *container, const char *id);
int crun_container_kill(crun_container_t *container, const char *id, int signal);
int crun_container_delete(crun_container_t *container, const char *id);
int crun_container_list(crun_container_t **containers, size_t *count);

#ifdef __cplusplus
}
#endif

