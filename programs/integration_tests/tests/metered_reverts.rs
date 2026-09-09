//! Exercise the executor directly: the pinned LEZ host predates metered reverts.
//! A panic loses the session; an expected rejection must retain a nonzero halt,
//! consumed cycles, and no committed output (including no chained calls).

use std::num::NonZeroU8;

use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{ProgramId, ProgramOutput},
};
use risc0_zkvm::{default_executor, ExecutorEnv, ExitCode, SessionInfo};
use serde::Serialize;
use token_core::TokenHolding;

const CYCLE_LIMIT: u64 = 8 * 1024 * 1024;

fn account(id: u8, authorized: bool) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        account_id: AccountId::new([id; 32]),
        is_authorized: authorized,
    }
}

fn holding(id: u8, balance: u128, authorized: bool) -> AccountWithMetadata {
    let mut value = account(id, authorized);
    value.account.program_owner = token_methods::TOKEN_ID;
    value.account.data = Data::from(&TokenHolding::Fungible {
        definition_id: AccountId::new([9; 32]),
        balance,
    });
    value
}

fn environment(
    id: ProgramId,
    accounts: &[AccountWithMetadata],
    instruction: &[u32],
) -> ExecutorEnv<'static> {
    ExecutorEnv::builder()
        .session_limit(Some(CYCLE_LIMIT))
        .write(&id)
        .expect("program id")
        .write(&None::<ProgramId>)
        .expect("caller id")
        .write(&accounts)
        .expect("accounts")
        .write(&instruction)
        .expect("instruction words")
        .build()
        .expect("executor environment")
}

fn execute(
    binary: &[u8],
    id: ProgramId,
    accounts: &[AccountWithMetadata],
    instruction: &impl Serialize,
) -> SessionInfo {
    let words = risc0_zkvm::serde::to_vec(instruction).expect("serialize instruction");
    default_executor()
        .execute(environment(id, accounts, &words), binary)
        .expect("expected rejection must preserve the executor session")
}

fn assert_revert(session: SessionInfo, code: NonZeroU8) {
    assert_eq!(session.exit_code, ExitCode::Halted(u32::from(code.get())));
    assert!(session.cycles() > 0);
    assert!(session.cycles() < CYCLE_LIMIT);
    assert!(
        session.journal.bytes.is_empty(),
        "reverts must not commit output"
    );
}

#[test]
fn token_insufficient_balance_and_overflow_preserve_metered_sessions() {
    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: 6,
    };
    assert_revert(
        execute(
            token_methods::TOKEN_ELF,
            token_methods::TOKEN_ID,
            &[holding(1, 5, true), holding(2, 0, false)],
            &instruction,
        ),
        token_core::error::INSUFFICIENT_BALANCE,
    );
    assert_revert(
        execute(
            token_methods::TOKEN_ELF,
            token_methods::TOKEN_ID,
            &[holding(1, 6, true), holding(2, u128::MAX, false)],
            &instruction,
        ),
        token_core::error::ARITHMETIC,
    );
}

#[test]
fn token_transfer_at_balance_boundary_still_commits_success() {
    let session = execute(
        token_methods::TOKEN_ELF,
        token_methods::TOKEN_ID,
        &[holding(1, 5, true), holding(2, 0, false)],
        &token_core::Instruction::Transfer {
            amount_to_transfer: 5,
        },
    );
    assert_eq!(session.exit_code, ExitCode::Halted(0));
    let output: ProgramOutput = session.journal.decode().expect("successful output");
    assert_eq!(output.post_states.len(), 2);
    for (post, expected) in output.post_states.iter().zip([0, 5]) {
        let value = TokenHolding::try_from(&post.account().data).expect("holding");
        assert_eq!(
            value,
            TokenHolding::Fungible {
                definition_id: AccountId::new([9; 32]),
                balance: expected,
            }
        );
    }
}

#[test]
fn framework_signer_init_and_account_count_failures_are_metered() {
    let transfer = token_core::Instruction::Transfer {
        amount_to_transfer: 1,
    };
    for accounts in [
        vec![holding(1, 5, false), holding(2, 0, false)],
        vec![holding(1, 5, true)],
        vec![holding(1, 5, true), holding(2, 0, false), account(3, false)],
    ] {
        assert_revert(
            execute(
                token_methods::TOKEN_ELF,
                token_methods::TOKEN_ID,
                &accounts,
                &transfer,
            ),
            token_core::error::INVALID_INPUT,
        );
    }
    assert_revert(
        execute(
            token_methods::TOKEN_ELF,
            token_methods::TOKEN_ID,
            &[holding(1, 5, true), account(2, true)],
            &token_core::Instruction::NewFungibleDefinition {
                name: "Token".into(),
                total_supply: 5,
                mint_authority: None,
            },
        ),
        token_core::error::INVALID_INPUT,
    );
}

#[test]
fn invalid_instruction_discriminant_is_metered() {
    let session = default_executor()
        .execute(
            environment(token_methods::TOKEN_ID, &[], &[u32::MAX]),
            token_methods::TOKEN_ELF,
        )
        .expect("invalid instruction must preserve the session");
    assert_revert(session, token_core::error::INVALID_INPUT);
}

#[test]
fn malformed_token_name_lengths_are_metered() {
    // NewFungibleDefinition followed by a truncated name. The high-bit lengths
    // used to panic in the 32-bit guest allocator before returning a decode error.
    for name_length in [0x8000_0000, u32::MAX, 1, 5] {
        let session = default_executor()
            .execute(
                environment(
                    token_methods::TOKEN_ID,
                    &[account(1, true), account(2, true)],
                    &[1, name_length],
                ),
                token_methods::TOKEN_ELF,
            )
            .expect("malformed string lengths must preserve the executor session");
        assert_revert(session, token_core::error::INVALID_INPUT);
    }
}

#[test]
fn oversized_token_name_is_an_expected_revert() {
    // Data holds at most 100 KiB. The name is caller supplied, unlike fixed-size
    // serialized program state whose conversion failure remains an invariant bug.
    assert_revert(
        execute(
            token_methods::TOKEN_ELF,
            token_methods::TOKEN_ID,
            &[account(1, true), account(2, true)],
            &token_core::Instruction::NewFungibleDefinition {
                name: "x".repeat(100 * 1024),
                total_supply: 1,
                mint_authority: None,
            },
        ),
        token_core::error::INVALID_INPUT,
    );
}

#[test]
fn ata_chained_token_rejection_preserves_callee_cycles() {
    let owner = account(3, true);
    let seed = ata_core::compute_ata_seed(
        token_methods::TOKEN_ID,
        owner.account_id,
        AccountId::new([9; 32]),
    );
    let mut sender = holding(1, 5, false);
    sender.account_id = ata_core::get_associated_token_account_id(&ata_methods::ATA_ID, &seed);
    let session = execute(
        ata_methods::ATA_ELF,
        ata_methods::ATA_ID,
        &[owner, sender, holding(2, 0, false)],
        &ata_core::Instruction::Transfer {
            token_program_id: token_methods::TOKEN_ID,
            amount: 6,
        },
    );
    assert_eq!(session.exit_code, ExitCode::Halted(0));
    let output: ProgramOutput = session.journal.decode().expect("ATA output");
    let [call] = output.chained_calls.as_slice() else {
        panic!("ATA must emit exactly one token call");
    };
    assert_eq!(call.program_id, token_methods::TOKEN_ID);
    assert_eq!(call.pda_seeds, vec![seed]);
    let env = ExecutorEnv::builder()
        .session_limit(Some(CYCLE_LIMIT))
        .write(&call.program_id)
        .expect("callee id")
        .write(&Some(ata_methods::ATA_ID))
        .expect("caller id")
        .write(&call.pre_states)
        .expect("callee accounts")
        .write(&call.instruction_data)
        .expect("callee instruction")
        .build()
        .expect("callee environment");
    let callee = default_executor()
        .execute(env, token_methods::TOKEN_ELF)
        .expect("callee rejection must retain a session");
    assert_revert(callee, token_core::error::INSUFFICIENT_BALANCE);
}

#[test]
fn amm_wrong_config_pda_is_metered() {
    assert_revert(
        execute(
            amm_methods::AMM_ELF,
            amm_methods::AMM_ID,
            &[account(1, false)],
            &amm_core::Instruction::Initialize {
                token_program_id: token_methods::TOKEN_ID,
                twap_oracle_program_id: twap_oracle_methods::TWAP_ORACLE_ID,
                authority: AccountId::new([2; 32]),
            },
        ),
        amm_core::error::INVALID_INPUT,
    );
}

#[test]
fn ata_wrong_token_owner_is_metered() {
    assert_revert(
        execute(
            ata_methods::ATA_ELF,
            ata_methods::ATA_ID,
            &[account(1, false), account(2, false), account(3, false)],
            &ata_core::Instruction::Create {
                token_program_id: token_methods::TOKEN_ID,
            },
        ),
        ata_core::error::INVALID_INPUT,
    );
}

#[test]
fn stablecoin_wrong_position_pda_is_metered() {
    let mut definition = account(9, false);
    definition.account.program_owner = token_methods::TOKEN_ID;
    assert_revert(
        execute(
            stablecoin_methods::STABLECOIN_ELF,
            stablecoin_methods::STABLECOIN_ID,
            &[
                account(1, true),
                account(2, false),
                account(3, false),
                holding(4, 10, true),
                definition,
            ],
            &stablecoin_core::Instruction::OpenPosition {
                position_nonce: 0,
                collateral_amount: 1,
            },
        ),
        stablecoin_core::error::INVALID_INPUT,
    );
}

#[test]
fn oracle_wrong_tick_pda_is_metered() {
    assert_revert(
        execute(
            twap_oracle_methods::TWAP_ORACLE_ELF,
            twap_oracle_methods::TWAP_ORACLE_ID,
            &[account(1, false), account(2, true), account(3, false)],
            &twap_oracle_core::Instruction::CreateCurrentTickAccount { initial_price: 1 },
        ),
        twap_oracle_core::error::INVALID_INPUT,
    );
}

#[test]
fn corrupted_system_clock_still_panics() {
    // Deliberately violate the host's system-account invariant to verify that
    // internal panics are not intercepted by the expected-error mechanism.
    let source = account(2, true);
    let mut tick = account(1, false);
    tick.account_id = twap_oracle_core::compute_current_tick_account_pda(
        twap_oracle_methods::TWAP_ORACLE_ID,
        source.account_id,
    );
    let mut clock = account(3, false);
    clock.account_id = clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
    let words =
        risc0_zkvm::serde::to_vec(&twap_oracle_core::Instruction::CreateCurrentTickAccount {
            initial_price: 1,
        })
        .expect("instruction");
    let error = default_executor()
        .execute(
            environment(
                twap_oracle_methods::TWAP_ORACLE_ID,
                &[tick, source, clock],
                &words,
            ),
            twap_oracle_methods::TWAP_ORACLE_ELF,
        )
        .expect_err("corrupted clock must panic without a session");
    assert!(format!("{error:#}").contains("Guest panicked"));
}
