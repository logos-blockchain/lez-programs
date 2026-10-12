use amm_core::{AmmConfig, RotateAuthority};
use lee_core::{account::ShardData, program::AccountMeta};

/// Plan half of `UpdateConfig`: check that the caller signed.
///
/// Whether the signer is the *right* account is not knowable here — the stored
/// authority lives in the config shard and nowhere else — so the plan reports
/// which account claims the role and [`rotate_authority`] does the comparison.
/// The config's own ownership gate is gone: an effect on `shards[self]` asserts
/// it structurally.
///
/// # Panics
/// If `authority.is_authorized` is false.
pub fn plan_update_config(authority: &AccountMeta) {
    assert!(
        authority.is_authorized,
        "Update config: admin authority must authorize the update"
    );
}

/// Apply half of `UpdateConfig`: transfer admin control.
///
/// The Token Program and TWAP oracle program ids are immutable deployment
/// parameters, set once at `Initialize` and baked into every derived PDA, so this
/// moves the admin authority and nothing else.
///
/// # Panics
/// If the config shard is not a valid [`AmmConfig`], or its stored authority is
/// not the account the plan saw sign.
#[must_use]
pub fn rotate_authority(pre_data: &ShardData, effect: &RotateAuthority) -> ShardData {
    let mut config = AmmConfig::try_from(pre_data)
        .expect("Update config: AMM Program must be initialized before use");
    assert_eq!(
        config.authority, effect.expected_authority,
        "Update config: caller is not the configured admin authority"
    );
    config.authority = effect.new_authority;
    ShardData::from(&config)
}
