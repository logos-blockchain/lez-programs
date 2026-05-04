use nssa_core::{
    account::AccountWithMetadata,
    program::{AccountPostState, ChainedCall, ProgramId},
};

pub fn burn_from_associated_token_account(
    owner: AccountWithMetadata,
    holder_ata: AccountWithMetadata,
    token_definition: AccountWithMetadata,
    ata_program_id: ProgramId,
    token_program_id: ProgramId,
    amount: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(owner.is_authorized, "Owner authorization is missing");
    let _definition =
        crate::validation::canonical_token_definition(&token_definition, token_program_id);
    let definition_id =
        crate::validation::canonical_ata_holding(&holder_ata, token_program_id).definition_id();
    assert_eq!(
        definition_id, token_definition.account_id,
        "Holder ATA token definition mismatch"
    );
    let seed =
        ata_core::verify_ata_and_get_seed(&holder_ata, &owner, definition_id, ata_program_id);

    let post_states = vec![
        AccountPostState::new(owner.account.clone()),
        AccountPostState::new(holder_ata.account.clone()),
        AccountPostState::new(token_definition.account.clone()),
    ];
    let mut holder_ata_auth = holder_ata.clone();
    holder_ata_auth.is_authorized = true;

    let chained_call = ChainedCall::new(
        token_program_id,
        vec![token_definition.clone(), holder_ata_auth],
        &token_core::Instruction::Burn {
            amount_to_burn: amount,
        },
    )
    .with_pda_seeds(vec![seed]);
    (post_states, vec![chained_call])
}
