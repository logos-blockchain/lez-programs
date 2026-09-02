//! Core data structures and utilities for the Token-Mint-Authority Program.
//!
//! The Token-Mint-Authority is a permissionless testnet faucet. It is a pure delegating
//! proxy in front of the Token Program: the call chain is
//! `user -> token-mint-authority -> token`. Its sole job is to hold the mint authority
//! for one or more faucet token definitions and to let any account mint a fixed
//! amount to itself, rate-limited to once per 24h per token.
//!
//! For a token to be mintable through here, its `TokenDefinition::Fungible`
//! `mint_authority` must be set — at token-creation time, via the Token
//! Program's `NewFungibleDefinition` — to [`compute_mint_authority_pda`] of the
//! deployed Token-Mint-Authority program. The Token-Mint-Authority then authorizes that PDA
//! implicitly (program id + seed) when it delegates the chained
//! `Token::MintWithAuthority`; it stores no key and needs no initialization.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{PdaSeed, ProgramId},
};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

/// One whole faucet token in base units (18 decimals), i.e. `1e18`.
pub const ONE_TOKEN: u128 = 1_000_000_000_000_000_000;

/// Fixed amount minted by every successful [`Instruction::FaucetMint`], in base
/// units: `10_000e18`. The mint amount is not caller-controllable — a fixed
/// grant is the whole point of a faucet and keeps supply drain bounded.
pub const FAUCET_MINT_AMOUNT: u128 = 10_000 * ONE_TOKEN;

/// Minimum wall-clock gap between two successful faucet mints for the same
/// `(recipient, token definition)` pair: 24 hours in milliseconds.
pub const MINT_COOLDOWN_MS: u64 = 24 * 60 * 60 * 1_000;

// Stable domain-separation tags for the Token-Mint-Authority PDAs; these must stay
// unchanged for address compatibility.
const MINT_AUTHORITY_PDA_DOMAIN: &[u8] = b"TOKEN_MINT_AUTHORITY__MINT_AUTHORITY";
const MINT_ALLOWANCE_PDA_DOMAIN: &[u8] = b"TOKEN_MINT_AUTHORITY__MINT_ALLOWANCE";

/// Token-Mint-Authority Program Instruction.
#[derive(Debug, Serialize, Deserialize)]
pub enum Instruction {
    /// Mint [`FAUCET_MINT_AMOUNT`] of the faucet token to the calling recipient,
    /// once per [`MINT_COOLDOWN_MS`] per `(recipient, token definition)`.
    ///
    /// Required accounts (6), in order:
    /// 1. `recipient` — authorized; the account being funded and the rate-limit subject. Also
    ///    authorizes its own `user_holding`.
    /// 2. `mint_allowance` — the per-`(recipient, definition)` rate-limit PDA at
    ///    [`compute_mint_allowance_pda`]; claimed on first use, then rewritten.
    /// 3. `user_holding` — recipient's Token Holding for the faucet token (uninitialized, or
    ///    initialized and authorized). Mutated by the chained `Token::MintWithAuthority`.
    /// 4. `token_definition` — the faucet `TokenDefinition::Fungible`, owned by the Token Program,
    ///    whose stored `mint_authority` is `mint_authority` below. Mutated by the chained mint.
    /// 5. `mint_authority` — the Token-Mint-Authority PDA at [`compute_mint_authority_pda`];
    ///    authorized in the chained mint via its seed (`user -> token-mint-authority -> token`),
    ///    so the Token-Mint-Authority never holds a key of its own.
    /// 6. `clock` — the system `CLOCK_01` account; read-only. Anchors the cooldown. (The pinned
    ///    spel-framework exposes no `ProgramContext` clock, so wall-clock time is read from this
    ///    account.)
    FaucetMint,
}

/// Per-`(recipient, token definition)` rate-limit state.
///
/// Stored at the [`compute_mint_allowance_pda`] address and owned by the
/// Token-Mint-Authority program. `recipient_id` / `definition_id` are redundant with the
/// PDA derivation but kept for `spel inspect`-ability and defense in depth.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct MintAllowance {
    /// The funded account.
    pub recipient_id: AccountId,
    /// The faucet token definition this allowance is scoped to.
    pub definition_id: AccountId,
    /// Unix milliseconds of the most recent successful faucet mint.
    pub last_mint_ms: u64,
}

impl TryFrom<&Data> for MintAllowance {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&MintAllowance> for Data {
    fn from(state: &MintAllowance) -> Self {
        let len = borsh::object_length(state).expect("MintAllowance length must be known");
        let mut buf = Vec::with_capacity(len);
        BorshSerialize::serialize(state, &mut buf)
            .expect("MintAllowance serialization should not fail");
        Self::try_from(buf).expect("MintAllowance encoded data should fit into Data")
    }
}

/// PDA seed for the Token-Mint-Authority's singleton mint-authority account. A single
/// authority per deployed program backs every faucet token: creators set their
/// definition's `mint_authority` to [`compute_mint_authority_pda`] of this
/// program, and the program's seed authorizes every chained mint.
#[must_use]
pub fn compute_mint_authority_pda_seed() -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(MINT_AUTHORITY_PDA_DOMAIN).as_bytes());
    PdaSeed::new(out)
}

/// Account id of the Token-Mint-Authority's mint-authority PDA under
/// `token_mint_authority_program_id`.
#[must_use]
pub fn compute_mint_authority_pda(token_mint_authority_program_id: ProgramId) -> AccountId {
    AccountId::for_public_pda(
        &token_mint_authority_program_id,
        &compute_mint_authority_pda_seed(),
    )
}

/// PDA seed for the [`MintAllowance`] of `(recipient_id, definition_id)`.
///
/// Keyed by both the recipient and the token so each faucet token carries its
/// own independent per-account cooldown.
#[must_use]
pub fn compute_mint_allowance_pda_seed(
    recipient_id: AccountId,
    definition_id: AccountId,
) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&recipient_id.to_bytes());
    bytes.extend_from_slice(&definition_id.to_bytes());
    bytes.extend_from_slice(MINT_ALLOWANCE_PDA_DOMAIN);

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(&bytes).as_bytes());
    PdaSeed::new(out)
}

/// Account id of the [`MintAllowance`] PDA for `(recipient_id, definition_id)`
/// under `token_mint_authority_program_id`.
#[must_use]
pub fn compute_mint_allowance_pda(
    token_mint_authority_program_id: ProgramId,
    recipient_id: AccountId,
    definition_id: AccountId,
) -> AccountId {
    AccountId::for_public_pda(
        &token_mint_authority_program_id,
        &compute_mint_allowance_pda_seed(recipient_id, definition_id),
    )
}

/// Verify the mint-authority account's address matches `token_mint_authority_program_id`
/// and return its [`PdaSeed`] for use in the chained mint.
///
/// # Panics
/// If `mint_authority.account_id` does not match the derived PDA.
pub fn verify_mint_authority_and_get_seed(
    mint_authority: &AccountWithMetadata,
    token_mint_authority_program_id: ProgramId,
) -> PdaSeed {
    let seed = compute_mint_authority_pda_seed();
    let expected_id = AccountId::for_public_pda(&token_mint_authority_program_id, &seed);
    assert_eq!(
        mint_authority.account_id, expected_id,
        "Mint authority account ID does not match expected PDA derivation"
    );
    seed
}

/// Verify the allowance account's address matches `(token_mint_authority_program_id,
/// recipient, definition)` and return its [`PdaSeed`] for the post-state claim.
///
/// # Panics
/// If `mint_allowance.account_id` does not match the derived PDA.
pub fn verify_mint_allowance_and_get_seed(
    mint_allowance: &AccountWithMetadata,
    recipient_id: AccountId,
    definition_id: AccountId,
    token_mint_authority_program_id: ProgramId,
) -> PdaSeed {
    let seed = compute_mint_allowance_pda_seed(recipient_id, definition_id);
    let expected_id = AccountId::for_public_pda(&token_mint_authority_program_id, &seed);
    assert_eq!(
        mint_allowance.account_id, expected_id,
        "Mint allowance account ID does not match expected PDA derivation"
    );
    seed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MintAllowance {
        MintAllowance {
            recipient_id: AccountId::new([3u8; 32]),
            definition_id: AccountId::new([7u8; 32]),
            last_mint_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn borsh_roundtrip_allowance() {
        let state = sample();
        let data: Data = (&state).into();
        let decoded = MintAllowance::try_from(&data).expect("decode");
        assert_eq!(decoded, state);
    }

    #[test]
    fn faucet_amount_is_ten_thousand_tokens() {
        assert_eq!(FAUCET_MINT_AMOUNT, 10_000 * ONE_TOKEN);
        assert_eq!(FAUCET_MINT_AMOUNT, 10_000_000_000_000_000_000_000);
    }

    #[test]
    fn cooldown_is_one_day() {
        assert_eq!(MINT_COOLDOWN_MS, 86_400_000);
    }

    #[test]
    fn authority_pda_is_deterministic_and_singleton() {
        let program_id: ProgramId = [9u32; 8];
        assert_eq!(
            compute_mint_authority_pda(program_id),
            compute_mint_authority_pda(program_id),
        );
    }

    #[test]
    fn allowance_pda_depends_on_recipient_and_definition() {
        let program_id: ProgramId = [9u32; 8];
        let a = AccountId::new([1u8; 32]);
        let b = AccountId::new([2u8; 32]);
        let def = AccountId::new([5u8; 32]);
        let def2 = AccountId::new([6u8; 32]);
        // Distinct recipient -> distinct allowance.
        assert_ne!(
            compute_mint_allowance_pda(program_id, a, def),
            compute_mint_allowance_pda(program_id, b, def),
        );
        // Distinct token -> distinct allowance (per-account-per-token scoping).
        assert_ne!(
            compute_mint_allowance_pda(program_id, a, def),
            compute_mint_allowance_pda(program_id, a, def2),
        );
        // Allowance PDA never collides with the authority PDA.
        assert_ne!(
            compute_mint_allowance_pda(program_id, a, def),
            compute_mint_authority_pda(program_id),
        );
    }
}
