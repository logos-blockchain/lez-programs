use lez_authority::Ownable;
use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, Claim, ProgramId},
};
use token_core::{TokenDefinition, TokenHolding};

pub fn mint(
    definition_account: AccountWithMetadata,
    user_holding_account: AccountWithMetadata,
    amount_to_mint: u128,
    authority_accounts: Vec<AccountWithMetadata>,
    token_program_id: ProgramId,
) -> Vec<AccountPostState> {
    assert_eq!(
        definition_account.account.program_owner, token_program_id,
        "Token definition must be owned by token program"
    );

    let mut definition = TokenDefinition::try_from(&definition_account.account.data)
        .expect("Token Definition account must be valid");

    // Minting is gated on the definition's stored mint authority. The proof of
    // authority is whichever account is presented as authorized AND whose id
    // matches the stored authority:
    //
    // - When `authority_accounts` is empty, the definition account itself must be the authority
    //   (self/PDA authority — e.g. the AMM's LP definition minting under its own seed). This is the
    //   original mint behavior.
    // - When `authority_accounts` has one entry, that account is the external authority (e.g. a
    //   rotated owner key). This lets a transferred authority actually mint, as RFP-001 requires.
    if let TokenDefinition::Fungible { .. } = &definition {
        let authority = authority_accounts.first().unwrap_or(&definition_account);
        assert!(
            authority.is_authorized,
            "Mint authority must authorize the transaction"
        );
        let signer: [u8; 32] = authority
            .account_id
            .as_ref()
            .try_into()
            .expect("AccountId is always 32 bytes");
        definition
            .require_owner(signer)
            .expect("Mint authority check failed");
    }

    let mut holding = if user_holding_account.account == Account::default() {
        TokenHolding::zeroized_from_definition(definition_account.account_id, &definition)
    } else {
        TokenHolding::try_from(&user_holding_account.account.data)
            .expect("Token Holding account must be valid")
    };

    assert_eq!(
        definition_account.account_id,
        holding.definition_id(),
        "Mismatch Token Definition and Token Holding"
    );

    match (&mut definition, &mut holding) {
        (
            TokenDefinition::Fungible {
                name: _,
                metadata_id: _,
                total_supply,
                authority: _,
            },
            TokenHolding::Fungible {
                definition_id: _,
                balance,
            },
        ) => {
            *balance = balance
                .checked_add(amount_to_mint)
                .expect("Balance overflow on minting");

            *total_supply = total_supply
                .checked_add(amount_to_mint)
                .expect("Total supply overflow");
        }
        (
            TokenDefinition::NonFungible { .. },
            TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. },
        ) => {
            panic!("Cannot mint additional supply for Non-Fungible Tokens");
        }
        _ => panic!("Mismatched Token Definition and Token Holding types"),
    }

    let mut definition_post = definition_account.account;
    definition_post.data = Data::from(&definition);

    let mut holding_post = user_holding_account.account;
    holding_post.data = Data::from(&holding);

    // Post-states must match pre-state order and count. Pre-state order is
    // [definition, holding, ...authority_accounts]; authority accounts are
    // read-only and pass through unchanged.
    let mut post_states = Vec::with_capacity(authority_accounts.len().saturating_add(2));
    post_states.push(AccountPostState::new(definition_post));
    post_states.push(AccountPostState::new_claimed_if_default(
        holding_post,
        Claim::Authorized,
    ));
    for authority in authority_accounts {
        post_states.push(AccountPostState::new(authority.account));
    }
    post_states
}
