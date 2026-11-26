//! NFT Program integration tests with private accounts: the real guest, proven in LEZ's
//! privacy-preserving circuit and settled on a `V03State`.
//!
//! Definitions stay public; masters and holdings are private or public. An NFT credited to a
//! private holding that isn't in the transaction is published and applied when its recipient
//! receives it.

use integration_tests::{
    account, deploy, key,
    private::{Pending, PrivateAccount, PrivateCall},
    submit,
};
use lee::{error::LeeError, PrivateKey, V03State};
use lee_core::{
    account::{AccountId, Actor},
    program::TransactionEvent,
};
use nft_core::{Message, MetadataStandard, NftCopy, NftMaster, NftMetadata, NftState};

struct Chain {
    state: V03State,
    nft: AccountId,
    definition: AccountId,
}

impl Chain {
    /// The NFT program, with a public NFT of `supply` created by `definition` whose master is
    /// `master`'s public account.
    fn new(definition: &PrivateKey, master: &PrivateKey, supply: u128) -> Self {
        let (state, ids) = deploy(&[nft_methods::NFT_ELF]);
        let [nft] = ids[..] else {
            panic!("one program deployed")
        };
        let mut chain = Self {
            state,
            nft,
            definition: account(definition),
        };
        chain
            .submit_public(
                account(definition),
                &[account(definition), account(master)],
                &[definition, master],
                &new_definition(supply, account(master)),
            )
            .expect("nft creation succeeds");
        chain
    }

    fn actor(&self, id: AccountId) -> Actor {
        Actor::new(id, self.nft)
    }

    /// Submits a public transaction with `message` to `(root, nft)`, declaring `declared`.
    fn submit_public(
        &mut self,
        root: AccountId,
        declared: &[AccountId],
        signers: &[&PrivateKey],
        message: &Message,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let declared: Vec<Actor> = declared.iter().map(|id| self.actor(*id)).collect();
        let root = self.actor(root);
        submit(&mut self.state, root, &declared, signers, message)
    }

    fn send(&mut self, call: PrivateCall<'_>) -> Result<(), LeeError> {
        call.submit(&mut self.state, &[(self.nft, nft_methods::NFT_ELF)])
    }

    fn printable_supply(&self) -> u128 {
        match NftState::decode(
            self.state
                .get_account_by_id(self.definition)
                .data
                .actor_state(self.nft),
        ) {
            Some(NftState::Definition(definition)) => definition.printable_supply,
            other => panic!("expected a definition, got {other:?}"),
        }
    }

    /// Initializes `holder`'s private holding: holding → definition → holding, with the
    /// definition public.
    fn initialize(&mut self, holder: &mut PrivateAccount) {
        let call = PrivateCall::to(
            self.actor(holder.id()),
            &Message::InitializeHolding {
                definition: self.definition,
            },
        )
        .authorized(holder)
        .public(self.actor(self.definition));
        self.send(call).expect("private initialization succeeds");
        holder.settle(&self.state, self.nft, self.copy(false));
    }

    /// Receives the latest message published for `holder`, which then holds `state`.
    fn receive(&mut self, holder: &mut PrivateAccount, state: Vec<u8>) {
        let pending = self.pending_for(holder);
        self.send(pending.receive(&self.state, holder))
            .expect("the recipient receives the NFT");
        holder.settle(&self.state, self.nft, state);
    }

    fn pending_for(&self, account: &PrivateAccount) -> Pending {
        Pending::from(&self.state, 0)
            .into_iter()
            .rev()
            .find(|message| message.is_for(account))
            .unwrap_or_else(|| panic!("a message waits for {}", account.id()))
    }

    fn copy(&self, owned: bool) -> Vec<u8> {
        NftState::Copy(NftCopy {
            definition_id: self.definition,
            owned,
        })
        .encode()
    }

    fn master(&self, print_balance: u128) -> Vec<u8> {
        NftState::Master(NftMaster {
            definition_id: self.definition,
            print_balance,
        })
        .encode()
    }

    /// A private holding that owns a copy printed from `master`'s public master, which then
    /// has `balance` left.
    fn private_copy(&mut self, byte: u8, master: &PrivateKey) -> PrivateAccount {
        let mut holder = PrivateAccount::new(byte);
        self.initialize(&mut holder);
        let call = PrivateCall::to(
            self.actor(account(master)),
            &Message::PrintNft { to: holder.id() },
        )
        .signer(master)
        .public(self.actor(account(master)))
        .recover(&holder);
        self.send(call).expect("the public print succeeds");
        let owned = self.copy(true);
        self.receive(&mut holder, owned);
        holder
    }
}

fn new_definition(printable_supply: u128, master: AccountId) -> Message {
    Message::NewDefinition {
        name: "Art".to_owned(),
        printable_supply,
        metadata: NftMetadata {
            standard: MetadataStandard::Simple,
            uri: "https://example.org/art.json".to_owned(),
            creators: "alice".to_owned(),
        },
        master,
    }
}

/// Panics unless `result` failed with an error mentioning `reason`.
fn assert_fails(result: &Result<(), LeeError>, reason: &str) {
    assert!(
        matches!(result, Err(error) if format!("{error:?}").contains(reason)),
        "expected a failure mentioning {reason:?}, got {result:?}"
    );
}

#[test]
fn private_master_receives_a_new_nft() {
    let (definition, alice) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice, 10);
    let (second, mut master) = (key(3), PrivateAccount::new(30));

    let call = PrivateCall::to(
        chain.actor(account(&second)),
        &new_definition(5, master.id()),
    )
    .signer(&second)
    .public(chain.actor(account(&second)))
    .authorized(&master);
    chain
        .send(call)
        .expect("the definition initializes the private master");

    let expected = NftState::Master(NftMaster {
        definition_id: account(&second),
        print_balance: 5,
    })
    .encode();
    master.settle(&chain.state, chain.nft, expected);
}

#[test]
fn private_holding_initializes_through_the_public_definition() {
    let (definition, alice) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice, 10);
    let mut bob = PrivateAccount::new(30);

    chain.initialize(&mut bob);

    assert_eq!(bob.actor_state(chain.nft), chain.copy(false));
}

#[test]
fn public_master_prints_to_a_private_holding_with_a_recovery_binding() {
    let (definition, alice) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice, 10);

    let bob = chain.private_copy(30, &alice);

    assert_eq!(bob.actor_state(chain.nft), chain.copy(true));
    assert_eq!(chain.printable_supply(), 10);
}

#[test]
fn private_master_prints_to_a_private_holding() {
    let (definition, alice) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice, 10);
    let mut master = PrivateAccount::new(30);
    chain.initialize(&mut master);
    let call = PrivateCall::to(
        chain.actor(account(&alice)),
        &Message::Transfer { to: master.id() },
    )
    .signer(&alice)
    .public(chain.actor(account(&alice)))
    .recover(&master);
    chain.send(call).expect("the public master moves");
    let whole = chain.master(10);
    chain.receive(&mut master, whole);
    let mut bob = PrivateAccount::new(40);
    chain.initialize(&mut bob);

    let call = PrivateCall::to(
        chain.actor(master.id()),
        &Message::PrintNft { to: bob.id() },
    )
    .authorized(&master)
    .seal(&bob);
    chain.send(call).expect("the private print succeeds");
    let printed = chain.master(9);
    master.settle(&chain.state, chain.nft, printed);

    let owned = chain.copy(true);
    chain.receive(&mut bob, owned);
    assert_eq!(chain.printable_supply(), 10);
}

#[test]
fn private_copy_moves_to_a_private_holding() {
    let (definition, alice) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice, 10);
    let mut bob = chain.private_copy(30, &alice);
    let mut carol = PrivateAccount::new(40);
    chain.initialize(&mut carol);

    let call = PrivateCall::to(chain.actor(bob.id()), &Message::Transfer { to: carol.id() })
        .authorized(&bob)
        .seal(&carol);
    chain.send(call).expect("the private transfer succeeds");
    let emptied = chain.copy(false);
    bob.settle(&chain.state, chain.nft, emptied);

    let owned = chain.copy(true);
    chain.receive(&mut carol, owned);
}

#[test]
fn private_copy_burn_lowers_the_public_supply() {
    let (definition, alice) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice, 10);
    let mut bob = chain.private_copy(30, &alice);

    let call = PrivateCall::to(chain.actor(bob.id()), &Message::Burn { amount: 1 })
        .authorized(&bob)
        .public(chain.actor(chain.definition));
    chain.send(call).expect("the private burn succeeds");

    let burned = chain.copy(false);
    bob.settle(&chain.state, chain.nft, burned);
    assert_eq!(chain.printable_supply(), 9);
}

/// A credit can't bounce: an NFT for a private holding that already holds one waits until
/// the holding is empty again.
#[test]
fn nft_for_a_private_holding_that_holds_one_waits_until_it_is_empty() {
    let (definition, alice) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice, 10);
    let mut bob = chain.private_copy(30, &alice);
    let mut carol = PrivateAccount::new(40);
    chain.initialize(&mut carol);

    // Bob's address has its recovery binding from the first print, so a public transaction
    // can publish a credit to it.
    chain
        .submit_public(
            account(&alice),
            &[account(&alice)],
            &[&alice],
            &Message::PrintNft { to: bob.id() },
        )
        .expect("the second print is published");
    let second = chain.pending_for(&bob);
    assert_fails(
        &chain.send(second.receive(&chain.state, &bob)),
        "recipient already holds an NFT",
    );

    let call = PrivateCall::to(chain.actor(bob.id()), &Message::Transfer { to: carol.id() })
        .authorized(&bob)
        .seal(&carol);
    chain.send(call).expect("bob gives his copy away");
    let emptied = chain.copy(false);
    bob.settle(&chain.state, chain.nft, emptied);

    chain
        .send(second.receive(&chain.state, &bob))
        .expect("bob receives the second copy once his holding is empty");
    let owned = chain.copy(true);
    bob.settle(&chain.state, chain.nft, owned);
}
