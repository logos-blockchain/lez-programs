use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use serde_json::{json, Value};
use token_core::{
    Instruction, MetadataStandard, NewTokenDefinition, TokenDefinition, TokenHolding, TokenMetadata,
};

use super::{
    burn_plan, create_fungible_plan, create_fungible_with_metadata_plan, create_non_fungible_plan,
    decode_account, decode_definition, decode_holding, decode_metadata, initialize_holding_plan,
    mint_plan, mint_with_authority_plan, print_nft_plan, set_authority_plan,
    set_authority_with_authority_plan, transfer_plan, BurnPlanRequest, CreateFungiblePlanRequest,
    CreateFungibleWithMetadataPlanRequest, CreateNonFungiblePlanRequest, DecodeAccountRequest,
    DecodeDefinitionRequest, DecodeHoldingRequest, DecodeMetadataRequest,
    InitializeHoldingPlanRequest, MintPlanRequest, MintWithAuthorityPlanRequest,
    PrintNftPlanRequest, SetAuthorityPlanRequest, SetAuthorityWithAuthorityPlanRequest,
    TransferPlanRequest,
};
use crate::account::{account_id_hex, account_read, program_id_bytes};

const TOKEN_PROGRAM_ID: ProgramId = [0x11_u32; 8];

fn account(owner: ProgramId, data: Data) -> Account {
    Account {
        program_owner: owner,
        balance: 0,
        data,
        nonce: Nonce(0),
    }
}

fn definition_id(seed: u8) -> AccountId {
    AccountId::new([seed; 32])
}

fn id_hex(seed: u8) -> String {
    account_id_hex(definition_id(seed))
}

fn token_program_id_hex() -> String {
    hex::encode(program_id_bytes(TOKEN_PROGRAM_ID))
}

fn ok<T, E: core::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{error}"),
    }
}

fn assert_error(result: super::TokenResult, expected: &str) {
    match result {
        Ok(value) => panic!("expected {expected}, got {value}"),
        Err(error) => assert_eq!(error.code(), expected),
    }
}

fn decode_instruction(value: &Value) -> Result<Instruction, String> {
    let words: Vec<u32> =
        serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
    risc0_zkvm::serde::from_slice::<Instruction, u32>(&words).map_err(|error| error.to_string())
}

fn assert_plan<const N: usize>(
    plan: &Value,
    expected_account_ids: [String; N],
    expected_signers: [bool; N],
) -> Instruction {
    assert_eq!(plan["programId"], token_program_id_hex());
    assert_eq!(
        plan["accountIds"],
        json!(Vec::from(expected_account_ids.clone()))
    );
    assert_eq!(
        plan["signingRequirements"],
        json!(Vec::from(expected_signers))
    );

    match plan.get("instruction") {
        Some(value) => ok(decode_instruction(value)),
        None => panic!("plan instruction is required"),
    }
}

fn decode_definition_request(definition: TokenDefinition) -> DecodeDefinitionRequest {
    DecodeDefinitionRequest {
        token_program_id: token_program_id_hex(),
        definition: account_read(
            definition_id(1),
            &account(TOKEN_PROGRAM_ID, Data::from(&definition)),
        ),
    }
}

fn definition_read_from_bytes(seed: u8, bytes: Vec<u8>) -> DecodeDefinitionRequest {
    DecodeDefinitionRequest {
        token_program_id: token_program_id_hex(),
        definition: account_read(
            definition_id(seed),
            &account(TOKEN_PROGRAM_ID, ok(Data::try_from(bytes))),
        ),
    }
}

fn transfer_request(amount_raw: Value, recipient_is_fresh: bool) -> TransferPlanRequest {
    TransferPlanRequest {
        token_program_id: token_program_id_hex(),
        sender_holding_id: id_hex(80),
        recipient_holding_id: id_hex(81),
        amount_raw,
        recipient_is_fresh,
    }
}

#[test]
fn definition_decode_reports_fungible_optionals_and_exact_values() {
    let metadata = definition_id(2);
    let authority = definition_id(3);
    let populated = ok(decode_definition(decode_definition_request(
        TokenDefinition::Fungible {
            name: String::from("Pebble"),
            total_supply: u128::MAX,
            metadata_id: Some(metadata),
            authority: Some(authority),
        },
    )));

    assert_eq!(populated["accountType"], "definition");
    assert_eq!(populated["kind"], "fungible");
    assert_eq!(populated["name"], "Pebble");
    assert_eq!(populated["totalSupplyRaw"], u128::MAX.to_string());
    assert_eq!(populated["metadataId"], metadata.to_string());
    assert_eq!(populated["metadataIdHex"], account_id_hex(metadata));
    assert_eq!(populated["mintAuthorityId"], authority.to_string());
    assert_eq!(populated["mintAuthorityIdHex"], account_id_hex(authority));

    let fixed = ok(decode_definition(decode_definition_request(
        TokenDefinition::Fungible {
            name: String::from("Fixed"),
            total_supply: 0,
            metadata_id: None,
            authority: None,
        },
    )));
    assert!(fixed["metadataId"].is_null());
    assert!(fixed["metadataIdHex"].is_null());
    assert!(fixed["mintAuthorityId"].is_null());
    assert!(fixed["mintAuthorityIdHex"].is_null());
}

#[test]
fn definition_decode_reports_non_fungible_fields() {
    let metadata = definition_id(4);
    let value = ok(decode_definition(decode_definition_request(
        TokenDefinition::NonFungible {
            name: String::from("One of many"),
            printable_supply: u128::MAX,
            metadata_id: metadata,
        },
    )));

    assert_eq!(value["accountType"], "definition");
    assert_eq!(value["kind"], "nonFungible");
    assert_eq!(value["name"], "One of many");
    assert_eq!(value["printableSupplyRaw"], u128::MAX.to_string());
    assert_eq!(value["metadataId"], metadata.to_string());
    assert_eq!(value["metadataIdHex"], account_id_hex(metadata));
}

#[test]
fn holding_decode_reports_all_variants_and_ownership_states() {
    let definition = definition_id(9);
    let fungible = ok(decode_holding(DecodeHoldingRequest {
        token_program_id: token_program_id_hex(),
        holding: account_read(
            definition_id(5),
            &account(
                TOKEN_PROGRAM_ID,
                Data::from(&TokenHolding::Fungible {
                    definition_id: definition,
                    balance: u128::MAX,
                }),
            ),
        ),
    }));
    assert_eq!(fungible["accountType"], "holding");
    assert_eq!(fungible["kind"], "fungible");
    assert_eq!(fungible["definitionIdHex"], account_id_hex(definition));
    assert_eq!(fungible["balanceRaw"], u128::MAX.to_string());

    let master = ok(decode_holding(DecodeHoldingRequest {
        token_program_id: token_program_id_hex(),
        holding: account_read(
            definition_id(6),
            &account(
                TOKEN_PROGRAM_ID,
                Data::from(&TokenHolding::NftMaster {
                    definition_id: definition,
                    print_balance: 7,
                }),
            ),
        ),
    }));
    assert_eq!(master["kind"], "nftMaster");
    assert_eq!(master["printBalanceRaw"], "7");

    for (account_seed, owned) in [(7, false), (8, true)] {
        let copy = ok(decode_holding(DecodeHoldingRequest {
            token_program_id: token_program_id_hex(),
            holding: account_read(
                definition_id(account_seed),
                &account(
                    TOKEN_PROGRAM_ID,
                    Data::from(&TokenHolding::NftPrintedCopy {
                        definition_id: definition,
                        owned,
                    }),
                ),
            ),
        }));
        assert_eq!(copy["kind"], "nftPrintedCopy");
        assert_eq!(copy["owned"], owned);
    }
}

#[test]
fn metadata_decode_reports_both_standards_and_exact_u64() {
    let definition = definition_id(10);
    for (account_seed, standard, expected_name, primary_sale_date) in [
        (11, MetadataStandard::Simple, "simple", 0),
        (12, MetadataStandard::Expanded, "expanded", u64::MAX),
    ] {
        let value = ok(decode_metadata(DecodeMetadataRequest {
            token_program_id: token_program_id_hex(),
            metadata: account_read(
                definition_id(account_seed),
                &account(
                    TOKEN_PROGRAM_ID,
                    Data::from(&TokenMetadata {
                        definition_id: definition,
                        standard,
                        uri: String::from("ipfs://hash"),
                        creators: String::from("alice,bob"),
                        primary_sale_date,
                    }),
                ),
            ),
        }));
        assert_eq!(value["accountType"], "metadata");
        assert_eq!(value["standard"], expected_name);
        assert_eq!(value["uri"], "ipfs://hash");
        assert_eq!(value["creators"], "alice,bob");
        assert_eq!(value["primarySaleDateRaw"], primary_sale_date.to_string());
    }
}

#[test]
fn definition_decode_rejects_malformed_truncated_trailing_and_wrong_type_data() {
    let definition = TokenDefinition::Fungible {
        name: String::from("Exact"),
        total_supply: 17,
        metadata_id: None,
        authority: None,
    };
    let valid = Data::from(&definition).as_ref().to_vec();

    let mut truncated = valid.clone();
    assert!(truncated.pop().is_some());
    assert_error(
        decode_definition(definition_read_from_bytes(13, truncated)),
        "invalid_definition_data",
    );

    let mut trailing = valid;
    trailing.push(0);
    assert_error(
        decode_definition(definition_read_from_bytes(14, trailing)),
        "invalid_definition_data",
    );
    assert_error(
        decode_definition(definition_read_from_bytes(15, vec![u8::MAX])),
        "invalid_definition_data",
    );

    let holding = TokenHolding::Fungible {
        definition_id: definition_id(16),
        balance: 1,
    };
    assert_error(
        decode_definition(definition_read_from_bytes(
            17,
            Data::from(&holding).as_ref().to_vec(),
        )),
        "invalid_definition_data",
    );
}

#[test]
fn discovery_requires_one_exact_account_type_match() {
    let definition = definition_id(18);
    let value = ok(decode_account(DecodeAccountRequest {
        token_program_id: token_program_id_hex(),
        account: account_read(
            definition_id(19),
            &account(
                TOKEN_PROGRAM_ID,
                Data::from(&TokenHolding::Fungible {
                    definition_id: definition,
                    balance: 1,
                }),
            ),
        ),
    }));
    assert_eq!(value["accountType"], "holding");

    assert_error(
        decode_account(DecodeAccountRequest {
            token_program_id: token_program_id_hex(),
            account: account_read(
                definition_id(20),
                &account(TOKEN_PROGRAM_ID, Data::default()),
            ),
        }),
        "invalid_account_data",
    );

    // Exact-valid as both forms: the holding AccountId prefix encodes a
    // 26-byte definition name, and its zero balance terminates the definition.
    let mut ambiguous = Vec::new();
    ambiguous.push(0);
    ambiguous.extend_from_slice(&26_u32.to_le_bytes());
    ambiguous.extend_from_slice(&[b'a'; 26]);
    ambiguous.extend_from_slice(&[0_u8; 2]);
    ambiguous.extend_from_slice(&[0_u8; 16]);
    assert_error(
        decode_account(DecodeAccountRequest {
            token_program_id: token_program_id_hex(),
            account: account_read(
                definition_id(21),
                &account(TOKEN_PROGRAM_ID, ok(Data::try_from(ambiguous))),
            ),
        }),
        "ambiguous_account_type",
    );
}

#[test]
fn decode_rejects_wrong_program_owner_failed_reads_and_bad_identifiers() {
    assert_error(
        decode_definition(DecodeDefinitionRequest {
            token_program_id: token_program_id_hex(),
            definition: account_read(
                definition_id(22),
                &account(
                    [0x22_u32; 8],
                    Data::from(&TokenDefinition::Fungible {
                        name: String::from("Wrong"),
                        total_supply: 1,
                        metadata_id: None,
                        authority: None,
                    }),
                ),
            ),
        }),
        "token_program_mismatch",
    );

    assert_error(
        decode_holding(DecodeHoldingRequest {
            token_program_id: token_program_id_hex(),
            holding: crate::AccountRead {
                id: id_hex(23),
                status: String::from("read_failed"),
                account: None,
            },
        }),
        "account_read_failed",
    );

    assert_error(
        decode_metadata(DecodeMetadataRequest {
            token_program_id: String::from("not-a-program-id"),
            metadata: crate::AccountRead {
                id: id_hex(24),
                status: String::from("read_failed"),
                account: None,
            },
        }),
        "bad_request",
    );
}

#[test]
fn fungible_creation_plans_cover_fixed_self_and_external_authority() {
    let definition = definition_id(25);
    let self_plan = ok(create_fungible_plan(CreateFungiblePlanRequest {
        token_program_id: token_program_id_hex(),
        definition_target_id: account_id_hex(definition),
        holding_target_id: id_hex(26),
        name: String::from("Self"),
        total_supply_raw: json!(u64::MAX),
        mint_authority: String::from("self"),
    }));
    let instruction = assert_plan(
        &self_plan,
        [account_id_hex(definition), id_hex(26)],
        [true, true],
    );
    let Instruction::NewFungibleDefinition {
        name,
        total_supply,
        mint_authority,
    } = instruction
    else {
        panic!("expected NewFungibleDefinition");
    };
    assert_eq!(name, "Self");
    assert_eq!(total_supply, u128::from(u64::MAX));
    assert_eq!(mint_authority, Some(definition));

    let fixed_plan = ok(create_fungible_plan(CreateFungiblePlanRequest {
        token_program_id: token_program_id_hex(),
        definition_target_id: id_hex(27),
        holding_target_id: id_hex(28),
        name: String::from("Fixed"),
        total_supply_raw: json!(0),
        mint_authority: String::from("none"),
    }));
    let instruction = assert_plan(&fixed_plan, [id_hex(27), id_hex(28)], [true, true]);
    let Instruction::NewFungibleDefinition {
        total_supply,
        mint_authority,
        ..
    } = instruction
    else {
        panic!("expected NewFungibleDefinition");
    };
    assert_eq!(total_supply, 0);
    assert!(mint_authority.is_none());

    let authority = definition_id(29);
    let external_plan = ok(create_fungible_plan(CreateFungiblePlanRequest {
        token_program_id: token_program_id_hex(),
        definition_target_id: id_hex(30),
        holding_target_id: id_hex(31),
        name: String::from("External"),
        total_supply_raw: json!("340282366920938463463374607431768211455"),
        mint_authority: account_id_hex(authority),
    }));
    let instruction = assert_plan(&external_plan, [id_hex(30), id_hex(31)], [true, true]);
    let Instruction::NewFungibleDefinition {
        name,
        total_supply,
        mint_authority,
    } = instruction
    else {
        panic!("expected NewFungibleDefinition");
    };
    assert_eq!(name, "External");
    assert_eq!(total_supply, u128::MAX);
    assert_eq!(mint_authority, Some(authority));
}

#[test]
fn metadata_creation_plans_round_trip_all_fields_and_account_contracts() {
    let fungible_plan = ok(create_fungible_with_metadata_plan(
        CreateFungibleWithMetadataPlanRequest {
            token_program_id: token_program_id_hex(),
            definition_target_id: id_hex(32),
            holding_target_id: id_hex(33),
            metadata_target_id: id_hex(34),
            name: String::from("Meta"),
            total_supply_raw: json!(7),
            mint_authority: String::from("none"),
            metadata_standard: String::from("simple"),
            uri: String::from("ipfs://fungible"),
            creators: String::from("alice,bob"),
        },
    ));
    let instruction = assert_plan(
        &fungible_plan,
        [id_hex(32), id_hex(33), id_hex(34)],
        [true, true, true],
    );
    let Instruction::NewDefinitionWithMetadata {
        new_definition,
        metadata,
    } = instruction
    else {
        panic!("expected NewDefinitionWithMetadata");
    };
    let NewTokenDefinition::Fungible {
        name,
        total_supply,
        mint_authority,
    } = new_definition
    else {
        panic!("expected fungible definition");
    };
    assert_eq!(name, "Meta");
    assert_eq!(total_supply, 7);
    assert!(mint_authority.is_none());
    assert_eq!(metadata.standard, MetadataStandard::Simple);
    assert_eq!(metadata.uri, "ipfs://fungible");
    assert_eq!(metadata.creators, "alice,bob");

    let nft_plan = ok(create_non_fungible_plan(CreateNonFungiblePlanRequest {
        token_program_id: token_program_id_hex(),
        definition_target_id: id_hex(35),
        master_holding_target_id: id_hex(36),
        metadata_target_id: id_hex(37),
        name: String::from("NFT"),
        printable_supply_raw: json!("340282366920938463463374607431768211455"),
        metadata_standard: String::from("expanded"),
        uri: String::from("ipfs://nft"),
        creators: String::from("carol"),
    }));
    let instruction = assert_plan(
        &nft_plan,
        [id_hex(35), id_hex(36), id_hex(37)],
        [true, true, true],
    );
    let Instruction::NewDefinitionWithMetadata {
        new_definition,
        metadata,
    } = instruction
    else {
        panic!("expected NewDefinitionWithMetadata");
    };
    let NewTokenDefinition::NonFungible {
        name,
        printable_supply,
    } = new_definition
    else {
        panic!("expected non-fungible definition");
    };
    assert_eq!(name, "NFT");
    assert_eq!(printable_supply, u128::MAX);
    assert_eq!(metadata.standard, MetadataStandard::Expanded);
    assert_eq!(metadata.uri, "ipfs://nft");
    assert_eq!(metadata.creators, "carol");
}

#[test]
fn initialize_transfer_and_burn_plans_round_trip_exact_contracts() {
    let initialize = ok(initialize_holding_plan(InitializeHoldingPlanRequest {
        token_program_id: token_program_id_hex(),
        definition_id: id_hex(38),
        holding_target_id: id_hex(39),
    }));
    let instruction = assert_plan(&initialize, [id_hex(38), id_hex(39)], [false, true]);
    assert!(matches!(instruction, Instruction::InitializeAccount));

    for (fresh, signers) in [(false, [true, false]), (true, [true, true])] {
        let transfer = ok(transfer_plan(TransferPlanRequest {
            token_program_id: token_program_id_hex(),
            sender_holding_id: id_hex(40),
            recipient_holding_id: id_hex(41),
            amount_raw: json!("9"),
            recipient_is_fresh: fresh,
        }));
        let instruction = assert_plan(&transfer, [id_hex(40), id_hex(41)], signers);
        let Instruction::Transfer { amount_to_transfer } = instruction else {
            panic!("expected Transfer");
        };
        assert_eq!(amount_to_transfer, 9);
    }

    let burn = ok(burn_plan(BurnPlanRequest {
        token_program_id: token_program_id_hex(),
        definition_id: id_hex(42),
        holding_id: id_hex(43),
        amount_raw: json!("11"),
    }));
    let instruction = assert_plan(&burn, [id_hex(42), id_hex(43)], [false, true]);
    let Instruction::Burn { amount_to_burn } = instruction else {
        panic!("expected Burn");
    };
    assert_eq!(amount_to_burn, 11);
}

#[test]
fn mint_plans_cover_initialized_and_fresh_holding_signers() {
    for (fresh, signers) in [(false, [true, false]), (true, [true, true])] {
        let mint = ok(mint_plan(MintPlanRequest {
            token_program_id: token_program_id_hex(),
            definition_id: id_hex(44),
            holding_id: id_hex(45),
            amount_raw: json!("13"),
            holding_is_fresh: fresh,
        }));
        let instruction = assert_plan(&mint, [id_hex(44), id_hex(45)], signers);
        let Instruction::Mint { amount_to_mint } = instruction else {
            panic!("expected Mint");
        };
        assert_eq!(amount_to_mint, 13);
    }

    for (fresh, signers) in [(false, [false, false, true]), (true, [false, true, true])] {
        let mint = ok(mint_with_authority_plan(MintWithAuthorityPlanRequest {
            token_program_id: token_program_id_hex(),
            definition_id: id_hex(46),
            holding_id: id_hex(47),
            authority_id: id_hex(48),
            amount_raw: json!("17"),
            holding_is_fresh: fresh,
        }));
        let instruction = assert_plan(&mint, [id_hex(46), id_hex(47), id_hex(48)], signers);
        let Instruction::MintWithAuthority { amount_to_mint } = instruction else {
            panic!("expected MintWithAuthority");
        };
        assert_eq!(amount_to_mint, 17);
    }
}

#[test]
fn authority_and_print_plans_round_trip_exact_contracts() {
    let external_new_authority = definition_id(49);
    let rotate = ok(set_authority_plan(SetAuthorityPlanRequest {
        token_program_id: token_program_id_hex(),
        definition_id: id_hex(50),
        new_authority: account_id_hex(external_new_authority),
    }));
    let instruction = assert_plan(&rotate, [id_hex(50)], [true]);
    let Instruction::SetAuthority { new_authority } = instruction else {
        panic!("expected SetAuthority");
    };
    assert_eq!(new_authority, Some(external_new_authority));

    let revoke = ok(set_authority_plan(SetAuthorityPlanRequest {
        token_program_id: token_program_id_hex(),
        definition_id: id_hex(51),
        new_authority: String::from("none"),
    }));
    let instruction = assert_plan(&revoke, [id_hex(51)], [true]);
    let Instruction::SetAuthority { new_authority } = instruction else {
        panic!("expected SetAuthority");
    };
    assert!(new_authority.is_none());

    let definition = definition_id(52);
    let rotate_with_external = ok(set_authority_with_authority_plan(
        SetAuthorityWithAuthorityPlanRequest {
            token_program_id: token_program_id_hex(),
            definition_id: account_id_hex(definition),
            authority_id: id_hex(53),
            new_authority: String::from("self"),
        },
    ));
    let instruction = assert_plan(
        &rotate_with_external,
        [account_id_hex(definition), id_hex(53)],
        [false, true],
    );
    let Instruction::SetAuthorityWithAuthority { new_authority } = instruction else {
        panic!("expected SetAuthorityWithAuthority");
    };
    assert_eq!(new_authority, Some(definition));

    let print = ok(print_nft_plan(PrintNftPlanRequest {
        token_program_id: token_program_id_hex(),
        master_holding_id: id_hex(54),
        printed_holding_target_id: id_hex(55),
    }));
    let instruction = assert_plan(&print, [id_hex(54), id_hex(55)], [true, true]);
    assert!(matches!(instruction, Instruction::PrintNft));
}

#[test]
fn tail_variants_use_token_core_enum_order() {
    let print = ok(print_nft_plan(PrintNftPlanRequest {
        token_program_id: token_program_id_hex(),
        master_holding_id: id_hex(56),
        printed_holding_target_id: id_hex(57),
    }));
    let set = ok(set_authority_plan(SetAuthorityPlanRequest {
        token_program_id: token_program_id_hex(),
        definition_id: id_hex(58),
        new_authority: String::from("none"),
    }));
    let set_with = ok(set_authority_with_authority_plan(
        SetAuthorityWithAuthorityPlanRequest {
            token_program_id: token_program_id_hex(),
            definition_id: id_hex(59),
            authority_id: id_hex(60),
            new_authority: String::from("none"),
        },
    ));

    for (plan, expected_discriminant) in [(print, 7), (set, 8), (set_with, 9)] {
        let words: Vec<u32> = ok(serde_json::from_value(plan["instruction"].clone()));
        assert_eq!(words.first().copied(), Some(expected_discriminant));
    }
}

#[test]
fn amount_parser_accepts_exact_boundaries_and_cli_quote_wrapper() {
    for (raw, expected) in [
        (json!(0), 0_u128),
        (json!(1), 1_u128),
        (json!(u64::MAX), u128::from(u64::MAX)),
        (json!("340282366920938463463374607431768211455"), u128::MAX),
        (json!("\"42\""), 42_u128),
        (json!("\" 42 \""), 42_u128),
    ] {
        let plan = ok(transfer_plan(transfer_request(raw, false)));
        let instruction = assert_plan(&plan, [id_hex(80), id_hex(81)], [true, false]);
        let Instruction::Transfer { amount_to_transfer } = instruction else {
            panic!("expected Transfer");
        };
        assert_eq!(amount_to_transfer, expected);
    }
}

#[test]
fn amount_parser_rejects_overflow_negative_float_exponent_letters_and_empty() {
    let exponent_number: Value = ok(serde_json::from_str("1e3"));
    for invalid in [
        json!("340282366920938463463374607431768211456"),
        json!(-1),
        json!(1.5),
        exponent_number,
        json!("1e3"),
        json!("abc"),
        json!(""),
        json!("\"\""),
    ] {
        assert_error(
            transfer_plan(transfer_request(invalid, false)),
            "bad_amount",
        );
    }
}

#[test]
fn planner_validation_rejects_invalid_authorities_standard_and_account_ids() {
    for authority in [String::new(), String::from("invalid"), "00".repeat(32)] {
        assert_error(
            create_fungible_plan(CreateFungiblePlanRequest {
                token_program_id: token_program_id_hex(),
                definition_target_id: id_hex(61),
                holding_target_id: id_hex(62),
                name: String::from("Bad authority"),
                total_supply_raw: json!(1),
                mint_authority: authority,
            }),
            "invalid_authority",
        );
    }

    assert_error(
        create_fungible_plan(CreateFungiblePlanRequest {
            token_program_id: token_program_id_hex(),
            definition_target_id: "00".repeat(32),
            holding_target_id: id_hex(63),
            name: String::from("Bad self"),
            total_supply_raw: json!(1),
            mint_authority: String::from("self"),
        }),
        "invalid_authority",
    );

    assert_error(
        mint_with_authority_plan(MintWithAuthorityPlanRequest {
            token_program_id: token_program_id_hex(),
            definition_id: id_hex(64),
            holding_id: id_hex(65),
            authority_id: "00".repeat(32),
            amount_raw: json!(1),
            holding_is_fresh: false,
        }),
        "invalid_authority",
    );

    assert_error(
        create_non_fungible_plan(CreateNonFungiblePlanRequest {
            token_program_id: token_program_id_hex(),
            definition_target_id: id_hex(66),
            master_holding_target_id: id_hex(67),
            metadata_target_id: id_hex(68),
            name: String::from("NFT"),
            printable_supply_raw: json!(1),
            metadata_standard: String::from("bad"),
            uri: String::from("uri"),
            creators: String::from("creators"),
        }),
        "invalid_metadata_standard",
    );

    assert_error(
        initialize_holding_plan(InitializeHoldingPlanRequest {
            token_program_id: token_program_id_hex(),
            definition_id: String::from("not-an-id"),
            holding_target_id: id_hex(69),
        }),
        "invalid_account_id",
    );
}
