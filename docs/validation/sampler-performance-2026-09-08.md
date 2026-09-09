# Multiway sampler allocation microbenchmark (2026-09-08)

Scope: `DealSampler::sample_counted` with six uniform full ranges, a fixed
`ChaCha20Rng` seed **91**, and 500,000 returned samples consumed through
`std::hint::black_box`. The benchmark was built and run with the workspace
release profile on Windows. Compilation time is excluded.

Command:

```text
cargo test --release -p multiway sampler_throughput_benchmark -- --ignored --nocapture
```

Raw results:

| implementation | seconds | samples/second |
|---|---:|---:|
| baseline (`Vec` deck and `Vec` hole combos) | 0.226790 | 2,204,682 |
| fixed deck only, run 1 | 0.208259 | 2,400,853 |
| fixed deck only, run 2 | 0.210118 | 2,379,618 |
| fixed deck and fixed hole-combo storage, run 1 | 0.189756 | 2,634,956 |
| fixed deck and fixed hole-combo storage, run 2 | 0.193644 | 2,582,052 |

The final path reduced sampler wall time by 14.6% to 16.3% in this narrow
microbenchmark and increased throughput by 17.1% to 19.5%. This is not an
end-to-end solver speedup: tree traversal, abstraction, terminal evaluation,
and delta merging are intentionally absent.

The optimized path keeps the original ordered live-card sequence and calls
the same slice shuffle, so its RNG consumption is unchanged. The regression
test `optimized_deck_path_matches_legacy_seed_streams` reconstructs the old
heap-backed implementation and compares 256 consecutive samples plus the next
RNG word for each of uniform, weighted, and collision-heavy ranges.
