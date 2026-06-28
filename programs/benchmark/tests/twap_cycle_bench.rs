//! Cycle-cost benchmark for the TWAP oracle's account writes.
//!
//! This runs the real `twap_oracle` guest ELF directly through the RISC Zero executor (no proving)
//! with the session limit lifted, so we can measure the zkVM cycle cost of instructions that exceed
//! the on-chain `MAX_NUM_CYCLES_PUBLIC_EXECUTION = 32 MiCycles` budget — `RecordTick` in
//! particular, which aborts under the normal runtime and therefore can't be measured through
//! `nssa`'s `transition_from_public_transaction`.
//!
//! It reproduces `nssa::program::Program::execute`'s input encoding (four `env.write` calls:
//! program id, caller program id, pre-states, instruction words) and reports the executor's
//! user / paging / reserved / total cycle split per scenario.
//!
//! Ignored by default (it executes the guest several times and prints a report). Run with:
//!
//! ```sh
//! cargo test --manifest-path programs/benchmark/Cargo.toml -- --ignored --nocapture
//! ```

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::ProgramId,
};
use risc0_zkvm::{default_executor, serde::to_vec, ExecutorEnv, ExecutorImpl};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_price_observations_pda, CurrentTickAccount,
    Instruction, ObservationEntry, PriceObservations, OBSERVATIONS_CAPACITY,
};

/// The on-chain public-execution cycle ceiling (`MAX_NUM_CYCLES_PUBLIC_EXECUTION` in `nssa`).
const PUBLIC_EXECUTION_CYCLE_LIMIT: u64 = 1024 * 1024 * 32;

/// A 24-hour window in milliseconds; `min_interval = WINDOW / OBSERVATIONS_CAPACITY ≈ 13.5 s`.
const WINDOW_MS: u64 = 24 * 60 * 60 * 1_000;

const PRICE_SOURCE_BYTES: [u8; 32] = [7u8; 32];
/// Timestamp of the most recent observation already in the buffer.
const LAST_OBSERVATION_TS: u64 = 1_000_000;

#[derive(Clone, Copy)]
struct Cycles {
    user: u64,
    paging: u64,
    reserved: u64,
    total: u64,
}

fn program_id() -> ProgramId {
    twap_oracle_methods::TWAP_ORACLE_ID
}

fn price_source_id() -> AccountId {
    AccountId::new(PRICE_SOURCE_BYTES)
}

fn clock_account(timestamp: u64) -> AccountWithMetadata {
    let data = ClockAccountData {
        block_id: 0,
        timestamp,
    }
    .to_bytes();
    AccountWithMetadata {
        account: Account {
            data: Data::try_from(data).expect("clock data fits"),
            ..Account::default()
        },
        is_authorized: false,
        account_id: CLOCK_01_PROGRAM_ACCOUNT_ID,
    }
}

/// Builds a fully-populated `PriceObservations` with `capacity` entries, as `RecordTick` would find
/// it on-chain (the real account always holds `OBSERVATIONS_CAPACITY` entries; smaller values are
/// synthetic, used only to trace the cost-vs-size curve).
fn observations_account(capacity: usize) -> AccountWithMetadata {
    let mut entries = vec![ObservationEntry::default(); capacity];
    entries[0] = ObservationEntry {
        timestamp: LAST_OBSERVATION_TS,
        tick_cumulative: 0,
    };
    let observations = PriceObservations {
        price_source_id: price_source_id(),
        write_index: 1,
        total_entries: 1,
        last_recorded_tick: 100,
        entries,
    };
    AccountWithMetadata {
        account: Account {
            // Owned by the oracle: this is an *initialized* account on chain. Ownership is what
            // makes the runtime commit the full ~100 KiB to the proof — the dominant cost, and the
            // reason `RecordTick` (which reads this account) exceeds the budget while
            // `CreatePriceObservations` (whose input is uninitialized) does not.
            program_owner: program_id(),
            data: Data::from(&observations),
            ..Account::default()
        },
        is_authorized: false,
        account_id: compute_price_observations_pda(program_id(), price_source_id(), WINDOW_MS),
    }
}

fn current_tick_account() -> AccountWithMetadata {
    let current = CurrentTickAccount {
        tick: 250,
        last_updated: LAST_OBSERVATION_TS,
    };
    AccountWithMetadata {
        account: Account {
            program_owner: program_id(),
            data: Data::from(&current),
            ..Account::default()
        },
        is_authorized: false,
        account_id: compute_current_tick_account_pda(program_id(), price_source_id()),
    }
}

/// Same as [`observations_account`] but left *uninitialized* (`program_owner = 0`), to isolate how
/// much of the cost is the runtime committing the owned account vs. the Borsh round-trip itself.
fn observations_account_unowned(capacity: usize) -> AccountWithMetadata {
    let mut meta = observations_account(capacity);
    meta.account.program_owner = [0u32; 8];
    meta
}

fn build_env<'a>(
    pre_states: &[AccountWithMetadata],
    instruction: &Instruction,
    session_limit: Option<u64>,
) -> ExecutorEnv<'a> {
    let instruction_data: Vec<u32> = to_vec(instruction).expect("instruction serializes");

    let mut builder = ExecutorEnv::builder();
    builder.write(&program_id()).expect("write program id");
    builder
        .write(&None::<ProgramId>)
        .expect("write caller program id");
    builder
        .write(&pre_states.to_vec())
        .expect("write pre-states");
    builder
        .write(&instruction_data)
        .expect("write instruction data");
    builder.session_limit(session_limit);
    builder.build().expect("env builds")
}

/// Runs the guest with the session limit lifted and returns the executor's cycle split.
fn run(pre_states: &[AccountWithMetadata], instruction: &Instruction) -> Cycles {
    let env = build_env(pre_states, instruction, None);
    let session = ExecutorImpl::from_elf(env, twap_oracle_methods::TWAP_ORACLE_ELF)
        .expect("loads ELF")
        .run()
        .expect("guest executes without panicking");

    Cycles {
        user: session.user_cycles,
        paging: session.paging_cycles,
        reserved: session.reserved_cycles,
        total: session.total_cycles,
    }
}

/// Runs the guest under a hard session limit — exactly as `nssa::program::Program::execute` does on
/// chain — and reports whether it completed or was aborted by the limit. This is the ground-truth
/// reproduction of the on-chain behaviour.
fn completes_under_limit(
    pre_states: &[AccountWithMetadata],
    instruction: &Instruction,
    limit: u64,
) -> bool {
    let env = build_env(pre_states, instruction, Some(limit));
    ExecutorImpl::from_elf(env, twap_oracle_methods::TWAP_ORACLE_ELF)
        .expect("loads ELF")
        .run()
        .is_ok()
}

/// Same as [`completes_under_limit`] but via `default_executor().execute()` — the EXACT entry point
/// `nssa::program::Program::execute` uses — to check whether the executor path (not just the input
/// encoding) accounts for the session limit differently than [`ExecutorImpl::run`].
fn completes_under_limit_via_default_executor(
    pre_states: &[AccountWithMetadata],
    instruction: &Instruction,
    limit: u64,
) -> bool {
    let env = build_env(pre_states, instruction, Some(limit));
    default_executor()
        .execute(env, twap_oracle_methods::TWAP_ORACLE_ELF)
        .is_ok()
}

/// `CreatePriceObservations` inputs: constructs and serializes the full buffer once (no
/// deserialize).
fn create_inputs() -> (Vec<AccountWithMetadata>, Instruction) {
    let observations = AccountWithMetadata {
        account: Account::default(), // must be uninitialized
        is_authorized: false,
        account_id: compute_price_observations_pda(program_id(), price_source_id(), WINDOW_MS),
    };
    let price_source = AccountWithMetadata {
        account: Account::default(),
        is_authorized: true, // caller must control the price source
        account_id: price_source_id(),
    };
    (
        vec![
            observations,
            price_source,
            clock_account(LAST_OBSERVATION_TS),
        ],
        Instruction::CreatePriceObservations {
            initial_tick: 0,
            window_duration: WINDOW_MS,
        },
    )
}

/// `RecordTick` inputs over a `capacity`-entry buffer. `elapsed_ms` selects the path: below
/// `min_interval` the sampling guard returns after deserializing only (no reserialize); above it
/// the full deserialize + mutate + reserialize write path runs.
fn record_inputs(capacity: usize, elapsed_ms: u64) -> (Vec<AccountWithMetadata>, Instruction) {
    (
        vec![
            observations_account(capacity),
            current_tick_account(),
            clock_account(LAST_OBSERVATION_TS + elapsed_ms),
        ],
        Instruction::RecordTick {
            price_source_id: price_source_id(),
            window_duration: WINDOW_MS,
        },
    )
}

fn pct(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64 * 100.0
    }
}

fn report(label: &str, c: Cycles) {
    let over = if c.total > PUBLIC_EXECUTION_CYCLE_LIMIT {
        "OVER"
    } else {
        "ok"
    };
    println!(
        "{label:<34} total={:>10}  user={:>10} ({:>4.1}%)  paging={:>10} ({:>4.1}%)  reserved={:>9}  [{over}]",
        c.total,
        c.user,
        pct(c.user, c.total),
        c.paging,
        pct(c.paging, c.total),
        c.reserved,
    );
}

#[test]
#[ignore = "cycle benchmark; run explicitly with --ignored --nocapture"]
fn twap_record_tick_cycle_budget_report() {
    let cap = OBSERVATIONS_CAPACITY as usize;
    let min_interval = WINDOW_MS / u64::from(OBSERVATIONS_CAPACITY);
    let limit = PUBLIC_EXECUTION_CYCLE_LIMIT;

    // ── Ground truth: reproduce the on-chain pass/fail under the real hard session limit. ──
    // `cap` is the live OBSERVATIONS_CAPACITY; after the capacity reduction both instructions fit.
    println!("\nUnder the on-chain hard limit (session_limit = {limit} = 2^25):\n");
    let (create_pre, create_instr) = create_inputs();
    let create_ok = completes_under_limit(&create_pre, &create_instr, limit);
    println!(
        "  CreatePriceObservations (cap {cap}): {}",
        verdict(create_ok)
    );

    let (write_pre, write_instr) = record_inputs(cap, min_interval * 4);
    let write_ok = completes_under_limit(&write_pre, &write_instr, limit);
    println!("  RecordTick            (cap {cap}): {}", verdict(write_ok));

    // The exact nssa entry point, to expose any executor-path accounting difference.
    let create_ok_de =
        completes_under_limit_via_default_executor(&create_pre, &create_instr, limit);
    let write_ok_de = completes_under_limit_via_default_executor(&write_pre, &write_instr, limit);
    println!(
        "  via default_executor(): create={}  record={}",
        verdict(create_ok_de),
        verdict(write_ok_de)
    );

    // ── Cycle breakdown (session limit lifted). ──
    println!("\nMeasured cycle split (session limit lifted):\n");
    let create = run(&create_pre, &create_instr);
    report(&format!("CreatePriceObservations (cap {cap})"), create);

    let write = run(&write_pre, &write_instr);
    report(&format!("RecordTick, owned account  (cap {cap})"), write);

    // Same instruction, same buffer bytes, but the input account is left uninitialized: isolates
    // the cost of committing the owned account from the Borsh round-trip.
    let (unowned_pre, unowned_instr) = (
        vec![
            observations_account_unowned(cap),
            current_tick_account(),
            clock_account(LAST_OBSERVATION_TS + min_interval * 4),
        ],
        Instruction::RecordTick {
            price_source_id: price_source_id(),
            window_duration: WINDOW_MS,
        },
    );
    let unowned = run(&unowned_pre, &unowned_instr);
    report(&format!("RecordTick, UNOWNED account (cap {cap})"), unowned);
    println!(
        "  -> committing the owned account costs ~{} cycles ({:.1}x).",
        write.total.saturating_sub(unowned.total),
        write.total as f64 / unowned.total as f64,
    );

    // Cost-vs-capacity curve. The list spans the reduced capacity through the original 6396, which
    // exceeds the budget — a guard against bumping OBSERVATIONS_CAPACITY back into the danger zone.
    println!("\nRecordTick (owned) cost vs buffer capacity:\n");
    for capacity in [512usize, 1024, 2048, 4096, 6396] {
        let (pre, instr) = record_inputs(capacity, WINDOW_MS / capacity as u64 * 4);
        let c = run(&pre, &instr);
        let ok = completes_under_limit(&pre, &instr, limit);
        report(&format!("RecordTick (cap {capacity}) [{}]", verdict(ok)), c);
    }
    println!();

    // After the capacity reduction, both instructions fit the on-chain budget.
    assert!(
        create_ok,
        "expected CreatePriceObservations to complete under the on-chain limit"
    );
    assert!(
        write_ok,
        "expected RecordTick (cap {cap}) to complete under the on-chain limit; \
         OBSERVATIONS_CAPACITY may be too large"
    );
    let _ = (create_ok_de, write_ok_de); // printed above for cross-checking the executor path
}

fn verdict(ok: bool) -> &'static str {
    if ok {
        "COMPLETES"
    } else {
        "ABORTED"
    }
}

/// An owned observations account whose `capacity` entries are ALL non-zero (a "full" ring buffer),
/// to test whether the commit cost depends on the data values / fill level or only on size.
fn observations_account_filled(capacity: usize) -> AccountWithMetadata {
    let entries = (0..capacity)
        .map(|i| ObservationEntry {
            timestamp: 1_000 + i as u64,
            tick_cumulative: -(i as i64) - 1,
        })
        .collect();
    let observations = PriceObservations {
        price_source_id: price_source_id(),
        write_index: 1,
        total_entries: capacity as u64,
        last_recorded_tick: -123,
        entries,
    };
    AccountWithMetadata {
        account: Account {
            program_owner: program_id(),
            data: Data::from(&observations),
            ..Account::default()
        },
        is_authorized: false,
        account_id: compute_price_observations_pda(program_id(), price_source_id(), WINDOW_MS),
    }
}

/// Serialized size of a `capacity`-entry observations account (52-byte header + 16 B/entry).
fn obs_bytes(capacity: usize) -> usize {
    52 + capacity * 16
}

/// Answers the follow-up questions:
///   a) empty vs filled ring buffer — does fill level matter, or only allocated size?
///   b) is the owned-account overhead proportional to size (a per-byte cost) or a flat per-account
///      tax? i.e. how much of the 2^25 budget does touching an owned account actually consume?
///   c) what is the largest account a program can *read-modify-write* within the budget?
#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn twap_owned_account_commit_cost() {
    let min_interval = WINDOW_MS / u64::from(OBSERVATIONS_CAPACITY);
    let elapsed = min_interval * 4;
    let cap = OBSERVATIONS_CAPACITY as usize;

    // (a) Same size, opposite fill: an effectively-empty buffer (write_index = 1, one used entry)
    //     vs an all-non-zero buffer. Both are fully allocated once created.
    println!(
        "\n(a) fill level, at cap {cap} (both {} bytes):",
        obs_bytes(cap)
    );
    let empty = run(
        &[
            observations_account(cap), // entries all default except [0]
            current_tick_account(),
            clock_account(LAST_OBSERVATION_TS + elapsed),
        ],
        &Instruction::RecordTick {
            price_source_id: price_source_id(),
            window_duration: WINDOW_MS,
        },
    );
    let filled = run(
        &[
            observations_account_filled(cap), // every entry non-zero
            current_tick_account(),
            clock_account(LAST_OBSERVATION_TS + elapsed),
        ],
        &Instruction::RecordTick {
            price_source_id: price_source_id(),
            window_duration: WINDOW_MS,
        },
    );
    println!("    empty buffer  total = {}", empty.total);
    println!("    filled buffer total = {}", filled.total);

    // (b)+(c) owned vs unowned across sizes → the per-byte commit cost.
    println!("\n(b) owned vs unowned RecordTick by account size:\n");
    println!(
        "{:>5} {:>8} | {:>11} {:>11} {:>11} {:>10}",
        "cap", "bytes", "owned", "unowned", "delta", "cyc/byte"
    );
    for capacity in [32usize, 256, 1024, 2048, 4096, 6396] {
        let owned = run(
            &[
                observations_account(capacity),
                current_tick_account(),
                clock_account(LAST_OBSERVATION_TS + elapsed),
            ],
            &Instruction::RecordTick {
                price_source_id: price_source_id(),
                window_duration: WINDOW_MS,
            },
        );
        let unowned = run(
            &[
                observations_account_unowned(capacity),
                current_tick_account(),
                clock_account(LAST_OBSERVATION_TS + elapsed),
            ],
            &Instruction::RecordTick {
                price_source_id: price_source_id(),
                window_duration: WINDOW_MS,
            },
        );
        let bytes = obs_bytes(capacity);
        let delta = owned.total.saturating_sub(unowned.total);
        println!(
            "{capacity:>5} {bytes:>8} | {:>11} {:>11} {:>11} {:>10.0}",
            owned.total,
            unowned.total,
            delta,
            delta as f64 / bytes as f64,
        );
    }
    println!();
}
