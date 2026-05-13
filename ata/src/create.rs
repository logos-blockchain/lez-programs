use nssa_core::{
    account::{Account, AccountWithMetadata},
    program::{AccountPostState, ChainedCall, Claim, ProgramId},
};
use token_core::{TokenDefinition, TokenHolding};

pub fn create_associated_token_account(
    owner: AccountWithMetadata,
    token_definition: AccountWithMetadata,
    ata_account: AccountWithMetadata,
    ata_program_id: ProgramId,
    token_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    // No explicit owner authorization check is needed here: ATA creation is idempotent, so the
    // call itself may proceed without `owner.is_authorized`. If the owner account is still
    // default, the returned post-state will still carry `Claim::Authorized` so the runtime can
    // claim that owner account when needed.
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
                AccountPostState::new_claimed_if_default(owner.account.clone(), Claim::Authorized),
                AccountPostState::new(token_definition.account.clone()),
                AccountPostState::new(ata_account.account.clone()),
            ],
            vec![],
        );
    }

    let post_states = vec![
        AccountPostState::new_claimed_if_default(owner.account.clone(), Claim::Authorized),
        AccountPostState::new(token_definition.account.clone()),
        AccountPostState::new(ata_account.account.clone()),
    ];
    let mut ata_account_auth = ata_account.clone();
    ata_account_auth.is_authorized = true;
    let chained_call = ChainedCall::new(
        token_program_id,
        vec![token_definition.clone(), ata_account_auth],
        &token_core::Instruction::InitializeAccount,
    )
    .with_pda_seeds(vec![seed]);
    (post_states, vec![chained_call])
}
