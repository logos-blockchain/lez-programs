use lee_core::account::ShardData;
use token_core::{TokenDefinition, TokenDescriptor, TokenKind};

/// Definition-side guard: turn the instruction's claimed `kind` into a fact.
///
/// Returns nothing — a guard reads a shard and asserts, and must leave it
/// untouched. The holding's apply never sees the definition, so without this the
/// claimed kind would be indistinguishable from a caller-invented one.
///
/// # Panics
/// If the definition does not describe holdings of `kind`.
pub fn check_holding_kind(pre_data: &ShardData, kind: TokenKind) {
    let definition = TokenDefinition::try_from(pre_data).expect("Definition account must be valid");

    assert_eq!(
        TokenKind::from_definition(&definition),
        kind,
        "Token Definition does not initialize this Token Holding kind"
    );
}

/// Holding-side effect: write the empty holding `descriptor` describes.
///
/// `is_authorized` is what the plan saw of this account. An empty shard needs no
/// authorization — writing it denies nothing to anyone — but overwriting one that
/// already holds something does.
///
/// # Panics
/// If the shard already holds data and the account did not authorize.
#[must_use]
pub fn initialize_holding(
    pre_data: &ShardData,
    descriptor: &TokenDescriptor,
    is_authorized: bool,
) -> ShardData {
    assert!(
        pre_data.is_empty() || is_authorized,
        "Only uninitialized or authorized accounts can be initialized"
    );

    ShardData::from(&descriptor.zeroized())
}
