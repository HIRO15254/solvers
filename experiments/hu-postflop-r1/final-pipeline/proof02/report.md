Final pipeline: **completed**

Three synthetic no-rake HU games, one worker, F32 Full artifacts; no overall R1 certification.

| Phase | Passed | Failed | Skipped | Pending |
|---|---:|---:|---:|---:|
| build | 9 | 0 | 0 | 0 |
| measurement | 156 | 0 | 0 | 0 |

Measured repetitions: 3 per arm; one warmup excluded. Ratios are new / old.

| Case | Old CLI median (s) | New CLI median (s) | Ratio | Old iterations | New iterations |
|---|---:|---:|---:|---|---|
| river | 0.177177178 | 0.06389673 | 0.360637 | 100, 100, 100 | 100, 100, 100 |
| turn | 5.23997854 | 0.196820227 | 0.037561 | 100, 100, 100 | 100, 100, 100 |
| flop | 34.7661764 | 0.562089922 | 0.016168 | 50, 50, 50 | 50, 50, 50 |

CLI ratio geometric mean: 0.060277; time screen: pass.

| Case / arm | Live NashConv (chips, repeats) | Saved NashConv (chips, repeats) | SOL bytes | Checkpoint bytes |
|---|---|---|---|---|
| river / old | 0.0297500589, 0.0297500589, 0.0297500589 | 0.0297477582, 0.0297477582, 0.0297477582 | 7605, 7605, 7608 | 81330, 81330, 81330 |
| river / new | 0.0297500589, 0.0297500589, 0.0297500589 | 0.0297477582, 0.0297477582, 0.0297477582 | 4685, 4682, 4684 | 11157, 11157, 11157 |
| turn / old | 0.0288279851, 0.0288279851, 0.0288279851 | 0.0288382401, 0.0288382401, 0.0288382401 | 24919, 24919, 24916 | 356288, 356288, 356288 |
| turn / new | 0.0288279851, 0.0288279851, 0.0288279851 | 0.0288382401, 0.0288382401, 0.0288382401 | 12489, 12489, 12490 | 20637, 20637, 20637 |
| flop / old | 0.0366989242, 0.0366989242, 0.0366989242 | 0.0367002487, 0.0367002487, 0.0367002487 | 176832, 176831, 176832 | 34545, 34545, 34545 |
| flop / new | 0.0366989242, 0.0366989242, 0.0366989242 | 0.0367002487, 0.0367002487, 0.0367002487 | 81335, 81334, 81336 | 1531, 1531, 1531 |

Both quality checks require NashConv strictly below 0.04 chips. The fixed negative roundoff allowance is 1e-6 chips; raw values are not clamped.

| Case | Conservative OS RSS bound (new / old) |
|---|---:|
| river | 9.530144 |
| turn | 3.422545 |
| flop | unavailable |

Memory uses max(new native OS peaks) / min(old sampled OS peaks). It is not a physical-memory median, population estimate or phase peak. An unmet bound is inconclusive about improvement.
Flop memory screen: unavailable.

| Case / I/O operation | Old median (ms) | New median (ms) | Ratio | Interpretation |
|---|---:|---:|---:|---|
| river / decode-all | 0.828930 | 0.234215 | 0.282551 | descriptive only (old < 10 ms) |
| river / read-root | 0.833798 | 0.225068 | 0.269931 | descriptive only (old < 10 ms) |
| river / stream-write | 3.798987 | 4.386660 | 1.154692 | descriptive only (old < 10 ms) |
| turn / decode-all | 23.295153 | 1.058663 | 0.045446 | pass |
| turn / read-root | 3.400831 | 0.222682 | 0.065479 | descriptive only (old < 10 ms) |
| turn / stream-write | 18.333028 | 4.389069 | 0.239408 | pass |
| flop / decode-all | 423.194389 | 6.961171 | 0.016449 | pass |
| flop / read-root | 2.682036 | 0.201857 | 0.075263 | descriptive only (old < 10 ms) |
| flop / stream-write | 227.240853 | 15.499053 | 0.068205 | pass |

Eligible I/O screen: pass. Its maximum ratio is 1.10; sub-10ms rows do not pass or fail it.
Codec decode/write times cover the operation; read-root also includes open time. Whole CLI solves include initialization, CFR, quality checks, checkpoint/SOL writes and shutdown. Internal phase timings are unavailable.
Single-case speed claims require an old CLI median of at least 1 second. These are scoped descriptive screens, not overall R1 certification.
