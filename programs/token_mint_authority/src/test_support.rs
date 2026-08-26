//! Shared account builders for the `faucet_mint` host-function unit tests.

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::ProgramId,
};
use token_core::{TokenDefinition, TokenHolding};
use token_mint_authority_core::{
    compute_mint_allowance_pda, compute_mint_authority_pda, MintAllowance,
};

pub(crate) const TOKEN_MINT_AUTHORITY_PROGRAM_ID: ProgramId = [11u32; 8];
pub(crate) const TOKEN_PROGRAM_ID: ProgramId = [2u32; 8];
pub(crate) const CLOCK_PROGRAM_ID: ProgramId = [5u32; 8];

/// The clock timestamp used as "now" in every test (Unix milliseconds).
pub(crate) const NOW: u64 = 1_700_000_000_000;

pub(crate) fn recipient_id() -> AccountId {
    AccountId::new([0xCA; 32])
}
pub(crate) fn user_holding_id() -> AccountId {
    AccountId::new([0xCB; 32])
}
pub(crate) fn definition_id() -> AccountId {
    AccountId::new([0x40; 32])
}
pub(crate) fn mint_authority_id() -> AccountId {
    compute_mint_authority_pda(TOKEN_MINT_AUTHORITY_PROGRAM_ID)
}
pub(crate) fn mint_allowance_id() -> AccountId {
    compute_mint_allowance_pda(
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
        recipient_id(),
        definition_id(),
    )
}

/// The funded caller: authorized, no state of its own.
pub(crate) fn recipient_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: recipient_id(),
    }
}

/// An existing, authorized holding for the faucet token.
pub(crate) fn user_holding_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: definition_id(),
                balance: 0,
            }),
            nonce: Nonce(0),
        },
        is_authorized: true,
        account_id: user_holding_id(),
    }
}

/// A not-yet-created holding — the Token Program materializes it during the
/// chained mint. Still authorized by the recipient's signature.
pub(crate) fn fresh_user_holding_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: user_holding_id(),
    }
}

/// The faucet token definition, minted by this program's authority PDA.
pub(crate) fn faucet_definition_account() -> AccountWithMetadata {
    faucet_definition_with_authority(Some(mint_authority_id()))
}

pub(crate) fn faucet_definition_with_authority(
    authority: Option<AccountId>,
) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Faucet Token"),
                total_supply: 0,
                metadata_id: None,
                authority,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: definition_id(),
    }
}

/// The mint-authority PDA as passed in: a bare, unclaimed account at the derived
/// address (the runtime authorizes it via the chained call's seed).
pub(crate) fn mint_authority_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: mint_authority_id(),
    }
}

/// A never-used allowance: default/unclaimed at the derived address.
pub(crate) fn uninitialized_allowance() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: mint_allowance_id(),
    }
}

/// An existing allowance owned by this program, last minted at `last_mint_ms`.
pub(crate) fn allowance_account(last_mint_ms: u64) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_MINT_AUTHORITY_PROGRAM_ID,
            balance: 0,
            data: Data::from(&MintAllowance {
                recipient_id: recipient_id(),
                definition_id: definition_id(),
                last_mint_ms,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: mint_allowance_id(),
    }
}

pub(crate) fn clock_account(timestamp: u64) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: CLOCK_PROGRAM_ID,
            balance: 0,
            data: Data::try_from(
                ClockAccountData {
                    block_id: 0,
                    timestamp,
                }
                .to_bytes(),
            )
            .expect("clock data fits"),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: CLOCK_01_PROGRAM_ACCOUNT_ID,
    }
}
