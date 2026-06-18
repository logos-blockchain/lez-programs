use amm_core::{
    compute_config_pda, compute_pool_pda, compute_pool_pda_seed, AmmConfig, PoolDefinition,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use nssa_core::{
    account::{Account, AccountWithMetadata},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_price_observations_pda, CurrentTickAccount,
};

/// Creates a TWAP price-observations account for `pool` over a time window, on behalf of the AMM.
///
/// The pool acts as the price source: the AMM authorizes it via its pool PDA seed so the oracle
/// ties the observations account to this pool. The work itself is delegated to the configured
/// TWAP oracle program through a single chained call to its `CreatePriceObservations` instruction,
/// which claims and initialises the observations PDA.
///
/// The initial tick is **not** caller-supplied: it is read from the pool's
/// [`CurrentTickAccount`] — the authoritative tick the AMM previously wrote via the oracle — so the
/// feed cannot be seeded at a forged price. This mirrors what `RecordTick` does, using the current
/// tick to seed the very first observation.
///
/// The TWAP oracle program ID is read from the AMM config account (the initialization gate). The
/// clock must be the canonical 1-block LEZ system clock, and the observations account must not yet
/// exist — both are checked here so the call is rejected early with an AMM-level error, in
/// addition to the oracle's own checks.
///
/// # Panics
/// Panics if:
/// - `config.account_id` does not match `compute_config_pda(amm_program_id)`, or the config is
///   uninitialized (the AMM Program has not been initialized).
/// - `clock.account_id` is not [`CLOCK_01_PROGRAM_ACCOUNT_ID`].
/// - `pool.account` does not hold a valid [`PoolDefinition`], or `pool.account_id` does not match
///   its pool PDA.
/// - `current_tick_account.account_id` does not match the pool's current-tick PDA, or the account
///   does not hold a valid [`CurrentTickAccount`] (it has not been created yet).
/// - `price_observations.account_id` does not match the expected TWAP PDA for `(pool,
///   window_duration)`, or `price_observations.account` already exists.
pub fn create_price_observations(
    config: AccountWithMetadata,
    pool: AccountWithMetadata,
    current_tick_account: AccountWithMetadata,
    price_observations: AccountWithMetadata,
    clock: AccountWithMetadata,
    window_duration: u64,
    amm_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    // Config gate: validate the config PDA and read the TWAP oracle program ID from it.
    assert_eq!(
        config.account_id,
        compute_config_pda(amm_program_id),
        "Create price observations: AMM config Account ID does not match PDA"
    );
    let twap_oracle_program_id = AmmConfig::try_from(&config.account.data)
        .expect("Create price observations: AMM Program must be initialized before use")
        .twap_oracle_program_id;

    // The clock must be the canonical 1-block LEZ system clock; otherwise a caller could seed the
    // feed with a forged base timestamp.
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "Create price observations: clock account must be the canonical 1-block LEZ clock account"
    );

    // The pool is the price source. Verify it is a genuine AMM pool PDA so we only ever authorize
    // a real pool as the source.
    let pool_def = PoolDefinition::try_from(&pool.account.data)
        .expect("Create price observations: AMM Program expects a valid Pool Definition Account");
    assert_eq!(
        pool.account_id,
        compute_pool_pda(
            amm_program_id,
            pool_def.definition_token_a_id,
            pool_def.definition_token_b_id,
        ),
        "Create price observations: Pool Account ID does not match PDA"
    );

    // The initial tick comes from the pool's authoritative CurrentTickAccount, not from the
    // caller. Verifying its PDA ties it to this exact pool, so the seed tick cannot be forged.
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id),
        "Create price observations: current tick Account ID does not match PDA"
    );
    let initial_tick = CurrentTickAccount::try_from(&current_tick_account.account.data)
        .expect("Create price observations: AMM Program expects a valid CurrentTickAccount")
        .tick;

    // Verify the observations account is the expected TWAP PDA for this (pool, window) pair and
    // reject if it already exists.
    assert_eq!(
        price_observations.account_id,
        compute_price_observations_pda(twap_oracle_program_id, pool.account_id, window_duration),
        "Create price observations: price observations Account ID does not match PDA"
    );
    assert_eq!(
        price_observations.account,
        Account::default(),
        "Create price observations: price observations account already exists"
    );

    // Authorize the pool as the price source so the oracle ties the feed to this pool. The AMM
    // proves control of the pool PDA via its seed.
    let mut pool_price_source = pool.clone();
    pool_price_source.is_authorized = true;

    let chained_call = ChainedCall::new(
        twap_oracle_program_id,
        vec![price_observations.clone(), pool_price_source, clock.clone()],
        &twap_oracle_core::Instruction::CreatePriceObservations {
            initial_tick,
            window_duration,
        },
    )
    .with_pda_seeds(vec![compute_pool_pda_seed(
        pool_def.definition_token_a_id,
        pool_def.definition_token_b_id,
    )]);

    let post_states = vec![
        AccountPostState::new(config.account.clone()),
        AccountPostState::new(pool.account.clone()),
        AccountPostState::new(current_tick_account.account.clone()),
        AccountPostState::new(price_observations.account.clone()),
        AccountPostState::new(clock.account.clone()),
    ];

    (post_states, vec![chained_call])
}

#[cfg(test)]
mod tests {
    use amm_core::compute_pool_pda_seed;
    use nssa_core::account::{Account, AccountId, Data, Nonce};
    use twap_oracle_core::compute_current_tick_account_pda;

    use super::*;

    const AMM_PROGRAM_ID: ProgramId = [42; 8];
    const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
    const TWAP_ORACLE_PROGRAM_ID: ProgramId = [77; 8];
    /// 24-hour window in milliseconds.
    const WINDOW_24H: u64 = 24 * 60 * 60 * 1_000;
    /// The authoritative tick stored in the pool's `CurrentTickAccount`.
    const CURRENT_TICK: i32 = -1_234;

    fn token_a_id() -> AccountId {
        AccountId::new([3; 32])
    }

    fn token_b_id() -> AccountId {
        AccountId::new([4; 32])
    }

    fn pool_id() -> AccountId {
        compute_pool_pda(AMM_PROGRAM_ID, token_a_id(), token_b_id())
    }

    fn config_init() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: AMM_PROGRAM_ID,
                balance: 0,
                data: Data::from(&AmmConfig {
                    token_program_id: TOKEN_PROGRAM_ID,
                    twap_oracle_program_id: TWAP_ORACLE_PROGRAM_ID,
                    authority: AccountId::new([9; 32]),
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: compute_config_pda(AMM_PROGRAM_ID),
        }
    }

    fn pool() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: AMM_PROGRAM_ID,
                balance: 0,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: token_a_id(),
                    definition_token_b_id: token_b_id(),
                    vault_a_id: AccountId::new([5; 32]),
                    vault_b_id: AccountId::new([6; 32]),
                    liquidity_pool_id: AccountId::new([7; 32]),
                    liquidity_pool_supply: 5_000,
                    reserve_a: 5_000,
                    reserve_b: 2_500,
                    fees: amm_core::FEE_TIER_BPS_30,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: pool_id(),
        }
    }

    fn current_tick_account() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TWAP_ORACLE_PROGRAM_ID,
                balance: 0,
                data: Data::from(&CurrentTickAccount {
                    tick: CURRENT_TICK,
                    last_updated: 1_700_000_000_000,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: compute_current_tick_account_pda(TWAP_ORACLE_PROGRAM_ID, pool_id()),
        }
    }

    fn price_observations_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_price_observations_pda(
                TWAP_ORACLE_PROGRAM_ID,
                pool_id(),
                WINDOW_24H,
            ),
        }
    }

    fn clock() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: CLOCK_01_PROGRAM_ACCOUNT_ID,
        }
    }

    fn call() -> (Vec<AccountPostState>, Vec<ChainedCall>) {
        create_price_observations(
            config_init(),
            pool(),
            current_tick_account(),
            price_observations_uninit(),
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        )
    }

    // ── happy path ────────────────────────────────────────────────────────────

    #[test]
    fn returns_five_post_states_unchanged() {
        let (post_states, _) = call();
        assert_eq!(post_states.len(), 5);
        assert_eq!(*post_states[0].account(), config_init().account);
        assert_eq!(*post_states[1].account(), pool().account);
        assert_eq!(*post_states[2].account(), current_tick_account().account);
        assert_eq!(
            *post_states[3].account(),
            price_observations_uninit().account
        );
        assert_eq!(*post_states[4].account(), clock().account);
    }

    #[test]
    fn seeds_chained_call_with_tick_from_current_tick_account() {
        let (_, chained_calls) = call();
        assert_eq!(chained_calls.len(), 1);

        // The chained call must carry the tick read from the CurrentTickAccount, not a
        // caller-supplied value, and authorize the pool as the price source.
        let mut pool_authorized = pool();
        pool_authorized.is_authorized = true;
        let expected = ChainedCall::new(
            TWAP_ORACLE_PROGRAM_ID,
            vec![price_observations_uninit(), pool_authorized, clock()],
            &twap_oracle_core::Instruction::CreatePriceObservations {
                initial_tick: CURRENT_TICK,
                window_duration: WINDOW_24H,
            },
        )
        .with_pda_seeds(vec![compute_pool_pda_seed(token_a_id(), token_b_id())]);

        assert_eq!(chained_calls[0], expected);
    }

    // ── precondition violations ───────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "AMM config Account ID does not match PDA")]
    fn wrong_config_pda_panics() {
        let mut config = config_init();
        config.account_id = AccountId::new([0; 32]);
        create_price_observations(
            config,
            pool(),
            current_tick_account(),
            price_observations_uninit(),
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "AMM Program must be initialized before use")]
    fn uninitialized_config_panics() {
        let config = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_config_pda(AMM_PROGRAM_ID),
        };
        create_price_observations(
            config,
            pool(),
            current_tick_account(),
            price_observations_uninit(),
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "clock account must be the canonical 1-block LEZ clock account")]
    fn non_canonical_clock_panics() {
        let mut clock = clock();
        clock.account_id = AccountId::new([9; 32]);
        create_price_observations(
            config_init(),
            pool(),
            current_tick_account(),
            price_observations_uninit(),
            clock,
            WINDOW_24H,
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "Pool Account ID does not match PDA")]
    fn forged_pool_account_panics() {
        let mut pool = pool();
        pool.account_id = AccountId::new([8; 32]);
        create_price_observations(
            config_init(),
            pool,
            current_tick_account(),
            price_observations_uninit(),
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        );
    }

    /// A caller cannot substitute a current-tick account they control to forge the seed tick: the
    /// account ID must match the pool's current-tick PDA.
    #[test]
    #[should_panic(expected = "current tick Account ID does not match PDA")]
    fn forged_current_tick_account_panics() {
        let mut current_tick = current_tick_account();
        current_tick.account_id = AccountId::new([2; 32]);
        create_price_observations(
            config_init(),
            pool(),
            current_tick,
            price_observations_uninit(),
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "AMM Program expects a valid CurrentTickAccount")]
    fn uninitialized_current_tick_account_panics() {
        let current_tick = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_current_tick_account_pda(TWAP_ORACLE_PROGRAM_ID, pool_id()),
        };
        create_price_observations(
            config_init(),
            pool(),
            current_tick,
            price_observations_uninit(),
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "price observations Account ID does not match PDA")]
    fn wrong_observations_pda_panics() {
        let mut observations = price_observations_uninit();
        observations.account_id = AccountId::new([1; 32]);
        create_price_observations(
            config_init(),
            pool(),
            current_tick_account(),
            observations,
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "price observations account already exists")]
    fn already_existing_observations_panics() {
        let mut observations = price_observations_uninit();
        observations.account.data = Data::try_from(vec![1u8; 8]).expect("fits in Data");
        create_price_observations(
            config_init(),
            pool(),
            current_tick_account(),
            observations,
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    fn different_windows_produce_distinct_observation_pdas() {
        let window_7d = 7 * 24 * 60 * 60 * 1_000u64;
        assert_ne!(
            compute_price_observations_pda(TWAP_ORACLE_PROGRAM_ID, pool_id(), WINDOW_24H),
            compute_price_observations_pda(TWAP_ORACLE_PROGRAM_ID, pool_id(), window_7d),
        );
    }
}
