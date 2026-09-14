//! Stateful user journeys across the stablecoin FFI and native program layers.
//!
//! These tests deliberately keep an account store in memory. Each plan is built
//! by the real FFI operation, executed by the native program handler, and fed
//! back into the next read. That makes the tests sensitive to mismatches between
//! read-side projections, controller quotes, serialized plans, and persisted
//! account data.

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::ProgramId,
};
use serde_json::{json, Value};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda,
    math::{FIXED_POINT_ONE, MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS},
    Instruction, ProtocolParameters, RedemptionPriceState, StabilityFeeAccumulator,
};
use stablecoin_program::{
    accrue_stability_fee::accrue_stability_fee, refresh_globals::refresh_globals,
    update_redemption_rate::update_redemption_rate,
};
use twap_oracle_core::OraclePriceAccount;

use super::{
    accrue_stability_fee_plan, current_global_state, decode_protocol_parameters,
    decode_redemption_price_state, decode_stability_fee_accumulator, redemption_rate_update_quote,
    refresh_globals_plan, update_redemption_rate_plan, AccrueStabilityFeePlanRequest,
    CurrentGlobalStateRequest, DecodeProtocolParametersRequest, DecodeRedemptionPriceStateRequest,
    DecodeStabilityFeeAccumulatorRequest, RedemptionRateUpdateQuoteRequest,
    RefreshGlobalsPlanRequest, StablecoinResult, UpdateRedemptionRatePlanRequest,
};
use crate::{
    account::{account_id_hex, account_read, program_id_bytes},
    AccountRead,
};

const STABLECOIN_PROGRAM_ID: ProgramId = [0x11_u32; 8];
const TOKEN_PROGRAM_ID: ProgramId = [0x22_u32; 8];
const ORACLE_PROGRAM_ID: ProgramId = [0x33_u32; 8];
const CLOCK_PROGRAM_ID: ProgramId = [0x44_u32; 8];

const START: u64 = 1_000;
const DUE: u64 = START + 1_800_000;
const LATER: u64 = DUE + 600_000;
const IDLE: u64 = DUE + MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS + 17;
const MIN_UPDATE_INTERVAL: u64 = 300_000;
const MAX_ORACLE_AGE: u64 = 900_000;

fn id(seed: u8) -> AccountId {
    AccountId::new([seed; 32])
}

fn program_id_hex() -> String {
    hex::encode(program_id_bytes(STABLECOIN_PROGRAM_ID))
}

fn account(owner: ProgramId, data: Data) -> Account {
    Account {
        program_owner: owner,
        balance: 0,
        data,
        nonce: Nonce(0),
    }
}

fn missing_read(account_id: AccountId) -> AccountRead {
    AccountRead {
        id: account_id_hex(account_id),
        status: String::from("not_found"),
        account: None,
    }
}

fn expect_error(result: StablecoinResult, expected: &str) {
    match result {
        Ok(value) => panic!("expected {expected}, got {value}"),
        Err(error) => assert_eq!(error.code(), expected),
    }
}

fn decode_instruction(value: &Value) -> Instruction {
    let words: Vec<u32> = serde_json::from_value(value["instruction"].clone())
        .expect("plan instruction must contain u32 words");
    risc0_zkvm::serde::from_slice::<Instruction, u32>(&words).expect("plan instruction must decode")
}

fn assert_plan(plan: &Value, account_ids: &[AccountId], instruction_word: u32) {
    let expected_ids: Vec<String> = account_ids.iter().copied().map(account_id_hex).collect();
    let mut expected_signers = vec![false; account_ids.len()];
    expected_signers[0] = true;
    assert_eq!(plan["programId"], program_id_hex());
    assert_eq!(plan["accountIds"], json!(expected_ids));
    assert_eq!(plan["signingRequirements"], json!(expected_signers));
    assert_eq!(plan["instruction"], json!([instruction_word]));
}

fn exact_u128(value: &Value, key: &str) -> u128 {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} must be an exact decimal string"))
        .parse::<u128>()
        .unwrap_or_else(|_| panic!("{key} must fit u128"))
}

fn exact_i128(value: &Value, key: &str) -> i128 {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} must be an exact decimal string"))
        .parse::<i128>()
        .unwrap_or_else(|_| panic!("{key} must fit i128"))
}

#[derive(Clone)]
struct JourneyState {
    parameters: ProtocolParameters,
    accumulator: StabilityFeeAccumulator,
    redemption: RedemptionPriceState,
    oracle: OraclePriceAccount,
    clock: ClockAccountData,
}

impl JourneyState {
    fn new() -> Self {
        Self {
            parameters: ProtocolParameters {
                admin_account_id: id(1),
                freeze_authority_account_id: id(2),
                stablecoin_definition_id: id(3),
                collateral_definition_id: id(4),
                market_price_oracle_id: id(5),
                stability_fee_per_millisecond: FIXED_POINT_ONE + 1_500_000_000_000_000,
                controller_proportional_gain: FIXED_POINT_ONE as i128,
                controller_integral_gain: FIXED_POINT_ONE as i128 / 100,
                minimum_collateralization_ratio: FIXED_POINT_ONE * 3 / 2,
                minimum_milliseconds_between_rate_updates: MIN_UPDATE_INTERVAL,
                maximum_oracle_price_age_milliseconds: MAX_ORACLE_AGE,
                is_frozen: false,
            },
            accumulator: StabilityFeeAccumulator {
                accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
                last_accrued_at: START,
            },
            redemption: RedemptionPriceState {
                redemption_price_at_last_update: FIXED_POINT_ONE,
                redemption_rate_per_millisecond: FIXED_POINT_ONE,
                controller_integral_term: 0,
                last_updated_at: START,
            },
            oracle: OraclePriceAccount {
                base_asset: id(3),
                quote_asset: id(4),
                price: FIXED_POINT_ONE / 2,
                timestamp: DUE,
                source_id: id(6),
                confidence_interval: 0,
            },
            clock: ClockAccountData {
                block_id: 1,
                timestamp: DUE,
            },
        }
    }

    fn caller_id() -> AccountId {
        id(15)
    }

    fn protocol_id() -> AccountId {
        compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)
    }

    fn accumulator_id() -> AccountId {
        compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)
    }

    fn redemption_id() -> AccountId {
        compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)
    }

    fn protocol_read(&self) -> AccountRead {
        account_read(
            Self::protocol_id(),
            &account(STABLECOIN_PROGRAM_ID, Data::from(&self.parameters)),
        )
    }

    fn accumulator_read(&self) -> AccountRead {
        account_read(
            Self::accumulator_id(),
            &account(STABLECOIN_PROGRAM_ID, Data::from(&self.accumulator)),
        )
    }

    fn redemption_read(&self) -> AccountRead {
        account_read(
            Self::redemption_id(),
            &account(STABLECOIN_PROGRAM_ID, Data::from(&self.redemption)),
        )
    }

    fn oracle_read(&self) -> AccountRead {
        account_read(
            self.parameters.market_price_oracle_id,
            &account(ORACLE_PROGRAM_ID, Data::from(&self.oracle)),
        )
    }

    fn clock_read(&self) -> AccountRead {
        account_read(
            CLOCK_01_PROGRAM_ACCOUNT_ID,
            &account(
                CLOCK_PROGRAM_ID,
                Data::try_from(self.clock.to_bytes()).expect("clock data fits"),
            ),
        )
    }

    fn current_request(&self) -> CurrentGlobalStateRequest {
        CurrentGlobalStateRequest {
            stablecoin_program_id: program_id_hex(),
            protocol_parameters: self.protocol_read(),
            stability_fee_accumulator: self.accumulator_read(),
            redemption_price_state: self.redemption_read(),
            clock: self.clock_read(),
        }
    }

    fn quote_request(&self) -> RedemptionRateUpdateQuoteRequest {
        RedemptionRateUpdateQuoteRequest {
            stablecoin_program_id: program_id_hex(),
            protocol_parameters: self.protocol_read(),
            redemption_price_state: self.redemption_read(),
            market_price_oracle: self.oracle_read(),
            clock: self.clock_read(),
        }
    }

    fn accrue_request(&self) -> AccrueStabilityFeePlanRequest {
        AccrueStabilityFeePlanRequest {
            stablecoin_program_id: program_id_hex(),
            caller_id: account_id_hex(Self::caller_id()),
            protocol_parameters: self.protocol_read(),
            stability_fee_accumulator: self.accumulator_read(),
            clock: self.clock_read(),
        }
    }

    fn update_request(&self) -> UpdateRedemptionRatePlanRequest {
        UpdateRedemptionRatePlanRequest {
            stablecoin_program_id: program_id_hex(),
            caller_id: account_id_hex(Self::caller_id()),
            protocol_parameters: self.protocol_read(),
            redemption_price_state: self.redemption_read(),
            market_price_oracle: self.oracle_read(),
            clock: self.clock_read(),
        }
    }

    fn refresh_request(&self) -> RefreshGlobalsPlanRequest {
        RefreshGlobalsPlanRequest {
            stablecoin_program_id: program_id_hex(),
            caller_id: account_id_hex(Self::caller_id()),
            protocol_parameters: self.protocol_read(),
            stability_fee_accumulator: self.accumulator_read(),
            redemption_price_state: self.redemption_read(),
            market_price_oracle: self.oracle_read(),
            clock: self.clock_read(),
        }
    }

    fn caller(&self) -> AccountWithMetadata {
        AccountWithMetadata::new(Account::default(), true, Self::caller_id())
    }

    fn protocol_account(&self) -> AccountWithMetadata {
        AccountWithMetadata::new(
            account(STABLECOIN_PROGRAM_ID, Data::from(&self.parameters)),
            false,
            Self::protocol_id(),
        )
    }

    fn accumulator_account(&self) -> AccountWithMetadata {
        AccountWithMetadata::new(
            account(STABLECOIN_PROGRAM_ID, Data::from(&self.accumulator)),
            false,
            Self::accumulator_id(),
        )
    }

    fn redemption_account(&self) -> AccountWithMetadata {
        AccountWithMetadata::new(
            account(STABLECOIN_PROGRAM_ID, Data::from(&self.redemption)),
            false,
            Self::redemption_id(),
        )
    }

    fn oracle_account(&self) -> AccountWithMetadata {
        AccountWithMetadata::new(
            account(ORACLE_PROGRAM_ID, Data::from(&self.oracle)),
            false,
            self.parameters.market_price_oracle_id,
        )
    }

    fn clock_account(&self) -> AccountWithMetadata {
        AccountWithMetadata::new(
            account(
                CLOCK_PROGRAM_ID,
                Data::try_from(self.clock.to_bytes()).expect("clock data fits"),
            ),
            false,
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        )
    }

    fn execute_instruction(&mut self, instruction: Instruction) {
        match instruction {
            Instruction::AccrueStabilityFee => {
                let (posts, calls) = accrue_stability_fee(
                    self.caller(),
                    self.protocol_account(),
                    self.accumulator_account(),
                    self.clock_account(),
                    STABLECOIN_PROGRAM_ID,
                );
                assert!(calls.is_empty());
                self.accumulator = StabilityFeeAccumulator::try_from(
                    &posts.get(2).expect("accrual post state").account().data,
                )
                .expect("accrual output must decode");
            }
            Instruction::UpdateRedemptionRate => {
                let (posts, calls) = update_redemption_rate(
                    self.caller(),
                    self.protocol_account(),
                    self.redemption_account(),
                    self.oracle_account(),
                    self.clock_account(),
                    STABLECOIN_PROGRAM_ID,
                );
                assert!(calls.is_empty());
                self.redemption = RedemptionPriceState::try_from(
                    &posts.get(2).expect("update post state").account().data,
                )
                .expect("update output must decode");
            }
            Instruction::RefreshGlobals => {
                let (posts, calls) = refresh_globals(
                    self.caller(),
                    self.protocol_account(),
                    self.accumulator_account(),
                    self.redemption_account(),
                    self.oracle_account(),
                    self.clock_account(),
                    STABLECOIN_PROGRAM_ID,
                );
                assert!(calls.is_empty());
                self.accumulator = StabilityFeeAccumulator::try_from(
                    &posts
                        .get(2)
                        .expect("refresh accumulator post state")
                        .account()
                        .data,
                )
                .expect("refresh accumulator output must decode");
                self.redemption = RedemptionPriceState::try_from(
                    &posts
                        .get(3)
                        .expect("refresh redemption post state")
                        .account()
                        .data,
                )
                .expect("refresh redemption output must decode");
            }
            other => panic!("unexpected journey instruction: {other:?}"),
        }
    }
}

fn exercise_soft_blocker(
    mut state: JourneyState,
    expected_blocker: &str,
    repair: impl FnOnce(&mut JourneyState),
) -> JourneyState {
    let redemption_before = state.redemption.clone();
    let quote = redemption_rate_update_quote(state.quote_request())
        .expect("a blocked quote is still a successful read");
    assert_eq!(quote["canSubmit"], false);
    assert_eq!(quote["code"], "blocked");
    assert_eq!(quote["errors"][0]["code"], expected_blocker);
    assert!(quote["nextRedemptionRatePerMillisecond"].is_null());
    assert!(quote["nextControllerIntegralTerm"].is_null());

    expect_error(
        update_redemption_rate_plan(state.update_request()),
        expected_blocker,
    );

    let refresh = refresh_globals_plan(state.refresh_request())
        .expect("soft gates must still produce a refresh plan");
    assert_plan(
        &refresh,
        &[
            JourneyState::caller_id(),
            JourneyState::protocol_id(),
            JourneyState::accumulator_id(),
            JourneyState::redemption_id(),
            state.parameters.market_price_oracle_id,
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        3,
    );
    state.execute_instruction(decode_instruction(&refresh));
    assert_eq!(state.accumulator.last_accrued_at, state.clock.timestamp);
    assert_eq!(state.redemption, redemption_before);

    repair(&mut state);
    let ready = redemption_rate_update_quote(state.quote_request())
        .expect("repaired oracle and time must produce a quote");
    assert_eq!(ready["canSubmit"], true);
    assert_eq!(ready["code"], "ready");
    let update = update_redemption_rate_plan(state.update_request())
        .expect("repaired oracle and time must produce an update plan");
    state.execute_instruction(decode_instruction(&update));
    assert_eq!(state.redemption.last_updated_at, state.clock.timestamp);
    state
}

#[test]
fn journey_starts_uninitialized_then_exposes_all_global_reads() {
    let state = JourneyState::new();
    let before = state.clone();

    let missing = CurrentGlobalStateRequest {
        stablecoin_program_id: program_id_hex(),
        protocol_parameters: missing_read(JourneyState::protocol_id()),
        stability_fee_accumulator: missing_read(JourneyState::accumulator_id()),
        redemption_price_state: missing_read(JourneyState::redemption_id()),
        clock: state.clock_read(),
    };
    expect_error(current_global_state(missing), "account_read_failed");

    let missing_quote = RedemptionRateUpdateQuoteRequest {
        stablecoin_program_id: program_id_hex(),
        protocol_parameters: missing_read(JourneyState::protocol_id()),
        redemption_price_state: missing_read(JourneyState::redemption_id()),
        market_price_oracle: state.oracle_read(),
        clock: state.clock_read(),
    };
    expect_error(
        redemption_rate_update_quote(missing_quote),
        "account_read_failed",
    );

    let parameters = decode_protocol_parameters(DecodeProtocolParametersRequest {
        stablecoin_program_id: program_id_hex(),
        protocol_parameters: state.protocol_read(),
    })
    .expect("initialized parameters must decode");
    let accumulator = decode_stability_fee_accumulator(DecodeStabilityFeeAccumulatorRequest {
        stablecoin_program_id: program_id_hex(),
        stability_fee_accumulator: state.accumulator_read(),
    })
    .expect("initialized accumulator must decode");
    let redemption = decode_redemption_price_state(DecodeRedemptionPriceStateRequest {
        stablecoin_program_id: program_id_hex(),
        redemption_price_state: state.redemption_read(),
    })
    .expect("initialized redemption state must decode");
    assert_eq!(
        parameters["stabilityFeePerMillisecond"],
        state.parameters.stability_fee_per_millisecond.to_string()
    );
    assert_eq!(accumulator["lastAccruedAt"], START.to_string());
    assert_eq!(redemption["lastUpdatedAt"], START.to_string());

    let projection =
        current_global_state(state.current_request()).expect("initialized globals must project");
    assert_eq!(projection["projectedAt"], DUE.to_string());
    assert_eq!(projection["lastAccruedAt"], START.to_string());
    assert_eq!(projection["lastUpdatedAt"], START.to_string());

    let quote = redemption_rate_update_quote(state.quote_request()).expect("quote must succeed");
    assert_eq!(quote["canSubmit"], true);
    assert_eq!(quote["code"], "ready");
    assert_eq!(quote["errors"], json!([]));
    assert_eq!(state.parameters, before.parameters);
    assert_eq!(state.accumulator, before.accumulator);
    assert_eq!(state.redemption, before.redemption);
    assert_eq!(state.clock.timestamp, before.clock.timestamp);
}

#[test]
fn journey_projects_fees_then_persists_only_the_accumulator() {
    let mut state = JourneyState::new();
    let redemption_before = state.redemption.clone();
    let projection =
        current_global_state(state.current_request()).expect("initialized globals must project");
    let projected_rate = projection["currentAccumulatedRate"]
        .as_str()
        .expect("projected accumulator must be an exact string")
        .parse::<u128>()
        .expect("projected accumulator must be a u128");
    assert!(projected_rate > state.accumulator.accumulated_rate_at_last_accrual);
    assert_eq!(
        projection["accumulatedRateAtLastAccrual"],
        state
            .accumulator
            .accumulated_rate_at_last_accrual
            .to_string()
    );
    assert_eq!(projection["lastAccruedAt"], START.to_string());
    assert_eq!(projection["projectedAt"], DUE.to_string());
    assert_eq!(state.redemption, redemption_before);

    let plan =
        accrue_stability_fee_plan(state.accrue_request()).expect("accrual plan must be ready");
    assert_plan(
        &plan,
        &[
            JourneyState::caller_id(),
            JourneyState::protocol_id(),
            JourneyState::accumulator_id(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        1,
    );
    assert!(matches!(
        decode_instruction(&plan),
        Instruction::AccrueStabilityFee
    ));
    state.execute_instruction(decode_instruction(&plan));

    assert_eq!(state.accumulator.last_accrued_at, DUE);
    assert_eq!(
        state.accumulator.accumulated_rate_at_last_accrual,
        projected_rate
    );
    assert_eq!(state.redemption, redemption_before);
    let after =
        current_global_state(state.current_request()).expect("post-accrual globals must project");
    assert_eq!(
        after["accumulatedRateAtLastAccrual"],
        projected_rate.to_string()
    );
    assert_eq!(after["currentAccumulatedRate"], projected_rate.to_string());
    assert_eq!(after["lastAccruedAt"], DUE.to_string());
    assert_eq!(
        after["redemptionPriceAtLastUpdate"],
        redemption_before
            .redemption_price_at_last_update
            .to_string()
    );

    let repeat = accrue_stability_fee_plan(state.accrue_request())
        .expect("same-time accrual must remain available");
    state.execute_instruction(decode_instruction(&repeat));
    assert_eq!(
        state.accumulator.accumulated_rate_at_last_accrual,
        projected_rate
    );
    assert_eq!(state.accumulator.last_accrued_at, DUE);
}

#[test]
fn journey_quotes_and_persists_successive_controller_updates() {
    let mut state = JourneyState::new();
    let accumulator_before = state.accumulator.clone();

    let first_quote = redemption_rate_update_quote(state.quote_request())
        .expect("first controller quote must be ready");
    assert_eq!(first_quote["canSubmit"], true);
    assert_eq!(first_quote["code"], "ready");
    assert_eq!(
        first_quote["elapsedMilliseconds"],
        (DUE - START).to_string()
    );

    let first_plan = update_redemption_rate_plan(state.update_request())
        .expect("first controller plan must be ready");
    assert_plan(
        &first_plan,
        &[
            JourneyState::caller_id(),
            JourneyState::protocol_id(),
            JourneyState::redemption_id(),
            state.parameters.market_price_oracle_id,
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        2,
    );
    assert!(matches!(
        decode_instruction(&first_plan),
        Instruction::UpdateRedemptionRate
    ));
    state.execute_instruction(decode_instruction(&first_plan));

    assert_eq!(
        state.redemption.redemption_price_at_last_update,
        exact_u128(&first_quote, "currentRedemptionPrice")
    );
    assert_eq!(
        state.redemption.redemption_rate_per_millisecond,
        exact_u128(&first_quote, "nextRedemptionRatePerMillisecond")
    );
    assert_eq!(
        state.redemption.controller_integral_term,
        exact_i128(&first_quote, "nextControllerIntegralTerm")
    );
    assert_eq!(state.redemption.last_updated_at, DUE);
    assert_eq!(state.accumulator, accumulator_before);

    state.clock.timestamp = LATER;
    state.oracle.timestamp = LATER;
    state.oracle.price = FIXED_POINT_ONE * 3 / 2;
    let second_quote = redemption_rate_update_quote(state.quote_request())
        .expect("second controller quote must be ready");
    assert_eq!(second_quote["canSubmit"], true);
    assert_eq!(
        second_quote["elapsedMilliseconds"],
        (LATER - DUE).to_string()
    );

    let second_plan = update_redemption_rate_plan(state.update_request())
        .expect("second controller plan must be ready");
    assert!(matches!(
        decode_instruction(&second_plan),
        Instruction::UpdateRedemptionRate
    ));
    state.execute_instruction(decode_instruction(&second_plan));
    assert_eq!(
        state.redemption.redemption_price_at_last_update,
        exact_u128(&second_quote, "currentRedemptionPrice")
    );
    assert_eq!(
        state.redemption.redemption_rate_per_millisecond,
        exact_u128(&second_quote, "nextRedemptionRatePerMillisecond")
    );
    assert_eq!(
        state.redemption.controller_integral_term,
        exact_i128(&second_quote, "nextControllerIntegralTerm")
    );
    assert_eq!(state.redemption.last_updated_at, LATER);
    assert_ne!(
        state.redemption.controller_integral_term,
        exact_i128(&first_quote, "nextControllerIntegralTerm")
    );
    assert_eq!(state.accumulator, accumulator_before);

    let mut combined = JourneyState::new();
    let combined_plan = refresh_globals_plan(combined.refresh_request())
        .expect("combined refresh plan must be ready");
    assert_plan(
        &combined_plan,
        &[
            JourneyState::caller_id(),
            JourneyState::protocol_id(),
            JourneyState::accumulator_id(),
            JourneyState::redemption_id(),
            combined.parameters.market_price_oracle_id,
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        3,
    );
    combined.execute_instruction(decode_instruction(&combined_plan));

    let mut standalone = JourneyState::new();
    let accrue = accrue_stability_fee_plan(standalone.accrue_request())
        .expect("standalone accrual plan must be ready");
    standalone.execute_instruction(decode_instruction(&accrue));
    let update = update_redemption_rate_plan(standalone.update_request())
        .expect("standalone update plan must be ready");
    standalone.execute_instruction(decode_instruction(&update));
    assert_eq!(combined.accumulator, standalone.accumulator);
    assert_eq!(combined.redemption, standalone.redemption);
}

#[test]
fn journey_recovers_from_blocked_updates_with_fee_only_refreshes() {
    let stale = exercise_soft_blocker(
        {
            let mut state = JourneyState::new();
            state.oracle.timestamp = DUE - MAX_ORACLE_AGE - 1;
            state
        },
        "oracle_stale",
        |state| state.oracle.timestamp = state.clock.timestamp,
    );
    assert_eq!(stale.redemption.last_updated_at, DUE);

    let zero_price = exercise_soft_blocker(
        {
            let mut state = JourneyState::new();
            state.oracle.price = 0;
            state
        },
        "oracle_price_zero",
        |state| state.oracle.price = FIXED_POINT_ONE / 2,
    );
    assert_eq!(zero_price.redemption.last_updated_at, DUE);

    let too_soon = exercise_soft_blocker(
        {
            let mut state = JourneyState::new();
            state.clock.timestamp = START + 100;
            state.oracle.timestamp = state.clock.timestamp;
            state
        },
        "rate_update_too_soon",
        |state| {
            state.clock.timestamp = DUE;
            state.oracle.timestamp = DUE;
        },
    );
    assert_eq!(too_soon.redemption.last_updated_at, DUE);

    let mut combined_blockers = JourneyState::new();
    combined_blockers.oracle.timestamp = DUE - MAX_ORACLE_AGE - 1;
    combined_blockers.oracle.price = 0;
    combined_blockers.clock.timestamp = DUE;
    combined_blockers.redemption.last_updated_at = DUE - 100;
    assert_eq!(
        redemption_rate_update_quote(combined_blockers.quote_request())
            .expect("combined blockers are a successful quote")["errors"][0]["code"],
        "oracle_stale"
    );
    expect_error(
        update_redemption_rate_plan(combined_blockers.update_request()),
        "oracle_stale",
    );

    let mut exact_interval = JourneyState::new();
    exact_interval.clock.timestamp = START + MIN_UPDATE_INTERVAL;
    exact_interval.oracle.timestamp = exact_interval.clock.timestamp;
    assert_eq!(
        redemption_rate_update_quote(exact_interval.quote_request())
            .expect("exact interval is due")["canSubmit"],
        true
    );
    let mut exact_age = JourneyState::new();
    exact_age.oracle.timestamp = DUE - MAX_ORACLE_AGE;
    assert_eq!(
        redemption_rate_update_quote(exact_age.quote_request()).expect("exact oracle age is fresh")
            ["canSubmit"],
        true
    );

    let frozen = exercise_soft_blocker(
        {
            let mut state = JourneyState::new();
            state.parameters.is_frozen = true;
            state.oracle.price = 0;
            state
        },
        "oracle_price_zero",
        |state| state.oracle.price = FIXED_POINT_ONE / 2,
    );
    assert_eq!(frozen.redemption.last_updated_at, DUE);
    assert!(frozen.parameters.is_frozen);
}

#[test]
fn journey_rechecks_state_after_a_quote_becomes_stale() {
    let mut state = JourneyState::new();
    let first_quote =
        redemption_rate_update_quote(state.quote_request()).expect("initial quote must be ready");
    assert_eq!(first_quote["canSubmit"], true);

    // A different keeper submits the update represented by the first quote.
    // This changes the persisted anchor and controller state before the user
    // attempts to submit their own previously observed quote.
    let other_keeper_plan = update_redemption_rate_plan(state.update_request())
        .expect("other keeper update must be ready");
    state.execute_instruction(decode_instruction(&other_keeper_plan));
    let updated_anchor = state.redemption.redemption_price_at_last_update;
    assert_eq!(
        updated_anchor,
        exact_u128(&first_quote, "currentRedemptionPrice")
    );

    // Time advances past the oracle freshness window while the user still has
    // the old ready quote. The module must read current accounts again and stop
    // before submission; it cannot use a quote as cached authorization.
    state.clock.timestamp = DUE + MAX_ORACLE_AGE + 1;
    let stale_quote = redemption_rate_update_quote(state.quote_request())
        .expect("stale oracle is a successful blocked quote");
    assert_eq!(stale_quote["canSubmit"], false);
    assert_eq!(stale_quote["errors"][0]["code"], "oracle_stale");
    expect_error(
        update_redemption_rate_plan(state.update_request()),
        "oracle_stale",
    );
    assert_eq!(
        state.redemption.redemption_price_at_last_update,
        updated_anchor
    );

    // Refreshing the same oracle account makes a fresh quote valid. Its price
    // reflects the intervening keeper update and the elapsed time, so it is a
    // distinct quote and is the only one executed below.
    state.oracle.timestamp = state.clock.timestamp;
    let fresh_quote = redemption_rate_update_quote(state.quote_request())
        .expect("refreshed oracle must produce a ready quote");
    assert_eq!(fresh_quote["canSubmit"], true);
    assert_ne!(
        fresh_quote["currentRedemptionPrice"],
        first_quote["currentRedemptionPrice"]
    );
    let retry =
        update_redemption_rate_plan(state.update_request()).expect("fresh retry must be ready");
    state.execute_instruction(decode_instruction(&retry));
    assert_eq!(
        state.redemption.redemption_price_at_last_update,
        exact_u128(&fresh_quote, "currentRedemptionPrice")
    );
    assert_eq!(
        state.redemption.redemption_rate_per_millisecond,
        exact_u128(&fresh_quote, "nextRedemptionRatePerMillisecond")
    );
    assert_eq!(state.redemption.last_updated_at, state.clock.timestamp);
}

#[test]
fn journey_recovers_from_bad_callers_and_account_reads_without_retries() {
    let mut state = JourneyState::new();
    let accumulator_before = state.accumulator.clone();

    let mut malformed_caller = state.accrue_request();
    malformed_caller.caller_id = String::from("not-an-account");
    expect_error(
        accrue_stability_fee_plan(malformed_caller),
        "invalid_account_id",
    );
    assert_eq!(state.accumulator, accumulator_before);

    let mut missing_parameters = state.accrue_request();
    missing_parameters.protocol_parameters = missing_read(JourneyState::protocol_id());
    expect_error(
        accrue_stability_fee_plan(missing_parameters),
        "account_read_failed",
    );
    assert_eq!(state.accumulator, accumulator_before);

    let mut private_accumulator = state.accrue_request();
    if let Some(account) = private_accumulator
        .stability_fee_accumulator
        .account
        .as_mut()
    {
        account.program_owner = hex::encode(program_id_bytes(TOKEN_PROGRAM_ID));
    }
    expect_error(
        accrue_stability_fee_plan(private_accumulator),
        "stablecoin_program_mismatch",
    );
    assert_eq!(state.accumulator, accumulator_before);

    let mut malformed_accumulator = state.accrue_request();
    if let Some(account) = malformed_accumulator
        .stability_fee_accumulator
        .account
        .as_mut()
    {
        account.data = String::from("00");
    }
    expect_error(
        accrue_stability_fee_plan(malformed_accumulator),
        "invalid_stability_fee_accumulator_data",
    );
    assert_eq!(state.accumulator, accumulator_before);

    let valid = accrue_stability_fee_plan(state.accrue_request())
        .expect("corrected account reads must produce a plan");
    assert_plan(
        &valid,
        &[
            JourneyState::caller_id(),
            JourneyState::protocol_id(),
            JourneyState::accumulator_id(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        1,
    );
    state.execute_instruction(decode_instruction(&valid));
    assert_eq!(state.accumulator.last_accrued_at, DUE);
    assert!(
        state.accumulator.accumulated_rate_at_last_accrual
            > accumulator_before.accumulated_rate_at_last_accrual
    );
}

#[test]
fn journey_survives_long_idle_periods_and_preserves_exact_values() {
    let mut at_clamp = JourneyState::new();
    at_clamp.clock.timestamp = START + MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS;
    let clamp_projection = current_global_state(at_clamp.current_request())
        .expect("projection at the compounding boundary must succeed");

    let mut beyond_clamp = at_clamp.clone();
    beyond_clamp.clock.timestamp = IDLE;
    let beyond_projection = current_global_state(beyond_clamp.current_request())
        .expect("projection beyond the compounding boundary must succeed");
    assert_eq!(
        clamp_projection["currentAccumulatedRate"],
        beyond_projection["currentAccumulatedRate"]
    );
    assert_eq!(
        clamp_projection["currentRedemptionPrice"],
        beyond_projection["currentRedemptionPrice"]
    );
    assert_eq!(
        clamp_projection["projectedAt"],
        (START + MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS).to_string()
    );
    assert_eq!(beyond_projection["projectedAt"], IDLE.to_string());

    let mut idle = JourneyState::new();
    idle.clock.timestamp = IDLE;
    idle.oracle.timestamp = IDLE;
    let idle_quote = redemption_rate_update_quote(idle.quote_request())
        .expect("fresh oracle after a long idle period must produce a quote");
    assert_eq!(idle_quote["canSubmit"], true);
    assert_eq!(
        idle_quote["elapsedMilliseconds"],
        (IDLE - START).to_string()
    );
    assert_eq!(
        idle_quote["currentRedemptionPrice"],
        beyond_projection["currentRedemptionPrice"]
    );
    assert!(
        exact_u128(&idle_quote, "elapsedMilliseconds")
            > u128::from(MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS)
    );

    let refresh =
        refresh_globals_plan(idle.refresh_request()).expect("long-idle refresh plan must be ready");
    assert_plan(
        &refresh,
        &[
            JourneyState::caller_id(),
            JourneyState::protocol_id(),
            JourneyState::accumulator_id(),
            JourneyState::redemption_id(),
            idle.parameters.market_price_oracle_id,
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        3,
    );
    idle.execute_instruction(decode_instruction(&refresh));
    assert_eq!(idle.accumulator.last_accrued_at, IDLE);
    assert_eq!(idle.redemption.last_updated_at, IDLE);
    let after_refresh = current_global_state(idle.current_request())
        .expect("post-refresh state must remain readable");
    assert_eq!(
        after_refresh["currentAccumulatedRate"],
        idle.accumulator
            .accumulated_rate_at_last_accrual
            .to_string()
    );
    assert_eq!(
        after_refresh["currentRedemptionPrice"],
        idle.redemption.redemption_price_at_last_update.to_string()
    );
    assert_eq!(after_refresh["projectedAt"], IDLE.to_string());

    let mut exact = JourneyState::new();
    exact.clock.timestamp = START;
    exact.oracle.timestamp = START;
    exact.parameters.stability_fee_per_millisecond = FIXED_POINT_ONE;
    exact.accumulator.accumulated_rate_at_last_accrual = u128::MAX;
    exact.redemption.redemption_price_at_last_update = u128::MAX;
    exact.redemption.controller_integral_term = i128::MIN;
    let exact_projection = current_global_state(exact.current_request())
        .expect("maximum anchors must cross the projection boundary");
    assert_eq!(
        exact_projection["accumulatedRateAtLastAccrual"],
        u128::MAX.to_string()
    );
    assert_eq!(
        exact_projection["currentAccumulatedRate"],
        u128::MAX.to_string()
    );
    assert_eq!(
        exact_projection["redemptionPriceAtLastUpdate"],
        u128::MAX.to_string()
    );
    assert_eq!(
        exact_projection["currentRedemptionPrice"],
        u128::MAX.to_string()
    );
    let exact_redemption = decode_redemption_price_state(DecodeRedemptionPriceStateRequest {
        stablecoin_program_id: program_id_hex(),
        redemption_price_state: exact.redemption_read(),
    })
    .expect("maximum signed state must decode");
    assert_eq!(
        exact_redemption["controllerIntegralTerm"],
        i128::MIN.to_string()
    );
}
