use amm_core::{compute_protocol_fee_pda, compute_protocol_fee_pda_seed, ConfigFacts};
use lee_core::{
    account::AccountId,
    program::{AccountMeta, ChainedCall},
};
use token_core::TokenDescriptor;

use crate::new_definition::transfer_call;

const CONTEXT: &str = "Withdraw protocol fees";

/// Plan half of `WithdrawProtocolFees`: gate on the admin and chain the transfer
/// out of the protocol-fee holding.
///
/// Protocol fees accrue in an AMM-owned PDA holding per `(config, token)`; this
/// moves `amount` of one token out to `destination` under that PDA's seed.
///
/// The two holding reads from the v0.2.5 version are gone and this instruction
/// emits no guard of its own to replace them, because the chained transfer
/// already carries exactly what they established. `descriptor` names the asset,
/// and the Token Program asserts it against *both* sides — so a protocol-fee
/// holding of a different token, or a destination that holds something else,
/// fails there. The one thing the descriptor cannot vouch for is the source's
/// *address*, and that is checked here against the protocol-fee PDA for the token
/// the descriptor names, which is what stops this being pointed at an arbitrary
/// AMM-controlled holding.
///
/// # Panics
/// Panics if:
/// - `authority` is not the config's stored admin (pinned by the config guard) or did not sign.
/// - `protocol_fee_holding` is not the protocol-fee PDA for `(config, declared token)`.
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit config, holding, destination, and authority accounts"
)]
#[must_use]
pub fn plan_withdraw_protocol_fees(
    config: &AccountMeta,
    protocol_fee_holding: &AccountMeta,
    destination: &AccountMeta,
    authority: &AccountMeta,
    amount: u128,
    facts: &ConfigFacts,
    descriptor: &TokenDescriptor,
    amm_program_id: AccountId,
) -> ChainedCall {
    assert_eq!(
        authority.account_id, facts.authority,
        "{CONTEXT}: caller is not the configured admin authority"
    );
    assert!(
        authority.is_authorized,
        "{CONTEXT}: admin authority must authorize the withdrawal"
    );
    assert_eq!(
        protocol_fee_holding.account_id,
        compute_protocol_fee_pda(amm_program_id, config.account_id, descriptor.definition_id),
        "{CONTEXT}: holding does not match the protocol-fee PDA"
    );

    transfer_call(
        facts.token_program_id,
        protocol_fee_holding.account_id,
        destination.account_id,
        descriptor.definition_id,
        amount,
    )
    .with_pda_seeds(vec![compute_protocol_fee_pda_seed(
        config.account_id,
        descriptor.definition_id,
    )])
}
