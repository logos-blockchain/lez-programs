//! Shared harness for the integration tests: real guests, run through LEZ's public
//! execution on a `V03State`.
//!
//! Handler tests (in each program crate) cover one message at a time; these cover what only
//! the engine decides: declared actors, admission, authorization travelling down a sync
//! chain, async messages, and a failing actor failing the whole transaction.

use std::borrow::Cow;

use borsh::BorshSerialize;
use clock_core::{clock_account_id, clock_actor, ClockAccountData, CLOCK_PROGRAM_ACCOUNT_ID};
use lee::{
    error::LeeError,
    program::Program,
    public_transaction::{Message, WitnessSet},
    PrivateKey, PublicKey, PublicTransaction, V03State,
};
use lee_core::{
    account::{Account, AccountId, Actor},
    program::TransactionEvent,
};

/// A state with `elfs` deployed, and each program's account id, in order.
///
/// `with_programs` seeds a program as the loader's deploy would, at
/// `AccountId::from_builtin_program(image_id)`.
#[must_use]
pub fn deploy(elfs: &[&'static [u8]]) -> (V03State, Vec<AccountId>) {
    let programs: Vec<Program> = elfs
        .iter()
        .map(|elf| Program::new(Cow::Borrowed(*elf)).expect("the guest loads"))
        .collect();
    let ids = programs
        .iter()
        .map(|program| AccountId::from_builtin_program(program.id()))
        .collect();
    (V03State::new().with_programs(programs), ids)
}

/// A state with each ELF deployed at the given program account id, as a loader deploy to a
/// fresh header would. The same ELF at two ids is two programs.
#[must_use]
pub fn deploy_at(programs: &[(AccountId, &'static [u8])]) -> V03State {
    V03State::new().with_named_programs(programs.iter().map(|(id, elf)| {
        (
            *id,
            Program::new(Cow::Borrowed(*elf)).expect("the guest loads"),
        )
    }))
}

/// Submits a public transaction with `message` to `root`, declaring `declared` and signed by
/// `signers` at their current nonces.
pub fn submit(
    state: &mut V03State,
    root: Actor,
    declared: &[Actor],
    signers: &[&PrivateKey],
    message: &impl BorshSerialize,
) -> Result<Vec<TransactionEvent>, LeeError> {
    let nonces = signers
        .iter()
        .map(|key| {
            let id = account(key);
            (id, state.get_account_by_id(id).nonce)
        })
        .collect();
    let tx = Message::try_new(root, declared.iter().copied(), nonces, message)
        .expect("the transaction message encodes");
    let witness_set = WitnessSet::for_message(&tx, signers);
    state.transition_from_public_transaction(&PublicTransaction::new(tx, witness_set), 1, 0)
}

/// A test signing key; `byte` picks it.
#[must_use]
pub fn key(byte: u8) -> PrivateKey {
    PrivateKey::try_new([byte; 32]).expect("a valid test key")
}

/// The public account a key controls.
#[must_use]
pub fn account(key: &PrivateKey) -> AccountId {
    AccountId::from(&PublicKey::new_from_private_key(key))
}

/// Panics unless `result` is a guest refusal whose message contains `reason`.
pub fn assert_refused<T: std::fmt::Debug>(result: &Result<T, LeeError>, reason: &str) {
    assert!(
        matches!(result, Err(LeeError::ProgramExecutionFailed(message)) if message.contains(reason)),
        "expected a refusal with {reason:?}, got {result:?}"
    );
}

/// Adds LEZ's clock, at block 1 and `timestamp`. Handlers read the clock's time; the chain ticks
/// it after every block, which the tests do with [`tick`].
#[must_use]
pub fn with_clock(state: V03State, timestamp: u64) -> V03State {
    let clock = ClockAccountData {
        block_id: 1,
        timestamp,
    };
    state
        .with_named_programs([(clock_account_id(), programs::clock())])
        .with_public_accounts([(
            CLOCK_PROGRAM_ACCOUNT_ID,
            Account::default().with_actor_state(clock_account_id(), clock.to_bytes().into()),
        )])
}

/// Ends the block: the clock moves to the next block at `timestamp`, as the sequencer's last
/// transaction would.
pub fn tick(state: &mut V03State, timestamp: u64) {
    let block_id = clock(state)
        .block_id
        .checked_add(1)
        .expect("block id fits in u64");
    submit(
        state,
        clock_actor(),
        &[clock_actor()],
        &[],
        &clock_core::Message::Tick {
            timestamp,
            block_id,
        },
    )
    .expect("the clock ticks");
}

/// The clock's block id and timestamp.
#[must_use]
pub fn clock(state: &V03State) -> ClockAccountData {
    let account = state.get_account_by_id(CLOCK_PROGRAM_ACCOUNT_ID);
    ClockAccountData::from_bytes(account.data.actor_state(clock_account_id()))
}
