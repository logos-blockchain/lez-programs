use lee_core::{
    account::{Account, AccountId, AccountWithMetadata},
    program::{AccountStateDiff, ChainedCall},
};
use token_core::TokenHolding;

pub fn transfer_from_associated_token_account(
    owner: AccountWithMetadata,
    sender_ata: AccountWithMetadata,
    recipient: AccountWithMetadata,
    ata_program_id: AccountId,
    token_program_id: AccountId,
    amount: u128,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    assert!(owner.is_authorized, "Owner authorization is missing");
    assert_eq!(
        sender_ata.account.program_owner, token_program_id,
        "Sender ATA must be owned by expected token program"
    );
    let sender_definition_id = TokenHolding::try_from(&sender_ata.account.data)
        .expect("Sender ATA must hold a valid token")
        .definition_id();
    let sender_seed = ata_core::verify_ata_and_get_seed(
        &sender_ata,
        &owner,
        token_program_id,
        sender_definition_id,
        ata_program_id,
    );

    // The recipient contract: ATA::Transfer requires a recipient token holding that is already
    // initialized, owned by the same token program as the sender ATA, and that points at the same
    // token definition as the sender. Anything else fails here rather than being silently
    // materialized by the downstream token transfer writing into a default recipient, so
    // integrators get an ATA-level failure rather than having to reverse-engineer
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

    let sender_ata_id = sender_ata.account_id;
    let recipient_id = recipient.account_id;

    // The balances move in the chained call, executed by the token program that owns
    // these holdings; this program reports them unchanged but must still report them.
    let state_diffs = vec![
        AccountStateDiff::unchanged(owner),
        AccountStateDiff::unchanged(sender_ata),
        AccountStateDiff::unchanged(recipient),
    ];

    // `pda_seeds` is what authorizes the callee over the sender ATA — the call carries
    // account ids, not pre-states, so there is no flag to set.
    let chained_call = ChainedCall::new(
        token_program_id,
        vec![sender_ata_id, recipient_id],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
        },
    )
    .with_pda_seeds(vec![sender_seed]);
    (state_diffs, vec![chained_call])
}
