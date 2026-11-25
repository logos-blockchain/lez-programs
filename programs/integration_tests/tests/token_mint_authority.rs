//! The testnet faucet, with the token and clock guests deployed to the same chain.

#[path = "token_mint_authority/privacy.rs"]
mod privacy;

use clock_core::clock_actor;
use integration_tests::{account, assert_refused, deploy_at, key, submit, tick, with_clock};
use lee::{error::LeeError, PrivateKey, V03State};
use lee_core::{
    account::{AccountId, Actor},
    program::TransactionEvent,
};
use token_core::{Message as TokenMessage, TokenState};
use token_methods::TOKEN_ELF;
use token_mint_authority_core::{
    compute_mint_allowance_pda, compute_mint_authority_pda, FaucetState, Message,
    FAUCET_MINT_AMOUNT, MINT_COOLDOWN_MS,
};
use token_mint_authority_methods::TOKEN_MINT_AUTHORITY_ELF;

const TOKEN: AccountId = AccountId::new([0xa0; 32]);
const FAUCET: AccountId = AccountId::new([0xa1; 32]);
const T0: u64 = 1_700_000_000_000;

fn token(id: AccountId) -> Actor {
    Actor::new(id, TOKEN)
}

fn faucet(id: AccountId) -> Actor {
    Actor::new(id, FAUCET)
}

fn claim_message(definition: AccountId, holding: AccountId) -> Message {
    Message::FaucetMint {
        token_program_id: TOKEN,
        definition,
        to: holding,
    }
}

fn claim_actors(recipient: AccountId, definition: AccountId, holding: AccountId) -> Vec<Actor> {
    vec![
        faucet(recipient),
        faucet(compute_mint_allowance_pda(
            FAUCET, TOKEN, recipient, definition,
        )),
        clock_actor(),
        faucet(holding),
        token(holding),
        token(compute_mint_authority_pda(FAUCET, TOKEN)),
        token(definition),
    ]
}

struct Chain {
    state: V03State,
}

impl Chain {
    fn new() -> Self {
        Self {
            state: with_clock(
                deploy_at(&[(TOKEN, TOKEN_ELF), (FAUCET, TOKEN_MINT_AUTHORITY_ELF)]),
                T0,
            ),
        }
    }

    /// Create the definition through the token guest. Its initial holding has zero balance.
    fn define(&mut self, definition: &PrivateKey, holder: &PrivateKey, authority: AccountId) {
        let holding = account(holder);
        let root = token(account(definition));
        submit(
            &mut self.state,
            root,
            &[root, token(holding)],
            &[definition, holder],
            &TokenMessage::NewDefinition {
                name: "Faucet".to_owned(),
                total_supply: 0,
                mint_authority: Some(authority),
                holding,
            },
        )
        .expect("create mintable definition");
    }

    fn token_state(&self, id: AccountId) -> Option<TokenState> {
        TokenState::decode(self.state.get_account_by_id(id).data.actor_state(TOKEN))
    }

    fn supply(&self, id: AccountId) -> u128 {
        match self.token_state(id) {
            Some(TokenState::Definition(state)) => state.total_supply,
            other => panic!("expected token definition, got {other:?}"),
        }
    }

    fn balance(&self, id: AccountId) -> u128 {
        match self.token_state(id) {
            Some(TokenState::Holding(state)) => state.balance,
            other => panic!("expected token holding, got {other:?}"),
        }
    }

    fn claim(
        &mut self,
        recipient: AccountId,
        definition: AccountId,
        holding: AccountId,
        signers: &[&PrivateKey],
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        submit(
            &mut self.state,
            faucet(recipient),
            &claim_actors(recipient, definition, holding),
            signers,
            &claim_message(definition, holding),
        )
    }

    fn last_mint(&self, recipient: AccountId, definition: AccountId) -> Option<u64> {
        let id = compute_mint_allowance_pda(FAUCET, TOKEN, recipient, definition);
        let account = self.state.get_account_by_id(id);
        let state = account.data.actor_state(FAUCET);
        match FaucetState::decode(state) {
            Some(FaucetState::Allowance { last_mint_ms }) => Some(last_mint_ms),
            None => None,
            other => panic!("expected settled allowance, got {other:?}"),
        }
    }
}

#[test]
fn fixed_grant_to_an_existing_holding_reopens_at_exactly_one_day() {
    let mut chain = Chain::new();
    let (definition, holding, recipient) = (key(1), key(2), key(3));
    chain.define(
        &definition,
        &holding,
        compute_mint_authority_pda(FAUCET, TOKEN),
    );
    let (def, to, who) = (account(&definition), account(&holding), account(&recipient));

    // The public holding is already initialized. Only the rate-limited recipient signs.
    chain
        .claim(who, def, to, &[&recipient])
        .expect("first grant");
    assert_eq!(chain.supply(def), FAUCET_MINT_AMOUNT);
    assert_eq!(chain.balance(to), FAUCET_MINT_AMOUNT);
    assert_eq!(chain.last_mint(who, def), Some(T0));

    tick(&mut chain.state, T0 + MINT_COOLDOWN_MS - 1);
    let early = chain.claim(who, def, to, &[&recipient]);
    assert_refused(&early, "cooldown");
    assert_eq!(chain.supply(def), FAUCET_MINT_AMOUNT);
    assert_eq!(chain.last_mint(who, def), Some(T0));

    tick(&mut chain.state, T0 + MINT_COOLDOWN_MS);
    chain
        .claim(who, def, to, &[&recipient])
        .expect("next day's grant");
    assert_eq!(chain.supply(def), 2 * FAUCET_MINT_AMOUNT);
    assert_eq!(chain.balance(to), 2 * FAUCET_MINT_AMOUNT);
    assert_eq!(chain.last_mint(who, def), Some(T0 + MINT_COOLDOWN_MS));
}

#[test]
fn funding_a_fresh_holding_requires_its_consent_and_a_failed_claim_can_retry() {
    let mut chain = Chain::new();
    let (definition, initial, recipient, fresh) = (key(1), key(2), key(3), key(4));
    chain.define(
        &definition,
        &initial,
        compute_mint_authority_pda(FAUCET, TOKEN),
    );
    let (def, to, who) = (account(&definition), account(&fresh), account(&recipient));

    // Admit the account without initializing a holding. This exercises the guest's
    // authorization check even though the runtime can already deliver messages to it.
    chain.state = chain.state.with_empty_public_accounts([to]);
    let unsigned = chain.claim(who, def, to, &[&recipient]);
    assert_refused(
        &unsigned,
        "destination must be an initialized token holding",
    );
    assert_eq!(chain.token_state(to), None);
    assert_eq!(chain.last_mint(who, def), None);
    assert_eq!(chain.supply(def), 0);

    chain
        .claim(who, def, to, &[&recipient, &fresh])
        .expect("initialize and mint");
    assert_eq!(chain.balance(to), FAUCET_MINT_AMOUNT);
    assert_eq!(chain.last_mint(who, def), Some(T0));
}

#[test]
fn a_failed_authority_check_rolls_back_the_allowance_and_the_holding_initialization() {
    let mut chain = Chain::new();
    let (definition, initial, recipient, fresh, other_authority) =
        (key(1), key(2), key(3), key(4), key(5));
    chain.define(&definition, &initial, account(&other_authority));
    let (def, to, who) = (account(&definition), account(&fresh), account(&recipient));

    let rejected = chain.claim(who, def, to, &[&recipient, &fresh]);

    assert_refused(&rejected, "sender is not the mint authority");
    assert_eq!(chain.token_state(to), None);
    assert_eq!(chain.last_mint(who, def), None);
    assert_eq!(chain.supply(def), 0);
}

#[test]
fn the_cooldown_is_independent_for_each_recipient_and_definition() {
    let mut chain = Chain::new();
    let (first_def, first_to, second_def, second_to) = (key(1), key(2), key(3), key(4));
    let (alice, bob) = (key(5), key(6));
    let authority = compute_mint_authority_pda(FAUCET, TOKEN);
    chain.define(&first_def, &first_to, authority);
    chain.define(&second_def, &second_to, authority);

    for (who, def, to) in [
        (&alice, &first_def, &first_to),
        (&bob, &first_def, &first_to),
        (&alice, &second_def, &second_to),
    ] {
        chain
            .claim(account(who), account(def), account(to), &[who])
            .expect("independent grant");
        assert_eq!(chain.last_mint(account(who), account(def)), Some(T0));
    }
    assert_eq!(chain.balance(account(&first_to)), 2 * FAUCET_MINT_AMOUNT);
    assert_eq!(chain.balance(account(&second_to)), FAUCET_MINT_AMOUNT);
}

#[test]
fn a_recipient_can_claim_directly_into_its_own_fresh_holding() {
    let mut chain = Chain::new();
    let (definition, initial, recipient) = (key(1), key(2), key(3));
    chain.define(
        &definition,
        &initial,
        compute_mint_authority_pda(FAUCET, TOKEN),
    );
    let (def, who) = (account(&definition), account(&recipient));

    chain
        .claim(who, def, who, &[&recipient])
        .expect("one signature authorizes both roles");

    assert_eq!(chain.balance(who), FAUCET_MINT_AMOUNT);
    assert_eq!(chain.last_mint(who, def), Some(T0));
}

#[test]
fn the_holding_owner_cannot_spend_another_recipients_allowance() {
    let mut chain = Chain::new();
    let (definition, holder, recipient) = (key(1), key(2), key(3));
    chain.define(
        &definition,
        &holder,
        compute_mint_authority_pda(FAUCET, TOKEN),
    );
    let (def, to, who) = (account(&definition), account(&holder), account(&recipient));
    chain.state = chain.state.with_empty_public_accounts([who]);

    let rejected = chain.claim(who, def, to, &[&holder]);

    assert_refused(&rejected, "recipient must authorize faucet mint");
    assert_eq!(chain.last_mint(who, def), None);
    assert_eq!(chain.supply(def), 0);
}

#[test]
fn omitting_the_public_holding_rolls_back_the_claim() {
    use lee::error::InvalidProgramBehaviorError;
    use lee_core::execution_state::ExecutionError;

    let mut chain = Chain::new();
    let (definition, holder, recipient) = (key(1), key(2), key(3));
    chain.define(
        &definition,
        &holder,
        compute_mint_authority_pda(FAUCET, TOKEN),
    );
    let (def, to, who) = (account(&definition), account(&holder), account(&recipient));
    let mut declared = claim_actors(who, def, to);
    declared.retain(|actor| *actor != token(to));

    let result = submit(
        &mut chain.state,
        faucet(who),
        &declared,
        &[&recipient],
        &claim_message(def, to),
    );

    assert!(
        matches!(
            &result,
            Err(LeeError::InvalidProgramBehavior(InvalidProgramBehaviorError::Execution(
                ExecutionError::UndeclaredActor { actor }
            ))) if *actor == token(to)
        ),
        "expected undeclared token holding, got {result:?}",
    );
    assert_eq!(chain.supply(def), 0);
    assert_eq!(chain.balance(to), 0);
    assert_eq!(chain.last_mint(who, def), None);
}
