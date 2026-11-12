use ata_core::{AtaState, Holding, Internal, TransferKind, TransferRequest};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, StateReply, SyncMessage},
};
use nft_core::NftState;
use token_core::TokenState;

use crate::holding_call;

fn source(input: &ReceiveInput, request: &TransferRequest) -> AccountId {
    ata_core::get_associated_token_account_id(
        &input.receiver.program_account_id,
        &ata_core::compute_ata_seed(request.asset_program, request.owner, request.definition),
    )
}

fn read(account: AccountId, request: &TransferRequest) -> SyncMessage {
    let actor = Actor::new(account, request.asset_program);
    match request.kind {
        TransferKind::Token { .. } => SyncMessage::new(actor, &token_core::Message::ReadState),
        TransferKind::Nft => SyncMessage::new(actor, &nft_core::Message::ReadState),
    }
}

pub(crate) fn start(input: &ReceiveInput, request: TransferRequest) -> Response {
    let first = read(source(input, &request), &request);
    Response::set_state(AtaState::ReadingSource(request).encode())
        .send(first)
        .send_sync(input.receiver, &Internal::AssertIdle)
}

fn snapshot(
    input: &ReceiveInput,
    account: AccountId,
    program: AccountId,
    definition: AccountId,
    kind: TransferKind,
) -> Holding {
    assert_eq!(
        input.from,
        Some(Actor::new(account, program)),
        "unexpected ATA state reply sender"
    );
    let reply: StateReply =
        borsh::from_slice(&input.message).expect("asset state reply must decode");
    let (actual_definition, holding) = match kind {
        TransferKind::Token { .. } => match TokenState::decode(&reply.state) {
            Some(TokenState::Holding(holding)) => {
                (holding.definition_id, Holding::Token(holding.balance))
            }
            Some(TokenState::Definition(_)) | None => panic!("expected initialized token holding"),
        },
        TransferKind::Nft => match NftState::decode(&reply.state) {
            Some(NftState::Master(holding)) => (
                holding.definition_id,
                Holding::Master(holding.print_balance),
            ),
            Some(NftState::Copy(holding)) => (holding.definition_id, Holding::Copy(holding.owned)),
            Some(NftState::Definition(_)) | None => panic!("expected initialized NFT holding"),
        },
    };
    assert_eq!(
        actual_definition, definition,
        "holding has another definition"
    );
    holding
}

pub(crate) fn reply(input: &ReceiveInput, state: AtaState) -> Response {
    match state {
        AtaState::ReadingSource(request) => {
            let source = snapshot(
                input,
                source(input, &request),
                request.asset_program,
                request.definition,
                request.kind,
            );
            let next = read(input.receiver.account_id, &request);
            Response::set_state(AtaState::ReadingRecipient { request, source }.encode()).send(next)
        }
        AtaState::ReadingRecipient {
            request,
            source: before_source,
        } => {
            let before_recipient = snapshot(
                input,
                input.receiver.account_id,
                request.asset_program,
                request.definition,
                request.kind,
            );
            let (expected, call) = match request.kind {
                TransferKind::Token { amount } => {
                    let Holding::Token(balance) = before_recipient else {
                        unreachable!("token snapshot")
                    };
                    // Both holdings and owner authorization have already been validated.
                    // A zero credit cannot distinguish deferred delivery from settlement.
                    if amount == 0 {
                        return Response::set_state(Vec::new());
                    }
                    let balance = if source(input, &request) == input.receiver.account_id {
                        balance
                    } else {
                        balance
                            .checked_add(amount)
                            .expect("recipient balance overflow")
                    };
                    (
                        Holding::Token(balance),
                        holding_call(
                            input,
                            request.owner,
                            request.asset_program,
                            request.definition,
                            &token_core::Message::Transfer {
                                to: input.receiver.account_id,
                                amount,
                            },
                        ),
                    )
                }
                TransferKind::Nft => {
                    assert!(
                        matches!(before_recipient, Holding::Master(0) | Holding::Copy(false)),
                        "recipient already holds an NFT"
                    );
                    assert!(
                        matches!(before_source, Holding::Master(n) if n > 0)
                            || before_source == Holding::Copy(true),
                        "source holds no NFT"
                    );
                    (
                        before_source,
                        holding_call(
                            input,
                            request.owner,
                            request.asset_program,
                            request.definition,
                            &nft_core::Message::Transfer {
                                to: input.receiver.account_id,
                            },
                        ),
                    )
                }
            };
            Response::set_state(
                AtaState::VerifyingRecipient {
                    asset_program: request.asset_program,
                    definition: request.definition,
                    expected,
                }
                .encode(),
            )
            .send(call)
            .send(read(input.receiver.account_id, &request))
        }
        AtaState::VerifyingRecipient {
            asset_program,
            definition,
            expected,
        } => {
            let kind = match expected {
                Holding::Token(_) => TransferKind::Token { amount: 0 },
                Holding::Master(_) | Holding::Copy(_) => TransferKind::Nft,
            };
            let actual = snapshot(
                input,
                input.receiver.account_id,
                asset_program,
                definition,
                kind,
            );
            assert_eq!(actual, expected, "ATA transfer did not settle atomically");
            Response::set_state(Vec::new())
        }
    }
}
