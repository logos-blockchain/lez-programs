use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::ProgramId,
};
use serde_json::{json, Value};
use stablecoin_core::{
    compute_position_pda, compute_position_vault_pda, compute_protocol_parameters_pda,
    compute_stablecoin_definition_pda, math::FIXED_POINT_ONE, Instruction, Position,
    ProtocolParameters,
};
use token_core::{TokenDefinition, TokenHolding};

use super::{close_position_plan, ClosePositionPlanRequest};
use crate::{
    account::{account_id_hex, account_read, decode_account, program_id_bytes},
    AccountRead,
};

const PROGRAM: ProgramId = [0x11; 8];
const TOKEN_PROGRAM: ProgramId = [0x22; 8];

fn id(seed: u8) -> AccountId {
    AccountId::new([seed; 32])
}

fn read(account_id: AccountId, owner: ProgramId, data: Data) -> AccountRead {
    account_read(
        account_id,
        &Account {
            program_owner: owner,
            data,
            balance: 17,
            nonce: Nonce(7),
        },
    )
}

struct Fixture {
    position: Position,
    parameters: ProtocolParameters,
}

impl Fixture {
    fn new(nonce: u64, frozen: bool) -> Self {
        let position_id = compute_position_pda(PROGRAM, id(20), nonce);
        Self {
            position: Position {
                owner_account_id: id(20),
                position_nonce: nonce,
                vault_account_id: compute_position_vault_pda(PROGRAM, position_id),
                collateral_amount: 0,
                normalized_debt_amount: 0,
                opened_at: 1_000,
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
                minimum_milliseconds_between_rate_updates: 50,
                maximum_oracle_price_age_milliseconds: 50,
                is_frozen: frozen,
            },
        }
    }

    fn request(&self) -> ClosePositionPlanRequest {
        ClosePositionPlanRequest {
            stablecoin_program_id: hex::encode(program_id_bytes(PROGRAM)),
            owner_id: account_id_hex(id(20)),
            position_nonce: self.position.position_nonce.to_string(),
            position: read(
                compute_position_pda(PROGRAM, id(20), self.position.position_nonce),
                PROGRAM,
                Data::from(&self.position),
            ),
            vault: read(
                self.position.vault_account_id,
                TOKEN_PROGRAM,
                Data::from(&TokenHolding::Fungible {
                    definition_id: id(4),
                    balance: 0,
                }),
            ),
            protocol_parameters: read(
                compute_protocol_parameters_pda(PROGRAM),
                PROGRAM,
                Data::from(&self.parameters),
            ),
        }
    }
}

fn error(request: ClosePositionPlanRequest, expected: &str) {
    assert_eq!(
        close_position_plan(request)
            .expect_err("preflight must reject")
            .code(),
        expected
    );
}

#[test]
fn close_pins_zero_argument_serialization_pdas_and_guest_account_flags() {
    let request = Fixture::new(u64::MAX, false).request();
    let plan = close_position_plan(request.clone()).expect("settled position");
    assert_eq!(plan["programId"], request.stablecoin_program_id);
    assert_eq!(
        plan["accountIds"],
        json!([
            request.owner_id,
            request.position.id,
            request.vault.id,
            request.protocol_parameters.id,
        ])
    );
    assert_eq!(
        plan["signingRequirements"],
        json!([true, false, false, false])
    );
    let words: Vec<u32> = serde_json::from_value(plan["instruction"].clone()).expect("words");
    assert_eq!(
        words,
        risc0_zkvm::serde::to_vec(&Instruction::ClosePosition).expect("encode")
    );
    assert_eq!(words.len(), 1);
    assert!(matches!(
        risc0_zkvm::serde::from_slice::<Instruction, u32>(&words).expect("decode"),
        Instruction::ClosePosition
    ));

    let idl: Value =
        serde_json::from_str(include_str!("../../../../../artifacts/stablecoin-idl.json"))
            .expect("IDL");
    let entry = idl["instructions"]
        .as_array()
        .expect("instructions")
        .iter()
        .find(|entry| entry["name"] == "close_position")
        .expect("close entry");
    assert_eq!(entry["args"], json!([]));
    assert_eq!(
        entry["accounts"],
        json!([
            {"name":"owner", "writable":false, "signer":true, "init":false},
            {"name":"position", "writable":true, "signer":false, "init":false},
            {"name":"vault", "writable":false, "signer":false, "init":false},
            {"name":"protocol_parameters", "writable":false, "signer":false, "init":false},
        ])
    );
}

#[test]
fn planned_close_preserves_accounts_and_cannot_release_or_reopen_the_pda() {
    for frozen in [false, true] {
        let fixture = Fixture::new(u64::MAX, frozen);
        let mut request = fixture.request();
        let plan = close_position_plan(request.clone()).expect("closure is allowed while frozen");
        let reads = [
            &request.position,
            &request.vault,
            &request.protocol_parameters,
        ];
        let ids: Vec<String> = serde_json::from_value(plan["accountIds"].clone()).expect("ids");
        let signers: Vec<bool> =
            serde_json::from_value(plan["signingRequirements"].clone()).expect("signers");
        let owner_account = Account {
            balance: 11,
            nonce: Nonce(3),
            ..Account::default()
        };
        let inputs: Vec<_> = ids
            .iter()
            .zip(signers)
            .map(|(account_id, signer)| {
                let (account_id, account) = if account_id == &request.owner_id {
                    (id(20), owner_account.clone())
                } else {
                    decode_account(
                        reads
                            .iter()
                            .find(|read| &read.id == account_id)
                            .expect("read"),
                    )
                    .expect("account")
                };
                AccountWithMetadata::new(account, signer, account_id)
            })
            .collect();
        let [owner, position, vault, parameters]: [_; 4] = inputs.try_into().expect("four inputs");
        let old_position = position.account.clone();
        let old_vault = vault.account.clone();
        let old_parameters = parameters.account.clone();
        let words: Vec<u32> = serde_json::from_value(plan["instruction"].clone()).expect("words");
        assert!(matches!(
            risc0_zkvm::serde::from_slice::<Instruction, u32>(&words).expect("decode"),
            Instruction::ClosePosition
        ));
        let (posts, calls) = stablecoin_program::close_position::close_position(
            owner.clone(),
            position,
            vault.clone(),
            parameters.clone(),
            PROGRAM,
        );
        assert!(calls.is_empty());
        assert_eq!(posts.len(), 4);
        assert_eq!(posts.first().expect("owner post").account(), &owner_account);
        let closed = posts.get(1).expect("position post").account();
        assert_eq!(
            closed,
            &Account {
                data: Data::default(),
                ..old_position
            }
        );
        assert_eq!(closed.program_owner, PROGRAM);
        assert_eq!(closed.nonce, Nonce(7));
        assert_eq!(posts.get(2).expect("vault post").account(), &old_vault);
        assert_eq!(
            posts.get(3).expect("parameters post").account(),
            &old_parameters
        );
        request.position = account_read(compute_position_pda(PROGRAM, id(20), u64::MAX), closed);
        error(request, "invalid_position_data");

        // Deliberately attempt to violate the fresh-position invariant with the
        // real closure result. Even unfrozen, this PDA cannot be reopened.
        let mut unfrozen = fixture.parameters.clone();
        unfrozen.is_frozen = false;
        let parameters = AccountWithMetadata::new(
            Account {
                data: Data::from(&unfrozen),
                ..parameters.account
            },
            false,
            parameters.account_id,
        );
        let holding = AccountWithMetadata::new(
            Account {
                program_owner: TOKEN_PROGRAM,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: id(4),
                    balance: 1,
                }),
                ..Account::default()
            },
            true,
            id(21),
        );
        let definition = AccountWithMetadata::new(
            Account {
                program_owner: TOKEN_PROGRAM,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("Collateral"),
                    total_supply: 1,
                    metadata_id: None,
                    authority: None,
                }),
                ..Account::default()
            },
            false,
            id(4),
        );
        let clock = AccountWithMetadata::new(
            Account {
                data: Data::try_from(
                    ClockAccountData {
                        block_id: 1,
                        timestamp: 1_000,
                    }
                    .to_bytes(),
                )
                .expect("clock fits"),
                ..Account::default()
            },
            false,
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        );
        let position = AccountWithMetadata::new(
            closed.clone(),
            false,
            compute_position_pda(PROGRAM, id(20), u64::MAX),
        );
        let failure = std::panic::catch_unwind(|| {
            stablecoin_program::open_position::open_position(
                owner,
                position,
                vault,
                holding,
                definition,
                parameters,
                clock,
                PROGRAM,
                u64::MAX,
                0,
            )
        })
        .expect_err("closed position is not uninitialized");
        let message = failure.downcast_ref::<String>().expect("assertion message");
        assert!(message.contains("Position account must be uninitialized"));
    }
}

#[test]
fn debt_recorded_collateral_and_actual_vault_balance_block_independently() {
    let mut fixture = Fixture::new(9, false);
    fixture.position.normalized_debt_amount = 1;
    error(fixture.request(), "position_has_debt");
    fixture.position.normalized_debt_amount = 0;
    fixture.position.collateral_amount = 1;
    error(fixture.request(), "position_has_collateral");
    fixture.position.collateral_amount = 0;
    let mut request = fixture.request();
    // A real public Token transfer can donate to the vault without changing
    // Position accounting or requiring the Position owner's signature.
    let sender = AccountWithMetadata::new(
        Account {
            program_owner: TOKEN_PROGRAM,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: id(4),
                balance: 1,
            }),
            ..Account::default()
        },
        true,
        id(21),
    );
    let (vault_id, vault) = decode_account(&request.vault).expect("vault");
    let posts = token_program::transfer::transfer(
        sender,
        AccountWithMetadata::new(vault, false, vault_id),
        1,
    );
    request.vault = account_read(vault_id, posts.get(1).expect("donated vault").account());
    assert_eq!(fixture.position.collateral_amount, 0);
    error(request, "vault_not_empty");
}

#[test]
fn close_validates_pdas_owners_and_stored_position_identity() {
    let fixture = Fixture::new(u64::MAX, false);
    let request = fixture.request();
    let mut wrong = request.clone();
    wrong.position.id = account_id_hex(id(99));
    error(wrong, "position_pda_mismatch");
    let mut wrong = request.clone();
    wrong
        .position
        .account
        .as_mut()
        .expect("position")
        .program_owner = hex::encode(program_id_bytes(TOKEN_PROGRAM));
    error(wrong, "stablecoin_program_mismatch");
    let mut wrong = request.clone();
    wrong.protocol_parameters.id = account_id_hex(id(99));
    error(wrong, "protocol_parameters_pda_mismatch");
    let mut wrong = request.clone();
    wrong
        .protocol_parameters
        .account
        .as_mut()
        .expect("parameters")
        .program_owner = hex::encode(program_id_bytes(TOKEN_PROGRAM));
    error(wrong, "stablecoin_program_mismatch");
    let mut wrong = request.clone();
    wrong.vault.id = account_id_hex(id(99));
    error(wrong, "vault_pda_mismatch");
    for (field, expected) in [
        (0, "position_owner_mismatch"),
        (1, "position_nonce_mismatch"),
        (2, "position_vault_mismatch"),
    ] {
        let mut position = fixture.position.clone();
        match field {
            0 => position.owner_account_id = id(99),
            1 => position.position_nonce -= 1,
            _ => position.vault_account_id = id(99),
        }
        let mut wrong = request.clone();
        wrong.position = read(
            compute_position_pda(PROGRAM, id(20), u64::MAX),
            PROGRAM,
            Data::from(&position),
        );
        error(wrong, expected);
    }
}

#[test]
fn close_requires_exact_position_parameters_and_fungible_vault_data() {
    let request = Fixture::new(9, false).request();
    for (field, expected) in [
        (0, "invalid_position_data"),
        (1, "invalid_protocol_parameters_data"),
        (2, "invalid_position_vault"),
    ] {
        for corruption in 0..3 {
            let mut wrong = request.clone();
            let read = match field {
                0 => &mut wrong.position,
                1 => &mut wrong.protocol_parameters,
                _ => &mut wrong.vault,
            };
            let account = read.account.as_mut().expect("account");
            match corruption {
                0 => account.data.clear(),
                1 => {
                    account.data.truncate(account.data.len() - 2);
                }
                _ => account.data.push_str("00"),
            }
            error(wrong, expected);
        }
    }
    for holding in [
        TokenHolding::NftMaster {
            definition_id: id(4),
            print_balance: 0,
        },
        TokenHolding::NftPrintedCopy {
            definition_id: id(4),
            owned: false,
        },
    ] {
        let mut wrong = request.clone();
        wrong.vault = read(
            compute_position_vault_pda(PROGRAM, compute_position_pda(PROGRAM, id(20), 9)),
            TOKEN_PROGRAM,
            Data::from(&holding),
        );
        error(wrong, "invalid_position_vault");
    }
    for field in 0..3 {
        for missing in [false, true] {
            let mut wrong = request.clone();
            let read = match field {
                0 => &mut wrong.position,
                1 => &mut wrong.protocol_parameters,
                _ => &mut wrong.vault,
            };
            if missing {
                read.account = None;
            } else {
                read.status = String::from("not_found");
            }
            error(wrong, "account_read_failed");
        }
    }
}

#[test]
fn close_preserves_exact_nonce_boundaries_and_accepts_base58_owner() {
    for nonce in [0, (1 << 53) + 1, u64::MAX] {
        let mut request = Fixture::new(nonce, false).request();
        request.owner_id = format!(" {} ", id(20));
        assert_eq!(
            close_position_plan(request).expect("exact nonce")["accountIds"][0],
            account_id_hex(id(20))
        );
    }
    for nonce in ["", "-1", "1.5", "+1", " 1", "18446744073709551616"] {
        let mut request = Fixture::new(0, false).request();
        request.position_nonce = String::from(nonce);
        error(request, "invalid_numeric_value");
    }
    for owner in [
        "invalid",
        "",
        "0000000000000000000000000000000000000000000000000000000000000000",
    ] {
        let mut request = Fixture::new(0, false).request();
        request.owner_id = String::from(owner);
        error(request, "invalid_account_id");
    }
    for program in [
        "",
        "0000000000000000000000000000000000000000000000000000000000000000",
    ] {
        let mut request = Fixture::new(0, false).request();
        request.stablecoin_program_id = String::from(program);
        error(request, "invalid_program_id");
    }
}
