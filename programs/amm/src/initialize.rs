use amm_core::{
    assert_valid_protocol_fee_bps, assert_valid_swap_fee_bps, compute_config_pda, AmmConfig,
};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, BalanceDiff, Data},
    program::AccountStateDiff,
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
/// `swap_fee_bps` is the instance-wide swap fee (basis points) every pool in this namespace
/// charges; it is stored in the config and read by each swap. Fees are no longer per-pool.
///
/// # Panics
/// Panics if:
/// - `owner.is_authorized` is false (the owner did not sign).
/// - `config.account_id` does not match `compute_config_pda(amm_program_id, owner.account_id,
///   nonce)`.
/// - `config.account` is not the default (the instance is already initialized).
/// - `swap_fee_bps` is not below `FEE_BPS_DENOMINATOR` (100%).
/// - `protocol_fee_bps` exceeds `FEE_BPS_DENOMINATOR` (100% of the swap fee).
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit owner, config, namespace, program ids, and fee"
)]
pub fn initialize(
    owner: AccountWithMetadata,
    config: AccountWithMetadata,
    nonce: [u8; 32],
    token_program_id: AccountId,
    twap_oracle_program_id: AccountId,
    authority: AccountId,
    swap_fee_bps: u128,
    protocol_fee_bps: u128,
    amm_program_id: AccountId,
) -> Vec<AccountStateDiff> {
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
    assert_valid_swap_fee_bps(swap_fee_bps);
    assert_valid_protocol_fee_bps(protocol_fee_bps);

    let config_data = Data::from(&AmmConfig {
        token_program_id,
        twap_oracle_program_id,
        authority,
        swap_fee_bps,
        protocol_fee_bps,
    });

    // The owner account is echoed unchanged: it is the signer, not a state holder.
    //
    // Before LEZ v0.2.5 this program claimed a fresh owner account as a persistent,
    // data-less AMM-owned namespace marker. v0.2.5 acquires ownership only on a data
    // write (`acquire_ownership_on_data_write` fires when `post.data != pre.data`, and
    // an account left unowned must carry no data), so that marker is no longer
    // expressible at all.
    //
    // The marker had exactly one reader: this function, which required the owner be
    // either fresh (and so claimable) or already AMM-owned. No other handler reads it —
    // they all gate on `config.account.program_owner`. So dropping it drops that one
    // gate, and with it the requirement that the owner be a fresh, dedicated account:
    // an everyday wallet can open a namespace now.
    //
    // Nothing that gate provided is lost. A namespace is squat-proofed by the
    // `owner.is_authorized` assert above — only the owner can open one in their own
    // name — and the config PDA is derived from `(amm_program_id, owner.account_id,
    // nonce)`, so distinct owners cannot collide regardless of who owns what.
    //
    // The config PDA is claimed by the write itself; its address was asserted above.
    vec![
        AccountStateDiff::unchanged(owner),
        AccountStateDiff::new(config, BalanceDiff::Add(0), config_data),
    ]
}

#[cfg(test)]
mod tests {
    use crate::StateDiffExt;
    use lee_core::account::Nonce;

    use super::*;

    const AMM_PROGRAM_ID: AccountId = AccountId::new([42u8; 32]);
    const TOKEN_PROGRAM_ID: AccountId = AccountId::new([15u8; 32]);
    const TWAP_ORACLE_PROGRAM_ID: AccountId = AccountId::new([77u8; 32]);
    const NONCE: [u8; 32] = [3; 32];
    const SWAP_FEE_BPS: u128 = 30;
    const PROTOCOL_FEE_BPS: u128 = 1_000;

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

    fn run() -> Vec<AccountStateDiff> {
        initialize(
            owner_signed(),
            config_uninit(),
            NONCE,
            TOKEN_PROGRAM_ID,
            TWAP_ORACLE_PROGRAM_ID,
            authority(),
            SWAP_FEE_BPS,
            PROTOCOL_FEE_BPS,
            AMM_PROGRAM_ID,
        )
    }

    #[test]
    fn owner_is_echoed_and_config_is_claimed_by_its_write() {
        let post_states = run();
        assert_eq!(post_states.len(), 2);
        // 0: the owner is echoed untouched. v0.2.4 claimed a fresh owner into the AMM as a
        //    namespace marker; v0.2.5 acquires ownership only on a data write and forbids an
        //    unowned account from carrying data, so that marker is gone. Squat-proofing is
        //    still `owner.is_authorized`, asserted by the handler.
        assert!(!post_states[0].writes_data());
        assert_eq!(
            post_states[0].post_owner(AMM_PROGRAM_ID),
            post_states[0].pre_state.account.program_owner
        );
        // 1: the config write is itself the claim on its PDA.
        assert!(post_states[1].writes_data());
        assert_eq!(post_states[1].post_owner(AMM_PROGRAM_ID), AMM_PROGRAM_ID);
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
            SWAP_FEE_BPS,
            PROTOCOL_FEE_BPS,
            AMM_PROGRAM_ID,
        );
        assert_eq!(
            post_states[0].post_owner(AMM_PROGRAM_ID),
            post_states[0].pre_state.account.program_owner
        );
        assert_eq!(post_states[0].post_owner(AMM_PROGRAM_ID), AMM_PROGRAM_ID);
    }

    #[test]
    fn stores_program_ids_and_authority() {
        let post_states = run();
        let config = AmmConfig::try_from(post_states[1].post_data())
            .expect("post state must contain a valid AmmConfig");
        assert_eq!(config.token_program_id, TOKEN_PROGRAM_ID);
        assert_eq!(config.twap_oracle_program_id, TWAP_ORACLE_PROGRAM_ID);
        assert_eq!(config.authority, authority());
        // The instance-wide swap + protocol fees are stored in the config (no longer per-pool).
        assert_eq!(config.swap_fee_bps, SWAP_FEE_BPS);
        assert_eq!(config.protocol_fee_bps, PROTOCOL_FEE_BPS);
    }

    /// A swap fee at or above 100% would leave a trade with zero effective input, so it is
    /// rejected — the only validation on the otherwise free-form per-namespace fee.
    #[test]
    #[should_panic(expected = "Swap fee must be below")]
    fn swap_fee_at_or_above_100_percent_panics() {
        initialize(
            owner_signed(),
            config_uninit(),
            NONCE,
            TOKEN_PROGRAM_ID,
            TWAP_ORACLE_PROGRAM_ID,
            authority(),
            amm_core::FEE_BPS_DENOMINATOR,
            PROTOCOL_FEE_BPS,
            AMM_PROGRAM_ID,
        );
    }

    /// The protocol fee is a fraction OF the swap fee, so 100% (all of it) is valid but more is
    /// not.
    #[test]
    #[should_panic(expected = "Protocol fee must be at most")]
    fn protocol_fee_above_100_percent_of_swap_fee_panics() {
        initialize(
            owner_signed(),
            config_uninit(),
            NONCE,
            TOKEN_PROGRAM_ID,
            TWAP_ORACLE_PROGRAM_ID,
            authority(),
            SWAP_FEE_BPS,
            amm_core::FEE_BPS_DENOMINATOR + 1,
            AMM_PROGRAM_ID,
        );
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
            SWAP_FEE_BPS,
            PROTOCOL_FEE_BPS,
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
            SWAP_FEE_BPS,
            PROTOCOL_FEE_BPS,
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
            swap_fee_bps: SWAP_FEE_BPS,
            protocol_fee_bps: 0,
        });
        initialized.account.nonce = Nonce(0);
        initialize(
            owner_signed(),
            initialized,
            NONCE,
            TOKEN_PROGRAM_ID,
            TWAP_ORACLE_PROGRAM_ID,
            authority(),
            SWAP_FEE_BPS,
            PROTOCOL_FEE_BPS,
            AMM_PROGRAM_ID,
        );
    }
}
