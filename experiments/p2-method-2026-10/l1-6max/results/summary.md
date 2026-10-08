## S4-2b runs

### Runs

Times exclude the checkpoints' evaluation. Phases are seconds per iteration over the whole run.

| run | tree | leaf | solver K4 | iterations | s/iteration | K4 | Postflop | T3 | T2 | reaches | update | evaluations | s/evaluation | solve hours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| b7-l0 | 6max_20bb.toml | l0 | 256, min 16 | 500 | 2.040 | 0.944 | 0.000 | 0.656 | 0.166 | 0.077 | 0.051 | 11 | 70.2 | 0.50 |
| b7-l0-all | 6max_20bb.toml | l0 | 256 | 500 | 4.907 | 4.338 | 0.000 | 0.379 | 0.047 | 0.074 | 0.048 | 11 | 62.0 | 0.87 |
| b7-l1 | 6max_20bb.toml | l1 | 256, min 16 | 2000 | 2.171 | 0.358 | 1.003 | 0.381 | 0.164 | 0.070 | 0.047 | 9 | 104.6 | 1.47 |
| b4s-l0 | 6max_100bb_nl50_partial_simple_reference.toml | l0 | 256, min 16 | 500 | 1.636 | 0.985 | 0.000 | 0.446 | 0.049 | 0.081 | 0.051 | 6 | 67.7 | 0.34 |
| b4s-l1 | 6max_100bb_nl50_partial_simple_reference.toml | l1 | 256, min 16 | 2000 | 3.585 | 0.589 | 2.353 | 0.327 | 0.166 | 0.074 | 0.051 | 9 | 152.8 | 2.37 |

### Seconds per iteration by stage of the solve

Medians of the printed iterations (every tenth) in each range, without evaluation: total / K4 / Postflop.

| run | 1-10 | 11-100 | 101-500 | 501-1000 | 1001-2000 |
|---|---|---|---|---|---|
| b7-l0 | 2.27 / 1.02 / 0.00 | 2.04 / 0.95 / 0.00 | 2.03 / 0.95 / 0.00 |  |  |
| b7-l0-all | 6.40 / 5.62 / 0.00 | 4.88 / 4.33 / 0.00 | 4.73 / 4.20 / 0.00 |  |  |
| b7-l1 | 3.05 / 0.76 / 1.20 | 2.53 / 0.56 / 1.14 | 2.31 / 0.43 / 1.08 | 1.93 / 0.26 / 0.94 | 2.08 / 0.36 / 0.96 |
| b4s-l0 | 2.07 / 1.12 / 0.00 | 1.80 / 1.07 / 0.00 | 1.57 / 0.97 / 0.00 |  |  |
| b4s-l1 | 4.77 / 0.99 / 2.63 | 4.56 / 0.87 / 2.73 | 3.84 / 0.66 / 2.51 | 3.57 / 0.60 / 2.31 | 3.24 / 0.49 / 2.16 |

### L1: primary metric (in-sample / held-out) and auxiliary metric, bb/hand

| iteration | b7-l1 primary | b7-l1 auxiliary | b4s-l1 primary | b4s-l1 auxiliary |
|---|---|---|---|---|
| 0 | 16.26 / 16.26 | 16.64 | 66.29 / 66.29 | 73.15 |
| 250 | 0.005988 / 0.0003239 | 1.726 | 0.0411 / 0.01802 | 16.74 |
| 500 | 0.004248 / -0.002356 | 1.746 | 0.01967 / -0.004578 | 18.46 |
| 750 | 0.003813 / -0.002599 | 1.754 | 0.0157 / -0.009656 | 19.44 |
| 1000 | 0.003751 / -0.003154 | 1.76 | 0.0134 / -0.01269 | 19.88 |
| 1250 | 0.003388 / -0.003566 | 1.765 | 0.01208 / -0.01495 | 20.28 |
| 1500 | 0.003427 / -0.003315 | 1.78 | 0.01136 / -0.01599 | 20.53 |
| 1750 | 0.0033 / -0.003798 | 1.792 | 0.01157 / -0.01502 | 20.84 |
| 2000 | 0.003341 / -0.003621 | 1.799 | 0.01114 / -0.01594 | 20.93 |

| run | seat gains at the last checkpoint (in-sample) |
|---|---|
| b7-l1 | 0.000309, 0.000306, 0.00249, 5.38e-05, 6.75e-05, 0.00011 |
| b4s-l1 | 0.00169, 0.00195, 0.0027, 0.00169, 0.00159, 0.00151 |

### L0: NashConv in the L0 model, bb/hand

| iteration | b7-l0 | b7-l0-all | b4s-l0 | b7-l0 / b7-l0-all |
|---|---|---|---|---|
| 0 | 16.25 | 16.25 | 65.93 | 1.000 |
| 50 | 0.01055 | 0.01215 |  | 0.868 |
| 100 | 0.002494 | 0.002861 | 0.02333 | 0.872 |
| 150 | 0.0012 | 0.00127 |  | 0.945 |
| 200 | 0.0007835 | 0.0007772 | 0.005501 | 1.008 |
| 250 | 0.0005494 | 0.0005433 |  | 1.011 |
| 300 | 0.0004147 | 0.0004029 | 0.002572 | 1.029 |
| 350 | 0.0003292 | 0.0003254 |  | 1.012 |
| 400 | 0.0002704 | 0.0002735 | 0.001503 | 0.989 |
| 450 | 0.0002409 | 0.0002426 |  | 0.993 |
| 500 | 0.0002227 | 0.0002169 | 0.001013 | 1.027 |

### The L1 solutions' Preflop in the L0 model (l0_eval)

| profile | NashConv | seat gains | L0 solution's NashConv (last checkpoint) |
|---|---|---|---|
| b4s-l1 | 0.7992 | 0.101, 0.172, 0.217, 0.0975, 0.0987, 0.113 | 0.001013 (b4s-l0) |
| b7-l1 | 0.1521 | 0.00453, 0.0208, 0.112, 0.00971, 0.00245, 0.0022 | 0.0002227 (b7-l0) |

### Preflop strategies: B7 L0 vs B7 L1

Frequencies weight each class by its combos and the acting seat's own reach in each profile. TV is the total variation between the two class rows, weighted by combos and the mean of both own reaches.

| path | actor | actions | B7 L0 | B7 L1 | TV (pp) | largest class TV |
|---|---|---|---|---|---|---|
| (root) | 3 | fold / raise-to:2500 / raise-to:20000 | 0.875 / 0.125 / 0.000 | 0.840 / 0.160 / 0.000 | 3.8 | A6s 100 |
| fold | 4 | fold / raise-to:2500 / raise-to:20000 | 0.850 / 0.138 / 0.012 | 0.813 / 0.187 / 0.000 | 4.9 | JTs 100 |
| raise-to:20000:all-in | 4 | fold / call | 0.954 / 0.046 | 0.950 / 0.050 | 0.5 | AQo 43 |
| raise-to:2500 | 4 | fold / call / raise-to:7500 / raise-to:20000 | 0.936 / 0.015 / 0.000 / 0.049 | 0.931 / 0.000 / 0.030 / 0.039 | 4.2 | KJs 100 |
| fold fold | 5 | fold / raise-to:2500 / raise-to:20000 | 0.819 / 0.142 / 0.039 | 0.766 / 0.234 / 0.000 | 9.2 | QTo 100 |
| fold raise-to:20000:all-in | 5 | fold / call | 0.957 / 0.043 | 0.938 / 0.062 | 1.9 | AJs 100 |
| fold raise-to:2500 | 5 | fold / call / raise-to:7500 / raise-to:20000 | 0.920 / 0.013 / 0.000 / 0.067 | 0.914 / 0.000 / 0.002 / 0.084 | 2.8 | KTs 100 |
| raise-to:20000:all-in call:20000:all-in | 5 | fold / call | 0.951 / 0.049 | 0.941 / 0.059 | 1.2 | 77 64 |
| raise-to:20000:all-in fold | 5 | fold / call | 0.955 / 0.045 | 0.952 / 0.048 | 0.5 | AQo 39 |
| raise-to:2500 call:2500 | 5 | fold / call / raise-to:7500 / raise-to:20000 | 0.937 / 0.008 / 0.001 / 0.055 | 0.913 / 0.023 / 0.012 / 0.052 | 4.5 | 99 99 |
| raise-to:2500 fold | 5 | fold / call / raise-to:7500 / raise-to:20000 | 0.930 / 0.014 / 0.000 / 0.056 | 0.921 / 0.006 / 0.029 / 0.044 | 5.4 | 88 100 |
| raise-to:2500 raise-to:20000:all-in | 5 | fold / call | 0.973 / 0.027 | 0.970 / 0.030 | 0.3 | JJ 71 |
| raise-to:2500 raise-to:7500 | 5 | fold / call / raise-to:20000 | 0.965 / 0.030 / 0.005 | 0.968 / 0.000 / 0.032 | 3.0 | AKo 98 |
| fold fold fold | 0 | fold / raise-to:2500 / raise-to:20000 | 0.736 / 0.137 / 0.127 | 0.692 / 0.302 / 0.005 | 17.3 | T9s 100 |
| fold fold raise-to:20000:all-in | 0 | fold / call | 0.934 / 0.066 | 0.921 / 0.079 | 1.5 | 66 100 |
| fold fold raise-to:2500 | 0 | fold / call / raise-to:7500 / raise-to:20000 | 0.892 / 0.021 / 0.000 / 0.088 | 0.889 / 0.002 / 0.000 / 0.108 | 4.2 | 66 100 |

### Preflop strategies: B4 Simple L0 vs B4 Simple L1

Frequencies weight each class by its combos and the acting seat's own reach in each profile. TV is the total variation between the two class rows, weighted by combos and the mean of both own reaches.

| path | actor | actions | B4 Simple L0 | B4 Simple L1 | TV (pp) | largest class TV |
|---|---|---|---|---|---|---|
| (root) | 3 | fold / raise-to:2000 | 0.849 / 0.151 | 0.784 / 0.216 | 9.2 | 88 100 |
| fold | 4 | fold / raise-to:2000 | 0.823 / 0.177 | 0.739 / 0.261 | 11.2 | JTs 100 |
| raise-to:2000 | 4 | fold / raise-to:6500 | 0.943 / 0.057 | 0.910 / 0.090 | 6.0 | KQo 100 |
| fold fold | 5 | fold / raise-to:2300 / raise-to:100000 | 0.776 / 0.224 / 0.000 | 0.702 / 0.298 / 0.000 | 9.4 | T9s 100 |
| fold raise-to:2000 | 5 | fold / raise-to:6500 | 0.933 / 0.067 | 0.886 / 0.114 | 7.3 | KQo 100 |
| raise-to:2000 fold | 5 | fold / raise-to:6500 | 0.939 / 0.061 | 0.899 / 0.101 | 6.9 | KQo 100 |
| raise-to:2000 raise-to:6500 | 5 | fold / call / raise-to:16250 / raise-to:100000 | 0.973 / 0.013 / 0.004 / 0.011 | 0.958 / 0.011 / 0.031 / 0.000 | 3.2 | AQs 100 |
| fold fold fold | 0 | fold / raise-to:2500 / raise-to:100000 | 0.725 / 0.275 / 0.000 | 0.551 / 0.449 / 0.000 | 18.1 | T6s 100 |
| fold fold raise-to:100000:all-in | 0 | fold / call | 0.977 / 0.023 | 0.974 / 0.026 | 0.5 | JJ 44 |
| fold fold raise-to:2300 | 0 | fold / raise-to:7500 / raise-to:100000 | 0.916 / 0.084 / 0.000 | 0.853 / 0.147 / 0.000 | 8.1 | QJs 100 |
| fold raise-to:2000 fold | 0 | fold / raise-to:7500 | 0.933 / 0.067 | 0.879 / 0.121 | 8.0 | QJs 100 |
| fold raise-to:2000 raise-to:6500 | 0 | fold / call / raise-to:16250 / raise-to:100000 | 0.968 / 0.013 / 0.003 / 0.016 | 0.944 / 0.010 / 0.045 / 0.000 | 5.3 | AQo 100 |
| raise-to:2000 fold fold | 0 | fold / raise-to:7500 | 0.941 / 0.059 | 0.893 / 0.107 | 6.7 | KQo 100 |
| raise-to:2000 fold raise-to:6500 | 0 | fold / call / raise-to:16250 / raise-to:100000 | 0.971 / 0.013 / 0.000 / 0.016 | 0.950 / 0.014 / 0.036 / 0.000 | 4.7 | AQs 100 |
| raise-to:2000 raise-to:6500 call:6500 | 0 | fold / call / raise-to:16250 / raise-to:100000 | 0.950 / 0.029 / 0.000 / 0.021 | 0.967 / 0.002 / 0.001 / 0.031 | 3.5 | AKo 51 |
| raise-to:2000 raise-to:6500 fold | 0 | fold / call / raise-to:16250 / raise-to:100000 | 0.971 / 0.015 / 0.000 / 0.014 | 0.954 / 0.014 / 0.032 / 0.000 | 4.0 | AQs 100 |

### B4 Simple: unopened opens against GTO Wizard Simple

Open-raise frequency (the reference menu's non-all-in raise) and all-in frequency, combo-weighted, and the combo-weighted mean absolute error of the open raise against the reference, per class.

| position | raise | GTO Wizard open | L1 open / all-in / MAE | L0 open / all-in / MAE |
|---|---|---|---|---|
| UTG | raise-to:2000 | 0.198 | 0.216 / 0.000 / 0.078 | 0.151 / 0.000 / 0.075 |
| HJ | raise-to:2000 | 0.244 | 0.261 / 0.000 / 0.085 | 0.177 / 0.000 / 0.084 |
| CO | raise-to:2300 | 0.294 | 0.298 / 0.000 / 0.076 | 0.224 / 0.000 / 0.112 |
| BTN | raise-to:2500 | 0.421 | 0.449 / 0.000 / 0.084 | 0.275 / 0.000 / 0.150 |
| SB | raise-to:3000 | 0.430 | 0.469 / 0.000 / 0.083 | 0.565 / 0.000 / 0.191 |
| mean MAE | | | 0.081 | 0.123 |
| pooled RMSE | | | 0.253 | 0.328 |

