#![deny(unsafe_op_in_unsafe_fn)]

mod account;
mod ffi;

pub mod api;

pub use api::{
    config_id, context, pair_ids, plan, quote, token_ids, AccountRead, AmmApiError, AmmResponse,
    AmmResult, ConfigIdRequest, ContextRequest, PairIdsRequest, PairSnapshot, PlanRequest,
    PositionRequest, QuoteRequest, TokenIdsRequest, WalletAccount, NEW_POSITION_SCHEMA,
};
