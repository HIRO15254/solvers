/* Linux-only direct-child RSS measurement. Never creates a session/process group. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#ifndef __linux__
#error "native_rss requires Linux wait4/procfs semantics"
#endif

static int allocation_mode(const char *text) {
    char *end = NULL;
    errno = 0;
    uintmax_t amount = strtoumax(text, &end, 10);
    if (errno || !text[0] || text[0] == '-' || *end || !amount || amount > SIZE_MAX) {
        fprintf(stderr, "invalid allocation size\n");
        return 2;
    }
    long page = sysconf(_SC_PAGESIZE);
    if (page <= 0) {
        fprintf(stderr, "invalid page size\n");
        return 2;
    }
    size_t size = (size_t)amount;
    volatile unsigned char *memory = malloc(size);
    if (!memory) {
        perror("malloc");
        return 2;
    }
    for (size_t offset = 0; offset < size;) {
        memory[offset] = 1;
        if (size - offset <= (size_t)page) break;
        offset += (size_t)page;
    }
    memory[size - 1] = 1;
    struct timespec pause = { .tv_sec = 0, .tv_nsec = 100000000 };
    while (nanosleep(&pause, &pause) < 0) {
        if (errno != EINTR) {
            perror("nanosleep");
            free((void *)memory);
            return 2;
        }
    }
    free((void *)memory);
    return 0;
}

static int resident_status(long *rss, long *hwm) {
    FILE *stream = fopen("/proc/self/status", "r");
    if (!stream) return -1;
    char line[512];
    *rss = *hwm = -1;
    while (fgets(line, sizeof(line), stream)) {
        long value;
        if (sscanf(line, "VmRSS: %ld kB", &value) == 1) *rss = value;
        if (sscanf(line, "VmHWM: %ld kB", &value) == 1) *hwm = value;
    }
    int failed = ferror(stream);
    if (fclose(stream) != 0) failed = 1;
    return failed || *rss < 0 || *hwm < 0 ? -1 : 0;
}

static void json_string(FILE *stream, const char *text) {
    fputc('"', stream);
    for (const unsigned char *p = (const unsigned char *)text; *p; ++p) {
        if (*p == '"' || *p == '\\') {
            fputc('\\', stream);
            fputc(*p, stream);
        } else if (*p < 0x20) {
            fprintf(stream, "\\u%04x", (unsigned int)*p);
        } else {
            fputc(*p, stream);
        }
    }
    fputc('"', stream);
}

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "--allocate") == 0) return allocation_mode(argv[2]);
    if (argc < 5 || strcmp(argv[1], "--report") || strcmp(argv[3], "--")) {
        fprintf(stderr, "usage: native_rss --report NEW_PATH -- COMMAND [ARG...]\n"
                        "       native_rss --allocate BYTES\n");
        return 2;
    }
    int fd = open(argv[2], O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (fd < 0) {
        perror("create exclusive report");
        return 2;
    }
    FILE *report = fdopen(fd, "w");
    if (!report) {
        perror("fdopen");
        close(fd);
        return 2;
    }
    long rss, hwm;
    struct rusage self_usage, child_usage;
    struct timespec start, end;
    if (resident_status(&rss, &hwm) || getrusage(RUSAGE_SELF, &self_usage) ||
        clock_gettime(CLOCK_MONOTONIC, &start)) {
        fprintf(stderr, "cannot measure launcher before fork\n");
        fclose(report);
        return 2;
    }
    pid_t child = fork();
    if (child < 0) {
        perror("fork");
        fclose(report);
        return 2;
    }
    if (child == 0) {
        close(fd);
        execvp(argv[4], &argv[4]);
        perror("execvp");
        _exit(127);
    }
    int status;
    pid_t waited;
    do {
        waited = wait4(child, &status, 0, &child_usage);
    } while (waited < 0 && errno == EINTR);
    if (waited != child || clock_gettime(CLOCK_MONOTONIC, &end)) {
        perror("wait4/clock_gettime");
        fclose(report);
        return 2;
    }
    double elapsed = (double)(end.tv_sec - start.tv_sec) +
                     (double)(end.tv_nsec - start.tv_nsec) / 1000000000.0;
    fprintf(report, "{\"schema\":\"r1.native-rss/v1\","
                    "\"source\":\"wait4.ru_maxrss_linux_kib\",\"argv\":[");
    for (int i = 4; i < argc; ++i) {
        if (i != 4) fputc(',', report);
        json_string(report, argv[i]);
    }
    fprintf(report, "],\"child_pid\":%ld,\"exit_code\":", (long)child);
    if (WIFEXITED(status)) fprintf(report, "%d", WEXITSTATUS(status));
    else fputs("null", report);
    fprintf(report, ",\"signaled\":%s,\"term_signal\":", WIFSIGNALED(status) ? "true" : "false");
    if (WIFSIGNALED(status)) fprintf(report, "%d", WTERMSIG(status));
    else fputs("null", report);
    fprintf(report, ",\"ru_maxrss_kib\":%ld,\"elapsed_seconds\":%.9f,"
                    "\"launcher_before_fork\":{\"vmrss_kib\":%ld,\"vmhwm_kib\":%ld,"
                    "\"rusage_self_maxrss_kib\":%ld}}\n",
            child_usage.ru_maxrss, elapsed, rss, hwm, self_usage.ru_maxrss);
    int failed = ferror(report);
    if (fflush(report) || fsync(fd)) failed = 1;
    if (fclose(report)) failed = 1;
    if (failed) {
        fprintf(stderr, "cannot finalize report\n");
        return 2;
    }
    if (WIFEXITED(status)) return WEXITSTATUS(status);
    if (WIFSIGNALED(status)) return 128 + WTERMSIG(status);
    return 2;
}
