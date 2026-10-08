| run | sweep | seed | BTN | SB | BB | UTG | HJ | CO | NashConv | defaulted mass (max) | k≥4 reach (max) | evaluation |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `20bb_cd` | 30k | 0 | 0.0557 | 0.0782 | 0.1105 | 0.0265 | 0.0324 | 0.0444 | 0.3478 | 9.5e-05 | 0.99% | 74秒 |
| `20bb_cd_s1` | 30k | 1 | 0.0615 | 0.0796 | 0.1048 | 0.0255 | 0.0398 | 0.0438 | 0.3550 | 1.2e-04 | 0.87% | 72秒 |
| `20bb_300k_s0` | 300k | 0 | 0.0079 | 0.0139 | 0.0164 | 0.0055 | 0.0047 | 0.0047 | 0.0531 | 2.7e-07 | 0.20% | 71秒 |
| `20bb_300k_s1` | 300k | 1 | 0.0077 | 0.0118 | 0.0129 | 0.0076 | 0.0053 | 0.0064 | 0.0516 | 5.0e-07 | 0.18% | 70秒 |
| `uniform` | - | - | 2.5113 | 2.4251 | 2.4538 | 3.1513 | 2.9941 | 2.7179 | 16.2535 | 0.0e+00 | 36.26% | 76秒 |

| variant of 20bb_300k_s0 | max |Δg_i| | Δ NashConv | identical seats |
|---|---|---|---|
| `20bb_300k_s0_threads8` | 0.00e+00 | +0.00e+00 | True |
| `20bb_300k_s0_k4seed1` | 5.31e-05 | +1.37e-05 | False |
| `20bb_300k_s0_t3seed1` | 1.67e-05 | +4.20e-05 | False |

## 20bb_cd: top local gains
- BTN 33 after `fold / fold / fold`: 0.00070 (profile [1.00, 0.00, 0.00], best `raise-to:20000:all-in`)
- BTN A4o after `fold / fold / fold`: 0.00058 (profile [0.98, 0.01, 0.01], best `raise-to:2500`)
- BTN AJo after `fold / fold / fold`: 0.00058 (profile [0.00, 0.08, 0.92], best `raise-to:2500`)
- SB KJo after `fold / fold / fold / fold / raise-to:2500 / raise-to:20000:all-in`: 0.00073 (profile [0.00, 1.00], best `fold`)
- SB KQo after `fold / fold / fold / fold`: 0.00056 (profile [0.00, 0.87, 0.13], best `raise-to:20000:all-in`)
- SB AJo after `fold / raise-to:2500 / fold / fold`: 0.00055 (profile [0.00, 0.01, 0.01, 0.98], best `call:2000`)
- BB A5o after `fold / fold / fold / fold / raise-to:20000:all-in`: 0.00039 (profile [0.00, 1.00], best `fold`)
- BB AJo after `raise-to:2500 / fold / fold / fold / fold`: 0.00031 (profile [0.00, 0.42, 0.12, 0.46], best `call:1500`)
- BB 98o after `fold / fold / fold / fold / raise-to:2500`: 0.00031 (profile [0.00, 0.99, 0.00, 0.01], best `raise-to:20000:all-in`)
- UTG AA after `root`: 0.00114 (profile [0.00, 0.98, 0.02], best `raise-to:20000:all-in`)
- UTG KK after `root`: 0.00102 (profile [0.00, 0.99, 0.01], best `raise-to:20000:all-in`)
- UTG AJo after `root`: 0.00092 (profile [0.00, 0.82, 0.18], best `raise-to:2500`)
- HJ A9o after `fold`: 0.00128 (profile [0.74, 0.21, 0.05], best `raise-to:2500`)
- HJ A8o after `fold`: 0.00082 (profile [0.98, 0.02, 0.00], best `raise-to:2500`)
- HJ AJo after `raise-to:20000:all-in`: 0.00067 (profile [0.00, 1.00], best `fold`)
- CO AJo after `fold / fold`: 0.00120 (profile [0.00, 0.19, 0.81], best `raise-to:2500`)
- CO KTo after `fold / fold`: 0.00093 (profile [0.85, 0.14, 0.00], best `raise-to:2500`)
- CO A7o after `fold / fold`: 0.00093 (profile [0.79, 0.20, 0.01], best `raise-to:2500`)
## 20bb_cd_s1: top local gains
- BTN A9o after `fold / fold / fold`: 0.00087 (profile [0.00, 0.54, 0.46], best `raise-to:2500`)
- BTN QTo after `fold / fold / fold`: 0.00071 (profile [1.00, 0.00, 0.00], best `raise-to:2500`)
- BTN K9o after `fold / fold / fold`: 0.00070 (profile [0.87, 0.12, 0.01], best `raise-to:2500`)
- SB ATo after `fold / fold / raise-to:2500 / fold`: 0.00055 (profile [0.00, 0.21, 0.28, 0.51], best `call:2000`)
- SB ATo after `fold / fold / fold / raise-to:20000:all-in`: 0.00044 (profile [0.00, 1.00], best `fold`)
- SB AJo after `fold / raise-to:2500 / fold / fold`: 0.00042 (profile [0.00, 0.03, 0.33, 0.64], best `call:2000`)
- BB JTo after `fold / fold / fold / fold / raise-to:2500`: 0.00035 (profile [0.00, 0.92, 0.00, 0.08], best `raise-to:20000:all-in`)
- BB AJo after `raise-to:20000:all-in / fold / fold / fold / fold`: 0.00034 (profile [0.15, 0.85], best `fold`)
- BB T9o after `fold / fold / fold / fold / raise-to:2500`: 0.00027 (profile [0.00, 1.00, 0.00, 0.00], best `raise-to:20000:all-in`)
- UTG AKo after `root`: 0.00206 (profile [0.00, 0.01, 0.99], best `raise-to:2500`)
- UTG KQo after `root`: 0.00133 (profile [0.00, 0.77, 0.23], best `raise-to:2500`)
- UTG 55 after `root`: 0.00056 (profile [0.89, 0.08, 0.03], best `raise-to:2500`)
- HJ AJo after `raise-to:2500`: 0.00159 (profile [0.00, 0.00, 0.00, 1.00], best `call:2500`)
- HJ KJo after `fold`: 0.00124 (profile [0.44, 0.39, 0.16], best `raise-to:2500`)
- HJ KQo after `fold`: 0.00103 (profile [0.00, 0.76, 0.24], best `raise-to:2500`)
- CO 88 after `fold / fold`: 0.00076 (profile [0.00, 0.00, 1.00], best `raise-to:2500`)
- CO ATo after `raise-to:2500 / fold`: 0.00075 (profile [0.14, 0.36, 0.49, 0.01], best `fold`)
- CO AQo after `fold / raise-to:2500`: 0.00073 (profile [0.00, 0.00, 0.21, 0.79], best `call:2500`)
## 20bb_300k_s0: top local gains
- BTN JTo after `fold / fold / fold`: 0.00029 (profile [1.00, 0.00, 0.00], best `raise-to:20000:all-in`)
- BTN 98s after `fold / fold / fold`: 0.00023 (profile [1.00, 0.00, 0.00], best `raise-to:20000:all-in`)
- BTN T8s after `fold / fold / fold`: 0.00017 (profile [1.00, 0.00, 0.00], best `raise-to:20000:all-in`)
- SB AKo after `fold / fold / fold / fold`: 0.00046 (profile [0.00, 0.92, 0.08], best `raise-to:20000:all-in`)
- SB ATo after `fold / fold / fold / fold`: 0.00040 (profile [0.00, 0.89, 0.11], best `raise-to:20000:all-in`)
- SB AQo after `fold / fold / fold / fold`: 0.00035 (profile [0.00, 0.88, 0.12], best `raise-to:20000:all-in`)
- BB T9o after `fold / fold / fold / fold / raise-to:2500`: 0.00017 (profile [0.00, 0.90, 0.00, 0.10], best `raise-to:20000:all-in`)
- BB QJo after `fold / fold / raise-to:2500 / fold / fold`: 0.00014 (profile [0.00, 0.96, 0.00, 0.04], best `raise-to:20000:all-in`)
- BB 98o after `fold / fold / fold / fold / raise-to:2500`: 0.00013 (profile [0.00, 0.94, 0.00, 0.06], best `raise-to:20000:all-in`)
- UTG AKo after `root`: 0.00030 (profile [0.00, 0.96, 0.04], best `raise-to:20000:all-in`)
- UTG A9o after `root`: 0.00016 (profile [0.58, 0.42, 0.00], best `raise-to:2500`)
- UTG 66 after `root`: 0.00012 (profile [0.84, 0.16, 0.00], best `raise-to:2500`)
- HJ AKo after `fold`: 0.00026 (profile [0.00, 0.80, 0.20], best `raise-to:20000:all-in`)
- HJ 99 after `fold`: 0.00011 (profile [0.00, 0.76, 0.24], best `raise-to:2500`)
- HJ KTo after `fold`: 0.00010 (profile [0.48, 0.52, 0.00], best `raise-to:2500`)
- CO 55 after `fold / fold`: 0.00021 (profile [0.71, 0.04, 0.25], best `raise-to:20000:all-in`)
- CO AQo after `raise-to:2500 / fold`: 0.00021 (profile [0.00, 0.04, 0.29, 0.67], best `call:2500`)
- CO AKo after `fold / fold`: 0.00020 (profile [0.00, 0.48, 0.52], best `raise-to:20000:all-in`)
## 20bb_300k_s1: top local gains
- BTN 88 after `fold / fold / fold`: 0.00019 (profile [0.00, 0.00, 1.00], best `raise-to:2500`)
- BTN 98s after `fold / fold / fold`: 0.00015 (profile [0.99, 0.00, 0.01], best `raise-to:20000:all-in`)
- BTN 77 after `fold / raise-to:2500 / fold`: 0.00015 (profile [0.00, 0.10, 0.00, 0.90], best `call:2500`)
- SB 98o after `fold / fold / fold / fold`: 0.00028 (profile [1.00, 0.00, 0.00], best `raise-to:20000:all-in`)
- SB JTo after `fold / fold / fold / fold`: 0.00024 (profile [0.00, 0.88, 0.12], best `raise-to:20000:all-in`)
- SB T9o after `fold / fold / fold / fold`: 0.00024 (profile [0.02, 0.98, 0.01], best `raise-to:20000:all-in`)
- BB 44 after `fold / fold / fold / raise-to:20000:all-in / fold`: 0.00013 (profile [1.00, 0.00], best `call:19000:all-in`)
- BB AJo after `fold / fold / raise-to:20000:all-in / fold / fold`: 0.00012 (profile [0.00, 1.00], best `fold`)
- BB AJo after `fold / fold / fold / raise-to:2500 / fold`: 0.00011 (profile [0.00, 0.00, 0.00, 1.00], best `raise-to:7500`)
- UTG KTo after `root`: 0.00048 (profile [1.00, 0.00, 0.00], best `raise-to:2500`)
- UTG A8o after `root`: 0.00026 (profile [1.00, 0.00, 0.00], best `raise-to:2500`)
- UTG 66 after `root`: 0.00026 (profile [0.88, 0.12, 0.00], best `raise-to:2500`)
- HJ KTo after `fold`: 0.00036 (profile [1.00, 0.00, 0.00], best `raise-to:2500`)
- HJ A8o after `fold`: 0.00019 (profile [0.76, 0.24, 0.00], best `raise-to:2500`)
- HJ 99 after `fold`: 0.00018 (profile [0.00, 0.53, 0.47], best `raise-to:2500`)
- CO AKo after `fold / fold`: 0.00014 (profile [0.00, 0.50, 0.50], best `raise-to:20000:all-in`)
- CO QJo after `fold / fold`: 0.00012 (profile [0.43, 0.57, 0.00], best `raise-to:2500`)
- CO AJo after `raise-to:2500 / fold`: 0.00012 (profile [0.88, 0.08, 0.01, 0.03], best `call:2500`)
