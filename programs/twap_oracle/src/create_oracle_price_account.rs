use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, Claim, ProgramId},
};
use twap_oracle_core::{
    compute_oracle_price_account_pda, compute_oracle_price_account_pda_seed, OraclePriceAccount,
};

/// Creates and initialises an [`OraclePriceAccount`] for a price source account and time window.
///
/// The account is initialised with `price = 0`, `timestamp = 0`, and `confidence_interval = 0`.
/// These are populated later by a `PublishPrice` instruction. Consumers must reject accounts
/// whose `timestamp` is zero or stale.
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
pub fn create_oracle_price_account(
    oracle_price_account: AccountWithMetadata,
    price_source: AccountWithMetadata,
    base_asset: AccountId,
    quote_asset: AccountId,
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

    let account = OraclePriceAccount {
        base_asset,
        quote_asset,
        price: 0,
        timestamp: 0,
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
    ]
}

#[cfg(test)]
mod tests {
    use nssa_core::account::Nonce;

    use super::*;

    const ORACLE_PROGRAM_ID: ProgramId = [77u32; 8];
    /// 24-hour window in milliseconds.
    const WINDOW_24H: u64 = 24 * 60 * 60 * 1_000;

    fn price_source_id() -> AccountId {
        AccountId::new([1u8; 32])
    }

    fn base_asset() -> AccountId {
        AccountId::new([10u8; 32])
    }

    fn quote_asset() -> AccountId {
        AccountId::new([11u8; 32])
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
    fn returns_two_post_states() {
        let post_states = create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source_authorized(),
            base_asset(),
            quote_asset(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(post_states.len(), 2);
    }

    #[test]
    fn oracle_price_account_post_state_is_pda_claimed() {
        let post_states = create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source_authorized(),
            base_asset(),
            quote_asset(),
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
    fn price_source_post_state_is_unchanged() {
        let price_source = price_source_authorized();
        let post_states = create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source.clone(),
            base_asset(),
            quote_asset(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(*post_states[1].account(), price_source.account);
    }

    #[test]
    fn account_initialised_with_zero_price_and_timestamp() {
        let post_states = create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source_authorized(),
            base_asset(),
            quote_asset(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let account = OraclePriceAccount::try_from(&post_states[0].account().data)
            .expect("post state must contain a valid OraclePriceAccount");
        assert_eq!(account.price, 0);
        assert_eq!(account.timestamp, 0);
        assert_eq!(account.confidence_interval, 0);
    }

    #[test]
    fn assets_and_source_id_stored_correctly() {
        let post_states = create_oracle_price_account(
            oracle_price_account_uninit(),
            price_source_authorized(),
            base_asset(),
            quote_asset(),
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
            base_asset(),
            quote_asset(),
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
            base_asset(),
            quote_asset(),
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
            base_asset(),
            quote_asset(),
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
            base_asset(),
            quote_asset(),
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
            base_asset(),
            quote_asset(),
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
            base_asset(),
            quote_asset(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }
}
