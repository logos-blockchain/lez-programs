use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ProgramId},
};
use stablecoin_core::{
    verify_redemption_controller_and_get_seed, RedemptionController, CONTROLLER_GAIN_SCALE,
};
use token_core::TokenDefinition;
use twap_oracle_core::OraclePriceAccount;

const CONTROLLER_GAIN_SCALE_I128: i128 = {
    assert!(CONTROLLER_GAIN_SCALE <= i128::MAX as u128);
    CONTROLLER_GAIN_SCALE as i128
};

/// Initialize the redemption-rate feedback controller for one stablecoin/feed pair.
///
/// # Panics
/// - `controller` is already initialized.
/// - `controller.account_id` does not match the stablecoin/feed PDA.
/// - `stablecoin_definition` is uninitialized or not a fungible token definition.
/// - `price_feed` is uninitialized, malformed, stale, or not the stablecoin/collateral feed.
/// - Initial price, gains, or clamp limits do not fit controller math bounds.
#[expect(
    clippy::too_many_arguments,
    reason = "public instruction configuration maps directly to controller parameters"
)]
pub fn initialize_redemption_controller(
    controller: AccountWithMetadata,
    stablecoin_definition: AccountWithMetadata,
    price_feed: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    collateral_definition_id: AccountId,
    initial_redemption_price: u128,
    proportional_gain: u128,
    integral_gain: u128,
    max_integral_error: u128,
    max_redemption_rate: u128,
    max_price_feed_age: u64,
    current_timestamp: u64,
) -> Vec<AccountPostState> {
    assert_eq!(
        controller.account,
        Account::default(),
        "Redemption controller account must be uninitialized"
    );
    assert_ne!(
        stablecoin_definition.account,
        Account::default(),
        "Stablecoin definition account must be initialized"
    );
    assert_ne!(
        price_feed.account,
        Account::default(),
        "Price feed account must be initialized"
    );
    assert_price_value_fits_i128(initial_redemption_price, "Initial redemption price");
    assert_price_value_fits_i128(max_integral_error, "Maximum integral error");
    assert_price_value_fits_i128(max_redemption_rate, "Maximum redemption rate");
    assert_price_value_fits_i128(proportional_gain, "Proportional gain");
    assert_price_value_fits_i128(integral_gain, "Integral gain");
    assert_ne!(
        initial_redemption_price, 0,
        "Initial redemption price must be nonzero"
    );
    assert_ne!(
        max_redemption_rate, 0,
        "Maximum redemption rate must be nonzero"
    );

    let token_definition = TokenDefinition::try_from(&stablecoin_definition.account.data)
        .expect("Stablecoin definition account must hold a valid TokenDefinition");
    assert!(
        matches!(token_definition, TokenDefinition::Fungible { .. }),
        "Stablecoin definition must be fungible"
    );
    assert_live_price_feed(
        &price_feed,
        stablecoin_definition.account_id,
        collateral_definition_id,
        current_timestamp,
        max_price_feed_age,
    );

    let controller_seed = verify_redemption_controller_and_get_seed(
        &controller,
        stablecoin_definition.account_id,
        price_feed.account_id,
        stablecoin_program_id,
    );
    let controller_state = RedemptionController {
        stablecoin_definition_id: stablecoin_definition.account_id,
        collateral_definition_id,
        price_feed_id: price_feed.account_id,
        redemption_price: initial_redemption_price,
        redemption_rate: 0,
        accumulated_error: 0,
        proportional_gain,
        integral_gain,
        max_integral_error,
        max_redemption_rate,
        max_price_feed_age,
        last_update_timestamp: current_timestamp,
    };
    let mut controller_post = controller.account;
    controller_post.data = Data::from(&controller_state);

    vec![
        AccountPostState::new_claimed(
            controller_post,
            nssa_core::program::Claim::Pda(controller_seed),
        ),
        AccountPostState::new(stablecoin_definition.account),
        AccountPostState::new(price_feed.account),
    ]
}

/// Update redemption price and redemption rate from the configured price feed.
///
/// If the configured feed is stale, unavailable, or malformed, the controller state is emitted
/// unchanged. That makes stale-oracle handling an explicit pause instead of a failed update.
///
/// # Panics
/// - `controller` is uninitialized, not owned by this program, malformed, or at the wrong PDA.
/// - `current_timestamp` is older than `RedemptionController.last_update_timestamp`.
pub fn update_redemption_controller(
    controller: AccountWithMetadata,
    price_feed: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    current_timestamp: u64,
) -> Vec<AccountPostState> {
    assert_ne!(
        controller.account,
        Account::default(),
        "Redemption controller account must be initialized"
    );
    assert_eq!(
        controller.account.program_owner, stablecoin_program_id,
        "Redemption controller is not owned by this stablecoin program"
    );

    let controller_data = RedemptionController::try_from(&controller.account.data)
        .expect("Redemption controller account must hold valid controller state");
    verify_redemption_controller_and_get_seed(
        &controller,
        controller_data.stablecoin_definition_id,
        controller_data.price_feed_id,
        stablecoin_program_id,
    );
    assert!(
        current_timestamp >= controller_data.last_update_timestamp,
        "Current timestamp is older than the last controller update"
    );

    let Some(market_price) = live_market_price(&controller_data, &price_feed, current_timestamp)
    else {
        return vec![
            AccountPostState::new(controller.account),
            AccountPostState::new(price_feed.account),
        ];
    };

    let updated_controller =
        compute_next_controller_state(&controller_data, market_price, current_timestamp);
    let mut controller_post = controller.account;
    controller_post.data = Data::from(&updated_controller);

    vec![
        AccountPostState::new(controller_post),
        AccountPostState::new(price_feed.account),
    ]
}

fn live_market_price(
    controller: &RedemptionController,
    price_feed: &AccountWithMetadata,
    current_timestamp: u64,
) -> Option<u128> {
    if price_feed.account_id != controller.price_feed_id || price_feed.account == Account::default()
    {
        return None;
    }

    let price_account = OraclePriceAccount::try_from(&price_feed.account.data).ok()?;
    if price_account.base_asset != controller.stablecoin_definition_id
        || price_account.quote_asset != controller.collateral_definition_id
        || price_account.price == 0
        || price_account.price > i128_max_as_u128()
        || price_account.timestamp > current_timestamp
    {
        return None;
    }

    let age = current_timestamp.checked_sub(price_account.timestamp)?;
    if age > controller.max_price_feed_age {
        return None;
    }

    Some(price_account.price)
}

fn assert_live_price_feed(
    price_feed: &AccountWithMetadata,
    stablecoin_definition_id: AccountId,
    collateral_definition_id: AccountId,
    current_timestamp: u64,
    max_price_feed_age: u64,
) {
    let price_account = OraclePriceAccount::try_from(&price_feed.account.data)
        .expect("Price feed account must hold a valid OraclePriceAccount");
    assert_eq!(
        price_account.base_asset, stablecoin_definition_id,
        "Price feed base asset must be the stablecoin definition"
    );
    assert_eq!(
        price_account.quote_asset, collateral_definition_id,
        "Price feed quote asset must be the collateral definition"
    );
    assert_ne!(price_account.price, 0, "Price feed price must be nonzero");
    assert_price_value_fits_i128(price_account.price, "Price feed price");
    assert!(
        price_account.timestamp <= current_timestamp,
        "Price feed timestamp cannot be in the future"
    );
    let age = current_timestamp
        .checked_sub(price_account.timestamp)
        .expect("Price feed timestamp was checked to be current or older");
    assert!(
        age <= max_price_feed_age,
        "Price feed age exceeds maximum allowed age"
    );
}

fn compute_next_controller_state(
    controller: &RedemptionController,
    market_price: u128,
    current_timestamp: u64,
) -> RedemptionController {
    let elapsed = current_timestamp
        .checked_sub(controller.last_update_timestamp)
        .expect("Current timestamp was checked to be monotonic");
    let redemption_price = apply_redemption_rate(
        controller.redemption_price,
        controller.redemption_rate,
        elapsed,
    );
    let error = price_error(redemption_price, market_price);
    let accumulated_error = clamp_signed(
        controller
            .accumulated_error
            .saturating_add(error.saturating_mul(i128::from(elapsed))),
        controller.max_integral_error,
    );
    let proportional_term = scaled_term(error, controller.proportional_gain);
    let integral_term = scaled_term(accumulated_error, controller.integral_gain);
    let rate_adjustment = proportional_term
        .saturating_add(integral_term)
        .saturating_neg();
    let redemption_rate = clamp_signed(rate_adjustment, controller.max_redemption_rate);

    RedemptionController {
        stablecoin_definition_id: controller.stablecoin_definition_id,
        collateral_definition_id: controller.collateral_definition_id,
        price_feed_id: controller.price_feed_id,
        redemption_price,
        redemption_rate,
        accumulated_error,
        proportional_gain: controller.proportional_gain,
        integral_gain: controller.integral_gain,
        max_integral_error: controller.max_integral_error,
        max_redemption_rate: controller.max_redemption_rate,
        max_price_feed_age: controller.max_price_feed_age,
        last_update_timestamp: current_timestamp,
    }
}

fn apply_redemption_rate(redemption_price: u128, redemption_rate: i128, elapsed: u64) -> u128 {
    let drift = redemption_rate.saturating_mul(i128::from(elapsed));
    if drift >= 0 {
        redemption_price.saturating_add(u128::try_from(drift).unwrap_or(u128::MAX))
    } else {
        let decrease = u128::try_from(drift.saturating_neg()).unwrap_or(u128::MAX);
        redemption_price.saturating_sub(decrease).max(1)
    }
}

fn price_error(redemption_price: u128, market_price: u128) -> i128 {
    if redemption_price >= market_price {
        let difference = redemption_price
            .checked_sub(market_price)
            .expect("checked by branch");
        i128::try_from(difference).expect("Redemption price difference must fit i128")
    } else {
        let difference = market_price
            .checked_sub(redemption_price)
            .expect("checked by branch");
        i128::try_from(difference)
            .expect("Market price difference must fit i128")
            .saturating_neg()
    }
}

fn scaled_term(value: i128, gain: u128) -> i128 {
    let gain = i128::try_from(gain).expect("Controller gain must fit i128");
    value
        .saturating_mul(gain)
        .checked_div(CONTROLLER_GAIN_SCALE_I128)
        .expect("Controller gain scale must be nonzero")
}

fn clamp_signed(value: i128, max_abs: u128) -> i128 {
    let max_abs = i128::try_from(max_abs).expect("Controller clamp must fit i128");
    value.clamp(max_abs.saturating_neg(), max_abs)
}

fn assert_price_value_fits_i128(value: u128, label: &str) {
    assert!(value <= i128_max_as_u128(), "{label} must fit i128");
}

fn i128_max_as_u128() -> u128 {
    u128::try_from(i128::MAX).expect("i128::MAX must fit u128")
}

#[cfg(test)]
mod tests {
    use nssa_core::{
        account::Nonce,
        program::{Claim, PdaSeed},
    };
    use stablecoin_core::{
        compute_redemption_controller_pda, compute_redemption_controller_pda_seed,
        CONTROLLER_GAIN_SCALE,
    };

    use super::*;

    const STABLECOIN_PROGRAM_ID: ProgramId = [3u32; 8];
    const TOKEN_PROGRAM_ID: ProgramId = [2u32; 8];
    const ORACLE_PROGRAM_ID: ProgramId = [4u32; 8];

    fn stablecoin_definition_id() -> AccountId {
        AccountId::new([1; 32])
    }

    fn collateral_definition_id() -> AccountId {
        AccountId::new([2; 32])
    }

    fn price_feed_id() -> AccountId {
        AccountId::new([3; 32])
    }

    fn oracle_source_id() -> AccountId {
        AccountId::new([15; 32])
    }

    fn controller_id() -> AccountId {
        compute_redemption_controller_pda(
            STABLECOIN_PROGRAM_ID,
            stablecoin_definition_id(),
            price_feed_id(),
        )
    }

    fn controller_seed() -> PdaSeed {
        compute_redemption_controller_pda_seed(stablecoin_definition_id(), price_feed_id())
    }

    fn stablecoin_definition_account() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0,
                data: Data::from(&TokenDefinition::Fungible {
                    name: "DAI".to_owned(),
                    total_supply: 1_000_000,
                    metadata_id: None,
                    authority: None,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: stablecoin_definition_id(),
        }
    }

    fn price_feed_account(price: u128, timestamp: u64) -> AccountWithMetadata {
        price_feed_account_with_owner(price, timestamp, ORACLE_PROGRAM_ID)
    }

    fn price_feed_account_with_owner(
        price: u128,
        timestamp: u64,
        program_owner: ProgramId,
    ) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner,
                balance: 0,
                data: Data::from(&OraclePriceAccount {
                    base_asset: stablecoin_definition_id(),
                    quote_asset: collateral_definition_id(),
                    price,
                    timestamp,
                    source_id: oracle_source_id(),
                    confidence_interval: 0,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: price_feed_id(),
        }
    }

    fn uninit_controller_account() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: controller_id(),
        }
    }

    fn controller_account(controller: &RedemptionController) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: STABLECOIN_PROGRAM_ID,
                balance: 0,
                data: Data::from(controller),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: controller_id(),
        }
    }

    fn controller_state() -> RedemptionController {
        RedemptionController {
            stablecoin_definition_id: stablecoin_definition_id(),
            collateral_definition_id: collateral_definition_id(),
            price_feed_id: price_feed_id(),
            redemption_price: 1_000,
            redemption_rate: 0,
            accumulated_error: 0,
            proportional_gain: CONTROLLER_GAIN_SCALE,
            integral_gain: 0,
            max_integral_error: 1_000,
            max_redemption_rate: 500,
            max_price_feed_age: 10,
            last_update_timestamp: 100,
        }
    }

    #[test]
    fn initialize_redemption_controller_claims_pda_and_stores_config() {
        let post_states = initialize_redemption_controller(
            uninit_controller_account(),
            stablecoin_definition_account(),
            price_feed_account(1_000, 100),
            STABLECOIN_PROGRAM_ID,
            collateral_definition_id(),
            1_000,
            CONTROLLER_GAIN_SCALE,
            0,
            1_000,
            500,
            10,
            100,
        );

        assert_eq!(post_states.len(), 3);
        assert_eq!(
            post_states[0].required_claim(),
            Some(Claim::Pda(controller_seed()))
        );
        let controller =
            RedemptionController::try_from(&post_states[0].account().data).expect("valid state");
        assert_eq!(
            controller.stablecoin_definition_id,
            stablecoin_definition_id()
        );
        assert_eq!(
            controller.collateral_definition_id,
            collateral_definition_id()
        );
        assert_eq!(controller.price_feed_id, price_feed_id());
        assert_eq!(controller.redemption_price, 1_000);
        assert_eq!(controller.redemption_rate, 0);
        assert_eq!(controller.last_update_timestamp, 100);
    }

    #[test]
    fn update_redemption_controller_uses_live_price_feed() {
        let post_states = update_redemption_controller(
            controller_account(&controller_state()),
            price_feed_account(900, 100),
            STABLECOIN_PROGRAM_ID,
            100,
        );

        assert_eq!(post_states.len(), 2);
        let controller =
            RedemptionController::try_from(&post_states[0].account().data).expect("valid state");
        assert_eq!(controller.redemption_rate, -100);
        assert_eq!(controller.last_update_timestamp, 100);
    }

    #[test]
    fn update_redemption_controller_accepts_matching_feed_from_any_program_owner() {
        let post_states = update_redemption_controller(
            controller_account(&controller_state()),
            price_feed_account_with_owner(900, 100, [7u32; 8]),
            STABLECOIN_PROGRAM_ID,
            100,
        );

        let controller =
            RedemptionController::try_from(&post_states[0].account().data).expect("valid state");
        assert_eq!(controller.redemption_rate, -100);
    }

    #[test]
    fn update_redemption_controller_pauses_when_price_feed_is_stale() {
        let controller = controller_state();
        let post_states = update_redemption_controller(
            controller_account(&controller),
            price_feed_account(900, 100),
            STABLECOIN_PROGRAM_ID,
            111,
        );

        let updated =
            RedemptionController::try_from(&post_states[0].account().data).expect("valid state");
        assert_eq!(updated, controller);
    }

    #[test]
    fn update_redemption_controller_pauses_when_price_feed_is_unavailable() {
        let controller = controller_state();
        let unavailable_feed = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: price_feed_id(),
        };
        let post_states = update_redemption_controller(
            controller_account(&controller),
            unavailable_feed,
            STABLECOIN_PROGRAM_ID,
            101,
        );

        let updated =
            RedemptionController::try_from(&post_states[0].account().data).expect("valid state");
        assert_eq!(updated, controller);
    }

    #[test]
    fn controller_sets_negative_rate_when_market_price_is_below_redemption_price() {
        let updated = compute_next_controller_state(&controller_state(), 900, 100);

        assert_eq!(updated.redemption_rate, -100);
        assert_eq!(updated.accumulated_error, 0);
        assert_eq!(updated.redemption_price, 1_000);
    }

    #[test]
    fn controller_sets_positive_rate_when_market_price_is_above_redemption_price() {
        let updated = compute_next_controller_state(&controller_state(), 1_100, 100);

        assert_eq!(updated.redemption_rate, 100);
    }

    #[test]
    fn controller_applies_existing_redemption_rate_over_elapsed_time() {
        let mut controller = controller_state();
        controller.redemption_rate = 2;
        controller.proportional_gain = 0;

        let updated = compute_next_controller_state(&controller, 1_010, 105);

        assert_eq!(updated.redemption_price, 1_010);
        assert_eq!(updated.redemption_rate, 0);
        assert_eq!(updated.last_update_timestamp, 105);
    }

    #[test]
    fn controller_clamps_accumulated_error_and_redemption_rate() {
        let mut controller = controller_state();
        controller.accumulated_error = 90;
        controller.proportional_gain = 0;
        controller.integral_gain = CONTROLLER_GAIN_SCALE;
        controller.max_integral_error = 100;
        controller.max_redemption_rate = 80;

        let updated = compute_next_controller_state(&controller, 900, 101);

        assert_eq!(updated.accumulated_error, 100);
        assert_eq!(updated.redemption_rate, -80);
    }
}
