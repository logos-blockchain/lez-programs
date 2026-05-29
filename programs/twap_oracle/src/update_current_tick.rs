use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use nssa_core::{
    account::{AccountWithMetadata, Data},
    program::{AccountPostState, ProgramId},
};
use twap_oracle_core::{compute_current_tick_account_pda, price_to_tick, CurrentTickAccount};

/// Updates the tick stored in an existing [`CurrentTickAccount`] from a new spot price.
///
/// The price source reports a spot **price** (`Q64.64` ratio); this function converts it to a
/// tick via [`price_to_tick`], so the source never needs to know about ticks.
///
/// The timestamp is taken from `clock`, which must be [`CLOCK_01_PROGRAM_ACCOUNT_ID`]; it is never
/// caller-supplied, so it cannot be forged.
///
/// # Panics
/// Panics if:
/// - `current_tick_account.account_id` does not match
///   `compute_current_tick_account_pda(oracle_program_id, price_source.account_id)`.
/// - `current_tick_account.account` is not a valid, initialised [`CurrentTickAccount`].
/// - `price_source.is_authorized` is false.
/// - `clock.account_id` is not [`CLOCK_01_PROGRAM_ACCOUNT_ID`].
pub fn update_current_tick(
    current_tick_account: AccountWithMetadata,
    price_source: AccountWithMetadata,
    clock: AccountWithMetadata,
    price: u128,
    oracle_program_id: ProgramId,
) -> Vec<AccountPostState> {
    let price_source_id = price_source.account_id;
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(oracle_program_id, price_source_id),
        "UpdateCurrentTick: current tick account ID does not match expected PDA"
    );
    assert!(
        price_source.is_authorized,
        "UpdateCurrentTick: price source account must be authorized"
    );
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "UpdateCurrentTick: clock account must be the canonical 1-block LEZ clock account"
    );

    let mut stored = CurrentTickAccount::try_from(&current_tick_account.account.data)
        .expect("UpdateCurrentTick: current tick account must be initialized");

    let clock_data = ClockAccountData::from_bytes(clock.account.data.as_ref());
    stored.tick = price_to_tick(price);
    stored.last_updated = clock_data.timestamp;

    let mut current_tick_account_post = current_tick_account.account.clone();
    current_tick_account_post.data = Data::from(&stored);

    vec![
        AccountPostState::new(current_tick_account_post),
        AccountPostState::new(price_source.account.clone()),
        AccountPostState::new(clock.account.clone()),
    ]
}

#[cfg(test)]
mod tests {
    use nssa_core::account::{Account, AccountId, Nonce};
    use twap_oracle_core::compute_current_tick_account_pda;

    use super::*;

    const ORACLE_PROGRAM_ID: ProgramId = [77u32; 8];
    const CLOCK_PROGRAM_ID: ProgramId = [88u32; 8];
    /// `1.0` in Q64.64 — the spot price at tick 0.
    const UNIT_PRICE: u128 = 1u128 << 64;

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

    fn current_tick_account_initialized(tick: i32, last_updated: u64) -> AccountWithMetadata {
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

    // ── happy path ────────────────────────────────────────────────────────────

    #[test]
    fn returns_three_post_states() {
        let post_states = update_current_tick(
            current_tick_account_initialized(0, 0),
            price_source_authorized(),
            clock_account_with_timestamp(1_000),
            UNIT_PRICE,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(post_states.len(), 3);
    }

    /// The function overwrites the stored tick with the one the oracle derives from the new
    /// price — i.e. it delegates to `price_to_tick`. A price above 1.0 yields a positive tick,
    /// so the stored value also changes away from the initial tick.
    #[test]
    fn price_converted_and_tick_updated() {
        let price = UNIT_PRICE << 1; // 2.0 → a positive tick
        let post_states = update_current_tick(
            current_tick_account_initialized(100, 0),
            price_source_authorized(),
            clock_account_with_timestamp(1_000),
            price,
            ORACLE_PROGRAM_ID,
        );
        let account = CurrentTickAccount::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid CurrentTickAccount");
        assert_eq!(account.tick, twap_oracle_core::price_to_tick(price));
        assert_ne!(account.tick, 100);
    }

    #[test]
    fn timestamp_updated_from_clock() {
        let post_states = update_current_tick(
            current_tick_account_initialized(0, 0),
            price_source_authorized(),
            clock_account_with_timestamp(999_000),
            UNIT_PRICE,
            ORACLE_PROGRAM_ID,
        );
        let account = CurrentTickAccount::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid CurrentTickAccount");
        assert_eq!(account.last_updated, 999_000);
    }

    /// The stored tick is whatever `price_to_tick` derives for the supplied price. The
    /// conversion's own correctness is covered by `twap_oracle_core` tests.
    #[test]
    fn prices_convert_via_price_to_tick() {
        for price in [
            1u128,
            UNIT_PRICE >> 10,
            UNIT_PRICE,
            UNIT_PRICE << 10,
            u128::MAX,
        ] {
            let post_states = update_current_tick(
                current_tick_account_initialized(0, 0),
                price_source_authorized(),
                clock_account_with_timestamp(0),
                price,
                ORACLE_PROGRAM_ID,
            );
            let account = CurrentTickAccount::try_from(&post_states[0].account().data)
                .expect("post state must contain a valid CurrentTickAccount");
            assert_eq!(account.tick, twap_oracle_core::price_to_tick(price));
        }
    }

    #[test]
    fn price_source_and_clock_post_states_are_unchanged() {
        let price_source = price_source_authorized();
        let clock = clock_account_with_timestamp(42_000);

        let post_states = update_current_tick(
            current_tick_account_initialized(0, 0),
            price_source.clone(),
            clock.clone(),
            UNIT_PRICE,
            ORACLE_PROGRAM_ID,
        );

        assert_eq!(*post_states[1].account(), price_source.account);
        assert_eq!(*post_states[2].account(), clock.account);
    }

    // ── precondition violations ───────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "current tick account ID does not match expected PDA")]
    fn wrong_account_id_panics() {
        let mut wrong = current_tick_account_initialized(0, 0);
        wrong.account_id = AccountId::new([0u8; 32]);
        update_current_tick(
            wrong,
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "current tick account must be initialized")]
    fn uninitialized_account_panics() {
        let uninit = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_current_tick_account_pda(ORACLE_PROGRAM_ID, price_source_id()),
        };
        update_current_tick(
            uninit,
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "price source account must be authorized")]
    fn unauthorized_price_source_panics() {
        let mut unauthorized = price_source_authorized();
        unauthorized.is_authorized = false;
        update_current_tick(
            current_tick_account_initialized(0, 0),
            unauthorized,
            clock_account_with_timestamp(0),
            0,
            ORACLE_PROGRAM_ID,
        );
    }

    /// The coarser-cadence clock accounts (10-block, 50-block) are rejected: the oracle must read
    /// the most fine-grained 1-block clock.
    #[test]
    #[should_panic(expected = "clock account must be the canonical 1-block LEZ clock account")]
    fn non_canonical_clock_account_id_panics() {
        use clock_core::CLOCK_10_PROGRAM_ACCOUNT_ID;
        update_current_tick(
            current_tick_account_initialized(0, 0),
            price_source_authorized(),
            clock_account_with_id(1_000, CLOCK_10_PROGRAM_ACCOUNT_ID),
            UNIT_PRICE,
            ORACLE_PROGRAM_ID,
        );
    }

    /// An attacker cannot supply an account they control — even one whose data deserializes as a
    /// valid [`ClockAccountData`] with a forged timestamp — in place of the system clock.
    #[test]
    #[should_panic(expected = "clock account must be the canonical 1-block LEZ clock account")]
    fn forged_clock_account_panics() {
        update_current_tick(
            current_tick_account_initialized(0, 0),
            price_source_authorized(),
            clock_account_with_id(1_000, AccountId::new([7u8; 32])),
            UNIT_PRICE,
            ORACLE_PROGRAM_ID,
        );
    }

    /// An attacker who controls their own price source cannot update a different (victim's)
    /// current tick account. The PDA is derived from the price source ID, so presenting an
    /// authorized attacker source against the victim's account ID will always fail the PDA check.
    #[test]
    #[should_panic(expected = "current tick account ID does not match expected PDA")]
    fn cannot_update_another_price_sources_tick_account() {
        let victim_source_id = AccountId::new([2u8; 32]);
        let victim_account_id =
            compute_current_tick_account_pda(ORACLE_PROGRAM_ID, victim_source_id);

        let mut victim_account = current_tick_account_initialized(500, 1_000);
        victim_account.account_id = victim_account_id;

        update_current_tick(
            victim_account,
            price_source_authorized(), // attacker controls price_source_id = [1u8; 32]
            clock_account_with_timestamp(2_000),
            999,
            ORACLE_PROGRAM_ID,
        );
    }
}
