use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::ProgramId,
};
use serde_json::{json, Value};
use stablecoin_core::{
    compute_position_pda, compute_position_vault_pda, compute_protocol_parameters_pda,
    compute_redemption_price_state_pda, compute_stability_fee_accumulator_pda,
    compute_stablecoin_definition_pda, compute_stablecoin_definition_pda_seed,
    math::{
        compute_current_accumulated_rate, mul_div_ceil, FIXED_POINT_ONE,
        MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
    },
    Instruction, Position, ProtocolParameters, RedemptionPriceState, StabilityFeeAccumulator,
    RATE_DELTA_CLAMP,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OraclePriceAccount;

use super::{
    debt::{checked_accumulator, checked_debt_delta},
    generate_debt_plan, GenerateDebtPlanRequest,
};
use crate::{
    account::{account_id_hex, account_read, decode_account, program_id_bytes},
    AccountRead,
};

const PROGRAM: ProgramId = [0x11; 8];
const TOKEN_PROGRAM: ProgramId = [0x22; 8];
const START: u64 = 1_000;
const NONCE: u64 = u64::MAX;
fn id(seed: u8) -> AccountId {
    AccountId::new([seed; 32])
}
fn read(account_id: AccountId, owner: ProgramId, data: Data) -> AccountRead {
    account_read(
        account_id,
        &Account {
            program_owner: owner,
            data,
            balance: 0,
            nonce: Nonce(0),
        },
    )
}
struct Fixture {
    position: Position,
    parameters: ProtocolParameters,
    accumulator: StabilityFeeAccumulator,
    redemption: RedemptionPriceState,
    oracle: OraclePriceAccount,
    supply: u128,
    balance: u128,
    now: u64,
}
impl Fixture {
    fn new(collateral: u128, debt: u128) -> Self {
        let position_id = compute_position_pda(PROGRAM, id(20), NONCE);
        Self {
            position: Position {
                owner_account_id: id(20),
                position_nonce: NONCE,
                vault_account_id: compute_position_vault_pda(PROGRAM, position_id),
                collateral_amount: collateral,
                normalized_debt_amount: debt,
                opened_at: START,
            },
            parameters: ProtocolParameters {
                admin_account_id: id(1),
                freeze_authority_account_id: id(2),
                stablecoin_definition_id: compute_stablecoin_definition_pda(PROGRAM),
                collateral_definition_id: id(4),
                market_price_oracle_id: id(5),
                stability_fee_per_millisecond: FIXED_POINT_ONE,
                controller_proportional_gain: 0,
                controller_integral_gain: 0,
                minimum_collateralization_ratio: FIXED_POINT_ONE * 11 / 10,
                minimum_milliseconds_between_rate_updates: 86_400_000,
                maximum_oracle_price_age_milliseconds: 50,
                is_frozen: false,
            },
            accumulator: StabilityFeeAccumulator {
                accumulated_rate_at_last_accrual: FIXED_POINT_ONE * 3 / 2,
                last_accrued_at: START,
            },
            redemption: RedemptionPriceState {
                redemption_price_at_last_update: FIXED_POINT_ONE,
                redemption_rate_per_millisecond: FIXED_POINT_ONE,
                controller_integral_term: 0,
                last_updated_at: START,
            },
            oracle: OraclePriceAccount {
                base_asset: compute_stablecoin_definition_pda(PROGRAM),
                quote_asset: id(4),
                price: 0,
                timestamp: START,
                source_id: id(6),
                confidence_interval: 0,
            },
            supply: 100,
            balance: 5,
            now: START,
        }
    }

    fn request(&self, amount: Value) -> GenerateDebtPlanRequest {
        let definition_id = compute_stablecoin_definition_pda(PROGRAM);
        GenerateDebtPlanRequest {
            stablecoin_program_id: hex::encode(program_id_bytes(PROGRAM)),
            owner_id: account_id_hex(id(20)),
            position_nonce: NONCE.to_string(),
            amount,
            user_stablecoin_holding_id: account_id_hex(id(21)),
            position: read(
                compute_position_pda(PROGRAM, id(20), NONCE),
                PROGRAM,
                Data::from(&self.position),
            ),
            stablecoin_definition: read(
                definition_id,
                TOKEN_PROGRAM,
                Data::from(&TokenDefinition::Fungible {
                    name: String::from("Stablecoin"),
                    total_supply: self.supply,
                    metadata_id: None,
                    authority: Some(definition_id),
                }),
            ),
            user_stablecoin_holding: read(
                id(21),
                TOKEN_PROGRAM,
                Data::from(&TokenHolding::Fungible {
                    definition_id,
                    balance: self.balance,
                }),
            ),
            stability_fee_accumulator: read(
                compute_stability_fee_accumulator_pda(PROGRAM),
                PROGRAM,
                Data::from(&self.accumulator),
            ),
            redemption_price_state: read(
                compute_redemption_price_state_pda(PROGRAM),
                PROGRAM,
                Data::from(&self.redemption),
            ),
            market_price_oracle: read(id(5), [0x33; 8], Data::from(&self.oracle)),
            protocol_parameters: read(
                compute_protocol_parameters_pda(PROGRAM),
                PROGRAM,
                Data::from(&self.parameters),
            ),
            clock: read(
                CLOCK_01_PROGRAM_ACCOUNT_ID,
                [0x44; 8],
                Data::try_from(
                    ClockAccountData {
                        block_id: 1,
                        timestamp: self.now,
                    }
                    .to_bytes(),
                )
                .expect("clock fits"),
            ),
        }
    }
}
fn error(request: GenerateDebtPlanRequest, expected: &str) {
    assert_eq!(
        generate_debt_plan(request)
            .expect_err("preflight rejection")
            .code(),
        expected
    );
}
fn execute(fixture: &Fixture, amount: u128, expected_delta: u128) {
    let request = fixture.request(json!(amount.to_string()));
    let plan = generate_debt_plan(request.clone()).expect("accepted plan");
    let reads = [
        &request.position,
        &request.stablecoin_definition,
        &request.user_stablecoin_holding,
        &request.stability_fee_accumulator,
        &request.redemption_price_state,
        &request.market_price_oracle,
        &request.protocol_parameters,
        &request.clock,
    ];
    let ids: Vec<String> = serde_json::from_value(plan["accountIds"].clone()).expect("ids");
    let signers: Vec<bool> =
        serde_json::from_value(plan["signingRequirements"].clone()).expect("signers");
    let inputs: Vec<_> = ids
        .iter()
        .zip(signers)
        .map(|(account_id, signer)| {
            let (account_id, account) = if account_id == &request.owner_id {
                (id(20), Account::default())
            } else {
                decode_account(
                    reads
                        .iter()
                        .find(|read| &read.id == account_id)
                        .expect("planned read"),
                )
                .expect("account")
            };
            AccountWithMetadata::new(account, signer, account_id)
        })
        .collect();
    let [owner,position,definition,destination,accumulator,redemption,oracle,parameters,clock]:[_;9]=inputs.try_into().expect("nine inputs");
    assert!(!definition.is_authorized);
    assert!(!destination.is_authorized);
    let words: Vec<u32> = serde_json::from_value(plan["instruction"].clone()).expect("words");
    let Instruction::GenerateDebt { amount } =
        risc0_zkvm::serde::from_slice(&words).expect("instruction")
    else {
        panic!("expected borrowing");
    };
    let (posts, calls) = stablecoin_program::generate_debt::generate_debt(
        owner,
        position,
        definition,
        destination,
        accumulator,
        redemption,
        oracle,
        parameters,
        clock,
        PROGRAM,
        amount,
    );
    let actual =
        Position::try_from(&posts.get(1).expect("position post").account().data).expect("position");
    let mut expected = fixture.position.clone();
    expected.normalized_debt_amount += expected_delta;
    assert_eq!(actual, expected);
    let [call] = calls.as_slice() else {
        panic!("one mint");
    };
    assert_eq!(call.program_id, TOKEN_PROGRAM);
    assert_eq!(
        call.pda_seeds,
        vec![compute_stablecoin_definition_pda_seed()]
    );
    let token_core::Instruction::Mint { amount_to_mint } =
        risc0_zkvm::serde::from_slice(&call.instruction_data).expect("mint instruction")
    else {
        panic!("expected mint");
    };
    assert_eq!(amount_to_mint, amount);
    let [definition, destination] = call.pre_states.as_slice() else {
        panic!("two mint accounts");
    };
    assert!(definition.is_authorized);
    assert!(!destination.is_authorized);
    assert_eq!(
        definition.account_id,
        compute_stablecoin_definition_pda(PROGRAM)
    );
    let minted = token_program::mint::mint(
        definition.clone(),
        destination.clone(),
        amount_to_mint,
        TOKEN_PROGRAM,
    );
    assert!(
        matches!(TokenDefinition::try_from(&minted.first().expect("definition post").account().data).expect("definition"),
        TokenDefinition::Fungible{total_supply,..} if total_supply==fixture.supply+amount)
    );
    assert_eq!(
        TokenHolding::try_from(&minted.get(1).expect("holding post").account().data)
            .expect("holding"),
        TokenHolding::Fungible {
            definition_id: compute_stablecoin_definition_pda(PROGRAM),
            balance: fixture.balance + amount
        }
    );
}

#[test]
fn borrowing_pins_nine_account_abi_and_owner_only_signature() {
    let fixture = Fixture::new(100, 0);
    let request = fixture.request(json!("1"));
    let plan = generate_debt_plan(request.clone()).expect("plan");
    assert_eq!(plan["programId"], request.stablecoin_program_id);
    assert_eq!(
        plan["accountIds"],
        json!([
            request.owner_id,
            request.position.id,
            request.stablecoin_definition.id,
            request.user_stablecoin_holding.id,
            request.stability_fee_accumulator.id,
            request.redemption_price_state.id,
            request.market_price_oracle.id,
            request.protocol_parameters.id,
            request.clock.id
        ])
    );
    assert_eq!(
        plan["signingRequirements"],
        json!([true, false, false, false, false, false, false, false, false])
    );
    let idl: Value =
        serde_json::from_str(include_str!("../../../../../artifacts/stablecoin-idl.json"))
            .expect("IDL");
    let instruction = idl["instructions"]
        .as_array()
        .expect("instructions")
        .iter()
        .find(|entry| entry["name"] == "generate_debt")
        .expect("entry");
    let flags: Vec<_> = instruction["accounts"]
        .as_array()
        .expect("accounts")
        .iter()
        .map(|entry| {
            (
                entry["name"].clone(),
                entry["writable"].clone(),
                entry["signer"].clone(),
                entry["init"].clone(),
            )
        })
        .collect();
    assert_eq!(
        flags,
        vec![
            (json!("owner"), json!(false), json!(true), json!(false)),
            (json!("position"), json!(true), json!(false), json!(false)),
            (
                json!("stablecoin_definition"),
                json!(true),
                json!(false),
                json!(false)
            ),
            (
                json!("user_stablecoin_holding"),
                json!(true),
                json!(false),
                json!(false)
            ),
            (
                json!("stability_fee_accumulator"),
                json!(false),
                json!(false),
                json!(false)
            ),
            (
                json!("redemption_price_state"),
                json!(false),
                json!(false),
                json!(false)
            ),
            (
                json!("market_price_oracle"),
                json!(false),
                json!(false),
                json!(false)
            ),
            (
                json!("protocol_parameters"),
                json!(false),
                json!(false),
                json!(false)
            ),
            (json!("clock"), json!(false), json!(false), json!(false)),
        ]
    );
    assert_eq!(
        instruction["args"],
        json!([{"name":"amount","type":"u128"}])
    );
}

#[test]
fn borrowing_rounds_up_and_executes_the_pda_authorized_token_mint() {
    for (amount, delta) in [(0, 0), (1, 1), (3, 2), (4, 3)] {
        execute(&Fixture::new(100, 0), amount, delta);
    }
    execute(&Fixture::new(33, 0), 30, 20); // Exact post-mint health equality.
    error(
        Fixture::new(32, 0).request(json!("30")),
        "position_undercollateralized",
    );
    error(
        Fixture::new(1, 0).request(json!("1")),
        "position_undercollateralized",
    ); // A floor-to-zero debt delta would incorrectly pass.
}

#[test]
fn oracle_freshness_is_inclusive_and_not_a_controller_quote_gate() {
    let mut fixture = Fixture::new(100, 0);
    fixture.oracle.timestamp = START - 50;
    execute(&fixture, 1, 1); // Fresh zero-market-price oracle and too-soon controller update are allowed.
    fixture.oracle.timestamp -= 1;
    error(fixture.request(json!("0")), "oracle_stale");
    fixture.oracle.timestamp = START + 1;
    error(fixture.request(json!("0")), "oracle_future");
    fixture.oracle.timestamp = START;
    let mut request = fixture.request(json!("1"));
    // The instruction deliberately does not pin the producer or asset fields.
    request
        .market_price_oracle
        .account
        .as_mut()
        .expect("account")
        .program_owner = hex::encode(program_id_bytes([0x99; 8]));
    fixture.oracle.base_asset = id(90);
    fixture.oracle.quote_asset = id(91);
    request
        .market_price_oracle
        .account
        .as_mut()
        .expect("account")
        .data = hex::encode(Data::from(&fixture.oracle).as_ref());
    generate_debt_plan(request).expect("liveness only, as in native decode_oracle");
}

#[test]
fn zero_mints_do_not_bypass_freeze_health_pricing_or_zero_redemption_price() {
    let mut fixture = Fixture::new(100, 0);
    fixture.parameters.is_frozen = true;
    error(fixture.request(json!("0")), "protocol_frozen");
    error(
        Fixture::new(1, 100).request(json!("0")),
        "position_undercollateralized",
    );
    fixture.parameters.is_frozen = false;
    fixture.now = START + 7_000_000;
    fixture.oracle.timestamp = fixture.now;
    fixture.redemption.redemption_rate_per_millisecond =
        FIXED_POINT_ONE - RATE_DELTA_CLAMP.unsigned_abs();
    error(fixture.request(json!("0")), "redemption_price_zero");
    fixture.parameters.stability_fee_per_millisecond = FIXED_POINT_ONE * 2;
    error(fixture.request(json!("0")), "debt_pricing_arithmetic_error");
}

#[test]
fn checked_pricing_matches_native_narrow_math_and_time_window() {
    for (anchor, rate, last, now) in [
        (FIXED_POINT_ONE, FIXED_POINT_ONE, START, START),
        (
            FIXED_POINT_ONE * 3 / 2,
            FIXED_POINT_ONE + 1_500_000_000_000_000,
            START,
            START + 100,
        ),
        (
            FIXED_POINT_ONE,
            FIXED_POINT_ONE + 1_500_000_000_000_000,
            START,
            START + MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS + 1,
        ),
        (u128::MAX, FIXED_POINT_ONE, START, START - 1),
    ] {
        assert_eq!(
            checked_accumulator(anchor, rate, last, now),
            Some(compute_current_accumulated_rate(anchor, rate, last, now))
        );
    }
    for amount in [0, 1, 3, 4, u64::MAX.into(), u128::MAX] {
        assert_eq!(
            checked_debt_delta(amount, FIXED_POINT_ONE * 3 / 2),
            Some(mul_div_ceil(
                amount,
                FIXED_POINT_ONE,
                FIXED_POINT_ONE * 3 / 2
            ))
        );
    }
    assert_eq!(
        checked_accumulator(FIXED_POINT_ONE, FIXED_POINT_ONE * 2, START, START + 40),
        None
    );
    assert_eq!(checked_debt_delta(0, 0), None);
}

#[test]
fn borrowing_accepts_wide_redemption_projection_and_maximum_amounts_losslessly() {
    let mut wide = Fixture::new(10_000_000_000_000, 0);
    wide.redemption.redemption_rate_per_millisecond =
        FIXED_POINT_ONE + RATE_DELTA_CLAMP.unsigned_abs();
    wide.now = START + 2_700_000;
    wide.oracle.timestamp = wide.now;
    execute(&wide, 1, 1);
    let mut maximum = Fixture::new(u128::MAX, 0);
    maximum.supply = 0;
    maximum.balance = 0;
    maximum.redemption.redemption_price_at_last_update = FIXED_POINT_ONE / 2;
    execute(&maximum, u128::MAX, u128::MAX / 3 * 2);
    let mut request = maximum.request(json!(u64::MAX));
    request.owner_id = id(20).to_string();
    request.user_stablecoin_holding_id = id(21).to_string();
    let plan = generate_debt_plan(request).expect("lossless JSON integer/base58");
    let words: Vec<u32> = serde_json::from_value(plan["instruction"].clone()).expect("words");
    assert!(
        matches!(risc0_zkvm::serde::from_slice::<Instruction,u32>(&words).expect("instruction"),Instruction::GenerateDebt{amount} if amount==u128::from(u64::MAX))
    );
    for amount in [
        json!(1.5),
        json!(1.0),
        json!(-1),
        json!("-1"),
        json!(""),
        json!("1e3"),
        json!("340282366920938463463374607431768211456"),
        Value::Null,
    ] {
        error(maximum.request(amount), "invalid_numeric_value");
    }
    for nonce in ["", "-1", "1.0", "18446744073709551616"] {
        let mut request = maximum.request(json!("0"));
        request.position_nonce = nonce.to_owned();
        error(request, "invalid_numeric_value");
    }
}

#[test]
fn borrowing_rejects_debt_supply_and_cross_snapshot_balance_overflow() {
    let mut fixture = Fixture::new(u128::MAX, u128::MAX);
    error(fixture.request(json!("1")), "normalized_debt_overflow");
    fixture.position.normalized_debt_amount = 0;
    fixture.supply = u128::MAX;
    error(fixture.request(json!("1")), "stablecoin_supply_overflow");
    // Definition was read before another mint; destination was read afterward.
    // This intentionally violates a single-snapshot supply invariant to model
    // independent RPC observations, not an impossible coherent ledger state.
    fixture.supply = 100;
    fixture.balance = u128::MAX;
    error(fixture.request(json!("1")), "stablecoin_balance_overflow");
}

#[test]
fn borrowing_validates_definition_authority_destination_and_position_identity() {
    let fixture = Fixture::new(100, 0);
    for authority in [None, Some(id(90))] {
        // Explicit initialization-invariant violation: simulate a corrupted RPC
        // definition whose self/PDA authority no longer matches the program.
        let mut request = fixture.request(json!("0"));
        request
            .stablecoin_definition
            .account
            .as_mut()
            .expect("account")
            .data = hex::encode(
            Data::from(&TokenDefinition::Fungible {
                name: String::from("Stablecoin"),
                total_supply: fixture.supply,
                metadata_id: None,
                authority,
            })
            .as_ref(),
        );
        error(request, "invalid_stablecoin_mint_authority");
    }
    let mut request = fixture.request(json!("0"));
    request.stablecoin_definition.id = account_id_hex(id(90));
    error(request, "stablecoin_definition_mismatch");
    let mut request = fixture.request(json!("0"));
    request.user_stablecoin_holding.id = account_id_hex(id(90));
    error(request, "invalid_user_stablecoin_holding");
    let mut request = fixture.request(json!("0"));
    request
        .user_stablecoin_holding
        .account
        .as_mut()
        .expect("account")
        .program_owner = hex::encode(program_id_bytes(PROGRAM));
    error(request, "token_program_mismatch");
    let mut request = fixture.request(json!("0"));
    request
        .user_stablecoin_holding
        .account
        .as_mut()
        .expect("account")
        .data = hex::encode(
        Data::from(&TokenHolding::Fungible {
            definition_id: id(90),
            balance: 0,
        })
        .as_ref(),
    );
    error(request, "stablecoin_definition_mismatch");
    for alias in [id(20), compute_stablecoin_definition_pda(PROGRAM)] {
        let mut request = fixture.request(json!("0"));
        request.user_stablecoin_holding_id = account_id_hex(alias);
        error(request, "invalid_user_stablecoin_holding");
    }
    for (state, expected) in [
        (
            Position {
                owner_account_id: id(90),
                ..fixture.position.clone()
            },
            "position_owner_mismatch",
        ),
        (
            Position {
                position_nonce: 1,
                ..fixture.position.clone()
            },
            "position_nonce_mismatch",
        ),
    ] {
        let mut request = fixture.request(json!("0"));
        request.position.account.as_mut().expect("account").data =
            hex::encode(Data::from(&state).as_ref());
        error(request, expected);
    }
}

#[test]
fn borrowing_validates_all_eight_reads_even_for_zero_amount() {
    let fixture = Fixture::new(100, 0);
    let data_errors = [
        "invalid_position_data",
        "invalid_stablecoin_definition",
        "invalid_user_stablecoin_holding",
        "invalid_stability_fee_accumulator_data",
        "invalid_redemption_price_state_data",
        "invalid_market_price_oracle",
        "invalid_protocol_parameters_data",
        "invalid_clock",
    ];
    let identity_errors = [
        "position_pda_mismatch",
        "stablecoin_definition_mismatch",
        "invalid_user_stablecoin_holding",
        "stability_fee_accumulator_pda_mismatch",
        "redemption_price_state_pda_mismatch",
        "market_price_oracle_mismatch",
        "protocol_parameters_pda_mismatch",
        "invalid_clock",
    ];
    for index in 0..8 {
        for mutation in 0..4 {
            let mut request = fixture.request(json!("0"));
            let reads = [
                &mut request.position,
                &mut request.stablecoin_definition,
                &mut request.user_stablecoin_holding,
                &mut request.stability_fee_accumulator,
                &mut request.redemption_price_state,
                &mut request.market_price_oracle,
                &mut request.protocol_parameters,
                &mut request.clock,
            ];
            let read = reads.into_iter().nth(index).expect("read");
            let expected = match mutation {
                0 => {
                    read.status = String::from("not_found");
                    read.account = None;
                    "account_read_failed"
                }
                1 => {
                    let data = &mut read.account.as_mut().expect("account").data;
                    data.truncate(data.len() - 2);
                    data_errors[index]
                }
                2 => {
                    read.account.as_mut().expect("account").data.push_str("ff");
                    data_errors[index]
                }
                _ => {
                    read.id = account_id_hex(id(90));
                    identity_errors[index]
                }
            };
            error(request, expected);
        }
    }
}
