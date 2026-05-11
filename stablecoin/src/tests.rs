use nssa_core::account::{Account, AccountId, AccountWithMetadata};

#[test]
fn noop_returns_single_post_state() {
    let account = AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: AccountId::new([0u8; 32]),
    };
    let post_states = crate::noop::noop(account);
    assert_eq!(post_states.len(), 1);
}
