use amm_core::{
    assert_valid_protocol_fee_bps, assert_valid_swap_fee_bps, compute_config_pda, AmmConfig,
};
use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};

/// Plan half of `Initialize`: build the config record for a namespaced AMM
/// instance.
///
/// A single deployed AMM Program hosts many independent instances, each keyed by
/// a namespace `(owner, nonce)`. `owner` signs, which squat-proofs the namespace:
/// nobody can open an instance — and set its program ids — under an account they
/// do not control. `nonce` discriminates multiple instances under one owner; the
/// all-zero nonce is that owner's default.
///
/// This is the one instruction that reads nothing, so it needs no declared
/// values: every field of the record comes from the instruction, and the config
/// account's own address is checked here. Whether the shard is still empty is
/// asked in the apply half, by [`shared::create`](crate::shared::create).
///
/// Two checks from the v0.2.5 version are gone, both because the thing they
/// guarded no longer exists. The fresh-owner marker is one: v0.3.0 removed
/// address claiming outright, so an AMM-owned, data-less namespace marker is not
/// expressible, and nothing read it except the check itself. The
/// `config.program_owner == amm_program_id` gate is the other: a shard is keyed
/// by the program that wrote it, so emitting an effect on `shards[self]` *is*
/// that assertion.
///
/// # Panics
/// Panics if:
/// - `owner.is_authorized` is false (the owner did not sign).
/// - `config.account_id` is not `compute_config_pda(amm_program_id, owner.account_id, nonce)`.
/// - `swap_fee_bps` is not below `FEE_BPS_DENOMINATOR` (100%).
/// - `protocol_fee_bps` exceeds `FEE_BPS_DENOMINATOR` (100% of the swap fee).
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit owner, config, namespace, program ids, and fees"
)]
#[must_use]
pub fn plan_initialize(
    owner: &AccountMeta,
    config: &AccountMeta,
    nonce: [u8; 32],
    token_program_id: AccountId,
    twap_oracle_program_id: AccountId,
    authority: AccountId,
    swap_fee_bps: u128,
    protocol_fee_bps: u128,
    amm_program_id: AccountId,
) -> ShardData {
    assert!(
        owner.is_authorized,
        "Initialize: owner account must sign to claim its namespace"
    );
    assert_eq!(
        config.account_id,
        compute_config_pda(amm_program_id, owner.account_id, nonce),
        "Initialize: AMM config Account ID does not match namespaced PDA"
    );
    assert_valid_swap_fee_bps(swap_fee_bps);
    assert_valid_protocol_fee_bps(protocol_fee_bps);

    ShardData::from(&AmmConfig {
        token_program_id,
        twap_oracle_program_id,
        authority,
        swap_fee_bps,
        protocol_fee_bps,
    })
}
