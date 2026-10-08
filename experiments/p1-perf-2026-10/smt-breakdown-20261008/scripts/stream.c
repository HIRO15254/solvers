/* Minimal STREAM-like bandwidth probe: copy (a=b), scale RMW (a=a*s+b, like the regret/strategy updates) and read-only sum. */
#include <omp.h>
#include <stdio.h>
#include <stdlib.h>
#define N (512L * 1024 * 1024) /* 2 GiB per f32 array */
int main(void) {
  float *a = malloc(N * sizeof(float)), *b = malloc(N * sizeof(float));
#pragma omp parallel for schedule(static)
  for (long i = 0; i < N; i++) { a[i] = 1.0f; b[i] = 0.5f; }
  for (int rep = 0; rep < 3; rep++) {
    double t = omp_get_wtime();
#pragma omp parallel for schedule(static)
    for (long i = 0; i < N; i++) a[i] = a[i] * 0.999f + b[i];
    double rmw = omp_get_wtime() - t;
    t = omp_get_wtime();
    double s = 0;
#pragma omp parallel for schedule(static) reduction(+ : s)
    for (long i = 0; i < N; i++) s += a[i];
    double rd = omp_get_wtime() - t;
    printf("threads=%d rmw2(a=a*s+b) %.1f GB/s  read %.1f GB/s  (%g)\n", omp_get_max_threads(),
           3.0 * N * 4 / rmw / 1e9, 1.0 * N * 4 / rd / 1e9, s);
  }
  return 0;
}
