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
    account::{Account, AccountId, AccountWithMetadata, BalanceDiff},
    program::{AccountStateDiff, ChainedCall},
};
use token_core::TokenDefinition;
use token_mint_authority_core::{
    verify_mint_allowance_and_get_seed, verify_mint_authority_and_get_seed, MintAllowance,
    FAUCET_MINT_AMOUNT, MINT_COOLDOWN_MS,
};

/// Grant [`FAUCET_MINT_AMOUNT`] of the faucet token to `recipient`, at most once
/// per [`MINT_COOLDOWN_MS`] per `(recipient, token definition)`.
///
/// Returns the six state diffs (in input-account order) and one chained
/// `Token::MintWithAuthority`.
///
/// Topping up an *existing* holding is deliberately permissive — anyone may fund
/// anyone's holding. Creating a *fresh* one requires `user_holding.is_authorized`.
///
/// That split used to be enforced downstream rather than here: v0.2.4's token program
/// claimed a fresh holding via `Claim::Authorized`, which failed unless the recipient
/// had authorized it. v0.2.5 removed claims —
/// `acquire_ownership_on_data_write` fires on any data write to a default-owned
/// account with no authorization check — so the chained `Token::MintWithAuthority`
/// would otherwise materialize a holding at *any* unowned address the caller names,
/// making it token-program-owned permanently and denying it to every other program.
/// The `else` branch below restores the v0.2.4 rule locally.
/// See `docs/lez-v0.2.5-changes.md` §4.
///
/// # Panics
/// - `recipient` is not authorized.
/// - `user_holding` is uninitialized and not authorized.
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
    token_mint_authority_program_id: AccountId,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
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
    if user_holding.account != Account::default() {
        assert_eq!(
            user_holding.account.program_owner, token_program_id,
            "User holding must be owned by the same Token Program as the token definition"
        );
    } else {
        // Creating a holding requires consent from the address that will receive it.
        // Topping up an existing holding stays open to anyone (see the doc comment).
        assert!(
            user_holding.is_authorized,
            "A fresh user holding must be authorized by its owner"
        );
    }
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

    // Called for its address assertion; the seed itself is no longer needed —
    // ownership of the allowance PDA now comes from writing its data.
    let _allowance_seed = verify_mint_allowance_and_get_seed(
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
    // The chained call names its accounts by id, so capture them before the
    // pre-states are moved into the state diffs below.
    let user_holding_id = user_holding.account_id;
    let mint_authority_id = mint_authority.account_id;

    // Diffs mirror the input account order, and every declared account must
    // appear: v0.2.5 fails a transaction whose output omits one. `user_holding`
    // and `token_definition` are echoed unchanged here; the chained mint applies
    // the actual mutation. Writing data to the allowance PDA *is* the ownership
    // claim on first use — `Claim::Pda` is gone, ownership is acquired implicitly
    // by the write — after which `last_mint_ms` persists and is rewritten. The
    // authority PDA is only echoed: it never holds state, and taking no ownership
    // of it is deliberate (the seed alone authorizes the mint).
    let state_diffs = vec![
        AccountStateDiff::unchanged(recipient),
        AccountStateDiff::new(mint_allowance, BalanceDiff::Add(0), (&updated).into()),
        AccountStateDiff::unchanged(user_holding),
        AccountStateDiff::unchanged(token_definition.clone()),
        AccountStateDiff::unchanged(mint_authority),
        AccountStateDiff::unchanged(clock),
    ];

    // Delegate the mint to the Token Program under the mint-authority PDA seed.
    // MintWithAuthority account order: [definition, holding, authority]. The
    // authority is carried by `pda_seeds`, not by an `is_authorized` pre-state
    // flag — the call ships ids, not accounts.
    let mint_call = ChainedCall::new(
        token_program_id,
        vec![definition_id, user_holding_id, mint_authority_id],
        &token_core::Instruction::MintWithAuthority {
            amount_to_mint: FAUCET_MINT_AMOUNT,
        },
    )
    .with_pda_seeds(vec![authority_seed]);

    (state_diffs, vec![mint_call])
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
