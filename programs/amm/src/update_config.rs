use amm_core::AmmConfig;
use lee_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ProgramId},
};

/// Transfers the AMM Program's admin authority to a new account.
///
/// Only the config's current admin `authority` may call this: the `authority` account must equal
/// the stored authority and be passed authorized (signed). The new admin is `new_authority`.
///
/// The Token Program and TWAP oracle program IDs are immutable deployment parameters (set once at
/// `initialize`) — baked into every derived PDA and the AMM's chained calls — so this instruction
/// cannot change them; it only moves the admin authority. The config account is already owned by
/// this Program (created at `initialize`), so its data is updated in place — no claim is required.
///
/// # Panics
/// Panics if:
/// - `config.account_id` does not match `compute_config_pda(amm_program_id)`, or the config is
///   uninitialized (the Program has not been initialized).
/// - `authority.account_id` is not the config's current admin authority.
/// - `authority.is_authorized` is false (the admin did not sign).
pub fn update_config(
    config: AccountWithMetadata,
    authority: AccountWithMetadata,
    new_authority: AccountId,
    amm_program_id: ProgramId,
) -> Vec<AccountPostState> {
    assert_eq!(
        config.account.program_owner, amm_program_id,
        "Update config: AMM config account must be owned by the AMM Program"
    );
    let mut config_data = AmmConfig::try_from(&config.account.data)
        .expect("Update config: AMM Program must be initialized before use");

    // Access control: the caller must be the configured admin and must have signed.
    assert_eq!(
        authority.account_id, config_data.authority,
        "Update config: caller is not the configured admin authority"
    );
    assert!(
        authority.is_authorized,
        "Update config: admin authority must authorize the update"
    );

    config_data.authority = new_authority;

    let mut config_post = config.account.clone();
    config_post.data = Data::from(&config_data);

    vec![
        AccountPostState::new(config_post),
        AccountPostState::new(authority.account.clone()),
    ]
}

#[cfg(test)]
mod tests {
    use amm_core::compute_config_pda;
    use lee_core::account::{Account, Nonce};

    use super::*;

    const AMM_PROGRAM_ID: ProgramId = [42; 8];
    const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
    const TWAP_ORACLE_PROGRAM_ID: ProgramId = [77; 8];
    /// Canonical test namespace: the owner that signs Initialize and the default (all-zero) nonce.
    const TEST_NONCE: [u8; 32] = [0; 32];

    fn amm_owner() -> AccountId {
        AccountId::new([200; 32])
    }

    fn config_id() -> AccountId {
        compute_config_pda(AMM_PROGRAM_ID, amm_owner(), TEST_NONCE)
    }

    fn admin_id() -> AccountId {
        AccountId::new([9; 32])
    }

    fn new_admin_id() -> AccountId {
        AccountId::new([7; 32])
    }

    fn config_init() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: AMM_PROGRAM_ID,
                balance: 0,
                data: Data::from(&AmmConfig {
                    token_program_id: TOKEN_PROGRAM_ID,
                    twap_oracle_program_id: TWAP_ORACLE_PROGRAM_ID,
                    authority: admin_id(),
                    swap_fee_bps: amm_core::FEE_TIER_BPS_30,
                    protocol_fee_bps: 0,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: config_id(),
        }
    }

    fn admin_authorized() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: admin_id(),
        }
    }

    fn updated_config(post_states: &[AccountPostState]) -> AmmConfig {
        AmmConfig::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid AmmConfig")
    }

    // ── happy path ────────────────────────────────────────────────────────────

    #[test]
    fn transfers_authority() {
        let post_states = update_config(
            config_init(),
            admin_authorized(),
            new_admin_id(),
            AMM_PROGRAM_ID,
        );
        let config = updated_config(&post_states);
        assert_eq!(config.authority, new_admin_id());
        // The immutable program IDs are untouched — they cannot be changed here.
        assert_eq!(config.token_program_id, TOKEN_PROGRAM_ID);
        assert_eq!(config.twap_oracle_program_id, TWAP_ORACLE_PROGRAM_ID);
    }

    #[test]
    fn returns_config_and_echoed_authority_post_states() {
        let authority = admin_authorized();
        let post_states = update_config(
            config_init(),
            authority.clone(),
            new_admin_id(),
            AMM_PROGRAM_ID,
        );
        assert_eq!(post_states.len(), 2);
        // The config keeps its program owner (it is updated in place, not claimed).
        assert_eq!(post_states[0].account().program_owner, AMM_PROGRAM_ID);
        assert_eq!(*post_states[1].account(), authority.account);
    }

    // ── precondition violations ───────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "must be owned by the AMM Program")]
    fn config_not_owned_by_amm_panics() {
        let mut config = config_init();
        config.account.program_owner = [0; 8];
        update_config(config, admin_authorized(), new_admin_id(), AMM_PROGRAM_ID);
    }

    #[test]
    #[should_panic(expected = "AMM Program must be initialized before use")]
    fn uninitialized_config_panics() {
        // Owned by the AMM Program (passes the ownership gate) but carrying no AmmConfig data, so
        // the config parse is what fails.
        let config = AccountWithMetadata {
            account: Account {
                program_owner: AMM_PROGRAM_ID,
                ..Account::default()
            },
            is_authorized: false,
            account_id: config_id(),
        };
        update_config(config, admin_authorized(), new_admin_id(), AMM_PROGRAM_ID);
    }

    /// A caller who is not the configured admin cannot change the config, even if they sign.
    #[test]
    #[should_panic(expected = "caller is not the configured admin authority")]
    fn non_admin_authority_panics() {
        let mut not_admin = admin_authorized();
        not_admin.account_id = AccountId::new([123; 32]);
        update_config(config_init(), not_admin, new_admin_id(), AMM_PROGRAM_ID);
    }

    /// The admin account must actually sign; passing it unauthorized is rejected.
    #[test]
    #[should_panic(expected = "admin authority must authorize the update")]
    fn unauthorized_admin_panics() {
        let mut unsigned = admin_authorized();
        unsigned.is_authorized = false;
        update_config(config_init(), unsigned, new_admin_id(), AMM_PROGRAM_ID);
    }
}
