#define _GNU_SOURCE
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>
#include <ucontext.h>

#define MAX_SAMPLES 100000
static uintptr_t samples[MAX_SAMPLES];
static volatile sig_atomic_t sample_count = 0;
static char output_path[1024];

static void sample_pc(int signal_number, siginfo_t *info, void *context) {
    (void)signal_number;
    (void)info;
    sig_atomic_t index = sample_count;
    if (index < MAX_SAMPLES) {
        samples[index] = (uintptr_t)((ucontext_t *)context)->uc_mcontext.pc;
        sample_count = index + 1;
    }
}

static void write_samples(void) {
    struct itimerval stopped = {0};
    setitimer(ITIMER_PROF, &stopped, NULL);
    FILE *out = fopen(output_path, "w");
    if (!out) return;
    FILE *maps = fopen("/proc/self/maps", "r");
    if (maps) {
        char line[2048];
        while (fgets(line, sizeof(line), maps)) fprintf(out, "MAP %s", line);
        fclose(maps);
    }
    for (sig_atomic_t i = 0; i < sample_count; ++i) {
        fprintf(out, "PC %lx\n", (unsigned long)samples[i]);
    }
    fclose(out);
}

__attribute__((constructor)) static void start_sampling(void) {
    const char *path = getenv("GUION_PC_SAMPLE_PATH");
    if (!path || !*path) return;
    snprintf(output_path, sizeof(output_path), "%s", path);
    struct sigaction action;
    memset(&action, 0, sizeof(action));
    action.sa_sigaction = sample_pc;
    action.sa_flags = SA_SIGINFO | SA_RESTART;
    sigemptyset(&action.sa_mask);
    if (sigaction(SIGPROF, &action, NULL) != 0) return;
    if (atexit(write_samples) != 0) return;
    struct itimerval timer = {0};
    timer.it_interval.tv_usec = 500;
    timer.it_value.tv_usec = 500;
    setitimer(ITIMER_PROF, &timer, NULL);
}
