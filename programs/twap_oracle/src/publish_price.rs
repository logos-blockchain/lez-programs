use clock_core::ClockAccountData;
use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ProgramId},
};
use twap_oracle_core::{
    compute_oracle_price_account_pda, compute_price_observations_pda, tick_to_oracle_price,
    OraclePriceAccount, PriceObservations, OBSERVATIONS_CAPACITY,
};

/// Computes the TWAP over the full span of the [`PriceObservations`] ring buffer and writes
/// the result to the [`OraclePriceAccount`].
///
/// Each observations account is calibrated to a specific `window_duration` via its sampling
/// guard (`min_interval = window_duration / OBSERVATIONS_CAPACITY`), so the oldest valid entry
/// is always the natural start of the window — no boundary search is needed.
///
/// Returns all accounts unchanged when fewer than two observations are available. The price
/// account stays at `timestamp = 0` (uninitialized signal) until there is something to publish.
///
/// # Panics
/// Panics if:
/// - `price_observations.account_id` does not match
///   `compute_price_observations_pda(oracle_program_id, price_source_id, window_duration)`.
/// - `oracle_price_account.account_id` does not match
///   `compute_oracle_price_account_pda(oracle_program_id, price_source_id, window_duration)`.
/// - Either account is not a valid initialised account of its respective type.
pub fn publish_price(
    price_observations: AccountWithMetadata,
    oracle_price_account: AccountWithMetadata,
    clock: AccountWithMetadata,
    price_source_id: AccountId,
    window_duration: u64,
    oracle_program_id: ProgramId,
) -> Vec<AccountPostState> {
    assert_eq!(
        price_observations.account_id,
        compute_price_observations_pda(oracle_program_id, price_source_id, window_duration),
        "PublishPrice: price observations account ID does not match expected PDA"
    );
    assert_eq!(
        oracle_price_account.account_id,
        compute_oracle_price_account_pda(oracle_program_id, price_source_id, window_duration),
        "PublishPrice: oracle price account ID does not match expected PDA"
    );

    let clock_data = ClockAccountData::from_bytes(clock.account.data.as_ref());
    let now = clock_data.timestamp;

    let observations = PriceObservations::try_from(&price_observations.account.data)
        .expect("PublishPrice: price observations account must be initialized");
    let mut price_account = OraclePriceAccount::try_from(&oracle_price_account.account.data)
        .expect("PublishPrice: oracle price account must be initialized");

    // No-op: need at least two observations to compute a TWAP.
    if observations.total_entries < 2 {
        return vec![
            AccountPostState::new(price_observations.account.clone()),
            AccountPostState::new(oracle_price_account.account.clone()),
            AccountPostState::new(clock.account.clone()),
        ];
    }

    let capacity =
        usize::try_from(OBSERVATIONS_CAPACITY).expect("OBSERVATIONS_CAPACITY fits in usize");

    // t2: the most recent observation.
    let t2_index = if observations.write_index == 0 {
        capacity
            .checked_sub(1)
            .expect("OBSERVATIONS_CAPACITY is non-zero")
    } else {
        usize::try_from(
            observations
                .write_index
                .checked_sub(1)
                .expect("write_index > 0"),
        )
        .expect("write_index - 1 fits in usize")
    };

    // t1: the oldest valid observation. Once the buffer is full, the oldest entry sits at
    // write_index (the slot about to be overwritten next). Before that, entries start at 0.
    let is_full = observations.total_entries >= u64::from(OBSERVATIONS_CAPACITY);
    let t1_index = if is_full {
        usize::try_from(observations.write_index).expect("write_index fits in usize")
    } else {
        0
    };

    let t1 = observations
        .entries
        .get(t1_index)
        .expect("t1_index is within bounds");
    let t2 = observations
        .entries
        .get(t2_index)
        .expect("t2_index is within bounds");

    let elapsed_ms = t2
        .timestamp
        .checked_sub(t1.timestamp)
        .expect("t2.timestamp >= t1.timestamp");
    let cumulative_diff = t2
        .tick_cumulative
        .checked_sub(t1.tick_cumulative)
        .expect("tick_cumulative difference fits in i64");
    let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
    let twap_tick_i64 = cumulative_diff
        .checked_div(elapsed_ms_i64)
        .expect("elapsed_ms is non-zero");
    let twap_tick = i32::try_from(twap_tick_i64).expect("TWAP tick fits in i32");

    price_account.price = tick_to_oracle_price(twap_tick);
    price_account.timestamp = now;

    let mut oracle_price_account_post = oracle_price_account.account.clone();
    oracle_price_account_post.data = Data::from(&price_account);

    vec![
        AccountPostState::new(price_observations.account.clone()),
        AccountPostState::new(oracle_price_account_post),
        AccountPostState::new(clock.account.clone()),
    ]
}

#[cfg(test)]
mod tests {
    use nssa_core::account::{Account, AccountId, Nonce};
    use twap_oracle_core::{
        compute_oracle_price_account_pda, compute_price_observations_pda, tick_to_oracle_price,
        ObservationEntry, OraclePriceAccount, PriceObservations, OBSERVATIONS_CAPACITY,
    };

    use super::*;

    const ORACLE_PROGRAM_ID: ProgramId = [77u32; 8];
    const CLOCK_PROGRAM_ID: ProgramId = [88u32; 8];
    const WINDOW_24H: u64 = 24 * 60 * 60 * 1_000;

    fn price_source_id() -> AccountId {
        AccountId::new([1u8; 32])
    }
    fn base_asset_id() -> AccountId {
        AccountId::new([10u8; 32])
    }
    fn quote_asset_id() -> AccountId {
        AccountId::new([11u8; 32])
    }

    fn clock_account_with_timestamp(timestamp: u64) -> AccountWithMetadata {
        let data = ClockAccountData {
            block_id: 0,
            timestamp,
        }
        .to_bytes();
        AccountWithMetadata {
            account: Account {
                program_owner: CLOCK_PROGRAM_ID,
                balance: 0,
                data: Data::try_from(data).expect("ClockAccountData fits in Data"),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: AccountId::new([99u8; 32]),
        }
    }

    /// Builds a [`PriceObservations`] from `(timestamp_ms, tick_cumulative)` pairs written in
    /// order starting at index 0. `write_index` is set to `entries_data.len()`.
    fn make_price_observations(
        entries_data: &[(u64, i64)],
        last_recorded_tick: i32,
    ) -> AccountWithMetadata {
        let capacity =
            usize::try_from(OBSERVATIONS_CAPACITY).expect("OBSERVATIONS_CAPACITY fits in usize");
        let mut entries = vec![ObservationEntry::default(); capacity];
        for (i, &(timestamp, tick_cumulative)) in entries_data.iter().enumerate() {
            *entries.get_mut(i).expect("i < capacity") = ObservationEntry {
                timestamp,
                tick_cumulative,
            };
        }
        let write_index = u32::try_from(entries_data.len()).expect("entry count fits in u32");
        let total_entries = u64::try_from(entries_data.len()).expect("entry count fits in u64");
        let obs = PriceObservations {
            price_source_id: price_source_id(),
            write_index,
            total_entries,
            last_recorded_tick,
            entries,
        };
        AccountWithMetadata {
            account: Account {
                program_owner: ORACLE_PROGRAM_ID,
                balance: 0,
                data: Data::from(&obs),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: compute_price_observations_pda(
                ORACLE_PROGRAM_ID,
                price_source_id(),
                WINDOW_24H,
            ),
        }
    }

    fn make_oracle_price_account() -> AccountWithMetadata {
        let account = OraclePriceAccount {
            base_asset: base_asset_id(),
            quote_asset: quote_asset_id(),
            price: 0,
            timestamp: 0,
            source_id: price_source_id(),
            confidence_interval: 0,
        };
        AccountWithMetadata {
            account: Account {
                program_owner: ORACLE_PROGRAM_ID,
                balance: 0,
                data: Data::from(&account),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: compute_oracle_price_account_pda(
                ORACLE_PROGRAM_ID,
                price_source_id(),
                WINDOW_24H,
            ),
        }
    }

    // ── happy path ────────────────────────────────────────────────────────────

    #[test]
    fn returns_three_post_states() {
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            make_oracle_price_account(),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(post_states.len(), 3);
    }

    #[test]
    fn twap_tick_computed_and_stored_as_price() {
        // Constant tick = 100 over 24 h → twap = 100
        let cumulative = 100_i64
            .checked_mul(i64::try_from(WINDOW_24H).expect("fits"))
            .expect("100 * WINDOW_24H fits in i64");
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, cumulative)], 100),
            make_oracle_price_account(),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        assert_eq!(account.price, tick_to_oracle_price(100));
    }

    #[test]
    fn negative_twap_tick_stored_correctly() {
        // Constant tick = -50 over 24 h → twap = -50
        let cumulative = (-50_i64)
            .checked_mul(i64::try_from(WINDOW_24H).expect("fits"))
            .expect("-50 * WINDOW_24H fits in i64");
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, cumulative)], -50),
            make_oracle_price_account(),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        assert_eq!(account.price, tick_to_oracle_price(-50));
    }

    #[test]
    fn zero_twap_tick_stored_correctly() {
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            make_oracle_price_account(),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        assert_eq!(account.price, tick_to_oracle_price(0));
    }

    #[test]
    fn timestamp_set_to_clock_now() {
        let now = WINDOW_24H
            .checked_mul(2)
            .expect("WINDOW_24H * 2 fits in u64");
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            make_oracle_price_account(),
            clock_account_with_timestamp(now),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        assert_eq!(account.timestamp, now);
    }

    #[test]
    fn other_price_account_fields_preserved() {
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            make_oracle_price_account(),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        assert_eq!(account.base_asset, base_asset_id());
        assert_eq!(account.quote_asset, quote_asset_id());
        assert_eq!(account.source_id, price_source_id());
        assert_eq!(account.confidence_interval, 0);
    }

    #[test]
    fn price_observations_account_is_not_modified() {
        let observations = make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0);
        let post_states = publish_price(
            observations.clone(),
            make_oracle_price_account(),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(*post_states[0].account(), observations.account);
    }

    #[test]
    fn clock_account_is_not_modified() {
        let clock = clock_account_with_timestamp(WINDOW_24H);
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            make_oracle_price_account(),
            clock.clone(),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(*post_states[2].account(), clock.account);
    }

    #[test]
    fn twap_uses_oldest_and_newest_entries() {
        // Three observations: tick 0 for first half, tick 200 for second half.
        // t1 = entry[0] (oldest), t2 = entry[2] (newest).
        // Average over full span = (0 * half + 200 * half) / full = 100.
        let half = WINDOW_24H.checked_div(2).expect("fits");
        let half_i64 = i64::try_from(half).expect("fits");
        let full_i64 = i64::try_from(WINDOW_24H).expect("fits");
        // entry[0]: t=0, cumulative=0 (tick was 0 before this)
        // entry[1]: t=half, cumulative=0 (tick=0 held from 0..half, so 0*half=0)
        // entry[2]: t=WINDOW_24H, cumulative=200*half (tick=200 held from half..full)
        let cumulative_at_half = 0_i64;
        let cumulative_at_full = 200_i64.checked_mul(half_i64).expect("fits");
        let post_states = publish_price(
            make_price_observations(
                &[
                    (0, 0),
                    (half, cumulative_at_half),
                    (WINDOW_24H, cumulative_at_full),
                ],
                200,
            ),
            make_oracle_price_account(),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        // twap = (cumulative_at_full - 0) / (WINDOW_24H - 0) = 200*half / full = 200/2 = 100
        let expected_tick = cumulative_at_full.checked_div(full_i64).expect("non-zero");
        assert_eq!(
            account.price,
            tick_to_oracle_price(i32::try_from(expected_tick).expect("tick fits in i32"))
        );
    }

    // ── no-op: insufficient history ───────────────────────────────────────────

    #[test]
    fn noop_when_only_one_observation() {
        let initial = make_oracle_price_account();
        let post_states = publish_price(
            make_price_observations(&[(0, 0)], 0),
            initial.clone(),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(*post_states[1].account(), initial.account);
    }

    #[test]
    fn noop_leaves_price_account_timestamp_at_zero() {
        let post_states = publish_price(
            make_price_observations(&[(0, 0)], 0),
            make_oracle_price_account(),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        assert_eq!(account.timestamp, 0);
    }

    // ── precondition violations ───────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "price observations account ID does not match expected PDA")]
    fn wrong_price_observations_id_panics() {
        let mut wrong = make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0);
        wrong.account_id = AccountId::new([0u8; 32]);
        publish_price(
            wrong,
            make_oracle_price_account(),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "oracle price account ID does not match expected PDA")]
    fn wrong_oracle_price_account_id_panics() {
        let mut wrong = make_oracle_price_account();
        wrong.account_id = AccountId::new([0u8; 32]);
        publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            wrong,
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "price observations account must be initialized")]
    fn uninitialized_price_observations_panics() {
        let uninit = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_price_observations_pda(
                ORACLE_PROGRAM_ID,
                price_source_id(),
                WINDOW_24H,
            ),
        };
        publish_price(
            uninit,
            make_oracle_price_account(),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "oracle price account must be initialized")]
    fn uninitialized_oracle_price_account_panics() {
        let uninit = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_oracle_price_account_pda(
                ORACLE_PROGRAM_ID,
                price_source_id(),
                WINDOW_24H,
            ),
        };
        publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            uninit,
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }
}
