#![cfg(test)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test fixtures use fixed values to lock boundary behavior"
)]

use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::{Claim, ProgramId},
};
use token_core::{
    MetadataStandard, NewTokenDefinition, NewTokenMetadata, TokenDefinition, TokenHolding,
};

use crate::{
    burn::burn,
    initialize::initialize_account,
    mint::{mint, mint_with_authority},
    new_definition::{new_definition_with_metadata, new_fungible_definition},
    print_nft::print_nft,
    set_authority::{set_authority, set_authority_with_authority},
    transfer::transfer,
};

// TODO: Move tests to a proper modules like burn, mint, transfer, etc, so that they are more
// unit-test.

struct BalanceForTests;
struct IdForTests;

struct AccountForTests;

const TOKEN_PROGRAM_ID: ProgramId = [5u32; 8];
const FOREIGN_TOKEN_PROGRAM_ID: ProgramId = [6u32; 8];

impl AccountForTests {
    fn definition_account_auth() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("test"),
                    total_supply: BalanceForTests::init_supply(),
                    metadata_id: None,
                    authority: Some(AccountId::new([15_u8; 32])),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn definition_account_foreign_owner() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: FOREIGN_TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("test"),
                    total_supply: BalanceForTests::init_supply(),
                    metadata_id: None,
                    authority: None,
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn definition_account_without_auth() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("test"),
                    total_supply: BalanceForTests::init_supply(),
                    metadata_id: None,
                    authority: None,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn holding_different_definition() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::pool_definition_id_diff(),
                    balance: BalanceForTests::holding_balance(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::holding_id(),
        }
    }

    fn holding_same_definition_with_authorization() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::pool_definition_id(),
                    balance: BalanceForTests::holding_balance(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::holding_id(),
        }
    }

    fn holding_same_definition_without_authorization() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::pool_definition_id(),
                    balance: BalanceForTests::holding_balance(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: IdForTests::holding_id(),
        }
    }

    fn holding_same_definition_without_authorization_overflow() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::pool_definition_id(),
                    balance: BalanceForTests::init_supply(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: IdForTests::holding_id(),
        }
    }

    fn definition_account_post_burn() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("test"),
                    total_supply: BalanceForTests::init_supply_burned(),
                    metadata_id: None,
                    authority: Some(AccountId::new([15_u8; 32])),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn holding_account_post_burn() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::pool_definition_id(),
                    balance: BalanceForTests::holding_balance_burned(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: IdForTests::holding_id(),
        }
    }

    fn holding_account_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: IdForTests::holding_id_2(),
        }
    }

    fn holding_account_uninit_auth() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: IdForTests::holding_id_2(),
        }
    }

    fn init_mint() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [0u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::pool_definition_id(),
                    balance: BalanceForTests::mint_success(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: IdForTests::holding_id(),
        }
    }

    fn holding_account_same_definition_mint() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::pool_definition_id(),
                    balance: BalanceForTests::holding_balance_mint(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn definition_account_mint() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("test"),
                    total_supply: BalanceForTests::init_supply_mint(),
                    metadata_id: None,
                    authority: Some(AccountId::new([15_u8; 32])),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn holding_same_definition_with_authorization_and_large_balance() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::pool_definition_id(),
                    balance: BalanceForTests::mint_overflow(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn definition_account_with_authorization_nonfungible() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenDefinition::NonFungible {
                    name: String::from("test"),
                    printable_supply: BalanceForTests::printable_copies(),
                    metadata_id: AccountId::new([0; 32]),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn definition_account_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn definition_account_uninit_auth() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn metadata_account_uninit_auth() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: AccountId::new([19; 32]),
        }
    }

    fn holding_account_init() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::pool_definition_id(),
                    balance: BalanceForTests::init_supply(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::holding_id(),
        }
    }

    fn definition_account_unclaimed() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [0u32; 8],
                balance: 0u128,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("test"),
                    total_supply: BalanceForTests::init_supply(),
                    metadata_id: None,
                    authority: None,
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn holding_account_unclaimed() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [0u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::pool_definition_id(),
                    balance: BalanceForTests::init_supply(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::holding_id(),
        }
    }

    fn holding_account2_init() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::pool_definition_id(),
                    balance: BalanceForTests::init_supply(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::holding_id_2(),
        }
    }

    fn holding_account2_init_post_transfer() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::pool_definition_id(),
                    balance: BalanceForTests::recipient_post_transfer(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::holding_id_2(),
        }
    }

    fn holding_account_init_post_transfer() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::pool_definition_id(),
                    balance: BalanceForTests::sender_post_transfer(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::holding_id(),
        }
    }

    fn holding_account_master_nft() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::NftMaster {
                    definition_id: IdForTests::pool_definition_id(),
                    print_balance: BalanceForTests::printable_copies(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::holding_id(),
        }
    }

    fn holding_account_master_nft_insufficient_balance() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::NftMaster {
                    definition_id: IdForTests::pool_definition_id(),
                    print_balance: 1,
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::holding_id(),
        }
    }

    fn holding_account_master_nft_after_print() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::NftMaster {
                    definition_id: IdForTests::pool_definition_id(),
                    print_balance: BalanceForTests::printable_copies() - 1,
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::holding_id(),
        }
    }

    fn holding_account_printed_nft() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [0u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::NftPrintedCopy {
                    definition_id: IdForTests::pool_definition_id(),
                    owned: true,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: IdForTests::holding_id(),
        }
    }

    fn holding_account_with_master_nft_transferred_to() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [0u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::NftMaster {
                    definition_id: IdForTests::pool_definition_id(),
                    print_balance: BalanceForTests::printable_copies(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::holding_id_2(),
        }
    }

    fn holding_account_master_nft_post_transfer() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5u32; 8],
                balance: 0u128,
                data: Data::from(&TokenHolding::NftMaster {
                    definition_id: IdForTests::pool_definition_id(),
                    print_balance: 0,
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::holding_id(),
        }
    }
}

impl BalanceForTests {
    fn init_supply() -> u128 {
        100_000
    }

    fn holding_balance() -> u128 {
        1_000
    }

    fn init_supply_burned() -> u128 {
        99_500
    }

    fn holding_balance_burned() -> u128 {
        500
    }

    fn burn_success() -> u128 {
        500
    }

    fn burn_insufficient() -> u128 {
        1_500
    }

    fn mint_success() -> u128 {
        50_000
    }

    fn holding_balance_mint() -> u128 {
        51_000
    }

    fn mint_overflow() -> u128 {
        u128::MAX - 40_000
    }

    fn init_supply_mint() -> u128 {
        150_000
    }

    fn sender_post_transfer() -> u128 {
        95_000
    }

    fn recipient_post_transfer() -> u128 {
        105_000
    }

    fn transfer_amount() -> u128 {
        5_000
    }

    fn printable_copies() -> u128 {
        10
    }
}

impl IdForTests {
    fn pool_definition_id() -> AccountId {
        AccountId::new([15; 32])
    }

    fn pool_definition_id_diff() -> AccountId {
        AccountId::new([16; 32])
    }

    fn holding_id() -> AccountId {
        AccountId::new([17; 32])
    }

    fn holding_id_2() -> AccountId {
        AccountId::new([42; 32])
    }
}

#[should_panic(expected = "Definition target account must have default values")]
#[test]
fn test_new_definition_non_default_first_account_should_fail() {
    let definition_account = AccountWithMetadata {
        account: Account {
            program_owner: [1, 2, 3, 4, 5, 6, 7, 8],
            ..Account::default()
        },
        is_authorized: true,
        account_id: AccountId::new([1; 32]),
    };
    let holding_account = AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: AccountId::new([2; 32]),
    };
    let _post_states = new_fungible_definition(
        definition_account,
        holding_account,
        String::from("test"),
        10,
        None,
    );
}

#[should_panic(expected = "Holding target account must have default values")]
#[test]
fn test_new_definition_non_default_second_account_should_fail() {
    let definition_account = AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: AccountId::new([1; 32]),
    };
    let holding_account = AccountWithMetadata {
        account: Account {
            program_owner: [1, 2, 3, 4, 5, 6, 7, 8],
            ..Account::default()
        },
        is_authorized: true,
        account_id: AccountId::new([2; 32]),
    };
    let _post_states = new_fungible_definition(
        definition_account,
        holding_account,
        String::from("test"),
        10,
        None,
    );
}

#[should_panic(expected = "Definition target account must be authorized")]
#[test]
fn test_new_definition_requires_authorized_definition_target() {
    let definition_account = AccountForTests::definition_account_uninit();
    let holding_account = AccountForTests::holding_account_uninit_auth();
    let _post_states = new_fungible_definition(
        definition_account,
        holding_account,
        String::from("test"),
        10,
        None,
    );
}

#[should_panic(expected = "Holding target account must be authorized")]
#[test]
fn test_new_definition_requires_authorized_holding_target() {
    let definition_account = AccountForTests::definition_account_uninit_auth();
    let holding_account = AccountForTests::holding_account_uninit();
    let _post_states = new_fungible_definition(
        definition_account,
        holding_account,
        String::from("test"),
        10,
        None,
    );
}

#[test]
fn test_new_definition_with_valid_inputs_succeeds() {
    let definition_account = AccountForTests::definition_account_uninit_auth();
    let holding_account = AccountForTests::holding_account_uninit_auth();

    let post_states = new_fungible_definition(
        definition_account,
        holding_account,
        String::from("test"),
        BalanceForTests::init_supply(),
        None,
    );

    let [definition_account, holding_account] = post_states.try_into().unwrap();
    assert_eq!(
        *definition_account.account(),
        AccountForTests::definition_account_unclaimed().account
    );
    assert_eq!(definition_account.required_claim(), Some(Claim::Authorized));

    assert_eq!(
        *holding_account.account(),
        AccountForTests::holding_account_unclaimed().account
    );
    assert_eq!(holding_account.required_claim(), Some(Claim::Authorized));
}

#[should_panic(expected = "Sender and recipient definition id mismatch")]
#[test]
fn test_transfer_with_different_definition_ids_should_fail() {
    let sender = AccountForTests::holding_same_definition_with_authorization();
    let recipient = AccountForTests::holding_different_definition();
    let _post_states = transfer(sender, recipient, 10);
}

#[should_panic(expected = "Insufficient balance")]
#[test]
fn test_transfer_with_insufficient_balance_should_fail() {
    let sender = AccountForTests::holding_same_definition_with_authorization();
    let recipient = AccountForTests::holding_account_same_definition_mint();
    // Attempt to transfer more than balance
    let _post_states = transfer(sender, recipient, BalanceForTests::burn_insufficient());
}

#[should_panic(expected = "Sender authorization is missing")]
#[test]
fn test_transfer_without_sender_authorization_should_fail() {
    let sender = AccountForTests::holding_same_definition_without_authorization();
    let recipient = AccountForTests::holding_account_uninit();
    let _post_states = transfer(sender, recipient, 37);
}

#[test]
fn test_transfer_with_valid_inputs_succeeds() {
    let sender = AccountForTests::holding_account_init();
    let recipient = AccountForTests::holding_account2_init();
    let post_states = transfer(sender, recipient, BalanceForTests::transfer_amount());
    let [sender_post, recipient_post] = post_states.try_into().unwrap();

    assert_eq!(
        *sender_post.account(),
        AccountForTests::holding_account_init_post_transfer().account
    );
    assert_eq!(
        *recipient_post.account(),
        AccountForTests::holding_account2_init_post_transfer().account
    );
    assert_eq!(sender_post.required_claim(), None);
    assert_eq!(recipient_post.required_claim(), None);
}

#[should_panic(expected = "Invalid balance for NFT Master transfer")]
#[test]
fn test_transfer_with_master_nft_invalid_balance() {
    let sender = AccountForTests::holding_account_master_nft();
    let recipient = AccountForTests::holding_account_uninit();
    let _post_states = transfer(sender, recipient, BalanceForTests::transfer_amount());
}

#[should_panic(expected = "Invalid balance in recipient account for NFT transfer")]
#[test]
fn test_transfer_with_master_nft_invalid_recipient_balance() {
    let sender = AccountForTests::holding_account_master_nft();
    let recipient = AccountForTests::holding_account_with_master_nft_transferred_to();
    let _post_states = transfer(sender, recipient, BalanceForTests::printable_copies());
}

#[test]
fn test_transfer_with_master_nft_success() {
    let sender = AccountForTests::holding_account_master_nft();
    let recipient = AccountForTests::holding_account_uninit();
    let post_states = transfer(sender, recipient, BalanceForTests::printable_copies());
    let [sender_post, recipient_post] = post_states.try_into().unwrap();

    assert_eq!(
        *sender_post.account(),
        AccountForTests::holding_account_master_nft_post_transfer().account
    );
    assert_eq!(
        *recipient_post.account(),
        AccountForTests::holding_account_with_master_nft_transferred_to().account
    );
}

#[test]
fn test_transfer_with_default_recipient_claims_recipient() {
    let sender = AccountForTests::holding_account_init();
    let recipient = AccountForTests::holding_account_uninit();
    let post_states = transfer(sender, recipient, BalanceForTests::transfer_amount());
    let [sender_post, recipient_post] = post_states.try_into().unwrap();

    assert_eq!(
        *sender_post.account(),
        AccountForTests::holding_account_init_post_transfer().account
    );
    assert_eq!(
        *recipient_post.account(),
        Account {
            program_owner: [0u32; 8],
            balance: 0u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: IdForTests::pool_definition_id(),
                balance: BalanceForTests::transfer_amount(),
            }),
            nonce: Nonce(0),
        }
    );
    assert_eq!(sender_post.required_claim(), None);
    assert_eq!(recipient_post.required_claim(), Some(Claim::Authorized));
}

#[test]
fn test_token_initialize_account_succeeds() {
    let definition_account = AccountForTests::definition_account_auth();
    let holding_account = AccountForTests::holding_account_uninit_auth();
    let post_states = initialize_account(definition_account, holding_account, TOKEN_PROGRAM_ID);
    let [definition_post, holding_post] = post_states.try_into().unwrap();

    assert_eq!(
        *definition_post.account(),
        AccountForTests::definition_account_auth().account
    );
    assert_eq!(
        *holding_post.account(),
        Account {
            program_owner: [0u32; 8],
            balance: 0u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: IdForTests::pool_definition_id(),
                balance: 0,
            }),
            nonce: Nonce(0),
        }
    );
    assert_eq!(definition_post.required_claim(), None);
    assert_eq!(holding_post.required_claim(), Some(Claim::Authorized));
}

#[test]
#[should_panic(expected = "Account to initialize must be authorized")]
fn test_token_initialize_account_requires_authorization() {
    let definition_account = AccountForTests::definition_account_auth();
    let holding_account = AccountForTests::holding_account_uninit();
    let _post_states = initialize_account(definition_account, holding_account, TOKEN_PROGRAM_ID);
}

#[test]
#[should_panic(expected = "Token definition must be owned by token program")]
fn test_token_initialize_account_rejects_foreign_owned_definition() {
    let definition_account = AccountForTests::definition_account_foreign_owner();
    let holding_account = AccountForTests::holding_account_uninit_auth();
    let _post_states = initialize_account(definition_account, holding_account, TOKEN_PROGRAM_ID);
}

#[test]
#[should_panic(expected = "Mismatch Token Definition and Token Holding")]
fn test_burn_mismatch_def() {
    let definition_account = AccountForTests::definition_account_auth();
    let holding_account = AccountForTests::holding_different_definition();
    let _post_states = burn(
        definition_account,
        holding_account,
        BalanceForTests::burn_success(),
    );
}

#[test]
#[should_panic(expected = "Authorization is missing")]
fn test_burn_missing_authorization() {
    let definition_account = AccountForTests::definition_account_auth();
    let holding_account = AccountForTests::holding_same_definition_without_authorization();
    let _post_states = burn(
        definition_account,
        holding_account,
        BalanceForTests::burn_success(),
    );
}

#[test]
#[should_panic(expected = "Insufficient balance to burn")]
fn test_burn_insufficient_balance() {
    let definition_account = AccountForTests::definition_account_auth();
    let holding_account = AccountForTests::holding_same_definition_with_authorization();
    let _post_states = burn(
        definition_account,
        holding_account,
        BalanceForTests::burn_insufficient(),
    );
}

#[test]
#[should_panic(expected = "Total supply underflow")]
fn test_burn_total_supply_underflow() {
    let definition_account = AccountForTests::definition_account_auth();
    let holding_account =
        AccountForTests::holding_same_definition_with_authorization_and_large_balance();
    let _post_states = burn(
        definition_account,
        holding_account,
        BalanceForTests::mint_overflow(),
    );
}

#[test]
fn test_burn_success() {
    let definition_account = AccountForTests::definition_account_auth();
    let holding_account = AccountForTests::holding_same_definition_with_authorization();
    let post_states = burn(
        definition_account,
        holding_account,
        BalanceForTests::burn_success(),
    );

    let [def_post, holding_post] = post_states.try_into().unwrap();

    assert_eq!(
        *def_post.account(),
        AccountForTests::definition_account_post_burn().account
    );
    assert_eq!(
        *holding_post.account(),
        AccountForTests::holding_account_post_burn().account
    );
}

#[test]
#[should_panic(expected = "Holding account must be valid")]
fn test_mint_not_valid_holding_account() {
    let definition_account = AccountForTests::definition_account_auth();
    let holding_account = AccountForTests::definition_account_without_auth();
    let _post_states = mint(
        definition_account,
        holding_account,
        BalanceForTests::mint_success(),
        TOKEN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Definition account must be valid")]
fn test_mint_not_valid_definition_account() {
    let definition_account = AccountForTests::holding_same_definition_with_authorization();
    let holding_account = AccountForTests::holding_same_definition_without_authorization();
    let _post_states = mint(
        definition_account,
        holding_account,
        BalanceForTests::mint_success(),
        TOKEN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Mint authority must authorize the transaction")]
fn test_mint_missing_authorization() {
    // The definition account itself is the authority; mark it unauthorized.
    let mut definition_account = AccountForTests::definition_account_auth();
    definition_account.is_authorized = false;
    let holding_account = AccountForTests::holding_same_definition_without_authorization();
    let _post_states = mint(
        definition_account,
        holding_account,
        BalanceForTests::mint_success(),
        TOKEN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Token definition must be owned by token program")]
fn test_mint_rejects_foreign_owned_definition() {
    let definition_account = AccountForTests::definition_account_foreign_owner();
    let holding_account = AccountForTests::holding_account_uninit();
    let _post_states = mint(
        definition_account,
        holding_account,
        BalanceForTests::mint_success(),
        TOKEN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Token definition must be owned by token program")]
fn test_set_authority_rejects_foreign_owned_definition() {
    // A foreign-owned account carrying token-shaped data must not be able to
    // rotate or revoke its authority through the token program.
    let definition_account = AccountForTests::definition_account_foreign_owner();
    let _post_states = set_authority(
        definition_account,
        Some(AccountId::new([7_u8; 32])),
        TOKEN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Mismatch Token Definition and Token Holding")]
fn test_mint_mismatched_token_definition() {
    //
    let definition_account = AccountForTests::definition_account_auth();
    let holding_account = AccountForTests::holding_different_definition();
    let _post_states = mint(
        definition_account,
        holding_account,
        BalanceForTests::mint_success(),
        TOKEN_PROGRAM_ID,
    );
}

#[test]
fn test_mint_success() {
    let definition_account = AccountForTests::definition_account_auth();
    let holding_account = AccountForTests::holding_same_definition_without_authorization();
    let post_states = mint(
        definition_account,
        holding_account,
        BalanceForTests::mint_success(),
        TOKEN_PROGRAM_ID,
    );

    let [def_post, holding_post] = post_states.try_into().unwrap();

    assert_eq!(
        *def_post.account(),
        AccountForTests::definition_account_mint().account
    );
    assert_eq!(
        *holding_post.account(),
        AccountForTests::holding_account_same_definition_mint().account
    );
    assert_eq!(def_post.required_claim(), None);
    assert_eq!(holding_post.required_claim(), None);
}

#[test]
fn test_mint_uninit_holding_success() {
    let definition_account = AccountForTests::definition_account_auth();
    let holding_account = AccountForTests::holding_account_uninit();
    let post_states = mint(
        definition_account,
        holding_account,
        BalanceForTests::mint_success(),
        TOKEN_PROGRAM_ID,
    );

    let [def_post, holding_post] = post_states.try_into().unwrap();

    assert_eq!(
        *def_post.account(),
        AccountForTests::definition_account_mint().account
    );
    assert_eq!(
        *holding_post.account(),
        AccountForTests::init_mint().account
    );
    assert_eq!(def_post.required_claim(), None);
    assert_eq!(holding_post.required_claim(), Some(Claim::Authorized));
}

#[test]
#[should_panic(expected = "Total supply overflow")]
fn test_mint_total_supply_overflow() {
    let definition_account = AccountForTests::definition_account_auth();
    let holding_account = AccountForTests::holding_same_definition_without_authorization();
    let _post_states = mint(
        definition_account,
        holding_account,
        BalanceForTests::mint_overflow(),
        TOKEN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Balance overflow on minting")]
fn test_mint_holding_account_overflow() {
    let definition_account = AccountForTests::definition_account_auth();
    let holding_account = AccountForTests::holding_same_definition_without_authorization_overflow();
    let _post_states = mint(
        definition_account,
        holding_account,
        BalanceForTests::mint_overflow(),
        TOKEN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Cannot mint additional supply for Non-Fungible Tokens")]
fn test_mint_cannot_mint_unmintable_tokens() {
    let definition_account = AccountForTests::definition_account_with_authorization_nonfungible();
    let holding_account = AccountForTests::holding_account_master_nft();
    let _post_states = mint(
        definition_account,
        holding_account,
        BalanceForTests::mint_success(),
        TOKEN_PROGRAM_ID,
    );
}

#[test]
fn test_new_definition_with_metadata_success() {
    let definition_account = AccountForTests::definition_account_uninit_auth();
    let holding_account = AccountForTests::holding_account_uninit_auth();
    let metadata_account = AccountForTests::metadata_account_uninit_auth();
    let new_definition = NewTokenDefinition::Fungible {
        name: String::from("test"),
        total_supply: 15u128,
        mint_authority: None,
    };
    let metadata = NewTokenMetadata {
        standard: MetadataStandard::Simple,
        uri: "test_uri".to_string(),
        creators: "test_creators".to_string(),
    };

    let post_states = new_definition_with_metadata(
        definition_account,
        holding_account,
        metadata_account,
        new_definition,
        metadata,
    );
    let [definition_post, holding_post, metadata_post] = post_states.try_into().unwrap();

    assert_eq!(definition_post.required_claim(), Some(Claim::Authorized));
    assert_eq!(holding_post.required_claim(), Some(Claim::Authorized));
    assert_eq!(metadata_post.required_claim(), Some(Claim::Authorized));
}

/// Comment #2: a metadata-backed fungible created with `mint_authority: Some(..)`
/// carries a real, non-renounced authority and is therefore mintable — no longer
/// force-fixed-supply the way the hardcoded `Authority::renounced()` made it.
#[test]
fn test_metadata_fungible_with_authority_is_mintable() {
    let definition_account = AccountForTests::definition_account_uninit_auth();
    let holding_account = AccountForTests::holding_account_uninit_auth();
    let metadata_account = AccountForTests::metadata_account_uninit_auth();
    let new_definition = NewTokenDefinition::Fungible {
        name: String::from("test"),
        total_supply: 15u128,
        mint_authority: Some(AccountId::new([15_u8; 32])),
    };
    let metadata = NewTokenMetadata {
        standard: MetadataStandard::Simple,
        uri: "test_uri".to_string(),
        creators: "test_creators".to_string(),
    };
    let post_states = new_definition_with_metadata(
        definition_account,
        holding_account,
        metadata_account,
        new_definition,
        metadata,
    );
    let [definition_post, _holding_post, _metadata_post] = post_states.try_into().unwrap();

    // The stored authority must be the requested key, NOT renounced.
    let def = TokenDefinition::try_from(&definition_post.account().data).unwrap();
    let stored = match def {
        TokenDefinition::Fungible { authority, .. } => authority,
        _ => None,
    };
    assert_eq!(stored, Some(AccountId::new([15_u8; 32])));
}

#[should_panic(expected = "Definition target account must be authorized")]
#[test]
fn test_call_new_definition_metadata_requires_authorized_definition() {
    let definition_account = AccountForTests::definition_account_uninit();
    let holding_account = AccountForTests::holding_account_uninit_auth();
    let metadata_account = AccountForTests::metadata_account_uninit_auth();
    let new_definition = NewTokenDefinition::Fungible {
        name: String::from("test"),
        total_supply: 15u128,
        mint_authority: None,
    };
    let metadata = NewTokenMetadata {
        standard: MetadataStandard::Simple,
        uri: "test_uri".to_string(),
        creators: "test_creators".to_string(),
    };
    let _post_states = new_definition_with_metadata(
        definition_account,
        holding_account,
        metadata_account,
        new_definition,
        metadata,
    );
}

#[should_panic(expected = "Holding target account must be authorized")]
#[test]
fn test_call_new_definition_metadata_requires_authorized_holding() {
    let definition_account = AccountForTests::definition_account_uninit_auth();
    let holding_account = AccountForTests::holding_account_uninit();
    let metadata_account = AccountForTests::metadata_account_uninit_auth();
    let new_definition = NewTokenDefinition::Fungible {
        name: String::from("test"),
        total_supply: 15u128,
        mint_authority: None,
    };
    let metadata = NewTokenMetadata {
        standard: MetadataStandard::Simple,
        uri: "test_uri".to_string(),
        creators: "test_creators".to_string(),
    };
    let _post_states = new_definition_with_metadata(
        definition_account,
        holding_account,
        metadata_account,
        new_definition,
        metadata,
    );
}

#[should_panic(expected = "Metadata target account must be authorized")]
#[test]
fn test_call_new_definition_metadata_requires_authorized_metadata() {
    let definition_account = AccountForTests::definition_account_uninit_auth();
    let holding_account = AccountForTests::holding_account_uninit_auth();
    let metadata_account = AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: AccountId::new([20; 32]),
    };
    let new_definition = NewTokenDefinition::Fungible {
        name: String::from("test"),
        total_supply: 15u128,
        mint_authority: None,
    };
    let metadata = NewTokenMetadata {
        standard: MetadataStandard::Simple,
        uri: "test_uri".to_string(),
        creators: "test_creators".to_string(),
    };
    let _post_states = new_definition_with_metadata(
        definition_account,
        holding_account,
        metadata_account,
        new_definition,
        metadata,
    );
}

#[should_panic(expected = "Definition target account must have default values")]
#[test]
fn test_call_new_definition_metadata_with_init_definition() {
    let definition_account = AccountForTests::definition_account_auth();
    let metadata_account = AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: AccountId::new([2; 32]),
    };
    let holding_account = AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: AccountId::new([3; 32]),
    };
    let new_definition = NewTokenDefinition::Fungible {
        name: String::from("test"),
        total_supply: 15u128,
        mint_authority: None,
    };
    let metadata = NewTokenMetadata {
        standard: MetadataStandard::Simple,
        uri: "test_uri".to_string(),
        creators: "test_creators".to_string(),
    };
    let _post_states = new_definition_with_metadata(
        definition_account,
        metadata_account,
        holding_account,
        new_definition,
        metadata,
    );
}

#[should_panic(expected = "Metadata target account must have default values")]
#[test]
fn test_call_new_definition_metadata_with_init_metadata() {
    let definition_account = AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: AccountId::new([1; 32]),
    };
    let holding_account = AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: AccountId::new([3; 32]),
    };
    let metadata_account = AccountForTests::holding_account_same_definition_mint();
    let new_definition = NewTokenDefinition::Fungible {
        name: String::from("test"),
        total_supply: 15u128,
        mint_authority: None,
    };
    let metadata = NewTokenMetadata {
        standard: MetadataStandard::Simple,
        uri: "test_uri".to_string(),
        creators: "test_creators".to_string(),
    };
    let _post_states = new_definition_with_metadata(
        definition_account,
        holding_account,
        metadata_account,
        new_definition,
        metadata,
    );
}

#[should_panic(expected = "Holding target account must have default values")]
#[test]
fn test_call_new_definition_metadata_with_init_holding() {
    let definition_account = AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: AccountId::new([1; 32]),
    };
    let metadata_account = AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: AccountId::new([2; 32]),
    };
    let holding_account = AccountForTests::holding_account_same_definition_mint();
    let new_definition = NewTokenDefinition::Fungible {
        name: String::from("test"),
        total_supply: 15u128,
        mint_authority: None,
    };
    let metadata = NewTokenMetadata {
        standard: MetadataStandard::Simple,
        uri: "test_uri".to_string(),
        creators: "test_creators".to_string(),
    };
    let _post_states = new_definition_with_metadata(
        definition_account,
        holding_account,
        metadata_account,
        new_definition,
        metadata,
    );
}

#[should_panic(expected = "Master NFT Account must be authorized")]
#[test]
fn test_print_nft_master_account_must_be_authorized() {
    let master_account = AccountForTests::holding_account_uninit();
    let printed_account = AccountForTests::holding_account_uninit();
    let _post_states = print_nft(master_account, printed_account);
}

#[should_panic(expected = "Printed Account must be uninitialized")]
#[test]
fn test_print_nft_print_account_initialized() {
    let master_account = AccountForTests::holding_account_master_nft();
    let printed_account = AccountForTests::holding_account_init();
    let _post_states = print_nft(master_account, printed_account);
}

#[should_panic(expected = "Printed Account must be authorized")]
#[test]
fn test_print_nft_print_account_must_be_authorized() {
    let master_account = AccountForTests::holding_account_master_nft();
    let printed_account = AccountForTests::holding_account_uninit();
    let _post_states = print_nft(master_account, printed_account);
}

#[should_panic(expected = "Invalid Token Holding data")]
#[test]
fn test_print_nft_master_nft_invalid_token_holding() {
    let master_account = AccountForTests::definition_account_auth();
    let printed_account = AccountForTests::holding_account_uninit_auth();
    let _post_states = print_nft(master_account, printed_account);
}

#[should_panic(expected = "Invalid Token Holding provided as NFT Master Account")]
#[test]
fn test_print_nft_master_nft_not_nft_master_account() {
    let master_account = AccountForTests::holding_account_init();
    let printed_account = AccountForTests::holding_account_uninit_auth();
    let _post_states = print_nft(master_account, printed_account);
}

#[should_panic(expected = "Insufficient balance to print another NFT copy")]
#[test]
fn test_print_nft_master_nft_insufficient_balance() {
    let master_account = AccountForTests::holding_account_master_nft_insufficient_balance();
    let printed_account = AccountForTests::holding_account_uninit_auth();
    let _post_states = print_nft(master_account, printed_account);
}

#[test]
fn test_print_nft_success() {
    let master_account = AccountForTests::holding_account_master_nft();
    let printed_account = AccountForTests::holding_account_uninit_auth();
    let post_states = print_nft(master_account, printed_account);

    let [post_master_nft, post_printed] = post_states.try_into().unwrap();

    assert_eq!(
        *post_master_nft.account(),
        AccountForTests::holding_account_master_nft_after_print().account
    );
    assert_eq!(
        *post_printed.account(),
        AccountForTests::holding_account_printed_nft().account
    );
    assert_eq!(post_master_nft.required_claim(), None);
    assert_eq!(post_printed.required_claim(), Some(Claim::Authorized));
}

#[cfg(test)]
mod authority_tests {
    use super::*;
    use crate::{mint::mint, set_authority::set_authority};

    const AUTHORITY: [u8; 32] = [15_u8; 32];
    const TOKEN_PROGRAM_ID: [u32; 8] = [5_u32; 8];

    /// A fungible definition whose own account id ([15;32]) equals its stored
    /// mint authority, authorized in the transaction. This models both an external
    /// owner signing the definition key and a PDA authorized via its seeds.
    fn def_with_authority() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5_u32; 8],
                balance: 0_u128,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("test"),
                    total_supply: 100_000_u128,
                    metadata_id: None,
                    authority: Some(AccountId::new(AUTHORITY)),
                }),
                nonce: 0_u128.into(),
            },
            is_authorized: true,
            account_id: AccountId::new([15; 32]),
        }
    }

    /// A definition whose authority has been renounced (fixed supply).
    fn def_with_authority_revoked() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5_u32; 8],
                balance: 0_u128,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("test"),
                    total_supply: 100_000_u128,
                    metadata_id: None,
                    authority: None,
                }),
                nonce: 0_u128.into(),
            },
            is_authorized: true,
            account_id: AccountId::new([15; 32]),
        }
    }

    /// A definition whose account id ([99;32]) does NOT match its stored
    /// authority ([15;32]) — models a caller that isn't the current authority.
    fn def_wrong_authority() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5_u32; 8],
                balance: 0_u128,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("test"),
                    total_supply: 100_000_u128,
                    metadata_id: None,
                    authority: Some(AccountId::new(AUTHORITY)),
                }),
                nonce: 0_u128.into(),
            },
            is_authorized: true,
            account_id: AccountId::new([99; 32]),
        }
    }

    fn holding_account() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: [5_u32; 8],
                balance: 0_u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: AccountId::new([15; 32]),
                    balance: 1_000_u128,
                }),
                nonce: 0_u128.into(),
            },
            is_authorized: false,
            account_id: AccountId::new([17; 32]),
        }
    }

    #[test]
    fn mint_with_authority_succeeds() {
        let post_states = mint(
            def_with_authority(),
            holding_account(),
            50_000,
            TOKEN_PROGRAM_ID,
        );
        let [def_post, holding_post] = post_states.try_into().unwrap();

        let def = TokenDefinition::try_from(&def_post.account().data).unwrap();
        let holding = TokenHolding::try_from(&holding_post.account().data).unwrap();

        assert!(matches!(
            def,
            TokenDefinition::Fungible {
                total_supply: 150_000,
                ..
            }
        ));
        assert!(matches!(
            holding,
            TokenHolding::Fungible {
                balance: 51_000,
                ..
            }
        ));
    }

    #[test]
    #[should_panic(expected = "Mint authority check failed")]
    fn mint_with_revoked_authority_fails() {
        let _ = mint(
            def_with_authority_revoked(),
            holding_account(),
            50_000,
            TOKEN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "Mint authority must authorize the transaction")]
    fn mint_without_is_authorized_fails() {
        let mut def = def_with_authority();
        def.is_authorized = false;
        let _ = mint(def, holding_account(), 50_000, TOKEN_PROGRAM_ID);
    }

    #[test]
    #[should_panic(expected = "Mint authority check failed")]
    fn mint_with_wrong_signer_fails() {
        let _ = mint(
            def_wrong_authority(),
            holding_account(),
            50_000,
            TOKEN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "New mint authority must be a valid non-zero account ID")]
    fn set_authority_rejects_zero_new_authority() {
        let _ = set_authority(
            def_with_authority(),
            Some(AccountId::new([0u8; 32])),
            TOKEN_PROGRAM_ID,
        );
    }

    #[test]
    fn set_authority_rotates_to_new_key() {
        let new_key = AccountId::new([7_u8; 32]);
        let post_states = set_authority(def_with_authority(), Some(new_key), TOKEN_PROGRAM_ID);
        let [def_post] = post_states.try_into().unwrap();

        let def = TokenDefinition::try_from(&def_post.account().data).unwrap();
        let auth = match def {
            TokenDefinition::Fungible { authority, .. } => authority,
            _ => None,
        };
        assert_eq!(auth, Some(AccountId::new([7_u8; 32])));
    }

    #[test]
    fn set_authority_revokes_permanently() {
        let post_states = set_authority(def_with_authority(), None, TOKEN_PROGRAM_ID);
        let [def_post] = post_states.try_into().unwrap();

        let def = TokenDefinition::try_from(&def_post.account().data).unwrap();
        let renounced = match def {
            TokenDefinition::Fungible { authority, .. } => authority.is_none(),
            _ => false,
        };
        assert!(renounced);
    }

    #[test]
    #[should_panic(expected = "SetAuthority failed")]
    fn set_authority_on_revoked_fails() {
        let _ = set_authority(
            def_with_authority_revoked(),
            Some(AccountId::new([7_u8; 32])),
            TOKEN_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "Mint authority must authorize the transaction")]
    fn set_authority_without_is_authorized_fails() {
        let mut def = def_with_authority();
        def.is_authorized = false;
        let _ = set_authority(def, Some(AccountId::new([7_u8; 32])), TOKEN_PROGRAM_ID);
    }

    #[test]
    #[should_panic(expected = "SetAuthority failed")]
    fn set_authority_wrong_signer_fails() {
        let _ = set_authority(
            def_wrong_authority(),
            Some(AccountId::new([7_u8; 32])),
            TOKEN_PROGRAM_ID,
        );
    }

    /// After rotating A ([15;32]) -> B ([7;32]) via self-authority, B can rotate
    /// again to C ([9;32]) by presenting itself as the external authority.
    #[test]
    fn set_authority_with_authority_rotates_again() {
        let rotate_post = set_authority(
            def_with_authority(),
            Some(AccountId::new([7_u8; 32])),
            TOKEN_PROGRAM_ID,
        );
        let [def_post] = rotate_post.try_into().unwrap();

        let mut rotated_def = def_with_authority();
        rotated_def.account = def_post.account().clone();

        // B ([7;32]) rotates to C ([9;32]) as the external authority.
        let post_states = set_authority_with_authority(
            rotated_def,
            new_authority_signer(),
            Some(AccountId::new([9_u8; 32])),
            TOKEN_PROGRAM_ID,
        );
        let [def_after, _auth] = post_states.try_into().unwrap();
        let auth = match TokenDefinition::try_from(&def_after.account().data).unwrap() {
            TokenDefinition::Fungible { authority, .. } => authority,
            _ => None,
        };
        assert_eq!(auth, Some(AccountId::new([9_u8; 32])));
    }

    /// A rotated external authority B ([7;32]) can permanently revoke.
    #[test]
    fn set_authority_with_authority_revokes() {
        let rotate_post = set_authority(
            def_with_authority(),
            Some(AccountId::new([7_u8; 32])),
            TOKEN_PROGRAM_ID,
        );
        let [def_post] = rotate_post.try_into().unwrap();

        let mut rotated_def = def_with_authority();
        rotated_def.account = def_post.account().clone();

        let post_states = set_authority_with_authority(
            rotated_def,
            new_authority_signer(),
            None,
            TOKEN_PROGRAM_ID,
        );
        let [def_after, _auth] = post_states.try_into().unwrap();
        let renounced = match TokenDefinition::try_from(&def_after.account().data).unwrap() {
            TokenDefinition::Fungible { authority, .. } => authority.is_none(),
            _ => false,
        };
        assert!(renounced);
    }

    /// An external account that is not the current authority cannot rotate/revoke.
    #[test]
    #[should_panic(expected = "SetAuthority failed: signer is not the current authority")]
    fn set_authority_with_authority_wrong_signer_fails() {
        // Stored authority is A ([15;32]); present a different authorized account.
        let wrong_authority = AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: AccountId::new([8_u8; 32]),
        };
        let _ = set_authority_with_authority(
            def_with_authority(),
            wrong_authority,
            Some(AccountId::new([9_u8; 32])),
            TOKEN_PROGRAM_ID,
        );
    }

    #[test]
    fn set_authority_rotate_then_old_cannot_mint() {
        let new_key = AccountId::new([7_u8; 32]);
        let post_states = set_authority(def_with_authority(), Some(new_key), TOKEN_PROGRAM_ID);
        let [def_post] = post_states.try_into().unwrap();

        let def = TokenDefinition::try_from(&def_post.account().data).unwrap();
        let auth = match def {
            TokenDefinition::Fungible { authority, .. } => authority,
            _ => None,
        };
        // Rotated to the new key; the old authority no longer controls it.
        assert_eq!(auth, Some(AccountId::new([7_u8; 32])));
        assert_ne!(auth, Some(AccountId::new(AUTHORITY)));
    }

    /// Authority signer for the rotated key B ([7;32]), authorized.
    fn new_authority_signer() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: AccountId::new([7_u8; 32]),
        }
    }

    /// RFP-001 end-to-end (comment #1): after rotating authority A -> B, the new
    /// authority B can actually mint by presenting itself in `authority_accounts`.
    #[test]
    fn rotated_authority_can_mint() {
        // Rotate A ([15;32]) -> B ([7;32]), signed by A via self-authority.
        let rotate_post = set_authority(
            def_with_authority(),
            Some(AccountId::new([7_u8; 32])),
            TOKEN_PROGRAM_ID,
        );
        let [def_post] = rotate_post.try_into().unwrap();

        // Rebuild the definition carrying the rotated authority, re-authorized.
        let mut rotated_def = def_with_authority();
        rotated_def.account = def_post.account().clone();

        // B mints by presenting itself as the external authority.
        let mint_post = mint_with_authority(
            rotated_def,
            holding_account(),
            new_authority_signer(),
            10_000,
            TOKEN_PROGRAM_ID,
        );
        let [def_after, holding_after, _auth] = mint_post.try_into().unwrap();
        let minted = TokenDefinition::try_from(&def_after.account().data).unwrap();
        assert!(matches!(
            minted,
            TokenDefinition::Fungible {
                total_supply: 110_000,
                ..
            }
        ));
        let holding = TokenHolding::try_from(&holding_after.account().data).unwrap();
        assert!(matches!(
            holding,
            TokenHolding::Fungible {
                balance: 11_000,
                ..
            }
        ));
    }

    /// Comment #1 negative: after rotation to B, the OLD authority A can no
    /// longer mint. Here A attempts self-authority (empty `authority_accounts`),
    /// but the definition's own id no longer matches the stored authority B.
    #[test]
    #[should_panic(expected = "Mint authority check failed")]
    fn rotated_authority_old_key_cannot_mint() {
        let rotate_post = set_authority(
            def_with_authority(),
            Some(AccountId::new([7_u8; 32])),
            TOKEN_PROGRAM_ID,
        );
        let [def_post] = rotate_post.try_into().unwrap();

        let mut rotated_def = def_with_authority();
        rotated_def.account = def_post.account().clone();

        // A ([15;32]) is no longer the authority; self-authority must fail.
        let _ = mint(rotated_def, holding_account(), 10_000, TOKEN_PROGRAM_ID);
    }
}
