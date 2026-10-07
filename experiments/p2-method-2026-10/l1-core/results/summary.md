## B6

### Primary NashConv (in-sample), bb/hand

| run | training | evaluation | 0 | 250 | 500 | 750 | 1000 | 1250 | 1500 | 1750 | 2000 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| b6-n1-plain | n1 random β0 | 4096 random reg | 2.41 | 0.113 | 0.082 | 0.0725 | 0.0712 | 0.0682 | 0.0641 | 0.0562 | 0.053 |
| b6-n32-cv-strat-reg | n32 stratified CV reg β0 | 4096 random reg | 2.41 | 0.0073 | 0.00581 | 0.00495 | 0.00441 | 0.0041 | 0.00418 | 0.00407 | 0.00393 |
| b6-n32-cv-strat | n32 stratified CV β0 | 4096 random reg | 2.41 | 0.00842 | 0.00641 | 0.00542 | 0.00505 | 0.00468 | 0.0047 | 0.0045 | 0.00426 |
| b6-n32-cv | n32 random CV β0 | 4096 random reg | 2.41 | 0.0106 | 0.00768 | 0.00694 | 0.00638 | 0.00568 | 0.00538 | 0.00529 | 0.00517 |
| b6-n32-final-eval16k | n32 stratified CV reg β1/0 | 16384 random reg | 2.41 |  |  |  | 0.00166 |  |  |  | 0.00121 |
| b6-n32-final-evalcv | n32 stratified CV reg β1/0 | 4096 random CV | 2.41 |  |  |  | 0.00274 |  |  |  | 0.00271 |
| b6-n32-final-evalplain | n32 stratified CV reg β1/0 | 4096 random plain | 2.41 |  |  |  | 0.0037 |  |  |  | 0.00358 |
| b6-n32-final-postflop1 | n32 stratified CV reg β1 | 4096 random reg | 2.41 | 0.00553 | 0.00372 | 0.00256 | 0.00249 | 0.00278 | 0.00228 | 0.00294 | 0.00283 |
| b6-n32-final | n32 stratified CV reg β1/0 | 4096 random reg | 2.41 | 0.00557 | 0.00351 | 0.00254 | 0.00244 | 0.00242 | 0.00241 | 0.00238 | 0.00252 |
| b6-n32-plain | n32 random β0 | 4096 random reg | 2.41 | 0.0152 | 0.0111 | 0.0103 | 0.00944 | 0.00835 | 0.00763 | 0.00744 | 0.00713 |
| b6-n64-final | n64 stratified CV reg β1/0 | 4096 random reg | 2.41 | 0.00525 | 0.00301 | 0.00257 | 0.00214 | 0.00237 | 0.00229 | 0.00232 | 0.00235 |
| b6-n8-final | n8 stratified CV reg β1/0 | 4096 random reg | 2.41 | 0.0124 | 0.00566 | 0.00475 | 0.0039 | 0.00337 | 0.00319 | 0.00338 | 0.00301 |

### Primary NashConv (held-out), bb/hand

| run | training | evaluation | 0 | 250 | 500 | 750 | 1000 | 1250 | 1500 | 1750 | 2000 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| b6-n1-plain | n1 random β0 | 4096 random reg | 2.41 | 0.113 | 0.0815 | 0.0717 | 0.0703 | 0.0674 | 0.0633 | 0.055 | 0.0519 |
| b6-n32-cv-strat-reg | n32 stratified CV reg β0 | 4096 random reg | 2.41 | 0.00567 | 0.00363 | 0.00227 | 0.00176 | 0.00111 | 0.00132 | 0.00129 | 0.000877 |
| b6-n32-cv-strat | n32 stratified CV β0 | 4096 random reg | 2.41 | 0.00687 | 0.00439 | 0.00265 | 0.00244 | 0.00184 | 0.00208 | 0.0017 | 0.00121 |
| b6-n32-cv | n32 random CV β0 | 4096 random reg | 2.41 | 0.00903 | 0.00559 | 0.00466 | 0.00396 | 0.00318 | 0.00256 | 0.00261 | 0.00255 |
| b6-n32-final-eval16k | n32 stratified CV reg β1/0 | 16384 random reg | 2.41 |  |  |  | 0.000518 |  |  |  | -0.000191 |
| b6-n32-final-evalcv | n32 stratified CV reg β1/0 | 4096 random CV | 2.41 |  |  |  | -0.00284 |  |  |  | -0.00362 |
| b6-n32-final-evalplain | n32 stratified CV reg β1/0 | 4096 random plain | 2.41 |  |  |  | -0.00426 |  |  |  | -0.00516 |
| b6-n32-final-postflop1 | n32 stratified CV reg β1 | 4096 random reg | 2.41 | 0.00317 | -0.000844 | -0.00161 | -0.00178 | -0.00158 | -0.00236 | -0.00164 | -0.00197 |
| b6-n32-final | n32 stratified CV reg β1/0 | 4096 random reg | 2.41 | 0.00347 | -0.000263 | -0.00131 | -0.00174 | -0.00206 | -0.00223 | -0.00224 | -0.002 |
| b6-n32-plain | n32 random β0 | 4096 random reg | 2.41 | 0.0134 | 0.00941 | 0.00803 | 0.00716 | 0.00602 | 0.00518 | 0.00524 | 0.00475 |
| b6-n64-final | n64 stratified CV reg β1/0 | 4096 random reg | 2.41 | 0.00208 | -0.00157 | -0.0017 | -0.00227 | -0.00228 | -0.00263 | -0.00252 | -0.00258 |
| b6-n8-final | n8 stratified CV reg β1/0 | 4096 random reg | 2.41 | 0.0114 | 0.00378 | 0.00231 | 0.000383 | 5.4e-05 | -0.000367 | 0.000222 | 0.000214 |

### Final checkpoint and timing

| run | iterations | in-sample | held-out | auxiliary | auxiliary held-out | s/iteration | postflop s/iteration | evaluation s (mean) |
|---|---|---|---|---|---|---|---|---|
| b6-n1-plain | 2000 | 0.05304 | 0.05191 | 1.287 | 1.287 | 0.0217 | 0.0175 | 28.3 |
| b6-n32-cv-strat-reg | 2000 | 0.003927 | 0.0008771 | 1.689 | 1.689 | 0.1718 | 0.1694 | 24.6 |
| b6-n32-cv-strat | 2000 | 0.00426 | 0.001206 | 1.686 | 1.685 | 0.1708 | 0.1684 | 24.6 |
| b6-n32-cv | 2000 | 0.005171 | 0.002549 | 1.661 | 1.661 | 0.1862 | 0.1836 | 25.6 |
| b6-n32-final-eval16k | 2000 | 0.001215 | -0.0001908 | 1.711 | 1.711 | 0.1744 | 0.1721 | 98.2 |
| b6-n32-final-evalcv | 2000 | 0.002707 | -0.003624 | 1.717 | 1.716 | 0.1660 | 0.1637 | 24.6 |
| b6-n32-final-evalplain | 2000 | 0.003577 | -0.005163 | 1.714 | 1.714 | 0.1699 | 0.1676 | 24.6 |
| b6-n32-final-postflop1 | 2000 | 0.002833 | -0.001973 | 1.91 | 1.909 | 0.1677 | 0.1655 | 24.6 |
| b6-n32-final | 2000 | 0.002518 | -0.001998 | 1.719 | 1.719 | 0.1682 | 0.1659 | 24.8 |
| b6-n32-plain | 2000 | 0.007128 | 0.004755 | 1.659 | 1.658 | 0.1941 | 0.1909 | 26.4 |
| b6-n64-final | 2000 | 0.002354 | -0.00258 | 1.774 | 1.774 | 0.3226 | 0.3200 | 24.7 |
| b6-n8-final | 2000 | 0.003008 | 0.0002142 | 1.536 | 1.536 | 0.0650 | 0.0630 | 24.7 |

### Training does not depend on the evaluator

Runs with the same training options but different evaluation options.

| training | runs | identical average profiles |
|---|---|---|
| n32 stratified CV reg β1/0 | b6-n32-final-eval16k, b6-n32-final-evalcv, b6-n32-final-evalplain, b6-n32-final | True |

### L0 runs

| run | iterations | NashConv | s/iteration |
|---|---|---|---|
| b6-l0 | 2000 | 1.714e-05 | 0.0013 |

### Preflop profiles in the L0 checkdown model (l0_eval)

| profile | NashConv | seat gains |
|---|---|---|
| b6-n1-plain | 0.1524 | 0.03043, 0.1219 |
| b6-n32-final | 0.128 | 0.05714, 0.07084 |

### Preflop strategies: L0 vs b6-n1-plain

Frequencies weight each class by its combos and the acting seat's own reach in each profile. TV is the total variation between the two class rows, weighted by combos and the mean of both own reaches.

| path | actor | actions | L0 | b6-n1-plain | TV (pp) | largest class TV |
|---|---|---|---|---|---|---|
| (root) | 0 | fold / call / raise-to:2500 / raise-to:20000 | 0.097 / 0.705 / 0.054 / 0.144 | 0.063 / 0.713 / 0.200 / 0.024 | 35.9 | 75s 99 |
| call:500 | 1 | check / raise-to:2500 / raise-to:20000 | 0.614 / 0.161 / 0.225 | 0.638 / 0.106 / 0.256 | 29.9 | K6s 95 |
| raise-to:20000:all-in | 1 | fold / call | 0.799 / 0.201 | 0.764 / 0.236 | 4.7 | 22 99 |
| raise-to:2500 | 1 | fold / call / raise-to:7500 / raise-to:20000 | 0.105 / 0.664 / 0.000 / 0.231 | 0.421 / 0.321 / 0.018 / 0.241 | 45.6 | 42o 95 |
| call:500 raise-to:20000:all-in | 0 | fold / call | 0.868 / 0.132 | 0.834 / 0.166 | 0.9 | A2s 68 |
| call:500 raise-to:2500 | 0 | fold / call / raise-to:7500 / raise-to:20000 | 0.103 / 0.778 / 0.000 / 0.119 | 0.352 / 0.574 / 0.031 / 0.043 | 39.2 | J8s 99 |
| raise-to:2500 raise-to:20000:all-in | 0 | fold / call | 0.707 / 0.293 | 0.562 / 0.438 | 16.0 | KJo 100 |
| raise-to:2500 raise-to:7500 | 0 | fold / call / raise-to:20000 | 0.483 / 0.303 / 0.214 | 0.483 / 0.195 / 0.321 | 39.2 | J2o 94 |
| call:500 raise-to:2500 raise-to:20000:all-in | 1 | fold / call | 0.711 / 0.289 | 0.623 / 0.377 | 7.9 | A8o 95 |
| call:500 raise-to:2500 raise-to:7500 | 1 | fold / call / raise-to:20000 | 0.485 / 0.306 / 0.209 | 0.537 / 0.217 / 0.245 | 29.0 | J4o 96 |
| raise-to:2500 raise-to:7500 raise-to:20000:all-in | 1 | fold / call | 0.338 / 0.662 | 0.002 / 0.998 | 27.2 | K2o 100 |
| call:500 raise-to:2500 raise-to:7500 raise-to:20000:all-in | 0 | fold / call | 0.332 / 0.668 | 0.297 / 0.703 | 32.5 | JTo 94 |

### Preflop strategies: L0 vs b6-n32-final

Frequencies weight each class by its combos and the acting seat's own reach in each profile. TV is the total variation between the two class rows, weighted by combos and the mean of both own reaches.

| path | actor | actions | L0 | b6-n32-final | TV (pp) | largest class TV |
|---|---|---|---|---|---|---|
| (root) | 0 | fold / call / raise-to:2500 / raise-to:20000 | 0.097 / 0.705 / 0.054 / 0.144 | 0.045 / 0.818 / 0.125 / 0.012 | 29.3 | A4s 100 |
| call:500 | 1 | check / raise-to:2500 / raise-to:20000 | 0.614 / 0.161 / 0.225 | 0.623 / 0.171 / 0.206 | 29.1 | JTs 100 |
| raise-to:20000:all-in | 1 | fold / call | 0.799 / 0.201 | 0.770 / 0.230 | 5.4 | 22 100 |
| raise-to:2500 | 1 | fold / call / raise-to:7500 / raise-to:20000 | 0.105 / 0.664 / 0.000 / 0.231 | 0.410 / 0.351 / 0.028 / 0.212 | 43.8 | K2o 100 |
| call:500 raise-to:20000:all-in | 0 | fold / call | 0.868 / 0.132 | 0.849 / 0.151 | 2.4 | KJo 100 |
| call:500 raise-to:2500 | 0 | fold / call / raise-to:7500 / raise-to:20000 | 0.103 / 0.778 / 0.000 / 0.119 | 0.379 / 0.526 / 0.001 / 0.094 | 42.1 | T6o 100 |
| raise-to:2500 raise-to:20000:all-in | 0 | fold / call | 0.707 / 0.293 | 0.599 / 0.401 | 8.1 | J2o 95 |
| raise-to:2500 raise-to:7500 | 0 | fold / call / raise-to:20000 | 0.483 / 0.303 / 0.214 | 0.595 / 0.009 / 0.396 | 40.7 | KJs 99 |
| call:500 raise-to:2500 raise-to:20000:all-in | 1 | fold / call | 0.711 / 0.289 | 0.669 / 0.331 | 10.5 | KQo 100 |
| call:500 raise-to:2500 raise-to:7500 | 1 | fold / call / raise-to:20000 | 0.485 / 0.306 / 0.209 | 0.558 / 0.197 / 0.245 | 32.8 | J4o 99 |
| raise-to:2500 raise-to:7500 raise-to:20000:all-in | 1 | fold / call | 0.338 / 0.662 | 0.000 / 1.000 | 19.2 | K7o 100 |
| call:500 raise-to:2500 raise-to:7500 raise-to:20000:all-in | 0 | fold / call | 0.332 / 0.668 | 0.300 / 0.700 | 24.6 | A7o 99 |

## B7

### b7-n32: n32 stratified CV reg β1/0; evaluation 1024 random reg

L1 leaves 186, postflop decisions 8052.

| phase | seconds per iteration |
|---|---|
| reaches | 0.101 |
| t2 | 0.169 |
| t3 | 0.686 |
| k4 | 3.064 |
| update | 0.050 |
| board_preparation | 0.004 |
| postflop | 2.899 |
| total | 7.121 |

| iteration | in-sample | held-out | auxiliary | evaluation s |
|---|---|---|---|---|
| 0 | 16.26 | 16.26 | 16.64 | 219.2 |
| 10 | 0.6341 | 0.6327 | 2.557 | 219.7 |

### b7-n8: n8 stratified CV reg β1/0; evaluation 1024 random reg

L1 leaves 186, postflop decisions 8052.

| phase | seconds per iteration |
|---|---|
| reaches | 0.096 |
| t2 | 0.166 |
| t3 | 0.679 |
| k4 | 3.142 |
| update | 0.042 |
| board_preparation | 0.005 |
| postflop | 0.790 |
| total | 5.201 |

## Exploration (development binaries, 1000 iterations)

### Primary NashConv (in-sample), bb/hand

| run | training | evaluation | 0 | 500 | 1000 |
|---|---|---|---|---|---|
| b6v3-n32-trainS-beta05-evalR-regression | n32 stratified CV β0.5 | 4096 random reg | 2.41 | 0.00375 | 0.00282 |
| b6v3-n32-trainS-beta1-evalR-regression | n32 stratified CV β1 | 4096 random reg | 2.41 | 0.00409 | 0.00266 |
| b6v3-n32-trainS-evalR-regression | n32 stratified CV β0 | 4096 random reg | 2.41 | 0.00641 | 0.00505 |
| b6v3-n32-trainS-evalR-unit | n32 stratified CV β0 | 4096 random CV | 2.41 | 0.00657 | 0.00535 |
| b6v3-n32-trainS-smooth90-evalR-regression | n32 stratified CV avg0.9 β0 | 4096 random reg | 2.41 | 0.00434 | 0.00372 |
| b6v3-n32-trainS-smooth98-evalR-regression | n32 stratified CV avg0.98 β0 | 4096 random reg | 2.41 | 0.0043 | 0.0027 |
| b6v3-n32-trainS-treg-evalR-regression | n32 stratified CV reg β0 | 4096 random reg | 2.41 | 0.00581 | 0.00441 |
| b6v4-n32-treg-a1-b1 | n32 stratified CV reg β1 α1 γ2 | 4096 random reg | 2.41 | 0.00371 | 0.00274 |
| b6v4-n32-treg-a15-b05 | n32 stratified CV reg β0.5 | 4096 random reg | 2.41 | 0.00365 | 0.00267 |
| b6v4-n32-treg-a15-b1 | n32 stratified CV reg β1 | 4096 random reg | 2.41 | 0.00372 | 0.00249 |
| b6v4-n32-treg-a15-b2 | n32 stratified CV reg β2 | 4096 random reg | 2.41 | 0.00715 | 0.00545 |
| b6v4-n32-treg-a3-b1 | n32 stratified CV reg β1 α3 γ2 | 4096 random reg | 2.41 | 0.00491 | 0.00278 |
| b6v5-n32-treg-b0-s98-e16k | n32 stratified CV reg avg0.98 β0 | 16384 random reg | 2.41 |  | 0.00163 |
| b6v5-n32-treg-b1-e16k | n32 stratified CV reg β1 | 16384 random reg | 2.41 |  | 0.00157 |
| b6v5-n32-treg-b1-s98-e16k | n32 stratified CV reg avg0.98 β1 | 16384 random reg | 2.41 |  | 0.00276 |
| b6v5-n32-treg-b1-s99-e16k | n32 stratified CV reg avg0.99 β1 | 16384 random reg | 2.41 |  | 0.00296 |
| b6v5-n8-treg-b1-s98-e16k | n8 stratified CV reg avg0.98 β1 | 16384 random reg | 2.41 |  | 0.00531 |
| b6v6-n32-treg-b0-pb1 | n32 stratified CV reg β0/1 | 4096 random reg | 2.41 | 0.00586 | 0.00475 |
| b6v6-n32-treg-b1-pb0 | n32 stratified CV reg β1/0 | 4096 random reg | 2.41 | 0.00351 | 0.00244 |
| b6v7-n16-split-e16k | n16 stratified CV reg β1/0 | 16384 random reg | 2.41 |  | 0.00225 |
| b6v7-n32-split-e16k | n32 stratified CV reg β1/0 | 16384 random reg | 2.41 |  | 0.00166 |
| b6v7-n32-split-s90-e16k | n32 stratified CV reg avg0.9 β1/0 | 16384 random reg | 2.41 |  | 0.00167 |
| b6v7-n64-split-e16k | n64 stratified CV reg β1/0 | 16384 random reg | 2.41 |  | 0.0013 |
| b6v7-n8-split-e16k | n8 stratified CV reg β1/0 | 16384 random reg | 2.41 |  | 0.00344 |

### Primary NashConv (held-out), bb/hand

| run | training | evaluation | 0 | 500 | 1000 |
|---|---|---|---|---|---|
| b6v3-n32-trainS-beta05-evalR-regression | n32 stratified CV β0.5 | 4096 random reg | 2.41 | 0.000428 | -0.00107 |
| b6v3-n32-trainS-beta1-evalR-regression | n32 stratified CV β1 | 4096 random reg | 2.41 | -0.000608 | -0.00126 |
| b6v3-n32-trainS-evalR-regression | n32 stratified CV β0 | 4096 random reg | 2.41 | 0.00439 | 0.00244 |
| b6v3-n32-trainS-evalR-unit | n32 stratified CV β0 | 4096 random CV | 2.41 | 0.00304 | 0.00106 |
| b6v3-n32-trainS-smooth90-evalR-regression | n32 stratified CV avg0.9 β0 | 4096 random reg | 2.41 | 0.00215 | 0.00128 |
| b6v3-n32-trainS-smooth98-evalR-regression | n32 stratified CV avg0.98 β0 | 4096 random reg | 2.41 | 0.00155 | -0.000686 |
| b6v3-n32-trainS-treg-evalR-regression | n32 stratified CV reg β0 | 4096 random reg | 2.41 | 0.00363 | 0.00176 |
| b6v4-n32-treg-a1-b1 | n32 stratified CV reg β1 α1 γ2 | 4096 random reg | 2.41 | -0.000126 | -0.00139 |
| b6v4-n32-treg-a15-b05 | n32 stratified CV reg β0.5 | 4096 random reg | 2.41 | 0.000537 | -0.000705 |
| b6v4-n32-treg-a15-b1 | n32 stratified CV reg β1 | 4096 random reg | 2.41 | -0.000844 | -0.00178 |
| b6v4-n32-treg-a15-b2 | n32 stratified CV reg β2 | 4096 random reg | 2.41 | 0.00218 | 0.00142 |
| b6v4-n32-treg-a3-b1 | n32 stratified CV reg β1 α3 γ2 | 4096 random reg | 2.41 | 0.000713 | -0.00133 |
| b6v5-n32-treg-b0-s98-e16k | n32 stratified CV reg avg0.98 β0 | 16384 random reg | 2.41 |  | 0.000592 |
| b6v5-n32-treg-b1-e16k | n32 stratified CV reg β1 | 16384 random reg | 2.41 |  | 0.000303 |
| b6v5-n32-treg-b1-s98-e16k | n32 stratified CV reg avg0.98 β1 | 16384 random reg | 2.41 |  | 0.00167 |
| b6v5-n32-treg-b1-s99-e16k | n32 stratified CV reg avg0.99 β1 | 16384 random reg | 2.41 |  | 0.00147 |
| b6v5-n8-treg-b1-s98-e16k | n8 stratified CV reg avg0.98 β1 | 16384 random reg | 2.41 |  | 0.00453 |
| b6v6-n32-treg-b0-pb1 | n32 stratified CV reg β0/1 | 4096 random reg | 2.41 | 0.00328 | 0.00203 |
| b6v6-n32-treg-b1-pb0 | n32 stratified CV reg β1/0 | 4096 random reg | 2.41 | -0.000263 | -0.00174 |
| b6v7-n16-split-e16k | n16 stratified CV reg β1/0 | 16384 random reg | 2.41 |  | 0.00121 |
| b6v7-n32-split-e16k | n32 stratified CV reg β1/0 | 16384 random reg | 2.41 |  | 0.000518 |
| b6v7-n32-split-s90-e16k | n32 stratified CV reg avg0.9 β1/0 | 16384 random reg | 2.41 |  | 0.000405 |
| b6v7-n64-split-e16k | n64 stratified CV reg β1/0 | 16384 random reg | 2.41 |  | -9.77e-06 |
| b6v7-n8-split-e16k | n8 stratified CV reg β1/0 | 16384 random reg | 2.41 |  | 0.00279 |

### Final checkpoint and timing

| run | iterations | in-sample | held-out | auxiliary | auxiliary held-out | s/iteration | postflop s/iteration | evaluation s (mean) |
|---|---|---|---|---|---|---|---|---|
| b6v3-n32-trainS-beta05-evalR-regression | 1000 | 0.002822 | -0.001065 | 1.72 | 1.72 | 0.1838 | 0.1811 | 26.2 |
| b6v3-n32-trainS-beta1-evalR-regression | 1000 | 0.002663 | -0.001261 | 1.85 | 1.849 | 0.1765 | 0.1738 | 25.4 |
| b6v3-n32-trainS-evalR-regression | 1000 | 0.005054 | 0.002443 | 1.627 | 1.627 | 0.1850 | 0.1814 | 25.8 |
| b6v3-n32-trainS-evalR-unit | 1000 | 0.005347 | 0.001055 | 1.625 | 1.624 | 0.1996 | 0.1957 | 26.2 |
| b6v3-n32-trainS-smooth90-evalR-regression | 1000 | 0.00372 | 0.001279 | 1.628 | 1.628 | 0.1963 | 0.1931 | 27.1 |
| b6v3-n32-trainS-smooth98-evalR-regression | 1000 | 0.002705 | -0.000686 | 1.659 | 1.659 | 0.2271 | 0.2213 | 29.4 |
| b6v3-n32-trainS-treg-evalR-regression | 1000 | 0.004407 | 0.001762 | 1.63 | 1.63 | 0.1894 | 0.1863 | 25.7 |
| b6v4-n32-treg-a1-b1 | 1000 | 0.002741 | -0.001393 | 1.862 | 1.862 | 0.1970 | 0.1930 | 27.2 |
| b6v4-n32-treg-a15-b05 | 1000 | 0.002671 | -0.0007052 | 1.722 | 1.722 | 0.2084 | 0.2031 | 28.3 |
| b6v4-n32-treg-a15-b1 | 1000 | 0.002488 | -0.001777 | 1.872 | 1.872 | 0.2048 | 0.2005 | 27.0 |
| b6v4-n32-treg-a15-b2 | 1000 | 0.005447 | 0.001421 | 1.94 | 1.94 | 0.2090 | 0.2052 | 27.1 |
| b6v4-n32-treg-a3-b1 | 1000 | 0.002779 | -0.001328 | 1.887 | 1.887 | 0.2214 | 0.2166 | 28.1 |
| b6v5-n32-treg-b0-s98-e16k | 1000 | 0.001632 | 0.0005917 | 1.639 | 1.639 | 0.2493 | 0.2413 | 114.6 |
| b6v5-n32-treg-b1-e16k | 1000 | 0.001571 | 0.0003033 | 1.864 | 1.864 | 0.2283 | 0.2239 | 109.3 |
| b6v5-n32-treg-b1-s98-e16k | 1000 | 0.002764 | 0.001669 | 1.822 | 1.822 | 0.2121 | 0.2088 | 107.6 |
| b6v5-n32-treg-b1-s99-e16k | 1000 | 0.002959 | 0.001474 | 1.881 | 1.881 | 0.2361 | 0.2301 | 111.0 |
| b6v5-n8-treg-b1-s98-e16k | 1000 | 0.005313 | 0.004533 | 1.772 | 1.772 | 0.0864 | 0.0836 | 109.7 |
| b6v6-n32-treg-b0-pb1 | 1000 | 0.004745 | 0.002028 | 1.864 | 1.864 | 0.2362 | 0.2291 | 28.2 |
| b6v6-n32-treg-b1-pb0 | 1000 | 0.002438 | -0.001738 | 1.655 | 1.655 | 0.2176 | 0.2128 | 28.2 |
| b6v7-n16-split-e16k | 1000 | 0.002253 | 0.001206 | 1.554 | 1.554 | 0.1515 | 0.1439 | 113.6 |
| b6v7-n32-split-e16k | 1000 | 0.001658 | 0.000518 | 1.648 | 1.647 | 0.2699 | 0.2586 | 154.7 |
| b6v7-n32-split-s90-e16k | 1000 | 0.001671 | 0.0004046 | 1.651 | 1.651 | 0.2733 | 0.2602 | 115.9 |
| b6v7-n64-split-e16k | 1000 | 0.001296 | -9.769e-06 | 1.727 | 1.727 | 0.4239 | 0.4145 | 114.1 |
| b6v7-n8-split-e16k | 1000 | 0.003444 | 0.002788 | 1.464 | 1.464 | 0.1310 | 0.1192 | 114.4 |

