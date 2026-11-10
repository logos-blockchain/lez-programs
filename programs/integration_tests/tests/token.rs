//! Token Program integration tests: the real guest, run through LEZ's public execution.
//!
//! Each handler's own tests (in `programs/token/src`) cover one message; these cover the
//! conversations between token actors.

use integration_tests::{account, assert_refused, deploy, deploy_at, key, submit};
use lee::{error::LeeError, PrivateKey, V03State};
use lee_core::{
    account::{AccountId, Actor},
    program::TransactionEvent,
};
use token_core::{
    event::{self, TokenEvent},
    Message, TokenDefinition, TokenHolding, TokenState,
};

struct Chain {
    state: V03State,
    token: AccountId,
}

impl Chain {
    fn new() -> Self {
        let (state, ids) = deploy(&[token_methods::TOKEN_ELF]);
        let [token] = ids[..] else {
            panic!("one program deployed")
        };
        Self { state, token }
    }

    /// A chain with the token program deployed twice: `token`, and a copy at `copy`.
    fn with_copy() -> (Self, AccountId) {
        let (token, copy) = (AccountId::new([0xa0; 32]), AccountId::new([0xb0; 32]));
        let state = deploy_at(&[
            (token, token_methods::TOKEN_ELF),
            (copy, token_methods::TOKEN_ELF),
        ]);
        (Self { state, token }, copy)
    }

    /// Submits `message` to `(root, token)`, declaring `declared` token actors.
    fn submit(
        &mut self,
        root: AccountId,
        declared: &[AccountId],
        signers: &[&PrivateKey],
        message: &Message,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        self.submit_to(self.token, root, declared, signers, message)
    }

    /// [`Self::submit`] to the token program deployed at `program`.
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

    fn token_state(&self, id: AccountId) -> Option<TokenState> {
        self.token_state_in(self.token, id)
    }

    /// The state of `id`'s actor under the token program deployed at `program`.
    fn token_state_in(&self, program: AccountId, id: AccountId) -> Option<TokenState> {
        TokenState::decode(self.state.get_account_by_id(id).data.actor_state(program))
    }

    fn holding(&self, id: AccountId) -> Option<TokenHolding> {
        match self.token_state(id) {
            Some(TokenState::Holding(holding)) => Some(holding),
            Some(TokenState::Definition(_)) | None => None,
        }
    }

    fn balance(&self, id: AccountId) -> u128 {
        self.holding(id).expect("a token holding").balance
    }

    fn definition(&self, id: AccountId) -> TokenDefinition {
        match self.token_state(id) {
            Some(TokenState::Definition(definition)) => definition,
            other => panic!("expected a definition, got {other:?}"),
        }
    }

    /// Creates a token with `supply` held by `holder`.
    fn create(
        &mut self,
        definition: &PrivateKey,
        holder: &PrivateKey,
        supply: u128,
        mint_authority: Option<AccountId>,
    ) -> Vec<TransactionEvent> {
        self.submit(
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
        .expect("token creation succeeds")
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
}

fn events_of<E: TokenEvent + borsh::BorshDeserialize>(events: &[TransactionEvent]) -> Vec<E> {
    let selector = event::selector(E::NAME);
    events
        .iter()
        .filter(|record| record.event.selector == selector)
        .map(|record| borsh::from_slice(&record.event.data).expect("the event decodes"))
        .collect()
}

#[test]
fn token_creating_a_token_writes_the_definition_and_the_initial_holding() {
    let (mut chain, definition, alice) = (Chain::new(), key(1), key(2));

    let events = chain.create(&definition, &alice, 1_000, None);

    assert_eq!(chain.definition(account(&definition)).total_supply, 1_000);
    assert_eq!(
        chain.holding(account(&alice)),
        Some(TokenHolding {
            definition_id: account(&definition),
            balance: 1_000,
        })
    );
    assert_eq!(
        events_of::<event::NewDefinition>(&events),
        vec![event::NewDefinition {
            definition: account(&definition),
            holding: account(&alice),
            name: "Test".to_owned(),
            total_supply: 1_000,
            authority: None,
        }]
    );
}

#[test]
fn token_creating_a_token_needs_the_holding_to_sign() {
    let (mut chain, definition, alice) = (Chain::new(), key(1), key(2));

    let result = chain.submit(
        account(&definition),
        &[account(&definition), account(&alice)],
        &[&definition],
        &Message::NewDefinition {
            name: "Test".to_owned(),
            total_supply: 1_000,
            mint_authority: None,
            holding: account(&alice),
        },
    );

    assert!(result.is_err(), "alice did not sign: {result:?}");
    assert_eq!(chain.token_state(account(&definition)), None);
}

/// H → D → H: the holding asks the definition, which answers with `InitHolding`.
#[test]
fn token_initializing_a_holding_round_trips_through_the_definition() {
    let (mut chain, definition, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&definition, &alice, 1_000, None);

    chain
        .initialize(&bob, account(&definition))
        .expect("initialization succeeds");

    assert_eq!(
        chain.holding(account(&bob)),
        Some(TokenHolding {
            definition_id: account(&definition),
            balance: 0,
        })
    );
}

#[test]
fn token_initializing_twice_is_a_no_op() {
    let (mut chain, definition, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&definition, &alice, 1_000, None);
    chain.initialize(&bob, account(&definition)).expect("first");

    chain
        .initialize(&bob, account(&definition))
        .expect("second");

    assert_eq!(chain.balance(account(&bob)), 0);
}

#[test]
fn token_initializing_against_a_non_definition_fails_and_writes_nothing() {
    let (mut chain, definition, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&definition, &alice, 1_000, None);

    // Alice's account holds a holding, not a definition.
    let result = chain.initialize(&bob, account(&alice));

    assert_refused(&result, "receiver is not a token definition");
    assert_eq!(chain.token_state(account(&bob)), None);
}

#[test]
fn token_initializing_without_declaring_the_definition_fails() {
    let (mut chain, definition, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&definition, &alice, 1_000, None);

    let result = chain.submit(
        account(&bob),
        &[account(&bob)],
        &[&bob],
        &Message::InitializeHolding {
            definition: account(&definition),
        },
    );

    assert!(result.is_err(), "the definition is undeclared: {result:?}");
    assert_eq!(chain.token_state(account(&bob)), None);
}

#[test]
fn token_a_transfer_credits_an_initialized_recipient() {
    let (mut chain, definition, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&definition, &alice, 1_000, None);
    chain.initialize(&bob, account(&definition)).expect("init");

    let events = chain
        .submit(
            account(&alice),
            &[account(&alice), account(&bob)],
            &[&alice],
            &Message::Transfer {
                to: account(&bob),
                amount: 300,
            },
        )
        .expect("the transfer succeeds");

    assert_eq!(chain.balance(account(&alice)), 700);
    assert_eq!(chain.balance(account(&bob)), 300);
    assert_eq!(
        events_of::<event::Transferred>(&events),
        vec![event::Transferred {
            definition: account(&definition),
            from: account(&alice),
            to: account(&bob),
            amount: 300,
        }]
    );
}

/// No credit creates a holding, so a junk token can't occupy an account.
#[test]
fn token_a_transfer_to_an_uninitialized_account_fails_and_debits_nothing() {
    let (mut chain, definition, alice, carol) = (Chain::new(), key(1), key(2), key(4));
    chain.create(&definition, &alice, 1_000, None);

    let result = chain.submit(
        account(&alice),
        &[account(&alice), account(&carol)],
        &[&alice, &carol],
        &Message::Transfer {
            to: account(&carol),
            amount: 300,
        },
    );

    assert_refused(
        &result,
        "recipient has no token holding; initialize it first",
    );
    assert_eq!(chain.balance(account(&alice)), 1_000);
    assert_eq!(chain.token_state(account(&carol)), None);
}

#[test]
fn token_a_transfer_of_another_token_fails() {
    let (mut chain, usdc, junk, alice, mallory) = (Chain::new(), key(1), key(5), key(2), key(6));
    chain.create(&usdc, &alice, 1_000, None);
    chain.create(&junk, &mallory, 1_000, None);

    let result = chain.submit(
        account(&mallory),
        &[account(&mallory), account(&alice)],
        &[&mallory],
        &Message::Transfer {
            to: account(&alice),
            amount: 1,
        },
    );

    assert_refused(&result, "recipient holds another token");
    assert_eq!(chain.balance(account(&alice)), 1_000);
    assert_eq!(chain.balance(account(&mallory)), 1_000);
}

#[test]
fn token_a_transfer_needs_the_sender_signature() {
    let (mut chain, definition, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&definition, &alice, 1_000, None);
    chain.initialize(&bob, account(&definition)).expect("init");

    let result = chain.submit(
        account(&alice),
        &[account(&alice), account(&bob)],
        &[&bob],
        &Message::Transfer {
            to: account(&bob),
            amount: 300,
        },
    );

    assert_refused(&result, "sender holding must authorize the transfer");
    assert_eq!(chain.balance(account(&alice)), 1_000);
}

#[test]
fn token_a_burn_reduces_the_holding_and_the_supply() {
    let (mut chain, definition, alice) = (Chain::new(), key(1), key(2));
    chain.create(&definition, &alice, 1_000, None);

    let events = chain
        .submit(
            account(&alice),
            &[account(&alice), account(&definition)],
            &[&alice],
            &Message::Burn { amount: 100 },
        )
        .expect("the burn succeeds");

    assert_eq!(chain.balance(account(&alice)), 900);
    assert_eq!(chain.definition(account(&definition)).total_supply, 900);
    assert_eq!(
        events_of::<event::Burned>(&events),
        vec![event::Burned {
            definition: account(&definition),
            holding: account(&alice),
            amount: 100,
        }]
    );
}

#[test]
fn token_an_external_authority_mints_through_its_own_actor() {
    let (mut chain, definition, alice, minter) = (Chain::new(), key(1), key(2), key(7));
    chain.create(&definition, &alice, 1_000, Some(account(&minter)));

    let events = chain
        .submit(
            account(&minter),
            &[account(&minter), account(&definition), account(&alice)],
            &[&minter],
            &Message::Mint {
                definition: account(&definition),
                to: account(&alice),
                amount: 50,
            },
        )
        .expect("the mint succeeds");

    assert_eq!(chain.definition(account(&definition)).total_supply, 1_050);
    assert_eq!(chain.balance(account(&alice)), 1_050);
    assert_eq!(
        events_of::<event::Minted>(&events),
        vec![event::Minted {
            definition: account(&definition),
            to: account(&alice),
            amount: 50,
        }]
    );
    assert!(
        events_of::<event::Transferred>(&events).is_empty(),
        "a mint is not a transfer"
    );
    // The forwarding actor's state is untouched.
    assert_eq!(chain.token_state(account(&minter)), None);
}

#[test]
fn token_a_self_authority_definition_mints() {
    let (mut chain, definition, alice) = (Chain::new(), key(1), key(2));
    chain.create(&definition, &alice, 1_000, Some(account(&definition)));

    chain
        .submit(
            account(&definition),
            &[account(&definition), account(&alice)],
            &[&definition],
            &Message::Mint {
                definition: account(&definition),
                to: account(&alice),
                amount: 50,
            },
        )
        .expect("the mint succeeds");

    assert_eq!(chain.balance(account(&alice)), 1_050);
}

#[test]
fn token_only_the_authority_mints() {
    let (mut chain, definition, alice, minter, mallory) =
        (Chain::new(), key(1), key(2), key(7), key(6));
    chain.create(&definition, &alice, 1_000, Some(account(&minter)));

    let result = chain.submit(
        account(&mallory),
        &[account(&mallory), account(&definition), account(&alice)],
        &[&mallory],
        &Message::Mint {
            definition: account(&definition),
            to: account(&alice),
            amount: 50,
        },
    );

    assert_refused(&result, "sender is not the mint authority");
    assert_eq!(chain.definition(account(&definition)).total_supply, 1_000);
}

#[test]
fn token_a_mint_to_an_uninitialized_account_fails_and_mints_nothing() {
    let (mut chain, definition, alice, minter, carol) =
        (Chain::new(), key(1), key(2), key(7), key(4));
    chain.create(&definition, &alice, 1_000, Some(account(&minter)));

    let result = chain.submit(
        account(&minter),
        &[account(&minter), account(&definition), account(&carol)],
        // Carol signs only to admit her fresh account; unadmitted, the engine refuses the
        // delivery before the token program runs (`UnadmittedPublicActor`).
        &[&minter, &carol],
        &Message::Mint {
            definition: account(&definition),
            to: account(&carol),
            amount: 10,
        },
    );

    assert_refused(
        &result,
        "recipient has no token holding; initialize it first",
    );
    assert_eq!(chain.definition(account(&definition)).total_supply, 1_000);
    assert_eq!(chain.token_state(account(&carol)), None);
}

#[test]
fn token_a_rotated_authority_replaces_the_old_one() {
    let (mut chain, definition, alice, minter, next) =
        (Chain::new(), key(1), key(2), key(7), key(8));
    chain.create(&definition, &alice, 1_000, Some(account(&minter)));

    let events = chain
        .submit(
            account(&minter),
            &[account(&minter), account(&definition)],
            &[&minter],
            &Message::SetAuthority {
                definition: account(&definition),
                new_authority: Some(account(&next)),
            },
        )
        .expect("the rotation succeeds");
    assert_eq!(
        events_of::<event::AuthorityChanged>(&events),
        vec![event::AuthorityChanged {
            definition: account(&definition),
            old: Some(account(&minter)),
            new: Some(account(&next)),
        }]
    );

    let old = chain.submit(
        account(&minter),
        &[account(&minter), account(&definition), account(&alice)],
        &[&minter],
        &Message::Mint {
            definition: account(&definition),
            to: account(&alice),
            amount: 1,
        },
    );
    assert_refused(&old, "sender is not the mint authority");

    chain
        .submit(
            account(&next),
            &[account(&next), account(&definition), account(&alice)],
            &[&next],
            &Message::Mint {
                definition: account(&definition),
                to: account(&alice),
                amount: 1,
            },
        )
        .expect("the new authority mints");
    assert_eq!(chain.balance(account(&alice)), 1_001);
    assert_eq!(chain.definition(account(&definition)).total_supply, 1_001);
}

#[test]
fn token_a_renounced_authority_can_no_longer_mint() {
    let (mut chain, definition, alice, minter) = (Chain::new(), key(1), key(2), key(7));
    chain.create(&definition, &alice, 1_000, Some(account(&minter)));

    let events = chain
        .submit(
            account(&minter),
            &[account(&minter), account(&definition)],
            &[&minter],
            &Message::SetAuthority {
                definition: account(&definition),
                new_authority: None,
            },
        )
        .expect("renouncing succeeds");
    assert_eq!(
        events_of::<event::AuthorityChanged>(&events),
        vec![event::AuthorityChanged {
            definition: account(&definition),
            old: Some(account(&minter)),
            new: None,
        }]
    );
    assert_eq!(chain.definition(account(&definition)).authority, None);

    let result = chain.submit(
        account(&minter),
        &[account(&minter), account(&definition), account(&alice)],
        &[&minter],
        &Message::Mint {
            definition: account(&definition),
            to: account(&alice),
            amount: 1,
        },
    );
    assert_refused(&result, "mint authority is renounced");
    assert_eq!(chain.definition(account(&definition)).total_supply, 1_000);
}

#[test]
fn token_a_failing_credit_rolls_back_the_debit() {
    let (mut chain, definition, alice, bob) = (Chain::new(), key(1), key(2), key(3));
    chain.create(&definition, &alice, 1_000, None);
    chain.initialize(&bob, account(&definition)).expect("init");

    let result = chain.submit(
        account(&alice),
        &[account(&alice), account(&bob)],
        &[&alice],
        &Message::Transfer {
            to: account(&bob),
            amount: 1_001,
        },
    );

    assert_refused(&result, "insufficient balance");
    assert_eq!(chain.balance(account(&alice)), 1_000);
    assert_eq!(chain.balance(account(&bob)), 0);
}

/// The same code deployed twice is two programs: the copy's definition is not a definition
/// to the original, so it cannot back a holding of it.
#[test]
fn token_a_definition_of_another_deployment_is_not_a_definition() {
    let ((mut chain, copy), definition, mallory, bob) =
        (Chain::with_copy(), key(5), key(6), key(3));
    chain
        .submit_to(
            copy,
            account(&definition),
            &[account(&definition), account(&mallory)],
            &[&definition, &mallory],
            &Message::NewDefinition {
                name: "Copy".to_owned(),
                total_supply: 1_000,
                mint_authority: None,
                holding: account(&mallory),
            },
        )
        .expect("the copy creates its own token");

    let result = chain.initialize(&bob, account(&definition));

    assert_refused(&result, "receiver is not a token definition");
    assert_eq!(chain.token_state(account(&bob)), None);
}

/// A holding under the copy is no holding to the original: a transfer to it is refused.
#[test]
fn token_a_holding_of_another_deployment_cannot_receive() {
    let ((mut chain, copy), definition, alice, copy_definition, bob) =
        (Chain::with_copy(), key(1), key(2), key(5), key(3));
    chain.create(&definition, &alice, 1_000, None);
    chain
        .submit_to(
            copy,
            account(&copy_definition),
            &[account(&copy_definition), account(&bob)],
            &[&copy_definition, &bob],
            &Message::NewDefinition {
                name: "Copy".to_owned(),
                total_supply: 1_000,
                mint_authority: None,
                holding: account(&bob),
            },
        )
        .expect("bob holds the copy's token");

    let result = chain.submit(
        account(&alice),
        &[account(&alice), account(&bob)],
        &[&alice],
        &Message::Transfer {
            to: account(&bob),
            amount: 300,
        },
    );

    assert_refused(
        &result,
        "recipient has no token holding; initialize it first",
    );
    assert_eq!(chain.balance(account(&alice)), 1_000);
    assert!(matches!(
        chain.token_state_in(copy, account(&bob)),
        Some(TokenState::Holding(TokenHolding { balance: 1_000, .. }))
    ));
}

/// A root call has no sender, so there is nobody to reply to.
#[test]
fn token_read_state_as_a_root_call_fails() {
    let (mut chain, definition, alice) = (Chain::new(), key(1), key(2));
    chain.create(&definition, &alice, 1_000, None);

    let result = chain.submit(
        account(&alice),
        &[account(&alice)],
        &[],
        &Message::ReadState,
    );

    assert_refused(&result, "ReadState needs a sender to reply to");
}
