use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, Data},
    program::ProgramId,
};
use serde_json::json;
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_oracle_price_account_pda,
    compute_price_observations_pda, Instruction, OBSERVATIONS_CAPACITY,
};

use crate::{
    account::{account_id_hex, account_read},
    api::{
        numeric::*,
        price::{validate_price_account, OraclePriceAccount, PriceAccountError},
    },
    decode_account, feed_addresses, price_account, program_info, validate_clock, AccountRead,
    FeedAddressesRequest, OracleResult, PriceAccountRequest, ProgramInfoRequest, TransactionPlan,
};

fn id(seed: u8) -> AccountId {
    AccountId::new([seed; 32])
}

/// The program id whose hex spelling equals `id(seed)`'s.
fn pid(seed: u8) -> ProgramId {
    [u32::from_le_bytes([seed; 4]); 8]
}

fn account(owner: ProgramId, data: Data) -> Account {
    Account {
        program_owner: owner,
        data,
        ..Account::default()
    }
}

fn error(result: OracleResult, code: &str) {
    assert_eq!(result.expect_err("must fail").code(), code);
}

fn discovery(window: &str) -> FeedAddressesRequest {
    FeedAddressesRequest {
        oracle_program_id: account_id_hex(id(1)),
        price_source_id: account_id_hex(id(2)),
        window_duration_ms: window.into(),
    }
}

#[test]
fn discovery_matches_core_and_preserves_source_window_identity() {
    let first_window = u64::from(OBSERVATIONS_CAPACITY);
    let first = feed_addresses(discovery(&first_window.to_string())).expect("valid window");
    let second = feed_addresses(discovery(&u64::MAX.to_string())).expect("maximum window");
    for (value, window) in [(&first, first_window), (&second, u64::MAX)] {
        for (key, expected) in [
            (
                "currentTickAccountId",
                compute_current_tick_account_pda(pid(1), id(2)),
            ),
            (
                "priceObservationsId",
                compute_price_observations_pda(pid(1), id(2), window),
            ),
            (
                "priceAccountId",
                compute_oracle_price_account_pda(pid(1), id(2), window),
            ),
            ("clockId", CLOCK_01_PROGRAM_ACCOUNT_ID),
        ] {
            assert_eq!(value[key], expected.to_string());
            assert_eq!(value[format!("{key}Hex")], account_id_hex(expected));
        }
    }
    assert_eq!(
        first["currentTickAccountId"],
        second["currentTickAccountId"]
    );
    assert_ne!(first["priceObservationsId"], second["priceObservationsId"]);
    assert_ne!(first["priceAccountId"], second["priceAccountId"]);
}

#[test]
fn discovery_rejects_invalid_windows_and_noncanonical_ids() {
    error(
        feed_addresses(discovery(&(OBSERVATIONS_CAPACITY - 1).to_string())),
        "invalid_window",
    );
    for value in [
        "",
        "+2048",
        "-2048",
        "2048.0",
        "2e4",
        "18446744073709551616",
    ] {
        error(feed_addresses(discovery(value)), "invalid_numeric_value");
    }
    for value in [
        "00".repeat(32),
        "AB".repeat(32),
        "ab".repeat(31),
        id(3).to_string(),
    ] {
        let mut request = discovery("2048");
        request.price_source_id = value.clone();
        error(feed_addresses(request), "invalid_account_id");
        let mut request = discovery("2048");
        request.oracle_program_id = value;
        error(feed_addresses(request), "invalid_program_id");
    }
}

#[test]
fn decimal_parsers_preserve_every_numeric_boundary() {
    assert_eq!(parse_u128(&u128::MAX.to_string()).expect("u128"), u128::MAX);
    assert_eq!(parse_u64(&u64::MAX.to_string()).expect("u64"), u64::MAX);
    assert_eq!(
        parse_u128("9007199254740993").expect("above JS precision"),
        9_007_199_254_740_993
    );
    assert_eq!(parse_u64("0000").expect("leading zeroes"), 0);
    for value in [i32::MIN, i32::MAX, 0, -1] {
        assert_eq!(parse_i32(&value.to_string()).expect("i32"), value);
    }
    for value in [i64::MIN, i64::MAX, 0, -1] {
        assert_eq!(parse_i64(&value.to_string()).expect("i64"), value);
    }
    assert_eq!(parse_i64("-0").expect("signed zero"), 0);
    for value in ["", "+0", "1.0", "1e3", " 1", "1 ", "--1", "１２"] {
        assert!(parse_u64(value).is_err());
        assert!(parse_u128(value).is_err());
        assert!(parse_i32(value).is_err());
        assert!(parse_i64(value).is_err());
    }
    for value in ["-1", "-0"] {
        assert!(parse_u64(value).is_err());
        assert!(parse_u128(value).is_err());
    }
    assert!(parse_u128("340282366920938463463374607431768211456").is_err());
    assert!(parse_u64("18446744073709551616").is_err());
    for value in ["2147483648", "-2147483649"] {
        assert!(parse_i32(value).is_err());
    }
    for value in ["9223372036854775808", "-9223372036854775809"] {
        assert!(parse_i64(value).is_err());
    }
}

fn program_request() -> ProgramInfoRequest {
    ProgramInfoRequest {
        oracle_program_id: Some(account_id_hex(id(1))),
        ..Default::default()
    }
}

#[test]
fn program_id_is_configured_or_derived() {
    error(
        program_info(ProgramInfoRequest::default()),
        "config_missing",
    );
    let value = program_info(program_request()).expect("explicit program id");
    assert_eq!(value["programIdHex"], account_id_hex(id(1)));
    assert_eq!(value["programId"], id(1).to_string());
    assert_eq!(value["imageIdHex"], account_id_hex(id(1)));
    assert_eq!(
        value["clockIdHex"],
        account_id_hex(CLOCK_01_PROGRAM_ACCOUNT_ID)
    );
    for configured in ["", "bad-id", &"0".repeat(64), &"AB".repeat(32)] {
        error(
            program_info(ProgramInfoRequest {
                oracle_program_id: Some(configured.into()),
                ..Default::default()
            }),
            "invalid_program_id",
        );
    }
    for binary in ["", "zz", "00", "7f454c46"] {
        let mut request = program_request();
        request.elf = Some(binary.into());
        error(program_info(request), "invalid_program_binary");
    }
}

#[test]
fn binary_supplies_or_verifies_the_image_id() {
    let bytes = twap_oracle_methods::TWAP_ORACLE_ELF;
    let binary = risc0_binfmt::ProgramBinary::decode(bytes).expect("built guest binary");
    let image: ProgramId = binary.compute_image_id().expect("image").into();
    let image_hex = hex::encode(
        image
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>(),
    );
    assert_ne!(image, pid(1));

    let derived = program_info(ProgramInfoRequest {
        elf: Some(hex::encode(bytes)),
        ..Default::default()
    })
    .expect("binary alone");
    assert_eq!(derived["programIdHex"], image_hex);
    assert_eq!(derived["imageIdHex"], image_hex);

    let matching = program_info(ProgramInfoRequest {
        oracle_program_id: Some(image_hex.clone()),
        elf: Some(hex::encode(bytes)),
    })
    .expect("matching id and binary");
    assert_eq!(matching, derived);

    let mut mismatch = program_request();
    mismatch.elf = Some(hex::encode(bytes));
    error(program_info(mismatch), "program_binary_mismatch");
}

#[test]
fn reads_distinguish_failure_default_and_initialized_snapshots() {
    let default = account_read(id(1), &Account::default());
    assert_eq!(
        decode_account(&default).expect("successful default read").1,
        Account::default()
    );
    let funded = account_read(
        id(1),
        &Account {
            balance: u128::MAX,
            ..Account::default()
        },
    );
    assert_eq!(
        decode_account(&funded)
            .expect("balance preserved")
            .1
            .balance,
        u128::MAX
    );
    for status in ["account_read_failed", "not_found", "error", ""] {
        assert!(decode_account(&AccountRead {
            status: status.into(),
            ..default.clone()
        })
        .is_err());
    }
    for field in ["program_owner", "balance", "nonce", "data"] {
        let mut value = json!({"id": default.id, "status": "ok", "account": {
            "program_owner": "00".repeat(32), "balance": "00".repeat(16), "nonce": "00".repeat(16), "data": ""
        }});
        value["account"][field] = json!("z");
        assert!(decode_account(&serde_json::from_value(value).expect("read structure")).is_err());
    }
}

#[test]
fn clock_requires_canonical_id_and_exact_borsh() {
    let clock = ClockAccountData {
        block_id: u64::MAX,
        timestamp: u64::MAX,
    };
    let data = Data::try_from(clock.to_bytes()).expect("clock bytes");
    let read = account_read(CLOCK_01_PROGRAM_ACCOUNT_ID, &account(pid(8), data.clone()));
    assert_eq!(validate_clock(&read).expect("clock"), clock);
    assert_eq!(
        validate_clock(&account_read(id(3), &account(pid(8), data)))
            .expect_err("wrong id")
            .code(),
        "invalid_clock"
    );
    for data in ["", "00", &(hex::encode(clock.to_bytes()) + "00")] {
        let mut bad = read.clone();
        bad.account.as_mut().expect("snapshot").data = data.into();
        assert_eq!(
            validate_clock(&bad).expect_err("bad layout").code(),
            "invalid_clock"
        );
    }
    let failed = AccountRead {
        status: "account_read_failed".into(),
        account: None,
        ..read
    };
    assert_eq!(
        validate_clock(&failed).expect_err("failed read").code(),
        "account_read_failed"
    );
}

#[test]
fn canonical_price_contract_is_independent_of_twap_addresses() {
    let price = OraclePriceAccount {
        base_asset: id(4),
        quote_asset: id(5),
        price: u128::MAX,
        timestamp: u64::MAX,
        source_id: id(6),
        confidence_interval: u128::MAX,
    };
    let read = account_read(id(20), &account(pid(21), Data::from(&price)));
    assert_eq!(
        validate_price_account(&read, id(20), pid(21)).expect("arbitrary producer"),
        price
    );
    assert_eq!(
        validate_price_account(&read, id(19), pid(21)),
        Err(PriceAccountError::IdMismatch)
    );
    assert_eq!(
        validate_price_account(&read, id(20), pid(19)),
        Err(PriceAccountError::OwnerMismatch)
    );
    assert_eq!(
        validate_price_account(&account_read(id(20), &Account::default()), id(20), pid(21)),
        Err(PriceAccountError::NotFound)
    );
    let mut trailing = read;
    trailing
        .account
        .as_mut()
        .expect("snapshot")
        .data
        .push_str("00");
    assert_eq!(
        validate_price_account(&trailing, id(20), pid(21)),
        Err(PriceAccountError::InvalidData)
    );
    let zero = OraclePriceAccount {
        price: 0,
        timestamp: 0,
        ..price
    };
    assert_eq!(
        validate_price_account(
            &account_read(id(20), &account(pid(21), Data::from(&zero))),
            id(20),
            pid(21)
        )
        .expect("stored zero"),
        zero
    );
}

#[test]
fn price_account_returns_stored_snapshot_with_exact_fields() {
    let price = OraclePriceAccount {
        base_asset: id(4),
        quote_asset: id(5),
        price: u128::MAX,
        timestamp: u64::MAX,
        source_id: id(6),
        confidence_interval: u128::MAX,
    };
    let value = price_account(PriceAccountRequest {
        price_account_id: account_id_hex(id(20)),
        expected_owner_id: account_id_hex(id(21)),
        read: account_read(id(20), &account(pid(21), Data::from(&price))),
    })
    .expect("stored snapshot");
    for (key, expected) in [
        ("priceAccountId", id(20)),
        ("ownerId", id(21)),
        ("baseAssetId", id(4)),
        ("quoteAssetId", id(5)),
        ("sourceId", id(6)),
    ] {
        assert_eq!(value[key], expected.to_string());
        assert_eq!(value[format!("{key}Hex")], account_id_hex(expected));
    }
    assert_eq!(value["price"], u128::MAX.to_string());
    assert_eq!(value["timestampMs"], u64::MAX.to_string());
    assert_eq!(value["confidenceInterval"], u128::MAX.to_string());
    assert_eq!(value["priceFractionalBits"], 64);
    let odd = OraclePriceAccount {
        price: 9_007_199_254_740_993,
        ..price
    };
    let value = price_account(PriceAccountRequest {
        price_account_id: account_id_hex(id(20)),
        expected_owner_id: account_id_hex(id(21)),
        read: account_read(id(20), &account(pid(21), Data::from(&odd))),
    })
    .expect("odd price above javascript precision");
    assert_eq!(value["price"], "9007199254740993");
}

#[test]
fn price_account_maps_validation_errors_to_stable_codes() {
    let price = OraclePriceAccount {
        base_asset: id(4),
        quote_asset: id(5),
        price: 1 << 64,
        timestamp: 1,
        source_id: id(6),
        confidence_interval: 1,
    };
    let read = account_read(id(20), &account(pid(21), Data::from(&price)));
    let request = || PriceAccountRequest {
        price_account_id: account_id_hex(id(20)),
        expected_owner_id: account_id_hex(id(21)),
        read: read.clone(),
    };
    let failed = AccountRead {
        status: "account_read_failed".into(),
        account: None,
        ..read.clone()
    };
    let mut variant = request();
    variant.read = failed;
    error(price_account(variant), "account_read_failed");
    let mut variant = request();
    variant.price_account_id = account_id_hex(id(19));
    error(price_account(variant), "price_account_id_mismatch");
    let mut variant = request();
    variant.read = account_read(id(20), &Account::default());
    error(price_account(variant), "not_found");
    let mut variant = request();
    variant.expected_owner_id = account_id_hex(id(19));
    error(price_account(variant), "oracle_owner_mismatch");
    let mut variant = request();
    variant
        .read
        .account
        .as_mut()
        .expect("snapshot")
        .data
        .push_str("00");
    error(price_account(variant), "invalid_account_data");
}

#[test]
fn price_account_preserves_zero_and_stale_snapshots() {
    let zero = OraclePriceAccount {
        base_asset: id(4),
        quote_asset: id(5),
        price: 0,
        timestamp: 0,
        source_id: id(6),
        confidence_interval: 0,
    };
    let value = price_account(PriceAccountRequest {
        price_account_id: account_id_hex(id(20)),
        expected_owner_id: account_id_hex(id(21)),
        read: account_read(id(20), &account(pid(21), Data::from(&zero))),
    })
    .expect("raw reads do not judge stored snapshots");
    assert_eq!(value["price"], "0");
    assert_eq!(value["timestampMs"], "0");
    assert_eq!(value["confidenceInterval"], "0");
}

#[test]
fn price_account_rejects_malformed_ids() {
    let price = OraclePriceAccount {
        base_asset: id(4),
        quote_asset: id(5),
        price: 1 << 64,
        timestamp: 1,
        source_id: id(6),
        confidence_interval: 1,
    };
    let read = account_read(id(20), &account(pid(21), Data::from(&price)));
    let request = || PriceAccountRequest {
        price_account_id: account_id_hex(id(20)),
        expected_owner_id: account_id_hex(id(21)),
        read: read.clone(),
    };
    for value in [
        "00".repeat(32),
        "AB".repeat(32),
        "ab".repeat(31),
        id(3).to_string(),
    ] {
        let mut variant = request();
        variant.price_account_id = value.clone();
        error(price_account(variant), "invalid_account_id");
        let mut variant = request();
        variant.expected_owner_id = value;
        error(price_account(variant), "invalid_account_id");
    }
}

#[test]
fn plans_preserve_instruction_words_account_order_and_production_signers() {
    let instructions = [
        Instruction::CreatePriceObservations {
            initial_tick: i32::MIN,
            window_duration: u64::MAX,
        },
        Instruction::CreateOraclePriceAccount {
            base_asset: id(4),
            quote_asset: id(5),
            initial_price: u128::MAX,
            window_duration: u64::MAX,
        },
        Instruction::CreateCurrentTickAccount {
            initial_price: u128::MAX,
        },
        Instruction::UpdateCurrentTick { price: u128::MAX },
        Instruction::RecordTick {
            price_source_id: id(2),
            window_duration: u64::MAX,
        },
        Instruction::PublishPrice {
            price_source_id: id(2),
            window_duration: u64::MAX,
        },
    ];
    for (index, instruction) in instructions.into_iter().enumerate() {
        let words = risc0_zkvm::serde::to_vec(&instruction).expect("typed instruction");
        let value = if index == 5 {
            serde_json::to_value(
                TransactionPlan::new(
                    pid(1),
                    [id(7), id(8), id(9), CLOCK_01_PROGRAM_ACCOUNT_ID],
                    instruction,
                )
                .expect("four accounts"),
            )
        } else {
            serde_json::to_value(
                TransactionPlan::new(
                    pid(1),
                    [id(7), id(2), CLOCK_01_PROGRAM_ACCOUNT_ID],
                    instruction,
                )
                .expect("three accounts"),
            )
        }
        .expect("plan JSON");
        assert_eq!(value["programId"], account_id_hex(id(1)));
        assert_eq!(value["accountIds"][0], account_id_hex(id(7)));
        let encoded: Vec<u32> =
            serde_json::from_value(value["instruction"].clone()).expect("word array");
        assert_eq!(encoded, words);
        let decoded: Instruction =
            risc0_zkvm::serde::from_slice(&encoded).expect("guest instruction decoding");
        assert_eq!(
            risc0_zkvm::serde::to_vec(&decoded).expect("roundtrip"),
            words
        );
        assert_eq!(
            value["signingRequirements"],
            if index < 4 {
                json!([false, true, false])
            } else if index == 4 {
                json!([false, false, false])
            } else {
                json!([false, false, false, false])
            }
        );
    }
    assert!(TransactionPlan::new(
        pid(1),
        [id(2), id(3), id(4)],
        Instruction::PublishPrice {
            price_source_id: id(2),
            window_duration: 2048
        }
    )
    .is_err());
}

mod planners {
    use serde_json::Value;

    use super::*;
    use crate::api::{
        create_current_tick_account, create_oracle_price_account, create_price_observations,
        publish_price, record_tick, update_current_tick, CreateCurrentTickAccountRequest,
        CreateOraclePriceAccountRequest, CreatePriceObservationsRequest, FeedMaintenanceRequest,
        UpdateCurrentTickRequest,
    };

    const WINDOW: u64 = 86_400_000;

    fn program() -> String {
        account_id_hex(id(1))
    }

    fn source() -> String {
        account_id_hex(id(2))
    }

    fn tick() -> AccountId {
        compute_current_tick_account_pda(pid(1), id(2))
    }

    fn observations() -> AccountId {
        compute_price_observations_pda(pid(1), id(2), WINDOW)
    }

    fn price() -> AccountId {
        compute_oracle_price_account_pda(pid(1), id(2), WINDOW)
    }

    fn maintenance(window: &str) -> FeedMaintenanceRequest {
        FeedMaintenanceRequest {
            oracle_program_id: program(),
            price_source_id: source(),
            window_duration_ms: window.into(),
        }
    }

    /// Checks the plan's program, accounts, signers and exact instruction words.
    fn assert_plan(
        value: &Value,
        accounts: &[AccountId],
        signers: &[bool],
        instruction: &Instruction,
    ) {
        let plan = &value["plan"];
        assert_eq!(plan["programId"], account_id_hex(id(1)));
        let expected: Vec<String> = accounts.iter().copied().map(account_id_hex).collect();
        assert_eq!(plan["accountIds"], json!(expected));
        assert_eq!(plan["signingRequirements"], json!(signers));
        let words: Vec<u32> =
            serde_json::from_value(plan["instruction"].clone()).expect("word array");
        assert_eq!(
            words,
            risc0_zkvm::serde::to_vec(instruction).expect("expected instruction")
        );
    }

    fn assert_identity(value: &Value, key: &str, expected: AccountId) {
        assert_eq!(value["identities"][key], expected.to_string());
        assert_eq!(
            value["identities"][format!("{key}Hex")],
            account_id_hex(expected)
        );
    }

    #[test]
    fn source_writes_sign_with_the_source_at_index_one() {
        let value = create_current_tick_account(CreateCurrentTickAccountRequest {
            oracle_program_id: program(),
            price_source_id: source(),
            initial_price: u128::MAX.to_string(),
        })
        .expect("current tick plan");
        assert_plan(
            &value,
            &[tick(), id(2), CLOCK_01_PROGRAM_ACCOUNT_ID],
            &[false, true, false],
            &Instruction::CreateCurrentTickAccount {
                initial_price: u128::MAX,
            },
        );
        assert_identity(&value, "currentTickAccountId", tick());
        assert_identity(&value, "priceSourceId", id(2));
        assert_identity(&value, "clockId", CLOCK_01_PROGRAM_ACCOUNT_ID);

        let value = create_price_observations(CreatePriceObservationsRequest {
            oracle_program_id: program(),
            price_source_id: source(),
            initial_tick: i32::MIN.to_string(),
            window_duration_ms: WINDOW.to_string(),
        })
        .expect("observations plan");
        assert_plan(
            &value,
            &[observations(), id(2), CLOCK_01_PROGRAM_ACCOUNT_ID],
            &[false, true, false],
            &Instruction::CreatePriceObservations {
                initial_tick: i32::MIN,
                window_duration: WINDOW,
            },
        );
        assert_identity(&value, "priceObservationsId", observations());

        let value = create_oracle_price_account(CreateOraclePriceAccountRequest {
            oracle_program_id: program(),
            price_source_id: source(),
            base_asset_id: account_id_hex(id(4)),
            quote_asset_id: account_id_hex(id(5)),
            initial_price: (1_u128 << 64).to_string(),
            window_duration_ms: WINDOW.to_string(),
        })
        .expect("price account plan");
        assert_plan(
            &value,
            &[price(), id(2), CLOCK_01_PROGRAM_ACCOUNT_ID],
            &[false, true, false],
            &Instruction::CreateOraclePriceAccount {
                base_asset: id(4),
                quote_asset: id(5),
                initial_price: 1 << 64,
                window_duration: WINDOW,
            },
        );
        assert_identity(&value, "priceAccountId", price());
        assert_identity(&value, "baseAssetId", id(4));
        assert_identity(&value, "quoteAssetId", id(5));

        let value = update_current_tick(UpdateCurrentTickRequest {
            oracle_program_id: program(),
            price_source_id: source(),
            price: "0".into(),
        })
        .expect("update plan");
        assert_plan(
            &value,
            &[tick(), id(2), CLOCK_01_PROGRAM_ACCOUNT_ID],
            &[false, true, false],
            &Instruction::UpdateCurrentTick { price: 0 },
        );
        assert_identity(&value, "currentTickAccountId", tick());
    }

    #[test]
    fn maintenance_is_permissionless() {
        let value = record_tick(maintenance(&WINDOW.to_string())).expect("record plan");
        assert_plan(
            &value,
            &[observations(), tick(), CLOCK_01_PROGRAM_ACCOUNT_ID],
            &[false, false, false],
            &Instruction::RecordTick {
                price_source_id: id(2),
                window_duration: WINDOW,
            },
        );
        assert_identity(&value, "priceObservationsId", observations());
        assert_identity(&value, "currentTickAccountId", tick());

        let value = publish_price(maintenance(&WINDOW.to_string())).expect("publish plan");
        assert_plan(
            &value,
            &[observations(), price(), tick(), CLOCK_01_PROGRAM_ACCOUNT_ID],
            &[false, false, false, false],
            &Instruction::PublishPrice {
                price_source_id: id(2),
                window_duration: WINDOW,
            },
        );
        assert_identity(&value, "priceAccountId", price());
    }

    #[test]
    fn windows_feed_the_same_addresses_as_discovery() {
        let discovered = feed_addresses(FeedAddressesRequest {
            oracle_program_id: program(),
            price_source_id: source(),
            window_duration_ms: WINDOW.to_string(),
        })
        .expect("discovery");
        let planned = publish_price(maintenance(&WINDOW.to_string())).expect("publish plan");
        for key in [
            "priceObservationsId",
            "priceAccountId",
            "currentTickAccountId",
            "clockId",
        ] {
            assert_eq!(planned["identities"][key], discovered[key]);
        }
    }

    #[test]
    fn planners_reject_invalid_arguments() {
        let window_errors = [(OBSERVATIONS_CAPACITY - 1).to_string(), "0".into()];
        for window in &window_errors {
            error(record_tick(maintenance(window)), "invalid_window");
            error(publish_price(maintenance(window)), "invalid_window");
            error(
                create_price_observations(CreatePriceObservationsRequest {
                    oracle_program_id: program(),
                    price_source_id: source(),
                    initial_tick: "0".into(),
                    window_duration_ms: window.clone(),
                }),
                "invalid_window",
            );
        }
        for value in ["", "+1", "1.0", "1e3", "-1"] {
            error(
                create_current_tick_account(CreateCurrentTickAccountRequest {
                    oracle_program_id: program(),
                    price_source_id: source(),
                    initial_price: value.into(),
                }),
                "invalid_numeric_value",
            );
            error(
                update_current_tick(UpdateCurrentTickRequest {
                    oracle_program_id: program(),
                    price_source_id: source(),
                    price: value.into(),
                }),
                "invalid_numeric_value",
            );
        }
        error(
            create_price_observations(CreatePriceObservationsRequest {
                oracle_program_id: program(),
                price_source_id: source(),
                initial_tick: "2147483648".into(),
                window_duration_ms: WINDOW.to_string(),
            }),
            "invalid_numeric_value",
        );

        let price_request = |base: String, quote: String, initial: &str| {
            create_oracle_price_account(CreateOraclePriceAccountRequest {
                oracle_program_id: program(),
                price_source_id: source(),
                base_asset_id: base,
                quote_asset_id: quote,
                initial_price: initial.into(),
                window_duration_ms: WINDOW.to_string(),
            })
        };
        let asset = || account_id_hex(id(4));
        error(price_request(asset(), asset(), "0"), "invalid_price");
        error(
            price_request("00".repeat(32), asset(), "1"),
            "invalid_account_id",
        );
        error(
            price_request(asset(), "bad".into(), "1"),
            "invalid_account_id",
        );

        let mut request = maintenance(&WINDOW.to_string());
        request.oracle_program_id = "00".repeat(32);
        error(record_tick(request), "invalid_program_id");
        let mut request = maintenance(&WINDOW.to_string());
        request.price_source_id = "AB".repeat(32);
        error(publish_price(request), "invalid_account_id");
    }
}
