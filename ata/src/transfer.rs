use nssa_core::{
    account::{Account, AccountWithMetadata},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use token_core::TokenHolding;

pub fn transfer_from_associated_token_account(
    owner: AccountWithMetadata,
    sender_ata: AccountWithMetadata,
    recipient: AccountWithMetadata,
    ata_program_id: ProgramId,
    amount: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let token_program_id = sender_ata.account.program_owner;
    assert!(owner.is_authorized, "Owner authorization is missing");
    let sender_definition_id = TokenHolding::try_from(&sender_ata.account.data)
        .expect("Sender ATA must hold a valid token")
        .definition_id();
    let sender_seed = ata_core::verify_ata_and_get_seed(
        &sender_ata,
        &owner,
        sender_definition_id,
        ata_program_id,
    );

    // The recipient contract: ATA::Transfer requires a recipient token holding that is already
    // initialized, owned by the same token program as the sender ATA, and that points at the same
    // token definition as the sender. Anything else fails here rather than being silently
    // materialized by the downstream token transfer (e.g. via `Claim::Authorized` on a default
    // recipient), so integrators get an ATA-level failure rather than having to reverse-engineer
    // token/runtime semantics.
    assert_ne!(
        recipient.account,
        Account::default(),
        "Recipient token holding must be initialized"
    );
    assert_eq!(
        recipient.account.program_owner, token_program_id,
        "Recipient must be owned by the same token program as the sender ATA"
    );
    let recipient_definition_id = TokenHolding::try_from(&recipient.account.data)
        .expect("Recipient must hold a valid token")
        .definition_id();
    assert_eq!(
        recipient_definition_id, sender_definition_id,
        "Recipient and sender token definitions do not match"
    );

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
