use lee_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, Claim, ProgramId},
};
use program_revert::UnwrapOrRevert as _;
use token_core::{error, TokenDefinition, TokenHolding};

/// Mint additional supply under **self/PDA authority**: the definition account
/// itself is the current mint authority and proves it by being authorized in
/// this transaction (a signer, or a PDA authorized under its seeds — e.g. the
/// AMM minting its own LP token via a chained call).
pub fn mint(
    definition_account: AccountWithMetadata,
    user_holding_account: AccountWithMetadata,
    amount_to_mint: u128,
    token_program_id: ProgramId,
) -> Vec<AccountPostState> {
    mint_inner(
        definition_account,
        user_holding_account,
        None,
        amount_to_mint,
        token_program_id,
    )
}

/// Mint additional supply under an **external authority**: a distinct account
/// (e.g. an owner key the authority was rotated to) proves authority by signing.
/// The definition account is still mutated but does not authorize the mint,
/// which is what lets a rotated authority mint without the definition's key.
pub fn mint_with_authority(
    definition_account: AccountWithMetadata,
    user_holding_account: AccountWithMetadata,
    authority_account: AccountWithMetadata,
    amount_to_mint: u128,
    token_program_id: ProgramId,
) -> Vec<AccountPostState> {
    mint_inner(
        definition_account,
        user_holding_account,
        Some(authority_account),
        amount_to_mint,
        token_program_id,
    )
}

/// Shared minting core for both authority modes. `authority_account` is the
/// external authority when `Some`; when `None` the definition account itself is
/// treated as the authority (self/PDA authority). Post-state order mirrors the
/// pre-state account order for each mode.
fn mint_inner(
    definition_account: AccountWithMetadata,
    user_holding_account: AccountWithMetadata,
    authority_account: Option<AccountWithMetadata>,
    amount_to_mint: u128,
    token_program_id: ProgramId,
) -> Vec<AccountPostState> {
    program_revert::require_eq!(
        error::INVALID_INPUT,
        definition_account.account.program_owner,
        token_program_id,
        "Token definition must be owned by token program"
    );

    let mut definition = TokenDefinition::try_from(&definition_account.account.data)
        .unwrap_or_revert(
            error::INVALID_INPUT,
            "Token Definition account must be valid",
        );

    // Minting is gated on the definition's stored mint authority: the account
    // that proves authority must be authorized AND its id must match the stored
    // authority. That account is the explicit external authority when present,
    // otherwise the definition account itself (self/PDA authority).
    if let TokenDefinition::Fungible { authority, .. } = &definition {
        // `None` means the supply is permanently fixed (renounced) — minting is rejected.
        let mint_authority = authority.unwrap_or_revert(
            error::INVALID_INPUT,
            "Mint authority check failed: authority revoked, supply is fixed",
        );
        let authority_ref = authority_account.as_ref().unwrap_or(&definition_account);
        program_revert::require!(
            error::INVALID_INPUT,
            authority_ref.is_authorized,
            "Mint authority must authorize the transaction"
        );
        program_revert::require_eq!(
            error::INVALID_INPUT,
            authority_ref.account_id,
            mint_authority,
            "Mint authority check failed: signer is not the current authority"
        );
    }

    let mut holding = if user_holding_account.account == Account::default() {
        TokenHolding::zeroized_from_definition(definition_account.account_id, &definition)
    } else {
        TokenHolding::try_from(&user_holding_account.account.data)
            .unwrap_or_revert(error::INVALID_INPUT, "Token Holding account must be valid")
    };

    program_revert::require_eq!(
        error::INVALID_INPUT,
        definition_account.account_id,
        holding.definition_id(),
        "Mismatch Token Definition and Token Holding"
    );

    match (&mut definition, &mut holding) {
        (
            TokenDefinition::Fungible {
                name: _,
                metadata_id: _,
                total_supply,
                authority: _,
            },
            TokenHolding::Fungible {
                definition_id: _,
                balance,
            },
        ) => {
            *balance = balance
                .checked_add(amount_to_mint)
                .unwrap_or_revert(error::ARITHMETIC, "Balance overflow on minting");

            *total_supply = total_supply
                .checked_add(amount_to_mint)
                .unwrap_or_revert(error::ARITHMETIC, "Total supply overflow");
        }
        (
            TokenDefinition::NonFungible { .. },
            TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. },
        ) => {
            program_revert::revert!(
                error::INVALID_INPUT,
                "Cannot mint additional supply for Non-Fungible Tokens"
            );
        }
        _ => program_revert::revert!(
            error::INVALID_INPUT,
            "Mismatched Token Definition and Token Holding types"
        ),
    }

    let mut definition_post = definition_account.account;
    definition_post.data = Data::from(&definition);

    let mut holding_post = user_holding_account.account;
    holding_post.data = Data::from(&holding);

    // Post-states must match pre-state order and count: [definition, holding]
    // for self authority, plus the read-only authority account when external.
    let mut post_states = Vec::with_capacity(3);
    post_states.push(AccountPostState::new(definition_post));
    post_states.push(AccountPostState::new_claimed_if_default(
        holding_post,
        Claim::Authorized,
    ));
    if let Some(authority) = authority_account {
        post_states.push(AccountPostState::new(authority.account));
    }
    post_states
}
