//! Host-side implementation of [`token_mint_authority_core::Instruction::FaucetMint`].
//!
//! Delegates to the Token Program (`user -> token-mint-authority -> token`): after the
//! per-day rate-limit check, it emits a single chained
//! `Token::MintWithAuthority` that mints [`FAUCET_MINT_AMOUNT`] to the caller,
//! authorized by the Token-Mint-Authority's mint-authority PDA seed. The Token-Mint-Authority
//! holds no key — the PDA seed is its authority.
//!
//! Wall-clock time comes from the system `CLOCK_01` account (the pinned
//! `spel-framework`'s `ProgramContext` exposes no clock), same as the stablecoin
//! program.

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountWithMetadata},
    program::{AccountPostState, ChainedCall, Claim, ProgramId},
};
use token_core::TokenDefinition;
use token_mint_authority_core::{
    verify_mint_allowance_and_get_seed, verify_mint_authority_and_get_seed, MintAllowance,
    FAUCET_MINT_AMOUNT, MINT_COOLDOWN_MS,
};

/// Grant [`FAUCET_MINT_AMOUNT`] of the faucet token to `recipient`, at most once
/// per [`MINT_COOLDOWN_MS`] per `(recipient, token definition)`.
///
/// Returns the six echoed/claimed post-states (in input-account order) and one
/// chained `Token::MintWithAuthority`.
///
/// The recipient's `user_holding` authorization is deliberately NOT checked
/// here — the Token Program enforces it downstream: an existing holding is just
/// written, while a fresh one is claimed via `Claim::Authorized`, which fails
/// unless the recipient also authorized the holding. So the faucet stays
/// permissive (mint into any existing holding) without letting anyone create a
/// holding they don't control.
///
/// # Panics
/// - `recipient` is not authorized.
/// - `token_definition` is uninitialized, not a `Fungible`, or its stored `mint_authority` is not
///   `mint_authority` / is renounced.
/// - `mint_authority` / `mint_allowance` do not match their PDA derivations.
/// - `mint_allowance` exists but is not owned by this program, or its cooldown has not elapsed
///   (`FaucetMint cooldown has not elapsed`).
/// - `clock` is not the system `CLOCK_01` account or is uninitialized.
#[allow(
    clippy::too_many_arguments,
    reason = "six account inputs + program id mirror the host-call ABI; a struct would obscure it"
)]
pub fn faucet_mint(
    recipient: AccountWithMetadata,
    mint_allowance: AccountWithMetadata,
    user_holding: AccountWithMetadata,
    token_definition: AccountWithMetadata,
    mint_authority: AccountWithMetadata,
    clock: AccountWithMetadata,
    token_mint_authority_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(
        recipient.is_authorized,
        "Recipient authorization is missing"
    );

    // The faucet token is owned by the Token Program; that owner is the target
    // of the chained mint.
    assert_ne!(
        token_definition.account,
        Account::default(),
        "Faucet token definition must be initialized"
    );
    let token_program_id = token_definition.account.program_owner;
    let definition_id = token_definition.account_id;

    // The definition must be a mintable Fungible whose stored authority is this
    // program's mint-authority PDA — otherwise the chained mint could never
    // succeed, so fail early with a clear message.
    let authority_seed =
        verify_mint_authority_and_get_seed(&mint_authority, token_mint_authority_program_id);
    match TokenDefinition::try_from(&token_definition.account.data)
        .expect("Faucet token definition must decode as a TokenDefinition")
    {
        TokenDefinition::Fungible { authority, .. } => {
            let authority =
                authority.expect("Faucet token has a renounced mint authority (fixed supply)");
            assert_eq!(
                authority, mint_authority.account_id,
                "Faucet token mint authority is not this program's mint-authority PDA"
            );
        }
        TokenDefinition::NonFungible { .. } => {
            panic!("Faucet token definition must be Fungible");
        }
    }

    let now = read_clock(&clock);

    let allowance_seed = verify_mint_allowance_and_get_seed(
        &mint_allowance,
        recipient.account_id,
        definition_id,
        token_mint_authority_program_id,
    );

    // First mint claims the allowance PDA; later mints must respect the cooldown.
    // A default (unowned) account means this recipient has never used this faucet
    // token, so there is nothing to throttle yet.
    if mint_allowance.account != Account::default() {
        assert_eq!(
            mint_allowance.account.program_owner, token_mint_authority_program_id,
            "Mint allowance account is not owned by this program"
        );
        let previous = MintAllowance::try_from(&mint_allowance.account.data)
            .expect("Mint allowance account must decode as a MintAllowance");
        // `saturating_sub` treats a backwards clock as "no time elapsed", which
        // conservatively keeps the faucet throttled rather than opening it.
        assert!(
            now.saturating_sub(previous.last_mint_ms) >= MINT_COOLDOWN_MS,
            "FaucetMint cooldown has not elapsed"
        );
    }

    let updated = MintAllowance {
        recipient_id: recipient.account_id,
        definition_id,
        last_mint_ms: now,
    };
    let mut allowance_post = mint_allowance.account.clone();
    allowance_post.data = (&updated).into();

    // Post-states mirror the input account order. `user_holding` and
    // `token_definition` are echoed unchanged here; the chained mint applies the
    // actual mutation. The allowance PDA is claimed on first use (Claim::Pda) so
    // this program owns it and its `last_mint_ms` persists, then rewritten. The
    // authority PDA is only echoed: it never holds state, and a default-state
    // account is retained by the framework's output filter without a claim — so
    // this program takes no ownership of it (the seed alone authorizes the mint).
    let post_states = vec![
        AccountPostState::new(recipient.account),
        AccountPostState::new_claimed_if_default(allowance_post, Claim::Pda(allowance_seed)),
        AccountPostState::new(user_holding.account.clone()),
        AccountPostState::new(token_definition.account.clone()),
        AccountPostState::new(mint_authority.account.clone()),
        AccountPostState::new(clock.account),
    ];

    // Delegate the mint to the Token Program under the mint-authority PDA seed.
    // MintWithAuthority account order: [definition, holding, authority].
    let mut authority_authorized = mint_authority;
    authority_authorized.is_authorized = true;
    let mint_call = ChainedCall::new(
        token_program_id,
        vec![token_definition, user_holding, authority_authorized],
        &token_core::Instruction::MintWithAuthority {
            amount_to_mint: FAUCET_MINT_AMOUNT,
        },
    )
    .with_pda_seeds(vec![authority_seed]);

    (post_states, vec![mint_call])
}

/// Read the millisecond wall-clock timestamp from the system `CLOCK_01` account.
pub(crate) fn read_clock(clock: &AccountWithMetadata) -> u64 {
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "Clock account must be the system CLOCK_01 account"
    );
    assert_ne!(
        clock.account,
        Account::default(),
        "Clock account must be initialized"
    );
    ClockAccountData::from_bytes(clock.account.data.as_ref()).timestamp
}
