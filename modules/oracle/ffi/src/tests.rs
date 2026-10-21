use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, Data},
    program::{ProgramHeader, PROGRAM_LOADER_ACCOUNT_ID},
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
    decode_account, feed_addresses, program_info, validate_clock, AccountRead,
    FeedAddressesRequest, OracleResult, ProgramInfoRequest, TransactionPlan,
};

fn id(seed: u8) -> AccountId {
    AccountId::new([seed; 32])
}

fn account(owner: AccountId, data: Data) -> Account {
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
                compute_current_tick_account_pda(id(1), id(2)),
            ),
            (
                "priceObservationsId",
                compute_price_observations_pda(id(1), id(2), window),
            ),
            (
                "priceAccountId",
                compute_oracle_price_account_pda(id(1), id(2), window),
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

fn header(image_id: AccountId) -> AccountRead {
    let data = ProgramHeader {
        image_id: image_id.into(),
        program_first_segment: id(7),
        immutable: true,
    };
    account_read(
        id(1),
        &account(
            PROGRAM_LOADER_ACCOUNT_ID,
            Data::try_from(data.to_bytes()).expect("header"),
        ),
    )
}

#[test]
fn program_address_is_required_and_distinct_from_image_id() {
    error(
        program_info(ProgramInfoRequest::default()),
        "config_missing",
    );
    error(
        program_info(ProgramInfoRequest {
            elf: Some("not even a binary".into()),
            ..Default::default()
        }),
        "config_missing",
    );
    let value = program_info(program_request()).expect("explicit address");
    assert_eq!(value["programIdHex"], account_id_hex(id(1)));
    assert_eq!(value["programId"], id(1).to_string());
    assert_eq!(
        value["clockIdHex"],
        account_id_hex(CLOCK_01_PROGRAM_ACCOUNT_ID)
    );
    assert_eq!(value["imageIdHex"], "");
    for configured in ["", "bad-id", &"0".repeat(64)] {
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
fn binary_validation_uses_live_exact_loader_header() {
    let bytes = twap_oracle_methods::TWAP_ORACLE_ELF;
    let binary = risc0_binfmt::ProgramBinary::decode(bytes).expect("built guest binary");
    let image: lee_core::program::ProgramId = binary.compute_image_id().expect("image").into();
    let image = AccountId::from(image);
    assert_ne!(id(1), image);
    let mut request = program_request();
    request.elf = Some(hex::encode(bytes));
    error(program_info(request.clone()), "program_header_unavailable");
    request.program_header = Some(header(image));
    let value = program_info(request.clone()).expect("matching deployment");
    assert_eq!(value["imageIdHex"], account_id_hex(image));
    assert_eq!(value["programIdHex"], account_id_hex(id(1)));
    request.program_header = Some(header(id(9)));
    error(program_info(request.clone()), "program_binary_mismatch");
    // Header reads are snapshots, never a cached identity. A second header can
    // now match again after deployment changes.
    request.program_header = Some(header(image));
    assert!(program_info(request.clone()).is_ok());
    for read in [
        AccountRead {
            id: account_id_hex(id(1)),
            status: "account_read_failed".into(),
            account: None,
        },
        account_read(id(1), &Account::default()),
        account_read(id(1), &account(id(8), Data::default())),
        account_read(id(1), &account(PROGRAM_LOADER_ACCOUNT_ID, Data::default())),
        AccountRead {
            id: account_id_hex(id(2)),
            ..header(image)
        },
    ] {
        request.program_header = Some(read);
        error(program_info(request.clone()), "program_header_unavailable");
    }
    let mut trailing = header(image);
    trailing
        .account
        .as_mut()
        .expect("snapshot")
        .data
        .push_str("00");
    request.program_header = Some(trailing);
    error(program_info(request), "program_header_unavailable");
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
    let read = account_read(CLOCK_01_PROGRAM_ACCOUNT_ID, &account(id(8), data.clone()));
    assert_eq!(validate_clock(&read).expect("clock"), clock);
    assert_eq!(
        validate_clock(&account_read(id(3), &account(id(8), data)))
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
    let read = account_read(id(20), &account(id(21), Data::from(&price)));
    assert_eq!(
        validate_price_account(&read, id(20), id(21)).expect("arbitrary producer"),
        price
    );
    assert_eq!(
        validate_price_account(&read, id(19), id(21)),
        Err(PriceAccountError::IdMismatch)
    );
    assert_eq!(
        validate_price_account(&read, id(20), id(19)),
        Err(PriceAccountError::OwnerMismatch)
    );
    assert_eq!(
        validate_price_account(&account_read(id(20), &Account::default()), id(20), id(21)),
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
        validate_price_account(&trailing, id(20), id(21)),
        Err(PriceAccountError::InvalidData)
    );
    let zero = OraclePriceAccount {
        price: 0,
        timestamp: 0,
        ..price
    };
    assert_eq!(
        validate_price_account(
            &account_read(id(20), &account(id(21), Data::from(&zero))),
            id(20),
            id(21)
        )
        .expect("stored zero"),
        zero
    );
}

#[test]
fn plans_preserve_borsh_bytes_account_order_and_production_signers() {
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
        let bytes = borsh::to_vec(&instruction).expect("typed instruction");
        let value = if index == 5 {
            serde_json::to_value(
                TransactionPlan::new(
                    id(1),
                    [id(7), id(8), id(9), CLOCK_01_PROGRAM_ACCOUNT_ID],
                    instruction,
                )
                .expect("four accounts"),
            )
        } else {
            serde_json::to_value(
                TransactionPlan::new(
                    id(1),
                    [id(7), id(2), CLOCK_01_PROGRAM_ACCOUNT_ID],
                    instruction,
                )
                .expect("three accounts"),
            )
        }
        .expect("plan JSON");
        assert_eq!(value["programId"], account_id_hex(id(1)));
        assert_eq!(value["accountIds"][0], account_id_hex(id(7)));
        let encoded: Vec<u8> =
            serde_json::from_value(value["instruction"].clone()).expect("byte array");
        assert_eq!(encoded, bytes);
        let decoded: Instruction = borsh::from_slice(&encoded).expect("guest Borsh decoding");
        assert_eq!(borsh::to_vec(&decoded).expect("roundtrip"), bytes);
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
        id(1),
        [id(2), id(3), id(4)],
        Instruction::PublishPrice {
            price_source_id: id(2),
            window_duration: 2048
        }
    )
    .is_err());
}
