use amm_core::{
    compute_config_pda, compute_pool_pda, compute_pool_pda_seed, spot_price_q64_64, AmmConfig,
    PoolDefinition,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use nssa_core::{
    account::{Account, AccountWithMetadata},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use twap_oracle_core::{compute_oracle_price_account_pda, OBSERVATIONS_CAPACITY};

/// Creates a TWAP oracle price account for `pool` over a time window, on behalf of the AMM.
///
/// Mirrors [`create_price_observations`](super::create_price_observations): the pool acts as the
/// price source, authorized via its pool PDA seed, and the work is delegated to the configured TWAP
/// oracle program through a single chained call to its `CreateOraclePriceAccount` instruction,
/// which claims and initialises the price-account PDA.
///
/// Neither the asset pair nor the initial price is caller-supplied: the base/quote assets are the
/// pool's token definitions and the initial price is the pool's current spot price
/// (`reserve_b / reserve_a` as a Q64.64), read from the validated pool — so the account cannot be
/// seeded at a forged price. The seed is a placeholder until `PublishPrice` writes the first TWAP.
///
/// The TWAP oracle program ID is read from the AMM config account (the initialization gate). The
/// clock must be the canonical 1-block LEZ system clock, and the price account must not yet exist —
/// both are checked here so the call is rejected early with an AMM-level error, in addition to the
/// oracle's own checks.
///
/// # Panics
/// Panics if:
/// - `config.account_id` does not match `compute_config_pda(amm_program_id)`, or the config is
///   uninitialized (the AMM Program has not been initialized).
/// - `clock.account_id` is not [`CLOCK_01_PROGRAM_ACCOUNT_ID`].
/// - `pool.account` does not hold a valid [`PoolDefinition`], or `pool.account_id` does not match
///   its pool PDA.
/// - `oracle_price_account.account_id` does not match the expected TWAP PDA for `(pool,
///   window_duration)`, or `oracle_price_account.account` already exists.
/// - `pool.account` has a zero token-A reserve (no spot price is defined).
/// - the pool's spot price is zero (`reserve_b` is zero or negligible relative to `reserve_a`);
///   zero is the no-price sentinel, so the account must never be seeded with it.
/// - `window_duration` is smaller than [`OBSERVATIONS_CAPACITY`]. Such a window can never have a
///   matching `PriceObservations` account, so the price account could never be updated by
///   `PublishPrice`. Checked here for an early AMM-level error, in addition to the oracle's own
///   check.
pub fn create_oracle_price_account(
    config: AccountWithMetadata,
    pool: AccountWithMetadata,
    oracle_price_account: AccountWithMetadata,
    clock: AccountWithMetadata,
    window_duration: u64,
    amm_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    // Config gate: validate the config PDA and read the TWAP oracle program ID from it.
    assert_eq!(
        config.account_id,
        compute_config_pda(amm_program_id),
        "Create oracle price account: AMM config Account ID does not match PDA"
    );
    let twap_oracle_program_id = AmmConfig::try_from(&config.account.data)
        .expect("Create oracle price account: AMM Program must be initialized before use")
        .twap_oracle_program_id;

    // The clock must be the canonical 1-block LEZ system clock; otherwise a caller could seed the
    // price account with a forged base timestamp.
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "Create oracle price account: clock account must be the canonical 1-block LEZ clock account"
    );

    // A window smaller than the observations capacity can never have a matching PriceObservations
    // account, so PublishPrice could never update the price account. Reject early with an AMM-level
    // error; the oracle enforces the same bound.
    assert!(
        window_duration >= u64::from(OBSERVATIONS_CAPACITY),
        "Create oracle price account: window_duration must be >= OBSERVATIONS_CAPACITY so a matching \
         PriceObservations account can exist and PublishPrice can update this price account"
    );

    // The pool is the price source. Verify it is a genuine AMM pool PDA so we only ever authorize a
    // real pool as the source, and derive the asset pair and initial price from its validated
    // state.
    let pool_def = PoolDefinition::try_from(&pool.account.data)
        .expect("Create oracle price account: AMM Program expects a valid Pool Definition Account");
    assert_eq!(
        pool.account_id,
        compute_pool_pda(
            amm_program_id,
            pool_def.definition_token_a_id,
            pool_def.definition_token_b_id,
        ),
        "Create oracle price account: Pool Account ID does not match PDA"
    );

    // Initial price is the pool's current spot price (quote per base), not caller-supplied.
    let initial_price = spot_price_q64_64(pool_def.reserve_a, pool_def.reserve_b);
    // A zero spot price is the sentinel consumers treat as "no valid price", so the account must
    // never be seeded with it. This happens when `reserve_b` is zero or so small relative to
    // `reserve_a` that the Q64.64 division floors to zero. The oracle enforces the same bound.
    assert!(
        initial_price != 0,
        "Create oracle price account: pool spot price must be non-zero (zero is the no-price \
         sentinel; pool reserve_b is zero or negligible relative to reserve_a)"
    );

    // Verify the price account is the expected TWAP PDA for this (pool, window) pair and reject if
    // it already exists.
    assert_eq!(
        oracle_price_account.account_id,
        compute_oracle_price_account_pda(twap_oracle_program_id, pool.account_id, window_duration),
        "Create oracle price account: oracle price Account ID does not match PDA"
    );
    assert_eq!(
        oracle_price_account.account,
        Account::default(),
        "Create oracle price account: oracle price account already exists"
    );

    // Authorize the pool as the price source so the oracle ties the account to this pool. The AMM
    // proves control of the pool PDA via its seed.
    let mut pool_price_source = pool.clone();
    pool_price_source.is_authorized = true;

    let chained_call = ChainedCall::new(
        twap_oracle_program_id,
        vec![
            oracle_price_account.clone(),
            pool_price_source,
            clock.clone(),
        ],
        &twap_oracle_core::Instruction::CreateOraclePriceAccount {
            base_asset: pool_def.definition_token_a_id,
            quote_asset: pool_def.definition_token_b_id,
            initial_price,
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
        AccountPostState::new(oracle_price_account.account.clone()),
        AccountPostState::new(clock.account.clone()),
    ];

    (post_states, vec![chained_call])
}

#[cfg(test)]
mod tests {
    use amm_core::compute_pool_pda_seed;
    use nssa_core::account::{Account, AccountId, Data, Nonce};

    use super::*;

    const AMM_PROGRAM_ID: ProgramId = [42; 8];
    const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
    const TWAP_ORACLE_PROGRAM_ID: ProgramId = [77; 8];
    /// 24-hour window in milliseconds.
    const WINDOW_24H: u64 = 24 * 60 * 60 * 1_000;
    const RESERVE_A: u128 = 5_000;
    const RESERVE_B: u128 = 2_500;

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

    fn pool_with_reserves(reserve_a: u128, reserve_b: u128) -> AccountWithMetadata {
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
                    reserve_a,
                    reserve_b,
                    fees: amm_core::FEE_TIER_BPS_30,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: pool_id(),
        }
    }

    fn pool() -> AccountWithMetadata {
        pool_with_reserves(RESERVE_A, RESERVE_B)
    }

    fn oracle_price_account_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_oracle_price_account_pda(
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
        create_oracle_price_account(
            config_init(),
            pool(),
            oracle_price_account_uninit(),
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        )
    }

    // ── happy path ────────────────────────────────────────────────────────────

    #[test]
    fn returns_four_post_states_unchanged() {
        let (post_states, _) = call();
        assert_eq!(post_states.len(), 4);
        assert_eq!(*post_states[0].account(), config_init().account);
        assert_eq!(*post_states[1].account(), pool().account);
        assert_eq!(
            *post_states[2].account(),
            oracle_price_account_uninit().account
        );
        assert_eq!(*post_states[3].account(), clock().account);
    }

    #[test]
    fn seeds_chained_call_with_pool_assets_and_spot_price() {
        let (_, chained_calls) = call();
        assert_eq!(chained_calls.len(), 1);

        // The chained call must carry the pool's asset pair and the spot price derived from the
        // pool's reserves (not a caller-supplied value), and authorize the pool as the price
        // source.
        let mut pool_authorized = pool();
        pool_authorized.is_authorized = true;
        let expected = ChainedCall::new(
            TWAP_ORACLE_PROGRAM_ID,
            vec![oracle_price_account_uninit(), pool_authorized, clock()],
            &twap_oracle_core::Instruction::CreateOraclePriceAccount {
                base_asset: token_a_id(),
                quote_asset: token_b_id(),
                initial_price: spot_price_q64_64(RESERVE_A, RESERVE_B),
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
        create_oracle_price_account(
            config,
            pool(),
            oracle_price_account_uninit(),
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
        create_oracle_price_account(
            config,
            pool(),
            oracle_price_account_uninit(),
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
        create_oracle_price_account(
            config_init(),
            pool(),
            oracle_price_account_uninit(),
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
        create_oracle_price_account(
            config_init(),
            pool,
            oracle_price_account_uninit(),
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "oracle price Account ID does not match PDA")]
    fn wrong_oracle_price_account_pda_panics() {
        let mut price_account = oracle_price_account_uninit();
        price_account.account_id = AccountId::new([1; 32]);
        create_oracle_price_account(
            config_init(),
            pool(),
            price_account,
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "oracle price account already exists")]
    fn already_existing_oracle_price_account_panics() {
        let mut price_account = oracle_price_account_uninit();
        price_account.account.data = Data::try_from(vec![1u8; 8]).expect("fits in Data");
        create_oracle_price_account(
            config_init(),
            pool(),
            price_account,
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        );
    }

    /// A pool with a zero quote reserve has a spot price of zero, which is the no-price sentinel
    /// and must be rejected rather than seeded into the price account.
    #[test]
    #[should_panic(expected = "pool spot price must be non-zero")]
    fn zero_quote_reserve_panics() {
        create_oracle_price_account(
            config_init(),
            pool_with_reserves(RESERVE_A, 0),
            oracle_price_account_uninit(),
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        );
    }

    /// A quote reserve so small relative to the base reserve that the Q64.64 division floors to
    /// zero is also rejected: `reserve_b << 64 < reserve_a` yields a zero spot price.
    #[test]
    #[should_panic(expected = "pool spot price must be non-zero")]
    fn negligible_quote_reserve_floors_to_zero_and_panics() {
        let reserve_a = 1u128 << 65;
        create_oracle_price_account(
            config_init(),
            pool_with_reserves(reserve_a, 1),
            oracle_price_account_uninit(),
            clock(),
            WINDOW_24H,
            AMM_PROGRAM_ID,
        );
    }

    /// A window smaller than `OBSERVATIONS_CAPACITY` can never have a matching `PriceObservations`
    /// account, so the price account could never be updated by `PublishPrice`; it is rejected early
    /// with an AMM-level error before the pool is even decoded.
    #[test]
    #[should_panic(expected = "window_duration must be >= OBSERVATIONS_CAPACITY")]
    fn window_duration_below_capacity_panics() {
        let small_window = u64::from(OBSERVATIONS_CAPACITY)
            .checked_sub(1)
            .expect("OBSERVATIONS_CAPACITY is non-zero");
        create_oracle_price_account(
            config_init(),
            pool(),
            oracle_price_account_uninit(),
            clock(),
            small_window,
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    fn different_windows_produce_distinct_price_account_pdas() {
        let window_7d = 7 * 24 * 60 * 60 * 1_000u64;
        assert_ne!(
            compute_oracle_price_account_pda(TWAP_ORACLE_PROGRAM_ID, pool_id(), WINDOW_24H),
            compute_oracle_price_account_pda(TWAP_ORACLE_PROGRAM_ID, pool_id(), window_7d),
        );
    }
}
