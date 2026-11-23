//! The stablecoin through its real guest, against the real token program, the real TWAP oracle
//! and LEZ's clock: every conversation of the design, as whole transactions on a `V03State`.
//!
//! [`harness::send`] checks after every stablecoin transaction, successful or refused, that the
//! engine is not left waiting for a reply.

#[path = "stablecoin/harness.rs"]
mod harness;

use clock_core::clock_actor;
use harness::*;
use integration_tests::{account, assert_refused, key, tick};
use lee_core::{account::AccountId, program::TransactionEvent};
use stablecoin_core::{
    event::{DebtRepaid, RedemptionRateUpdated, StabilityFeeAccrued},
    math::compute_repayment,
    Message, FIXED_POINT_ONE,
};
use token_core::TokenDefinition;

/// The surplus and stablecoin balances that move together, read at once.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct Stablecoin {
    surplus: u128,
    supply: u128,
}

fn stablecoin_balances(world: &World) -> Stablecoin {
    Stablecoin {
        surplus: holding_balance(world, surplus_id(world)),
        supply: total_supply(world, definition_id(world)),
    }
}

/// The stablecoin it costs to clear `owner`'s debt at the engine's current accumulator.
fn cost_to_clear(world: &World, owner: AccountId) -> u128 {
    let debt = position(world, owner)
        .expect("the owner has a position")
        .normalized_debt_amount;
    compute_repayment(
        u128::MAX,
        debt,
        engine(world).fee.accumulated_rate_at_last_accrual,
    )
    .amount_to_burn
}

fn debt(world: &World, user: &User) -> u128 {
    position(world, user.id())
        .expect("the user has a position")
        .normalized_debt_amount
}

fn collateral(world: &World, user: &User) -> u128 {
    position(world, user.id())
        .expect("the user has a position")
        .collateral_amount
}

/// An initialized world and a user with `collateral` in a holding.
fn world_with_user(collateral: u128) -> (World, User) {
    let mut world = world();
    initialize(&mut world);
    let user = user(&mut world, 30, collateral);
    (world, user)
}

// ── Initialize ───────────────────────────────────────────────────────────────────

#[test]
fn initialize_creates_the_engine_and_the_stablecoin_token() {
    let mut world = world();
    initialize(&mut world);
    let engine = engine(&world);
    assert_eq!(engine.pending, None);
    assert_eq!(engine.fee.last_accrued_at, T0);
    assert_eq!(engine.redemption.last_updated_at, T0);
    assert_eq!(engine.parameters.token_program_id, world.token);
    assert_eq!(engine.fee.accumulated_rate_at_last_accrual, FIXED_POINT_ONE);
    assert_eq!(engine.fee.total_normalized_debt, 0);
    let parameters = &engine.parameters;
    assert_eq!(parameters.admin_account_id, account(&world.admin));
    assert_eq!(
        parameters.freeze_authority_account_id,
        account(&world.freeze_authority)
    );
    assert_eq!(parameters.stablecoin_definition_id, definition_id(&world));
    assert_eq!(
        parameters.collateral_definition_id,
        world.collateral_definition
    );
    assert_eq!(parameters.market_price_oracle_id, price_account_id(&world));
    assert_eq!(parameters.market_price_oracle_program_id, world.oracle);
    assert_eq!(parameters.minimum_debt_amount, 10);
    assert_eq!(parameters.debt_ceiling, 1_000_000_000);
    assert!(!parameters.is_frozen);
    assert_eq!(
        stablecoin_balances(&world),
        Stablecoin {
            surplus: 0,
            supply: 0
        }
    );
    // The stablecoin definition is its own mint authority, so the engine can mint through it.
    assert_eq!(
        definition(&world, definition_id(&world)),
        TokenDefinition {
            name: "Stable".to_owned(),
            total_supply: 0,
            authority: Some(definition_id(&world)),
        }
    );
    assert_eq!(
        holding(&world, surplus_id(&world)).definition_id,
        definition_id(&world)
    );
    assert_eq!(position(&world, account(&world.admin)), None);
}

#[test]
fn initialize_signed_by_anyone_else_is_refused() {
    let mut world = world();
    let other = key(9);
    let root = stablecoin(&world, account(&other));
    let past_root = initialize_actors(&world);
    let message = Message::Initialize(initialize_args(&world));

    let result = send(&mut world, root, &past_root, &[&other], &message);

    assert_refused(&result, "only the program's own account may initialize it");
    assert_eq!(stablecoin_state(&world, engine_id(&world)), None);
}

#[test]
fn initialize_twice_is_refused() {
    let mut world = world();
    initialize(&mut world);
    let before = engine(&world);
    let definition_before = definition(&world, definition_id(&world));

    let result = try_initialize(&mut world);

    // The program account's first message is the token relay, so the token's `NewDefinition`
    // at the existing definition fails before the engine sees `InitializeEngine`.
    assert_refused(&result, "definition account must be empty");
    assert_eq!(engine(&world), before);
    assert_eq!(definition(&world, definition_id(&world)), definition_before);
}

// ── AccrueStabilityFee ───────────────────────────────────────────────────────────

#[test]
fn anyone_can_accrue_and_the_engine_follows_the_clock() {
    let mut world = world();
    initialize(&mut world);
    tick(&mut world.state, T0 + 60_000);
    accrue(&mut world).expect("the poke succeeds");
    let engine = engine(&world);
    assert_eq!(engine.pending, None);
    assert_eq!(engine.fee.last_accrued_at, T0 + 60_000);
    // The fee is above 1.0 per millisecond, so 60_000 ms of the clock's time compound it.
    assert!(engine.fee.accumulated_rate_at_last_accrual > FIXED_POINT_ONE);
}

#[test]
fn accrue_with_debt_mints_the_fees_to_the_surplus_holding() {
    let (mut world, user) = world_with_user(200_000_000);
    open_position(&mut world, &user, 150_000_000).expect("the position opens");
    generate(&mut world, &user, 100_000_000).expect("the debt is generated");
    let before = stablecoin_balances(&world);
    let rate_before = engine(&world).fee.accumulated_rate_at_last_accrual;
    tick(&mut world.state, T0 + 3_600_000);

    let events = accrue(&mut world).expect("the poke succeeds");

    let accrued = events_of::<StabilityFeeAccrued>(&events);
    let [StabilityFeeAccrued { accrued_fees, .. }] = accrued.as_slice() else {
        panic!("expected one accrual event, got {accrued:?}");
    };
    // An hour at about 100% a year on 100_000_000 is about 7_900.
    assert!(*accrued_fees > 7_000, "{accrued_fees}");
    assert_eq!(
        stablecoin_balances(&world),
        Stablecoin {
            surplus: before.surplus + accrued_fees,
            supply: before.supply + accrued_fees,
        }
    );
    let engine = engine(&world);
    assert!(engine.fee.accumulated_rate_at_last_accrual > rate_before);
    assert_eq!(engine.fee.last_accrued_at, T0 + 3_600_000);
    assert_eq!(engine.fee.total_normalized_debt, 100_000_000);
    assert_eq!(debt(&world, &user), 100_000_000);
}

// ── Positions ────────────────────────────────────────────────────────────────────

#[test]
fn full_position_lifecycle() {
    let (mut world, user) = world_with_user(10_000);
    let vault = vault_id(&world, user.id());

    open_position(&mut world, &user, 1_500).expect("the position opens");
    assert_eq!(collateral(&world, &user), 1_500);
    assert_eq!(debt(&world, &user), 0);
    assert_eq!(holding_balance(&world, vault), 1_500);
    assert_eq!(holding_balance(&world, user.collateral_id()), 8_500);

    generate(&mut world, &user, 1_000).expect("the debt is generated");
    // The accumulator is still 1.0: no time has passed since initialize.
    assert_eq!(debt(&world, &user), 1_000);
    assert_eq!(engine(&world).fee.total_normalized_debt, 1_000);
    assert_eq!(holding_balance(&world, user.stable_id()), 1_000);
    assert_eq!(total_supply(&world, definition_id(&world)), 1_000);

    // A second owner provides the stablecoin to top the holding up for the accrued fee.
    let donor = user_with_debt(&mut world, 40, 100);
    tick(&mut world.state, T0 + 60_000);
    transfer(&mut world, &donor.stable, user.stable_id(), 1).expect("the top-up arrives");
    let before = stablecoin_balances(&world);

    let events = repay(&mut world, &user, 2_000).expect("the debt is repaid");

    // A minute at about 100% a year makes 1_000 of debt cost 1_000.0013: the repayment burns
    // the ceiling of that, not the 2_000 the owner authorized.
    let repaid = events_of::<DebtRepaid>(&events);
    assert_eq!(
        repaid,
        vec![DebtRepaid {
            owner: user.id(),
            amount_burned: 1_001,
            normalized_debt_delta: 1_000,
        }]
    );
    assert_eq!(cost_to_clear(&world, donor.id()), 101);
    let accrued: u128 = events_of::<StabilityFeeAccrued>(&events)
        .iter()
        .map(|event| event.accrued_fees)
        .sum();
    assert_eq!(holding_balance(&world, user.stable_id()), 0);
    // The repayment passes through the surplus holding and is burned there: the surplus moves
    // only by the accrued fees, the supply falls by the burn.
    assert_eq!(
        stablecoin_balances(&world),
        Stablecoin {
            surplus: before.surplus + accrued,
            supply: before.supply + accrued - 1_001,
        }
    );
    assert_eq!(debt(&world, &user), 0);
    assert_eq!(engine(&world).fee.total_normalized_debt, 100);

    withdraw(&mut world, &user, 1_500).expect("the collateral is withdrawn");
    assert_eq!(collateral(&world, &user), 0);
    assert_eq!(holding_balance(&world, vault), 0);
    assert_eq!(holding_balance(&world, user.collateral_id()), 10_000);

    close(&mut world, &user).expect("the position closes");
    assert_eq!(position(&world, user.id()), None);
}

/// A user (keys `byte..byte + 3`) with an open position of 10_000 collateral and `amount` of
/// debt in their stablecoin holding.
fn user_with_debt(world: &mut World, byte: u8, amount: u128) -> User {
    let user = user(world, byte, 10_000);
    open_position(world, &user, 10_000).expect("the position opens");
    generate(world, &user, amount).expect("the debt is generated");
    user
}

#[test]
fn debt_limits_bound_minting_and_partial_repayment() {
    let (mut world, user) = world_with_user(5_000);
    open_position(&mut world, &user, 5_000).expect("the position opens");

    assert_refused(
        &generate(&mut world, &user, 5),
        "Position debt is below minimum_debt_amount",
    );
    assert_refused(
        &generate(&mut world, &user, 1_000_000_001),
        "Global debt ceiling exceeded",
    );

    assert_refused(
        &admin(
            &mut world,
            &Message::SetDebtLimits {
                minimum_debt_amount: 0,
                debt_ceiling: 10,
            },
        ),
        "Debt limits require 0 < minimum_debt_amount <= debt_ceiling",
    );
    admin(
        &mut world,
        &Message::SetDebtLimits {
            minimum_debt_amount: 100,
            debt_ceiling: 2_000,
        },
    )
    .expect("the admin sets the debt limits");
    let parameters = engine(&world).parameters;
    assert_eq!(parameters.minimum_debt_amount, 100);
    assert_eq!(parameters.debt_ceiling, 2_000);

    assert_refused(
        &generate(&mut world, &user, 50),
        "Position debt is below minimum_debt_amount",
    );
    assert_refused(
        &generate(&mut world, &user, 2_001),
        "Global debt ceiling exceeded",
    );
    generate(&mut world, &user, 1_000).expect("a debt within the limits is generated");
    assert_eq!(debt(&world, &user), 1_000);

    // A partial repayment may not leave the position under the floor.
    assert_refused(
        &repay(&mut world, &user, 950),
        "Position debt is below minimum_debt_amount",
    );
    assert_eq!(debt(&world, &user), 1_000);
    repay(&mut world, &user, 900).expect("a repayment leaving the floor succeeds");
    assert_eq!(debt(&world, &user), 100);
    assert_eq!(holding_balance(&world, user.stable_id()), 100);
    assert_eq!(engine(&world).fee.total_normalized_debt, 100);
}

#[test]
fn a_debt_free_position_withdraws_while_frozen() {
    let (mut world, user) = world_with_user(2_000);
    let vault = vault_id(&world, user.id());
    open_position(&mut world, &user, 1_500).expect("the position opens");
    let mut accrued = 0;
    let mut count_fees = |events: &[TransactionEvent]| {
        accrued += events_of::<StabilityFeeAccrued>(events)
            .iter()
            .map(|event| event.accrued_fees)
            .sum::<u128>();
    };
    count_fees(&generate(&mut world, &user, 1_000).expect("the debt is generated"));
    count_fees(&repay(&mut world, &user, 1_000).expect("the debt is repaid"));
    tick(&mut world.state, T0 + 60_000);
    count_fees(&accrue(&mut world).expect("the poke succeeds"));
    freeze(&mut world);
    assert!(engine(&world).parameters.is_frozen);

    withdraw(&mut world, &user, 1_500).expect("a debt-free position withdraws while frozen");
    close(&mut world, &user).expect("the position closes while frozen");

    assert_eq!(holding_balance(&world, vault), 0);
    assert_eq!(holding_balance(&world, user.collateral_id()), 2_000);
    assert_eq!(position(&world, user.id()), None);
    assert_eq!(engine(&world).fee.total_normalized_debt, 0);
    assert_eq!(holding_balance(&world, surplus_id(&world)), accrued);
}

#[test]
fn deposit_then_withdraw_respects_the_ratio() {
    let (mut world, user) = world_with_user(10_000);
    let vault = vault_id(&world, user.id());
    open_position(&mut world, &user, 1_500).expect("the position opens");
    // 1_000 of debt at a redemption price of 1.0 and a ratio of 1.5 needs exactly 1_500.
    generate(&mut world, &user, 1_000).expect("the debt is generated");

    assert_refused(
        &withdraw(&mut world, &user, 1),
        "Position is undercollateralized",
    );
    assert_eq!(collateral(&world, &user), 1_500);

    deposit(&mut world, &user, 300).expect("the deposit succeeds");
    assert_eq!(collateral(&world, &user), 1_800);
    assert_eq!(holding_balance(&world, vault), 1_800);

    withdraw(&mut world, &user, 300).expect("the surplus collateral is withdrawn");
    assert_eq!(collateral(&world, &user), 1_500);
    assert_eq!(holding_balance(&world, vault), 1_500);
    assert_eq!(holding_balance(&world, user.collateral_id()), 8_500);
    assert_refused(
        &withdraw(&mut world, &user, 1),
        "Position is undercollateralized",
    );
}

#[test]
fn freeze_blocks_open_generate_and_indebted_withdraw_but_not_deposit_repay_close() {
    let (mut world, indebted) = world_with_user(10_000);
    open_position(&mut world, &indebted, 3_000).expect("the position opens");
    generate(&mut world, &indebted, 1_000).expect("the debt is generated");
    let newcomer = user(&mut world, 40, 1_000);
    let empty = user(&mut world, 50, 0);
    open_position(&mut world, &empty, 0).expect("an empty position opens");
    freeze(&mut world);

    assert_refused(
        &open_position(&mut world, &newcomer, 500),
        "Protocol is frozen",
    );
    assert_eq!(position(&world, newcomer.id()), None);
    assert_refused(&generate(&mut world, &indebted, 10), "Protocol is frozen");
    assert_refused(&withdraw(&mut world, &indebted, 1), "Protocol is frozen");
    assert_eq!(debt(&world, &indebted), 1_000);
    assert_eq!(collateral(&world, &indebted), 3_000);

    deposit(&mut world, &indebted, 100).expect("a deposit succeeds while frozen");
    assert_eq!(collateral(&world, &indebted), 3_100);
    repay(&mut world, &indebted, 500).expect("a repayment succeeds while frozen");
    assert_eq!(debt(&world, &indebted), 500);
    close(&mut world, &empty).expect("a close succeeds while frozen");
    assert_eq!(position(&world, empty.id()), None);

    unfreeze(&mut world);
    generate(&mut world, &indebted, 10).expect("generating resumes after unfreeze");
    assert_eq!(debt(&world, &indebted), 510);
}

#[test]
fn only_the_owner_moves_a_position() {
    let (mut world, user) = world_with_user(10_000);
    open_position(&mut world, &user, 1_500).expect("the position opens");
    let before = position(&world, user.id());
    let other = key(9);
    let root = stablecoin(&world, user.id());
    let past_root = [
        stablecoin(&world, engine_id(&world)),
        clock_actor(),
        token(&world, vault_id(&world, user.id())),
        token(&world, account(&other)),
    ];
    let message = Message::WithdrawCollateral {
        to: account(&other),
        amount: 1_500,
    };

    let signed_by_another = send(&mut world, root, &past_root, &[&other], &message);
    let unsigned = send(&mut world, root, &past_root, &[], &message);

    assert_refused(&signed_by_another, "only the account's owner may send this");
    assert_refused(&unsigned, "only the account's owner may send this");
    assert_eq!(position(&world, user.id()), before);
    assert_eq!(holding_balance(&world, vault_id(&world, user.id())), 1_500);
}

#[test]
fn a_donation_to_the_vault_is_ignored() {
    let (mut world, user) = world_with_user(10_000);
    let vault = vault_id(&world, user.id());
    open_position(&mut world, &user, 1_500).expect("the position opens");

    transfer(&mut world, &user.collateral, vault, 500).expect("the donation arrives");
    assert_eq!(holding_balance(&world, vault), 2_000);
    assert_eq!(collateral(&world, &user), 1_500);

    withdraw(&mut world, &user, 1_500).expect("the stored collateral is withdrawn");
    close(&mut world, &user).expect("the position closes");
    assert_eq!(position(&world, user.id()), None);
    // The donation stays in the vault, stranded.
    assert_eq!(holding_balance(&world, vault), 500);
}

#[test]
fn a_deposit_from_a_holding_of_another_token_is_refused() {
    let (mut world, user) = world_with_user(10_000);
    open_position(&mut world, &user, 1_500).expect("the position opens");
    let other_holding = key(61);
    create_token(&mut world, &key(60), "Other", 1_000, &other_holding);

    let result = deposit_from(&mut world, &user, &other_holding, 100);

    // The vault is a collateral holding, so the token refuses to credit it with another token.
    assert_refused(&result, "recipient holds another token");
    assert_eq!(collateral(&world, &user), 1_500);
    assert_eq!(holding_balance(&world, account(&other_holding)), 1_000);
    assert_eq!(holding_balance(&world, vault_id(&world, user.id())), 1_500);
}

// ── Debt ─────────────────────────────────────────────────────────────────────────

#[test]
fn replies_reach_only_the_requesting_position() {
    let (mut world, first) = world_with_user(10_000);
    let second = user(&mut world, 40, 10_000);
    open_position(&mut world, &first, 1_500).expect("the first position opens");
    open_position(&mut world, &second, 1_500).expect("the second position opens");
    let second_before = position(&world, second.id());

    generate(&mut world, &second, 700).expect("the second owner generates debt");

    assert_eq!(debt(&world, &second), 700);
    assert_eq!(holding_balance(&world, second.stable_id()), 700);
    assert_eq!(debt(&world, &first), 0);
    assert_eq!(holding_balance(&world, first.stable_id()), 0);
    assert_eq!(
        position(&world, second.id()).map(|p| p.collateral_amount),
        second_before.map(|p| p.collateral_amount)
    );
    assert_eq!(engine(&world).fee.total_normalized_debt, 700);

    generate(&mut world, &first, 300).expect("the first owner generates debt");
    assert_eq!(debt(&world, &first), 300);
    assert_eq!(holding_balance(&world, first.stable_id()), 300);
    assert_eq!(debt(&world, &second), 700);
    assert_eq!(holding_balance(&world, second.stable_id()), 700);
}

#[test]
fn a_zero_generate_and_a_zero_repay_change_nothing() {
    let (mut world, user) = world_with_user(10_000);
    open_position(&mut world, &user, 1_500).expect("the position opens");
    generate(&mut world, &user, 0).expect("a zero generate on a debt-free position succeeds");
    assert_eq!(debt(&world, &user), 0);
    generate(&mut world, &user, 1_000).expect("the debt is generated");
    let snapshot = |world: &World| {
        (
            position(world, user.id()),
            engine(world).fee,
            holding_balance(world, user.stable_id()),
            stablecoin_balances(world),
        )
    };
    let before = snapshot(&world);

    // The token accepts a zero mint, a zero transfer and a zero burn, so the engine sends them.
    generate(&mut world, &user, 0).expect("a zero generate succeeds");
    assert_eq!(snapshot(&world), before);
    repay(&mut world, &user, 0).expect("a zero repay succeeds");
    assert_eq!(snapshot(&world), before);
}

#[test]
fn repay_with_a_holding_of_another_token_is_refused() {
    let (mut world, user) = world_with_user(10_000);
    open_position(&mut world, &user, 1_500).expect("the position opens");
    generate(&mut world, &user, 1_000).expect("the debt is generated");
    let collateral_before = holding_balance(&world, user.collateral_id());
    let stablecoin_before = stablecoin_balances(&world);

    // The owner signs for their collateral holding and names it as the stablecoin to burn.
    let result = repay_from(&mut world, &user, &user.collateral, 500);

    // The repayment is paid into the surplus holding first, a stablecoin holding, so the
    // token refuses the collateral.
    assert_refused(&result, "recipient holds another token");
    assert_eq!(debt(&world, &user), 1_000);
    assert_eq!(engine(&world).fee.total_normalized_debt, 1_000);
    assert_eq!(
        holding_balance(&world, user.collateral_id()),
        collateral_before
    );
    assert_eq!(
        total_supply(&world, world.collateral_definition),
        COLLATERAL_SUPPLY
    );
    assert_eq!(stablecoin_balances(&world), stablecoin_before);
}

// ── Admin ────────────────────────────────────────────────────────────────────────

#[test]
fn admin_retunes_parameters_then_rotates_the_role() {
    let mut world = world();
    initialize(&mut world);

    admin(
        &mut world,
        &Message::SetMinimumCollateralizationRatio {
            new_ratio: FIXED_POINT_ONE * 2,
        },
    )
    .expect("the ratio changes");
    admin(
        &mut world,
        &Message::SetControllerGains {
            new_proportional_gain: 3,
            new_integral_gain: 4,
        },
    )
    .expect("the gains change");
    admin(
        &mut world,
        &Message::SetTimingParameters {
            minimum_milliseconds_between_rate_updates: 2_000,
            maximum_oracle_price_age_milliseconds: 20_000,
        },
    )
    .expect("the timings change");
    let elsewhere = AccountId::new([0xC1; 32]);
    let oracle = world.oracle;
    admin(
        &mut world,
        &Message::SetMarketPriceOracle {
            oracle_account_id: elsewhere,
            oracle_program_id: oracle,
        },
    )
    .expect("the oracle changes");
    assert_eq!(engine(&world).parameters.market_price_oracle_id, elsewhere);
    let price_account = price_account_id(&world);
    admin(
        &mut world,
        &Message::SetMarketPriceOracle {
            oracle_account_id: price_account,
            oracle_program_id: oracle,
        },
    )
    .expect("the oracle changes back");
    admin(
        &mut world,
        &Message::SetDebtLimits {
            minimum_debt_amount: 20,
            debt_ceiling: 5_000,
        },
    )
    .expect("the debt limits change");
    tick(&mut world.state, T0 + 10_000);
    admin(
        &mut world,
        &Message::SetStabilityFeePerMillisecond {
            new_rate: FIXED_POINT_ONE,
        },
    )
    .expect("the fee changes");
    tick(&mut world.state, T0 + 20_000);
    admin(&mut world, &Message::ResetRedemptionRateController).expect("the controller is reset");

    let engine_after = engine(&world);
    let parameters = &engine_after.parameters;
    assert_eq!(
        parameters.minimum_collateralization_ratio,
        FIXED_POINT_ONE * 2
    );
    assert_eq!(parameters.controller_proportional_gain, 3);
    assert_eq!(parameters.controller_integral_gain, 4);
    assert_eq!(parameters.minimum_milliseconds_between_rate_updates, 2_000);
    assert_eq!(parameters.maximum_oracle_price_age_milliseconds, 20_000);
    assert_eq!(parameters.market_price_oracle_id, price_account);
    assert_eq!(parameters.minimum_debt_amount, 20);
    assert_eq!(parameters.debt_ceiling, 5_000);
    assert_eq!(parameters.stability_fee_per_millisecond, FIXED_POINT_ONE);
    // The fee setter accrued at the old rate up to its own time.
    assert_eq!(engine_after.fee.last_accrued_at, T0 + 10_000);
    assert_eq!(engine_after.redemption.last_updated_at, T0 + 20_000);
    assert_eq!(
        engine_after.redemption.redemption_rate_per_millisecond,
        FIXED_POINT_ONE
    );
    assert_eq!(engine_after.redemption.controller_integral_term, 0);

    // The freeze authority may not retune; the admin may not freeze.
    let freeze_authority = world.freeze_authority.clone();
    assert_refused(
        &seat(
            &mut world,
            &freeze_authority,
            &Message::SetControllerGains {
                new_proportional_gain: 5,
                new_integral_gain: 5,
            },
        ),
        "Signer is not the protocol's admin",
    );
    assert_refused(
        &admin(&mut world, &Message::Freeze),
        "Signer is not the protocol's freeze authority",
    );

    let new_freeze_authority = key(71);
    admin(
        &mut world,
        &Message::SetFreezeAuthority {
            new_freeze_authority_account_id: account(&new_freeze_authority),
        },
    )
    .expect("the freeze authority rotates");
    assert_refused(
        &seat(&mut world, &freeze_authority, &Message::Freeze),
        "Signer is not the protocol's freeze authority",
    );
    seat(&mut world, &new_freeze_authority, &Message::Freeze)
        .expect("the new freeze authority freezes");
    assert!(engine(&world).parameters.is_frozen);

    let new_admin = key(70);
    admin(
        &mut world,
        &Message::SetAdmin {
            new_admin_account_id: account(&new_admin),
        },
    )
    .expect("the admin rotates");
    assert_eq!(
        engine(&world).parameters.admin_account_id,
        account(&new_admin)
    );
    let retune = Message::SetControllerGains {
        new_proportional_gain: 6,
        new_integral_gain: 7,
    };
    assert_refused(
        &admin(&mut world, &retune),
        "Signer is not the protocol's admin",
    );
    seat(&mut world, &new_admin, &retune).expect("the new admin retunes");
    assert_eq!(engine(&world).parameters.controller_proportional_gain, 6);
}

// ── The oracle ───────────────────────────────────────────────────────────────────

#[test]
fn update_redemption_rate_follows_the_oracle() {
    let mut world = world();
    initialize(&mut world);
    admin(
        &mut world,
        &Message::SetControllerGains {
            new_proportional_gain: 10_i128.pow(24),
            new_integral_gain: 0,
        },
    )
    .expect("the gains change");
    tick(&mut world.state, T0 + 2_000);
    // The market trades the stablecoin at 2.0 collateral, above its redemption price of 1.0.
    publish_price(&mut world, 2 * ORACLE_ONE);
    assert_eq!(oracle_price(&world).timestamp, T0 + 2_000);

    let events = update_redemption_rate(&mut world).expect("the update succeeds");

    let redemption = engine(&world).redemption;
    assert_eq!(redemption.last_updated_at, T0 + 2_000);
    assert!(
        redemption.redemption_rate_per_millisecond < FIXED_POINT_ONE,
        "the redemption rate falls toward the market: {redemption:?}"
    );
    assert_eq!(
        events_of::<RedemptionRateUpdated>(&events),
        vec![RedemptionRateUpdated {
            redemption_price: redemption.redemption_price_at_last_update,
            redemption_rate_per_millisecond: redemption.redemption_rate_per_millisecond,
            now: T0 + 2_000,
        }]
    );
}

#[test]
fn launches_before_the_market_price_oracle_exists() {
    let mut world = world();
    // The engine is bound to the price account's address before the account exists.
    initialize(&mut world);
    assert!(world
        .state
        .get_account_by_id(price_account_id(&world))
        .data
        .actor_state(world.oracle)
        .is_empty());
    let redemption_before = engine(&world).redemption;
    tick(&mut world.state, T0 + 2_000);

    refresh(&mut world, false).expect("a refresh without the oracle succeeds");
    assert_eq!(engine(&world).fee.last_accrued_at, T0 + 2_000);
    let result = update_redemption_rate(&mut world);
    assert_price_account_missing(&world, &result);
    let result = refresh(&mut world, true);
    assert_price_account_missing(&world, &result);
    assert_eq!(engine(&world).redemption, redemption_before);

    publish_price(&mut world, 2 * ORACLE_ONE);
    update_redemption_rate(&mut world).expect("the update succeeds");
    assert_eq!(engine(&world).redemption.last_updated_at, T0 + 2_000);
    tick(&mut world.state, T0 + 3_000);
    refresh(&mut world, true).expect("the refresh succeeds");
    let engine = engine(&world);
    assert_eq!(engine.redemption.last_updated_at, T0 + 3_000);
    assert_eq!(engine.fee.last_accrued_at, T0 + 3_000);
}

/// Panics unless `result` failed because the oracle's price account does not exist yet: LEZ
/// does not admit a message to an empty public PDA actor nobody grants the seed for, so the
/// oracle never runs and the whole transaction fails.
fn assert_price_account_missing<T: std::fmt::Debug>(
    world: &World,
    result: &Result<T, lee::error::LeeError>,
) {
    let expected = format!(
        "UnadmittedPublicActor {{ actor: {:?} }}",
        price_actor(world)
    );
    assert!(
        format!("{result:?}").contains(&expected),
        "expected {expected}, got {result:?}"
    );
}

/// A world whose oracle published at `T0 + 2_000`, the clock just past the price's maximum age.
fn world_with_a_stale_price() -> World {
    let mut world = world();
    initialize(&mut world);
    tick(&mut world.state, T0 + 2_000);
    publish_price(&mut world, 2 * ORACLE_ONE);
    tick(&mut world.state, T0 + 2_000 + 10_001);
    world
}

#[test]
fn refresh_skips_a_stale_price() {
    let mut world = world_with_a_stale_price();
    let redemption_before = engine(&world).redemption;

    refresh(&mut world, true).expect("a refresh with a stale price succeeds");

    let engine = engine(&world);
    assert_eq!(engine.redemption, redemption_before);
    assert_eq!(engine.fee.last_accrued_at, T0 + 12_001);
}

#[test]
fn update_refuses_a_stale_price() {
    let mut world = world_with_a_stale_price();
    let redemption_before = engine(&world).redemption;

    let result = update_redemption_rate(&mut world);

    assert_refused(&result, "Market price oracle observation is stale");
    assert_eq!(engine(&world).redemption, redemption_before);
}

// ── Every transaction leaves the engine idle ─────────────────────────────────────

/// `send` asserts this after every transaction of every test; this one walks the refusals that
/// happen while the engine is parked, at each kind of reply.
#[test]
fn the_engine_is_never_left_waiting() {
    let (mut world, user) = world_with_user(10_000);
    open_position(&mut world, &user, 1_500).expect("the position opens");

    // Refused on the clock's reply.
    assert_refused(
        &generate(&mut world, &user, 1_001),
        "Position is undercollateralized",
    );
    assert_eq!(engine(&world).pending, None);
    generate(&mut world, &user, 1_000).expect("the debt is generated");
    assert_refused(
        &withdraw(&mut world, &user, 1),
        "Position is undercollateralized",
    );
    assert_eq!(engine(&world).pending, None);
    assert_refused(
        &update_redemption_rate(&mut world),
        "update_redemption_rate called too soon since last update",
    );
    assert_eq!(engine(&world).pending, None);

    // Failed while waiting for the oracle (its price account does not exist).
    tick(&mut world.state, T0 + 2_000);
    let result = update_redemption_rate(&mut world);
    assert_price_account_missing(&world, &result);
    assert_eq!(engine(&world).pending, None);

    // The engine still serves the next conversation.
    accrue(&mut world).expect("the poke succeeds");
    assert_eq!(engine(&world).fee.last_accrued_at, T0 + 2_000);
}
