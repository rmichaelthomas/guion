#define _GNU_SOURCE
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

extern void *__libc_malloc(size_t);
extern void *__libc_calloc(size_t, size_t);
extern void *__libc_realloc(void *, size_t);
extern void __libc_free(void *);

static uint64_t malloc_calls, calloc_calls, realloc_calls, free_calls;
static uint64_t malloc_bytes, calloc_bytes, realloc_bytes;

void *malloc(size_t size) {
    __atomic_fetch_add(&malloc_calls, 1, __ATOMIC_RELAXED);
    __atomic_fetch_add(&malloc_bytes, size, __ATOMIC_RELAXED);
    return __libc_malloc(size);
}
void *calloc(size_t count, size_t size) {
    __atomic_fetch_add(&calloc_calls, 1, __ATOMIC_RELAXED);
    __atomic_fetch_add(&calloc_bytes, count * size, __ATOMIC_RELAXED);
    return __libc_calloc(count, size);
}
void *realloc(void *ptr, size_t size) {
    __atomic_fetch_add(&realloc_calls, 1, __ATOMIC_RELAXED);
    __atomic_fetch_add(&realloc_bytes, size, __ATOMIC_RELAXED);
    return __libc_realloc(ptr, size);
}
void free(void *ptr) {
    __atomic_fetch_add(&free_calls, 1, __ATOMIC_RELAXED);
    __libc_free(ptr);
}
__attribute__((destructor)) static void report_allocations(void) {
    fprintf(stderr, "GUION_ALLOC {\"malloc_calls\":%lu,\"calloc_calls\":%lu,\"realloc_calls\":%lu,\"free_calls\":%lu,\"malloc_bytes\":%lu,\"calloc_bytes\":%lu,\"realloc_bytes\":%lu}\n",
        malloc_calls, calloc_calls, realloc_calls, free_calls, malloc_bytes, calloc_bytes, realloc_bytes);
}
