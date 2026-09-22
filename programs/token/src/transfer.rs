use lee_core::{
    account::{Account, AccountWithMetadata, BalanceDiff, Data},
    program::AccountStateDiff,
};
use token_core::TokenHolding;

pub fn transfer(
    sender: AccountWithMetadata,
    recipient: AccountWithMetadata,
    balance_to_move: u128,
) -> Vec<AccountStateDiff> {
    assert!(sender.is_authorized, "Sender authorization is missing");

    let mut sender_holding =
        TokenHolding::try_from(&sender.account.data).expect("Invalid sender data");

    let mut recipient_holding = if recipient.account == Account::default() {
        TokenHolding::zeroized_clone_from(&sender_holding)
    } else {
        TokenHolding::try_from(&recipient.account.data).expect("Invalid recipient data")
    };

    assert_eq!(
        sender_holding.definition_id(),
        recipient_holding.definition_id(),
        "Sender and recipient definition id mismatch"
    );

    match (&mut sender_holding, &mut recipient_holding) {
        (
            TokenHolding::Fungible {
                definition_id: _,
                balance: sender_balance,
            },
            TokenHolding::Fungible {
                definition_id: _,
                balance: recipient_balance,
            },
        ) => {
            *sender_balance = sender_balance
                .checked_sub(balance_to_move)
                .expect("Insufficient balance");

            *recipient_balance = recipient_balance
                .checked_add(balance_to_move)
                .expect("Recipient balance overflow");
        }
        (
            TokenHolding::NftMaster {
                definition_id: _,
                print_balance: sender_print_balance,
            },
            TokenHolding::NftMaster {
                definition_id: _,
                print_balance: recipient_print_balance,
            },
        ) => {
            assert_eq!(
                *recipient_print_balance, 0,
                "Invalid balance in recipient account for NFT transfer"
            );

            assert_eq!(
                *sender_print_balance, balance_to_move,
                "Invalid balance for NFT Master transfer"
            );

            std::mem::swap(sender_print_balance, recipient_print_balance);
        }
        (
            TokenHolding::NftPrintedCopy {
                definition_id: _,
                owned: sender_owned,
            },
            TokenHolding::NftPrintedCopy {
                definition_id: _,
                owned: recipient_owned,
            },
        ) => {
            assert_eq!(
                balance_to_move, 1,
                "Invalid balance for NFT Printed Copy transfer"
            );

            assert!(*sender_owned, "Sender does not own the NFT Printed Copy");

            assert!(
                !*recipient_owned,
                "Recipient already owns the NFT Printed Copy"
            );

            *sender_owned = false;
            *recipient_owned = true;
        }
        _ => {
            panic!("Mismatched token holding types for transfer");
        }
    };

    // Deliberately no `is_authorized` check on an uninitialized recipient: crediting a
    // recipient who has not participated is a supported flow (see the private foreign-init
    // path, where the recipient is known only by `npk`/`vpk`). v0.2.4 expressed the claim as
    // `Claim::Authorized`, but logos-execution-zone PR #621 made foreign-init pre-states carry
    // `is_authorized = true` as a circuit artifact rather than as recipient consent, so that
    // claim never actually gated this path. v0.2.5 makes it honest — `is_authorized` now
    // matches whether a credential was supplied — and acquires ownership on any data write.
    // A guard here cannot distinguish an unauthorized public recipient from a legitimate
    // private foreign init, so squatting on unowned accounts is the runtime's to prevent.
    vec![
        AccountStateDiff::new(sender, BalanceDiff::Add(0), Data::from(&sender_holding)),
        AccountStateDiff::new(
            recipient,
            BalanceDiff::Add(0),
            Data::from(&recipient_holding),
        ),
    ]
}
