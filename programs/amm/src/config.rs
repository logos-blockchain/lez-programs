use amm_core::{
    assert_valid_protocol_fee_bps, assert_valid_swap_fee_bps, compute_config_pda,
    compute_config_pda_seed, compute_protocol_fee_pda, compute_protocol_fee_pda_seed, event::Event,
    AmmState, Internal, Message, Pending,
};
use lee_core::program::{ReceiveInput, Response, SyncMessage};

use crate::{actor, reads, settlement};

pub(crate) fn forward(input: &ReceiveInput, message: Message) -> Response {
    assert!(input.is_authorized, "AMM owner or authority must authorize");
    let seed = if let Message::Initialize { nonce, .. } = &message {
        Some(compute_config_pda_seed(input.receiver.account_id, *nonce))
    } else {
        None
    };
    let (to, message) = match message {
        Message::Initialize { nonce, config } => (
            compute_config_pda(
                input.receiver.program_account_id,
                input.receiver.account_id,
                nonce,
            ),
            Internal::InitializeConfig { nonce, config },
        ),
        Message::UpdateConfig {
            config,
            new_authority,
        } => (config, Internal::UpdateConfig { new_authority }),
        Message::WithdrawProtocolFees {
            config,
            token,
            destination,
            amount,
        } => (
            config,
            Internal::WithdrawProtocolFees {
                token,
                destination,
                amount,
            },
        ),
        Message::NewDefinition { .. }
        | Message::AddLiquidity { .. }
        | Message::RemoveLiquidity { .. }
        | Message::SwapExactInput { .. }
        | Message::SwapExactOutput { .. }
        | Message::SyncReserves
        | Message::CreatePriceObservations { .. }
        | Message::CreateOraclePriceAccount { .. }
        | Message::ReadState => unreachable!("only admin messages are forwarded"),
    };
    Response::keep_state().send(SyncMessage::new(actor(input, to), &message).with_pda_seeds(seed))
}

pub(crate) fn internal(
    input: &ReceiveInput,
    state: Option<AmmState>,
    message: Internal,
) -> Response {
    let from = input.from.expect("internal message has a sender");
    if let Internal::InitializeConfig { nonce, config } = message {
        assert!(state.is_none(), "AMM config already exists");
        assert_eq!(
            input.receiver.account_id,
            compute_config_pda(input.receiver.program_account_id, from.account_id, nonce),
            "config does not match the owner's namespace"
        );
        assert_valid_swap_fee_bps(config.swap_fee_bps);
        assert_valid_protocol_fee_bps(config.protocol_fee_bps);
        assert_ne!(
            config.token_program_id, input.receiver.program_account_id,
            "token program must differ from AMM"
        );
        assert_ne!(
            config.twap_oracle_program_id, input.receiver.program_account_id,
            "oracle program must differ from AMM"
        );
        return Response::set_state(AmmState::Config(config).encode()).event(
            Event::ConfigInitialized {
                config: input.receiver.account_id,
            }
            .to_program_event(),
        );
    }
    let Some(AmmState::Config(mut config)) = state else {
        panic!("receiver is not an AMM config")
    };
    assert_eq!(
        from.account_id, config.authority,
        "sender is not the AMM admin authority"
    );
    match message {
        Internal::UpdateConfig { new_authority } => {
            config.authority = new_authority;
            Response::set_state(AmmState::Config(config).encode()).event(
                Event::AuthorityChanged {
                    authority: new_authority,
                }
                .to_program_event(),
            )
        }
        Internal::WithdrawProtocolFees {
            token,
            destination,
            amount,
        } => {
            let config_id = input.receiver.account_id;
            let source =
                compute_protocol_fee_pda(input.receiver.program_account_id, config_id, token);
            let pending = Pending {
                config_id,
                config: Some(config),
                pool: None,
                request: Message::WithdrawProtocolFees {
                    config: config_id,
                    token,
                    destination,
                    amount,
                },
                expected: input.receiver,
                remaining: vec![],
                holdings: vec![],
            };
            reads::holdings(input, pending, vec![source, destination])
        }
        Internal::InitializeConfig { .. }
        | Internal::ReadState
        | Internal::Complete
        | Internal::NewPool { .. } => {
            unreachable!("handled before admin dispatch")
        }
    }
}

pub(crate) fn withdraw(input: &ReceiveInput, pending: &Pending) -> Response {
    let Message::WithdrawProtocolFees {
        token,
        destination,
        amount,
        ..
    } = pending.request
    else {
        unreachable!("withdraw continuation")
    };
    let [source, target] = pending.holdings.as_slice() else {
        unreachable!("two holdings read")
    };
    let source = reads::holding(source, token);
    reads::holding(target, token);
    assert!(amount <= source.balance, "insufficient protocol fees");
    let config = pending.config.as_ref().expect("config was read");
    let source_id =
        compute_protocol_fee_pda(input.receiver.program_account_id, pending.config_id, token);
    let response = Response::keep_state()
        .send(settlement::token_call(
            config,
            source_id,
            &token_core::Message::Transfer {
                to: destination,
                amount,
            },
            Some(compute_protocol_fee_pda_seed(pending.config_id, token)),
        ))
        .event(
            Event::ProtocolFeesWithdrawn {
                token,
                destination,
                amount,
            }
            .to_program_event(),
        );
    settlement::finish(input, AmmState::Config(config.clone()), response)
}
