# GTO Wizard (GTOW) postflop trees, accuracy targets, simplification cost, and solver benchmarks

Research date: 2026-10-09. Every factual claim cites a URL. Lines marked **[INFERENCE]** are my reasoning and are not sourced. "% pot" means percent of the pot at the start of the solved street or tree, as each source defines it.

---

## 0. Executive summary

- **GTOW has never published a full per-node spec for its current cash library trees.** The only detailed public spec is the original 2020 tree, which is the tree later sold as "Basic". The current tiers are documented only roughly: Complex has 12–19 sizings, General 4–8, Simple 3–6, Simplified 1–2 for hero, and the 2026 "Single Size" tier uses one optimal size per node. Smallest flop c-bets: General 33% and Basic 27% in BTN vs BB SRP; General 3BP 33%; Complex adds smaller sizes. Sources: [status page](https://blog.gtowizard.com/status-and-info-about-our-solutions/), [why-doesn't-my-solution-match](https://blog.gtowizard.com/why-doesnt-my-solution-match-gto-wizard/), [Single Size launch](https://blog.gtowizard.com/single-size-solutions-are-live-new-pricing-50x-more-solutions/).
- **GTOW accuracy targets (Nash Distance, % of starting pot):**
  - Presolved library: "at least 0.3%". Low-SPR 3BP/4BP spots are often 0.1%.
  - Rivers: re-solved in real time to 0.1%.
  - GTO Wizard AI flop solutions: 0.21% average (2023), then 0.165% (Nov 2024), then **0.12%** (Apr 2025 onward). Most spots fall in 0.06–0.18%.
  - "Simplified" library: 0.005% (4BP) to 0.045% (SRP).
  - Sources: [AI benchmarks](https://blog.gtowizard.com/gto-wizard-ai-benchmarks/), [Simplified launch](https://blog.gtowizard.com/simplified-solutions-and-a-new-interface/).
  - **P1's target of NashConv/2 ≤ 0.1% pot is in line with GTO Wizard AI, and is 2–3× tighter than the presolved library.**
- **Simplifying the tree costs little EV, especially early in the hand.**
  - Flop: one Automatic size vs. fixed 40/67/127% lost ≤ 0.02 bb in UTG vs BB SRP (< 0.5% pot), and usually ≤ 0.01 bb ([source](https://blog.gtowizard.com/do-multiple-sizes-matter/)).
  - River: one dynamic size vs. an 8-bet/5-raise tree lost 0.30% pot on average ([source](https://blog.gtowizard.com/dynamic-sizing-benchmarks/)).
  - Best single fixed river size: about 75–100% pot IP and about 50% pot OOP (same source).
- **Benchmarks:**
  - On Pio's "3betpotFAST" preset, the published best for an exact CPU solver is postflop-solver (the Desktop Postflop build): **44 s to 0.1% on 6 threads and 27 s on 16 threads (Ryzen 3700X), 679 MB in i16 mode**. PioSOLVER's CFR took 60 s and 1.41 GB on 6 threads; GTO+ took 68 s and 705 MB ([source](https://github.com/b-inary/wasm-postflop)).
  - PioSOLVER 3.10 (July 2026) ships a new "V4" algorithm that is 1.5–2× faster, and up to about 4.6× faster on 5-size trees ([source](https://piosolver.com/blog/2026-07-01-new-alg), [data](https://piofiles.com/benchmark/v4)).
  - **That makes Pio V4 the up-to-date commercial baseline P1 should beat.**

---

## 1. GTOW tree settings (Question 1)

### 1.1 Library solution types (6-max cash, 100bb)

| Type | Postflop sizings | Accuracy (% pot) | Notes | Source |
|---|---|---|---|---|
| General | 4–8 | 0.075–0.3 (main spots ~0.2) | "Best solutions, using GTO preferred preflop sizes" (UTG/HJ 2x, CO 2.3x, BTN 2.5x, SB 3x) | [status](https://blog.gtowizard.com/status-and-info-about-our-solutions/) |
| General 2.5x | 4–8 | 0.2–0.3 | Every position opens 2.5x | same |
| Complex | 12–19 | 0.2–0.3 (FAQ: main spots 0.25–0.4) | "lots of postflop sizings" | same |
| Simple | 3–6 | 0.2–0.3 | Preflop: 3bet-or-fold vs opens, no 4bet all-in, no BvB limp | same |
| Basic | 4–8 | 0.3–0.4 (rare spots 0.8) | "basic postflop sizings"; matches the 2020 tree in §1.2 **[INFERENCE: same accuracy figures and the same 27% minimum c-bet]** | same; [why-doesn't-match](https://blog.gtowizard.com/why-doesnt-my-solution-match-gto-wizard/) |
| Simplified (NL50/NL500, 100bb) | 1–2 for hero; villain gets "a range of sizes" | **0.005 (4BP) to 0.045 (SRP)** | The chosen "simplified player" is IP or OOP | [Simplified launch](https://blog.gtowizard.com/simplified-solutions-and-a-new-interface/) |
| Single Size (Mar 2026) | 1 "optimal size per spot", chosen by Dynamic Sizing | not stated | Built with GTO Wizard AI; extends postflop coverage to every preflop spot | [Single Size launch](https://blog.gtowizard.com/single-size-solutions-are-live-new-pricing-50x-more-solutions/) |

Rake: NL500 is 5% with a 0.6 bb cap; NL50 is 5% with a 4 bb cap ([status](https://blog.gtowizard.com/status-and-info-about-our-solutions/)).

Sizes visible in published strategy screenshots and text (not a spec):

- **General BTN vs BB SRP**: flop c-bet minimum is **33%**. BTN c-bets about 53% aggregated over 1,755 flops ([why-doesn't-match](https://blog.gtowizard.com/why-doesnt-my-solution-match-gto-wizard/)). Larger sizes seen: 75% and 125% ([c-bet mechanics](https://blog.gtowizard.com/the-mechanics-of-c-bet-sizing/)).
- **Basic BTN vs BB**: flop c-bet minimum is **27%**, and BTN c-bets about 64% aggregated (same source).
- **Simple BTN vs BB**: an aggregate report shows 33%, 66% and 130–133% c-bets ([IP c-bet heuristics](https://blog.gtowizard.com/flop-heuristics-ip-c-betting-in-cash-games/)).
- **General vs Complex, SB vs BTN 3BP**: General's smallest bet is 33% pot. Complex "has many small bet sizes". Complex uses a 10 bb 3-bet and General a 12 bb 3-bet ([why-doesn't-match](https://blog.gtowizard.com/why-doesnt-my-solution-match-gto-wizard/)).
- **Simplified**: hero's only flop c-bet size is 33% (tombos21 on 2+2, seen as a search snippet only: [thread](https://forumserver.twoplustwo.com/15/poker-theory-amp-gto/gtowizard-1823569)).
- **Recreating a GTOW General sim in your own solver**: GTOW's instructions are to use 5.5 bb pot and 97.5 bb stack, rake 5% capped at 0.6 bb, "a betting tree similar to the solution, including the overbets on later streets", and **accuracy 0.3% pot** ([why-doesn't-match](https://blog.gtowizard.com/why-doesnt-my-solution-match-gto-wizard/)).

### 1.2 The 2020 published tree (the "Basic" generation) — the only per-node spec GTOW published

Source for everything in this subsection: [All you need to know about our solutions](https://blog.gtowizard.com/all-you-need-to-know-about-our-solutions/).

**General design rules**

- Three sizes (small, medium, large) in most nodes.
- Two raise sizes (small and large). The stated reason: some value hands want protection, others do not.
- The overbet was included so that a later re-solve could drop it on boards where it is unused. For example, J♠9♥3♣ overbets 14% of the time, A♥9♦2♠ only 1.5%.
- Donk options are kept narrow, and are dropped in re-solves where unused.

**SRP, blind defender vs an open from UTG/HJ/CO/BTN (pot 5.5 or 6 bb)**

| Node | Sizes |
|---|---|
| Flop donk (OOP) | 1.5 bb (27%) or 4 bb (72%); IP has 2 raise sizes. "Any small 20–35% and large 55–85% is appropriate." The large donk is rarely used. |
| Flop c-bet (IP) | 3 sizes: small, middle, overbet. [INFERENCE: 27/73/127%, matching the probe sizes and the stated 27% minimum c-bet]. 2 raise / check-raise sizes. |
| Turn probe / delayed c-bet | 27%, 73%, 127%; opponent gets 2 raise sizes |
| Turn donk | 29%, 76%; 2 raise sizes |
| Turn barrel | 25–30%, 70–80%, 140–170% (depends on flop size) |
| River | small, medium, large, all-in |
| Open vs a non-blind (IP) caller | Same as above, plus an OOP flop donk overbet (rarely used) |

**3-bet pot**

| Node | Sizes |
|---|---|
| Flop (both players) | 20%, 56%, 122% + all-in. "Appropriate" ranges are 15–30, 45–70 and 100–140%. IP rarely overbets and never shoves. |
| Turn probe / delayed c-bet | 20/56/122%; 2 raises; **vs the large bet, all-in only** |
| Turn donk | 15–25 / 50–70 / 100–120% |
| Turn barrel after a flop 20% bet is called | 19%, 57%, 113%, all-in |
| … after a flop 56% bet is called | 21%, 68%, all-in |
| … after a flop 122% bet is called | 23%, all-in |
| River | small, medium, large, all-in |

**4-bet pot**

| Node | Sizes |
|---|---|
| Flop | 13%, 38%, 67%, all-in. The 13% bet is "by far the most used" on dry boards. Donks are mostly removed. |
| Turn probe / delayed c-bet | 13/38/67% + all-in |
| Raises vs probe | vs small: small, large, all-in; vs medium: small + all-in; vs large: all-in only |
| Turn barrel after a flop 13% bet is called | 14%, 34%, 54%, all-in |
| … after a flop 38% bet is called | 18%, all-in |
| … after a flop 67% bet is called | all-in |

**Pattern [INFERENCE]**

- The number of non-all-in sizes shrinks as SPR drops.
- Once a non-all-in bet would commit most of the remaining stack, it is replaced by all-in.
- In the low-SPR branches above, the turn after a large flop bet allows only a small bet plus all-in.

### 1.3 GTO Wizard AI (custom solver) tree options

- **Sizing modes** ([help](https://help.gtowizard.com/how-to-build-custom-solutions/), [FAQ](https://help.gtowizard.com/custom-solving-faq/)):
  - **Automatic**: picks how many sizes to use, and which ones, from a candidate list. The candidate list "adjusts… based on the SPR" ([Dynamic Sizing](https://blog.gtowizard.com/dynamic-sizing-a-gto-breakthrough/)).
  - **Dynamic**: the user sets the number of bet/raise sizes and can optionally supply the candidate list.
  - **Fixed**: a classic solver tree that is never simplified.
  - **The Automatic candidate lists are not published.**
- **Dynamic Sizing limits**:
  - Dynamic 2.0 (June 2025) supports up to 3 bet and 2 raise sizes; 1.0 supported 2 bets and 1 raise.
  - 2.0 works by solving tiny depth-limited trees for every subset of sizes and keeping the subset with the lowest EV loss ([Dynamic 2.0](https://blog.gtowizard.com/introducing_dynamic_sizing_2/)).
  - Automatic always adds an all-in option on the river ([Do multiple sizes matter](https://blog.gtowizard.com/do-multiple-sizes-matter/)).
- **Size inputs**: % of pot, geometric (`e`), multiple of previous bet (`x`), or bb.
- **Inheritance**: IP inherits OOP's settings, and turn/river inherit flop's, unless toggled off. Help tip: "IP will almost always use a bet size of at least 50% or higher on the river" ([help](https://help.gtowizard.com/how-to-build-custom-solutions/)).
- **Advanced options** ([help](https://help.gtowizard.com/how-to-build-custom-solutions/)):
  - *Always add all-in (Dynamic)*.
  - *Force all-in threshold*: a bet or raise larger than X% of the **effective stack** becomes all-in. Example: 80% at 50 bb makes anything over 40 bb a shove. Set 100% to disable.
  - *Add all-in threshold*: add a shove if it is less than X% of the **pot**.
  - *Bet-size merging*: works top-down; two sizes merge if X% > (1+Higher)/(1+Lower) − 1. Example: 50% and 25% merge when X > 20%. Recommended 5–20%; the author uses about 12%.
  - **Default numeric values of these thresholds are not published.**
- **Limits**: SPR ≤ 100 "to ensure accuracy"; max stack/pot 999. Heads-up postflop only at launch ([FAQ](https://help.gtowizard.com/custom-solving-faq/)).
- **GTOW pro tips** ([help](https://help.gtowizard.com/how-to-build-custom-solutions/)): save separate settings per SRP/3BP/4BP; "smaller bet and raise sizes in 4bet pots, and larger bet and raise sizes in single-raised pots."

### 1.4 Equivalent semantics in other solvers (for matching P1's tree builder)

- **PioSOLVER** ([tree building](https://piosolver.com/docs/viewer/postflop_tree_building)):
  - Sizes are set per street, per player, for bet/raise/donk. Raises can be `Nx` (2x is the min-raise).
  - "Add all-in" only applies where at least one size already exists, and only if the shove is below the threshold (× pot).
  - *All-in threshold*: a bet that would put in more than X% of the **starting stack** becomes all-in.
  - Merging keeps the highest size within the threshold.
  - *Betting cap*: NL mode turns the last allowed bet into all-in.
  - "Use only one bet/raise size if there was a raise before" is available.
  - Same betting lines on every runout.
- **postflop-solver (Rust)** ([action_tree.rs](https://github.com/b-inary/postflop-solver/blob/main/src/action_tree.rs)):
  - `add_allin_threshold`: ratio of max bet to pot.
  - `force_allin_threshold`: SPR after the opponent's call is ≤ X; author recommends 0.1–0.2.
  - `merging_threshold`: same algorithm as Pio; recommended about 0.1.
  - WASM UI defaults: add 150%, force 20%, merge 10% ([store.ts](https://github.com/b-inary/wasm-postflop/blob/main/src/store.ts)).
- **GTOBase (a competitor library, useful as a second reference)** ([source](https://blog.gtobase.com/theory/overview-of-the-new-gto-poker-solutions-in-the-6-max-cash-library/)):
  - All bets 33/75/150%; raises 33%/100%; donk 33% only.
  - Limped pots: bets 33/100/200%, raises 50/100%.
  - Rivers: OOP 25/50/100/150%, IP 50/75/100/150%, raises 100/150%.
  - Switches to 25/50% when SPR becomes too low for 33/75%.

---

## 2. Accuracy GTOW targets (Question 2)

**Definition.**

- "Nash Distance (dEV)" is "the maximum potential EV loss of the current solution in big blinds divided by the pot". Example: 0.3% of a 5.5 bb pot is 0.0165 bb/hand, about 1.5 bb/100 ([AI benchmarks](https://blog.gtowizard.com/gto-wizard-ai-benchmarks/); [glossary](https://gtowizard.com/en/glossary/nash-distance-dev)).
- Pio reports "exploitable for" in chips per hand ([Pio technical details](https://site.piosolver.com/docs/technical_details)).
- In Pio logs, that number equals (MES_OOP + MES_IP − pot)/2, i.e. **NashConv/2**. Example: 5.900 + 6.229 − 12 = 0.129, and /2 = 0.064, which matches the logged "Exploitable for: 0.064" ([Jesolver log](http://jeskola.net/jesolver_beta/newbench.html)). So P1's NashConv/2 metric is directly comparable with Pio-style accuracy. **[INFERENCE: GTOW presolved targets use the same convention, since they solved with Pio-class CFR solvers.]**

**How the AI benchmark measures accuracy.**

- For 900 flops, GTOW nodelocked only the AI's **flop** strategy for one player in a traditional solver.
- It then measured (a) EV loss vs. the fixed GTO strategy, and (b) loss vs. a re-solved best counter-strategy, which is what GTOW calls Nash Distance ([AI benchmarks](https://blog.gtowizard.com/gto-wizard-ai-benchmarks/)).
- **[INFERENCE]** This is a flop-strategy, one-player metric. Later streets are re-solved rather than locked, so it is looser than a full-tree NashConv/2.

**Published numbers (% of starting pot)**

| Product / date | Nash Distance | Source |
|---|---|---|
| 2020 launch (Basic) | main spots 0.4, rare spots 0.8 | [2020 article](https://blog.gtowizard.com/all-you-need-to-know-about-our-solutions/) |
| 2021 General / Simple / Complex / Basic | 0.075–0.3 / 0.2–0.3 / 0.2–0.4 / 0.3–0.4 | [status](https://blog.gtowizard.com/status-and-info-about-our-solutions/) |
| 2022 statement | "typically 0.2%–0.3%" | [Nash distance](https://blog.gtowizard.com/understanding-nash-distance/) |
| Presolved (current statement) | ≥ 0.3; 3BP/4BP often 0.1 | [AI benchmarks](https://blog.gtowizard.com/gto-wizard-ai-benchmarks/) |
| Simplified library (2023) | 0.045 (SRP), 0.005 (4BP); "industry standard ~0.5" | [Simplified launch](https://blog.gtowizard.com/simplified-solutions-and-a-new-interface/) |
| Rivers (all) | re-solved live to 0.1 | [AI benchmarks](https://blog.gtowizard.com/gto-wizard-ai-benchmarks/); [QRE](https://blog.gtowizard.com/introducing-quantal-response-equilibrium-the-next-evolution-of-gto/) |
| AI flop, May 2023 – Nov 2024 | mean 0.21; most 0.15–0.30 | [AI benchmarks](https://blog.gtowizard.com/gto-wizard-ai-benchmarks/) |
| AI flop, Nov 2024 – Apr 2025 | mean 0.165; most 0.09–0.24 | same |
| AI flop, Apr 2025 onward (QRE engine) | **mean 0.12; most 0.06–0.18** | same; QRE article says it "solves to an exploitability of about 0.1% pot" |
| Spin & Go library overhaul | 0.1–0.017 | [Spins](https://blog.gtowizard.com/new-spins-solutions-study-plans-and-ev-comparison/) |
| GTO Hero (competitor) | ≤ 0.6, some 0.3 | [GTO Hero](https://gtohero.com/solutions) |

**Other relevant facts.**

- **Cost of accuracy.**
  - GTOW: "It takes about as much time to go from completely unsolved to 0.5% dEV as it does to go from 0.5% to 0.25% dEV", and "0.3% is almost identical to 0.15%" ([Nash distance](https://blog.gtowizard.com/understanding-nash-distance/)).
  - **[INFERENCE]** For postflop-solver's DCFR, the 0.3% → 0.1% step costs about 1.8× time (24.9 s → 44.4 s, [wasm-postflop](https://github.com/b-inary/wasm-postflop)), which is better than the 1/T rule of thumb.
- **Solver choice changes the equilibrium shown.**
  - GTOW, GTO+ and Pio give visibly different JT5 strategies, all within 0.3% ([why-doesn't-match](https://blog.gtowizard.com/why-doesnt-my-solution-match-gto-wizard/)).
  - **[INFERENCE] "Matching GTOW" should be judged on EV and exploitability, plus high-frequency actions, not on node-by-node frequencies.**
- **Custom solves are QRE, not plain NE.**
  - Since April 2025, every custom GTOW AI solve uses Quantal Response Equilibrium; presolved solutions still use Nash.
  - QRE gives well-defined answers in zero-frequency "ghostlines". GTOW scores this with "Tree Payoff Weighted Loss": on the river, the NE solver plateaus around 0.38% while QRE reaches 0.01%.
  - QRE is 0.32 s vs 0.36 s for NE on typical rivers, but 7 s vs 18 s on very large ones ([QRE](https://blog.gtowizard.com/introducing-quantal-response-equilibrium-the-next-evolution-of-gto/)).
  - **[INFERENCE]** P1 is exact NE, so its ghost-line strategies will differ from GTOW AI's. Compare P1 with GTOW presolved (NE) solutions, or only on nodes reached with meaningful probability.
- **Solver noise threshold.** GTOW flags actions taken under 3.5% frequency as "inaccuracy" ([Nash distance](https://blog.gtowizard.com/understanding-nash-distance/)).

---

## 3. EV cost of simplifying trees (Question 3)

| Study | Setup | Result | Source |
|---|---|---|---|
| Flop c-bet, 1 size vs 3 | UTG vs BB SRP 100bb cash. Complex: UTG c-bets 40/67/127%, raise 50%. Simple: Automatic single size. | Most flops within **0.01 bb**; worst **0.02 bb** (< 0.5% pot, about 2 bb/100). Q93r loses the most per hand class (e.g. QTs 3.96 → 3.84 bb), but this is offset elsewhere. | [Do multiple sizes matter](https://blog.gtowizard.com/do-multiple-sizes-matter/) |
| Turn barrel, 1 size vs 4 | Same spot, A♥K♥9♦, after a 40% flop bet | Similar to the flop. Biggest loss is on the K♠ turn: complex bets 40% (31.6% of range) and 200% (14.7%); weaker Kx lose about 0.6 bb (8.9 → 8.3). | same |
| River, 1 size (+ all-in) vs 5 | Same line | Consistent EV gain for the complex tree; largest on a K♣ river (small bet + shove both wanted) | same |
| River, dynamic 1 size vs 8 bets + 5 raises | 500 rivers from 100bb HU self-play, solved to 0.05%; one player simplified and maximally exploited; donk spots excluded | Mean loss **0.30% pot** vs complex (keeps 99.7% of EV); 0.05% vs the best single size. Optimal size picked 78% of the time; within 0.25% EV 95% of the time; typical loss 0.1–0.5%, rarely > 1%. | [Dynamic Sizing benchmarks](https://blog.gtowizard.com/dynamic-sizing-benchmarks/) |
| Best fixed single river size | same | **IP 75–100% pot, OOP ~50% pot** | same |
| Dynamic 2.0 | 100 rivers, same method | Lower mean and worst-5% loss than 1.0, especially deep-stacked; no numbers given in text | [Dynamic 2.0](https://blog.gtowizard.com/introducing_dynamic_sizing_2/) |
| Flop 2 vs 3 c-bet sizes (Pio) | 2020 tree design | "EV is the same whether using 2 sizes or 3 sizes as long as you choose appropriate flop sizes"; overbets can be dropped on dry A-high boards (1.5% usage) | [2020 article](https://blog.gtowizard.com/all-you-need-to-know-about-our-solutions/) |
| Slumbot match, 7 s/hand | 1-size dynamic vs complex | 19.4 vs 13.1 bb/100. A simpler tree converges further within a fixed time budget. | [Dynamic Sizing benchmarks](https://blog.gtowizard.com/dynamic-sizing-benchmarks/) |
| Single-size flop overbet | BTN vs BB SRP | When restricted to one size, the solver picks **253% pot** on AK6r with about 1/3 of its range | [Flop overbet](https://blog.gtowizard.com/the_art_of_the_flop_overbet_and_why_youre_probably_doing_it_wrong/) |

**Takeaways for tree design [INFERENCE from the rows above].**

- Flop size count barely moves EV, but it multiplies tree size. Turn sizes matter more than river sizes for flop strategy ("river complexity has a smaller effect on the flop than turn complexity", [why-doesn't-match](https://blog.gtowizard.com/why-doesnt-my-solution-match-gto-wizard/)).
- Rivers benefit most from a second, very different size: a small or thin size plus all-in or overbet.
- Close sizes such as 40% vs 67% "aren't generally worth the hassle" ([Do multiple sizes matter](https://blog.gtowizard.com/do-multiple-sizes-matter/)).

---

## 4. Published speed / memory benchmarks (Question 4)

### 4.1 Pio "3betpotFAST" preset, all-in threshold set to 100%

Source: [wasm-postflop README](https://github.com/b-inary/wasm-postflop). Ryzen 7 3700X; PioSOLVER Free is limited to 6 threads. Desktop = postflop-solver native (DCFR, γ = 3, cumulative strategy reset at powers of 4, f32 or i16 compression, isomorphism; [postflop-solver](https://github.com/b-inary/postflop-solver)).

**6 threads**

| Solver | 0.5% | 0.3% | 0.1% | Memory |
|---|---|---|---|---|
| Desktop Postflop f32 | 20.0 s | 24.9 s | 44.4 s | 1.27 GB |
| Desktop Postflop i16 | 19.8 s | 24.7 s | **44.0 s** | **679 MB** |
| WASM f32 / i16 | 33.4 / 42.2 s | 41.2 / 52.3 s | 71.9 / 92.6 s | 1.25 GB / 660 MB |
| Pio CFR (2.0.8) | 22.9 s | 28.2 s | 60.1 s | 1.41 GB |
| Pio original algorithm | 30.3 s | 42.4 s | 108.4 s | 634 MB |
| GTO+ 1.5.0 | 22.0 s | 31.4 s | 67.7 s | 705 MB |
| TexasSolver 0.2.0 | 103.5 s | 149.0 s | 285.9 s | 2.84 GB |

**16 threads**

| Solver | 0.5% | 0.3% | 0.1% | Memory |
|---|---|---|---|---|
| Desktop f32 | 12.6 s | 15.6 s | 27.9 s | 1.27 GB |
| Desktop i16 | 12.2 s | 15.1 s | **27.0 s** | 679 MB |
| GTO+ | 13.9 s | 19.7 s | 41.7 s | 705 MB |
| TexasSolver | 67.1 s | 95.9 s | 182.6 s | 2.84 GB |

Results at 0.1%: bet frequency is 55.2% for WASM, Pio and GTO+, and EV is 105.1 for each. TexasSolver differed (63% bet).

**The preset's exact bet sizes are not reproduced in the README.** [INFERENCE: it ships as a sample script with PioSOLVER.]

### 4.2 PioSOLVER 3.10 "V4" vs Pio CFR (July 2026)

- Pio says V4 is 1.5–2× faster on average. The gap grows with tree size and with tighter accuracy targets.
- V4 comes in two variants: `v4_large` (2× memory) and `v4_small` (more CPU). On 16+ cores with dual-channel RAM, speed is limited by memory bandwidth.
- Save files are 10–17× smaller ([Pio blog](https://piosolver.com/blog/2026-07-01-new-alg)).
- Per-spot data ([piofiles benchmark](https://piofiles.com/benchmark/v4), raw `data.js`): AMD EPYC 32-core, 8×32 GB DDR4. Times below are Pio CFR / V4 in seconds.
  - **[INFERENCE: units are seconds; the page does not label them.]**
  - Tree specs beyond the labels are not published.

| Tree (label), board, pot | 0.5% | 0.2% | 0.1% | 0.05% |
|---|---|---|---|---|
| 3-bet pot (rainbow), Js7h3d, 180 | 3.3 / 3.8 | 6.5 / 5.1 | 9.6 / 11.5 | 13.8 / 36.9 |
| 3BP 3 sizes + 2 raises, Ks4h4d, 180 | 67.5 / 24.4 | 102 / 42.7 | 119 / 67.2 | 183 / 104 |
| Cap, 3 sizes, Js8s3s, 45 | 44.3 / 29.4 | 61.5 / 41.3 | 117.7 / 59.0 | 258.6 / 94.6 |
| HU SRP, Js7h3d, 50 | 130.8 / 84.8 | 258.4 / 153.1 | 339.1 / 221.4 | 671.9 / 615.3 |
| HU SRP, Js9s4h, 50 | 100.8 / 59.6 | 150.3 / 83.6 | 332.0 / 119.6 | 421.7 / 203.9 |
| BU vs BB large, Js8s3s, 1365 | 23.5 / 15.6 | 37.3 / 27.4 | 61.3 / 35.3 | 90.6 / 58.8 |
| MTT 40bb BTN vs BB, Js9s4h, 600 | 271 / 159 | 536 / 223 | 924 / 351 | 1182 / 542 |
| 3BP SB vs BU, 5 sizes, Js9s4h, 2300 | 205 / 69 | 414 / 110 | 559 / 194 | 778 / — |
| 2BP BB vs CO, 5 sizes, Js8s3s, 550 | 687 / 267 | 2938 / 420 | 3260 / 611 | — / — |
| 2BP BB vs CO, 5 sizes, Ks4h4d, 550 | 2286 / 590 | — / 959 | — / 1476 | — / — |

Notes on the table:

- "—" means the target was not reached within the time budget.
- In some cases only Pio CFR reached 0.05–0.1% (e.g. 3BP 5 sizes on Ks4h4d).
- **[INFERENCE]** Large multi-size SRP trees are where both Pio algorithms struggle, and where P1's DCFR (no reset) plus i16 storage can differentiate.

### 4.3 Fully specified, reproducible benchmark trees

1. **TexasSolver vs Pio 1.0** ([TexasSolver](https://github.com/bupticybee/TexasSolver), [config](https://github.com/bupticybee/TexasSolver/blob/master/benchmark/benchmark_texassolver.txt)).
   - Tree: pot 10, effective stack 95 (SPR 9.5), board Qs Jh 2h. Every street, both players: bet 100% pot, raise 50%, all-in. Turn/river donk 100%. All-in threshold 1.0.
   - Results on 6 threads: Pio 1.0 reached 0.29% in 242 s using 492 MB; TexasSolver 0.1.0 reached 0.275% in 172 s using 1,600 MB.
2. **Jesolver "BigTree" vs PioSOLVER edge 1.10.21** (Sept 2020, i7-7800X 6c/12t; [page](http://jeskola.net/jesolver_beta/newbench.html), [script](http://jeskola.net/jesolver_beta/BigTree.txt)).
   - Tree: pot 12, effective stack 195 (SPR 16.25), board As 8s 4s, 6-max-like ranges.
   - Flop: bets 35/50/70/100% + all-in; raises 50% + all-in.
   - Turn/river: bets 33/57/80/130% + all-in; raises 50% + all-in. Turn donk 50% + all-in; river donk all-in only.
   - Cap 4 (NL mode), all-in threshold 67%, add all-in when ≤ 5× pot. Target 0.5% pot (0.06 chips).
   - Jesolver: 0.058 after 400 iterations, 418.9 s, peak 7.66 GB.
   - Pio: 0.059 after about 3,474 s, 6,378 MB. That is 8.3× faster for Jesolver, and 16.6× on 48-core EC2.
3. **Jesolver 2015 scripts**, time to 0.05% pot vs Pio 1.x: 3betpotFAST 7:03 vs 0:32; COsmall 32:47 vs 3:48; CAP2sizes 40:52 vs 10:02 ([Jesolver](http://jeskola.net/jesolver_beta/)). These are dated and use 2015 hardware.
4. **GTO Wizard AI vs Pio** ([AI benchmarks](https://blog.gtowizard.com/gto-wizard-ai-benchmarks/)).
   - Spot: 100bb HU SRP, A♠7♥3♦, "95 GB tree".
   - PioSolver: 4,862 s to 0.23% on 16 cores @ 5 GHz with 128 GB.
   - GTOW AI: 6 s to 0.22% on 2 cores with 8 GB.
   - **Not comparable to P1**: GTOW AI is depth-limited with neural-network leaf values ([AI explained](https://blog.gtowizard.com/gto-wizard-ai-explained/)), which the product excludes.
   - GTOW AI averages about 3 s per street.
5. **Pio memory references** ([Pio technical details](https://site.piosolver.com/docs/technical_details)):
   - Full SRP with 2/3-pot bets everywhere: 1.2 GB with 6-max ranges, 1.9 GB with wide HU ranges.
   - 100bb tree, 2 sizes everywhere, 6-max ranges: 7.8 GB.
   - 25bb HU tree with 30%/60% everywhere: 5.9 GB.
   - Pio's typical study accuracy: "something like 1 bb/100".

---

## 5. Recommended P1 benchmark / template trees

All trees below are **[INFERENCE]**: they are built from GTOW's published sizes (§1) plus common solver conventions (§1.4).

### Conventions

- **Bet sizes**: % of the pot at the start of the betting round.
- **Raise sizes**: Pio-style "% pot" raise, where raise-to = call amount + X% × (pot after calling). Write `Nx` for a multiple of the facing bet.
- **All-in rules** (all must hold):
  - Force all-in if a bet/raise would commit more than **67% of the remaining effective stack**. This is the threshold Pio scripts use ([BigTree](http://jeskola.net/jesolver_beta/BigTree.txt)) and the role of GTOW's force-all-in threshold.
  - Add an all-in option wherever the shove is **≤ 150% pot** (postflop-solver default).
  - On the river, always add all-in, as GTOW Automatic does.
- **Merging threshold**: 12% (GTOW recommendation).
- **Cap**: 4 bets per street in NL mode; the last allowed raise is all-in.
- **Accuracy target**: NashConv/2 ≤ 0.1% of starting pot.
- **Rake**: chip-EV for speed work; 5% capped at 0.6 bb for NL500 parity with GTOW.
- **Ranges**: use the GTOW General NL500 100bb ranges for the formation (copyable from GTOW's Ranges tab, per [why-doesn't-match](https://blog.gtowizard.com/why-doesnt-my-solution-match-gto-wizard/)).
- **Boards**: run at least 4 textures — dry rainbow, two-tone connected, monotone, paired. For example, use Pio's set: Js7h3d, Js8s3s, Js9s4h, Ks4h4d ([piofiles](https://piofiles.com/benchmark/v4)).

### B0 — Speed-parity trees (published competitor numbers exist)

- **B0a "TexasSolver tree"**: pot 10, stack 95, Qs Jh 2h. Every street, both players: bet 100%, raise 50%, plus all-in; turn/river donk 100%. Target 0.3% and 0.1%. Compare with Pio 1.0 (242 s, 492 MB to 0.29%) and TexasSolver (172 s, 1.6 GB) on 6 threads.
- **B0b "BigTree"**: exact Jesolver script (§4.3). Targets 0.5% and 0.1%. Compare with Jesolver (419 s, 7.66 GB to 0.5%) and Pio edge 1.10 (3,474 s, 6.4 GB) on 12 threads.
- **B0c "3betpotFAST"**: load the preset from a PioSOLVER Free install and set its all-in threshold to 100%. Compare with postflop-solver: 44.0 s / 679 MB on 6 threads and 27.0 s on 16 threads to 0.1% (Ryzen 3700X).

### T1 — GTOW-like SRP, BTN vs BB, 100bb (main template)

Pot 5.5 bb, effective stack 97.5 bb, SPR ≈ 17.7.

| Street | OOP (BB, caller) | IP (BTN, preflop raiser) |
|---|---|---|
| Flop | Donk: off. Or, on the "donk" variant only, 1 size of 33%: GTOW's donk is a small "block" bet, and the large donk is rarely used. Check-raise: 2 sizes (small ≈ 50% pot-raise, large ≈ 100%) + all-in by threshold. | C-bet 33%, 75%, 125%, matching visible General sizes. Facing a check-raise: 1 raise (all-in via threshold, or 50%). |
| Turn | After calling the flop: donk 33% / 75% (Basic used 29/76%). After the flop checks through: probe 33%, 75%, 125% (Basic 27/73/127%). Raise 2 sizes (50%, 100%). | Barrel 33%, 75%, 150% (Basic: 25–30 / 70–80 / 140–170%). Delayed c-bet 33%, 75%, 125%. Raise 2 sizes (50%, 100%). |
| River | Bets 33%, 75%, 125% + all-in; donk 33/75% + all-in. Raise 50% + all-in. | Bets 33% (optional), 75%, 125% + all-in. Raise 50% + all-in. |

Variants:

- **T1-S ("Simple")**: flop 33/75/125 → 33/125; all turns 33/100 (barrel 33/150); rivers 50/100 + all-in; 1 raise size + all-in.
- **T1-1 ("Single-size")**: one size per node: flop 33%, turn 75%, river 75% IP and 50% OOP, plus all-in; raise all-in only. This reflects GTOW's finding on the best fixed river sizes ([Dynamic benchmarks](https://blog.gtowizard.com/dynamic-sizing-benchmarks/)).
- **T1-C ("Complex", stress test)**: flop c-bet 25/33/50/75/100/125%; turn/river 33/50/75/100/150% + all-in; 2 raises.

### T2 — GTOW-like 3BP, SB (3-bettor, OOP) vs BTN, 100bb

Pot ≈ 25 bb (12 bb 3-bet called, + 1 bb dead), effective stack 88 bb, SPR ≈ 3.5.

- **Flop**, both players: 20% (or 33% to mirror General), 56%, 122%, plus all-in by the 150%-pot add rule. IP: no all-in node unless created by the threshold.
- **Facing a bet**: 2 raises (small and large); facing the 122% bet, all-in only.
- **Turn** (Basic rules):
  - After a 20% flop bet is called: 19/57/113% + all-in.
  - After 56% is called: 21/68% + all-in.
  - After 122% is called: 23% + all-in.
  - Probe/delayed: 20/56/122%.
  - Donk: 20/60% (Basic: 15–25 / 50–70 / 100–120%).
- **River**: small (≈ 25%), medium (≈ 60%), all-in; or geometric `e` + all-in.

### T3 — GTOW-like 4BP, BTN (4-bettor) vs SB 3-bettor, 100bb

Pot ≈ 50 bb (4-bet to ≈ 24.5 bb), stack ≈ 75 bb, SPR ≈ 1.5.

- **Flop**: 13%, 38%, 67%, all-in. Donk off.
- **Raises**: vs 13%: one small raise + all-in; vs 38%: all-in only; vs 67%: all-in only.
- **Turn**:
  - After 13% is called: 14/34/54% + all-in.
  - After 38% is called: 18% + all-in.
  - After 67% is called: all-in.
  - Probe: 13/38/67% + all-in.
- **River**: all-in, plus one small size (≈ 33%) if the shove is more than 150% pot.

### T4 — River-only and turn-only micro-trees (kernel / regression)

- **River**: 8 bet sizes (10, 25, 33, 50, 75, 100, 150, 200%) + all-in, and 5 raises (33, 50, 75, 100% + all-in). This mirrors GTOW's "complex" river reference used to measure simplification loss ([Dynamic benchmarks](https://blog.gtowizard.com/dynamic-sizing-benchmarks/)).
  - Targets: 0.05% pot (GTOW's reference accuracy) and 0.1% (GTOW's live river re-solve target).
  - GTOW's QRE engine solves typical rivers in about 0.3 s and very large ones in 7–18 s ([QRE](https://blog.gtowizard.com/introducing-quantal-response-equilibrium-the-next-evolution-of-gto/)).
- **Turn**: T1's turn and river layers from a fixed flop line, to measure the cost of turn sizes in isolation.

### Acceptance metrics per tree

Report for each tree:

- Time to NashConv/2 = 0.3% / 0.1% / 0.05% of starting pot.
- Peak RSS.
- Thread count and hardware.
- EV of OOP and IP.
- Root strategy frequencies (bet % per size).

For GTOW parity, compare P1 with presolved GTOW General (NE) solutions on root and high-frequency nodes, with tolerance set by both solutions' accuracies. Ignore ghost lines, because custom GTOW AI solves are QRE.
