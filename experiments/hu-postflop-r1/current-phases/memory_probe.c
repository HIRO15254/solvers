/* Linux-only calibration for clear_refs=5; run later, never part of solver. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

struct snapshot { long rss, hwm; };
static struct snapshot status(void) {
    FILE *f = fopen("/proc/self/status", "r");
    if (!f) { perror("status"); exit(2); }
    char line[512]; struct snapshot s = {-1, -1};
    while (fgets(line, sizeof line, f)) {
        if (sscanf(line, "VmRSS: %ld kB", &s.rss) == 1) continue;
        if (sscanf(line, "VmHWM: %ld kB", &s.hwm) == 1) continue;
    }
    if (ferror(f) || fclose(f) || s.rss <= 0 || s.hwm < s.rss) {
        fprintf(stderr, "invalid proc snapshot\n"); exit(2);
    }
    return s;
}
static struct snapshot reset(void) {
    int fd = open("/proc/self/clear_refs", O_WRONLY | O_CLOEXEC);
    if (fd < 0) { perror("clear_refs open"); exit(2); }
    if (write(fd, "5\n", 2) != 2 || close(fd)) { perror("clear_refs write"); exit(2); }
    return status();
}
static void *touch(size_t n) {
    unsigned char *p = mmap(NULL, n, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p == MAP_FAILED) { perror("mmap"); exit(2); }
    long page = sysconf(_SC_PAGESIZE);
    if (page <= 0) { fprintf(stderr, "invalid page size\n"); exit(2); }
    for (size_t i = 0; i < n; i += (size_t)page) ((volatile unsigned char *)p)[i] = 1;
    ((volatile unsigned char *)p)[n - 1] = 1;
    return p;
}
static void release(void *p, size_t n) {
    if (munmap(p, n)) { perror("munmap"); exit(2); }
}
int main(int argc, char **argv) {
    if (argc != 2) { fprintf(stderr, "usage: memory-probe NEW-REPORT.json\n"); return 2; }
    int fd = open(argv[1], O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (fd < 0) { perror("new report"); return 2; }
    FILE *out = fdopen(fd, "w");
    if (!out) { perror("fdopen"); close(fd); return 2; }
    const size_t mib = 1024 * 1024;
    void *prior = touch(128 * mib);
    struct snapshot history = status();
    release(prior, 128 * mib);
    struct snapshot after_release = status(), low_start = reset();
    void *small = touch(mib);
    struct snapshot low_end = status();
    release(small, mib);
    struct snapshot large_start = reset();
    void *large = touch(64 * mib);
    struct snapshot large_end = status();
    release(large, 64 * mib);
    struct snapshot final = reset();
    struct snapshot values[] = {history, after_release, low_start, low_end, large_start, large_end, final};
    const char *names[] = {"history128", "after_release", "small_start", "small_end", "large_start", "large_end", "final_reset"};
    int passed = history.hwm >= 112 * 1024 && history.hwm - low_start.hwm >= 96 * 1024
        && low_end.hwm <= low_start.hwm + 8 * 1024
        && large_end.hwm >= large_start.hwm + 48 * 1024
        && large_end.hwm - low_end.hwm >= 32 * 1024
        && large_end.hwm - final.hwm >= 32 * 1024;
    fprintf(out, "{\"schema\":\"r1.current-phases-memory-calibration/v1\",\"passed\":%s,\"reset_value\":5,\"snapshots\":{", passed ? "true" : "false");
    for (size_t i = 0; i < sizeof values / sizeof values[0]; ++i)
        fprintf(out, "%s\"%s\":{\"rss_kib\":%ld,\"hwm_kib\":%ld}", i ? "," : "", names[i], values[i].rss, values[i].hwm);
    fputs("}}\n", out);
    int failed = ferror(out);
    if (fflush(out) || fsync(fd)) failed = 1;
    if (fclose(out)) failed = 1;
    return failed ? 2 : (passed ? 0 : 1);
}
