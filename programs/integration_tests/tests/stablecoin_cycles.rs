//! What each stablecoin transaction is charged in zkVM cycles.
//!
//! A transaction's cycles are capped by its `gas_limit`, at most `MAX_GAS_EXEC` (10M), and shared
//! by every handler run in it; the wallet declares 2M by default. The charge is what
//! `ValidatedStateDiff::from_public_transaction_with_cycle_budget` reports for the whole
//! transaction with the budget lifted, as in `programs/benchmark/tests/twap_cycle_bench.rs`,
//! measured on the real guests just before each transaction is applied.
//!
//! The fee and redemption math compound over the time since the last poke, clamped to
//! `MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS` (7 days), so each measured transaction comes six days
//! after the previous one that moved the anchors.
//!
//! Ignored by default. Run with:
//!
//! ```sh
//! RISC0_DEV_MODE=1 cargo test -p integration_tests --test stablecoin_cycles -- --ignored --nocapture
//! ```

#[expect(
    dead_code,
    reason = "the harness is shared with tests/stablecoin.rs; this target uses part of it"
)]
#[path = "stablecoin/harness.rs"]
mod harness;

use std::cell::Cell;

use fee_core::market::MAX_GAS_EXEC;
use harness::*;
use integration_tests::tick;
use lee::{error::LeeError, PublicTransaction, V03State, ValidatedStateDiff};
use lee_core::program::TransactionEvent;

/// Lifted far above `MAX_GAS_EXEC`, to measure what does not fit.
const UNLIMITED: u64 = 1 << 36;
/// The wallet's default `gas_limit`.
const WALLET_DEFAULT_GAS_LIMIT: u64 = 2_000_000;
const SIX_DAYS: u64 = 6 * 24 * 60 * 60 * 1_000;

thread_local! {
    static CHARGED: Cell<Option<u64>> = const { Cell::new(None) };
}

/// The dry run every measured transaction must pass; a failure panics with its cause.
fn record_charge(state: &V03State, tx: &PublicTransaction) {
    let charged =
        ValidatedStateDiff::from_public_transaction_with_cycle_budget(tx, state, 1, 0, UNLIMITED)
            .map(|(_, charge)| charge.cycles)
            .unwrap_or_else(|error| panic!("the metered dry run fails: {error:?}"));
    CHARGED.with(|cell| cell.set(Some(charged)));
}

/// Runs `transaction` and returns what it was charged; it must succeed.
fn measure(
    name: &'static str,
    rows: &mut Vec<(&'static str, u64)>,
    transaction: impl FnOnce() -> Result<Vec<TransactionEvent>, LeeError>,
) {
    CHARGED.with(|cell| cell.set(None));
    transaction().unwrap_or_else(|error| panic!("{name} fails: {error:?}"));
    let cycles = CHARGED
        .with(Cell::get)
        .unwrap_or_else(|| panic!("{name} was not measured"));
    rows.push((name, cycles));
}

#[test]
#[ignore = "cycle measurement; run explicitly with --ignored --nocapture"]
fn stablecoin_transactions_fit_the_cycle_budget() {
    let mut world = world();
    world.observer = Some(record_charge);
    let mut rows = Vec::new();
    let mut now = T0;

    measure("Initialize", &mut rows, || try_initialize(&mut world));
    let user = user(&mut world, 30, 600_000_000);
    measure("OpenPosition (with collateral)", &mut rows, || {
        open_position(&mut world, &user, 300_000_000)
    });
    measure("DepositCollateral", &mut rows, || {
        deposit(&mut world, &user, 1_000)
    });
    measure("GenerateDebt (no time passed)", &mut rows, || {
        generate(&mut world, &user, 100_000_000)
    });

    now += SIX_DAYS;
    tick(&mut world.state, now);
    measure("GenerateDebt (6 days, surplus mint)", &mut rows, || {
        generate(&mut world, &user, 1_000_000)
    });

    now += SIX_DAYS;
    tick(&mut world.state, now);
    assert!(
        holding_balance(&world, surplus_id(&world)) > 0,
        "the surplus was minted"
    );
    measure("RepayDebt (6 days, surplus mint)", &mut rows, || {
        repay(&mut world, &user, 1_000_000)
    });

    now += SIX_DAYS;
    tick(&mut world.state, now);
    measure("WithdrawCollateral (with debt, 6 days)", &mut rows, || {
        withdraw(&mut world, &user, 1_000)
    });

    measure(
        "AccrueStabilityFee (6 days, surplus mint)",
        &mut rows,
        || accrue(&mut world),
    );

    now += SIX_DAYS;
    tick(&mut world.state, now);
    publish_price(&mut world, 2 * ORACLE_ONE);
    measure(
        "RefreshGlobals { consult_oracle: true } (6 days)",
        &mut rows,
        || refresh(&mut world, true),
    );

    assert_eq!(
        engine(&world).redemption.last_updated_at,
        now,
        "the refresh applied the oracle's price"
    );

    now += 1_000;
    tick(&mut world.state, now);
    measure("UpdateRedemptionRate", &mut rows, || {
        update_redemption_rate(&mut world)
    });

    println!(
        "\nCharged cycles per transaction (MAX_GAS_EXEC = {MAX_GAS_EXEC}, wallet default \
         {WALLET_DEFAULT_GAS_LIMIT}):\n"
    );
    println!(
        "| {:<50} | {:>10} | {:>12} | {:>9} |",
        "transaction", "cycles", "share of 10M", "under 2M"
    );
    for (name, cycles) in &rows {
        let per_mille = cycles.saturating_mul(1_000) / MAX_GAS_EXEC;
        println!(
            "| {name:<50} | {cycles:>10} | {:>9}.{}% | {:>9} |",
            per_mille / 10,
            per_mille % 10,
            if *cycles < WALLET_DEFAULT_GAS_LIMIT {
                "yes"
            } else {
                "no"
            },
        );
    }
    println!();

    for (name, cycles) in &rows {
        assert!(
            *cycles < MAX_GAS_EXEC,
            "{name} is charged {cycles} cycles, over MAX_GAS_EXEC"
        );
    }
}
