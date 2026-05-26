use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, Claim, ProgramId},
};
use token_core::{TokenDefinition, TokenHolding};

pub fn initialize_account(
    definition_account: AccountWithMetadata,
    account_to_initialize: AccountWithMetadata,
    token_program_id: ProgramId,
) -> Vec<AccountPostState> {
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

    let definition_post = definition_account.account;
    let mut account_to_initialize = account_to_initialize.account;
    account_to_initialize.data = Data::from(&holding);

    vec![
        AccountPostState::new(definition_post),
        AccountPostState::new_claimed(account_to_initialize, Claim::Authorized),
    ]
}
