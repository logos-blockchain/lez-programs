use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, Claim, ProgramId},
};
use twap_oracle_core::{
    compute_oracle_price_account_pda, compute_oracle_price_account_pda_seed, OraclePriceAccount,
};

/// Creates and initialises an [`OraclePriceAccount`] for a price source account and time window.
///
/// The account is initialised with a non-zero `initial_price` and the timestamp read from the
/// canonical 1-block LEZ clock. Both must be non-zero: a zero price or zero timestamp is the
/// sentinel consumers treat as "no valid price", so an account must never be created in that
/// state. `confidence_interval` starts at zero (the source may not provide one).
///
/// `initial_price` is a `Q64.64` fixed-point value: the real price is `initial_price / 2^64`, so
/// `1.0` is `1 << 64`. The non-zero check rejects the sentinel but cannot validate scale — a
/// caller is responsible for supplying a correctly-scaled fixed-point price, not a plain integer.
///
/// The timestamp is taken from `clock`, which must be [`CLOCK_01_PROGRAM_ACCOUNT_ID`]; it is never
/// caller-supplied, so it cannot be forged.
///
/// Authorization is implicit in the PDA relationship: the oracle price account is derived from
/// `price_source.account_id` and `window_duration`, so whoever controls the price source
/// controls this account.
///
/// # Panics
/// Panics if:
/// - `oracle_price_account.account_id` does not match
///   `compute_oracle_price_account_pda(oracle_program_id, price_source.account_id,
///   window_duration)`.
/// - `oracle_price_account.account` is not the default (already initialised).
/// - `price_source.is_authorized` is false (caller does not control the price source account).
/// - `clock.account_id` is not [`CLOCK_01_PROGRAM_ACCOUNT_ID`].
/// - `initial_price` is zero.
/// - the clock timestamp is zero.
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit account inputs alongside the asset pair, initial price, and window"
)]
pub fn create_oracle_price_account(
    oracle_price_account: AccountWithMetadata,
    price_source: AccountWithMetadata,
    clock: AccountWithMetadata,
    base_asset: AccountId,
    quote_asset: AccountId,
    initial_price: u128,
    window_duration: u64,
    oracle_program_id: ProgramId,
) -> Vec<AccountPostState> {
    let price_source_id = price_source.account_id;
    assert_eq!(
        oracle_price_account.account_id,
        compute_oracle_price_account_pda(oracle_program_id, price_source_id, window_duration),
        "CreateOraclePriceAccount: oracle price account ID does not match expected PDA"
    );
    assert_eq!(
        oracle_price_account.account,
        Account::default(),
        "CreateOraclePriceAccount: oracle price account must be uninitialized"
    );
    assert!(
        price_source.is_authorized,
        "CreateOraclePriceAccount: price source account must be authorized (caller must control it via a PDA)"
    );
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "CreateOraclePriceAccount: clock account must be the canonical 1-block LEZ clock account"
    );

    let timestamp = ClockAccountData::from_bytes(clock.account.data.as_ref()).timestamp;

    assert!(
        initial_price != 0,
        "CreateOraclePriceAccount: initial price must be non-zero"
    );
    assert!(
        timestamp != 0,
        "CreateOraclePriceAccount: clock timestamp must be non-zero"
    );

    let account = OraclePriceAccount {
        base_asset,
        quote_asset,
        price: initial_price,
        timestamp,
        source_id: price_source_id,
        confidence_interval: 0,
    };

    let mut oracle_price_account_post = oracle_price_account.account.clone();
    oracle_price_account_post.data = Data::from(&account);

    vec![
        AccountPostState::new_claimed(
            oracle_price_account_post,
            Claim::Pda(compute_oracle_price_account_pda_seed(
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
    use nssa_core::account::Nonce;

    use super::*;

    const ORACLE_PROGRAM_ID: ProgramId = [77u32; 8];
    const CLOCK_PROGRAM_ID: ProgramId = [88u32; 8];
    /// 24-hour window in milliseconds.
    const WINDOW_24H: u64 = 24 * 60 * 60 * 1_000;
    /// A representative non-zero initialisation price. Prices are `Q64.64` fixed point
    /// (`price / 2^64`), so this is `1.0`.
    const INITIAL_PRICE: u128 = 1u128 << 64;
    /// A representative non-zero clock timestamp (milliseconds since the Unix epoch).
    const TIMESTAMP: u64 = 1_700_000_000_000;

    fn price_source_id() -> AccountId {
        AccountId::new([1u8; 32])
    }

    fn base_asset() -> AccountId {
        AccountId::new([10u8; 32])
    }

    fn quote_asset() -> AccountId {
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

    fn clock_account(timestamp: u64) -> AccountWithMetadata {
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

    fn oracle_price_account_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
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
        let post_states = create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source_authorized(),
            clock_account(TIMESTAMP),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(post_states.len(), 3);
    }

    #[test]
    fn oracle_price_account_post_state_is_pda_claimed() {
        let post_states = create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source_authorized(),
            clock_account(TIMESTAMP),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(
            post_states[0].required_claim(),
            Some(Claim::Pda(compute_oracle_price_account_pda_seed(
                price_source_id(),
                WINDOW_24H,
            )))
        );
    }

    #[test]
    fn price_source_and_clock_post_states_are_unchanged() {
        let price_source = price_source_authorized();
        let clock = clock_account(TIMESTAMP);
        let post_states = create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source.clone(),
            clock.clone(),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(*post_states[1].account(), price_source.account);
        assert_eq!(*post_states[2].account(), clock.account);
    }

    #[test]
    fn account_initialised_with_price_and_clock_timestamp() {
        let post_states = create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source_authorized(),
            clock_account(TIMESTAMP),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid OraclePriceAccount");
        assert_eq!(account.price, INITIAL_PRICE);
        assert_eq!(account.timestamp, TIMESTAMP);
        assert_eq!(account.confidence_interval, 0);
    }

    #[test]
    fn assets_and_source_id_stored_correctly() {
        let post_states = create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source_authorized(),
            clock_account(TIMESTAMP),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid OraclePriceAccount");
        assert_eq!(account.base_asset, base_asset());
        assert_eq!(account.quote_asset, quote_asset());
        assert_eq!(account.source_id, price_source_id());
    }

    /// `source_id` must always equal the price source's `account_id`, regardless of which
    /// price source is used. This test uses a distinct source ID to make the invariant explicit.
    #[test]
    fn source_id_equals_price_source_account_id() {
        let other_source_id = AccountId::new([99u8; 32]);
        let other_source = AccountWithMetadata {
            account: Account {
                program_owner: [42u32; 8],
                balance: 0,
                data: Data::default(),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: other_source_id,
        };
        let other_price_account = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_oracle_price_account_pda(
                ORACLE_PROGRAM_ID,
                other_source_id,
                WINDOW_24H,
            ),
        };
        let post_states = create_oracle_price_account(
            other_price_account,
            other_source,
            clock_account(TIMESTAMP),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid OraclePriceAccount");
        assert_eq!(account.source_id, other_source_id);
    }

    #[test]
    fn different_price_sources_produce_distinct_pdas() {
        let other_source_id = AccountId::new([2u8; 32]);
        assert_ne!(
            compute_oracle_price_account_pda(ORACLE_PROGRAM_ID, price_source_id(), WINDOW_24H),
            compute_oracle_price_account_pda(ORACLE_PROGRAM_ID, other_source_id, WINDOW_24H),
        );
    }

    #[test]
    fn different_windows_produce_distinct_pdas() {
        let window_7d = 7 * 24 * 60 * 60 * 1_000u64;
        assert_ne!(
            compute_oracle_price_account_pda(ORACLE_PROGRAM_ID, price_source_id(), WINDOW_24H),
            compute_oracle_price_account_pda(ORACLE_PROGRAM_ID, price_source_id(), window_7d),
        );
    }

    #[test]
    fn oracle_price_account_pda_differs_from_price_observations_pda() {
        use twap_oracle_core::compute_price_observations_pda;
        assert_ne!(
            compute_oracle_price_account_pda(ORACLE_PROGRAM_ID, price_source_id(), WINDOW_24H),
            compute_price_observations_pda(ORACLE_PROGRAM_ID, price_source_id(), WINDOW_24H),
        );
    }

    /// A plain wallet account (no program owner, no data) can act as the price source just as
    /// well as a program-owned PDA. Authorization is conveyed via `is_authorized = true`
    /// regardless of account type.
    #[test]
    fn wallet_account_as_price_source_works() {
        let wallet_id = AccountId::new([55u8; 32]);
        let wallet = AccountWithMetadata {
            account: Account {
                program_owner: [0u32; 8],
                balance: 1_000,
                data: Data::default(),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: wallet_id,
        };
        let price_account = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_oracle_price_account_pda(ORACLE_PROGRAM_ID, wallet_id, WINDOW_24H),
        };
        let post_states = create_oracle_price_account(
            price_account,
            wallet,
            clock_account(TIMESTAMP),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid OraclePriceAccount");
        assert_eq!(account.source_id, wallet_id);
        assert_eq!(account.base_asset, base_asset());
        assert_eq!(account.quote_asset, quote_asset());
    }

    // ── precondition violations ───────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "oracle price account ID does not match expected PDA")]
    fn wrong_oracle_price_account_id_panics() {
        let mut wrong = oracle_price_account_uninit();
        wrong.account_id = AccountId::new([0u8; 32]);
        create_oracle_price_account(
            wrong,
            price_source_authorized(),
            clock_account(TIMESTAMP),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "oracle price account must be uninitialized")]
    fn already_initialized_oracle_price_account_panics() {
        let mut initialized = oracle_price_account_uninit();
        initialized.account.data = Data::try_from(vec![1u8; 10]).expect("fits in Data");
        create_oracle_price_account(
            initialized,
            price_source_authorized(),
            clock_account(TIMESTAMP),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "price source account must be authorized")]
    fn unauthorized_price_source_panics() {
        let mut unauthorized = price_source_authorized();
        unauthorized.is_authorized = false;
        create_oracle_price_account(
            oracle_price_account_uninit(),
            unauthorized,
            clock_account(TIMESTAMP),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    /// An attacker who controls their own price source cannot register an oracle price account
    /// that claims to be derived from a different (victim's) price source.
    #[test]
    #[should_panic(expected = "oracle price account ID does not match expected PDA")]
    fn cannot_register_price_account_for_another_price_source() {
        let victim_source_id = AccountId::new([2u8; 32]);
        let victim_pda =
            compute_oracle_price_account_pda(ORACLE_PROGRAM_ID, victim_source_id, WINDOW_24H);
        let mut attacker_account = oracle_price_account_uninit();
        attacker_account.account_id = victim_pda;
        create_oracle_price_account(
            attacker_account,
            price_source_authorized(),
            clock_account(TIMESTAMP),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    /// A zero initial price is the consumer-side "no valid price" sentinel and must never be
    /// written at creation time.
    #[test]
    #[should_panic(expected = "initial price must be non-zero")]
    fn zero_initial_price_panics() {
        create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source_authorized(),
            clock_account(TIMESTAMP),
            base_asset(),
            quote_asset(),
            0,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    /// A zero timestamp is the consumer-side "no valid price" sentinel and must never be written
    /// at creation time, even when the clock account itself reports zero.
    #[test]
    #[should_panic(expected = "clock timestamp must be non-zero")]
    fn zero_clock_timestamp_panics() {
        create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source_authorized(),
            clock_account(0),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    /// The coarser-cadence clock accounts (10-block, 50-block) are rejected: the oracle must read
    /// the most fine-grained 1-block clock.
    #[test]
    #[should_panic(expected = "clock account must be the canonical 1-block LEZ clock account")]
    fn non_canonical_clock_account_id_panics() {
        use clock_core::CLOCK_10_PROGRAM_ACCOUNT_ID;
        create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source_authorized(),
            clock_account_with_id(TIMESTAMP, CLOCK_10_PROGRAM_ACCOUNT_ID),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    /// An attacker cannot supply an account they control — even one whose data deserializes as a
    /// valid [`ClockAccountData`] with a forged timestamp — in place of the system clock.
    #[test]
    #[should_panic(expected = "clock account must be the canonical 1-block LEZ clock account")]
    fn forged_clock_account_panics() {
        create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source_authorized(),
            clock_account_with_id(TIMESTAMP, AccountId::new([7u8; 32])),
            base_asset(),
            quote_asset(),
            INITIAL_PRICE,
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }
}
