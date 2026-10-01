use lee_core::{
    account::{AccountId, AccountWithMetadata, BalanceDiff, Data},
    program::{AccountStateDiff, ChainedCall},
};
use stablecoin_core::{compute_protocol_parameters_pda, ProtocolParameters};

/// Halt the risk-increasing instructions (spec §10.17).
///
/// Sets `ProtocolParameters.is_frozen`. While frozen, `open_position`,
/// `withdraw_collateral` and `generate_debt` panic; everything that reduces risk
/// — `deposit_collateral`, `repay_debt`, `close_position` — and the permissionless
/// pokes keep working (§7). Idempotent: freezing an already-frozen protocol is a
/// successful no-op.
///
/// # Panics
/// - `freeze_authority` is not authorized, or its id does not match
///   `ProtocolParameters.freeze_authority_account_id`.
/// - `protocol_parameters` is uninitialized, wrongly owned, not at its canonical PDA, or does not
///   decode.
pub fn freeze(
    freeze_authority: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stablecoin_program_id: AccountId,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    set_frozen(
        freeze_authority,
        protocol_parameters,
        stablecoin_program_id,
        true,
    )
}

/// Resume normal operation (spec §10.18). Clears `ProtocolParameters.is_frozen`.
/// Idempotent, with the same preconditions as [`freeze`].
///
/// # Panics
/// See [`freeze`].
pub fn unfreeze(
    freeze_authority: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stablecoin_program_id: AccountId,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    set_frozen(
        freeze_authority,
        protocol_parameters,
        stablecoin_program_id,
        false,
    )
}

fn set_frozen(
    freeze_authority: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stablecoin_program_id: AccountId,
    is_frozen: bool,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    assert!(
        freeze_authority.is_authorized,
        "Freeze authority authorization is missing"
    );

    let mut parameters = ProtocolParameters::try_from(&crate::checks::decode_global(
        &protocol_parameters,
        compute_protocol_parameters_pda(stablecoin_program_id),
        stablecoin_program_id,
        "ProtocolParameters",
    ))
    .expect("ProtocolParameters must decode");
    assert_eq!(
        freeze_authority.account_id, parameters.freeze_authority_account_id,
        "Signer is not the protocol's freeze authority"
    );

    // Assigned unconditionally rather than only-on-change, so a future
    // "short-circuit when already set" edit can't turn a no-op into a panic.
    parameters.is_frozen = is_frozen;

    let state_diffs = vec![
        AccountStateDiff::unchanged(freeze_authority),
        AccountStateDiff::new(
            protocol_parameters,
            BalanceDiff::Add(0),
            Data::from(&parameters),
        ),
    ];

    (state_diffs, vec![])
}
