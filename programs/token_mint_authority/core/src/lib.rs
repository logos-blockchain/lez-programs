//! The testnet faucet grants 10,000 tokens per claimant and token every 24 hours.
//!
//! Set the token definition's mint authority to [`compute_mint_authority_pda`]. The
//! faucet derives a separate authority for each token program because a PDA grant
//! authorizes an account throughout the receiving program's synchronous call chain.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{account::AccountId, program::PdaSeed};
use risc0_zkvm::sha::{Impl, Sha256 as _};
use serde::{Deserialize, Serialize};

/// One whole faucet token in base units (18 decimals).
pub const ONE_TOKEN: u128 = 1_000_000_000_000_000_000;

/// Fixed grant per successful [`Message::FaucetMint`], in base units.
pub const FAUCET_MINT_AMOUNT: u128 = 10_000 * ONE_TOKEN;

/// Minimum time between successful grants to one claimant for one token, in milliseconds.
pub const MINT_COOLDOWN_MS: u64 = 24 * 60 * 60 * 1_000;

const MINT_AUTHORITY_PDA_DOMAIN: &[u8] = b"TOKEN_MINT_AUTHORITY__MINT_AUTHORITY";
const MINT_ALLOWANCE_PDA_DOMAIN: &[u8] = b"TOKEN_MINT_AUTHORITY__MINT_ALLOWANCE";

/// Public messages, Borsh encoded. Send to the claimant's faucet actor.
#[cfg_attr(feature = "idl", spel_attrs::message)]
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Message {
    /// Claim a fixed grant for the holding at `to`. The claimant must authorize the call.
    /// An empty holding also requires its account's authorization. An existing holding
    /// must belong to `definition` in `token_program_id`.
    ///
    /// Include the claimant, allowance PDA and destination under the faucet program,
    /// the mint authority PDA, definition and destination under the token program,
    /// and the system clock actor. The allowance reads the clock. The destination
    /// asks the token program to mint and checks that the credit settled. A private
    /// destination must promote the token credit within the transaction.
    FaucetMint {
        token_program_id: AccountId,
        definition: AccountId,
        to: AccountId,
    },
}

/// Messages exchanged between faucet actors within a claim.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Internal {
    /// Ask the allowance actor to check and record the claim against the system clock.
    Claim,
    /// Ask the destination's faucet actor to fund the token holding on its account.
    Fund {
        token_program_id: AccountId,
        definition: AccountId,
        to: AccountId,
    },
    /// Ask the destination to confirm it finished checking the credit.
    AssertSettled,
}

/// Persisted allowance and the temporary state used by a destination during settlement.
/// When a destination also hosts an allowance, settlement preserves its last claim time.
#[cfg_attr(feature = "idl", spel_attrs::state)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum FaucetState {
    /// Timestamp of the most recent successful grant, in Unix milliseconds.
    Allowance { last_mint_ms: u64 },
    /// Waiting for the destination token actor's balance before minting.
    ReadingHolding {
        token_program_id: AccountId,
        definition: AccountId,
        /// Destination as presented by the claimant, including a private alias when used.
        to: AccountId,
        last_mint_ms: Option<u64>,
    },
    /// Waiting for the destination token actor's balance after minting.
    VerifyingCredit {
        token_program_id: AccountId,
        definition: AccountId,
        expected_balance: u128,
        last_mint_ms: Option<u64>,
    },
}

impl FaucetState {
    /// Decode existing state. An unused faucet actor has empty state.
    ///
    /// # Panics
    /// If nonempty bytes do not encode a faucet state.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() {
            return None;
        }
        Some(borsh::from_slice(bytes).expect("faucet actor state must decode"))
    }

    /// Encode the Borsh state written by the faucet actor.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("borsh serialization is infallible")
    }
}

/// Mint authority seed, SHA-256 of the token program ID followed by the authority domain.
#[must_use]
pub fn compute_mint_authority_pda_seed(token_program_id: AccountId) -> PdaSeed {
    hash_seed(&[&token_program_id.to_bytes(), MINT_AUTHORITY_PDA_DOMAIN])
}

/// Mint authority for tokens issued by this token program and funded by this faucet.
#[must_use]
pub fn compute_mint_authority_pda(faucet: AccountId, token_program_id: AccountId) -> AccountId {
    AccountId::for_public_pda(&faucet, &compute_mint_authority_pda_seed(token_program_id))
}

/// Allowance seed, SHA-256 of the token program, claimant and definition IDs in order,
/// followed by the allowance domain. Each ID contributes 32 bytes.
#[must_use]
pub fn compute_mint_allowance_pda_seed(
    token_program_id: AccountId,
    recipient: AccountId,
    definition: AccountId,
) -> PdaSeed {
    hash_seed(&[
        &token_program_id.to_bytes(),
        &recipient.to_bytes(),
        &definition.to_bytes(),
        MINT_ALLOWANCE_PDA_DOMAIN,
    ])
}

/// Allowance tracking this claimant's cooldown for one deployed token definition.
#[must_use]
pub fn compute_mint_allowance_pda(
    faucet: AccountId,
    token_program_id: AccountId,
    recipient: AccountId,
    definition: AccountId,
) -> AccountId {
    AccountId::for_public_pda(
        &faucet,
        &compute_mint_allowance_pda_seed(token_program_id, recipient, definition),
    )
}

fn hash_seed(parts: &[&[u8]]) -> PdaSeed {
    let digest = Impl::hash_bytes(&parts.concat());
    PdaSeed::new(
        *digest
            .as_bytes()
            .first_chunk::<32>()
            .expect("a sha256 digest has 32 bytes"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowance_round_trips_and_empty_state_is_unused() {
        let state = FaucetState::Allowance { last_mint_ms: 0 };
        assert_eq!(FaucetState::decode(&state.encode()), Some(state));
        assert_eq!(FaucetState::decode(&[]), None);
    }

    #[test]
    fn faucet_grant_is_ten_thousand_tokens_per_day() {
        assert_eq!(FAUCET_MINT_AMOUNT, 10_000_000_000_000_000_000_000);
        assert_eq!(MINT_COOLDOWN_MS, 86_400_000);
    }

    #[test]
    fn mint_authority_is_scoped_to_both_deployments() {
        let faucet = AccountId::new([1; 32]);
        let token = AccountId::new([2; 32]);
        let other = AccountId::new([3; 32]);
        let authority = compute_mint_authority_pda(faucet, token);
        assert_ne!(authority, compute_mint_authority_pda(other, token));
        assert_ne!(authority, compute_mint_authority_pda(faucet, other));
    }

    #[test]
    fn allowance_is_scoped_to_claimant_and_token_actor() {
        let faucet = AccountId::new([1; 32]);
        let token = AccountId::new([2; 32]);
        let claimant = AccountId::new([3; 32]);
        let definition = AccountId::new([4; 32]);
        let other = AccountId::new([5; 32]);
        let allowance = compute_mint_allowance_pda(faucet, token, claimant, definition);
        for (program, owner, asset) in [
            (other, claimant, definition),
            (token, other, definition),
            (token, claimant, other),
        ] {
            assert_ne!(
                allowance,
                compute_mint_allowance_pda(faucet, program, owner, asset)
            );
        }
        assert_ne!(allowance, compute_mint_authority_pda(faucet, token));
    }
}
