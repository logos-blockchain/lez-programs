use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, Claim, ProgramId},
};
use twap_oracle_core::{
    compute_price_observations_pda, compute_price_observations_pda_seed, ObservationEntry,
    PriceObservations, OBSERVATIONS_CAPACITY,
};

/// Creates and initialises a [`PriceObservations`] for a price source account and time window.
///
/// Authorization is implicit in the PDA relationship: the price observations account is derived
/// from `price_source.account_id` and `window_duration`, so whoever controls the price source
/// controls the observations account.
///
/// The initial observation timestamp is read from `clock`, which must be the canonical 1-block
/// LEZ system clock ([`CLOCK_01_PROGRAM_ACCOUNT_ID`]). Enforcing this prevents a caller from
/// supplying an account they control to seed the TWAP with a forged base timestamp.
///
/// # Panics
/// Panics if:
/// - `price_observations.account_id` does not match
///   `compute_price_observations_pda(oracle_program_id, price_source.account_id, window_duration)`.
/// - `price_observations.account` is not the default (already initialised).
/// - `price_source.is_authorized` is false (caller does not control the price source account).
/// - `clock.account_id` is not [`CLOCK_01_PROGRAM_ACCOUNT_ID`].
pub fn create_price_observations(
    price_observations: AccountWithMetadata,
    price_source: AccountWithMetadata,
    clock: AccountWithMetadata,
    initial_tick: i32,
    window_duration: u64,
    oracle_program_id: ProgramId,
) -> Vec<AccountPostState> {
    let price_source_id = price_source.account_id;
    assert_eq!(
        price_observations.account_id,
        compute_price_observations_pda(oracle_program_id, price_source_id, window_duration),
        "CreatePriceObservations: price observations account ID does not match expected PDA"
    );
    assert_eq!(
        price_observations.account,
        Account::default(),
        "CreatePriceObservations: price observations account must be uninitialized"
    );
    assert!(
        price_source.is_authorized,
        "CreatePriceObservations: price source account must be authorized (caller must control it via a PDA)"
    );
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "CreatePriceObservations: clock account must be the canonical 1-block LEZ clock account"
    );

    let clock_data = ClockAccountData::from_bytes(clock.account.data.as_ref());

    let capacity =
        usize::try_from(OBSERVATIONS_CAPACITY).expect("OBSERVATIONS_CAPACITY fits in usize");
    let mut entries = vec![ObservationEntry::default(); capacity];
    *entries
        .first_mut()
        .expect("OBSERVATIONS_CAPACITY is non-zero") = ObservationEntry {
        timestamp: clock_data.timestamp,
        tick_cumulative: 0,
    };

    let observations = PriceObservations {
        price_source_id,
        write_index: 1,
        total_entries: 1,
        last_recorded_tick: initial_tick,
        entries,
    };

    let mut price_observations_post = price_observations.account.clone();
    price_observations_post.data = Data::from(&observations);

    vec![
        AccountPostState::new_claimed(
            price_observations_post,
            Claim::Pda(compute_price_observations_pda_seed(
                price_source_id,
                window_duration,
            )),
        ),
        AccountPostState::new(price_source.account.clone()),
        AccountPostState::new(clock.account.clone()),
    ]
}

#[cfg(test)]
mod tests {
    use nssa_core::account::{AccountId, Nonce};

    use super::*;

    const ORACLE_PROGRAM_ID: ProgramId = [77u32; 8];
    const CLOCK_PROGRAM_ID: ProgramId = [88u32; 8];
    /// 24-hour window in milliseconds, used as the default window for tests.
    const WINDOW_24H: u64 = 24 * 60 * 60 * 1_000;

    fn price_source_id() -> AccountId {
        AccountId::new([1u8; 32])
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

    fn price_source_authorized() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [42u32; 8],
                balance: 0,
                data: Data::default(),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: price_source_id(),
        }
    }

    fn price_observations_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_price_observations_pda(
                ORACLE_PROGRAM_ID,
                price_source_id(),
                WINDOW_24H,
            ),
        }
    }

    // ── happy path ────────────────────────────────────────────────────────────

    #[test]
    fn returns_three_post_states() {
        let post_states = create_price_observations(
            price_observations_uninit(),
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(post_states.len(), 3);
    }

    #[test]
    fn price_observations_post_state_is_pda_claimed() {
        let post_states = create_price_observations(
            price_observations_uninit(),
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(
            post_states[0].required_claim(),
            Some(Claim::Pda(compute_price_observations_pda_seed(
                price_source_id(),
                WINDOW_24H
            )))
        );
    }

    #[test]
    fn price_source_and_clock_post_states_are_unchanged() {
        let price_source = price_source_authorized();
        let clock = clock_account_with_timestamp(42_000);
        let post_states = create_price_observations(
            price_observations_uninit(),
            price_source.clone(),
            clock.clone(),
            10,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(*post_states[1].account(), price_source.account);
        assert_eq!(*post_states[2].account(), clock.account);
    }

    #[test]
    fn initial_observation_has_zero_cumulative_and_correct_timestamp() {
        let timestamp = 123_456_789u64;

        let post_states = create_price_observations(
            price_observations_uninit(),
            price_source_authorized(),
            clock_account_with_timestamp(timestamp),
            -42,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );

        let feed = PriceObservations::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid PriceObservations");

        assert_eq!(feed.entries[0].tick_cumulative, 0);
        assert_eq!(feed.entries[0].timestamp, timestamp);
    }

    #[test]
    fn initial_tick_stored_as_last_recorded_tick() {
        let initial_tick = -42i32;

        let post_states = create_price_observations(
            price_observations_uninit(),
            price_source_authorized(),
            clock_account_with_timestamp(0),
            initial_tick,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );

        let feed = PriceObservations::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid PriceObservations");

        assert_eq!(feed.last_recorded_tick, initial_tick);
    }

    #[test]
    fn write_index_and_total_entries_start_at_one() {
        let post_states = create_price_observations(
            price_observations_uninit(),
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );

        let feed = PriceObservations::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid PriceObservations");

        assert_eq!(feed.write_index, 1);
        assert_eq!(feed.total_entries, 1);
    }

    #[test]
    fn remaining_entries_are_default() {
        let post_states = create_price_observations(
            price_observations_uninit(),
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );

        let feed = PriceObservations::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid PriceObservations");

        assert_eq!(
            feed.entries.len(),
            usize::try_from(OBSERVATIONS_CAPACITY).expect("OBSERVATIONS_CAPACITY fits in usize")
        );
        assert!(feed.entries[1..]
            .iter()
            .all(|e| *e == ObservationEntry::default()));
    }

    #[test]
    fn price_source_id_stored_correctly() {
        let post_states = create_price_observations(
            price_observations_uninit(),
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );

        let feed = PriceObservations::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid PriceObservations");

        assert_eq!(feed.price_source_id, price_source_id());
    }

    #[test]
    fn different_windows_produce_distinct_pdas() {
        let window_24h = 24 * 60 * 60 * 1_000u64;
        let window_7d = 7 * 24 * 60 * 60 * 1_000u64;
        assert_ne!(
            compute_price_observations_pda(ORACLE_PROGRAM_ID, price_source_id(), window_24h),
            compute_price_observations_pda(ORACLE_PROGRAM_ID, price_source_id(), window_7d),
        );
    }

    #[test]
    fn positive_and_negative_initial_ticks_stored_as_last_recorded_tick() {
        for tick in [i32::MIN, -1, 0, 1, i32::MAX] {
            let post_states = create_price_observations(
                price_observations_uninit(),
                price_source_authorized(),
                clock_account_with_timestamp(0),
                tick,
                WINDOW_24H,
                ORACLE_PROGRAM_ID,
            );
            let feed = PriceObservations::try_from(&post_states[0].account().data)
                .expect("post state must contain a valid PriceObservations");
            assert_eq!(feed.last_recorded_tick, tick);
        }
    }

    // ── precondition violations ───────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "price observations account ID does not match expected PDA")]
    fn wrong_price_feed_account_id_panics() {
        let mut wrong_feed = price_observations_uninit();
        wrong_feed.account_id = AccountId::new([0u8; 32]);
        create_price_observations(
            wrong_feed,
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    /// An attacker who controls their own price source cannot register an observations account
    /// that claims to be derived from a *different* (victim's) price source.
    ///
    /// The PDA derivation ties the observations account to the price source that was passed: if
    /// the caller supplies their own authorized source but the victim's observations account ID,
    /// the PDA check will fail because the computed PDA (from attacker's source) won't match.
    #[test]
    #[should_panic(expected = "price observations account ID does not match expected PDA")]
    fn cannot_register_observations_for_another_price_source() {
        let victim_source_id = AccountId::new([2u8; 32]);
        // The attacker passes the victim's observations PDA as the target account…
        let victim_observations_pda =
            compute_price_observations_pda(ORACLE_PROGRAM_ID, victim_source_id, WINDOW_24H);
        let mut attacker_observations = price_observations_uninit();
        attacker_observations.account_id = victim_observations_pda;

        // …but only controls their own price source (price_source_id = [1u8; 32]).
        create_price_observations(
            attacker_observations,
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "price observations account must be uninitialized")]
    fn already_initialized_price_feed_panics() {
        let mut initialized_feed = price_observations_uninit();
        initialized_feed.account.data = Data::try_from(vec![1u8; 10]).expect("fits in Data");
        create_price_observations(
            initialized_feed,
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "price source account must be authorized")]
    fn unauthorized_price_source_panics() {
        let mut unauthorized = price_source_authorized();
        unauthorized.is_authorized = false;
        create_price_observations(
            price_observations_uninit(),
            unauthorized,
            clock_account_with_timestamp(0),
            0,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    /// The coarser-cadence clock accounts (10-block, 50-block) are still rejected: the oracle
    /// must read the most fine-grained 1-block clock.
    #[test]
    #[should_panic(expected = "clock account must be the canonical 1-block LEZ clock account")]
    fn non_canonical_clock_account_id_panics() {
        use clock_core::CLOCK_10_PROGRAM_ACCOUNT_ID;
        create_price_observations(
            price_observations_uninit(),
            price_source_authorized(),
            clock_account_with_id(0, CLOCK_10_PROGRAM_ACCOUNT_ID),
            0,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    /// An attacker cannot supply an account they control — even one whose data deserializes as a
    /// valid [`ClockAccountData`] with a forged timestamp — in place of the system clock.
    #[test]
    #[should_panic(expected = "clock account must be the canonical 1-block LEZ clock account")]
    fn forged_clock_account_panics() {
        let forged_clock = clock_account_with_id(9_999_999_999, AccountId::new([7u8; 32]));
        create_price_observations(
            price_observations_uninit(),
            price_source_authorized(),
            forged_clock,
            0,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }
}
