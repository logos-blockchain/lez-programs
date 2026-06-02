use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ProgramId},
};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_oracle_price_account_pda,
    compute_price_observations_pda, tick_to_oracle_price, CurrentTickAccount, OraclePriceAccount,
    PriceObservations, MAX_TICK_DELTA, OBSERVATIONS_CAPACITY,
};

/// Computes the TWAP over the span from the oldest stored observation up to `now` and writes the
/// result to the [`OraclePriceAccount`].
///
/// The *body* of the average comes from the [`PriceObservations`] ring buffer (oldest valid entry
/// `t1` through newest stored entry `t2`). The *final segment* from `t2` to `now` is extrapolated
/// from the [`CurrentTickAccount`], mirroring Uniswap's `OracleLibrary.consult`, which always
/// projects the accumulator to the present rather than reading only stored points.
///
/// Projecting to `now` is what makes the published `timestamp = now` truthful even when no tick has
/// been recorded for a while: a price that has simply not changed yields a *fresh* timestamp and
/// the correct (unchanged) value, instead of a stale window stamped with a current time. If the
/// price *did* move since `t2` and the move was reported to the current tick account, the tail
/// carries it forward (clamped) rather than publishing a frozen pre-move average.
///
/// The tail is split at the current tick's `last_updated`: the live tick is only known to have held
/// since then, so it is integrated only over `[last_updated, now]`, while `[t2, last_updated]`
/// carries the tick stored at t2 (`last_recorded_tick`). Without this split a tick written moments
/// before publish would be smeared across the whole unrecorded gap, letting a late spot move
/// dominate a long stale tail and inflating a supposedly fresh TWAP. The live-tick segment is
/// clamped against `last_recorded_tick` by [`MAX_TICK_DELTA`] — the same anti-manipulation bound
/// `RecordTick` applies — bounding how far an attacker who can move the current tick can shift the
/// published average.
///
/// Each observations account is calibrated to a specific `window_duration` via its sampling guard
/// (`min_interval = window_duration / OBSERVATIONS_CAPACITY`), so the oldest valid entry is always
/// the natural start of the window — no boundary search is needed.
///
/// Returns all accounts unchanged when fewer than two observations are available. The price account
/// stays at `timestamp = 0` (uninitialized signal) until there is something to publish.
///
/// The publication timestamp is taken from `clock`, which must be [`CLOCK_01_PROGRAM_ACCOUNT_ID`].
/// It becomes the consumer-facing freshness signal on the [`OraclePriceAccount`], so a forged or
/// caller-controlled clock could make a stale price look current; it must never be caller-supplied.
///
/// # Panics
/// Panics if:
/// - `price_observations.account_id` does not match
///   `compute_price_observations_pda(oracle_program_id, price_source_id, window_duration)`.
/// - `oracle_price_account.account_id` does not match
///   `compute_oracle_price_account_pda(oracle_program_id, price_source_id, window_duration)`.
/// - `current_tick_account.account_id` does not match
///   `compute_current_tick_account_pda(oracle_program_id, price_source_id)`.
/// - `clock.account_id` is not [`CLOCK_01_PROGRAM_ACCOUNT_ID`].
/// - Any account is not a valid initialised account of its respective type.
pub fn publish_price(
    price_observations: AccountWithMetadata,
    oracle_price_account: AccountWithMetadata,
    current_tick_account: AccountWithMetadata,
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
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(oracle_program_id, price_source_id),
        "PublishPrice: current tick account ID does not match expected PDA"
    );
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "PublishPrice: clock account must be the canonical 1-block LEZ clock account"
    );

    let clock_data = ClockAccountData::from_bytes(clock.account.data.as_ref());
    let now = clock_data.timestamp;

    let observations = PriceObservations::try_from(&price_observations.account.data)
        .expect("PublishPrice: price observations account must be initialized");
    let mut price_account = OraclePriceAccount::try_from(&oracle_price_account.account.data)
        .expect("PublishPrice: oracle price account must be initialized");
    let current_tick_data = CurrentTickAccount::try_from(&current_tick_account.account.data)
        .expect("PublishPrice: current tick account must be initialized");

    // No-op: need at least two observations to compute a TWAP.
    if observations.total_entries < 2 {
        return vec![
            AccountPostState::new(price_observations.account.clone()),
            AccountPostState::new(oracle_price_account.account.clone()),
            AccountPostState::new(current_tick_account.account.clone()),
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

    // Extrapolate the accumulator from the newest stored observation (t2) to `now`. The live tick
    // is only known to have held since `current_tick_data.last_updated`; before that, the best
    // estimate for the unrecorded gap is the tick stored at t2 (`last_recorded_tick`). Splitting
    // the tail at that boundary stops a tick written moments before publish from being smeared
    // across the whole gap — which would let a late price move dominate a long stale tail. The
    // live-tick segment is clamped against last_recorded_tick by MAX_TICK_DELTA — the same bound
    // RecordTick applies. With no elapsed tail (`now == t2.timestamp`) both segments are empty and
    // the TWAP is exactly the stored-window average.
    let current_tick = current_tick_data.tick;
    let delta = current_tick.saturating_sub(observations.last_recorded_tick);
    let clamped_delta = delta.clamp(-MAX_TICK_DELTA, MAX_TICK_DELTA);
    let clamped_tick = observations
        .last_recorded_tick
        .saturating_add(clamped_delta);

    // `now` comes from the canonical clock and is monotonic, so it is never before the newest
    // stored observation. The boundary is clamped into `[t2.timestamp, now]`: a `last_updated` at
    // or before t2 means the live tick provably held for the whole tail (pre-boundary segment
    // empty); one at or after `now` means the live tick has accrued no elapsed time yet
    // (post-boundary segment empty). The clamp also keeps a degenerate clock from underflowing the
    // saturating subtractions into a huge tail.
    let boundary = current_tick_data.last_updated.clamp(t2.timestamp, now);
    let pre_ms = boundary.saturating_sub(t2.timestamp);
    let post_ms = now.saturating_sub(boundary);
    let pre_ms_i64 = i64::try_from(pre_ms).expect("pre_ms fits in i64");
    let post_ms_i64 = i64::try_from(post_ms).expect("post_ms fits in i64");

    // Pre-boundary segment carries `last_recorded_tick` (the tick at t2); post-boundary segment
    // carries the clamped live tick.
    let pre_cumulative = i64::from(observations.last_recorded_tick)
        .checked_mul(pre_ms_i64)
        .expect("pre-boundary tail cumulative fits in i64");
    let post_cumulative = i64::from(clamped_tick)
        .checked_mul(post_ms_i64)
        .expect("post-boundary tail cumulative fits in i64");
    let cum_now = t2
        .tick_cumulative
        .checked_add(pre_cumulative)
        .and_then(|acc| acc.checked_add(post_cumulative))
        .expect("extrapolated tick_cumulative fits in i64");

    // Average over [t1, now]. `now > t1.timestamp` because `now >= t2.timestamp > t1.timestamp`
    // (the sampling guard makes stored timestamps strictly increasing), so the divisor is positive.
    let elapsed_ms = now.checked_sub(t1.timestamp).expect("now >= t1.timestamp");
    let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
    let cumulative_diff = cum_now
        .checked_sub(t1.tick_cumulative)
        .expect("tick_cumulative difference fits in i64");
    // Floor division (round toward −∞), matching Uniswap's `OracleLibrary.consult`. The divisor is
    // always positive, so `div_euclid` is exactly the floor. Plain truncating division would round
    // toward zero, biasing negative TWAPs upward by one tick.
    let twap_tick_i64 = cumulative_diff
        .checked_div_euclid(elapsed_ms_i64)
        .expect("elapsed_ms is non-zero");
    let twap_tick = i32::try_from(twap_tick_i64).expect("TWAP tick fits in i32");

    price_account.price = tick_to_oracle_price(twap_tick);
    price_account.timestamp = now;

    let mut oracle_price_account_post = oracle_price_account.account.clone();
    oracle_price_account_post.data = Data::from(&price_account);

    vec![
        AccountPostState::new(price_observations.account.clone()),
        AccountPostState::new(oracle_price_account_post),
        AccountPostState::new(current_tick_account.account.clone()),
        AccountPostState::new(clock.account.clone()),
    ]
}

#[cfg(test)]
mod tests {
    use nssa_core::account::{Account, AccountId, Nonce};
    use twap_oracle_core::{
        compute_current_tick_account_pda, compute_oracle_price_account_pda,
        compute_price_observations_pda, tick_to_oracle_price, CurrentTickAccount, ObservationEntry,
        OraclePriceAccount, PriceObservations, MAX_TICK_DELTA, OBSERVATIONS_CAPACITY,
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

    fn clock_account_with_id(timestamp: u64, account_id: AccountId) -> AccountWithMetadata {
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
            account_id,
        }
    }

    fn clock_account_with_timestamp(timestamp: u64) -> AccountWithMetadata {
        clock_account_with_id(timestamp, CLOCK_01_PROGRAM_ACCOUNT_ID)
    }

    /// Builds a [`CurrentTickAccount`] at the canonical PDA for [`price_source_id`].
    fn make_current_tick_account(tick: i32, last_updated: u64) -> AccountWithMetadata {
        let stored = CurrentTickAccount { tick, last_updated };
        AccountWithMetadata {
            account: Account {
                program_owner: ORACLE_PROGRAM_ID,
                balance: 0,
                data: Data::from(&stored),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: compute_current_tick_account_pda(ORACLE_PROGRAM_ID, price_source_id()),
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
    fn returns_four_post_states() {
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            make_oracle_price_account(),
            make_current_tick_account(0, WINDOW_24H),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(post_states.len(), 4);
    }

    #[test]
    fn twap_tick_computed_and_stored_as_price() {
        // Constant tick = 100 over 24 h, published exactly at the newest observation (zero tail) →
        // twap = 100
        let cumulative = 100_i64
            .checked_mul(i64::try_from(WINDOW_24H).expect("fits"))
            .expect("100 * WINDOW_24H fits in i64");
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, cumulative)], 100),
            make_oracle_price_account(),
            make_current_tick_account(100, WINDOW_24H),
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
        // Constant tick = -50 over 24 h (zero tail) → twap = -50
        let cumulative = (-50_i64)
            .checked_mul(i64::try_from(WINDOW_24H).expect("fits"))
            .expect("-50 * WINDOW_24H fits in i64");
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, cumulative)], -50),
            make_oracle_price_account(),
            make_current_tick_account(-50, WINDOW_24H),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        assert_eq!(account.price, tick_to_oracle_price(-50));
    }

    /// When the average tick is negative and the division has a remainder, the result must round
    /// down (toward −∞), like Uniswap's oracle. The cumulative here is `-(50·W + 1)` over a span of
    /// `W` (zero tail), so the exact average is just below −50 and must floor to **−51**.
    /// Truncating division would instead round toward zero and give −50 — one tick too high.
    #[test]
    fn negative_twap_with_remainder_floors_toward_negative_infinity() {
        let elapsed_i64 = i64::try_from(WINDOW_24H).expect("fits");
        let cumulative_diff = 50_i64
            .checked_mul(elapsed_i64)
            .and_then(|v| v.checked_add(1))
            .and_then(i64::checked_neg)
            .expect("-(50 * WINDOW_24H + 1) fits in i64");
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, cumulative_diff)], -50),
            make_oracle_price_account(),
            make_current_tick_account(-50, WINDOW_24H),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        assert_eq!(
            account.price,
            tick_to_oracle_price(-51),
            "must floor to -51"
        );
        assert_ne!(
            account.price,
            tick_to_oracle_price(-50),
            "truncating toward zero (-50) would be wrong"
        );
    }

    #[test]
    fn zero_twap_tick_stored_correctly() {
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            make_oracle_price_account(),
            make_current_tick_account(0, WINDOW_24H),
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
            make_current_tick_account(0, WINDOW_24H),
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
            make_current_tick_account(0, WINDOW_24H),
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
            make_current_tick_account(0, WINDOW_24H),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(*post_states[0].account(), observations.account);
    }

    #[test]
    fn current_tick_account_is_not_modified() {
        let current_tick = make_current_tick_account(123, WINDOW_24H);
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            make_oracle_price_account(),
            current_tick.clone(),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(*post_states[2].account(), current_tick.account);
    }

    #[test]
    fn clock_account_is_not_modified() {
        let clock = clock_account_with_timestamp(WINDOW_24H);
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            make_oracle_price_account(),
            make_current_tick_account(0, WINDOW_24H),
            clock.clone(),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(*post_states[3].account(), clock.account);
    }

    #[test]
    fn twap_uses_oldest_and_newest_entries() {
        // Three observations: tick 0 for first half, tick 200 for second half, published at the
        // newest observation (zero tail). t1 = entry[0] (oldest), t2 = entry[2] (newest).
        // Average over full span = (0 * half + 200 * half) / full = 100.
        let half = WINDOW_24H.checked_div(2).expect("fits");
        let half_i64 = i64::try_from(half).expect("fits");
        let full_i64 = i64::try_from(WINDOW_24H).expect("fits");
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
            make_current_tick_account(200, WINDOW_24H),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        let expected_tick = cumulative_at_full.checked_div(full_i64).expect("non-zero");
        assert_eq!(
            account.price,
            tick_to_oracle_price(i32::try_from(expected_tick).expect("tick fits in i32"))
        );
    }

    // ── tail extrapolation to `now` ─────────────────────────────────────────────

    /// The motivating case: the price has not changed and no tick has been recorded for a full
    /// window, but the current tick account still reports the same tick. Publishing must extend the
    /// average all the way to `now`, yielding the (unchanged) price and a *fresh* `now` timestamp —
    /// not a stale window. The newest stored observation is one whole window old.
    #[test]
    fn extrapolates_constant_price_to_now_with_fresh_timestamp() {
        let tick = 100_i32;
        let t2_time = WINDOW_24H;
        let cumulative_at_t2 = i64::from(tick)
            .checked_mul(i64::try_from(t2_time).expect("fits"))
            .expect("fits");
        let now = t2_time.checked_mul(2).expect("fits"); // one window past the last observation
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (t2_time, cumulative_at_t2)], tick),
            make_oracle_price_account(),
            make_current_tick_account(tick, t2_time),
            clock_account_with_timestamp(now),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        // Constant tick extended through the tail → average is still exactly the constant tick.
        assert_eq!(account.price, tick_to_oracle_price(tick));
        // The timestamp is `now` and it is honest: the average genuinely runs to `now`.
        assert_eq!(account.timestamp, now);
    }

    /// Proves the tail is actually integrated (not just relabeled): stored history is flat at tick
    /// 0 over `[0, W]`, then the price moved to tick 100 and the move reached the current tick
    /// account but was *not* yet recorded into observations. Publishing at `2W` must blend
    /// `0 over [0, W]` with `100 over [W, 2W]` → average 50. Without extrapolation this would be 0.
    #[test]
    fn tail_segment_extrapolated_with_current_tick() {
        let t2_time = WINDOW_24H;
        let now = t2_time.checked_mul(2).expect("fits");
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (t2_time, 0)], 0),
            make_oracle_price_account(),
            make_current_tick_account(100, t2_time),
            clock_account_with_timestamp(now),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        // (0 * W + 100 * W) / 2W = 50
        assert_eq!(account.price, tick_to_oracle_price(50));
        assert_ne!(
            account.price,
            tick_to_oracle_price(0),
            "ignoring the tail (old behavior) would publish 0"
        );
        assert_eq!(account.timestamp, now);
    }

    /// A large spot jump in the unrecorded tail is clamped to `MAX_TICK_DELTA`, bounding how much
    /// an attacker who can move the current tick can shift the published average. History is
    /// flat at tick 0 over `[0, W]`; the current tick jumps to `10 * MAX_TICK_DELTA`;
    /// publishing at `2W` integrates the *clamped* tick over the tail → average `MAX_TICK_DELTA
    /// / 2`.
    #[test]
    fn tail_extrapolation_clamps_large_tick_jump() {
        let t2_time = WINDOW_24H;
        let now = t2_time.checked_mul(2).expect("fits");
        let huge = MAX_TICK_DELTA.checked_mul(10).expect("fits in i32");
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (t2_time, 0)], 0),
            make_oracle_price_account(),
            make_current_tick_account(huge, t2_time),
            clock_account_with_timestamp(now),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        // (0 * W + MAX_TICK_DELTA * W) / 2W = MAX_TICK_DELTA / 2
        let expected_tick = MAX_TICK_DELTA.checked_div(2).expect("non-zero");
        assert_eq!(account.price, tick_to_oracle_price(expected_tick));
        assert_eq!(account.timestamp, now);
    }

    /// A downward move in the tail is integrated and floors toward −∞: history flat at tick 0 over
    /// `[0, W]`, current tick `-100`, published at `2W` → `(0·W + (−100)·W) / 2W = −50`.
    #[test]
    fn negative_tail_segment_extrapolated() {
        let t2_time = WINDOW_24H;
        let now = t2_time.checked_mul(2).expect("fits");
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (t2_time, 0)], 0),
            make_oracle_price_account(),
            make_current_tick_account(-100, t2_time),
            clock_account_with_timestamp(now),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        assert_eq!(account.price, tick_to_oracle_price(-50));
        assert_eq!(account.timestamp, now);
    }

    /// A spot move reported to the current tick account *just before* publish must not be smeared
    /// across the whole unrecorded tail. History is flat at tick 0 over `[0, W]`; the tail runs
    /// `[W, 2W]` but the current tick only moved to 100 at `2W - 1` ms. The move may be integrated
    /// only over its 1 ms of validity, leaving the average effectively at tick 0 — not the tick 50
    /// that ignoring `last_updated` and extrapolating 100 over the full tail would produce.
    #[test]
    fn tail_extrapolation_respects_current_tick_last_updated() {
        let t2_time = WINDOW_24H;
        let now = t2_time.checked_mul(2).expect("fits");
        let tick_update_time = now.checked_sub(1).expect("now > 0");
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (t2_time, 0)], 0),
            make_oracle_price_account(),
            make_current_tick_account(100, tick_update_time),
            clock_account_with_timestamp(now),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        assert_eq!(account.price, tick_to_oracle_price(0));
    }

    /// Publishing exactly at the newest observation leaves no tail, so the live tick does not
    /// affect the result even if it has moved: the published price is purely the stored-window
    /// average.
    #[test]
    fn zero_tail_ignores_current_tick() {
        let cumulative = 100_i64
            .checked_mul(i64::try_from(WINDOW_24H).expect("fits"))
            .expect("fits");
        let post_states = publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, cumulative)], 100),
            make_oracle_price_account(),
            // Current tick wildly different, but the tail is zero-length so it is irrelevant.
            make_current_tick_account(5_000, WINDOW_24H),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        assert_eq!(account.price, tick_to_oracle_price(100));
    }

    // ── no-op: insufficient history ───────────────────────────────────────────

    #[test]
    fn noop_when_only_one_observation() {
        let initial = make_oracle_price_account();
        let post_states = publish_price(
            make_price_observations(&[(0, 0)], 0),
            initial.clone(),
            make_current_tick_account(0, 0),
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
            make_current_tick_account(0, 0),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[1].account().data)
            .expect("valid OraclePriceAccount");
        assert_eq!(account.timestamp, 0);
    }

    #[test]
    fn noop_returns_four_post_states() {
        let post_states = publish_price(
            make_price_observations(&[(0, 0)], 0),
            make_oracle_price_account(),
            make_current_tick_account(0, 0),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(post_states.len(), 4);
    }

    // ── clock validation ──────────────────────────────────────────────────────

    /// The coarser-cadence clock accounts (10-block, 50-block) are rejected: the published
    /// timestamp must come from the canonical 1-block clock.
    #[test]
    #[should_panic(expected = "clock account must be the canonical 1-block LEZ clock account")]
    fn non_canonical_clock_account_id_panics() {
        use clock_core::CLOCK_10_PROGRAM_ACCOUNT_ID;
        publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            make_oracle_price_account(),
            make_current_tick_account(0, WINDOW_24H),
            clock_account_with_id(WINDOW_24H, CLOCK_10_PROGRAM_ACCOUNT_ID),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    /// An attacker cannot supply an account they control — even one whose data deserializes as a
    /// valid [`ClockAccountData`] with a forged timestamp — in place of the system clock. The
    /// published timestamp is the consumer-facing freshness signal, so a forged clock could make a
    /// stale price look current.
    #[test]
    #[should_panic(expected = "clock account must be the canonical 1-block LEZ clock account")]
    fn forged_clock_account_panics() {
        publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            make_oracle_price_account(),
            make_current_tick_account(0, WINDOW_24H),
            clock_account_with_id(WINDOW_24H, AccountId::new([7u8; 32])),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
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
            make_current_tick_account(0, WINDOW_24H),
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
            make_current_tick_account(0, WINDOW_24H),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "current tick account ID does not match expected PDA")]
    fn wrong_current_tick_account_id_panics() {
        let mut wrong = make_current_tick_account(0, WINDOW_24H);
        wrong.account_id = AccountId::new([0u8; 32]);
        publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            make_oracle_price_account(),
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
            make_current_tick_account(0, WINDOW_24H),
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
            make_current_tick_account(0, WINDOW_24H),
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "current tick account must be initialized")]
    fn uninitialized_current_tick_account_panics() {
        let uninit = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_current_tick_account_pda(ORACLE_PROGRAM_ID, price_source_id()),
        };
        publish_price(
            make_price_observations(&[(0, 0), (WINDOW_24H, 0)], 0),
            make_oracle_price_account(),
            uninit,
            clock_account_with_timestamp(WINDOW_24H),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }
}
