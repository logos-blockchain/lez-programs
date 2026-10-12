use lee_core::{account::ShardData, program::AccountMeta};
use token_core::{TokenDescriptor, TokenHolding};

/// Plan half: the only thing a transfer can check before any shard is read.
///
/// Deliberately no `is_authorized` check on the recipient: crediting a recipient
/// who has not participated is a supported flow (see the private foreign-init
/// path, where the recipient is known only by `npk`/`vpk`). Under v0.3.0 the
/// question is moot anyway — writing `shards[token]` at an address denies that
/// address to nobody, so there is nothing to squat on.
///
/// # Panics
/// If the sender is not authorized.
pub fn authorize(sender: &AccountMeta) {
    assert!(sender.is_authorized, "Sender authorization is missing");
}

/// Apply half for the sender: debit this shard.
#[must_use]
pub fn withdraw(pre_data: &ShardData, descriptor: &TokenDescriptor, amount: u128) -> ShardData {
    let mut holding = TokenHolding::try_from(pre_data).expect("Invalid sender data");
    holding.assert_matches(descriptor);

    match &mut holding {
        TokenHolding::Fungible { balance, .. } => {
            *balance = balance.checked_sub(amount).expect("Insufficient balance");
        }
        TokenHolding::NftMaster { print_balance, .. } => {
            assert_eq!(
                *print_balance, amount,
                "Invalid balance for NFT Master transfer"
            );
            *print_balance = 0;
        }
        TokenHolding::NftPrintedCopy { owned, .. } => {
            assert_eq!(amount, 1, "Invalid balance for NFT Printed Copy transfer");
            assert!(*owned, "Sender does not own the NFT Printed Copy");
            *owned = false;
        }
    }

    ShardData::from(&holding)
}

/// Apply half for the recipient: credit this shard, initializing it if empty.
#[must_use]
pub fn deposit(pre_data: &ShardData, descriptor: &TokenDescriptor, amount: u128) -> ShardData {
    let mut holding = if pre_data.is_empty() {
        descriptor.zeroized()
    } else {
        TokenHolding::try_from(pre_data).expect("Invalid recipient data")
    };
    holding.assert_matches(descriptor);

    match &mut holding {
        TokenHolding::Fungible { balance, .. } => {
            *balance = balance
                .checked_add(amount)
                .expect("Recipient balance overflow");
        }
        TokenHolding::NftMaster { print_balance, .. } => {
            assert_eq!(
                *print_balance, 0,
                "Invalid balance in recipient account for NFT transfer"
            );
            *print_balance = amount;
        }
        TokenHolding::NftPrintedCopy { owned, .. } => {
            assert_eq!(amount, 1, "Invalid balance for NFT Printed Copy transfer");
            assert!(!*owned, "Recipient already owns the NFT Printed Copy");
            *owned = true;
        }
    }

    ShardData::from(&holding)
}
