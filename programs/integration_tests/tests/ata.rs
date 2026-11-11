//! Real-guest ATA conversations, including atomic delivery and privacy boundaries.
#[path = "ata/privacy.rs"]
mod privacy;
#[path = "ata/support.rs"]
mod support;

use integration_tests::{account, assert_refused, key};
use lee_core::account::{AccountId, Actor};
use nft_core::NftState;
use support::*;
use token_core::TokenState;

#[test]
fn create_is_permissionless_idempotent_and_preserves_funded_holdings() {
    let mut chain = Chain::new();
    let owner = key(12);
    let def = chain.define(false, 100);
    let holding = chain.create(TOKEN, account(&owner), def);
    chain.fund(TOKEN, holding, 100);
    chain.create(TOKEN, account(&owner), def);
    assert_eq!(chain.balance(holding), 100);
    chain.idle(account(&key(11)));
    chain.idle(account(&owner));
    let nft_def = chain.define(true, 10);
    let nft_holding = chain.create(NFT, account(&owner), nft_def);
    chain.fund(NFT, nft_holding, 0);
    chain.create(NFT, account(&owner), nft_def);
    assert!(matches!(chain.nft(nft_holding), NftState::Master(m) if m.print_balance == 10));
    assert_ne!(holding, nft_holding);
}

#[test]
fn fungible_transfer_self_transfer_zero_and_burn_settle() {
    let mut chain = Chain::new();
    let (owner, recipient) = (key(12), key(13));
    let def = chain.define(false, 100);
    let source = chain.create(TOKEN, account(&owner), def);
    chain.fund(TOKEN, source, 100);
    chain.initialize(TOKEN, &recipient, def);
    chain
        .transfer(TOKEN, &owner, def, account(&recipient), 20)
        .unwrap();
    assert_eq!(chain.balance(source), 80);
    assert_eq!(chain.balance(account(&recipient)), 20);
    chain.transfer(TOKEN, &owner, def, source, 70).unwrap();
    assert_eq!(chain.balance(source), 80);
    let events = chain
        .transfer(TOKEN, &owner, def, account(&recipient), 0)
        .unwrap();
    assert!(events.is_empty());
    chain.burn(TOKEN, &owner, def, 30).unwrap();
    assert_eq!(chain.balance(source), 50);
    assert!(matches!(chain.token(def), TokenState::Definition(d) if d.total_supply == 70));
    chain.idle(account(&owner));
    chain.idle(account(&recipient));
    chain.idle(source);
    assert_eq!(chain.state.publications_from(0).count(), 0);
}

#[test]
fn token_failures_roll_back_balances_and_coordinator() {
    let mut chain = Chain::new();
    let (owner, recipient, other) = (key(12), key(13), key(14));
    let def = chain.define(false, 100);
    let source = chain.create(TOKEN, account(&owner), def);
    chain.fund(TOKEN, source, 100);
    chain.initialize(TOKEN, &recipient, def);
    // Admit the owner independently of the failed transactions below.
    chain.initialize(TOKEN, &owner, def);
    // Admit a recipient without initializing its asset holding.
    let other_root = Actor::new(account(&other), ATA);
    chain
        .call(
            other_root,
            &[
                other_root,
                Actor::new(source, TOKEN),
                Actor::new(def, TOKEN),
            ],
            &[&other],
            &ata_core::Message::Create {
                token_program_id: TOKEN,
                owner: account(&owner),
                definition: def,
            },
        )
        .unwrap();
    for (to, amount, reason) in [
        (account(&recipient), 101, "insufficient balance"),
        (account(&other), 1, "expected initialized token holding"),
        (account(&other), 0, "expected initialized token holding"),
        (def, 1, "expected initialized token holding"),
    ] {
        let before = chain.state.clone();
        assert_refused(&chain.transfer(TOKEN, &owner, def, to, amount), reason);
        assert!(
            chain.state == before,
            "failed transfer must leave all state unchanged"
        );
    }
    // Recipient coordinator is declared, but its asset actor is omitted.
    let root = Actor::new(account(&owner), ATA);
    let declared = [
        root,
        Actor::new(source, TOKEN),
        Actor::new(account(&recipient), ATA),
    ];
    let before = chain.state.clone();
    assert!(chain
        .call(
            root,
            &declared,
            &[&owner],
            &transfer_message(TOKEN, def, account(&recipient), 1)
        )
        .is_err());
    assert!(
        chain.state == before,
        "failed transfer must leave all state unchanged"
    );
    // No owner credential; admission alone must not authorize spending.
    let declared = [
        root,
        Actor::new(source, TOKEN),
        Actor::new(account(&recipient), ATA),
        Actor::new(account(&recipient), TOKEN),
    ];
    let result = chain.call(
        root,
        &declared,
        &[],
        &transfer_message(TOKEN, def, account(&recipient), 1),
    );
    assert_refused(&result, "ATA owner must authorize spending");
    assert!(
        chain.state == before,
        "failed transfer must leave all state unchanged"
    );
    // Burn must declare the definition, so supply and balance cannot diverge.
    assert!(chain
        .call(
            root,
            &[root, Actor::new(source, TOKEN)],
            &[&owner],
            &burn_message(TOKEN, def, 1)
        )
        .is_err());
    assert!(
        chain.state == before,
        "failed transfer must leave all state unchanged"
    );
}

#[test]
fn recipient_must_hold_the_same_definition() {
    let mut chain = Chain::new();
    let (owner, recipient, other_def) = (key(12), key(13), key(14));
    let def = chain.define(false, 100);
    let source = chain.create(TOKEN, account(&owner), def);
    chain.fund(TOKEN, source, 100);
    let root = Actor::new(account(&other_def), TOKEN);
    chain
        .call(
            root,
            &[root, Actor::new(account(&recipient), TOKEN)],
            &[&other_def, &recipient],
            &token_core::Message::NewDefinition {
                name: "Other".into(),
                total_supply: 0,
                mint_authority: None,
                holding: account(&recipient),
            },
        )
        .unwrap();
    let before = chain.state.clone();
    let result = chain.transfer(TOKEN, &owner, def, account(&recipient), 1);
    assert_refused(&result, "holding has another definition");
    assert!(
        chain.state == before,
        "failed transfer must leave all state unchanged"
    );
}

#[test]
fn nft_master_transfer_and_partial_and_full_burn() {
    let mut chain = Chain::new();
    let (owner, recipient) = (key(12), key(13));
    let def = chain.define(true, 10);
    let source = chain.create(NFT, account(&owner), def);
    chain.fund(NFT, source, 0);
    chain.burn(NFT, &owner, def, 3).unwrap();
    let destination = chain.create(NFT, account(&recipient), def);
    chain.transfer(NFT, &owner, def, destination, 0).unwrap();
    assert!(matches!(chain.nft(source), NftState::Master(m) if m.print_balance == 0));
    assert!(matches!(chain.nft(destination), NftState::Master(m) if m.print_balance == 7));
    let before = chain.state.clone();
    assert_refused(
        &chain.transfer(NFT, &owner, def, source, 0),
        "source holds no NFT",
    );
    assert!(chain.state == before);
    chain.burn(NFT, &recipient, def, 7).unwrap();
    assert!(matches!(chain.nft(def), NftState::Definition(d) if d.printable_supply == 0));
    chain.idle(destination);
    assert_eq!(chain.state.publications_from(0).count(), 0);
}

#[test]
fn nft_copy_transfer_requires_empty_recipient_and_burns_exactly_one() {
    let mut chain = Chain::new();
    let (owner, recipient) = (key(12), key(13));
    let def = chain.define(true, 10);
    let source = chain.create(NFT, account(&owner), def);
    let destination = chain.create(NFT, account(&recipient), def);
    let master = key(11);
    let root = Actor::new(account(&master), NFT);
    for to in [source, destination] {
        chain
            .call(
                root,
                &[root, Actor::new(to, NFT)],
                &[&master],
                &nft_core::Message::PrintNft { to },
            )
            .unwrap();
    }
    let before = chain.state.clone();
    assert_refused(
        &chain.transfer(NFT, &owner, def, destination, 0),
        "recipient already holds an NFT",
    );
    assert!(
        chain.state == before,
        "failed transfer must leave all state unchanged"
    );
    assert!(chain.burn(NFT, &recipient, def, 0).is_err());
    assert!(chain.burn(NFT, &recipient, def, 2).is_err());
    assert!(
        chain.state == before,
        "failed transfer must leave all state unchanged"
    );
    chain.burn(NFT, &recipient, def, 1).unwrap();
    chain.transfer(NFT, &owner, def, destination, 0).unwrap();
    assert!(matches!(chain.nft(source), NftState::Copy(c) if !c.owned));
    assert!(matches!(chain.nft(destination), NftState::Copy(c) if c.owned));
    chain.burn(NFT, &recipient, def, 1).unwrap();
    assert!(matches!(chain.nft(def), NftState::Definition(d) if d.printable_supply == 8));
}

#[test]
fn alternate_ata_deployments_and_owners_have_distinct_holdings() {
    let mut chain = Chain::new();
    let (owner, other) = (key(12), key(13));
    let def = chain.define(false, 100);
    let first = chain.create(TOKEN, account(&owner), def);
    let second = chain.create(TOKEN, account(&other), def);
    assert_ne!(first, second);
    let twin = AccountId::new([0xb1; 32]);
    chain.state = chain.state.with_named_programs([(
        twin,
        lee::program::Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap(),
    )]);
    let third = ata_core::get_associated_token_account_id(
        &twin,
        &ata_core::compute_ata_seed(TOKEN, account(&owner), def),
    );
    let root = Actor::new(account(&key(11)), twin);
    chain
        .call(
            root,
            &[root, Actor::new(third, TOKEN), Actor::new(def, TOKEN)],
            &[],
            &ata_core::Message::Create {
                token_program_id: TOKEN,
                owner: account(&owner),
                definition: def,
            },
        )
        .unwrap();
    assert_ne!(first, third);
    assert_eq!(chain.balance(third), 0);
}

#[test]
fn missing_asset_reply_triggers_completion_assertion_and_rolls_back() {
    let mut chain = Chain::new();
    let (owner, recipient) = (key(12), key(13));
    let definition = chain.define(false, 100);
    chain.initialize(TOKEN, &recipient, definition);
    let silent_asset = AccountId::new([0xf1; 32]);
    chain.state = chain.state.with_named_programs([(
        silent_asset,
        lee::program::Program::new(ata_mock_methods::NO_REPLY_ELF.to_vec().into())
            .expect("silent guest loads"),
    )]);
    // The nonconforming asset accepts initialization, admitting the source PDA,
    // but returns no ReadState reply. Real ATA must reject at its final self-call.
    let source = chain.create(silent_asset, account(&owner), definition);
    let before = chain.state.clone();
    let result = chain.transfer(silent_asset, &owner, definition, account(&recipient), 1);
    assert_refused(&result, "ATA transfer did not complete");
    assert!(
        chain.state == before,
        "missing reply cannot commit pending state"
    );
    chain.idle(account(&owner));
    chain.idle(account(&recipient));
    chain.idle(source);
    assert_eq!(chain.state.publications_from(0).count(), 0);
}
