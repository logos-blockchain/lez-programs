use amm_core::{compute_config_pda, AmmConfig};
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
        config.account_id,
        compute_config_pda(amm_program_id),
        "Update config: AMM config Account ID does not match PDA"
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
    use lee_core::account::{Account, Nonce};

    use super::*;

    const AMM_PROGRAM_ID: ProgramId = [42; 8];
    const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
    const TWAP_ORACLE_PROGRAM_ID: ProgramId = [77; 8];

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
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: compute_config_pda(AMM_PROGRAM_ID),
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
    #[should_panic(expected = "AMM config Account ID does not match PDA")]
    fn wrong_config_pda_panics() {
        let mut config = config_init();
        config.account_id = AccountId::new([0; 32]);
        update_config(config, admin_authorized(), new_admin_id(), AMM_PROGRAM_ID);
    }

    #[test]
    #[should_panic(expected = "AMM Program must be initialized before use")]
    fn uninitialized_config_panics() {
        let config = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_config_pda(AMM_PROGRAM_ID),
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
