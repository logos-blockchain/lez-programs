use amm_core::{compute_config_pda, compute_config_pda_seed, AmmConfig};
use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, Claim, ProgramId},
};

/// Initializes the AMM Program by creating its singleton configuration account.
///
/// The config account is a PDA derived from the constant `"CONFIG"` seed
/// (`compute_config_pda(amm_program_id)`) and stores the program IDs the AMM issues chained calls
/// to — `token_program_id` (the Token Program) and `twap_oracle_program_id` (the TWAP oracle) —
/// plus `authority` (the admin allowed to change configuration later via `update_config`). Its
/// existence is the Program's "initialized" flag: the chained-call instructions read these
/// program IDs from it and reject calls until it exists.
///
/// # Panics
/// Panics if:
/// - `config.account_id` does not match `compute_config_pda(amm_program_id)`.
/// - `config.account` is not the default (the Program is already initialized).
pub fn initialize(
    config: AccountWithMetadata,
    token_program_id: ProgramId,
    twap_oracle_program_id: ProgramId,
    authority: AccountId,
    amm_program_id: ProgramId,
) -> Vec<AccountPostState> {
    assert_eq!(
        config.account_id,
        compute_config_pda(amm_program_id),
        "Initialize: AMM config Account ID does not match PDA"
    );
    assert_eq!(
        config.account,
        Account::default(),
        "Initialize: AMM config account must be uninitialized"
    );

    let mut config_post = config.account.clone();
    config_post.data = Data::from(&AmmConfig {
        token_program_id,
        twap_oracle_program_id,
        authority,
    });

    vec![AccountPostState::new_claimed(
        config_post,
        Claim::Pda(compute_config_pda_seed()),
    )]
}

#[cfg(test)]
mod tests {
    use amm_core::compute_config_pda;
    use nssa_core::account::{AccountId, Nonce};

    use super::*;

    const AMM_PROGRAM_ID: ProgramId = [42; 8];
    const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
    const TWAP_ORACLE_PROGRAM_ID: ProgramId = [77; 8];

    fn authority() -> AccountId {
        AccountId::new([9; 32])
    }

    fn config_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_config_pda(AMM_PROGRAM_ID),
        }
    }

    #[test]
    fn returns_single_pda_claimed_post_state() {
        let post_states = initialize(
            config_uninit(),
            TOKEN_PROGRAM_ID,
            TWAP_ORACLE_PROGRAM_ID,
            authority(),
            AMM_PROGRAM_ID,
        );
        assert_eq!(post_states.len(), 1);
        assert_eq!(
            post_states[0].required_claim(),
            Some(Claim::Pda(compute_config_pda_seed()))
        );
    }

    #[test]
    fn stores_program_ids_and_authority() {
        let post_states = initialize(
            config_uninit(),
            TOKEN_PROGRAM_ID,
            TWAP_ORACLE_PROGRAM_ID,
            authority(),
            AMM_PROGRAM_ID,
        );
        let config = AmmConfig::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid AmmConfig");
        assert_eq!(config.token_program_id, TOKEN_PROGRAM_ID);
        assert_eq!(config.twap_oracle_program_id, TWAP_ORACLE_PROGRAM_ID);
        assert_eq!(config.authority, authority());
    }

    #[test]
    #[should_panic(expected = "AMM config Account ID does not match PDA")]
    fn wrong_config_account_id_panics() {
        let mut wrong = config_uninit();
        wrong.account_id = AccountId::new([0; 32]);
        initialize(
            wrong,
            TOKEN_PROGRAM_ID,
            TWAP_ORACLE_PROGRAM_ID,
            authority(),
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "AMM config account must be uninitialized")]
    fn already_initialized_config_panics() {
        let mut initialized = config_uninit();
        initialized.account.data = Data::from(&AmmConfig {
            token_program_id: TOKEN_PROGRAM_ID,
            twap_oracle_program_id: TWAP_ORACLE_PROGRAM_ID,
            authority: authority(),
        });
        initialized.account.nonce = Nonce(0);
        initialize(
            initialized,
            TOKEN_PROGRAM_ID,
            TWAP_ORACLE_PROGRAM_ID,
            authority(),
            AMM_PROGRAM_ID,
        );
    }
}
