//! Token Program integration tests with private accounts: the real guest, proven in LEZ's
//! privacy-preserving circuit and settled on a `V03State`.
//!
//! Definitions stay public; holdings are private or public. A credit to a private holding
//! that isn't in the transaction is published (sealed from private execution, with a recovery
//! binding from public execution) and applied when its recipient receives it.

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
use token_core::{Message, TokenHolding, TokenState};

struct Chain {
    state: V03State,
    token: AccountId,
}

impl Chain {
    /// The token program, with a public token of `supply` created by `definition` and held by
    /// `holder`, mintable by `mint_authority`.
    fn new(
        definition: &PrivateKey,
        holder: &PrivateKey,
        supply: u128,
        mint_authority: Option<AccountId>,
    ) -> Self {
        let (state, ids) = deploy(&[token_methods::TOKEN_ELF]);
        let [token] = ids[..] else {
            panic!("one program deployed")
        };
        let mut chain = Self { state, token };
        chain
            .submit_public(
                account(definition),
                &[account(definition), account(holder)],
                &[definition, holder],
                &Message::NewDefinition {
                    name: "Test".to_owned(),
                    total_supply: supply,
                    mint_authority,
                    holding: account(holder),
                },
            )
            .expect("token creation succeeds");
        chain
    }

    fn actor(&self, id: AccountId) -> Actor {
        Actor::new(id, self.token)
    }

    /// Submits a public transaction with `message` to `(root, token)`, declaring `declared`.
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
        call.submit(&mut self.state, &[(self.token, token_methods::TOKEN_ELF)])
    }

    /// A public account's token state.
    fn token_state(&self, id: AccountId) -> Option<TokenState> {
        TokenState::decode(
            self.state
                .get_account_by_id(id)
                .data
                .actor_state(self.token),
        )
    }

    fn balance(&self, id: AccountId) -> u128 {
        match self.token_state(id) {
            Some(TokenState::Holding(holding)) => holding.balance,
            other => panic!("expected a holding, got {other:?}"),
        }
    }

    fn total_supply(&self, id: AccountId) -> u128 {
        match self.token_state(id) {
            Some(TokenState::Definition(definition)) => definition.total_supply,
            other => panic!("expected a definition, got {other:?}"),
        }
    }

    /// Initializes `holder`'s private holding of `definition`: holding → definition → holding,
    /// with the definition public.
    fn initialize(&mut self, holder: &mut PrivateAccount, definition: AccountId) {
        let call = PrivateCall::to(
            self.actor(holder.id()),
            &Message::InitializeHolding { definition },
        )
        .authorized(holder)
        .public(self.actor(definition));
        self.send(call).expect("private initialization succeeds");
        holder.settle(&self.state, self.token, holding(definition, 0));
    }

    /// The latest message published for `account`.
    fn pending_for(&self, account: &PrivateAccount) -> Pending {
        Pending::from(&self.state, 0)
            .into_iter()
            .rev()
            .find(|message| message.is_for(account))
            .unwrap_or_else(|| panic!("a message waits for {}", account.id()))
    }
}

fn holding(definition: AccountId, balance: u128) -> Vec<u8> {
    TokenState::Holding(TokenHolding {
        definition_id: definition,
        balance,
    })
    .encode()
}

/// Panics unless `result` failed with an error mentioning `reason`.
fn assert_fails(result: &Result<(), LeeError>, reason: &str) {
    assert!(
        matches!(result, Err(error) if format!("{error:?}").contains(reason)),
        "expected a failure mentioning {reason:?}, got {result:?}"
    );
}

/// Pays `amount` from `payer`'s public holding to `holder`'s private one, which then holds
/// `balance`: published with a recovery binding, then received.
fn pay(
    chain: &mut Chain,
    payer: &PrivateKey,
    holder: &mut PrivateAccount,
    amount: u128,
    balance: u128,
) {
    let call = PrivateCall::to(
        chain.actor(account(payer)),
        &Message::Transfer {
            to: holder.id(),
            amount,
        },
    )
    .signer(payer)
    .public(chain.actor(account(payer)))
    .recover(holder);
    chain.send(call).expect("the public transfer succeeds");
    let pending = chain.pending_for(holder);
    chain
        .send(pending.receive(&chain.state, holder))
        .expect("the recipient receives the transfer");
    let definition = match chain.token_state(account(payer)) {
        Some(TokenState::Holding(holding)) => holding.definition_id,
        other => panic!("the payer holds the token, got {other:?}"),
    };
    holder.settle(&chain.state, chain.token, holding(definition, balance));
}

/// A private holding of `balance`, initialized and paid from `payer`'s public holding.
fn funded(
    chain: &mut Chain,
    byte: u8,
    definition: AccountId,
    payer: &PrivateKey,
    balance: u128,
) -> PrivateAccount {
    let mut holder = PrivateAccount::new(byte);
    chain.initialize(&mut holder, definition);
    pay(chain, payer, &mut holder, balance, balance);
    holder
}

#[test]
fn private_holding_initializes_through_the_public_definition() {
    let (definition, alice) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice, 1_000, None);
    let mut bob = PrivateAccount::new(30);

    chain.initialize(&mut bob, account(&definition));

    assert_eq!(
        bob.actor_state(chain.token),
        holding(account(&definition), 0)
    );
    assert_eq!(chain.total_supply(account(&definition)), 1_000);
}

#[test]
fn private_holding_initializes_under_a_blinded_alias() {
    let (definition, alice) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice, 1_000, None);
    let mut bob = PrivateAccount::new(30);

    let call = PrivateCall::to(
        chain.actor(bob.id()),
        &Message::InitializeHolding {
            definition: account(&definition),
        },
    )
    .authorized(&bob)
    .public(chain.actor(account(&definition)))
    .blinded();
    chain
        .send(call)
        .expect("the definition replies to the alias");

    bob.settle(&chain.state, chain.token, holding(account(&definition), 0));
}

#[test]
fn private_holding_can_receive_a_new_token_s_initial_supply() {
    let (definition, alice) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice, 1_000, None);
    let (second, mut bob) = (key(3), PrivateAccount::new(30));

    let call = PrivateCall::to(
        chain.actor(account(&second)),
        &Message::NewDefinition {
            name: "Private".to_owned(),
            total_supply: 500,
            mint_authority: None,
            holding: bob.id(),
        },
    )
    .signer(&second)
    .public(chain.actor(account(&second)))
    .authorized(&bob);
    chain
        .send(call)
        .expect("the definition initializes the private holding");

    bob.settle(&chain.state, chain.token, holding(account(&second), 500));
    assert_eq!(chain.total_supply(account(&second)), 500);
}

#[test]
fn private_to_private_transfer_is_sealed_then_received() {
    let (definition, alice_key) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice_key, 1_000, None);
    let mut alice = funded(&mut chain, 30, account(&definition), &alice_key, 400);
    let mut bob = PrivateAccount::new(40);
    chain.initialize(&mut bob, account(&definition));

    let call = PrivateCall::to(
        chain.actor(alice.id()),
        &Message::Transfer {
            to: bob.id(),
            amount: 150,
        },
    )
    .authorized(&alice)
    .seal(&bob);
    chain.send(call).expect("the private transfer succeeds");
    alice.settle(
        &chain.state,
        chain.token,
        holding(account(&definition), 250),
    );

    let pending = chain.pending_for(&bob);
    assert!(
        !pending.is_for(&alice),
        "only the recipient opens the credit"
    );
    chain
        .send(pending.receive(&chain.state, &bob))
        .expect("bob receives the credit");
    bob.settle(
        &chain.state,
        chain.token,
        holding(account(&definition), 150),
    );

    let again = chain.send(pending.receive(&chain.state, &bob));
    assert!(again.is_err(), "a credit is received once: {again:?}");
}

#[test]
fn public_to_private_transfer_needs_a_recovery_binding() {
    let (definition, alice) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice, 1_000, None);
    let mut bob = PrivateAccount::new(30);
    chain.initialize(&mut bob, account(&definition));

    let unbound = chain.submit_public(
        account(&alice),
        &[account(&alice)],
        &[&alice],
        &Message::Transfer {
            to: bob.id(),
            amount: 100,
        },
    );
    assert!(
        matches!(&unbound, Err(error) if format!("{error:?}").contains("UnboundAsyncDestination")),
        "a public credit to an unbound private address can't be published: {unbound:?}"
    );

    pay(&mut chain, &alice, &mut bob, 100, 100);

    assert_eq!(
        bob.actor_state(chain.token),
        holding(account(&definition), 100)
    );
    assert_eq!(chain.balance(account(&alice)), 900);
}

#[test]
fn private_to_public_transfer_applies_in_the_transaction() {
    let (definition, alice_key, carol) = (key(1), key(2), key(3));
    let mut chain = Chain::new(&definition, &alice_key, 1_000, None);
    chain
        .submit_public(
            account(&carol),
            &[account(&carol), account(&definition)],
            &[&carol],
            &Message::InitializeHolding {
                definition: account(&definition),
            },
        )
        .expect("carol initializes a public holding");
    let mut alice = funded(&mut chain, 30, account(&definition), &alice_key, 400);

    let published = Pending::from(&chain.state, 0).len();
    let call = PrivateCall::to(
        chain.actor(alice.id()),
        &Message::Transfer {
            to: account(&carol),
            amount: 120,
        },
    )
    .authorized(&alice)
    .public(chain.actor(account(&carol)));
    chain
        .send(call)
        .expect("the transfer to a public holding succeeds");

    alice.settle(
        &chain.state,
        chain.token,
        holding(account(&definition), 280),
    );
    assert_eq!(chain.balance(account(&carol)), 120);
    assert_eq!(
        Pending::from(&chain.state, 0).len(),
        published,
        "the credit to a declared public holding is applied, not published"
    );
}

#[test]
fn private_burn_lowers_the_public_supply() {
    let (definition, alice_key) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice_key, 1_000, None);
    let mut alice = funded(&mut chain, 30, account(&definition), &alice_key, 400);

    let call = PrivateCall::to(chain.actor(alice.id()), &Message::Burn { amount: 100 })
        .authorized(&alice)
        .public(chain.actor(account(&definition)));
    chain.send(call).expect("the private burn succeeds");

    alice.settle(
        &chain.state,
        chain.token,
        holding(account(&definition), 300),
    );
    assert_eq!(chain.total_supply(account(&definition)), 900);
}

#[test]
fn private_holding_needs_its_authorization_key_to_spend() {
    let (definition, alice_key) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice_key, 1_000, None);
    let alice = funded(&mut chain, 30, account(&definition), &alice_key, 400);
    let bob = PrivateAccount::new(40);

    let call = PrivateCall::to(
        chain.actor(alice.id()),
        &Message::Transfer {
            to: bob.id(),
            amount: 100,
        },
    )
    .witnessed(&alice)
    .seal(&bob);

    assert_fails(
        &chain.send(call),
        "sender holding must authorize the transfer",
    );
}

/// A credit can't bounce: one to a private account without a holding waits until it has one.
#[test]
fn credit_to_a_private_account_without_a_holding_waits_for_it() {
    let (definition, alice_key) = (key(1), key(2));
    let mut chain = Chain::new(&definition, &alice_key, 1_000, None);
    let mut alice = funded(&mut chain, 30, account(&definition), &alice_key, 400);
    let mut dave = PrivateAccount::new(50);

    let call = PrivateCall::to(
        chain.actor(alice.id()),
        &Message::Transfer {
            to: dave.id(),
            amount: 75,
        },
    )
    .authorized(&alice)
    .seal(&dave);
    chain.send(call).expect("the transfer is published");
    alice.settle(
        &chain.state,
        chain.token,
        holding(account(&definition), 325),
    );

    let pending = chain.pending_for(&dave);
    assert_fails(
        &chain.send(pending.receive(&chain.state, &dave)),
        "recipient has no token holding",
    );

    chain.initialize(&mut dave, account(&definition));
    chain
        .send(pending.receive(&chain.state, &dave))
        .expect("dave receives the credit once he holds the token");
    dave.settle(&chain.state, chain.token, holding(account(&definition), 75));
}

#[test]
fn mint_to_a_private_holding_is_published_with_a_recovery_binding() {
    let (definition, alice, minter) = (key(1), key(2), key(3));
    let mut chain = Chain::new(&definition, &alice, 1_000, Some(account(&minter)));
    let mut bob = PrivateAccount::new(30);
    chain.initialize(&mut bob, account(&definition));

    let call = PrivateCall::to(
        chain.actor(account(&minter)),
        &Message::Mint {
            definition: account(&definition),
            to: bob.id(),
            amount: 250,
        },
    )
    .signer(&minter)
    .public(chain.actor(account(&minter)))
    .public(chain.actor(account(&definition)))
    .recover(&bob);
    chain.send(call).expect("the mint succeeds");
    assert_eq!(chain.total_supply(account(&definition)), 1_250);

    let pending = chain.pending_for(&bob);
    chain
        .send(pending.receive(&chain.state, &bob))
        .expect("bob receives the minted tokens");
    bob.settle(
        &chain.state,
        chain.token,
        holding(account(&definition), 250),
    );
}
