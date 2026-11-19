//! The TWAP oracle through its real guest, with LEZ's clock, ticked between blocks.
//!
//! A test price-source guest stands in for the AMM: its actor forwards what a root message asks,
//! with the seeds it is given. Key sources need nothing but a signature.

use borsh::{BorshDeserialize, BorshSerialize};
use clock_core::clock_actor;
use integration_test_methods::PRICE_SOURCE_ELF;
use integration_tests::{account, assert_refused, deploy_at, key, submit, tick, with_clock};
use lee::{error::LeeError, PrivateKey, V03State};
use lee_core::{
    account::{Account, AccountId, Actor},
    program::{PdaSeed, TransactionEvent},
};
use twap_oracle_core::{
    compute_current_tick_account_pda_seed, compute_oracle_price_account_pda,
    compute_price_observations_pda,
    event::{self, OracleEvent, Published, Recorded},
    tick_to_oracle_price, CurrentTickState, Message, OraclePrice, OracleState, PriceObservations,
    PriceReply, PriceSource, ReadPrice,
};
use twap_oracle_methods::TWAP_ORACLE_ELF;

const ORACLE: AccountId = AccountId::new([0xA0; 32]);
const SOURCE: AccountId = AccountId::new([0xA1; 32]);
const OTHER_SOURCE: AccountId = AccountId::new([0xA2; 32]);
const BASE: AccountId = AccountId::new([0xB0; 32]);
const QUOTE: AccountId = AccountId::new([0xB1; 32]);

const T0: u64 = 1_700_000_000_000;
/// Sampling interval 1000 ms; a tail may reach 4000 ms.
const WINDOW: u64 = 2_048_000;
const ONE: u128 = 1 << 64;

/// The price source guest's message (`methods/guest/src/bin/price_source.rs`).
#[derive(BorshSerialize, BorshDeserialize)]
enum SourceMessage {
    Forward {
        to: Actor,
        message: Vec<u8>,
        seeds: Vec<PdaSeed>,
    },
}

fn oracle(account_id: AccountId) -> Actor {
    Actor::new(account_id, ORACLE)
}

fn observations_id(current_tick: AccountId) -> AccountId {
    compute_price_observations_pda(ORACLE, current_tick, WINDOW)
}

fn price_id(current_tick: AccountId) -> AccountId {
    compute_oracle_price_account_pda(ORACLE, current_tick, WINDOW)
}

/// A program source: the price source guest's actor on a key's account (the key signs, which
/// admits the account).
struct Source {
    key: PrivateKey,
    program: AccountId,
}

impl Source {
    fn new(byte: u8) -> Self {
        Self::of(byte, SOURCE)
    }

    fn of(byte: u8, program: AccountId) -> Self {
        Self {
            key: key(byte),
            program,
        }
    }

    fn actor(&self) -> Actor {
        Actor::new(account(&self.key), self.program)
    }

    fn current_tick(&self) -> AccountId {
        PriceSource::program(self.actor()).current_tick_account_id()
    }

    fn seed(&self) -> PdaSeed {
        compute_current_tick_account_pda_seed(account(&self.key))
    }
}

struct Chain {
    state: V03State,
}

impl Chain {
    fn new() -> Self {
        let state = deploy_at(&[
            (ORACLE, TWAP_ORACLE_ELF),
            (SOURCE, PRICE_SOURCE_ELF),
            (OTHER_SOURCE, PRICE_SOURCE_ELF),
        ]);
        Self {
            state: with_clock(state, T0),
        }
    }

    fn tick(&mut self, timestamp: u64) {
        tick(&mut self.state, timestamp);
    }

    fn oracle_state(&self, id: AccountId) -> Option<OracleState> {
        OracleState::decode(self.state.get_account_by_id(id).data.actor_state(ORACLE))
    }

    fn current_tick(&self, id: AccountId) -> CurrentTickState {
        match self.oracle_state(id) {
            Some(OracleState::CurrentTick(state)) => state,
            other => panic!("not a current-tick actor: {other:?}"),
        }
    }

    fn observations(&self, id: AccountId) -> PriceObservations {
        match self.oracle_state(id) {
            Some(OracleState::PriceObservations(state)) => state,
            other => panic!("not an observations actor: {other:?}"),
        }
    }

    fn price(&self, id: AccountId) -> OraclePrice {
        match self.oracle_state(id) {
            Some(OracleState::Price(state)) => state,
            other => panic!("not a price actor: {other:?}"),
        }
    }

    /// `source`'s actor sends `message` to `to`, granting `seeds`. `declared` lists the actors
    /// past the source.
    fn forward(
        &mut self,
        source: &Source,
        to: Actor,
        message: Vec<u8>,
        seeds: Vec<PdaSeed>,
        declared: &[Actor],
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let mut actors = vec![source.actor()];
        actors.extend_from_slice(declared);
        submit(
            &mut self.state,
            source.actor(),
            &actors,
            &[&source.key],
            &SourceMessage::Forward { to, message, seeds },
        )
    }

    /// A root message to the oracle actor `to`.
    fn send(
        &mut self,
        to: AccountId,
        declared: &[Actor],
        signers: &[&PrivateKey],
        message: &impl BorshSerialize,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        submit(&mut self.state, oracle(to), declared, signers, message)
    }

    fn create(
        &mut self,
        source: &Source,
        initial_price: u128,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let message = encode(&Message::CreateCurrentTickAccount { initial_price });
        let t = source.current_tick();
        self.forward(
            source,
            oracle(t),
            message,
            vec![source.seed()],
            &[oracle(t)],
        )
    }

    fn update(&mut self, source: &Source, price: u128) -> Result<Vec<TransactionEvent>, LeeError> {
        let message = encode(&Message::UpdateCurrentTick { price });
        let t = source.current_tick();
        self.forward(
            source,
            oracle(t),
            message,
            vec![],
            &[oracle(t), clock_actor()],
        )
    }

    fn open_window(&mut self, source: &Source) -> Result<Vec<TransactionEvent>, LeeError> {
        let message = encode(&open_window());
        let t = source.current_tick();
        self.forward(source, oracle(t), message, vec![], &window_actors(t))
    }

    fn record(&mut self, current_tick: AccountId) -> Result<Vec<TransactionEvent>, LeeError> {
        let declared = [
            oracle(current_tick),
            clock_actor(),
            oracle(observations_id(current_tick)),
        ];
        self.send(
            current_tick,
            &declared,
            &[],
            &Message::RecordTick {
                window_duration: WINDOW,
            },
        )
    }

    fn publish(&mut self, current_tick: AccountId) -> Result<Vec<TransactionEvent>, LeeError> {
        let declared = window_actors(current_tick);
        self.send(
            current_tick,
            &declared,
            &[],
            &Message::PublishPrice {
                window_duration: WINDOW,
            },
        )
    }

    /// A program source with its current-tick actor at `initial_price` and a window opened at
    /// `T0`.
    fn with_window(source: &Source, initial_price: u128) -> Self {
        let mut chain = Self::new();
        chain
            .create(source, initial_price)
            .expect("creation succeeds");
        chain.open_window(source).expect("the window opens");
        chain
    }
}

fn open_window() -> Message {
    Message::CreatePriceAccounts {
        window_duration: WINDOW,
        base_asset: BASE,
        quote_asset: QUOTE,
    }
}

fn window_actors(current_tick: AccountId) -> [Actor; 4] {
    [
        oracle(current_tick),
        clock_actor(),
        oracle(observations_id(current_tick)),
        oracle(price_id(current_tick)),
    ]
}

fn encode(message: &impl BorshSerialize) -> Vec<u8> {
    borsh::to_vec(message).expect("the message encodes")
}

fn events_of<E: OracleEvent + BorshDeserialize>(events: &[TransactionEvent]) -> Vec<E> {
    let selector = event::selector(E::NAME);
    events
        .iter()
        .filter(|record| record.event.selector == selector)
        .map(|record| borsh::from_slice(&record.event.data).expect("the event decodes"))
        .collect()
}

/// The newest `(timestamp, cumulative)` of a buffer.
fn newest(observations: &PriceObservations) -> (u64, i64) {
    let index = usize::try_from(observations.write_index)
        .expect("write_index fits in usize")
        .checked_sub(1)
        .or_else(|| observations.entries.len().checked_sub(1))
        .expect("the buffer is not empty");
    let entry = observations
        .entries
        .get(index)
        .expect("index is within bounds");
    (entry.timestamp, entry.tick_cumulative)
}

// ── Creation and admission ───────────────────────────────────────────────────────

#[test]
fn a_program_sources_seed_grant_admits_its_current_tick_actor() {
    let source = Source::new(1);
    let mut chain = Chain::new();

    chain.create(&source, ONE).expect("creation succeeds");

    assert_eq!(
        chain.current_tick(source.current_tick()),
        CurrentTickState {
            source: PriceSource::program(source.actor()),
            tick: 0,
            last_updated: 0,
            pending: None,
        }
    );
}

#[test]
fn without_the_seed_the_current_tick_account_is_not_admitted() {
    let source = Source::new(1);
    let mut chain = Chain::new();
    let t = source.current_tick();
    let message = encode(&Message::CreateCurrentTickAccount { initial_price: ONE });

    let result = chain.forward(&source, oracle(t), message, vec![], &[oracle(t)]);

    assert!(
        format!("{result:?}").contains("UnadmittedPublicActor"),
        "expected an admission refusal, got {result:?}"
    );
    assert_eq!(chain.oracle_state(t), None);
}

#[test]
fn opening_a_window_creates_both_actors_at_the_clocks_time() {
    let source = Source::new(1);
    let mut chain = Chain::new();
    chain.create(&source, ONE << 1).expect("creation succeeds");
    let t = source.current_tick();
    let tick = chain.current_tick(t).tick;
    chain.tick(T0 + 1_000);

    chain.open_window(&source).expect("the window opens");

    let observations = chain.observations(observations_id(t));
    assert_eq!(observations.price_source_id, account(&source.key));
    assert_eq!(observations.current_tick, t);
    assert_eq!(observations.last_recorded_tick, tick);
    assert_eq!(
        (observations.total_entries, observations.write_index),
        (1, 1)
    );
    assert_eq!(newest(&observations), (T0 + 1_000, 0));

    let price = chain.price(price_id(t));
    assert_eq!(price.price, tick_to_oracle_price(tick));
    assert_eq!(price.timestamp, T0 + 1_000);
    assert_eq!(price.source_id, account(&source.key));
    assert_eq!((price.base_asset, price.quote_asset), (BASE, QUOTE));
    assert_eq!(
        chain.current_tick(t).pending,
        None,
        "the pending slot is cleared"
    );
}

#[test]
fn opening_a_window_twice_is_refused() {
    let source = Source::new(1);
    let mut chain = Chain::with_window(&source, ONE);

    let result = chain.open_window(&source);

    assert_refused(&result, "observations actor already exists");
}

// ── Recording and publishing ─────────────────────────────────────────────────────

#[test]
fn records_across_blocks_accumulate_into_a_time_weighted_average() {
    let source = Source::new(1);
    let mut chain = Chain::with_window(&source, ONE);
    let t = source.current_tick();

    chain.tick(T0 + 1_000);
    chain
        .update(&source, tick_to_oracle_price(100))
        .expect("the update succeeds");
    let tick = chain.current_tick(t).tick;
    assert_eq!(chain.current_tick(t).last_updated, T0 + 1_000);
    let events = chain.record(t).expect("the record succeeds");
    assert_eq!(
        events_of::<Recorded>(&events),
        vec![Recorded {
            price_observations: observations_id(t),
            window_duration: WINDOW,
            timestamp: T0 + 1_000,
            tick,
        }]
    );

    chain.tick(T0 + 2_000);
    chain.record(t).expect("the record succeeds");
    let observations = chain.observations(observations_id(t));
    assert_eq!(observations.total_entries, 3);
    assert_eq!(newest(&observations), (T0 + 2_000, i64::from(tick) * 2_000));

    let events = chain.publish(t).expect("the publish succeeds");
    let published = tick_to_oracle_price(tick);
    assert_eq!(chain.price(price_id(t)).price, published);
    assert_eq!(chain.price(price_id(t)).timestamp, T0 + 2_000);
    assert_eq!(
        events_of::<Published>(&events),
        vec![Published {
            oracle_price: price_id(t),
            window_duration: WINDOW,
            price: published,
            timestamp: T0 + 2_000,
        }]
    );
}

#[test]
fn a_second_record_in_the_same_block_adds_nothing() {
    let source = Source::new(1);
    let mut chain = Chain::with_window(&source, ONE);
    let t = source.current_tick();
    chain.tick(T0 + 1_000);

    chain.record(t).expect("the first record succeeds");
    let after_first = chain.observations(observations_id(t));
    let events = chain.record(t).expect("the second record succeeds");

    assert_eq!(chain.observations(observations_id(t)), after_first);
    assert!(events_of::<Recorded>(&events).is_empty());
}

#[test]
fn a_publish_extrapolates_the_tail_to_now() {
    let source = Source::new(1);
    let mut chain = Chain::with_window(&source, tick_to_oracle_price(200));
    let t = source.current_tick();
    let tick = chain.current_tick(t).tick;
    chain.tick(T0 + 1_000);
    chain.record(t).expect("the record succeeds");

    chain.tick(T0 + 4_000);
    chain.publish(t).expect("the publish succeeds");

    let price = chain.price(price_id(t));
    assert_eq!(price.price, tick_to_oracle_price(tick));
    assert_eq!(
        price.timestamp,
        T0 + 4_000,
        "stamped now, past the newest entry"
    );
}

#[test]
fn a_publish_with_one_observation_changes_nothing() {
    let source = Source::new(1);
    let mut chain = Chain::with_window(&source, ONE);
    let t = source.current_tick();
    let before = chain.price(price_id(t));
    chain.tick(T0 + 1_000);

    let events = chain.publish(t).expect("the publish succeeds");

    assert_eq!(chain.price(price_id(t)), before);
    assert!(events_of::<Published>(&events).is_empty());
}

#[test]
fn a_stale_feed_is_refused() {
    let source = Source::new(1);
    let mut chain = Chain::with_window(&source, ONE);
    let t = source.current_tick();
    chain.tick(T0 + 1_000);
    chain.record(t).expect("the record succeeds");

    chain.tick(T0 + 1_000 + 4_001);
    let result = chain.publish(t);

    assert_refused(
        &result,
        "observations are stale; call RecordTick before publishing",
    );
}

// ── Who may send what ────────────────────────────────────────────────────────────

#[test]
fn recording_from_a_program_is_refused() {
    let source = Source::new(1);
    let mut chain = Chain::with_window(&source, ONE);
    let t = source.current_tick();
    chain.tick(T0 + 1_000);
    let message = encode(&Message::RecordTick {
        window_duration: WINDOW,
    });

    let result = chain.forward(
        &source,
        oracle(t),
        message,
        vec![],
        &[oracle(t), clock_actor(), oracle(observations_id(t))],
    );

    assert_refused(&result, "RecordTick must be the transaction's root message");
}

#[test]
fn another_programs_actor_on_the_source_account_cannot_update() {
    let source = Source::new(1);
    let mut chain = Chain::with_window(&source, ONE);
    let impostor = Source::of(1, OTHER_SOURCE);
    let t = source.current_tick();
    let message = encode(&Message::UpdateCurrentTick { price: ONE << 4 });

    let result = chain.forward(
        &impostor,
        oracle(t),
        message,
        vec![],
        &[oracle(t), clock_actor()],
    );

    assert_refused(&result, "sender is not the price source");
}

#[test]
fn a_key_source_runs_the_whole_flow() {
    let source = key(20);
    let k = account(&source);
    let mut chain = Chain::new();

    chain
        .send(
            k,
            &[oracle(k)],
            &[&source],
            &Message::CreateCurrentTickAccount { initial_price: ONE },
        )
        .expect("a signed root creation succeeds");
    assert_eq!(chain.current_tick(k).source, PriceSource::Key(k));
    chain
        .send(k, &window_actors(k), &[&source], &open_window())
        .expect("a signed root opens the window");

    chain.tick(T0 + 1_000);
    chain
        .send(
            k,
            &[oracle(k), clock_actor()],
            &[&source],
            &Message::UpdateCurrentTick {
                price: tick_to_oracle_price(50),
            },
        )
        .expect("a signed root update succeeds");
    let tick = chain.current_tick(k).tick;
    chain.record(k).expect("anyone records");
    chain.publish(k).expect("anyone publishes");

    let price = chain.price(price_id(k));
    assert_eq!(price.price, tick_to_oracle_price(tick));
    assert_eq!(price.source_id, k);
}

#[test]
fn an_unsigned_key_creation_is_refused() {
    let k = account(&key(20));
    let payer = key(21);
    let mut chain = Chain::new();
    // The account exists, so the message is admitted and reaches the oracle.
    chain.state = std::mem::take(&mut chain.state).with_public_accounts([(
        k,
        Account::default().with_actor_state(SOURCE, vec![1].into()),
    )]);

    let result = chain.send(
        k,
        &[oracle(k)],
        &[&payer],
        &Message::CreateCurrentTickAccount { initial_price: ONE },
    );

    assert_refused(&result, "a key source must sign for its own account");
}

#[test]
fn a_key_signature_reaching_the_oracle_through_a_program_is_refused() {
    let source = key(20);
    let k = account(&source);
    let mut chain = Chain::new();
    chain
        .send(
            k,
            &[oracle(k)],
            &[&source],
            &Message::CreateCurrentTickAccount { initial_price: ONE },
        )
        .expect("a signed root creation succeeds");
    let relay = Source::new(1);
    let message = encode(&Message::UpdateCurrentTick { price: ONE << 4 });

    let result = submit(
        &mut chain.state,
        relay.actor(),
        &[relay.actor(), oracle(k), clock_actor()],
        &[&relay.key, &source],
        &SourceMessage::Forward {
            to: oracle(k),
            message,
            seeds: vec![],
        },
    );

    assert_refused(
        &result,
        "the price source must sign the transaction's root message",
    );
}

// ── Reading the price ────────────────────────────────────────────────────────────

#[test]
fn read_price_replies_to_the_reader() {
    let source = Source::new(1);
    let mut chain = Chain::with_window(&source, ONE << 3);
    let p = price_id(source.current_tick());
    let reader = Source::new(2);

    chain
        .forward(
            &reader,
            oracle(p),
            ReadPrice::BYTES.to_vec(),
            vec![],
            &[oracle(p)],
        )
        .expect("the read succeeds");

    let reply = chain
        .state
        .get_account_by_id(account(&reader.key))
        .data
        .actor_state(SOURCE)
        .to_vec();
    let reply: PriceReply = borsh::from_slice(&reply).expect("the reply decodes");
    assert_eq!(reply.price, chain.price(p));
}

#[test]
fn read_price_at_the_current_tick_actor_is_refused() {
    let source = Source::new(1);
    let mut chain = Chain::with_window(&source, ONE);
    let t = source.current_tick();
    let reader = Source::new(2);

    let result = chain.forward(
        &reader,
        oracle(t),
        ReadPrice::BYTES.to_vec(),
        vec![],
        &[oracle(t)],
    );

    assert_refused(&result, "receiver is not a price actor");
}
