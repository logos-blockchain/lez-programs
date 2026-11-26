//! Cycle cost of the TWAP oracle's record and publish, on LEZ's actor model.
//!
//! A transaction's cycles are capped by its `gas_limit`, at most `MAX_GAS_EXEC`, and shared by
//! every handler in it, so the ground truth is the cycles a whole transaction is charged:
//! `RecordTick` runs the current-tick actor twice, the clock and the observations actor;
//! `PublishPrice` adds the price actor. Those come from
//! `ValidatedStateDiff::from_public_transaction_with_cycle_budget` over a state seeded with the
//! oracle, LEZ's clock and a key source's three actors. The observations actor's own step
//! (`SampleTick`, `ComputePrice`) is the one that scales with capacity; it is also run alone
//! through the executor for the user / paging split.
//!
//! Buffers larger than `OBSERVATIONS_CAPACITY` are synthetic: the guest still wraps at the
//! compiled capacity, so they hold few entries and are measured only for their size. Creating a
//! window builds the buffer at the compiled capacity, so it is measured there only.
//!
//! Ignored by default. Run with:
//!
//! ```sh
//! cargo test --release --manifest-path programs/benchmark/Cargo.toml -- --ignored --nocapture
//! ```

use std::{borrow::Cow, collections::BTreeMap};

use clock_core::{clock_account_id, clock_actor, ClockAccountData, CLOCK_PROGRAM_ACCOUNT_ID};
use fee_core::market::MAX_GAS_EXEC;
use lee::{
    program::Program,
    public_transaction::{Message as TxMessage, WitnessSet},
    PrivateKey, PublicKey, PublicTransaction, V03State, ValidatedStateDiff,
};
use lee_core::{
    account::{Account, AccountId, Actor, ActorState},
    program::ReceiveInput,
};
use risc0_zkvm::{ExecutorEnv, ExecutorImpl, ExitCode};
use twap_oracle_core::{
    compute_oracle_price_account_pda, compute_price_observations_pda, CurrentTickState, Internal,
    Message, ObservationEntry, OraclePrice, OracleState, PriceObservations,
    PriceSource, OBSERVATIONS_CAPACITY,
};
use twap_oracle_methods::TWAP_ORACLE_ELF;

const ORACLE: AccountId = AccountId::new([0xA0; 32]);
const BASE: AccountId = AccountId::new([1; 32]);
const QUOTE: AccountId = AccountId::new([2; 32]);
const T0: u64 = 1_700_000_000_000;
/// A 24-hour window; `min_interval = WINDOW_MS / OBSERVATIONS_CAPACITY`.
const WINDOW_MS: u64 = 24 * 60 * 60 * 1_000;
/// Lifted far above `MAX_GAS_EXEC`, to measure what doesn't fit.
const UNLIMITED: u64 = 1 << 36;

const CAPACITIES: [usize; 7] = [512, 1024, 2048, 4096, 6396, 8192, 16_384];

/// A key source: its current-tick actor lives on its account.
fn source_key() -> PrivateKey {
    PrivateKey::try_new([7; 32]).expect("a valid key")
}

fn account_of(key: &PrivateKey) -> AccountId {
    AccountId::from(&PublicKey::new_from_private_key(key))
}

fn source() -> AccountId {
    account_of(&source_key())
}

fn min_interval() -> u64 {
    WINDOW_MS / u64::from(OBSERVATIONS_CAPACITY)
}

fn oracle(account_id: AccountId) -> Actor {
    Actor::new(account_id, ORACLE)
}

fn observations_id() -> AccountId {
    compute_price_observations_pda(ORACLE, source(), WINDOW_MS)
}

fn price_id() -> AccountId {
    compute_oracle_price_account_pda(ORACLE, source(), WINDOW_MS)
}

fn window_actors() -> [Actor; 4] {
    [
        oracle(source()),
        clock_actor(),
        oracle(observations_id()),
        oracle(price_id()),
    ]
}

/// A `len`-entry buffer whose first `used` entries hold observations one interval apart.
fn observations(len: usize, used: usize) -> OracleState {
    let mut entries = vec![ObservationEntry::default(); len];
    for (i, entry) in entries.iter_mut().take(used).enumerate() {
        let i = i as u64;
        *entry = ObservationEntry {
            timestamp: T0 + i * min_interval(),
            tick_cumulative: (100 * i * min_interval()) as i64,
        };
    }
    OracleState::PriceObservations(PriceObservations {
        price_source_id: source(),
        current_tick: source(),
        window_duration: WINDOW_MS,
        write_index: used as u32,
        total_entries: used as u64,
        last_recorded_tick: 100,
        entries,
    })
}

fn price() -> OracleState {
    OracleState::Price(OraclePrice {
        base_asset: BASE,
        quote_asset: QUOTE,
        price: 1 << 64,
        timestamp: T0,
        source_id: source(),
        confidence_interval: 0,
    })
}

fn actor_state(state: &OracleState) -> Account {
    Account::default().with_actor_state(ORACLE, ActorState::from(state.encode()))
}

/// The oracle, the clock at `now`, and the key source's current-tick actor; no window yet.
fn with_current_tick(now: u64) -> V03State {
    let clock = ClockAccountData {
        block_id: 1,
        timestamp: now,
    };
    let current_tick = OracleState::CurrentTick(CurrentTickState {
        source: PriceSource::Key(source()),
        tick: 150,
        last_updated: T0,
        pending: None,
    });
    V03State::new()
        .with_named_programs([
            (
                ORACLE,
                Program::new(Cow::Borrowed(TWAP_ORACLE_ELF)).expect("the oracle guest loads"),
            ),
            (clock_account_id(), programs::clock()),
        ])
        .with_public_accounts([
            (
                CLOCK_PROGRAM_ACCOUNT_ID,
                Account::default().with_actor_state(clock_account_id(), clock.to_bytes().into()),
            ),
            (source(), actor_state(&current_tick)),
        ])
}

/// [`with_current_tick`] plus the window's actors, with a `len`-entry buffer.
fn chain(len: usize, used: usize, now: u64) -> V03State {
    with_current_tick(now).with_public_accounts([
        (observations_id(), actor_state(&observations(len, used))),
        (price_id(), actor_state(&price())),
    ])
}

/// A root message to the current-tick actor, signed by `signers` at nonce 0.
fn transaction(
    message: &Message,
    declared: &[Actor],
    signers: &[&PrivateKey],
) -> PublicTransaction {
    let nonces: BTreeMap<_, _> = signers
        .iter()
        .map(|key| (account_of(key), Default::default()))
        .collect();
    let message = TxMessage::try_new(oracle(source()), declared.iter().copied(), nonces, message)
        .expect("the transaction encodes");
    let witness_set = WitnessSet::for_message(&message, signers);
    PublicTransaction::new(message, witness_set)
}

fn record() -> PublicTransaction {
    transaction(
        &Message::RecordTick {
            window_duration: WINDOW_MS,
        },
        &[oracle(source()), clock_actor(), oracle(observations_id())],
        &[],
    )
}

fn publish() -> PublicTransaction {
    transaction(
        &Message::PublishPrice {
            window_duration: WINDOW_MS,
        },
        &window_actors(),
        &[],
    )
}

fn create_window() -> PublicTransaction {
    transaction(
        &Message::CreatePriceAccounts {
            window_duration: WINDOW_MS,
            base_asset: BASE,
            quote_asset: QUOTE,
        },
        &window_actors(),
        &[&source_key()],
    )
}

/// The cycles a whole transaction is charged, with the budget lifted.
fn charged(state: &V03State, tx: &PublicTransaction) -> u64 {
    ValidatedStateDiff::from_public_transaction_with_cycle_budget(tx, state, 1, 0, UNLIMITED)
        .map(|(_, charge)| charge.cycles)
        .expect("the transaction executes")
}

/// Whether the transaction completes within the most gas a transaction may declare.
fn fits(state: &V03State, tx: &PublicTransaction) -> bool {
    ValidatedStateDiff::from_public_transaction_with_cycle_budget(tx, state, 1, 0, MAX_GAS_EXEC)
        .is_ok()
}

struct Split {
    user: u64,
    paging: u64,
    total: u64,
}

/// The observations actor's step alone, through the executor.
fn observations_step(len: usize, used: usize, message: &Internal) -> Split {
    let input = ReceiveInput {
        receiver: oracle(observations_id()),
        from: Some(oracle(source())),
        is_authorized: false,
        pre_state: ActorState::from(observations(len, used).encode()),
        message: borsh::to_vec(message).expect("the message encodes"),
    };
    let mut builder = ExecutorEnv::builder();
    Program::write_receive_input(&input, &mut builder).expect("the input writes");
    builder.session_limit(Some(UNLIMITED));
    let session = ExecutorImpl::from_elf(builder.build().expect("env builds"), TWAP_ORACLE_ELF)
        .expect("the ELF loads")
        .run()
        .expect("the guest executes");
    assert_eq!(session.exit_code, ExitCode::Halted(0), "the step succeeds");
    Split {
        user: session.user_cycles,
        paging: session.paging_cycles,
        total: session.total_cycles,
    }
}

fn oracle_state(state: &V03State, id: AccountId) -> OracleState {
    OracleState::decode(state.get_account_by_id(id).data.actor_state(ORACLE))
        .expect("the actor exists")
}

fn mark(fits: bool) -> &'static str {
    if fits {
        "ok"
    } else {
        "OVER"
    }
}

#[test]
#[ignore = "cycle benchmark; run explicitly with --ignored --nocapture"]
fn twap_record_and_publish_cycle_report() {
    // The record lands four intervals after entry 0; the publish's tail is two intervals long.
    let record_now = T0 + 4 * min_interval();
    let publish_now = T0 + 3 * min_interval();
    let budget = MAX_GAS_EXEC;
    let cap = OBSERVATIONS_CAPACITY as usize;

    // Each measured transaction takes the path it claims.
    let mut state = with_current_tick(T0);
    state
        .transition_from_public_transaction(&create_window(), 1, 0)
        .expect("the window opens");
    assert!(matches!(
        oracle_state(&state, observations_id()),
        OracleState::PriceObservations(o) if o.entries.len() == cap
    ));
    let mut state = chain(cap, 1, record_now);
    state
        .transition_from_public_transaction(&record(), 1, 0)
        .expect("the record succeeds");
    assert!(matches!(
        oracle_state(&state, observations_id()),
        OracleState::PriceObservations(o) if o.total_entries == 2
    ));
    let mut state = chain(cap, 2, publish_now);
    state
        .transition_from_public_transaction(&publish(), 1, 0)
        .expect("the publish succeeds");
    assert!(matches!(
        oracle_state(&state, price_id()),
        OracleState::Price(p) if p.timestamp == publish_now
    ));

    let create_state = with_current_tick(T0);
    println!(
        "\nCreatePriceAccounts (cap {cap}): {} cycles [{}]",
        charged(&create_state, &create_window()),
        mark(fits(&create_state, &create_window())),
    );

    println!(
        "\nWhole transactions (MAX_GAS_EXEC = {budget}, shared by every handler):\n\n\
         {:>6} {:>8} | {:>12} {:>5} | {:>12} {:>5}",
        "cap", "bytes", "RecordTick", "", "PublishPrice", ""
    );
    for len in CAPACITIES {
        let bytes = observations(len, 1).encode().len();
        let record_state = chain(len, 1, record_now);
        let publish_state = chain(len, 2, publish_now);
        println!(
            "{len:>6} {bytes:>8} | {:>12} {:>5} | {:>12} {:>5}",
            charged(&record_state, &record()),
            mark(fits(&record_state, &record())),
            charged(&publish_state, &publish()),
            mark(fits(&publish_state, &publish())),
        );
    }

    println!(
        "\nThe observations actor's step alone (executor split):\n\n\
         {:>6} | {:>10} {:>10} {:>10} | {:>12} {:>10} {:>10}",
        "cap", "SampleTick", "user", "paging", "ComputePrice", "user", "paging"
    );
    for len in CAPACITIES {
        let sample = observations_step(
            len,
            1,
            &Internal::SampleTick {
                tick: 160,
                now: record_now,
            },
        );
        let compute = observations_step(
            len,
            2,
            &Internal::ComputePrice {
                tick: 160,
                last_updated: T0,
                now: publish_now,
            },
        );
        println!(
            "{len:>6} | {:>10} {:>10} {:>10} | {:>12} {:>10} {:>10}",
            sample.total,
            sample.user,
            sample.paging,
            compute.total,
            compute.user,
            compute.paging
        );
    }
    println!();

    assert!(
        fits(&create_state, &create_window()),
        "CreatePriceAccounts must fit the budget"
    );
    assert!(
        fits(&chain(cap, 1, record_now), &record()),
        "RecordTick at OBSERVATIONS_CAPACITY must fit the budget"
    );
    assert!(
        fits(&chain(cap, 2, publish_now), &publish()),
        "PublishPrice at OBSERVATIONS_CAPACITY must fit the budget"
    );
}
