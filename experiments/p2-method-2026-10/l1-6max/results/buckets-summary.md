## S4-2b runs

### Runs

Times exclude the checkpoints' evaluation. Phases are seconds per iteration over the whole run.

| run | tree | leaf | solver K4 | iterations | s/iteration | K4 | Postflop | T3 | T2 | reaches | update | evaluations | s/evaluation | solve hours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| b4s-b128 | b4s-b128.toml | l1 | 256, min 16 | 2000 | 4.718 | 0.838 | 3.179 | 0.449 | 0.064 | 0.103 | 0.057 | 5 | 315.2 | 3.06 |

### Seconds per iteration by stage of the solve

Medians of the printed iterations (every tenth) in each range, without evaluation: total / K4 / Postflop.

| run | 1-10 | 11-100 | 101-500 | 501-1000 | 1001-2000 |
|---|---|---|---|---|---|
| b4s-b128 | 6.20 / 1.31 / 3.55 | 4.84 / 0.95 / 3.20 | 7.68 / 1.45 / 5.01 | 3.74 / 0.65 / 2.57 | 3.33 / 0.54 / 2.29 |

### L1: primary metric (in-sample / held-out) and auxiliary metric, bb/hand

| iteration | b4s-b128 primary | b4s-b128 auxiliary |
|---|---|---|
| 0 | 66.29 / 66.29 | 73.15 |
| 500 | 0.02682 / 0.0034 | 17.22 |
| 1000 | 0.01672 / -0.00827 | 18.45 |
| 1500 | 0.01385 / -0.0119 | 19.09 |
| 2000 | 0.0134 / -0.01285 | 19.58 |

| run | seat gains at the last checkpoint (in-sample) |
|---|---|
| b4s-b128 | 0.00189, 0.0021, 0.00288, 0.0021, 0.00217, 0.00227 |

### Preflop strategies: b32 vs b128

Frequencies weight each class by its combos and the acting seat's own reach in each profile. TV is the total variation between the two class rows, weighted by combos and the mean of both own reaches.

| path | actor | actions | b32 | b128 | TV (pp) | largest class TV |
|---|---|---|---|---|---|---|
| (root) | 3 | fold / raise-to:2000 | 0.784 / 0.216 | 0.786 / 0.214 | 1.0 | K4s 52 |
| fold | 4 | fold / raise-to:2000 | 0.739 / 0.261 | 0.740 / 0.260 | 0.6 | K3s 45 |
| raise-to:2000 | 4 | fold / raise-to:6500 | 0.910 / 0.090 | 0.910 / 0.090 | 0.7 | A2s 45 |
| fold fold | 5 | fold / raise-to:2300 / raise-to:100000 | 0.702 / 0.298 / 0.000 | 0.701 / 0.299 / 0.000 | 1.1 | 77 51 |
| fold raise-to:2000 | 5 | fold / raise-to:6500 | 0.886 / 0.114 | 0.886 / 0.114 | 1.0 | A7s 69 |
| raise-to:2000 fold | 5 | fold / raise-to:6500 | 0.899 / 0.101 | 0.899 / 0.101 | 1.5 | A2s 49 |
| raise-to:2000 raise-to:6500 | 5 | fold / call / raise-to:16250 / raise-to:100000 | 0.958 / 0.011 / 0.031 / 0.000 | 0.959 / 0.010 / 0.031 / 0.000 | 0.6 | A4s 22 |
| fold fold fold | 0 | fold / raise-to:2500 / raise-to:100000 | 0.551 / 0.449 / 0.000 | 0.550 / 0.450 / 0.000 | 0.9 | K5o 46 |
| fold fold raise-to:100000:all-in | 0 | fold / call | 0.974 / 0.026 | 0.974 / 0.026 | 0.1 | AKo 6 |
| fold fold raise-to:2300 | 0 | fold / raise-to:7500 / raise-to:100000 | 0.853 / 0.147 / 0.000 | 0.853 / 0.147 / 0.000 | 0.5 | A9s 51 |
| fold raise-to:2000 fold | 0 | fold / raise-to:7500 | 0.879 / 0.121 | 0.878 / 0.122 | 0.9 | QJo 25 |
| fold raise-to:2000 raise-to:6500 | 0 | fold / call / raise-to:16250 / raise-to:100000 | 0.944 / 0.010 / 0.045 / 0.000 | 0.945 / 0.010 / 0.046 / 0.000 | 0.5 | K7s 26 |
| raise-to:2000 fold fold | 0 | fold / raise-to:7500 | 0.893 / 0.107 | 0.893 / 0.107 | 1.0 | Q6s 49 |
| raise-to:2000 fold raise-to:6500 | 0 | fold / call / raise-to:16250 / raise-to:100000 | 0.950 / 0.014 / 0.036 / 0.000 | 0.951 / 0.013 / 0.036 / 0.000 | 0.6 | KTs 28 |
| raise-to:2000 raise-to:6500 call:6500 | 0 | fold / call / raise-to:16250 / raise-to:100000 | 0.967 / 0.002 / 0.001 / 0.031 | 0.968 / 0.001 / 0.000 / 0.030 | 0.2 | JJ 6 |
| raise-to:2000 raise-to:6500 fold | 0 | fold / call / raise-to:16250 / raise-to:100000 | 0.954 / 0.014 / 0.032 / 0.000 | 0.957 / 0.012 / 0.031 / 0.000 | 0.8 | AQo 38 |

