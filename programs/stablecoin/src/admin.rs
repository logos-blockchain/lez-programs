//! Administration: the admin and freeze-authority messages.
//!
//! The admin (or the freeze authority) sends the message to its own account, the seat, which
//! only checks the signature and forwards the action to the engine ([`forward`]). The engine
//! checks the role against the seat's account id, applies the unchanged band checks and writes
//! ([`apply`]).

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
use stablecoin_core::{
    bound_redemption_price,
    event::{FrozenChanged, ParametersChanged, StablecoinEvent as _},
    math::{compute_current_redemption_price_wide, FIXED_POINT_ONE},
    AdminAction, Engine, Internal, Pending, ProtocolParameters, RedemptionPriceState,
};

use crate::{accrue_stability_fee::accrue, checks, clock, engine_actor, engine_state, save_engine};

/// At the seat, signed by the seat's account as a root message: forward `action` to the engine.
///
/// # Panics
/// - `"only the account's owner may send this"` when the message is nested or unsigned.
pub(crate) fn forward(input: &ReceiveInput, action: AdminAction) -> Response {
    checks::assert_owner(input);
    Response::keep_state().send_sync(engine_actor(input), &Internal::Admin(action))
}

/// At the engine, from a seat: check the seat's role, then apply `action`.
///
/// # Panics
/// - `"Signer is not the protocol's admin"` for a setter from anyone but the admin.
/// - `"Signer is not the protocol's freeze authority"` for a freeze or unfreeze from anyone else.
/// - `"Signer is neither the protocol's admin nor its freeze authority"` for a reset from anyone
///   else.
/// - The band panics of each setter.
pub(crate) fn apply(input: &ReceiveInput, from: Actor, action: AdminAction) -> Response {
    let engine = engine_state(input);
    let signer = from.account_id;
    match action {
        AdminAction::Freeze => set_frozen(engine, signer, true),
        AdminAction::Unfreeze => set_frozen(engine, signer, false),
        AdminAction::ResetRedemptionRateController => {
            assert!(
                signer == engine.parameters.admin_account_id
                    || signer == engine.parameters.freeze_authority_account_id,
                "Signer is neither the protocol's admin nor its freeze authority"
            );
            clock::read_clock(engine, Pending::ResetController)
        }
        AdminAction::SetStabilityFeePerMillisecond { new_rate } => {
            assert_admin(&engine, signer);
            checks::assert_stability_fee_in_band(new_rate, "new_stability_fee_per_millisecond");
            clock::read_clock(engine, Pending::SetStabilityFee { new_rate })
        }
        AdminAction::SetMinimumCollateralizationRatio { new_ratio } => {
            change(engine, signer, |parameters| {
                checks::assert_collateralization_ratio_in_band(
                    new_ratio,
                    "new_minimum_collateralization_ratio",
                );
                parameters.minimum_collateralization_ratio = new_ratio;
            })
        }
        AdminAction::SetControllerGains {
            new_proportional_gain,
            new_integral_gain,
        } => change(engine, signer, |parameters| {
            checks::assert_controller_gains_in_band(new_proportional_gain, new_integral_gain);
            parameters.controller_proportional_gain = new_proportional_gain;
            parameters.controller_integral_gain = new_integral_gain;
        }),
        AdminAction::SetMarketPriceOracle {
            oracle_account_id,
            oracle_program_id,
        } => change(engine, signer, |parameters| {
            checks::assert_oracle_program_is_set(oracle_program_id);
            parameters.market_price_oracle_id = oracle_account_id;
            parameters.market_price_oracle_program_id = oracle_program_id;
        }),
        AdminAction::SetTimingParameters {
            minimum_milliseconds_between_rate_updates,
            maximum_oracle_price_age_milliseconds,
        } => change(engine, signer, |parameters| {
            checks::assert_timing_milliseconds_in_band(
                minimum_milliseconds_between_rate_updates,
                "new_minimum_milliseconds_between_rate_updates",
            );
            checks::assert_timing_milliseconds_in_band(
                maximum_oracle_price_age_milliseconds,
                "new_maximum_oracle_price_age_milliseconds",
            );
            parameters.minimum_milliseconds_between_rate_updates =
                minimum_milliseconds_between_rate_updates;
            parameters.maximum_oracle_price_age_milliseconds =
                maximum_oracle_price_age_milliseconds;
        }),
        AdminAction::SetAdmin {
            new_admin_account_id,
        } => change(engine, signer, |parameters| {
            parameters.admin_account_id = new_admin_account_id;
        }),
        AdminAction::SetFreezeAuthority {
            new_freeze_authority_account_id,
        } => change(engine, signer, |parameters| {
            parameters.freeze_authority_account_id = new_freeze_authority_account_id;
        }),
        AdminAction::SetDebtLimits {
            minimum_debt_amount,
            debt_ceiling,
        } => change(engine, signer, |parameters| {
            assert!(
                stablecoin_core::debt_limits::valid_debt_limits(minimum_debt_amount, debt_ceiling),
                "Debt limits require 0 < minimum_debt_amount <= debt_ceiling"
            );
            parameters.minimum_debt_amount = minimum_debt_amount;
            parameters.debt_ceiling = debt_ceiling;
        }),
    }
}

fn assert_admin(engine: &Engine, signer: AccountId) {
    assert!(
        signer == engine.parameters.admin_account_id,
        "Signer is not the protocol's admin"
    );
}

/// The shape of the plain setters: the admin signed, `mutate` checks the band and writes.
fn change(
    mut engine: Engine,
    signer: AccountId,
    mutate: impl FnOnce(&mut ProtocolParameters),
) -> Response {
    assert_admin(&engine, signer);
    mutate(&mut engine.parameters);
    parameters_changed(engine)
}

fn parameters_changed(engine: Engine) -> Response {
    let event = ParametersChanged {
        parameters: engine.parameters.clone(),
    }
    .to_program_event();
    save_engine(engine).event(event)
}

/// Idempotent: freezing a frozen protocol (or unfreezing an unfrozen one) succeeds.
fn set_frozen(mut engine: Engine, signer: AccountId, is_frozen: bool) -> Response {
    assert!(
        signer == engine.parameters.freeze_authority_account_id,
        "Signer is not the protocol's freeze authority"
    );
    engine.parameters.is_frozen = is_frozen;
    save_engine(engine).event(FrozenChanged { is_frozen }.to_program_event())
}

/// At the engine, on the clock's reply: accrue at the OLD rate up to `now`, then store the new
/// rate, so the change is never retroactive.
pub(crate) fn finish_set_stability_fee(
    input: &ReceiveInput,
    mut engine: Engine,
    new_rate: u128,
    now: u64,
) -> Response {
    let accrued = accrue(input, &mut engine, now);
    engine.parameters.stability_fee_per_millisecond = new_rate;
    let event = ParametersChanged {
        parameters: engine.parameters.clone(),
    }
    .to_program_event();
    accrued.attach(save_engine(engine)).event(event)
}

/// At the engine, on the clock's reply: re-anchor the redemption price at its current
/// projection (clamped into its bounds), then set the rate to exactly 1.0 and the integral
/// term to 0. The price does not jump while the projection is within the bounds, so no
/// position's collateralization changes. A projection outside them is stored as the bound, and
/// that does change the collateralization of positions with debt: a withdrawal checks against
/// the unclamped projection.
pub(crate) fn finish_reset_controller(
    _input: &ReceiveInput,
    mut engine: Engine,
    now: u64,
) -> Response {
    let redemption = &engine.redemption;
    let current_price = bound_redemption_price(compute_current_redemption_price_wide(
        redemption.redemption_price_at_last_update,
        redemption.redemption_rate_per_millisecond,
        redemption.last_updated_at,
        now,
    ));
    engine.redemption = RedemptionPriceState {
        redemption_price_at_last_update: current_price,
        redemption_rate_per_millisecond: FIXED_POINT_ONE,
        controller_integral_term: 0,
        last_updated_at: now,
    };
    save_engine(engine)
}

#[cfg(test)]
mod tests {
    use stablecoin_core::{
        event::StablecoinEvent as _,
        math::{compute_current_accumulated_rate, FIXED_POINT_ONE},
        MAXIMUM_STABILITY_FEE_PER_MILLISECOND,
    };

    use super::*;
    use crate::test_utils::*;

    fn seat(account_id: AccountId) -> Actor {
        stablecoin(account_id)
    }

    fn pre() -> StablecoinState {
        StablecoinState::Engine(engine_at(1_000))
    }

    /// The engine receives `action` from the seat of `signer`.
    fn apply_from(signer: AccountId, action: AdminAction) -> Response {
        run(
            engine(),
            Some(seat(signer)),
            false,
            Some(&pre()),
            &Internal::Admin(action),
        )
    }

    fn apply_as_admin(action: AdminAction) -> Engine {
        saved_engine(&apply_from(ADMIN, action))
    }

    // --- the seat ---

    fn forwarded_pairs() -> Vec<(Message, AdminAction)> {
        vec![
            (
                Message::SetMinimumCollateralizationRatio {
                    new_ratio: FIXED_POINT_ONE * 2,
                },
                AdminAction::SetMinimumCollateralizationRatio {
                    new_ratio: FIXED_POINT_ONE * 2,
                },
            ),
            (
                Message::SetControllerGains {
                    new_proportional_gain: 1,
                    new_integral_gain: 2,
                },
                AdminAction::SetControllerGains {
                    new_proportional_gain: 1,
                    new_integral_gain: 2,
                },
            ),
            (
                Message::SetMarketPriceOracle {
                    oracle_account_id: OWNER,
                    oracle_program_id: OTHER_PROGRAM,
                },
                AdminAction::SetMarketPriceOracle {
                    oracle_account_id: OWNER,
                    oracle_program_id: OTHER_PROGRAM,
                },
            ),
            (
                Message::SetTimingParameters {
                    minimum_milliseconds_between_rate_updates: 5,
                    maximum_oracle_price_age_milliseconds: 6,
                },
                AdminAction::SetTimingParameters {
                    minimum_milliseconds_between_rate_updates: 5,
                    maximum_oracle_price_age_milliseconds: 6,
                },
            ),
            (
                Message::SetAdmin {
                    new_admin_account_id: OWNER,
                },
                AdminAction::SetAdmin {
                    new_admin_account_id: OWNER,
                },
            ),
            (
                Message::SetFreezeAuthority {
                    new_freeze_authority_account_id: OWNER,
                },
                AdminAction::SetFreezeAuthority {
                    new_freeze_authority_account_id: OWNER,
                },
            ),
            (
                Message::SetStabilityFeePerMillisecond { new_rate: 7 },
                AdminAction::SetStabilityFeePerMillisecond { new_rate: 7 },
            ),
            (Message::Freeze, AdminAction::Freeze),
            (Message::Unfreeze, AdminAction::Unfreeze),
            (
                Message::ResetRedemptionRateController,
                AdminAction::ResetRedemptionRateController,
            ),
            (
                Message::SetDebtLimits {
                    minimum_debt_amount: 1,
                    debt_ceiling: 2,
                },
                AdminAction::SetDebtLimits {
                    minimum_debt_amount: 1,
                    debt_ceiling: 2,
                },
            ),
        ]
    }

    #[test]
    fn the_seat_forwards_every_admin_message_to_the_engine() {
        let pairs = forwarded_pairs();
        assert_eq!(pairs.len(), 11);
        for (message, action) in pairs {
            assert_eq!(
                run(seat(ADMIN), None, true, None, &message),
                Response::keep_state().send_sync(engine(), &Internal::Admin(action)),
                "{message:?}",
            );
        }
    }

    #[test]
    fn a_seat_that_also_holds_a_position_still_forwards() {
        let position = StablecoinState::Position(Position {
            token_program_id: TOKEN,
            collateral_definition_id: COLLATERAL_DEFINITION,
            collateral_amount: 5,
            normalized_debt_amount: 3,
        });
        assert_eq!(
            run(seat(ADMIN), None, true, Some(&position), &Message::Freeze),
            Response::keep_state().send_sync(engine(), &Internal::Admin(AdminAction::Freeze)),
        );
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn an_unsigned_seat_message_panics() {
        let _ = run(seat(ADMIN), None, false, None, &Message::Freeze);
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn a_nested_seat_message_panics() {
        let _ = run(
            seat(ADMIN),
            Some(outsider(OWNER)),
            true,
            None,
            &Message::Freeze,
        );
    }

    // --- roles ---

    #[test]
    #[should_panic(expected = "Signer is not the protocol's admin")]
    fn a_setter_from_the_freeze_authority_panics() {
        let _ = apply_from(
            FREEZE_AUTHORITY,
            AdminAction::SetAdmin {
                new_admin_account_id: OWNER,
            },
        );
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's admin")]
    fn the_fee_setter_from_a_stranger_panics() {
        let _ = apply_from(
            OWNER,
            AdminAction::SetStabilityFeePerMillisecond {
                new_rate: FIXED_POINT_ONE,
            },
        );
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's admin")]
    fn the_debt_limits_from_a_stranger_panics() {
        let _ = apply_from(
            OWNER,
            AdminAction::SetDebtLimits {
                minimum_debt_amount: 1,
                debt_ceiling: 2,
            },
        );
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's freeze authority")]
    fn freeze_from_the_admin_panics() {
        let _ = apply_from(ADMIN, AdminAction::Freeze);
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's freeze authority")]
    fn unfreeze_from_a_stranger_panics() {
        let _ = apply_from(OWNER, AdminAction::Unfreeze);
    }

    #[test]
    #[should_panic(expected = "Signer is neither the protocol's admin nor its freeze authority")]
    fn reset_from_a_stranger_panics() {
        let _ = apply_from(OWNER, AdminAction::ResetRedemptionRateController);
    }

    #[test]
    #[should_panic(expected = "this actor is not the engine")]
    fn an_admin_action_at_an_actor_that_is_not_the_engine_panics() {
        let _ = run(
            stablecoin(OWNER),
            Some(seat(ADMIN)),
            false,
            None,
            &Internal::Admin(AdminAction::Freeze),
        );
    }

    // --- setters ---

    #[test]
    fn each_setter_changes_only_its_fields_and_emits_the_event() {
        let before = parameters();
        let cases: Vec<(AdminAction, ProtocolParameters)> = vec![
            (
                AdminAction::SetMinimumCollateralizationRatio {
                    new_ratio: FIXED_POINT_ONE * 2,
                },
                ProtocolParameters {
                    minimum_collateralization_ratio: FIXED_POINT_ONE * 2,
                    ..before.clone()
                },
            ),
            (
                AdminAction::SetControllerGains {
                    new_proportional_gain: 5,
                    new_integral_gain: 6,
                },
                ProtocolParameters {
                    controller_proportional_gain: 5,
                    controller_integral_gain: 6,
                    ..before.clone()
                },
            ),
            (
                AdminAction::SetMarketPriceOracle {
                    oracle_account_id: OWNER,
                    oracle_program_id: OTHER_PROGRAM,
                },
                ProtocolParameters {
                    market_price_oracle_id: OWNER,
                    market_price_oracle_program_id: OTHER_PROGRAM,
                    ..before.clone()
                },
            ),
            (
                AdminAction::SetTimingParameters {
                    minimum_milliseconds_between_rate_updates: 5,
                    maximum_oracle_price_age_milliseconds: 6,
                },
                ProtocolParameters {
                    minimum_milliseconds_between_rate_updates: 5,
                    maximum_oracle_price_age_milliseconds: 6,
                    ..before.clone()
                },
            ),
            (
                AdminAction::SetAdmin {
                    new_admin_account_id: OWNER,
                },
                ProtocolParameters {
                    admin_account_id: OWNER,
                    ..before.clone()
                },
            ),
            (
                AdminAction::SetFreezeAuthority {
                    new_freeze_authority_account_id: OWNER,
                },
                ProtocolParameters {
                    freeze_authority_account_id: OWNER,
                    ..before.clone()
                },
            ),
            (
                // A floor equal to the ceiling is accepted; the fixture's floor 10 and ceiling
                // 1_000_000 both change, and nothing else does.
                AdminAction::SetDebtLimits {
                    minimum_debt_amount: 20,
                    debt_ceiling: 20,
                },
                ProtocolParameters {
                    minimum_debt_amount: 20,
                    debt_ceiling: 20,
                    ..before.clone()
                },
            ),
        ];
        for (action, expected) in cases {
            let response = apply_from(ADMIN, action.clone());
            let after = saved_engine(&response);
            assert_eq!(after.parameters, expected, "{action:?}");
            assert_eq!(after.fee, engine_at(1_000).fee);
            assert_eq!(after.redemption, engine_at(1_000).redemption);
            assert!(response.sync_messages.is_empty());
            assert_eq!(
                response.events,
                vec![ParametersChanged {
                    parameters: expected
                }
                .to_program_event()],
                "{action:?}",
            );
        }
    }

    #[test]
    #[should_panic(expected = "Market price oracle program must not be the default owner")]
    fn a_zero_oracle_program_id_panics() {
        let _ = apply_as_admin(AdminAction::SetMarketPriceOracle {
            oracle_account_id: OWNER,
            oracle_program_id: AccountId::new([0; 32]),
        });
    }

    #[test]
    fn the_gains_setter_keeps_the_integral_term() {
        let mut parked = engine_at(1_000);
        parked.redemption.controller_integral_term = 99;
        let response = run(
            engine(),
            Some(seat(ADMIN)),
            false,
            Some(&StablecoinState::Engine(parked)),
            &Internal::Admin(AdminAction::SetControllerGains {
                new_proportional_gain: 1,
                new_integral_gain: 1,
            }),
        );
        assert_eq!(
            saved_engine(&response).redemption.controller_integral_term,
            99
        );
    }

    #[test]
    fn the_band_endpoints_are_accepted() {
        let low = apply_as_admin(AdminAction::SetMinimumCollateralizationRatio {
            new_ratio: checks::MIN_COLLATERALIZATION_RATIO,
        });
        assert_eq!(
            low.parameters.minimum_collateralization_ratio,
            checks::MIN_COLLATERALIZATION_RATIO
        );
        let high = apply_as_admin(AdminAction::SetTimingParameters {
            minimum_milliseconds_between_rate_updates: 1,
            maximum_oracle_price_age_milliseconds: checks::MAX_TIMING_MILLISECONDS,
        });
        assert_eq!(high.parameters.minimum_milliseconds_between_rate_updates, 1);
    }

    #[test]
    #[should_panic(expected = "new_minimum_collateralization_ratio below 1.1x")]
    fn a_ratio_below_the_band_panics() {
        let _ = apply_as_admin(AdminAction::SetMinimumCollateralizationRatio {
            new_ratio: checks::MIN_COLLATERALIZATION_RATIO - 1,
        });
    }

    #[test]
    #[should_panic(expected = "new_minimum_collateralization_ratio above 10x")]
    fn a_ratio_above_the_band_panics() {
        let _ = apply_as_admin(AdminAction::SetMinimumCollateralizationRatio {
            new_ratio: checks::MAX_COLLATERALIZATION_RATIO + 1,
        });
    }

    #[test]
    #[should_panic(expected = "controller_proportional_gain out of band")]
    fn a_negative_proportional_gain_panics() {
        let _ = apply_as_admin(AdminAction::SetControllerGains {
            new_proportional_gain: -1,
            new_integral_gain: 0,
        });
    }

    #[test]
    #[should_panic(expected = "controller_proportional_gain out of band")]
    fn an_oversized_proportional_gain_panics() {
        let _ = apply_as_admin(AdminAction::SetControllerGains {
            new_proportional_gain: stablecoin_core::MAXIMUM_CONTROLLER_PROPORTIONAL_GAIN + 1,
            new_integral_gain: 0,
        });
    }

    #[test]
    #[should_panic(expected = "controller_integral_gain out of band")]
    fn an_oversized_integral_gain_panics() {
        let _ = apply_as_admin(AdminAction::SetControllerGains {
            new_proportional_gain: 0,
            new_integral_gain: stablecoin_core::MAXIMUM_CONTROLLER_INTEGRAL_GAIN + 1,
        });
    }

    #[test]
    #[should_panic(expected = "new_minimum_milliseconds_between_rate_updates below minimum 1ms")]
    fn a_zero_interval_panics() {
        let _ = apply_as_admin(AdminAction::SetTimingParameters {
            minimum_milliseconds_between_rate_updates: 0,
            maximum_oracle_price_age_milliseconds: 1,
        });
    }

    #[test]
    #[should_panic(
        expected = "new_maximum_oracle_price_age_milliseconds above maximum 86_400_000ms"
    )]
    fn an_oversized_staleness_window_panics() {
        let _ = apply_as_admin(AdminAction::SetTimingParameters {
            minimum_milliseconds_between_rate_updates: 1,
            maximum_oracle_price_age_milliseconds: checks::MAX_TIMING_MILLISECONDS + 1,
        });
    }

    #[test]
    #[should_panic(expected = "Debt limits require 0 < minimum_debt_amount <= debt_ceiling")]
    fn a_zero_minimum_debt_panics() {
        let _ = apply_as_admin(AdminAction::SetDebtLimits {
            minimum_debt_amount: 0,
            debt_ceiling: 100,
        });
    }

    #[test]
    #[should_panic(expected = "Debt limits require 0 < minimum_debt_amount <= debt_ceiling")]
    fn a_minimum_debt_above_the_ceiling_panics() {
        let _ = apply_as_admin(AdminAction::SetDebtLimits {
            minimum_debt_amount: 10,
            debt_ceiling: 9,
        });
    }

    // --- freeze ---

    #[test]
    fn freeze_and_unfreeze_are_idempotent() {
        let freeze = apply_from(FREEZE_AUTHORITY, AdminAction::Freeze);
        assert!(saved_engine(&freeze).parameters.is_frozen);
        assert_eq!(
            freeze.events,
            vec![FrozenChanged { is_frozen: true }.to_program_event()],
        );
        let mut frozen = engine_at(1_000);
        frozen.parameters.is_frozen = true;
        let state = StablecoinState::Engine(frozen);
        let again = run(
            engine(),
            Some(seat(FREEZE_AUTHORITY)),
            false,
            Some(&state),
            &Internal::Admin(AdminAction::Freeze),
        );
        assert!(saved_engine(&again).parameters.is_frozen);
        let unfreeze = run(
            engine(),
            Some(seat(FREEZE_AUTHORITY)),
            false,
            Some(&state),
            &Internal::Admin(AdminAction::Unfreeze),
        );
        assert!(!saved_engine(&unfreeze).parameters.is_frozen);
        assert_eq!(
            unfreeze.events,
            vec![FrozenChanged { is_frozen: false }.to_program_event()],
        );
        let unfreeze_again = apply_from(FREEZE_AUTHORITY, AdminAction::Unfreeze);
        assert!(!saved_engine(&unfreeze_again).parameters.is_frozen);
    }

    // --- the fee setter ---

    #[test]
    fn the_fee_setter_parks_and_asks_the_clock() {
        let response = apply_from(
            ADMIN,
            AdminAction::SetStabilityFeePerMillisecond {
                new_rate: FIXED_POINT_ONE + 1,
            },
        );
        let mut parked = engine_at(1_000);
        parked.pending = Some(Pending::SetStabilityFee {
            new_rate: FIXED_POINT_ONE + 1,
        });
        assert_eq!(
            response,
            Response::set_state(StablecoinState::Engine(parked).encode()).send_sync(
                clock(),
                &clock_core::Message::ReadTimestamp { granularity: 1 }
            ),
        );
    }

    #[test]
    #[should_panic(expected = "new_stability_fee_per_millisecond below FIXED_POINT_ONE")]
    fn a_fee_below_the_band_panics() {
        let _ = apply_as_admin(AdminAction::SetStabilityFeePerMillisecond {
            new_rate: FIXED_POINT_ONE - 1,
        });
    }

    #[test]
    #[should_panic(expected = "new_stability_fee_per_millisecond above sane upper bound")]
    fn a_fee_above_the_band_panics() {
        let _ = apply_as_admin(AdminAction::SetStabilityFeePerMillisecond {
            new_rate: MAXIMUM_STABILITY_FEE_PER_MILLISECOND + 1,
        });
    }

    #[test]
    #[should_panic(expected = "the engine is already waiting for a reply")]
    fn the_fee_setter_while_another_operation_is_parked_panics() {
        let mut parked = engine_at(1_000);
        parked.pending = Some(Pending::AccrueStabilityFee);
        let _ = run(
            engine(),
            Some(seat(ADMIN)),
            false,
            Some(&StablecoinState::Engine(parked)),
            &Internal::Admin(AdminAction::SetStabilityFeePerMillisecond {
                new_rate: FIXED_POINT_ONE,
            }),
        );
    }

    fn fee_reply(old_rate: u128, new_rate: u128, now: u64) -> (Response, Engine) {
        let mut parked = engine_at(1_000);
        parked.parameters.stability_fee_per_millisecond = old_rate;
        parked.fee.total_normalized_debt = 1_000_000;
        parked.pending = Some(Pending::SetStabilityFee { new_rate });
        let response = run(
            engine(),
            Some(clock()),
            false,
            Some(&StablecoinState::Engine(parked)),
            &clock_reply(now),
        );
        let after = saved_engine(&response);
        (response, after)
    }

    #[test]
    fn the_fee_setter_accrues_the_gap_at_the_old_rate_then_stores_the_new_one() {
        let old_rate = FIXED_POINT_ONE + FIXED_POINT_ONE / 1_000_000;
        let new_rate = FIXED_POINT_ONE + FIXED_POINT_ONE / 100_000;
        let (response, after) = fee_reply(old_rate, new_rate, 4_000);
        let at_old = compute_current_accumulated_rate(FIXED_POINT_ONE, old_rate, 1_000, 4_000);
        let at_new = compute_current_accumulated_rate(FIXED_POINT_ONE, new_rate, 1_000, 4_000);
        assert_ne!(at_old, at_new, "the fixture must tell the two rates apart");
        assert_eq!(after.fee.accumulated_rate_at_last_accrual, at_old);
        assert_eq!(after.fee.last_accrued_at, 4_000);
        assert_eq!(after.parameters.stability_fee_per_millisecond, new_rate);
        assert_eq!(after.pending, None);
        // The surplus mint comes first, then the accrual event, then the parameters event.
        assert_eq!(response.sync_messages.len(), 1);
        assert_eq!(response.events.len(), 2);
        assert_eq!(
            response.events.last(),
            Some(
                &ParametersChanged {
                    parameters: after.parameters.clone()
                }
                .to_program_event()
            ),
        );
    }

    #[test]
    fn the_fee_setter_with_no_time_elapsed_keeps_the_anchor() {
        let (response, after) = fee_reply(FIXED_POINT_ONE * 2, FIXED_POINT_ONE, 1_000);
        assert_eq!(after.fee.accumulated_rate_at_last_accrual, FIXED_POINT_ONE);
        assert_eq!(
            after.parameters.stability_fee_per_millisecond,
            FIXED_POINT_ONE
        );
        assert!(response.sync_messages.is_empty());
    }

    // --- the controller reset ---

    fn reset_reply(parked: Engine, now: u64) -> (Response, Engine) {
        let response = run(
            engine(),
            Some(clock()),
            false,
            Some(&StablecoinState::Engine(parked)),
            &clock_reply(now),
        );
        let after = saved_engine(&response);
        (response, after)
    }

    fn wound_up(rate: u128, last_updated_at: u64) -> Engine {
        let mut parked = engine_at(last_updated_at);
        parked.redemption.redemption_rate_per_millisecond = rate;
        parked.redemption.controller_integral_term = 777;
        parked.pending = Some(Pending::ResetController);
        parked
    }

    #[test]
    fn the_reset_parks_for_the_admin_and_the_freeze_authority() {
        for signer in [ADMIN, FREEZE_AUTHORITY] {
            let response = apply_from(signer, AdminAction::ResetRedemptionRateController);
            let mut parked = engine_at(1_000);
            parked.pending = Some(Pending::ResetController);
            assert_eq!(
                response,
                Response::set_state(StablecoinState::Engine(parked).encode()).send_sync(
                    clock(),
                    &clock_core::Message::ReadTimestamp { granularity: 1 }
                ),
            );
        }
    }

    #[test]
    fn the_reset_re_anchors_the_price_at_now_and_zeroes_the_controller() {
        let rate = FIXED_POINT_ONE + FIXED_POINT_ONE / 1_000_000;
        let (_, after) = reset_reply(wound_up(rate, 1_000), 3_000);
        let projected = bound_redemption_price(compute_current_redemption_price_wide(
            FIXED_POINT_ONE,
            rate,
            1_000,
            3_000,
        ));
        assert!(projected > FIXED_POINT_ONE);
        assert_eq!(after.redemption.redemption_price_at_last_update, projected);
        assert_eq!(
            after.redemption.redemption_rate_per_millisecond,
            FIXED_POINT_ONE
        );
        assert_eq!(after.redemption.controller_integral_term, 0);
        assert_eq!(after.redemption.last_updated_at, 3_000);
        assert_eq!(after.pending, None);
    }

    #[test]
    fn the_reset_recovers_a_projection_that_overflows() {
        let rate = FIXED_POINT_ONE + FIXED_POINT_ONE / 100_000;
        let (_, after) = reset_reply(wound_up(rate, 1_000), 1_000 + 86_400_000);
        assert_eq!(
            after.redemption.redemption_price_at_last_update,
            stablecoin_core::MAXIMUM_REDEMPTION_PRICE
        );
        assert_eq!(
            after.redemption.redemption_rate_per_millisecond,
            FIXED_POINT_ONE
        );
    }

    #[test]
    fn the_reset_recovers_a_projection_that_decays_to_zero() {
        let rate = FIXED_POINT_ONE - FIXED_POINT_ONE / 100_000;
        let (_, after) = reset_reply(wound_up(rate, 1_000), 1_000 + 86_400_000);
        assert_eq!(
            after.redemption.redemption_price_at_last_update,
            stablecoin_core::MINIMUM_REDEMPTION_PRICE
        );
    }

    #[test]
    fn the_reset_is_idempotent() {
        let (_, once) = reset_reply(wound_up(FIXED_POINT_ONE + 5, 1_000), 3_000);
        let mut again = once.clone();
        again.pending = Some(Pending::ResetController);
        let (_, twice) = reset_reply(again, 3_000);
        assert_eq!(twice.redemption, once.redemption);
    }
}
