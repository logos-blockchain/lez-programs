//! Host-side implementation of [`token_mint_authority_core::Instruction::FaucetMint`].
//!
//! Delegates to the Token Program (`user -> token-mint-authority -> token`): after
//! the per-day rate-limit check, it emits a single chained
//! `Token::MintWithAuthority` that mints [`FAUCET_MINT_AMOUNT`] to the caller,
//! authorized by the Token-Mint-Authority's mint-authority PDA seed. The
//! Token-Mint-Authority holds no key — the PDA seed is its authority.
//!
//! Under v0.3.0 the three reads this used to perform are split out:
//!
//! * The Token Program's id came from `token_definition.account.program_owner`. It is now
//!   `token_definition.program_account_id` — the account slot names whose shard it refers to, so
//!   the plan can read it directly.
//! * The definition's stored mint authority is in the Token Program's shard, so it is checked by a
//!   guard ([`check_faucet_authority`]).
//! * Wall-clock time is a declared instruction field pinned against `CLOCK_01` ([`check_clock`]);
//!   the plan phase cannot read the clock account.
//!
//! The v0.2.5 squatting workaround is **gone**: a fresh `user_holding` no longer
//! has to be authorized. That branch existed only because
//! `acquire_ownership_on_data_write` let a chained mint claim any unowned address
//! permanently. Writing `shards[token]` at an address denies that address to
//! nobody, so there is nothing left to squat on.

use clock_core::{clock_account_id, ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{AccountId, ProgramShardSelector, ShardData},
    program::{AccountMeta, ChainedCall},
};
use token_core::TokenDefinition;
use token_mint_authority_core::{
    verify_mint_allowance_and_get_seed, verify_mint_authority_and_get_seed, MintAllowance,
    FAUCET_MINT_AMOUNT, MINT_COOLDOWN_MS,
};

/// Plan half: check the addresses the faucet depends on and build the chained
/// mint, authorized by this program's mint-authority PDA seed.
///
/// # Panics
/// - `recipient` is not authorized.
/// - `clock` is not the system `CLOCK_01` account, or does not select the clock program's shard of
///   it.
/// - `mint_authority` or `mint_allowance` do not match their PDA derivations.
pub fn plan_faucet_mint(
    recipient: &AccountMeta,
    mint_allowance: &AccountMeta,
    user_holding: &AccountMeta,
    token_definition: &AccountMeta,
    mint_authority: &AccountMeta,
    clock: &AccountMeta,
    token_mint_authority_program_id: AccountId,
) -> ChainedCall {
    assert!(
        recipient.is_authorized,
        "Recipient authorization is missing"
    );
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "Clock account must be the system CLOCK_01 account"
    );
    // The address is half the identity. A shard is keyed by the program that
    // wrote it, so the timestamp lives in `shards[clock_account_id()]` at that
    // address; a selector naming the address under another program would hand
    // `check_clock` bytes that program authored.
    assert_eq!(
        clock.program_account_id,
        clock_account_id(),
        "Clock account must select the clock program's shard"
    );

    let authority_seed =
        verify_mint_authority_and_get_seed(mint_authority, token_mint_authority_program_id);

    // Called for its address assertion; the seed itself is not needed, since the
    // allowance shard is ours to write without presenting one.
    let _allowance_seed = verify_mint_allowance_and_get_seed(
        mint_allowance,
        recipient.account_id,
        token_definition.account_id,
        token_mint_authority_program_id,
    );

    // The Token Program's id used to be discovered by reading the definition's
    // owner. The account slot carries it now, so nothing has to be read to
    // address the call.
    let token_program_id = token_definition.program_account_id;

    // MintWithAuthority account order: [definition, holding, authority]. The
    // authority travels as a PDA seed, not an `is_authorized` flag — the call
    // ships shard selectors, not accounts.
    ChainedCall::new(
        token_program_id,
        vec![
            ProgramShardSelector::new(token_definition.account_id, token_program_id),
            ProgramShardSelector::new(user_holding.account_id, token_program_id),
            ProgramShardSelector::new(mint_authority.account_id, token_program_id),
        ],
        &token_core::Instruction::MintWithAuthority {
            amount_to_mint: FAUCET_MINT_AMOUNT,
        },
    )
    .with_pda_seeds(vec![authority_seed])
}

/// Guard on the `CLOCK_01` shard: the chain must be at or past the declared time.
///
/// Bounded deliberately — see [`token_mint_authority_core::Instruction::FaucetMint`].
///
/// # Panics
/// If the clock is behind the declared time.
pub fn check_clock(pre_data: &ShardData, now: u64) {
    assert!(!pre_data.is_empty(), "Clock account must be initialized");
    let timestamp = ClockAccountData::from_bytes(pre_data.as_ref()).timestamp;
    assert!(
        now <= timestamp,
        "FaucetMint declared a time ahead of the clock"
    );
}

/// Guard on the Token Program's definition shard: the faucet token must be a
/// mintable fungible whose authority is this program's PDA.
///
/// # Panics
/// If the definition is absent, non-fungible, has a renounced authority, or names
/// a different authority.
pub fn check_faucet_authority(pre_data: &ShardData, mint_authority_id: AccountId) {
    assert!(
        !pre_data.is_empty(),
        "Faucet token definition must be initialized"
    );
    match TokenDefinition::try_from(pre_data)
        .expect("Faucet token definition must decode as a TokenDefinition")
    {
        TokenDefinition::Fungible { authority, .. } => {
            let authority =
                authority.expect("Faucet token has a renounced mint authority (fixed supply)");
            assert_eq!(
                authority, mint_authority_id,
                "Faucet token mint authority is not this program's mint-authority PDA"
            );
        }
        TokenDefinition::NonFungible { .. } => {
            panic!("Faucet token definition must be Fungible");
        }
    }
}

/// Apply half on our allowance shard: enforce the cooldown, then stamp it.
///
/// An empty shard means this recipient has never used this faucet token, so there
/// is nothing to throttle yet. `saturating_sub` treats a backwards clock as "no
/// time elapsed", which keeps the faucet throttled rather than opening it.
///
/// # Panics
/// If the shard holds a previous stamp whose cooldown has not elapsed.
#[must_use]
pub fn record_mint(
    pre_data: &ShardData,
    recipient_id: AccountId,
    definition_id: AccountId,
    now: u64,
) -> ShardData {
    if !pre_data.is_empty() {
        let previous = MintAllowance::try_from(pre_data)
            .expect("Mint allowance account must decode as a MintAllowance");
        assert!(
            now.saturating_sub(previous.last_mint_ms) >= MINT_COOLDOWN_MS,
            "FaucetMint cooldown has not elapsed"
        );
    }

    ShardData::from(&MintAllowance {
        recipient_id,
        definition_id,
        last_mint_ms: now,
    })
}
