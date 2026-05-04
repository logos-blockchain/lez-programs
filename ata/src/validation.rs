use nssa_core::{account::AccountWithMetadata, program::ProgramId};
use token_core::{TokenDefinition, TokenHolding};

pub fn canonical_token_definition(
    token_definition: &AccountWithMetadata,
    token_program_id: ProgramId,
) -> TokenDefinition {
    assert_eq!(
        token_definition.account.program_owner, token_program_id,
        "Token definition must be owned by token program"
    );
    TokenDefinition::try_from(&token_definition.account.data)
        .expect("Token definition account must be valid")
}

pub fn canonical_ata_holding(
    ata_account: &AccountWithMetadata,
    token_program_id: ProgramId,
) -> TokenHolding {
    assert_eq!(
        ata_account.account.program_owner, token_program_id,
        "ATA account must be owned by token program"
    );
    TokenHolding::try_from(&ata_account.account.data).expect("ATA account must hold a valid token")
}
