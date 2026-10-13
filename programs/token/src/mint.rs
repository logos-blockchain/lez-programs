use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, BalanceDiff, Data},
    program::AccountStateDiff,
};
use token_core::{TokenDefinition, TokenHolding};

/// Mint additional supply under **self/PDA authority**: the definition account
/// itself is the current mint authority and proves it by being authorized in
/// this transaction (a signer, or a PDA authorized under its seeds — e.g. the
/// AMM minting its own LP token via a chained call).
pub fn mint(
    definition_account: AccountWithMetadata,
    user_holding_account: AccountWithMetadata,
    amount_to_mint: u128,
    token_program_id: AccountId,
) -> Vec<AccountStateDiff> {
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
    token_program_id: AccountId,
) -> Vec<AccountStateDiff> {
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
    token_program_id: AccountId,
) -> Vec<AccountStateDiff> {
    assert_eq!(
        definition_account.account.program_owner, token_program_id,
        "Token definition must be owned by token program"
    );

    let mut definition = TokenDefinition::try_from(&definition_account.account.data)
        .expect("Token Definition account must be valid");

    // Minting is gated on the definition's stored mint authority: the account
    // that proves authority must be authorized AND its id must match the stored
    // authority. That account is the explicit external authority when present,
    // otherwise the definition account itself (self/PDA authority).
    if let TokenDefinition::Fungible { authority, .. } = &definition {
        // `None` means the supply is permanently fixed (renounced) — minting is rejected.
        let mint_authority =
            authority.expect("Mint authority check failed: authority revoked, supply is fixed");
        let authority_ref = authority_account.as_ref().unwrap_or(&definition_account);
        assert!(
            authority_ref.is_authorized,
            "Mint authority must authorize the transaction"
        );
        assert_eq!(
            authority_ref.account_id, mint_authority,
            "Mint authority check failed: signer is not the current authority"
        );
    }

    let mut holding = if user_holding_account.account == Account::default() {
        TokenHolding::zeroized_from_definition(definition_account.account_id, &definition)
    } else {
        TokenHolding::try_from(&user_holding_account.account.data)
            .expect("Token Holding account must be valid")
    };

    assert_eq!(
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
                .expect("Balance overflow on minting");

            *total_supply = total_supply
                .checked_add(amount_to_mint)
                .expect("Total supply overflow");
        }
        (
            TokenDefinition::NonFungible { .. },
            TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. },
        ) => {
            panic!("Cannot mint additional supply for Non-Fungible Tokens");
        }
        _ => panic!("Mismatched Token Definition and Token Holding types"),
    }

    // Deliberately no `is_authorized` check on an uninitialized holding: crediting a
    // recipient who has not participated is a supported flow (see the private foreign-init
    // path, where the recipient is known only by `npk`/`vpk`). v0.2.4 expressed the claim as
    // `Claim::Authorized`, but logos-execution-zone PR #621 made foreign-init pre-states carry
    // `is_authorized = true` as a circuit artifact rather than as recipient consent, so that
    // claim never actually gated this path. v0.2.5 makes it honest — `is_authorized` now
    // matches whether a credential was supplied — and acquires ownership on any data write.
    // A guard here cannot distinguish an unauthorized public recipient from a legitimate
    // private foreign init, so squatting on unowned accounts is the runtime's to prevent.
    // Diffs must match pre-state order and count: [definition, holding]
    // for self authority, plus the read-only authority account when external.
    let mut state_diffs = Vec::with_capacity(3);
    state_diffs.push(AccountStateDiff::new(
        definition_account,
        BalanceDiff::Add(0),
        Data::from(&definition),
    ));
    state_diffs.push(AccountStateDiff::new(
        user_holding_account,
        BalanceDiff::Add(0),
        Data::from(&holding),
    ));
    if let Some(authority) = authority_account {
        state_diffs.push(AccountStateDiff::unchanged(authority));
    }
    state_diffs
}
