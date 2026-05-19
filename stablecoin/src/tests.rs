#![allow(
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests deliberately panic on bad state via assert!/#[should_panic] and index fixed-size vectors"
)]

use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::{ChainedCall, Claim, ProgramId},
};
use stablecoin_core::{
    compute_position_pda, compute_position_pda_seed, compute_position_vault_pda,
    compute_position_vault_pda_seed, Position,
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
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: collateral_definition_id(),
    }
}

fn user_holding_account(balance: u128) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: collateral_definition_id(),
                balance,
            }),
            nonce: Nonce(0),
        },
        is_authorized: true,
        account_id: user_holding_id(),
    }
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
    AccountWithMetadata {
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
    }
}

fn destination_holding_account() -> AccountWithMetadata {
    AccountWithMetadata {
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
        account_id: destination_holding_id(),
    }
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
    assert_eq!(position_post.account().program_owner, STABLECOIN_PROGRAM_ID);

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
