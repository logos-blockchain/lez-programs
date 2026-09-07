use amm_core::{compute_config_pda, compute_config_pda_seed, AmmConfig};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, Claim, ProgramId},
};

/// Initializes a namespaced AMM instance by creating its configuration account.
///
/// A single deployed AMM Program hosts many independent instances, each identified by a namespace
/// `(owner, nonce)`. `owner` is the account that signs this instruction; signing squat-proofs the
/// namespace, so nobody can create an instance (and set its program IDs) under an account they do
/// not control. `nonce` discriminates multiple instances under the same owner — the all-zero nonce
/// is the owner's default instance.
///
/// The config account is a PDA derived as `compute_config_pda(amm_program_id, owner.account_id,
/// nonce)` and stores the program IDs the AMM issues chained calls to — `token_program_id` and
/// `twap_oracle_program_id` — plus `authority` (the admin allowed to change configuration later via
/// `update_config`). Its existence is the instance's "initialized" flag, and its account id is the
/// namespace root every pool and downstream PDA derives from.
///
/// # Panics
/// Panics if:
/// - `owner.is_authorized` is false (the owner did not sign).
/// - `config.account_id` does not match `compute_config_pda(amm_program_id, owner.account_id,
///   nonce)`.
/// - `config.account` is not the default (the instance is already initialized).
pub fn initialize(
    owner: AccountWithMetadata,
    config: AccountWithMetadata,
    nonce: [u8; 32],
    token_program_id: ProgramId,
    twap_oracle_program_id: ProgramId,
    authority: AccountId,
    amm_program_id: ProgramId,
) -> Vec<AccountPostState> {
    assert!(
        owner.is_authorized,
        "Initialize: owner account must sign to claim its namespace"
    );

    assert_eq!(
        config.account_id,
        compute_config_pda(amm_program_id, owner.account_id, nonce),
        "Initialize: AMM config Account ID does not match namespaced PDA"
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

    // On first use the owner is a fresh EOA; the program claims it (the owner authorizes this by
    // signing), binding the account to the AMM as a persistent namespace-owner marker. On every
    // later `initialize` under the same owner (a different `nonce`) the owner is already AMM-owned,
    // so it is echoed unchanged. This is what lets one owner open multiple instances.
    //
    // Consequence: the owner account becomes AMM-owned, so it must be a fresh, dedicated account
    // (a pre-used wallet cannot be claimed) rather than an everyday wallet.
    let owner_post = if owner.account == Account::default() {
        AccountPostState::new_claimed(owner.account.clone(), Claim::Authorized)
    } else {
        assert_eq!(
            owner.account.program_owner, amm_program_id,
            "Initialize: owner must be a fresh account or an existing AMM namespace owner"
        );
        AccountPostState::new(owner.account.clone())
    };

    vec![
        owner_post,
        AccountPostState::new_claimed(
            config_post,
            Claim::Pda(compute_config_pda_seed(owner.account_id, nonce)),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use lee_core::account::Nonce;

    use super::*;

    const AMM_PROGRAM_ID: ProgramId = [42; 8];
    const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
    const TWAP_ORACLE_PROGRAM_ID: ProgramId = [77; 8];
    const NONCE: [u8; 32] = [3; 32];

    fn authority() -> AccountId {
        AccountId::new([9; 32])
    }

    fn owner_id() -> AccountId {
        AccountId::new([5; 32])
    }

    fn owner_signed() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: owner_id(),
        }
    }

    fn config_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_config_pda(AMM_PROGRAM_ID, owner_id(), NONCE),
        }
    }

    fn run() -> Vec<AccountPostState> {
        initialize(
            owner_signed(),
            config_uninit(),
            NONCE,
            TOKEN_PROGRAM_ID,
            TWAP_ORACLE_PROGRAM_ID,
            authority(),
            AMM_PROGRAM_ID,
        )
    }

    #[test]
    fn fresh_owner_is_claimed_and_config_pda_claimed() {
        let post_states = run();
        assert_eq!(post_states.len(), 2);
        // 0: fresh owner claimed into the AMM (Authorized); 1: config claimed via its PDA seed.
        assert_eq!(post_states[0].required_claim(), Some(Claim::Authorized));
        assert_eq!(
            post_states[1].required_claim(),
            Some(Claim::Pda(compute_config_pda_seed(owner_id(), NONCE)))
        );
    }

    /// A second instance under the same owner: the owner is already AMM-owned, so it is echoed
    /// unchanged (no claim) — this is what makes multi-instance-per-owner work.
    #[test]
    fn already_owned_owner_is_echoed_for_next_instance() {
        let mut amm_owned = owner_signed();
        amm_owned.account.program_owner = AMM_PROGRAM_ID;
        amm_owned.account.nonce = Nonce(1);
        let post_states = initialize(
            amm_owned,
            AccountWithMetadata {
                account: Account::default(),
                is_authorized: false,
                account_id: compute_config_pda(AMM_PROGRAM_ID, owner_id(), [1; 32]),
            },
            [1; 32],
            TOKEN_PROGRAM_ID,
            TWAP_ORACLE_PROGRAM_ID,
            authority(),
            AMM_PROGRAM_ID,
        );
        assert_eq!(post_states[0].required_claim(), None);
        assert_eq!(post_states[0].account().program_owner, AMM_PROGRAM_ID);
    }

    #[test]
    fn stores_program_ids_and_authority() {
        let post_states = run();
        let config = AmmConfig::try_from(&post_states[1].account().data)
            .expect("post state must contain a valid AmmConfig");
        assert_eq!(config.token_program_id, TOKEN_PROGRAM_ID);
        assert_eq!(config.twap_oracle_program_id, TWAP_ORACLE_PROGRAM_ID);
        assert_eq!(config.authority, authority());
    }

    /// A different nonce is a different instance: same owner, distinct config PDA.
    #[test]
    fn distinct_nonce_yields_distinct_config() {
        assert_ne!(
            compute_config_pda(AMM_PROGRAM_ID, owner_id(), [0; 32]),
            compute_config_pda(AMM_PROGRAM_ID, owner_id(), [1; 32]),
        );
    }

    #[test]
    #[should_panic(expected = "owner account must sign")]
    fn unauthorized_owner_panics() {
        let mut unsigned = owner_signed();
        unsigned.is_authorized = false;
        initialize(
            unsigned,
            config_uninit(),
            NONCE,
            TOKEN_PROGRAM_ID,
            TWAP_ORACLE_PROGRAM_ID,
            authority(),
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "does not match namespaced PDA")]
    fn wrong_config_account_id_panics() {
        let mut wrong = config_uninit();
        wrong.account_id = AccountId::new([0; 32]);
        initialize(
            owner_signed(),
            wrong,
            NONCE,
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
            owner_signed(),
            initialized,
            NONCE,
            TOKEN_PROGRAM_ID,
            TWAP_ORACLE_PROGRAM_ID,
            authority(),
            AMM_PROGRAM_ID,
        );
    }
}
