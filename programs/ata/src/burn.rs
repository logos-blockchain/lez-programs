use lee_core::{
    account::{AccountId, AccountWithMetadata},
    program::{AccountStateDiff, ChainedCall},
};
use token_core::TokenHolding;

pub fn burn_from_associated_token_account(
    owner: AccountWithMetadata,
    holder_ata: AccountWithMetadata,
    token_definition: AccountWithMetadata,
    ata_program_id: AccountId,
    token_program_id: AccountId,
    amount: u128,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    assert!(owner.is_authorized, "Owner authorization is missing");
    assert_eq!(
        holder_ata.account.program_owner, token_program_id,
        "Holder ATA must be owned by expected token program"
    );
    assert_eq!(
        token_definition.account.program_owner, token_program_id,
        "Token definition must be owned by expected token program"
    );
    let definition_id = TokenHolding::try_from(&holder_ata.account.data)
        .expect("Holder ATA must hold a valid token")
        .definition_id();
    assert_eq!(
        definition_id, token_definition.account_id,
        "Holder ATA token definition does not match"
    );
    let seed = ata_core::verify_ata_and_get_seed(
        &holder_ata,
        &owner,
        token_program_id,
        definition_id,
        ata_program_id,
    );

    let definition_account_id = token_definition.account_id;
    let holder_ata_id = holder_ata.account_id;

    // The burn happens in the chained call, executed by the token program that owns
    // these accounts; this program reports them unchanged but must still report them.
    let state_diffs = vec![
        AccountStateDiff::unchanged(owner),
        AccountStateDiff::unchanged(holder_ata),
        AccountStateDiff::unchanged(token_definition),
    ];

    // `pda_seeds` is what authorizes the callee over the holder ATA — the call carries
    // account ids, not pre-states, so there is no flag to set.
    let chained_call = ChainedCall::new(
        token_program_id,
        vec![definition_account_id, holder_ata_id],
        &token_core::Instruction::Burn {
            amount_to_burn: amount,
        },
    )
    .with_pda_seeds(vec![seed]);
    (state_diffs, vec![chained_call])
}
