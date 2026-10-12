use lee_core::{account::ShardData, program::AccountMeta};
use token_core::{TokenDefinition, TokenDescriptor, TokenHolding, TokenKind};

/// Plan half: the holder must authorize the burn.
///
/// # Panics
/// If the holding account is not authorized.
pub fn authorize(user_holding_account: &AccountMeta) {
    assert!(
        user_holding_account.is_authorized,
        "Authorization is missing"
    );
}

/// Definition-side effect: decrement the supply the burnt `kind` draws on.
///
/// # Panics
/// If the definition does not describe holdings of `kind`, or the supply would
/// underflow.
#[must_use]
pub fn burn_supply(pre_data: &ShardData, kind: TokenKind, amount_to_burn: u128) -> ShardData {
    let mut definition =
        TokenDefinition::try_from(pre_data).expect("Token Definition account must be valid");

    match (&mut definition, kind) {
        (TokenDefinition::Fungible { total_supply, .. }, TokenKind::Fungible) => {
            *total_supply = total_supply
                .checked_sub(amount_to_burn)
                .expect("Total supply underflow");
        }
        (
            TokenDefinition::NonFungible {
                printable_supply, ..
            },
            TokenKind::NftMaster,
        ) => {
            *printable_supply = printable_supply
                .checked_sub(amount_to_burn)
                .expect("Printable supply underflow");
        }
        (
            TokenDefinition::NonFungible {
                printable_supply, ..
            },
            TokenKind::NftPrintedCopy,
        ) => {
            assert_eq!(
                amount_to_burn, 1,
                "Invalid balance to burn for NFT Printed Copy"
            );
            *printable_supply = printable_supply
                .checked_sub(1)
                .expect("Printable supply underflow");
        }
        _ => panic!("Mismatched Token Definition and Token Holding types"),
    }

    ShardData::from(&definition)
}

/// Holding-side effect: debit the burnt amount from this shard.
///
/// # Panics
/// If the holding is not the asset `descriptor` names, or the balance is
/// insufficient.
#[must_use]
pub fn burn_holding(
    pre_data: &ShardData,
    descriptor: &TokenDescriptor,
    amount_to_burn: u128,
) -> ShardData {
    let mut holding =
        TokenHolding::try_from(pre_data).expect("Token Holding account must be valid");
    holding.assert_matches(descriptor);

    match &mut holding {
        TokenHolding::Fungible { balance, .. } => {
            *balance = balance
                .checked_sub(amount_to_burn)
                .expect("Insufficient balance to burn");
        }
        TokenHolding::NftMaster { print_balance, .. } => {
            *print_balance = print_balance
                .checked_sub(amount_to_burn)
                .expect("Insufficient balance to burn");
        }
        TokenHolding::NftPrintedCopy { owned, .. } => {
            assert_eq!(
                amount_to_burn, 1,
                "Invalid balance to burn for NFT Printed Copy"
            );
            assert!(*owned, "Cannot burn unowned NFT Printed Copy");
            *owned = false;
        }
    }

    ShardData::from(&holding)
}
