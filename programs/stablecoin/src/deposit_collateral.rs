use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId, DEFAULT_PROGRAM_ID},
};
use stablecoin_core::{verify_position_and_get_seed, verify_position_vault_and_get_seed, Position};
use token_core::{TokenDefinition, TokenHolding};

pub(crate) const ERR_OWNER_AUTHORIZATION_MISSING: &str = "Owner authorization is missing";
pub(crate) const ERR_USER_HOLDING_AUTHORIZATION_MISSING: &str =
    "User collateral holding authorization is missing";
pub(crate) const ERR_POSITION_UNINITIALIZED: &str = "Position account must be initialized";
pub(crate) const ERR_POSITION_WRONG_PROGRAM_OWNER: &str =
    "Position is not owned by this stablecoin program";
pub(crate) const ERR_VAULT_UNINITIALIZED: &str = "Vault must be initialized";
pub(crate) const ERR_USER_HOLDING_UNINITIALIZED: &str =
    "User collateral holding must be initialized";
pub(crate) const ERR_POSITION_INVALID_STATE: &str =
    "Position account must hold valid Position state";
pub(crate) const ERR_POSITION_VAULT_MISMATCH: &str =
    "Position collateral vault does not match provided vault";
pub(crate) const ERR_TOKEN_DEFINITION_MISMATCH: &str =
    "Token definition does not match the position's collateral definition";
pub(crate) const ERR_TOKEN_DEFINITION_UNINITIALIZED: &str =
    "Collateral token definition must be initialized";
pub(crate) const ERR_TOKEN_DEFINITION_INVALID: &str =
    "Collateral token definition must hold a valid TokenDefinition";
pub(crate) const ERR_TOKEN_DEFINITION_NOT_FUNGIBLE: &str =
    "Collateral token definition must be fungible";
pub(crate) const ERR_TOKEN_PROGRAM_MISMATCH: &str =
    "Collateral token definition, position vault, and user collateral holding must be owned by the same Token Program";
pub(crate) const ERR_VAULT_INVALID_HOLDING: &str = "Vault account must hold a valid TokenHolding";
pub(crate) const ERR_VAULT_WRONG_DEFINITION: &str =
    "Vault token holding is not for the position's collateral definition";
pub(crate) const ERR_VAULT_NOT_FUNGIBLE: &str = "Position vault must be fungible";
pub(crate) const ERR_USER_HOLDING_INVALID: &str =
    "User collateral holding must hold a valid TokenHolding";
pub(crate) const ERR_USER_HOLDING_WRONG_DEFINITION: &str =
    "User collateral holding does not match the position's collateral definition";
pub(crate) const ERR_USER_HOLDING_INSUFFICIENT_BALANCE: &str =
    "Deposit amount exceeds user collateral balance";
pub(crate) const ERR_USER_HOLDING_NOT_FUNGIBLE: &str = "User collateral holding must be fungible";
pub(crate) const ERR_COLLATERAL_OVERFLOW: &str = "Deposit amount overflows position collateral";

fn account_is_initialized(account: &Account) -> bool {
    // Runtime account claims assign a non-default owner; default-owned accounts are still
    // uninitialized for Stablecoin account validation even if other fields are non-default.
    account.program_owner != DEFAULT_PROGRAM_ID
}

/// Deposit `amount` collateral tokens from `user_holding` into `position`'s vault.
///
/// Increases `Position.collateral_amount` by `amount` and emits a single chained
/// [`token_core::Instruction::Transfer`] from the user holding to the vault when `amount` is
/// nonzero. The token program is anchored to the collateral token definition, and the vault and
/// user holding must be owned by that same program.
/// Only the owner alignment state and updated position are returned as stablecoin post-states.
/// Token-account balance post-states are produced by the chained transfer in the token program.
/// No collateralization check is required because debt is unchanged.
///
/// # Panics
/// - `owner` or `user_holding` is not authorized.
/// - `position` is uninitialized, not owned by `stablecoin_program_id`, holds data that does not
///   decode as a [`Position`], or sits at an address that does not match
///   `compute_position_pda(stablecoin_program_id, owner, Position.collateral_definition_id)`.
/// - `vault` is uninitialized, sits at an address that does not match
///   `compute_position_vault_pda(stablecoin_program_id, position_id)`, is not owned by the
///   collateral Token Program, holds a [`TokenHolding`] whose `definition_id` does not match the
///   position's collateral definition, or is not fungible.
/// - `user_holding` is uninitialized, owned by a different Token Program than the collateral
///   definition, or holds a [`TokenHolding`] whose `definition_id` does not match the position's
///   collateral definition, is not fungible, or has less than `amount` balance.
/// - `token_definition` is uninitialized, does not match `Position.collateral_definition_id`, is
///   owned by a different Token Program than the vault, does not hold a valid [`TokenDefinition`],
///   or is not fungible.
/// - `Position.collateral_amount + amount` overflows.
pub fn deposit_collateral(
    owner: AccountWithMetadata,
    position: AccountWithMetadata,
    vault: AccountWithMetadata,
    user_holding: AccountWithMetadata,
    token_definition: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    amount: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    if !owner.is_authorized {
        panic!("{ERR_OWNER_AUTHORIZATION_MISSING}");
    }
    if !user_holding.is_authorized {
        panic!("{ERR_USER_HOLDING_AUTHORIZATION_MISSING}");
    }
    if !account_is_initialized(&position.account) {
        panic!("{ERR_POSITION_UNINITIALIZED}");
    }
    if position.account.program_owner != stablecoin_program_id {
        panic!("{ERR_POSITION_WRONG_PROGRAM_OWNER}");
    }
    if !account_is_initialized(&vault.account) {
        panic!("{ERR_VAULT_UNINITIALIZED}");
    }
    if !account_is_initialized(&user_holding.account) {
        panic!("{ERR_USER_HOLDING_UNINITIALIZED}");
    }

    let position_data = Position::try_from(&position.account.data)
        .unwrap_or_else(|error| panic!("{ERR_POSITION_INVALID_STATE}: {error:?}"));
    let _ = verify_position_and_get_seed(
        &position,
        &owner,
        position_data.collateral_definition_id,
        stablecoin_program_id,
    );
    let _ = verify_position_vault_and_get_seed(&vault, position.account_id, stablecoin_program_id);
    if position_data.collateral_vault_id != vault.account_id {
        panic!("{ERR_POSITION_VAULT_MISMATCH}");
    }

    if !account_is_initialized(&token_definition.account) {
        panic!("{ERR_TOKEN_DEFINITION_UNINITIALIZED}");
    }
    if token_definition.account_id != position_data.collateral_definition_id {
        panic!("{ERR_TOKEN_DEFINITION_MISMATCH}");
    }
    match TokenDefinition::try_from(&token_definition.account.data)
        .unwrap_or_else(|error| panic!("{ERR_TOKEN_DEFINITION_INVALID}: {error:?}"))
    {
        TokenDefinition::Fungible { .. } => {}
        TokenDefinition::NonFungible { .. } => panic!("{ERR_TOKEN_DEFINITION_NOT_FUNGIBLE}"),
    }

    let token_program_id = token_definition.account.program_owner;
    if vault.account.program_owner != token_program_id {
        panic!("{ERR_TOKEN_PROGRAM_MISMATCH}");
    }

    let vault_holding = TokenHolding::try_from(&vault.account.data)
        .unwrap_or_else(|error| panic!("{ERR_VAULT_INVALID_HOLDING}: {error:?}"));
    if vault_holding.definition_id() != position_data.collateral_definition_id {
        panic!("{ERR_VAULT_WRONG_DEFINITION}");
    }
    match vault_holding {
        TokenHolding::Fungible { .. } => {}
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("{ERR_VAULT_NOT_FUNGIBLE}");
        }
    }

    if user_holding.account.program_owner != token_program_id {
        panic!("{ERR_TOKEN_PROGRAM_MISMATCH}");
    }
    let user_holding_data = TokenHolding::try_from(&user_holding.account.data)
        .unwrap_or_else(|error| panic!("{ERR_USER_HOLDING_INVALID}: {error:?}"));
    if user_holding_data.definition_id() != position_data.collateral_definition_id {
        panic!("{ERR_USER_HOLDING_WRONG_DEFINITION}");
    }
    match user_holding_data {
        TokenHolding::Fungible { balance, .. } => {
            if balance < amount {
                panic!("{ERR_USER_HOLDING_INSUFFICIENT_BALANCE}");
            }
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("{ERR_USER_HOLDING_NOT_FUNGIBLE}");
        }
    }

    let new_collateral = position_data
        .collateral_amount
        .checked_add(amount)
        .unwrap_or_else(|| panic!("{ERR_COLLATERAL_OVERFLOW}"));

    let updated_position = Position {
        collateral_vault_id: position_data.collateral_vault_id,
        collateral_definition_id: position_data.collateral_definition_id,
        collateral_amount: new_collateral,
        debt_amount: position_data.debt_amount,
    };
    let mut position_post = position.account.clone();
    position_post.data = Data::from(&updated_position);

    let post_states = vec![
        AccountPostState::new(owner.account),
        AccountPostState::new(position_post),
    ];

    if amount == 0 {
        return (post_states, vec![]);
    }

    let transfer_call = ChainedCall::new(
        token_program_id,
        vec![user_holding, vault],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
        },
    );

    (post_states, vec![transfer_call])
}
