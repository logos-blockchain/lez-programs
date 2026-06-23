# TWAP Oracle — `RecordTick` and the zkVM Cycle Budget

> **Status: fixed.** `OBSERVATIONS_CAPACITY` was reduced from 6396 to **2048**, bringing `RecordTick`
> to ~16.8M cycles (~half the limit). The rest of this document is the diagnosis that led there; the
> numbers labelled "cap 6396" are the pre-fix measurements that motivated the change.

At capacity 6396, `RecordTick` could not run on chain: a single call over a full-size observations
buffer cost **~50.9M zkVM cycles**, over the **~33.5M (2²⁵)** public-execution limit, and the runtime
aborted it. This document explains what a cycle is, why the limit exists, what the measurements
actually show (the cause is **not** what it first looks like), and the fix.

It is backed by a runnable benchmark — `programs/benchmark/tests/twap_cycle_bench.rs` — and
by two end-to-end tests in `tests/amm.rs`
(`amm_twap_observations_accumulate_*`, `amm_twap_record_tick_sampling_guard_*`).

---

## 1. What a "cycle" is

LEZ programs run inside the RISC Zero zkVM — a **proven RISC-V (rv32im) virtual machine**. A *cycle*
is one step of that virtual CPU: essentially one executed RISC-V instruction (most cost 1 cycle, a
few cost more, plus some fixed overhead). "50M cycles" ≈ "~50 million instruction-steps," **not** a
unit of wall-clock time.

The prover turns execution into an arithmetic trace with **one row per cycle**, and proving cost —
time and memory — scales roughly **linearly with cycle count**. So the cap is fundamentally an
**economic/latency bound on how large a computation the network will prove for one public call.**

The executor reports cycles in a few buckets (visible in the benchmark output):

- **user cycles** — the RISC-V instructions the guest logic actually runs.
- **paging cycles** — the zkVM's memory is a Merkle-committed image of ~1 KiB pages; touching a
  page hashes it in/out. Large buffers page many pages. (In practice ~3–4% here.)
- **reserved cycles** — padding up to the proof system's power-of-two boundaries.

## 2. The limit

`nssa` sets the executor's `session_limit` to
`MAX_NUM_CYCLES_PUBLIC_EXECUTION = 1024 * 1024 * 32 = 33_554_432 = 2²⁵` cycles
(`nssa/src/program.rs`). Every program invocation — public or chained — runs under this cap via the
single `Program::execute` path; when a run reaches it, the executor aborts with
`Session limit exceeded: 33554432 >= 33554432`, which `nssa` surfaces as `ProgramExecutionFailed`.

## 3. What the measurements show

All numbers below are total cycles from the RISC Zero executor running the **real** `twap_oracle`
guest ELF, reproducing `Program::execute`'s exact input encoding (see the benchmark). They were
cross-checked against accounts extracted from a live `V03State`, which match to the cycle.

```
                                     total cycles     vs 2²⁵ limit
CreatePriceObservations (cap 6396)     17_301_504      ok
RecordTick, owned account  (cap 6396)  50_855_936      OVER  ← aborts on chain
RecordTick, UNOWNED account (cap 6396) 17_825_792      ok
```

The first surprise: **`RecordTick`'s Borsh work is not the problem.** With the observations account
left *uninitialized* (`program_owner = 0`) the same instruction over the same 102,388-byte buffer
costs only **17.8M** — comfortably under budget. The full deserialize + mutate + reserialize round
trip is the cheap part.

The actual driver is **account ownership**. The benchmark holds everything else fixed and flips only
the observations account's `program_owner`:

```
obs_owner = 0       → 17_825_792
obs_owner = oracle  → 50_855_936   (+33.0M, 2.9×)
```

(The `current_tick` account's owner makes no difference — it's only 12 bytes.)

### Why ownership costs ~33M cycles

When a program touches an **initialized (owned)** account, the runtime cryptographically binds that
account's state into the proof — on both sides: the **pre-state it read** and the **post-state it
wrote**. That work is proportional to the account's serialized size. For a max-size ~100 KiB account
it is **~16–17M cycles per side**.

That single fact explains the whole picture:

| instruction | owned ~100 KiB account is… | owned commits | total |
|---|---|---|---|
| `CreatePriceObservations` | **written** only (input is uninitialized) | 1× (write) | 17.3M ✅ |
| `RecordTick` | **read and written** | 2× (read + write) | 50.9M ❌ |

`CreatePriceObservations` pays the commit once and fits; `RecordTick` pays it twice and blows the
budget. Everything else (Borsh, paging, the tick arithmetic) is secondary.

### Cost scales linearly with capacity

`OBSERVATIONS_CAPACITY = 6396` exists to fill the 100 KiB account ceiling. Because both the commit
cost and the Borsh floor scale with account size, `RecordTick`'s cost scales ~linearly with
capacity:

```
RecordTick (owned)   total cycles     vs 2²⁵
cap   512              4_259_840       ok
cap  1024              8_388_608       ok
cap  2048             16_777_216       ok
cap  4096             32_538_624       ok  (only ~3% headroom)
cap  6396             50_855_936       OVER
```

The largest power-of-two capacity that fits is **4096**, but only barely. **2048 (≈16.8M, ~2×
headroom)** or smaller is a safe target.

### Empty buffers cost exactly the same

Fill level is irrelevant. An effectively-empty buffer (just created, `write_index = 1`, one used
entry) and an all-non-zero buffer of the same capacity cost **identical** cycles:

```
empty buffer  total = 50_855_936
filled buffer total = 50_855_936   (cap 6396, both 102,388 bytes)
```

The account is allocated at full size the moment `CreatePriceObservations` runs (it writes all
`OBSERVATIONS_CAPACITY` entries up front), so a brand-new, "empty" feed already pays the full cost.
The cost tracks **allocated size**, not how much meaningful data is stored. Reducing capacity is
therefore the only lever — you can't dodge it by keeping the buffer sparse.

### The overhead is per-byte, not a flat per-account tax

Measuring owned vs. uninitialized across account sizes, the owned-account overhead is **linear in
size at ~320 cycles per byte** (for the read + write the account gets in `RecordTick`; ~160/byte per
side):

```
  cap    bytes |       owned     unowned       delta   cyc/byte
   32      564 |      524_288      262_144      262_144      465*
  256    4_148 |    2_359_296    1_048_576    1_310_720      316
 1024   16_436 |    8_388_608    3_145_728    5_242_880      319
 2048   32_820 |   16_777_216    5_767_168   11_010_048      335
 4096   65_588 |   32_538_624   11_534_336   21_004_288      320
 6396  102_388 |   50_855_936   17_825_792   33_030_144      323
(* tiny accounts round up to the executor's 2^18-cycle segment quantum)
```

So you do **not** lose a fixed slice of the 2²⁵ budget for every owned account — you lose
~320 cycles per byte of owned account you **read-modify-write** (~160/byte if you only read, or only
write). For ordinary accounts this is noise: a ~50-byte token holding costs ~16 K cycles
(<0.05% of budget). It only becomes significant in the tens-of-KB range.

### This is a general size/cycle tension, not a TWAP quirk

The practical consequence: with ~320 cyc/byte for a read-modify-write plus the Borsh/IO floor, the
**largest owned account a single instruction can read-modify-write within budget is ~65 KB** — and
at that size there is essentially no budget left for real work. Any program attempting to
read-modify-write a near-max-size (100 KiB) owned account hits the same wall TWAP did, needing
~50 M cycles against a ~33.5 M cap.

In other words, `DATA_MAX_LENGTH = 100 KiB` and `MAX_NUM_CYCLES_PUBLIC_EXECUTION = 2²⁵` are **not
jointly satisfiable for full-size read-modify-write**. The runtime permits 100 KiB accounts, but the
cycle budget can't commit one on both the read and write side. The implicit design rule is: large
accounts must be **read-only, write-only, or paged** per instruction — never fully rewritten in
place. That's a normal ZK-rollup constraint, but it's currently unstated.

This is worth raising with the LEZ runtime team as a protocol-parameter question. Three levers, none
free:

- **Lower `DATA_MAX_LENGTH`** to a size that is committable *and* leaves room to compute (e.g. so a
  full read-modify-write fits well under budget). Safest guarantee, but caps every program's account
  size — and read-only consumers of large accounts don't need it lowered.
- **Raise `MAX_NUM_CYCLES_PUBLIC_EXECUTION`** so a max-size account is affordable. Directly inflates
  proving time/cost for *every* program, including ones that never touch big accounts.
- **Leave both, document the rule** that large accounts are not full-rewrite-able in one call, and
  provide a paging pattern. Lowest blast radius; pushes complexity to programs that need big state.

The TWAP fix below (a smaller buffer) sidesteps the tension for this program regardless of which
lever the protocol ultimately picks.

## 4. The fix

The budget-breaking cost is **committing the owned ~100 KiB account**, which is intrinsic to the
account's *size*. The fix must shrink what gets committed.

### ✅ Reduce `OBSERVATIONS_CAPACITY` — the simple, effective fix

Cutting capacity reduces the committed account size, and cost falls ~linearly. Critically, **window
coverage is unaffected**: the sampling guard derives `min_interval = window_duration / capacity`, so
coverage = `capacity × min_interval = window_duration` regardless of capacity — only *resolution*
(samples per window) drops. At capacity 2048 a 24 h window still samples every ~42 s; a 7 d window
every ~5 min. That is ample for a TWAP.

**Applied:** `OBSERVATIONS_CAPACITY = 2048` (≈16.8M cycles, ~2× headroom). 4096 fits but leaves no
margin for runtime variation; 2048 keeps `RecordTick` at roughly half the limit.

### ✅ Linked observation pages — if full resolution is required

Already sketched in `twap-oracle-observation-capacity.md`: keep a small fixed-size "head" account
plus older page accounts. `RecordTick` then commits only the small head, so per-call cost is bounded
regardless of total history. More moving parts (page PDAs, chain-walking readers); reserve it for
when a single reduced-capacity account genuinely can't hold enough resolution.

### ❌ Byte-patching `RecordTick` to skip the Borsh round-trip — does NOT fix this

An earlier hypothesis was that the cost was the full-buffer Borsh deserialize/reserialize, and that
patching the serialized bytes in place would make it O(1). **The measurements refute this.** The
Borsh round trip is only part of the 17.8M *unowned* floor; the ~33M that breaks the budget is the
owned-account commitment, which the runtime performs regardless of how the guest computes the new
bytes. Byte-patching would shave a little off the floor and leave `RecordTick` at ~33M+ — still at
or over the edge. Avoid this as the primary fix; it addresses the wrong cost.

### ❌ Raising `MAX_NUM_CYCLES_PUBLIC_EXECUTION` — not a real fix

It's a platform-wide `nssa` constant; raising it inflates proving cost/time for every program and
only defers the wall, since the cost still scales with account size.

## 5. Reproducing

```sh
# Faithful cycle benchmark (synthetic inputs; reproduces the on-chain pass/fail at 2²⁵).
# `programs/benchmark` is a standalone crate, excluded from the workspace, so run it by manifest:
cargo test --manifest-path programs/benchmark/Cargo.toml -- --ignored --nocapture

# The end-to-end TWAP tests through the real zkVM path:
RISC0_DEV_MODE=1 cargo test -p integration_tests --test amm twap
```

The benchmark uses `risc0-zkvm` with the `prove` feature to run the guest with the session limit
lifted and read the `user/paging/reserved/total` cycle split. It only *executes* the guest — it
never proves. It lives in the workspace-excluded `programs/benchmark` crate so normal
builds/tests/CI never compile it.

## 6. Acceptance — met

With `OBSERVATIONS_CAPACITY = 2048`:

- `twap_cycle_bench` reports `RecordTick (owned, cap 2048)` at ~16.8M, under the 2²⁵ limit (and its
  sweep still shows cap 6396 aborting, guarding against bumping capacity back up).
- The two `tests/amm.rs` TWAP tests pass through the real zkVM path (no longer `#[ignore]`d).
