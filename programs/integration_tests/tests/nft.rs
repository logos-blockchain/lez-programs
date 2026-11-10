//! NFT Program integration tests: the real guest, run through LEZ's public execution.
//!
//! Each handler's own tests (in `programs/nft/src`) cover one message; these cover the
//! conversations between NFT actors.

use integration_tests::{account, assert_refused, deploy, deploy_at, key, submit};
use lee::{error::LeeError, PrivateKey, V03State};
use lee_core::{
    account::{AccountId, Actor},
    program::TransactionEvent,
};
use nft_core::{
    event::{self, NftEvent},
    Message, MetadataStandard, NftCopy, NftDefinition, NftMaster, NftMetadata, NftState, NftValue,
};

struct Chain {
    state: V03State,
    nft: AccountId,
}

impl Chain {
    fn new() -> Self {
        let (state, ids) = deploy(&[nft_methods::NFT_ELF]);
        let [nft] = ids[..] else {
            panic!("one program deployed")
        };
        Self { state, nft }
    }

    /// A chain with the NFT program deployed twice: `nft`, and a copy at `copy`.
    fn with_copy() -> (Self, AccountId) {
        let (nft, copy) = (AccountId::new([0xa0; 32]), AccountId::new([0xb0; 32]));
        let state = deploy_at(&[(nft, nft_methods::NFT_ELF), (copy, nft_methods::NFT_ELF)]);
        (Self { state, nft }, copy)
    }

    /// Submits `message` to `(root, nft)`, declaring `declared` NFT actors.
    fn submit(
        &mut self,
        root: AccountId,
        declared: &[AccountId],
        signers: &[&PrivateKey],
        message: &Message,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        self.submit_to(self.nft, root, declared, signers, message)
    }

    /// [`Self::submit`] to the NFT program deployed at `program`.
    fn submit_to(
        &mut self,
        program: AccountId,
        root: AccountId,
        declared: &[AccountId],
        signers: &[&PrivateKey],
        message: &Message,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let declared: Vec<Actor> = declared.iter().map(|id| Actor::new(*id, program)).collect();
        submit(
            &mut self.state,
            Actor::new(root, program),
            &declared,
            signers,
            message,
        )
    }

    fn nft_state(&self, id: AccountId) -> Option<NftState> {
        self.nft_state_in(self.nft, id)
    }

    /// The state of `id`'s actor under the NFT program deployed at `program`.
    fn nft_state_in(&self, program: AccountId, id: AccountId) -> Option<NftState> {
        NftState::decode(self.state.get_account_by_id(id).data.actor_state(program))
    }

    fn definition(&self, id: AccountId) -> NftDefinition {
        match self.nft_state(id) {
            Some(NftState::Definition(definition)) => definition,
            other => panic!("expected a definition, got {other:?}"),
        }
    }

    /// Creates an NFT of `supply` units, all on `master`.
    fn create(
        &mut self,
        definition: &PrivateKey,
        master: &PrivateKey,
        supply: u128,
    ) -> Vec<TransactionEvent> {
        self.create_in(self.nft, definition, master, supply)
            .expect("nft creation succeeds")
    }

    /// [`Self::create`] under the NFT program deployed at `program`.
    fn create_in(
        &mut self,
        program: AccountId,
        definition: &PrivateKey,
        master: &PrivateKey,
        supply: u128,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        self.submit_to(
            program,
            account(definition),
            &[account(definition), account(master)],
            &[definition, master],
            &new_definition(supply, account(master)),
        )
    }

    fn initialize(
        &mut self,
        holder: &PrivateKey,
        definition: AccountId,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        self.submit(
            account(holder),
            &[account(holder), definition],
            &[holder],
            &Message::InitializeHolding { definition },
        )
    }

    fn transfer(
        &mut self,
        from: &PrivateKey,
        to: AccountId,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        self.submit(
            account(from),
            &[account(from), to],
            &[from],
            &Message::Transfer { to },
        )
    }

    fn print(
        &mut self,
        master: &PrivateKey,
        to: AccountId,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        self.submit(
            account(master),
            &[account(master), to],
            &[master],
            &Message::PrintNft { to },
        )
    }

    fn burn(
        &mut self,
        holder: &PrivateKey,
        definition: AccountId,
        amount: u128,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        self.submit(
            account(holder),
            &[account(holder), definition],
            &[holder],
            &Message::Burn { amount },
        )
    }

    /// Panics unless the supply invariant holds over `holdings` (`nft.md` §6).
    fn assert_supply(&self, definition: AccountId, holdings: &[AccountId]) {
        let held: u128 = holdings
            .iter()
            .map(|id| match self.nft_state(*id) {
                Some(NftState::Master(master)) => master.print_balance,
                Some(NftState::Copy(NftCopy { owned: true, .. })) => 1,
                Some(NftState::Copy(NftCopy { owned: false, .. })) | None => 0,
                Some(NftState::Definition(_)) => panic!("a holding, not a definition"),
            })
            .sum();
        assert_eq!(self.definition(definition).printable_supply, held);
    }
}

fn metadata() -> NftMetadata {
    NftMetadata {
        standard: MetadataStandard::Simple,
        uri: "https://example.org/art.json".to_owned(),
        creators: "alice".to_owned(),
    }
}

fn new_definition(printable_supply: u128, master: AccountId) -> Message {
    Message::NewDefinition {
        name: "Art".to_owned(),
        printable_supply,
        metadata: metadata(),
        master,
    }
}

fn master(definition: AccountId, print_balance: u128) -> Option<NftState> {
    Some(NftState::Master(NftMaster {
        definition_id: definition,
        print_balance,
    }))
}

fn copy(definition: AccountId, owned: bool) -> Option<NftState> {
    Some(NftState::Copy(NftCopy {
        definition_id: definition,
        owned,
    }))
}

fn events_of<E: NftEvent + borsh::BorshDeserialize>(events: &[TransactionEvent]) -> Vec<E> {
    let selector = event::selector(E::NAME);
    events
        .iter()
        .filter(|record| record.event.selector == selector)
        .map(|record| borsh::from_slice(&record.event.data).expect("the event decodes"))
        .collect()
}

#[test]
fn nft_creating_an_nft_writes_the_definition_and_the_master() {
    let (mut chain, art, alice) = (Chain::new(), key(1), key(2));

    let events = chain.create(&art, &alice, 100);

    assert_eq!(
        chain.definition(account(&art)),
        NftDefinition {
            name: "Art".to_owned(),
            printable_supply: 100,
            metadata: metadata(),
        }
    );
    assert_eq!(chain.nft_state(account(&alice)), master(account(&art), 100));
    assert_eq!(
        events_of::<event::NewDefinition>(&events),
        vec![event::NewDefinition {
            definition: account(&art),
            master: account(&alice),
            name: "Art".to_owned(),
            printable_supply: 100,
        }]
    );
}

#[test]
fn nft_creating_an_nft_needs_the_master_to_sign() {
    let (mut chain, art, alice) = (Chain::new(), key(1), key(2));

    let result = chain.submit(
        account(&art),
        &[account(&art), account(&alice)],
        &[&art],
        &new_definition(100, account(&alice)),
    );

    // Unsigned, the fresh master is not even admitted (`UnadmittedPublicActor`); the handler's
    // own check is covered by `init_master::refuses_an_unauthorized_master`.
    assert!(result.is_err(), "alice did not sign: {result:?}");
    assert_eq!(chain.nft_state(account(&art)), None);
}

#[test]
fn nft_creating_an_nft_on_an_existing_master_fails() {
    let (mut chain, art, other, alice) = (Chain::new(), key(1), key(5), key(2));
    chain.create(&art, &alice, 100);

    let result = chain.create_in(chain.nft, &other, &alice, 10);

    assert_refused(&result, "master account must be empty");
    assert_eq!(chain.nft_state(account(&other)), None);
    assert_eq!(chain.nft_state(account(&alice)), master(account(&art), 100));
}

/// H → D → H: the holding asks the definition, which answers with `InitHolding`.
#[test]
fn nft_initializing_a_holding_round_trips_through_the_definition() {
    let (mut chain, art, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&art, &alice, 100);

    chain
        .initialize(&bob, account(&art))
        .expect("initialization succeeds");

    assert_eq!(chain.nft_state(account(&bob)), copy(account(&art), false));
}

#[test]
fn nft_initializing_twice_is_a_no_op() {
    let (mut chain, art, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&art, &alice, 100);
    chain.initialize(&bob, account(&art)).expect("first");

    chain.initialize(&bob, account(&art)).expect("second");

    assert_eq!(chain.nft_state(account(&bob)), copy(account(&art), false));
}

#[test]
fn nft_initializing_against_a_non_definition_fails_and_writes_nothing() {
    let (mut chain, art, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&art, &alice, 100);

    // Alice's account holds the master, not a definition.
    let result = chain.initialize(&bob, account(&alice));

    assert_refused(&result, "receiver is not an NFT definition");
    assert_eq!(chain.nft_state(account(&bob)), None);
}

#[test]
fn nft_a_print_credits_an_initialized_holding_without_its_signature() {
    let (mut chain, art, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&art, &alice, 100);
    chain.initialize(&bob, account(&art)).expect("init");

    let events = chain
        .print(&alice, account(&bob))
        .expect("the print succeeds");

    assert_eq!(chain.nft_state(account(&alice)), master(account(&art), 99));
    assert_eq!(chain.nft_state(account(&bob)), copy(account(&art), true));
    assert_eq!(
        events_of::<event::Printed>(&events),
        vec![event::Printed {
            definition: account(&art),
            master: account(&alice),
            to: account(&bob),
        }]
    );
    assert!(
        events_of::<event::Transferred>(&events).is_empty(),
        "a print is not a transfer"
    );
    chain.assert_supply(account(&art), &[account(&alice), account(&bob)]);
}

#[test]
fn nft_a_print_to_an_uninitialized_account_fails_and_prints_nothing() {
    let (mut chain, art, alice, carol) = (Chain::new(), key(1), key(2), key(4));
    chain.create(&art, &alice, 100);

    let result = chain.submit(
        account(&alice),
        &[account(&alice), account(&carol)],
        // Carol signs only to admit her fresh account; unadmitted, the engine refuses the
        // delivery before the NFT program runs (`UnadmittedPublicActor`).
        &[&alice, &carol],
        &Message::PrintNft {
            to: account(&carol),
        },
    );

    assert_refused(&result, "recipient has no NFT holding; initialize it first");
    assert_eq!(chain.nft_state(account(&alice)), master(account(&art), 100));
    assert_eq!(chain.nft_state(account(&carol)), None);
}

#[test]
fn nft_a_print_to_a_holding_with_a_copy_fails() {
    let (mut chain, art, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&art, &alice, 100);
    chain.initialize(&bob, account(&art)).expect("init");
    chain.print(&alice, account(&bob)).expect("first print");

    let result = chain.print(&alice, account(&bob));

    assert_refused(&result, "recipient already holds an NFT");
    assert_eq!(chain.nft_state(account(&alice)), master(account(&art), 99));
}

#[test]
fn nft_a_print_needs_the_master_signature() {
    let (mut chain, art, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&art, &alice, 100);
    chain.initialize(&bob, account(&art)).expect("init");

    let result = chain.submit(
        account(&alice),
        &[account(&alice), account(&bob)],
        &[&bob],
        &Message::PrintNft { to: account(&bob) },
    );

    assert_refused(&result, "master must authorize the print");
    assert_eq!(chain.nft_state(account(&bob)), copy(account(&art), false));
}

#[test]
fn nft_a_master_keeps_its_own_unit() {
    let (mut chain, art, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&art, &alice, 1);
    chain.initialize(&bob, account(&art)).expect("init");

    let result = chain.print(&alice, account(&bob));

    assert_refused(&result, "insufficient print balance");
}

#[test]
fn nft_a_copy_moves_to_an_initialized_holding() {
    let (mut chain, art, alice, bob, carol) = (Chain::new(), key(1), key(2), key(3), key(4));
    chain.create(&art, &alice, 100);
    chain.initialize(&bob, account(&art)).expect("init bob");
    chain.initialize(&carol, account(&art)).expect("init carol");
    chain.print(&alice, account(&bob)).expect("print");

    let events = chain
        .transfer(&bob, account(&carol))
        .expect("the transfer succeeds");

    assert_eq!(chain.nft_state(account(&bob)), copy(account(&art), false));
    assert_eq!(chain.nft_state(account(&carol)), copy(account(&art), true));
    assert_eq!(
        events_of::<event::Transferred>(&events),
        vec![event::Transferred {
            definition: account(&art),
            from: account(&bob),
            to: account(&carol),
            value: NftValue::Copy,
        }]
    );
    chain.assert_supply(
        account(&art),
        &[account(&alice), account(&bob), account(&carol)],
    );
}

/// `nft.md` §2.2: initialization writes a copy, and it can still become the master.
#[test]
fn nft_the_master_moves_to_an_initialized_holding() {
    let (mut chain, art, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&art, &alice, 100);
    chain.initialize(&bob, account(&art)).expect("init");

    let events = chain
        .transfer(&alice, account(&bob))
        .expect("the transfer succeeds");

    assert_eq!(chain.nft_state(account(&alice)), master(account(&art), 0));
    assert_eq!(chain.nft_state(account(&bob)), master(account(&art), 100));
    assert_eq!(
        events_of::<event::Transferred>(&events),
        vec![event::Transferred {
            definition: account(&art),
            from: account(&alice),
            to: account(&bob),
            value: NftValue::Master(100),
        }]
    );

    // The new master prints, back to alice's emptied master.
    chain.print(&key(3), account(&alice)).expect("bob prints");
    assert_eq!(chain.nft_state(account(&alice)), copy(account(&art), true));
    chain.assert_supply(account(&art), &[account(&alice), account(&bob)]);
}

#[test]
fn nft_a_transfer_to_an_uninitialized_account_fails_and_moves_nothing() {
    let (mut chain, art, alice, carol) = (Chain::new(), key(1), key(2), key(4));
    chain.create(&art, &alice, 100);

    let result = chain.submit(
        account(&alice),
        &[account(&alice), account(&carol)],
        &[&alice, &carol],
        &Message::Transfer {
            to: account(&carol),
        },
    );

    assert_refused(&result, "recipient has no NFT holding; initialize it first");
    assert_eq!(chain.nft_state(account(&alice)), master(account(&art), 100));
    assert_eq!(chain.nft_state(account(&carol)), None);
}

/// A failed credit fails the transaction, the sender's debit included.
#[test]
fn nft_a_transfer_to_a_holding_with_a_copy_rolls_back() {
    let (mut chain, art, alice, bob, carol) = (Chain::new(), key(1), key(2), key(3), key(4));
    chain.create(&art, &alice, 100);
    chain.initialize(&bob, account(&art)).expect("init bob");
    chain.initialize(&carol, account(&art)).expect("init carol");
    chain.print(&alice, account(&bob)).expect("print bob");
    chain.print(&alice, account(&carol)).expect("print carol");

    let result = chain.transfer(&bob, account(&carol));

    assert_refused(&result, "recipient already holds an NFT");
    assert_eq!(chain.nft_state(account(&bob)), copy(account(&art), true));
    assert_eq!(chain.nft_state(account(&carol)), copy(account(&art), true));
}

#[test]
fn nft_a_transfer_of_another_nft_fails() {
    let (mut chain, art, junk, alice, bob, mallory) =
        (Chain::new(), key(1), key(5), key(2), key(3), key(6));
    chain.create(&art, &alice, 100);
    chain.create(&junk, &mallory, 100);
    chain.initialize(&bob, account(&art)).expect("init");

    let result = chain.transfer(&mallory, account(&bob));

    assert_refused(&result, "recipient holds another NFT");
    assert_eq!(chain.nft_state(account(&bob)), copy(account(&art), false));
    assert_eq!(
        chain.nft_state(account(&mallory)),
        master(account(&junk), 100)
    );
}

#[test]
fn nft_a_transfer_needs_the_sender_signature() {
    let (mut chain, art, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&art, &alice, 100);
    chain.initialize(&bob, account(&art)).expect("init");

    let result = chain.submit(
        account(&alice),
        &[account(&alice), account(&bob)],
        &[&bob],
        &Message::Transfer { to: account(&bob) },
    );

    assert_refused(&result, "sender holding must authorize the transfer");
    assert_eq!(chain.nft_state(account(&alice)), master(account(&art), 100));
}

#[test]
fn nft_burning_a_copy_lowers_the_supply() {
    let (mut chain, art, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&art, &alice, 100);
    chain.initialize(&bob, account(&art)).expect("init");
    chain.print(&alice, account(&bob)).expect("print");

    let events = chain
        .burn(&bob, account(&art), 1)
        .expect("the burn succeeds");

    assert_eq!(chain.nft_state(account(&bob)), copy(account(&art), false));
    assert_eq!(chain.definition(account(&art)).printable_supply, 99);
    assert_eq!(
        events_of::<event::Burned>(&events),
        vec![event::Burned {
            definition: account(&art),
            holding: account(&bob),
            amount: 1,
        }]
    );
    chain.assert_supply(account(&art), &[account(&alice), account(&bob)]);
}

#[test]
fn nft_burning_print_rights_lowers_the_supply() {
    let (mut chain, art, alice) = (Chain::new(), key(1), key(2));
    chain.create(&art, &alice, 100);

    chain
        .burn(&alice, account(&art), 40)
        .expect("the burn succeeds");

    assert_eq!(chain.nft_state(account(&alice)), master(account(&art), 60));
    chain.assert_supply(account(&art), &[account(&alice)]);
}

#[test]
fn nft_burning_without_declaring_the_definition_fails() {
    let (mut chain, art, alice) = (Chain::new(), key(1), key(2));
    chain.create(&art, &alice, 100);

    let result = chain.submit(
        account(&alice),
        &[account(&alice)],
        &[&alice],
        &Message::Burn { amount: 1 },
    );

    assert!(result.is_err(), "the definition is undeclared: {result:?}");
    assert_eq!(chain.nft_state(account(&alice)), master(account(&art), 100));
}

/// The same code deployed twice is two programs: the copy's definition is not a definition
/// to the original, so it cannot back a holding of it.
#[test]
fn nft_a_definition_of_another_deployment_is_not_a_definition() {
    let ((mut chain, copy_program), art, mallory, bob) =
        (Chain::with_copy(), key(5), key(6), key(3));
    chain
        .create_in(copy_program, &art, &mallory, 100)
        .expect("the copy creates its own NFT");

    let result = chain.initialize(&bob, account(&art));

    assert_refused(&result, "receiver is not an NFT definition");
    assert_eq!(chain.nft_state(account(&bob)), None);
}

/// A holding under the copy is no holding to the original: a print to it is refused.
#[test]
fn nft_a_holding_of_another_deployment_cannot_receive() {
    let ((mut chain, copy_program), art, alice, copy_art, bob) =
        (Chain::with_copy(), key(1), key(2), key(5), key(3));
    chain.create(&art, &alice, 100);
    chain
        .create_in(copy_program, &copy_art, &bob, 100)
        .expect("bob holds the copy's master");

    let result = chain.print(&alice, account(&bob));

    assert_refused(&result, "recipient has no NFT holding; initialize it first");
    assert_eq!(chain.nft_state(account(&alice)), master(account(&art), 100));
    assert_eq!(
        chain.nft_state_in(copy_program, account(&bob)),
        master(account(&copy_art), 100)
    );
}

/// A root call has no sender, so there is nobody to reply to.
#[test]
fn nft_read_state_as_a_root_call_fails() {
    let (mut chain, art, alice) = (Chain::new(), key(1), key(2));
    chain.create(&art, &alice, 100);

    let result = chain.submit(account(&art), &[account(&art)], &[], &Message::ReadState);

    assert_refused(&result, "ReadState needs a sender to reply to");
}
