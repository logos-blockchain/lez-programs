use nssa_core::{
    account::AccountWithMetadata,
    program::{AccountPostState, ChainedCall, ProgramId},
};

pub fn transfer_from_associated_token_account(
    owner: AccountWithMetadata,
    sender_ata: AccountWithMetadata,
    recipient: AccountWithMetadata,
    ata_program_id: ProgramId,
    token_program_id: ProgramId,
    amount: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(owner.is_authorized, "Owner authorization is missing");
    let definition_id =
        crate::validation::canonical_ata_holding(&sender_ata, token_program_id).definition_id();
    let sender_seed =
        ata_core::verify_ata_and_get_seed(&sender_ata, &owner, definition_id, ata_program_id);

    let post_states = vec![
        AccountPostState::new(owner.account.clone()),
        AccountPostState::new(sender_ata.account.clone()),
        AccountPostState::new(recipient.account.clone()),
    ];
    let mut sender_ata_auth = sender_ata.clone();
    sender_ata_auth.is_authorized = true;

    let chained_call = ChainedCall::new(
        token_program_id,
        vec![sender_ata_auth, recipient],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
        },
    )
    .with_pda_seeds(vec![sender_seed]);
    (post_states, vec![chained_call])
}
