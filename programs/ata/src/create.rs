use lee_core::{
    account::{Account, AccountId, AccountWithMetadata},
    program::{AccountStateDiff, ChainedCall},
};
use token_core::{TokenDefinition, TokenHolding};

pub fn create_associated_token_account(
    owner: AccountWithMetadata,
    token_definition: AccountWithMetadata,
    ata_account: AccountWithMetadata,
    ata_program_id: AccountId,
    token_program_id: AccountId,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    // No explicit owner authorization check is needed here: ATA creation is idempotent, so the
    // call itself may proceed without `owner.is_authorized`. Nothing writes to the owner account,
    // so it is reported unchanged — since LEZ v0.2.5 ownership is acquired by writing data to a
    // default-owned account, not by claiming one.
    assert_eq!(
        token_definition.account.program_owner, token_program_id,
        "Token definition must be owned by expected token program"
    );
    let _definition = TokenDefinition::try_from(&token_definition.account.data)
        .expect("Token definition must be valid");
    let seed = ata_core::verify_ata_and_get_seed(
        &ata_account,
        &owner,
        token_program_id,
        token_definition.account_id,
        ata_program_id,
    );

    // Idempotent: already initialized → no-op
    if ata_account.account != Account::default() {
        assert_eq!(
            ata_account.account.program_owner, token_program_id,
            "Existing ATA must be owned by expected token program"
        );
        let holding = TokenHolding::try_from(&ata_account.account.data)
            .expect("Existing ATA must hold a valid token");
        assert_eq!(
            holding.definition_id(),
            token_definition.account_id,
            "Existing ATA token definition does not match"
        );
        return (
            vec![
                AccountStateDiff::unchanged(owner),
                AccountStateDiff::unchanged(token_definition),
                AccountStateDiff::unchanged(ata_account),
            ],
            vec![],
        );
    }

    let definition_id = token_definition.account_id;
    let ata_account_id = ata_account.account_id;

    // Every declared account is reported, untouched ones included: v0.2.5 fails a
    // transaction whose output omits one. The ATA itself is written by the token
    // program in the chained call below, not here.
    let state_diffs = vec![
        AccountStateDiff::unchanged(owner),
        AccountStateDiff::unchanged(token_definition),
        AccountStateDiff::unchanged(ata_account),
    ];

    // The call names its pre-states by id; the callee's authority over the ATA comes
    // from `pda_seeds` — LEZ derives the authorized set as
    // `for_public_pda(caller_account_id, seed)` — rather than from a pre-state flag.
    let chained_call = ChainedCall::new(
        token_program_id,
        vec![definition_id, ata_account_id],
        &token_core::Instruction::InitializeAccount,
    )
    .with_pda_seeds(vec![seed]);
    (state_diffs, vec![chained_call])
}
