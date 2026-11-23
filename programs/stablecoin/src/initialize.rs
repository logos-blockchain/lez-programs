//! `Initialize`: the program's own account sets the program up.
//!
//! The conversation:
//!
//! 1. The program account syncs [`Internal::CreateStablecoinToken`] to the surplus holding, with
//!    the surplus seed. In LEZ a PDA seed on a sync message authorizes only that message's
//!    receiver, and the authorization holds further down the chain. The token's `NewDefinition`
//!    needs both the definition and the initial holding authorized, so the surplus holding relays:
//!    authorized itself, it sends `NewDefinition` to the definition with the definition seed, and
//!    the token's own `InitHolding` at the surplus holding inherits the authorization.
//! 2. The program account syncs [`Internal::InitializeEngine`] to the engine, with the engine seed.
//!    The engine validates, writes its state with both timestamps at 0, and asks the clock; the
//!    reply stamps them.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use stablecoin_core::{
    compute_engine_pda_seed, compute_stablecoin_definition_pda,
    compute_stablecoin_definition_pda_seed, compute_surplus_holding_pda,
    compute_surplus_holding_pda_seed,
    event::{Initialized, StablecoinEvent as _},
    math::FIXED_POINT_ONE,
    Engine, InitializeArgs, Internal, Pending, ProtocolParameters, RedemptionPriceState,
    StabilityFeeAccumulator, StablecoinState, MAXIMUM_REDEMPTION_PRICE, MAX_NAME_LEN,
    MINIMUM_REDEMPTION_PRICE,
};

use crate::{checks, clock, engine_actor, program_id, save_engine, stablecoin_actor};

/// At the program account, signed by it, as the transaction's root message.
///
/// # Panics
/// - `"only the program's own account may initialize it"` when the receiver is not the program
///   account, the message is unsigned, or it is nested.
pub(crate) fn initialize(input: &ReceiveInput, args: InitializeArgs) -> Response {
    assert!(
        input.receiver.account_id == input.receiver.program_account_id
            && input.is_authorized
            && input.from.is_none(),
        "only the program's own account may initialize it"
    );
    let program = program_id(input);
    let name = args.stablecoin_name.clone();
    let token_program_id = args.token_program_id;
    Response::keep_state()
        .send(
            SyncMessage::new(
                stablecoin_actor(input, compute_surplus_holding_pda(program)),
                &Internal::CreateStablecoinToken {
                    name,
                    token_program_id,
                },
            )
            .with_pda_seeds([compute_surplus_holding_pda_seed()]),
        )
        .send(
            SyncMessage::new(engine_actor(input), &Internal::InitializeEngine(args))
                .with_pda_seeds([compute_engine_pda_seed()]),
        )
}

/// At the surplus holding, from the program account: create the stablecoin token with an empty
/// supply and this holding as its initial holding.
///
/// # Panics
/// - `"only the program account may send this"` when the sender is anyone else.
pub(crate) fn create_stablecoin_token(
    input: &ReceiveInput,
    from: Actor,
    name: String,
    token_program_id: AccountId,
) -> Response {
    assert_from_program_account(input, from);
    let program = program_id(input);
    let definition = compute_stablecoin_definition_pda(program);
    Response::keep_state().send(
        SyncMessage::new(
            Actor::new(definition, token_program_id),
            &token_core::Message::NewDefinition {
                name,
                total_supply: 0,
                mint_authority: Some(definition),
                holding: compute_surplus_holding_pda(program),
            },
        )
        .with_pda_seeds([compute_stablecoin_definition_pda_seed()]),
    )
}

/// At the engine, from the program account: validate, write the engine and ask the clock.
///
/// # Panics
/// - `"only the program account may send this"` when the sender is anyone else.
/// - `"the engine is already initialized"` when the engine has state.
/// - The band and name messages of the individual checks.
pub(crate) fn initialize_engine(
    input: &ReceiveInput,
    from: Actor,
    args: InitializeArgs,
) -> Response {
    assert_from_program_account(input, from);
    assert!(
        StablecoinState::decode(&input.pre_state).is_none(),
        "the engine is already initialized"
    );
    assert_args_in_bands(&args);
    let engine = Engine {
        parameters: ProtocolParameters {
            admin_account_id: args.initial_admin_account_id,
            freeze_authority_account_id: args.freeze_authority_account_id,
            token_program_id: args.token_program_id,
            stablecoin_definition_id: compute_stablecoin_definition_pda(program_id(input)),
            collateral_definition_id: args.collateral_definition_id,
            market_price_oracle_id: args.market_price_oracle_id,
            market_price_oracle_program_id: args.market_price_oracle_program_id,
            stability_fee_per_millisecond: args.initial_stability_fee_per_millisecond,
            controller_proportional_gain: args.initial_controller_proportional_gain,
            controller_integral_gain: args.initial_controller_integral_gain,
            minimum_collateralization_ratio: args.initial_minimum_collateralization_ratio,
            minimum_milliseconds_between_rate_updates: args
                .minimum_milliseconds_between_rate_updates,
            maximum_oracle_price_age_milliseconds: args.maximum_oracle_price_age_milliseconds,
            is_frozen: false,
            minimum_debt_amount: args.minimum_debt_amount,
            debt_ceiling: args.debt_ceiling,
        },
        fee: StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
            last_accrued_at: 0,
            total_normalized_debt: 0,
        },
        redemption: RedemptionPriceState {
            redemption_price_at_last_update: args.initial_redemption_price,
            redemption_rate_per_millisecond: FIXED_POINT_ONE,
            controller_integral_term: 0,
            last_updated_at: 0,
        },
        pending: None,
    };
    clock::read_clock(engine, Pending::Initialize)
}

/// At the engine, on the clock's reply: stamp both timestamps with `now` and announce it.
pub(crate) fn finish(input: &ReceiveInput, mut engine: Engine, now: u64) -> Response {
    engine.fee.last_accrued_at = now;
    engine.redemption.last_updated_at = now;
    let event = Initialized {
        engine: input.receiver.account_id,
        stablecoin_definition: engine.parameters.stablecoin_definition_id,
    }
    .to_program_event();
    save_engine(engine).event(event)
}

fn assert_from_program_account(input: &ReceiveInput, from: Actor) {
    let program = program_id(input);
    assert!(
        from == Actor::new(program, program),
        "only the program account may send this"
    );
}

fn assert_args_in_bands(args: &InitializeArgs) {
    checks::assert_stability_fee_in_band(
        args.initial_stability_fee_per_millisecond,
        "initial_stability_fee_per_millisecond",
    );
    checks::assert_collateralization_ratio_in_band(
        args.initial_minimum_collateralization_ratio,
        "initial_minimum_collateralization_ratio",
    );
    checks::assert_controller_gains_in_band(
        args.initial_controller_proportional_gain,
        args.initial_controller_integral_gain,
    );
    checks::assert_timing_milliseconds_in_band(
        args.minimum_milliseconds_between_rate_updates,
        "minimum_milliseconds_between_rate_updates",
    );
    checks::assert_timing_milliseconds_in_band(
        args.maximum_oracle_price_age_milliseconds,
        "maximum_oracle_price_age_milliseconds",
    );
    assert!(
        (MINIMUM_REDEMPTION_PRICE..=MAXIMUM_REDEMPTION_PRICE)
            .contains(&args.initial_redemption_price),
        "initial_redemption_price must be within the §8 redemption-price bounds"
    );
    assert!(
        (1..=MAX_NAME_LEN).contains(&args.stablecoin_name.len()),
        "stablecoin_name must be 1 to 64 bytes"
    );
    checks::assert_oracle_program_is_set(args.market_price_oracle_program_id);
    assert!(
        stablecoin_core::debt_limits::valid_debt_limits(
            args.minimum_debt_amount,
            args.debt_ceiling
        ),
        "Debt limits require 0 < minimum_debt_amount <= debt_ceiling"
    );
}

#[cfg(test)]
mod tests {
    use stablecoin_core::{
        compute_engine_pda_seed, compute_stablecoin_definition_pda_seed,
        compute_surplus_holding_pda_seed, event::StablecoinEvent as _, Internal,
        MAXIMUM_STABILITY_FEE_PER_MILLISECOND,
    };

    use super::*;
    use crate::test_utils::*;

    fn args() -> InitializeArgs {
        InitializeArgs {
            initial_admin_account_id: ADMIN,
            freeze_authority_account_id: FREEZE_AUTHORITY,
            token_program_id: TOKEN,
            collateral_definition_id: COLLATERAL_DEFINITION,
            market_price_oracle_id: ORACLE_PRICE,
            market_price_oracle_program_id: ORACLE_PROGRAM,
            initial_stability_fee_per_millisecond: FIXED_POINT_ONE + 1,
            initial_controller_proportional_gain: 1,
            initial_controller_integral_gain: 2,
            initial_minimum_collateralization_ratio: FIXED_POINT_ONE * 3 / 2,
            minimum_milliseconds_between_rate_updates: 1_000,
            maximum_oracle_price_age_milliseconds: 10_000,
            initial_redemption_price: FIXED_POINT_ONE / 2,
            stablecoin_name: "test-stable".to_owned(),
            minimum_debt_amount: 10,
            debt_ceiling: 1_000_000,
        }
    }

    fn program_account() -> Actor {
        stablecoin(STABLECOIN)
    }

    fn initialize_with(args: InitializeArgs) -> Response {
        run(program_account(), None, true, None, &Message::from(args))
    }

    fn engine_with(args: InitializeArgs) -> Response {
        run(
            engine(),
            Some(program_account()),
            false,
            None,
            &Internal::InitializeEngine(args),
        )
    }

    #[test]
    fn the_program_account_creates_the_token_then_the_engine() {
        assert_eq!(
            initialize_with(args()),
            Response::keep_state()
                .send(
                    SyncMessage::new(
                        stablecoin(surplus_id()),
                        &Internal::CreateStablecoinToken {
                            name: "test-stable".to_owned(),
                            token_program_id: TOKEN,
                        },
                    )
                    .with_pda_seeds([compute_surplus_holding_pda_seed()]),
                )
                .send(
                    SyncMessage::new(engine(), &Internal::InitializeEngine(args()))
                        .with_pda_seeds([compute_engine_pda_seed()]),
                ),
        );
    }

    #[test]
    fn the_surplus_relay_creates_the_definition_with_the_surplus_holding() {
        assert_eq!(
            run(
                stablecoin(surplus_id()),
                Some(program_account()),
                true,
                None,
                &Internal::CreateStablecoinToken {
                    name: "test-stable".to_owned(),
                    token_program_id: TOKEN,
                },
            ),
            Response::keep_state().send(
                SyncMessage::new(
                    token(definition_id()),
                    &token_core::Message::NewDefinition {
                        name: "test-stable".to_owned(),
                        total_supply: 0,
                        mint_authority: Some(definition_id()),
                        holding: surplus_id(),
                    },
                )
                .with_pda_seeds([compute_stablecoin_definition_pda_seed()]),
            ),
        );
    }

    #[test]
    fn the_engine_writes_its_state_and_asks_the_clock() {
        let expected = Engine {
            parameters: ProtocolParameters {
                stability_fee_per_millisecond: FIXED_POINT_ONE + 1,
                controller_proportional_gain: 1,
                controller_integral_gain: 2,
                ..parameters()
            },
            fee: StabilityFeeAccumulator {
                accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
                last_accrued_at: 0,
                total_normalized_debt: 0,
            },
            redemption: RedemptionPriceState {
                redemption_price_at_last_update: FIXED_POINT_ONE / 2,
                redemption_rate_per_millisecond: FIXED_POINT_ONE,
                controller_integral_term: 0,
                last_updated_at: 0,
            },
            pending: Some(Pending::Initialize),
        };
        assert_eq!(
            engine_with(args()),
            Response::set_state(StablecoinState::Engine(expected).encode()).send_sync(
                clock(),
                &clock_core::Message::ReadTimestamp { granularity: 1 }
            ),
        );
    }

    #[test]
    fn the_clock_reply_stamps_the_timestamps_and_announces() {
        let mut parked = engine_at(0);
        parked.pending = Some(Pending::Initialize);
        let mut expected = engine_at(7_000);
        expected.pending = None;
        assert_eq!(
            run(
                engine(),
                Some(clock()),
                false,
                Some(&StablecoinState::Engine(parked)),
                &clock_reply(7_000),
            ),
            Response::set_state(StablecoinState::Engine(expected).encode()).event(
                Initialized {
                    engine: engine_id(),
                    stablecoin_definition: definition_id(),
                }
                .to_program_event()
            ),
        );
    }

    #[test]
    #[should_panic(expected = "only the program's own account may initialize it")]
    fn initialize_at_another_account_panics() {
        let _ = run(stablecoin(OWNER), None, true, None, &Message::from(args()));
    }

    #[test]
    #[should_panic(expected = "only the program's own account may initialize it")]
    fn an_unsigned_initialize_panics() {
        let _ = run(program_account(), None, false, None, &Message::from(args()));
    }

    #[test]
    #[should_panic(expected = "only the program's own account may initialize it")]
    fn a_nested_initialize_panics() {
        let _ = run(
            program_account(),
            Some(outsider(OWNER)),
            true,
            None,
            &Message::from(args()),
        );
    }

    #[test]
    #[should_panic(expected = "only the program account may send this")]
    fn create_stablecoin_token_from_another_actor_panics() {
        let _ = run(
            stablecoin(surplus_id()),
            Some(stablecoin(OWNER)),
            true,
            None,
            &Internal::CreateStablecoinToken {
                name: "x".to_owned(),
                token_program_id: TOKEN,
            },
        );
    }

    #[test]
    #[should_panic(expected = "only the program account may send this")]
    fn initialize_engine_from_another_actor_panics() {
        let _ = run(
            engine(),
            Some(stablecoin(OWNER)),
            false,
            None,
            &Internal::InitializeEngine(args()),
        );
    }

    #[test]
    #[should_panic(expected = "the engine is already initialized")]
    fn initializing_an_initialized_engine_panics() {
        let _ = run(
            engine(),
            Some(program_account()),
            false,
            Some(&StablecoinState::Engine(engine_at(1_000))),
            &Internal::InitializeEngine(args()),
        );
    }

    fn engine_with_changed_args(change: impl FnOnce(&mut InitializeArgs)) -> Response {
        let mut changed = args();
        change(&mut changed);
        engine_with(changed)
    }

    #[test]
    #[should_panic(expected = "initial_stability_fee_per_millisecond below FIXED_POINT_ONE")]
    fn rejects_stability_fee_below_one() {
        let _ = engine_with_changed_args(|a| {
            a.initial_stability_fee_per_millisecond = FIXED_POINT_ONE - 1;
        });
    }

    #[test]
    #[should_panic(expected = "initial_stability_fee_per_millisecond above sane upper bound")]
    fn rejects_stability_fee_above_the_cap() {
        let _ = engine_with_changed_args(|a| {
            a.initial_stability_fee_per_millisecond = MAXIMUM_STABILITY_FEE_PER_MILLISECOND + 1;
        });
    }

    #[test]
    #[should_panic(expected = "initial_minimum_collateralization_ratio below 1.1x")]
    fn rejects_collateralization_ratio_too_low() {
        let _ = engine_with_changed_args(|a| {
            a.initial_minimum_collateralization_ratio = FIXED_POINT_ONE;
        });
    }

    #[test]
    #[should_panic(expected = "initial_minimum_collateralization_ratio above 10x")]
    fn rejects_collateralization_ratio_too_high() {
        let _ = engine_with_changed_args(|a| {
            a.initial_minimum_collateralization_ratio = FIXED_POINT_ONE * 11;
        });
    }

    #[test]
    #[should_panic(expected = "controller_proportional_gain out of band")]
    fn rejects_negative_proportional_gain() {
        let _ = engine_with_changed_args(|a| {
            a.initial_controller_proportional_gain = -1;
        });
    }

    #[test]
    #[should_panic(expected = "controller_integral_gain out of band")]
    fn rejects_negative_integral_gain() {
        let _ = engine_with_changed_args(|a| {
            a.initial_controller_integral_gain = -1;
        });
    }

    #[test]
    #[should_panic(expected = "minimum_milliseconds_between_rate_updates below minimum 1ms")]
    fn rejects_zero_rate_update_interval() {
        let _ = engine_with_changed_args(|a| {
            a.minimum_milliseconds_between_rate_updates = 0;
        });
    }

    #[test]
    #[should_panic(
        expected = "minimum_milliseconds_between_rate_updates above maximum 86_400_000ms"
    )]
    fn rejects_rate_update_interval_above_a_day() {
        let _ = engine_with_changed_args(|a| {
            a.minimum_milliseconds_between_rate_updates = 86_400_001;
        });
    }

    #[test]
    #[should_panic(expected = "maximum_oracle_price_age_milliseconds below minimum 1ms")]
    fn rejects_zero_oracle_max_age() {
        let _ = engine_with_changed_args(|a| {
            a.maximum_oracle_price_age_milliseconds = 0;
        });
    }

    #[test]
    #[should_panic(expected = "maximum_oracle_price_age_milliseconds above maximum 86_400_000ms")]
    fn rejects_oracle_max_age_above_a_day() {
        let _ = engine_with_changed_args(|a| {
            a.maximum_oracle_price_age_milliseconds = 86_400_001;
        });
    }

    #[test]
    #[should_panic(
        expected = "initial_redemption_price must be within the §8 redemption-price bounds"
    )]
    fn rejects_zero_redemption_price() {
        let _ = engine_with_changed_args(|a| {
            a.initial_redemption_price = 0;
        });
    }

    #[test]
    #[should_panic(
        expected = "initial_redemption_price must be within the §8 redemption-price bounds"
    )]
    fn rejects_redemption_price_below_the_floor() {
        let _ = engine_with_changed_args(|a| {
            a.initial_redemption_price = MINIMUM_REDEMPTION_PRICE - 1;
        });
    }

    #[test]
    #[should_panic(
        expected = "initial_redemption_price must be within the §8 redemption-price bounds"
    )]
    fn rejects_redemption_price_above_the_ceiling() {
        let _ = engine_with_changed_args(|a| {
            a.initial_redemption_price = MAXIMUM_REDEMPTION_PRICE + 1;
        });
    }

    #[test]
    #[should_panic(expected = "stablecoin_name must be 1 to 64 bytes")]
    fn rejects_empty_name() {
        let _ = engine_with_changed_args(|a| {
            a.stablecoin_name = String::new();
        });
    }

    #[test]
    #[should_panic(expected = "stablecoin_name must be 1 to 64 bytes")]
    fn rejects_name_above_the_maximum() {
        let _ = engine_with_changed_args(|a| {
            a.stablecoin_name = "x".repeat(MAX_NAME_LEN + 1);
        });
    }

    #[test]
    #[should_panic(expected = "Market price oracle program must not be the default owner")]
    fn rejects_a_zero_oracle_program_id() {
        let _ = engine_with_changed_args(|a| {
            a.market_price_oracle_program_id = AccountId::new([0; 32]);
        });
    }

    #[test]
    #[should_panic(expected = "Debt limits require 0 < minimum_debt_amount <= debt_ceiling")]
    fn rejects_zero_minimum_debt() {
        let _ = engine_with_changed_args(|a| {
            a.minimum_debt_amount = 0;
            a.debt_ceiling = 100;
        });
    }

    #[test]
    #[should_panic(expected = "Debt limits require 0 < minimum_debt_amount <= debt_ceiling")]
    fn rejects_minimum_debt_above_the_ceiling() {
        let _ = engine_with_changed_args(|a| {
            a.minimum_debt_amount = 10;
            a.debt_ceiling = 9;
        });
    }

    #[test]
    fn a_name_of_exactly_the_maximum_length_is_accepted() {
        let _ = engine_with_changed_args(|a| a.stablecoin_name = "x".repeat(MAX_NAME_LEN));
    }

    #[test]
    fn a_minimum_debt_equal_to_the_ceiling_is_accepted() {
        let _ = engine_with_changed_args(|a| {
            a.minimum_debt_amount = 100;
            a.debt_ceiling = 100;
        });
    }
}
