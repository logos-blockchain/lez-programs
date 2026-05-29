use clock_core::ClockAccountData;
use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, Claim, ProgramId},
};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_current_tick_account_pda_seed, CurrentTickAccount,
};

/// Creates and initialises a [`CurrentTickAccount`] for a price source.
///
/// Authorization is implicit in the PDA relationship: the current tick account is derived from
/// `price_source.account_id`, so whoever controls the price source controls this account.
///
/// # Panics
/// Panics if:
/// - `current_tick_account.account_id` does not match
///   `compute_current_tick_account_pda(oracle_program_id, price_source.account_id)`.
/// - `current_tick_account.account` is not the default (already initialised).
/// - `price_source.is_authorized` is false.
pub fn create_current_tick_account(
    current_tick_account: AccountWithMetadata,
    price_source: AccountWithMetadata,
    clock: AccountWithMetadata,
    initial_tick: i32,
    oracle_program_id: ProgramId,
) -> Vec<AccountPostState> {
    let price_source_id = price_source.account_id;
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(oracle_program_id, price_source_id),
        "CreateCurrentTickAccount: current tick account ID does not match expected PDA"
    );
    assert_eq!(
        current_tick_account.account,
        Account::default(),
        "CreateCurrentTickAccount: current tick account must be uninitialized"
    );
    assert!(
        price_source.is_authorized,
        "CreateCurrentTickAccount: price source account must be authorized"
    );

    let clock_data = ClockAccountData::from_bytes(clock.account.data.as_ref());

    let account = CurrentTickAccount {
        tick: initial_tick,
        last_updated: clock_data.timestamp,
    };

    let mut current_tick_account_post = current_tick_account.account.clone();
    current_tick_account_post.data = Data::from(&account);

    vec![
        AccountPostState::new_claimed(
            current_tick_account_post,
            Claim::Pda(compute_current_tick_account_pda_seed(price_source_id)),
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

    fn price_source_id() -> AccountId {
        AccountId::new([1u8; 32])
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

    fn current_tick_account_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_current_tick_account_pda(ORACLE_PROGRAM_ID, price_source_id()),
        }
    }

    // ── happy path ────────────────────────────────────────────────────────────

    #[test]
    fn returns_three_post_states() {
        let post_states = create_current_tick_account(
            current_tick_account_uninit(),
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(post_states.len(), 3);
    }

    #[test]
    fn current_tick_account_post_state_is_pda_claimed() {
        let post_states = create_current_tick_account(
            current_tick_account_uninit(),
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(
            post_states[0].required_claim(),
            Some(Claim::Pda(compute_current_tick_account_pda_seed(
                price_source_id()
            )))
        );
    }

    #[test]
    fn tick_and_timestamp_stored_correctly() {
        let timestamp = 123_456_789u64;
        let initial_tick = -42i32;

        let post_states = create_current_tick_account(
            current_tick_account_uninit(),
            price_source_authorized(),
            clock_account_with_timestamp(timestamp),
            initial_tick,
            ORACLE_PROGRAM_ID,
        );

        let account = CurrentTickAccount::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid CurrentTickAccount");

        assert_eq!(account.tick, initial_tick);
        assert_eq!(account.last_updated, timestamp);
    }

    #[test]
    fn positive_and_negative_initial_ticks_stored_correctly() {
        for tick in [i32::MIN, -1, 0, 1, i32::MAX] {
            let post_states = create_current_tick_account(
                current_tick_account_uninit(),
                price_source_authorized(),
                clock_account_with_timestamp(0),
                tick,
                ORACLE_PROGRAM_ID,
            );
            let account = CurrentTickAccount::try_from(&post_states[0].account().data)
                .expect("post state must contain a valid CurrentTickAccount");
            assert_eq!(account.tick, tick);
        }
    }

    #[test]
    fn price_source_and_clock_post_states_are_unchanged() {
        let price_source = price_source_authorized();
        let clock = clock_account_with_timestamp(42_000);

        let post_states = create_current_tick_account(
            current_tick_account_uninit(),
            price_source.clone(),
            clock.clone(),
            0,
            ORACLE_PROGRAM_ID,
        );

        assert_eq!(*post_states[1].account(), price_source.account);
        assert_eq!(*post_states[2].account(), clock.account);
    }

    #[test]
    fn different_price_sources_produce_distinct_pdas() {
        let other_source_id = AccountId::new([2u8; 32]);
        assert_ne!(
            compute_current_tick_account_pda(ORACLE_PROGRAM_ID, price_source_id()),
            compute_current_tick_account_pda(ORACLE_PROGRAM_ID, other_source_id),
        );
    }

    #[test]
    fn current_tick_account_pda_differs_from_price_observations_pda() {
        use twap_oracle_core::compute_price_observations_pda;
        let window = 24 * 60 * 60 * 1_000u64;
        assert_ne!(
            compute_current_tick_account_pda(ORACLE_PROGRAM_ID, price_source_id()),
            compute_price_observations_pda(ORACLE_PROGRAM_ID, price_source_id(), window),
        );
    }

    // ── precondition violations ───────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "current tick account ID does not match expected PDA")]
    fn wrong_account_id_panics() {
        let mut wrong = current_tick_account_uninit();
        wrong.account_id = AccountId::new([0u8; 32]);
        create_current_tick_account(
            wrong,
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "current tick account must be uninitialized")]
    fn already_initialized_account_panics() {
        let mut initialized = current_tick_account_uninit();
        initialized.account.data = Data::try_from(vec![1u8; 10]).expect("fits in Data");
        create_current_tick_account(
            initialized,
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
        create_current_tick_account(
            current_tick_account_uninit(),
            unauthorized,
            clock_account_with_timestamp(0),
            0,
            ORACLE_PROGRAM_ID,
        );
    }

    /// An attacker who controls their own price source cannot register a current tick account
    /// that claims to be derived from a different (victim's) price source.
    #[test]
    #[should_panic(expected = "current tick account ID does not match expected PDA")]
    fn cannot_register_for_another_price_source() {
        let victim_source_id = AccountId::new([2u8; 32]);
        let victim_pda = compute_current_tick_account_pda(ORACLE_PROGRAM_ID, victim_source_id);
        let mut attacker_account = current_tick_account_uninit();
        attacker_account.account_id = victim_pda;
        create_current_tick_account(
            attacker_account,
            price_source_authorized(),
            clock_account_with_timestamp(0),
            0,
            ORACLE_PROGRAM_ID,
        );
    }
}
