use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, BalanceDiff, Data},
    program::AccountStateDiff,
};
use token_core::{TokenDefinition, TokenHolding};

pub fn initialize_account(
    definition_account: AccountWithMetadata,
    account_to_initialize: AccountWithMetadata,
    token_program_id: AccountId,
) -> Vec<AccountStateDiff> {
    assert_eq!(
        account_to_initialize.account,
        Account::default(),
        "Only Uninitialized accounts can be initialized"
    );
    assert!(
        account_to_initialize.is_authorized,
        "Account to initialize must be authorized"
    );
    assert_eq!(
        definition_account.account.program_owner, token_program_id,
        "Token definition must be owned by token program"
    );

    let definition = TokenDefinition::try_from(&definition_account.account.data)
        .expect("Definition account must be valid");
    let holding =
        TokenHolding::zeroized_from_definition(definition_account.account_id, &definition);

    // Writing the holding data is the claim on the account: v0.2.5 makes the writing
    // program the owner of a default-owned account it writes to. The runtime no longer
    // checks a claim's authorization, so the `is_authorized` assert above — previously
    // redundant with `Claim::Authorized` — is now the only thing stopping a caller from
    // seizing someone else's default account.
    vec![
        AccountStateDiff::unchanged(definition_account),
        AccountStateDiff::new(
            account_to_initialize,
            BalanceDiff::Add(0),
            Data::from(&holding),
        ),
    ]
}
