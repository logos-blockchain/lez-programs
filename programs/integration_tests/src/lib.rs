//! Shared harness for the integration tests: real guests, run through LEZ's public
//! execution on a `V03State`.
//!
//! Handler tests (in each program crate) cover one message at a time; these cover what only
//! the engine decides: declared actors, admission, authorization travelling down a sync
//! chain, async messages, and a failing actor failing the whole transaction.

use std::borrow::Cow;

use borsh::BorshSerialize;
use lee::{
    error::LeeError,
    program::Program,
    public_transaction::{Message, WitnessSet},
    PrivateKey, PublicKey, PublicTransaction, V03State,
};
use lee_core::{
    account::{AccountId, Actor},
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
