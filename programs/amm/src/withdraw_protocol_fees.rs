use amm_core::{compute_protocol_fee_pda, compute_protocol_fee_pda_seed, AmmConfig};
use lee_core::{
    account::AccountWithMetadata,
    program::{AccountPostState, ChainedCall, ProgramId},
};

/// Withdraw `amount` of accrued protocol fees for one token from its instance-wide protocol-fee
/// holding to a destination holding. Only the config's admin `authority` may call this.
///
/// Protocol fees accrue in an AMM-owned PDA holding per `(config, token)`
/// ([`compute_protocol_fee_pda`]); this moves `amount` out to `destination` under that PDA's seed.
/// The withdrawn token is the protocol-fee holding's own definition, so one call drains one token.
///
/// # Panics
/// - `config` is not owned by the AMM Program, or does not decode as an [`AmmConfig`].
/// - `authority.account_id` is not `config.authority`, or `authority` is not authorized (signed).
/// - `protocol_fee_holding` or `destination` is not owned by the config's token program, or does
///   not decode as a token holding.
/// - `protocol_fee_holding.account_id` is not the protocol-fee PDA for its own token definition.
/// - `destination` holds a different token than `protocol_fee_holding`.
/// - `amount` exceeds the holding's balance (the chained transfer's checked debit).
pub fn withdraw_protocol_fees(
    config: AccountWithMetadata,
    protocol_fee_holding: AccountWithMetadata,
    destination: AccountWithMetadata,
    authority: AccountWithMetadata,
    amount: u128,
    amm_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert_eq!(
        config.account.program_owner, amm_program_id,
        "Withdraw protocol fees: AMM config account must be owned by the AMM Program"
    );
    let config_data = AmmConfig::try_from(&config.account.data)
        .expect("Withdraw protocol fees: AMM Program must be initialized before use");

    // Admin gate: only the configured authority may move protocol fees.
    assert_eq!(
        authority.account_id, config_data.authority,
        "Withdraw protocol fees: caller is not the configured admin authority"
    );
    assert!(
        authority.is_authorized,
        "Withdraw protocol fees: admin authority must authorize the withdrawal"
    );

    // Route the chained transfer through the AMM's *configured* Token Program — not whatever
    // program a caller-supplied account happens to name — so a malformed account fails here with a
    // clear error instead of chaining a call into an unexpected program. Both holdings must be
    // owned by it (mirrors the swap path, which validates vaults/holdings against the config's
    // token program).
    let token_program_id = config_data.token_program_id;
    assert_eq!(
        protocol_fee_holding.account.program_owner, token_program_id,
        "Withdraw protocol fees: protocol-fee holding must be owned by the configured Token Program"
    );

    // The source must be the instance's protocol-fee PDA for its own token definition, so a caller
    // can't point this at an arbitrary AMM-owned holding.
    let token_def = token_core::TokenHolding::try_from(&protocol_fee_holding.account.data)
        .expect("Withdraw protocol fees: protocol-fee holding must be a valid token holding")
        .definition_id();
    assert_eq!(
        protocol_fee_holding.account_id,
        compute_protocol_fee_pda(amm_program_id, config.account_id, token_def),
        "Withdraw protocol fees: holding does not match the protocol-fee PDA"
    );

    // The destination must be an initialized holding of the SAME token, owned by the configured
    // Token Program — otherwise the chained credit would fail deep in the token program. Rejecting
    // a fresh / wrong-token / wrong-program destination here gives a clear, early error.
    assert_eq!(
        destination.account.program_owner, token_program_id,
        "Withdraw protocol fees: destination must be owned by the configured Token Program"
    );
    let destination_token_def = token_core::TokenHolding::try_from(&destination.account.data)
        .expect("Withdraw protocol fees: destination must be a valid token holding")
        .definition_id();
    assert_eq!(
        destination_token_def, token_def,
        "Withdraw protocol fees: destination token does not match the protocol-fee holding's token"
    );

    // Move `amount` out of the protocol-fee holding to the destination, under the protocol-fee
    // PDA's seed. `destination` must be an initialized holding of the same token (an existing
    // holding is credited without a signature; a fresh one would fail the token program's claim).
    let mut source = protocol_fee_holding.clone();
    source.is_authorized = true;
    let transfer = ChainedCall::new(
        token_program_id,
        vec![source, destination.clone()],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
        },
    )
    .with_pda_seeds(vec![compute_protocol_fee_pda_seed(
        config.account_id,
        token_def,
    )]);

    // Post-states mirror the input account order; the chained transfer applies the balance moves.
    let post_states = vec![
        AccountPostState::new(config.account),
        AccountPostState::new(protocol_fee_holding.account),
        AccountPostState::new(destination.account),
        AccountPostState::new(authority.account),
    ];

    (post_states, vec![transfer])
}

#[cfg(test)]
mod tests {
    use amm_core::compute_config_pda;
    use lee_core::account::{Account, AccountId, Data, Nonce};

    use super::*;

    const AMM_PROGRAM_ID: ProgramId = [42; 8];
    const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
    const NONCE: [u8; 32] = [3; 32];
    const WITHDRAW_AMOUNT: u128 = 500;

    fn owner_id() -> AccountId {
        AccountId::new([5; 32])
    }

    fn authority_id() -> AccountId {
        AccountId::new([9; 32])
    }

    fn token_def_id() -> AccountId {
        AccountId::new([42; 32])
    }

    fn config_id() -> AccountId {
        compute_config_pda(AMM_PROGRAM_ID, owner_id(), NONCE)
    }

    fn config_init() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: AMM_PROGRAM_ID,
                balance: 0,
                data: Data::from(&AmmConfig {
                    token_program_id: TOKEN_PROGRAM_ID,
                    twap_oracle_program_id: [77; 8],
                    authority: authority_id(),
                    swap_fee_bps: 30,
                    protocol_fee_bps: 1_000,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: config_id(),
        }
    }

    fn protocol_fee_holding() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0,
                data: Data::from(&token_core::TokenHolding::Fungible {
                    definition_id: token_def_id(),
                    balance: 10_000,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: compute_protocol_fee_pda(AMM_PROGRAM_ID, config_id(), token_def_id()),
        }
    }

    fn destination() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0,
                data: Data::from(&token_core::TokenHolding::Fungible {
                    definition_id: token_def_id(),
                    balance: 0,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: AccountId::new([88; 32]),
        }
    }

    fn authority_signed() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: authority_id(),
        }
    }

    fn run() -> (Vec<AccountPostState>, Vec<ChainedCall>) {
        withdraw_protocol_fees(
            config_init(),
            protocol_fee_holding(),
            destination(),
            authority_signed(),
            WITHDRAW_AMOUNT,
            AMM_PROGRAM_ID,
        )
    }

    #[test]
    fn emits_transfer_from_protocol_pda_to_destination() {
        let (post_states, chained_calls) = run();
        assert_eq!(post_states.len(), 4);
        assert_eq!(chained_calls.len(), 1);

        let transfer = &chained_calls[0];
        assert_eq!(transfer.program_id, TOKEN_PROGRAM_ID);
        // Source is the protocol-fee PDA (authorized via its seed); destination is the target.
        assert_eq!(
            transfer.pre_states[0].account_id,
            compute_protocol_fee_pda(AMM_PROGRAM_ID, config_id(), token_def_id())
        );
        assert!(transfer.pre_states[0].is_authorized);
        assert_eq!(transfer.pre_states[1].account_id, destination().account_id);
        assert_eq!(
            transfer.pda_seeds,
            vec![compute_protocol_fee_pda_seed(config_id(), token_def_id())]
        );
        // The transfer moves exactly `WITHDRAW_AMOUNT` of the protocol-fee token.
        let expected = ChainedCall::new(
            TOKEN_PROGRAM_ID,
            transfer.pre_states.clone(),
            &token_core::Instruction::Transfer {
                amount_to_transfer: WITHDRAW_AMOUNT,
            },
        );
        assert_eq!(transfer.instruction_data, expected.instruction_data);
    }

    #[test]
    #[should_panic(expected = "caller is not the configured admin authority")]
    fn non_admin_authority_panics() {
        let mut stranger = authority_signed();
        stranger.account_id = AccountId::new([123; 32]);
        withdraw_protocol_fees(
            config_init(),
            protocol_fee_holding(),
            destination(),
            stranger,
            WITHDRAW_AMOUNT,
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "admin authority must authorize")]
    fn unauthorized_authority_panics() {
        let mut unsigned = authority_signed();
        unsigned.is_authorized = false;
        withdraw_protocol_fees(
            config_init(),
            protocol_fee_holding(),
            destination(),
            unsigned,
            WITHDRAW_AMOUNT,
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "AMM config account must be owned by the AMM Program")]
    fn config_not_amm_owned_panics() {
        let mut foreign = config_init();
        foreign.account.program_owner = [1; 8];
        withdraw_protocol_fees(
            foreign,
            protocol_fee_holding(),
            destination(),
            authority_signed(),
            WITHDRAW_AMOUNT,
            AMM_PROGRAM_ID,
        );
    }

    /// A holding whose account id is not the protocol-fee PDA for its own token is rejected, so a
    /// caller cannot drain an arbitrary AMM-owned holding through this instruction.
    #[test]
    #[should_panic(expected = "holding does not match the protocol-fee PDA")]
    fn wrong_protocol_holding_pda_panics() {
        let mut wrong = protocol_fee_holding();
        wrong.account_id = AccountId::new([7; 32]);
        withdraw_protocol_fees(
            config_init(),
            wrong,
            destination(),
            authority_signed(),
            WITHDRAW_AMOUNT,
            AMM_PROGRAM_ID,
        );
    }

    /// The source must be owned by the config's token program, so a malformed holding can't route
    /// the chained transfer into an unexpected program.
    #[test]
    #[should_panic(expected = "protocol-fee holding must be owned by the configured Token Program")]
    fn protocol_holding_owned_by_foreign_program_panics() {
        let mut foreign = protocol_fee_holding();
        foreign.account.program_owner = [1; 8];
        withdraw_protocol_fees(
            config_init(),
            foreign,
            destination(),
            authority_signed(),
            WITHDRAW_AMOUNT,
            AMM_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "destination must be owned by the configured Token Program")]
    fn destination_owned_by_foreign_program_panics() {
        let mut foreign = destination();
        foreign.account.program_owner = [1; 8];
        withdraw_protocol_fees(
            config_init(),
            protocol_fee_holding(),
            foreign,
            authority_signed(),
            WITHDRAW_AMOUNT,
            AMM_PROGRAM_ID,
        );
    }

    /// The destination must hold the same token as the protocol-fee holding being drained.
    #[test]
    #[should_panic(expected = "destination token does not match the protocol-fee holding's token")]
    fn destination_of_a_different_token_panics() {
        let mut other_token = destination();
        other_token.account.data = Data::from(&token_core::TokenHolding::Fungible {
            definition_id: AccountId::new([99; 32]),
            balance: 0,
        });
        withdraw_protocol_fees(
            config_init(),
            protocol_fee_holding(),
            other_token,
            authority_signed(),
            WITHDRAW_AMOUNT,
            AMM_PROGRAM_ID,
        );
    }
}
