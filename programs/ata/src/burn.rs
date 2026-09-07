use ata_core::error;
use lee_core::{
    account::AccountWithMetadata,
    program::{AccountPostState, ChainedCall, ProgramId},
};
use program_revert::UnwrapOrRevert as _;
use token_core::TokenHolding;

pub fn burn_from_associated_token_account(
    owner: AccountWithMetadata,
    holder_ata: AccountWithMetadata,
    token_definition: AccountWithMetadata,
    ata_program_id: ProgramId,
    token_program_id: ProgramId,
    amount: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    program_revert::require!(
        error::INVALID_INPUT,
        owner.is_authorized,
        "Owner authorization is missing"
    );
    program_revert::require_eq!(
        error::INVALID_INPUT,
        holder_ata.account.program_owner,
        token_program_id,
        "Holder ATA must be owned by expected token program"
    );
    program_revert::require_eq!(
        error::INVALID_INPUT,
        token_definition.account.program_owner,
        token_program_id,
        "Token definition must be owned by expected token program"
    );
    let definition_id = TokenHolding::try_from(&holder_ata.account.data)
        .unwrap_or_revert(error::INVALID_INPUT, "Holder ATA must hold a valid token")
        .definition_id();
    program_revert::require_eq!(
        error::INVALID_INPUT,
        definition_id,
        token_definition.account_id,
        "Holder ATA token definition does not match"
    );
    let seed = ata_core::verify_ata_and_get_seed(
        &holder_ata,
        &owner,
        token_program_id,
        definition_id,
        ata_program_id,
    );

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
