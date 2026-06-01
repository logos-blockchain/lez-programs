#![allow(
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests deliberately panic on bad state via assert!/#[should_panic] and index fixed-size vectors"
)]

use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::{AccountPostState, ChainedCall, Claim, ProgramId},
};
use stablecoin_core::{
    compute_position_pda, compute_position_pda_seed, compute_position_vault_pda,
    compute_position_vault_pda_seed, Position, ERR_POSITION_ACCOUNT_ID_MISMATCH,
    ERR_POSITION_VAULT_ACCOUNT_ID_MISMATCH,
};
use token_core::{TokenDefinition, TokenHolding};

const STABLECOIN_PROGRAM_ID: ProgramId = [3u32; 8];
const TOKEN_PROGRAM_ID: ProgramId = [2u32; 8];

fn owner_id() -> AccountId {
    AccountId::new([0x10u8; 32])
}

fn collateral_definition_id() -> AccountId {
    AccountId::new([0x20u8; 32])
}

fn user_holding_id() -> AccountId {
    AccountId::new([0x30u8; 32])
}

fn token_holding_account(
    account_id: AccountId,
    definition_id: AccountId,
    balance: u128,
) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id,
                balance,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id,
    }
}

fn position_id() -> AccountId {
    compute_position_pda(
        STABLECOIN_PROGRAM_ID,
        owner_id(),
        collateral_definition_id(),
    )
}

fn vault_id() -> AccountId {
    compute_position_vault_pda(STABLECOIN_PROGRAM_ID, position_id())
}

fn owner_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: owner_id(),
    }
}

fn collateral_definition_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenDefinition::Fungible {
                name: "SNT".to_owned(),
                total_supply: 1_000_000,
                metadata_id: None,
                authority: None,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: collateral_definition_id(),
    }
}

fn user_holding_account(balance: u128) -> AccountWithMetadata {
    let mut account = token_holding_account(user_holding_id(), collateral_definition_id(), balance);
    account.is_authorized = true;
    account
}

fn uninit_position_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: position_id(),
    }
}

fn uninit_vault_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: vault_id(),
    }
}

fn destination_holding_id() -> AccountId {
    AccountId::new([0x40u8; 32])
}

fn init_position_account(collateral_amount: u128, debt_amount: u128) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: STABLECOIN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&Position {
                collateral_vault_id: vault_id(),
                collateral_definition_id: collateral_definition_id(),
                collateral_amount,
                debt_amount,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: position_id(),
    }
}

fn init_vault_account() -> AccountWithMetadata {
    token_holding_account(vault_id(), collateral_definition_id(), 0)
}

fn destination_holding_account() -> AccountWithMetadata {
    token_holding_account(destination_holding_id(), collateral_definition_id(), 0)
}

fn stablecoin_definition_id() -> AccountId {
    AccountId::new([0x50u8; 32])
}

fn user_stablecoin_holding_id() -> AccountId {
    AccountId::new([0x60u8; 32])
}

fn stablecoin_definition_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenDefinition::Fungible {
                name: "DAI".to_owned(),
                total_supply: 1_000_000,
                metadata_id: None,
                authority: None,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: stablecoin_definition_id(),
    }
}

fn user_stablecoin_holding_account(balance: u128) -> AccountWithMetadata {
    let mut account = token_holding_account(
        user_stablecoin_holding_id(),
        stablecoin_definition_id(),
        balance,
    );
    account.is_authorized = true;
    account
}

struct DepositCollateralFixture {
    owner: AccountWithMetadata,
    position: AccountWithMetadata,
    vault: AccountWithMetadata,
    user_holding: AccountWithMetadata,
    token_definition: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    amount: u128,
}

impl DepositCollateralFixture {
    fn run(self) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
        crate::deposit_collateral::deposit_collateral(
            self.owner,
            self.position,
            self.vault,
            self.user_holding,
            self.token_definition,
            self.stablecoin_program_id,
            self.amount,
        )
    }
}

fn deposit_fixture() -> DepositCollateralFixture {
    DepositCollateralFixture {
        owner: owner_account(),
        position: init_position_account(500, 0),
        vault: init_vault_account(),
        user_holding: user_holding_account(1_000),
        token_definition: collateral_definition_account(),
        stablecoin_program_id: STABLECOIN_PROGRAM_ID,
        amount: 100,
    }
}

fn assert_panics_with_message<F>(action: F, expected: &str)
where
    F: FnOnce(),
{
    let panic =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)).expect_err("expected panic");
    let message = if let Some(message) = panic.downcast_ref::<String>() {
        message.as_str()
    } else if let Some(message) = panic.downcast_ref::<&str>() {
        message
    } else {
        panic!("panic payload must be a string");
    };
    assert!(
        message.contains(expected),
        "panic message `{message}` did not contain `{expected}`"
    );
}

fn assert_deposit_collateral_panics(fixture: DepositCollateralFixture, expected: &str) {
    assert_panics_with_message(
        || {
            fixture.run();
        },
        expected,
    );
}

fn post_state_matching<'a>(
    post_states: &'a [AccountPostState],
    post_state_account_ids: &[AccountId],
    expected_account_id: AccountId,
    label: &str,
    mut predicate: impl FnMut(&AccountPostState) -> bool,
) -> &'a AccountPostState {
    assert_eq!(
        post_states.len(),
        post_state_account_ids.len(),
        "post-state account id list must match post-state length"
    );
    let mut matched_posts =
        post_states
            .iter()
            .zip(post_state_account_ids)
            .filter_map(|(post_state, account_id)| {
                (*account_id == expected_account_id && predicate(post_state)).then_some(post_state)
            });
    let post_state = matched_posts.next().expect(label);
    assert!(
        matched_posts.next().is_none(),
        "expected exactly one {label} post-state"
    );
    post_state
}

fn position_post_state<'a>(
    post_states: &'a [AccountPostState],
    post_state_account_ids: &[AccountId],
    expected_account_id: AccountId,
    expected_position: &Position,
) -> &'a AccountPostState {
    post_state_matching(
        post_states,
        post_state_account_ids,
        expected_account_id,
        "position post-state",
        |post_state| {
            post_state.account().program_owner == STABLECOIN_PROGRAM_ID
                && Position::try_from(&post_state.account().data)
                    .is_ok_and(|position| &position == expected_position)
        },
    )
}

#[test]
fn open_position_claims_pda_and_emits_chained_calls() {
    let collateral_amount: u128 = 500;
    let (post_states, chained_calls) = crate::open_position::open_position(
        owner_account(),
        uninit_position_account(),
        uninit_vault_account(),
        user_holding_account(1_000),
        collateral_definition_account(),
        STABLECOIN_PROGRAM_ID,
        collateral_amount,
    );

    assert_eq!(post_states.len(), 5);

    // Position is PDA-claimed and carries the encoded Position state.
    let position_post = &post_states[1];
    assert_eq!(
        position_post.required_claim(),
        Some(Claim::Pda(compute_position_pda_seed(
            owner_id(),
            collateral_definition_id()
        )))
    );
    let position = Position::try_from(&position_post.account().data).expect("valid Position");
    assert_eq!(
        position,
        Position {
            collateral_vault_id: vault_id(),
            collateral_definition_id: collateral_definition_id(),
            collateral_amount,
            debt_amount: 0,
        }
    );
    // The runtime sets the program_owner on the claimed account after validating Claim::Pda.
    assert_eq!(position_post.account().program_owner, ProgramId::default());

    assert_eq!(chained_calls.len(), 2);

    let mut vault_authorized = uninit_vault_account();
    vault_authorized.is_authorized = true;
    let expected_initialize = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![collateral_definition_account(), vault_authorized],
        &token_core::Instruction::InitializeAccount,
    )
    .with_pda_seeds(vec![compute_position_vault_pda_seed(position_id())]);
    assert_eq!(chained_calls[0], expected_initialize);

    let post_init_vault = AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: collateral_definition_id(),
                balance: 0,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: vault_id(),
    };
    let expected_transfer = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![user_holding_account(1_000), post_init_vault],
        &token_core::Instruction::Transfer {
            amount_to_transfer: collateral_amount,
        },
    );
    assert_eq!(chained_calls[1], expected_transfer);
}

#[test]
#[should_panic(expected = "Owner authorization is missing")]
fn open_position_requires_owner_authorization() {
    let mut owner = owner_account();
    owner.is_authorized = false;

    crate::open_position::open_position(
        owner,
        uninit_position_account(),
        uninit_vault_account(),
        user_holding_account(1_000),
        collateral_definition_account(),
        STABLECOIN_PROGRAM_ID,
        500,
    );
}

#[test]
#[should_panic(expected = "User collateral holding authorization is missing")]
fn open_position_requires_user_holding_authorization() {
    let mut holding = user_holding_account(1_000);
    holding.is_authorized = false;

    crate::open_position::open_position(
        owner_account(),
        uninit_position_account(),
        uninit_vault_account(),
        holding,
        collateral_definition_account(),
        STABLECOIN_PROGRAM_ID,
        500,
    );
}

#[test]
#[should_panic(expected = "Position account must be uninitialized")]
fn open_position_rejects_initialized_position() {
    let position = AccountWithMetadata {
        account: Account {
            program_owner: STABLECOIN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&Position {
                collateral_vault_id: vault_id(),
                collateral_definition_id: collateral_definition_id(),
                collateral_amount: 1,
                debt_amount: 0,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: position_id(),
    };

    crate::open_position::open_position(
        owner_account(),
        position,
        uninit_vault_account(),
        user_holding_account(1_000),
        collateral_definition_account(),
        STABLECOIN_PROGRAM_ID,
        500,
    );
}

#[test]
#[should_panic(expected = "Position vault account must be uninitialized")]
fn open_position_rejects_initialized_vault() {
    let vault = AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: collateral_definition_id(),
                balance: 0,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: vault_id(),
    };

    crate::open_position::open_position(
        owner_account(),
        uninit_position_account(),
        vault,
        user_holding_account(1_000),
        collateral_definition_account(),
        STABLECOIN_PROGRAM_ID,
        500,
    );
}

#[test]
#[should_panic(expected = "Position account ID does not match expected derivation")]
fn open_position_rejects_wrong_position_address() {
    let bad_position = AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: AccountId::new([0xFFu8; 32]),
    };

    crate::open_position::open_position(
        owner_account(),
        bad_position,
        uninit_vault_account(),
        user_holding_account(1_000),
        collateral_definition_account(),
        STABLECOIN_PROGRAM_ID,
        500,
    );
}

#[test]
#[should_panic(expected = "Position vault account ID does not match expected derivation")]
fn open_position_rejects_wrong_vault_address() {
    let bad_vault = AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: AccountId::new([0xEEu8; 32]),
    };

    crate::open_position::open_position(
        owner_account(),
        uninit_position_account(),
        bad_vault,
        user_holding_account(1_000),
        collateral_definition_account(),
        STABLECOIN_PROGRAM_ID,
        500,
    );
}

#[test]
#[should_panic(expected = "User collateral holding does not match the provided token definition")]
fn open_position_rejects_mismatched_token_definition() {
    let other_definition = AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenDefinition::Fungible {
                name: "OTHER".to_owned(),
                total_supply: 1,
                metadata_id: None,
                authority: None,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: AccountId::new([0x21u8; 32]),
    };

    crate::open_position::open_position(
        owner_account(),
        uninit_position_account(),
        uninit_vault_account(),
        user_holding_account(1_000),
        other_definition,
        STABLECOIN_PROGRAM_ID,
        500,
    );
}

#[test]
#[should_panic(
    expected = "Collateral token definition is not owned by the user holding's Token Program"
)]
fn open_position_rejects_definition_with_wrong_token_program() {
    let mut definition = collateral_definition_account();
    definition.account.program_owner = [9u32; 8];

    crate::open_position::open_position(
        owner_account(),
        uninit_position_account(),
        uninit_vault_account(),
        user_holding_account(1_000),
        definition,
        STABLECOIN_PROGRAM_ID,
        500,
    );
}

#[test]
fn position_pda_is_deterministic_and_owner_and_collateral_specific() {
    let id_a = compute_position_pda(
        STABLECOIN_PROGRAM_ID,
        owner_id(),
        collateral_definition_id(),
    );
    let id_b = compute_position_pda(
        STABLECOIN_PROGRAM_ID,
        owner_id(),
        collateral_definition_id(),
    );
    assert_eq!(id_a, id_b);

    let other_owner = AccountId::new([0x11u8; 32]);
    assert_ne!(
        compute_position_pda(
            STABLECOIN_PROGRAM_ID,
            other_owner,
            collateral_definition_id()
        ),
        id_a
    );

    let other_definition = AccountId::new([0x21u8; 32]);
    assert_ne!(
        compute_position_pda(STABLECOIN_PROGRAM_ID, owner_id(), other_definition),
        id_a
    );
}

#[test]
fn position_pda_and_vault_pda_do_not_collide() {
    // Distinct domain tags must keep the position id and its vault id disjoint.
    let position = compute_position_pda(
        STABLECOIN_PROGRAM_ID,
        owner_id(),
        collateral_definition_id(),
    );
    let vault = compute_position_vault_pda(STABLECOIN_PROGRAM_ID, position);
    assert_ne!(position, vault);
}

#[test]
fn withdraw_collateral_updates_position_and_emits_transfer() {
    let initial_collateral: u128 = 500;
    let amount: u128 = 200;
    let (post_states, chained_calls) = crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        init_position_account(initial_collateral, 0),
        init_vault_account(),
        destination_holding_account(),
        STABLECOIN_PROGRAM_ID,
        amount,
    );

    assert_eq!(post_states.len(), 4);

    // Position post-state: plain `new`, holds the decremented Position.
    let position_post = &post_states[1];
    assert_eq!(position_post.required_claim(), None);
    let position = Position::try_from(&position_post.account().data).expect("valid Position");
    assert_eq!(
        position,
        Position {
            collateral_vault_id: vault_id(),
            collateral_definition_id: collateral_definition_id(),
            collateral_amount: initial_collateral - amount,
            debt_amount: 0,
        }
    );
    assert_eq!(position_post.account().program_owner, STABLECOIN_PROGRAM_ID);

    // Vault and destination post-states are pre-transfer (mutation comes via chained call).
    assert_eq!(post_states[2].account(), &init_vault_account().account);
    assert_eq!(
        post_states[3].account(),
        &destination_holding_account().account
    );

    // Single chained Token::Transfer with vault PDA seed.
    assert_eq!(chained_calls.len(), 1);
    let mut vault_authorized = init_vault_account();
    vault_authorized.is_authorized = true;
    let expected_transfer = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![vault_authorized, destination_holding_account()],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
        },
    )
    .with_pda_seeds(vec![compute_position_vault_pda_seed(position_id())]);
    assert_eq!(chained_calls[0], expected_transfer);
}

#[test]
fn deposit_collateral_updates_position_and_emits_transfer() {
    let initial_collateral: u128 = 500;
    let initial_debt: u128 = 300;
    let amount: u128 = 200;
    let holding_balance: u128 = 1_000;
    let position_account = init_position_account(initial_collateral, initial_debt);
    let vault = init_vault_account();
    let user_holding = user_holding_account(holding_balance);

    let (post_states, chained_calls) = crate::deposit_collateral::deposit_collateral(
        owner_account(),
        position_account.clone(),
        vault.clone(),
        user_holding.clone(),
        collateral_definition_account(),
        STABLECOIN_PROGRAM_ID,
        amount,
    );

    assert_eq!(post_states.len(), 2);
    assert!(post_states
        .iter()
        .all(|post_state| post_state.account().program_owner != TOKEN_PROGRAM_ID));

    let expected_position = Position {
        collateral_vault_id: vault_id(),
        collateral_definition_id: collateral_definition_id(),
        collateral_amount: initial_collateral + amount,
        debt_amount: initial_debt,
    };
    let post_state_account_ids = [owner_id(), position_account.account_id];
    let position_post = position_post_state(
        &post_states,
        &post_state_account_ids,
        position_account.account_id,
        &expected_position,
    );
    assert_eq!(position_post.required_claim(), None);
    let position = Position::try_from(&position_post.account().data).expect("valid Position");
    assert_eq!(position, expected_position);
    assert_eq!(position_post.account().program_owner, STABLECOIN_PROGRAM_ID);

    assert_eq!(chained_calls.len(), 1);
    let expected_transfer = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![user_holding, vault],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
        },
    );
    assert_eq!(chained_calls[0], expected_transfer);
}

#[test]
fn deposit_collateral_allows_zero_amount() {
    let initial: u128 = 500;
    let position_account = init_position_account(initial, 0);
    let (post_states, chained_calls) = crate::deposit_collateral::deposit_collateral(
        owner_account(),
        position_account.clone(),
        init_vault_account(),
        user_holding_account(1_000),
        collateral_definition_account(),
        STABLECOIN_PROGRAM_ID,
        0,
    );
    let expected_position = Position {
        collateral_vault_id: vault_id(),
        collateral_definition_id: collateral_definition_id(),
        collateral_amount: initial,
        debt_amount: 0,
    };
    assert_eq!(post_states.len(), 2);
    assert!(post_states
        .iter()
        .all(|post_state| post_state.account().program_owner != TOKEN_PROGRAM_ID));
    let post_state_account_ids = [owner_id(), position_account.account_id];
    let position_post = position_post_state(
        &post_states,
        &post_state_account_ids,
        position_account.account_id,
        &expected_position,
    );
    let position = Position::try_from(&position_post.account().data).expect("valid Position");
    assert_eq!(position.collateral_amount, initial);
    assert!(chained_calls.is_empty());
}

#[test]
fn deposit_collateral_zero_amount_validates_token_definition() {
    let mut fixture = deposit_fixture();
    fixture.amount = 0;
    fixture.token_definition.account.data = Data::try_from(vec![0xFC]).expect("test data fits");

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_TOKEN_DEFINITION_INVALID,
    );
}

#[test]
fn deposit_collateral_zero_amount_validates_vault_owner() {
    let mut fixture = deposit_fixture();
    fixture.amount = 0;
    fixture.vault.account.program_owner = [9u32; 8];

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_TOKEN_PROGRAM_MISMATCH,
    );
}

#[test]
fn deposit_collateral_zero_amount_validates_user_holding_data() {
    let mut fixture = deposit_fixture();
    fixture.amount = 0;
    fixture.user_holding.account.data = Data::try_from(vec![0xFD]).expect("test data fits");

    assert_deposit_collateral_panics(fixture, crate::deposit_collateral::ERR_USER_HOLDING_INVALID);
}

#[test]
fn deposit_collateral_requires_owner_authorization() {
    let mut fixture = deposit_fixture();
    fixture.owner.is_authorized = false;

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_OWNER_AUTHORIZATION_MISSING,
    );
}

#[test]
fn deposit_collateral_requires_user_holding_authorization() {
    let mut fixture = deposit_fixture();
    fixture.user_holding.is_authorized = false;

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_USER_HOLDING_AUTHORIZATION_MISSING,
    );
}

#[test]
fn deposit_collateral_rejects_uninitialized_position() {
    let mut fixture = deposit_fixture();
    fixture.position = uninit_position_account();

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_POSITION_UNINITIALIZED,
    );
}

#[test]
fn deposit_collateral_rejects_default_owned_position_as_uninitialized() {
    let mut fixture = deposit_fixture();
    fixture.position.account.program_owner = ProgramId::default();

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_POSITION_UNINITIALIZED,
    );
}

#[test]
fn deposit_collateral_rejects_position_owned_by_other_program() {
    let mut fixture = deposit_fixture();
    fixture.position.account.program_owner = [9u32; 8];

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_POSITION_WRONG_PROGRAM_OWNER,
    );
}

#[test]
fn deposit_collateral_rejects_invalid_position_data() {
    let mut fixture = deposit_fixture();
    fixture.position.account.data = Data::try_from(vec![0xFB]).expect("test data fits");

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_POSITION_INVALID_STATE,
    );
}

#[test]
fn deposit_collateral_rejects_wrong_position_address() {
    let mut fixture = deposit_fixture();
    fixture.position.account_id = AccountId::new([0xFFu8; 32]);

    assert_deposit_collateral_panics(fixture, ERR_POSITION_ACCOUNT_ID_MISMATCH);
}

#[test]
fn deposit_collateral_rejects_wrong_vault_address() {
    let mut fixture = deposit_fixture();
    fixture.vault.account_id = AccountId::new([0xEEu8; 32]);

    assert_deposit_collateral_panics(fixture, ERR_POSITION_VAULT_ACCOUNT_ID_MISMATCH);
}

#[test]
fn deposit_collateral_rejects_position_vault_id_mismatch() {
    let mut fixture = deposit_fixture();
    fixture.position.account.data = Data::from(&Position {
        collateral_vault_id: AccountId::new([0x71u8; 32]),
        collateral_definition_id: collateral_definition_id(),
        collateral_amount: 500,
        debt_amount: 0,
    });

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_POSITION_VAULT_MISMATCH,
    );
}

#[test]
fn deposit_collateral_rejects_uninitialized_vault() {
    let mut fixture = deposit_fixture();
    fixture.vault = uninit_vault_account();

    assert_deposit_collateral_panics(fixture, crate::deposit_collateral::ERR_VAULT_UNINITIALIZED);
}

#[test]
fn deposit_collateral_rejects_default_owned_vault_as_uninitialized() {
    let mut fixture = deposit_fixture();
    fixture.vault.account.program_owner = ProgramId::default();

    assert_deposit_collateral_panics(fixture, crate::deposit_collateral::ERR_VAULT_UNINITIALIZED);
}

#[test]
fn deposit_collateral_rejects_invalid_vault_holding_data() {
    let mut fixture = deposit_fixture();
    fixture.vault.account.data = Data::try_from(vec![0xFA]).expect("test data fits");

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_VAULT_INVALID_HOLDING,
    );
}

#[test]
fn deposit_collateral_rejects_vault_for_other_definition() {
    let mut fixture = deposit_fixture();
    fixture.vault.account.data = Data::from(&TokenHolding::Fungible {
        definition_id: AccountId::new([0x21u8; 32]),
        balance: 0,
    });

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_VAULT_WRONG_DEFINITION,
    );
}

#[test]
fn deposit_collateral_rejects_nonfungible_vault() {
    let mut fixture = deposit_fixture();
    fixture.vault.account.data = Data::from(&TokenHolding::NftPrintedCopy {
        definition_id: collateral_definition_id(),
        owned: false,
    });

    assert_deposit_collateral_panics(fixture, crate::deposit_collateral::ERR_VAULT_NOT_FUNGIBLE);
}

#[test]
fn deposit_collateral_rejects_vault_definition_owner_mismatch() {
    let mut fixture = deposit_fixture();
    fixture.vault.account.program_owner = [9u32; 8];

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_TOKEN_PROGRAM_MISMATCH,
    );
}

#[test]
fn deposit_collateral_rejects_uninitialized_user_holding() {
    let mut fixture = deposit_fixture();
    fixture.user_holding = AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: user_holding_id(),
    };

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_USER_HOLDING_UNINITIALIZED,
    );
}

#[test]
fn deposit_collateral_rejects_default_owned_user_holding_as_uninitialized() {
    let mut fixture = deposit_fixture();
    fixture.user_holding.account.program_owner = ProgramId::default();

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_USER_HOLDING_UNINITIALIZED,
    );
}

#[test]
fn deposit_collateral_rejects_holding_with_different_token_program() {
    let mut fixture = deposit_fixture();
    fixture.user_holding.account.program_owner = [9u32; 8];

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_TOKEN_PROGRAM_MISMATCH,
    );
}

#[test]
fn deposit_collateral_rejects_holding_for_other_definition() {
    let mut fixture = deposit_fixture();
    fixture.user_holding.account.data = Data::from(&TokenHolding::Fungible {
        definition_id: AccountId::new([0x21u8; 32]),
        balance: 1_000,
    });

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_USER_HOLDING_WRONG_DEFINITION,
    );
}

#[test]
fn deposit_collateral_rejects_insufficient_user_balance() {
    let mut fixture = deposit_fixture();
    fixture.user_holding = user_holding_account(99);

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_USER_HOLDING_INSUFFICIENT_BALANCE,
    );
}

#[test]
fn deposit_collateral_rejects_nonfungible_user_holding() {
    let mut fixture = deposit_fixture();
    fixture.amount = 1;
    fixture.user_holding.account.data = Data::from(&TokenHolding::NftPrintedCopy {
        definition_id: collateral_definition_id(),
        owned: true,
    });

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_USER_HOLDING_NOT_FUNGIBLE,
    );
}

#[test]
fn deposit_collateral_rejects_other_token_definition() {
    let mut fixture = deposit_fixture();
    fixture.token_definition.account_id = AccountId::new([0x21u8; 32]);

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_TOKEN_DEFINITION_MISMATCH,
    );
}

#[test]
fn deposit_collateral_rejects_token_definition_with_wrong_token_program() {
    let mut fixture = deposit_fixture();
    fixture.token_definition.account.program_owner = [9u32; 8];

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_TOKEN_PROGRAM_MISMATCH,
    );
}

#[test]
fn deposit_collateral_rejects_uninitialized_token_definition() {
    let mut fixture = deposit_fixture();
    fixture.token_definition.account = Account::default();

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_TOKEN_DEFINITION_UNINITIALIZED,
    );
}

#[test]
fn deposit_collateral_rejects_invalid_token_definition_data() {
    let mut fixture = deposit_fixture();
    fixture.token_definition.account.data = Data::try_from(vec![0xFF]).expect("test data fits");

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_TOKEN_DEFINITION_INVALID,
    );
}

#[test]
fn deposit_collateral_rejects_nonfungible_token_definition() {
    let mut fixture = deposit_fixture();
    fixture.token_definition.account.data = Data::from(&TokenDefinition::NonFungible {
        name: "NFT".to_owned(),
        printable_supply: 1,
        metadata_id: AccountId::new([0x70u8; 32]),
    });

    assert_deposit_collateral_panics(
        fixture,
        crate::deposit_collateral::ERR_TOKEN_DEFINITION_NOT_FUNGIBLE,
    );
}

#[test]
fn deposit_collateral_rejects_collateral_overflow() {
    let mut fixture = deposit_fixture();
    fixture.position = init_position_account(u128::MAX, 0);
    fixture.amount = 1;

    assert_deposit_collateral_panics(fixture, crate::deposit_collateral::ERR_COLLATERAL_OVERFLOW);
}

#[test]
fn withdraw_collateral_allows_full_drain() {
    let amount: u128 = 500;
    let (post_states, _chained_calls) = crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        init_position_account(amount, 0),
        init_vault_account(),
        destination_holding_account(),
        STABLECOIN_PROGRAM_ID,
        amount,
    );
    let position = Position::try_from(&post_states[1].account().data).expect("valid Position");
    assert_eq!(position.collateral_amount, 0);
    assert_eq!(position.debt_amount, 0);
}

#[test]
fn withdraw_collateral_allows_zero_amount() {
    let initial: u128 = 500;
    let (post_states, chained_calls) = crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        init_position_account(initial, 0),
        init_vault_account(),
        destination_holding_account(),
        STABLECOIN_PROGRAM_ID,
        0,
    );
    let position = Position::try_from(&post_states[1].account().data).expect("valid Position");
    assert_eq!(position.collateral_amount, initial);

    let mut vault_authorized = init_vault_account();
    vault_authorized.is_authorized = true;
    let expected_transfer = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![vault_authorized, destination_holding_account()],
        &token_core::Instruction::Transfer {
            amount_to_transfer: 0,
        },
    )
    .with_pda_seeds(vec![compute_position_vault_pda_seed(position_id())]);
    assert_eq!(chained_calls, vec![expected_transfer]);
}

#[test]
#[should_panic(expected = "Owner authorization is missing")]
fn withdraw_collateral_requires_owner_authorization() {
    let mut owner = owner_account();
    owner.is_authorized = false;
    crate::withdraw_collateral::withdraw_collateral(
        owner,
        init_position_account(500, 0),
        init_vault_account(),
        destination_holding_account(),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Position account must be initialized")]
fn withdraw_collateral_rejects_uninitialized_position() {
    crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        uninit_position_account(),
        init_vault_account(),
        destination_holding_account(),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Position is not owned by this stablecoin program")]
fn withdraw_collateral_rejects_position_owned_by_other_program() {
    let mut position = init_position_account(500, 0);
    position.account.program_owner = [9u32; 8];
    crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        position,
        init_vault_account(),
        destination_holding_account(),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Position account ID does not match expected derivation")]
fn withdraw_collateral_rejects_wrong_position_address() {
    let mut position = init_position_account(500, 0);
    position.account_id = AccountId::new([0xFFu8; 32]);
    crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        position,
        init_vault_account(),
        destination_holding_account(),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Position vault account ID does not match expected derivation")]
fn withdraw_collateral_rejects_wrong_vault_address() {
    let mut vault = init_vault_account();
    vault.account_id = AccountId::new([0xEEu8; 32]);
    crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        init_position_account(500, 0),
        vault,
        destination_holding_account(),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Vault token holding is not for the position's collateral definition")]
fn withdraw_collateral_rejects_vault_for_other_definition() {
    let mut vault = init_vault_account();
    vault.account.data = Data::from(&TokenHolding::Fungible {
        definition_id: AccountId::new([0x21u8; 32]),
        balance: 0,
    });
    crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        init_position_account(500, 0),
        vault,
        destination_holding_account(),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Destination must be initialized")]
fn withdraw_collateral_rejects_uninitialized_destination() {
    let destination = AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: destination_holding_id(),
    };
    crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        init_position_account(500, 0),
        init_vault_account(),
        destination,
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Destination must be owned by the same Token Program as the vault")]
fn withdraw_collateral_rejects_destination_with_wrong_token_program() {
    let mut destination = destination_holding_account();
    destination.account.program_owner = [9u32; 8];
    crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        init_position_account(500, 0),
        init_vault_account(),
        destination,
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(
    expected = "Destination token definition does not match the position's collateral definition"
)]
fn withdraw_collateral_rejects_destination_for_other_definition() {
    let mut destination = destination_holding_account();
    destination.account.data = Data::from(&TokenHolding::Fungible {
        definition_id: AccountId::new([0x21u8; 32]),
        balance: 0,
    });
    crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        init_position_account(500, 0),
        init_vault_account(),
        destination,
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "withdraw_collateral with debt is not supported yet")]
fn withdraw_collateral_rejects_withdrawal_with_outstanding_debt() {
    crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        init_position_account(500, 1),
        init_vault_account(),
        destination_holding_account(),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Withdrawal amount exceeds position collateral")]
fn withdraw_collateral_rejects_overdraw() {
    crate::withdraw_collateral::withdraw_collateral(
        owner_account(),
        init_position_account(100, 0),
        init_vault_account(),
        destination_holding_account(),
        STABLECOIN_PROGRAM_ID,
        200,
    );
}

#[test]
fn repay_debt_decreases_debt_and_emits_burn() {
    let initial_collateral: u128 = 500;
    let initial_debt: u128 = 300;
    let amount: u128 = 100;
    let holding_balance: u128 = 1_000;

    let (post_states, chained_calls) = crate::repay_debt::repay_debt(
        owner_account(),
        init_position_account(initial_collateral, initial_debt),
        stablecoin_definition_account(),
        user_stablecoin_holding_account(holding_balance),
        STABLECOIN_PROGRAM_ID,
        amount,
    );

    assert_eq!(post_states.len(), 4);

    // Position post-state: plain `new`, holds the decremented Position.
    let position_post = &post_states[1];
    assert_eq!(position_post.required_claim(), None);
    let position = Position::try_from(&position_post.account().data).expect("valid Position");
    assert_eq!(
        position,
        Position {
            collateral_vault_id: vault_id(),
            collateral_definition_id: collateral_definition_id(),
            collateral_amount: initial_collateral,
            debt_amount: initial_debt - amount,
        }
    );
    assert_eq!(position_post.account().program_owner, STABLECOIN_PROGRAM_ID);

    // Stablecoin definition and user holding post-states are pre-burn.
    assert_eq!(
        post_states[2].account(),
        &stablecoin_definition_account().account
    );
    assert_eq!(
        post_states[3].account(),
        &user_stablecoin_holding_account(holding_balance).account
    );

    // Single chained Token::Burn, no PDA seeds (user-authorized burn source).
    assert_eq!(chained_calls.len(), 1);
    let expected_burn = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![
            stablecoin_definition_account(),
            user_stablecoin_holding_account(holding_balance),
        ],
        &token_core::Instruction::Burn {
            amount_to_burn: amount,
        },
    );
    assert_eq!(chained_calls[0], expected_burn);
}

#[test]
fn repay_debt_allows_full_repayment() {
    let debt: u128 = 300;
    let (post_states, _chained_calls) = crate::repay_debt::repay_debt(
        owner_account(),
        init_position_account(500, debt),
        stablecoin_definition_account(),
        user_stablecoin_holding_account(1_000),
        STABLECOIN_PROGRAM_ID,
        debt,
    );
    let position = Position::try_from(&post_states[1].account().data).expect("valid Position");
    assert_eq!(position.debt_amount, 0);
    assert_eq!(position.collateral_amount, 500);
}

#[test]
fn repay_debt_allows_zero_amount() {
    let initial_debt: u128 = 300;
    let (post_states, chained_calls) = crate::repay_debt::repay_debt(
        owner_account(),
        init_position_account(500, initial_debt),
        stablecoin_definition_account(),
        user_stablecoin_holding_account(1_000),
        STABLECOIN_PROGRAM_ID,
        0,
    );
    let position = Position::try_from(&post_states[1].account().data).expect("valid Position");
    assert_eq!(position.debt_amount, initial_debt);

    let expected_burn = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![
            stablecoin_definition_account(),
            user_stablecoin_holding_account(1_000),
        ],
        &token_core::Instruction::Burn { amount_to_burn: 0 },
    );
    assert_eq!(chained_calls, vec![expected_burn]);
}

#[test]
#[should_panic(expected = "Owner authorization is missing")]
fn repay_debt_requires_owner_authorization() {
    let mut owner = owner_account();
    owner.is_authorized = false;
    crate::repay_debt::repay_debt(
        owner,
        init_position_account(500, 300),
        stablecoin_definition_account(),
        user_stablecoin_holding_account(1_000),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Position account must be initialized")]
fn repay_debt_rejects_uninitialized_position() {
    crate::repay_debt::repay_debt(
        owner_account(),
        uninit_position_account(),
        stablecoin_definition_account(),
        user_stablecoin_holding_account(1_000),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Position is not owned by this stablecoin program")]
fn repay_debt_rejects_position_owned_by_other_program() {
    let mut position = init_position_account(500, 300);
    position.account.program_owner = [9u32; 8];
    crate::repay_debt::repay_debt(
        owner_account(),
        position,
        stablecoin_definition_account(),
        user_stablecoin_holding_account(1_000),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Position account ID does not match expected derivation")]
fn repay_debt_rejects_wrong_position_address() {
    let mut position = init_position_account(500, 300);
    position.account_id = AccountId::new([0xFFu8; 32]);
    crate::repay_debt::repay_debt(
        owner_account(),
        position,
        stablecoin_definition_account(),
        user_stablecoin_holding_account(1_000),
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "User stablecoin holding authorization is missing")]
fn repay_debt_requires_user_holding_authorization() {
    let mut holding = user_stablecoin_holding_account(1_000);
    holding.is_authorized = false;
    crate::repay_debt::repay_debt(
        owner_account(),
        init_position_account(500, 300),
        stablecoin_definition_account(),
        holding,
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "User stablecoin holding must be initialized")]
fn repay_debt_rejects_uninitialized_user_holding() {
    let holding = AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: user_stablecoin_holding_id(),
    };
    crate::repay_debt::repay_debt(
        owner_account(),
        init_position_account(500, 300),
        stablecoin_definition_account(),
        holding,
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(
    expected = "Stablecoin holding and definition must be owned by the same Token Program"
)]
fn repay_debt_rejects_holding_with_different_token_program() {
    let mut holding = user_stablecoin_holding_account(1_000);
    holding.account.program_owner = [9u32; 8];
    crate::repay_debt::repay_debt(
        owner_account(),
        init_position_account(500, 300),
        stablecoin_definition_account(),
        holding,
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Stablecoin holding does not match the provided stablecoin definition")]
fn repay_debt_rejects_holding_for_other_definition() {
    let mut holding = user_stablecoin_holding_account(1_000);
    holding.account.data = Data::from(&TokenHolding::Fungible {
        definition_id: AccountId::new([0x21u8; 32]),
        balance: 1_000,
    });
    crate::repay_debt::repay_debt(
        owner_account(),
        init_position_account(500, 300),
        stablecoin_definition_account(),
        holding,
        STABLECOIN_PROGRAM_ID,
        100,
    );
}

#[test]
#[should_panic(expected = "Repay amount exceeds outstanding debt")]
fn repay_debt_rejects_overrepay() {
    crate::repay_debt::repay_debt(
        owner_account(),
        init_position_account(500, 100),
        stablecoin_definition_account(),
        user_stablecoin_holding_account(1_000),
        STABLECOIN_PROGRAM_ID,
        200,
    );
}
