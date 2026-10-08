// Simple multi-threaded memory bandwidth probe (STREAM-like): read-only sum, scale-in-place (read+write like
// regret/strategy updates) and triad, over 3 x 1 GiB float arrays. Prints GB/s counting bytes actually moved
// (write-allocate traffic not counted).
#include <omp.h>
#include <stdio.h>
#include <stdlib.h>

#define N (256L * 1024 * 1024)

static double now(void) { return omp_get_wtime(); }

int main(void) {
    float *a = aligned_alloc(64, N * sizeof(float));
    float *b = aligned_alloc(64, N * sizeof(float));
    float *c = aligned_alloc(64, N * sizeof(float));
#pragma omp parallel for schedule(static)
    for (long i = 0; i < N; i++) { a[i] = 1.0f; b[i] = 2.0f; c[i] = 0.5f; }
    double best_read = 1e9, best_rmw = 1e9, best_triad = 1e9;
    volatile float sink = 0;
    for (int rep = 0; rep < 6; rep++) {
        double t = now();
        float s = 0;
#pragma omp parallel for schedule(static) reduction(+:s)
        for (long i = 0; i < N; i++) s += a[i];
        t = now() - t; sink += s; if (t < best_read) best_read = t;
        t = now();
#pragma omp parallel for schedule(static)
        for (long i = 0; i < N; i++) b[i] = b[i] * 0.999f + c[i];
        t = now() - t; if (t < best_rmw) best_rmw = t;
        t = now();
#pragma omp parallel for schedule(static)
        for (long i = 0; i < N; i++) a[i] = b[i] + 0.5f * c[i];
        t = now() - t; if (t < best_triad) best_triad = t;
    }
    double gb = N * sizeof(float) / 1e9;
    printf("threads=%d read=%.1f rmw(2r+1w)=%.1f triad(2r+1w)=%.1f GB/s sink=%g\n", omp_get_max_threads(),
           gb / best_read, 3 * gb / best_rmw, 3 * gb / best_triad, (double)sink);
    return 0;
}
