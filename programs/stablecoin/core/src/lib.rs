//! Core types for the stablecoin program: actor state, messages, addresses and events.
//!
//! Two actors carry state: the engine (global parameters, fee accumulator and redemption
//! price, on one PDA) and a position (on its owner's own account).
//!
//! State and messages outlive code (state stays on accounts, async messages in mailboxes),
//! and Borsh encodes an enum variant as a leading tag byte. Every enum here is therefore
//! append-only: a changed layout is a new variant, never an edit of an existing one.

#[expect(
    clippy::arithmetic_side_effects,
    reason = "pure fixed-point math predating the workspace lints; see its tests"
)]
pub mod collateralization;
#[expect(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "pure fixed-point math predating the workspace lints; see its tests"
)]
pub mod controller;
/// Exact debt admission comparisons shared by runtime and clients.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "pure fixed-point math predating the workspace lints; see its tests"
)]
pub mod debt_limits;
mod engine;
pub mod event;
#[expect(
    clippy::arithmetic_side_effects,
    reason = "pure fixed-point math predating the workspace lints; see its tests"
)]
pub mod math;
mod position;
pub mod protocol_parameters;
pub mod redemption_price_state;
pub mod stability_fee_accumulator;

use borsh::{BorshDeserialize, BorshSerialize};
pub use controller::{
    bound_redemption_price, run_controller_tick, ControllerOutput, INTEGRAL_CLAMP,
    MAXIMUM_REDEMPTION_PRICE, MINIMUM_REDEMPTION_PRICE, RATE_DELTA_CLAMP,
};
pub use engine::{Engine, Pending};
use lee_core::{account::AccountId, program::PdaSeed};
pub use math::FIXED_POINT_ONE;
pub use position::Position;
pub use protocol_parameters::{
    ProtocolParameters, MAXIMUM_CONTROLLER_INTEGRAL_GAIN, MAXIMUM_CONTROLLER_PROPORTIONAL_GAIN,
    MAXIMUM_STABILITY_FEE_PER_MILLISECOND,
};
pub use redemption_price_state::RedemptionPriceState;
use risc0_zkvm::sha::{Impl, Sha256 as _};
use serde::{Deserialize, Serialize};
pub use stability_fee_accumulator::StabilityFeeAccumulator;

/// Longest accepted stablecoin name, in bytes.
pub const MAX_NAME_LEN: usize = 64;

// Domain-separation tags of the PDA seeds. They must stay unchanged for address stability.
const ENGINE_PDA_DOMAIN: [u8; 32] = *b"STABLECOIN__PROTOCOL_PARAMS_____";
const STABLECOIN_DEFINITION_PDA_DOMAIN: [u8; 32] = *b"STABLECOIN__DEFINITION__________";
const SURPLUS_HOLDING_PDA_DOMAIN: [u8; 32] = *b"STABLECOIN__MASTER_HOLDING______";
const POSITION_VAULT_PDA_DOMAIN: &[u8] = b"POSITION_VAULT";

// ──────────────────────────────────────────────────────────────────────────────
// State
// ──────────────────────────────────────────────────────────────────────────────

/// The stablecoin's state on one account.
///
/// An empty actor state means "nothing here yet" and is not a `StablecoinState`.
#[spel_attrs::state]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[expect(
    clippy::large_enum_variant,
    reason = "decoded once per call; the engine is the common large state and boxing would add an indirection to every read"
)]
pub enum StablecoinState {
    Engine(Engine),
    Position(Position),
}

impl StablecoinState {
    /// Decodes an actor state; `None` for an empty one.
    ///
    /// # Panics
    /// If the bytes are not a `StablecoinState`. Only stablecoin code writes stablecoin
    /// actor state.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() {
            return None;
        }
        Some(borsh::from_slice(bytes).expect("stablecoin state must decode"))
    }

    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("borsh serialization is infallible")
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Messages
// ──────────────────────────────────────────────────────────────────────────────

/// Messages anyone may send: a transaction author or another program.
#[spel_attrs::message]
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[expect(
    clippy::large_enum_variant,
    reason = "decoded once per transaction; boxing `InitializeArgs` would not change the wire format but adds noise"
)]
pub enum Message {
    // At the program account, signed by it, root only.
    Initialize {
        initial_admin_account_id: AccountId,
        freeze_authority_account_id: AccountId,
        token_program_id: AccountId,
        collateral_definition_id: AccountId,
        market_price_oracle_id: AccountId,
        market_price_oracle_program_id: AccountId,
        initial_stability_fee_per_millisecond: u128,
        initial_controller_proportional_gain: i128,
        initial_controller_integral_gain: i128,
        initial_minimum_collateralization_ratio: u128,
        minimum_milliseconds_between_rate_updates: u64,
        maximum_oracle_price_age_milliseconds: u64,
        initial_redemption_price: u128,
        stablecoin_name: String,
        minimum_debt_amount: u128,
        debt_ceiling: u128,
    },

    // At the engine, anyone, no signature.
    AccrueStabilityFee,
    UpdateRedemptionRate,
    RefreshGlobals {
        consult_oracle: bool,
    },

    // At the owner's account, signed by the owner, root only.
    OpenPosition {
        collateral_holding: AccountId,
        initial_collateral_amount: u128,
    },
    DepositCollateral {
        collateral_holding: AccountId,
        amount: u128,
    },
    WithdrawCollateral {
        to: AccountId,
        amount: u128,
    },
    GenerateDebt {
        to: AccountId,
        amount: u128,
    },
    RepayDebt {
        stablecoin_holding: AccountId,
        maximum_amount: u128,
    },
    ClosePosition,

    // At the signer's own account (seat), signed, root only; forwarded to the engine.
    SetMinimumCollateralizationRatio {
        new_ratio: u128,
    },
    SetControllerGains {
        new_proportional_gain: i128,
        new_integral_gain: i128,
    },
    SetMarketPriceOracle {
        oracle_account_id: AccountId,
        oracle_program_id: AccountId,
    },
    SetTimingParameters {
        minimum_milliseconds_between_rate_updates: u64,
        maximum_oracle_price_age_milliseconds: u64,
    },
    SetAdmin {
        new_admin_account_id: AccountId,
    },
    SetFreezeAuthority {
        new_freeze_authority_account_id: AccountId,
    },
    SetStabilityFeePerMillisecond {
        new_rate: u128,
    },
    Freeze,
    Unfreeze,
    ResetRedemptionRateController,
    SetDebtLimits {
        minimum_debt_amount: u128,
        debt_ceiling: u128,
    },
}

/// Everything `Initialize` sets up: the fields of `Message::Initialize`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct InitializeArgs {
    pub initial_admin_account_id: AccountId,
    pub freeze_authority_account_id: AccountId,
    pub token_program_id: AccountId,
    pub collateral_definition_id: AccountId,
    pub market_price_oracle_id: AccountId,
    pub market_price_oracle_program_id: AccountId,
    pub initial_stability_fee_per_millisecond: u128,
    pub initial_controller_proportional_gain: i128,
    pub initial_controller_integral_gain: i128,
    pub initial_minimum_collateralization_ratio: u128,
    pub minimum_milliseconds_between_rate_updates: u64,
    pub maximum_oracle_price_age_milliseconds: u64,
    pub initial_redemption_price: u128,
    pub stablecoin_name: String,
    pub minimum_debt_amount: u128,
    pub debt_ceiling: u128,
}

impl From<InitializeArgs> for Message {
    fn from(args: InitializeArgs) -> Self {
        let InitializeArgs {
            initial_admin_account_id,
            freeze_authority_account_id,
            token_program_id,
            collateral_definition_id,
            market_price_oracle_id,
            market_price_oracle_program_id,
            initial_stability_fee_per_millisecond,
            initial_controller_proportional_gain,
            initial_controller_integral_gain,
            initial_minimum_collateralization_ratio,
            minimum_milliseconds_between_rate_updates,
            maximum_oracle_price_age_milliseconds,
            initial_redemption_price,
            stablecoin_name,
            minimum_debt_amount,
            debt_ceiling,
        } = args;
        Self::Initialize {
            initial_admin_account_id,
            freeze_authority_account_id,
            token_program_id,
            collateral_definition_id,
            market_price_oracle_id,
            market_price_oracle_program_id,
            initial_stability_fee_per_millisecond,
            initial_controller_proportional_gain,
            initial_controller_integral_gain,
            initial_minimum_collateralization_ratio,
            minimum_milliseconds_between_rate_updates,
            maximum_oracle_price_age_milliseconds,
            initial_redemption_price,
            stablecoin_name,
            minimum_debt_amount,
            debt_ceiling,
        }
    }
}

/// The eleven seat-forwarded actions, with the fields of the `Message` variants they come
/// from.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum AdminAction {
    SetMinimumCollateralizationRatio {
        new_ratio: u128,
    },
    SetControllerGains {
        new_proportional_gain: i128,
        new_integral_gain: i128,
    },
    SetMarketPriceOracle {
        oracle_account_id: AccountId,
        oracle_program_id: AccountId,
    },
    SetTimingParameters {
        minimum_milliseconds_between_rate_updates: u64,
        maximum_oracle_price_age_milliseconds: u64,
    },
    SetAdmin {
        new_admin_account_id: AccountId,
    },
    SetFreezeAuthority {
        new_freeze_authority_account_id: AccountId,
    },
    SetStabilityFeePerMillisecond {
        new_rate: u128,
    },
    Freeze,
    Unfreeze,
    ResetRedemptionRateController,
    SetDebtLimits {
        minimum_debt_amount: u128,
        debt_ceiling: u128,
    },
}

/// Messages only stablecoin actors send each other. The guest decodes these only when the
/// sender is the stablecoin program itself, so each is trusted because only stablecoin code
/// can have produced it.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[expect(
    clippy::large_enum_variant,
    reason = "decoded once per message; boxing `InitializeArgs` would not change the wire format but adds noise"
)]
pub enum Internal {
    // Program account to surplus relay, which creates the token definition.
    CreateStablecoinToken {
        name: String,
        token_program_id: AccountId,
    },
    // Program account to engine.
    InitializeEngine(InitializeArgs),
    // Seat to engine.
    Admin(AdminAction),
    // Position to engine.
    OpenRequest {
        collateral_holding: AccountId,
        initial_collateral_amount: u128,
    },
    GenerateDebtRequest {
        amount: u128,
        to: AccountId,
        collateral_amount: u128,
        normalized_debt_amount: u128,
    },
    RepayDebtRequest {
        maximum_amount: u128,
        from_holding: AccountId,
        normalized_debt_amount: u128,
    },
    WithdrawRequest {
        amount: u128,
        to: AccountId,
        collateral_after: u128,
        normalized_debt_amount: u128,
    },
    // Engine to position.
    OpenApproved {
        token_program_id: AccountId,
        collateral_definition_id: AccountId,
        collateral_holding: AccountId,
        initial_collateral_amount: u128,
    },
    DebtGenerated {
        normalized_debt_delta: u128,
    },
    DebtRepaid {
        normalized_debt_delta: u128,
    },
    WithdrawApproved {
        amount: u128,
        to: AccountId,
    },
    // Engine to itself, right after it asks the oracle for the price: the oracle's reply, which
    // runs before this message, must have cleared `pending`.
    AssertOracleReplied,
}

// ──────────────────────────────────────────────────────────────────────────────
// Addresses
// ──────────────────────────────────────────────────────────────────────────────

/// The engine's PDA seed: `sha256("STABLECOIN__PROTOCOL_PARAMS_____")`.
#[must_use]
pub fn compute_engine_pda_seed() -> PdaSeed {
    hash_seed(&[&ENGINE_PDA_DOMAIN])
}

/// The engine's account.
#[must_use]
pub fn compute_engine_pda(program_id: AccountId) -> AccountId {
    AccountId::for_public_pda(&program_id, &compute_engine_pda_seed())
}

/// The seed of the stablecoin's token definition, its own mint authority.
#[must_use]
pub fn compute_stablecoin_definition_pda_seed() -> PdaSeed {
    hash_seed(&[&STABLECOIN_DEFINITION_PDA_DOMAIN])
}

#[must_use]
pub fn compute_stablecoin_definition_pda(program_id: AccountId) -> AccountId {
    AccountId::for_public_pda(&program_id, &compute_stablecoin_definition_pda_seed())
}

/// The seed of the surplus holding (the old "master holding"), which receives accrued fees.
#[must_use]
pub fn compute_surplus_holding_pda_seed() -> PdaSeed {
    hash_seed(&[&SURPLUS_HOLDING_PDA_DOMAIN])
}

#[must_use]
pub fn compute_surplus_holding_pda(program_id: AccountId) -> AccountId {
    AccountId::for_public_pda(&program_id, &compute_surplus_holding_pda_seed())
}

/// The seed of an owner's collateral vault.
///
/// Hash input: `owner_account_id (32 bytes) || POSITION_VAULT`.
#[must_use]
pub fn compute_position_vault_pda_seed(owner_account_id: AccountId) -> PdaSeed {
    hash_seed(&[&owner_account_id.to_bytes(), POSITION_VAULT_PDA_DOMAIN])
}

/// The account of an owner's collateral vault.
#[must_use]
pub fn compute_position_vault_pda(program_id: AccountId, owner_account_id: AccountId) -> AccountId {
    AccountId::for_public_pda(
        &program_id,
        &compute_position_vault_pda_seed(owner_account_id),
    )
}

fn hash_seed(parts: &[&[u8]]) -> PdaSeed {
    let bytes = parts.concat();
    PdaSeed::new(
        *Impl::hash_bytes(&bytes)
            .as_bytes()
            .first_chunk::<32>()
            .expect("a sha256 digest has 32 bytes"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_hex(seed: &PdaSeed) -> String {
        seed.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn initialize_message_encodes_like_its_args_after_the_tag() {
        let id = |n: u8| AccountId::new([n; 32]);
        let args = InitializeArgs {
            initial_admin_account_id: id(1),
            freeze_authority_account_id: id(2),
            token_program_id: id(3),
            collateral_definition_id: id(4),
            market_price_oracle_id: id(5),
            market_price_oracle_program_id: id(6),
            initial_stability_fee_per_millisecond: 7,
            initial_controller_proportional_gain: -8,
            initial_controller_integral_gain: 9,
            initial_minimum_collateralization_ratio: 10,
            minimum_milliseconds_between_rate_updates: 11,
            maximum_oracle_price_age_milliseconds: 12,
            initial_redemption_price: 13,
            stablecoin_name: "Stable".to_owned(),
            minimum_debt_amount: 14,
            debt_ceiling: 15,
        };
        let mut expected = vec![0_u8];
        expected.extend(borsh::to_vec(&args).expect("args encode"));
        let message = Message::from(args);
        assert_eq!(borsh::to_vec(&message).expect("message encodes"), expected);
    }

    #[test]
    fn empty_bytes_decode_as_no_state() {
        assert_eq!(StablecoinState::decode(&[]), None);
    }

    #[test]
    fn a_position_round_trips() {
        let state = StablecoinState::Position(Position {
            token_program_id: AccountId::new([7; 32]),
            collateral_definition_id: AccountId::new([8; 32]),
            collateral_amount: 500,
            normalized_debt_amount: 20,
        });
        assert_eq!(StablecoinState::decode(&state.encode()), Some(state));
    }

    /// Distinct values for every `ProtocolParameters` field.
    fn distinct_parameters() -> ProtocolParameters {
        let id = |n: u8| AccountId::new([n; 32]);
        ProtocolParameters {
            admin_account_id: id(1),
            freeze_authority_account_id: id(2),
            token_program_id: id(3),
            stablecoin_definition_id: id(4),
            collateral_definition_id: id(5),
            market_price_oracle_id: id(6),
            market_price_oracle_program_id: id(7),
            stability_fee_per_millisecond: 8,
            controller_proportional_gain: -9,
            controller_integral_gain: 10,
            minimum_collateralization_ratio: 11,
            minimum_milliseconds_between_rate_updates: 12,
            maximum_oracle_price_age_milliseconds: 13,
            is_frozen: true,
            minimum_debt_amount: 14,
            debt_ceiling: 15,
        }
    }

    fn distinct_engine(pending: Option<Pending>) -> Engine {
        Engine {
            parameters: distinct_parameters(),
            fee: StabilityFeeAccumulator {
                accumulated_rate_at_last_accrual: 16,
                last_accrued_at: 17,
                total_normalized_debt: 18,
            },
            redemption: RedemptionPriceState {
                redemption_price_at_last_update: 19,
                redemption_rate_per_millisecond: 20,
                controller_integral_term: -21,
                last_updated_at: 22,
            },
            pending,
        }
    }

    #[test]
    fn a_position_is_stored_as_tag_one_then_its_fields_in_order() {
        let state = StablecoinState::Position(Position {
            token_program_id: AccountId::new([1; 32]),
            collateral_definition_id: AccountId::new([2; 32]),
            collateral_amount: 3,
            normalized_debt_amount: 4,
        });
        let mut expected = vec![1_u8];
        expected.extend([1_u8; 32]);
        expected.extend([2_u8; 32]);
        expected.extend(3_u128.to_le_bytes());
        expected.extend(4_u128.to_le_bytes());
        assert_eq!(state.encode(), expected);
    }

    #[test]
    fn an_engine_is_stored_as_tag_zero_then_the_parameters_in_order() {
        let state = StablecoinState::Engine(distinct_engine(None));
        let mut expected = vec![0_u8];
        for n in 1_u8..=7 {
            expected.extend([n; 32]);
        }
        expected.extend(8_u128.to_le_bytes());
        expected.extend((-9_i128).to_le_bytes());
        expected.extend(10_i128.to_le_bytes());
        expected.extend(11_u128.to_le_bytes());
        expected.extend(12_u64.to_le_bytes());
        expected.extend(13_u64.to_le_bytes());
        expected.push(1);
        expected.extend(14_u128.to_le_bytes());
        expected.extend(15_u128.to_le_bytes());
        let encoded = state.encode();
        assert_eq!(encoded.get(..expected.len()), Some(expected.as_slice()));
    }

    #[test]
    fn an_engine_round_trips_with_and_without_a_pending_operation() {
        for pending in [
            None,
            Some(Pending::GenerateDebt {
                position: AccountId::new([23; 32]),
                amount: 24,
                to: AccountId::new([25; 32]),
                collateral_amount: 26,
                normalized_debt_amount: 27,
            }),
        ] {
            let state = StablecoinState::Engine(distinct_engine(pending));
            assert_eq!(StablecoinState::decode(&state.encode()), Some(state));
        }
    }

    #[test]
    #[should_panic(expected = "stablecoin state must decode")]
    fn garbage_bytes_panic() {
        let _ = StablecoinState::decode(&[0xFF, 1, 2]);
    }

    #[test]
    fn the_engine_seed_is_unchanged_from_v0_2_5() {
        // sha256(b"STABLECOIN__PROTOCOL_PARAMS_____")
        assert_eq!(
            seed_hex(&compute_engine_pda_seed()),
            "998dcdf7cba5f45df1e2296055d5f1f26e69ad24fc4feedc1f1a034c1539e0fd"
        );
    }

    #[test]
    fn the_definition_seed_is_unchanged_from_v0_2_5() {
        // sha256(b"STABLECOIN__DEFINITION__________")
        assert_eq!(
            seed_hex(&compute_stablecoin_definition_pda_seed()),
            "8251e63f0096a09bcf24b5eed3be2b8e385350826d5ef5d6e6a01554ec9e4aa0"
        );
    }

    #[test]
    fn the_surplus_holding_seed_is_unchanged_from_v0_2_5() {
        // sha256(b"STABLECOIN__MASTER_HOLDING______")
        assert_eq!(
            seed_hex(&compute_surplus_holding_pda_seed()),
            "432f53122869e896f19173a7709f366154aa4127efe54e2b57ece812891578f7"
        );
    }

    #[test]
    fn the_fixed_pdas_are_distinct() {
        let program = AccountId::new([11; 32]);
        let ids = [
            compute_engine_pda(program),
            compute_stablecoin_definition_pda(program),
            compute_surplus_holding_pda(program),
        ];
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[0], ids[2]);
        assert_ne!(ids[1], ids[2]);
    }

    #[test]
    fn vaults_differ_per_owner() {
        let program = AccountId::new([3; 32]);
        assert_ne!(
            compute_position_vault_pda(program, AccountId::new([1; 32])),
            compute_position_vault_pda(program, AccountId::new([2; 32])),
        );
    }

    #[test]
    fn internal_variants_keep_their_tags() {
        // Append-only: these tags are the wire format.
        assert_eq!(
            borsh::to_vec(&Internal::CreateStablecoinToken {
                name: String::new(),
                token_program_id: AccountId::new([0; 32]),
            })
            .unwrap()
            .first(),
            Some(&0)
        );
        assert_eq!(
            borsh::to_vec(&Internal::WithdrawApproved {
                amount: 1,
                to: AccountId::new([0; 32]),
            })
            .unwrap()
            .first(),
            Some(&10)
        );
        assert_eq!(
            borsh::to_vec(&Internal::AssertOracleReplied).unwrap(),
            vec![11]
        );
    }

    #[test]
    fn message_variants_keep_their_tags() {
        // Append-only: these tags are the wire format.
        assert_eq!(
            borsh::to_vec(&Message::AccrueStabilityFee).unwrap(),
            vec![1]
        );
        assert_eq!(borsh::to_vec(&Message::ClosePosition).unwrap(), vec![9]);
        assert_eq!(
            borsh::to_vec(&Message::ResetRedemptionRateController).unwrap(),
            vec![19]
        );
        assert_eq!(
            borsh::to_vec(&Message::SetDebtLimits {
                minimum_debt_amount: 1,
                debt_ceiling: 2
            })
            .unwrap()
            .first(),
            Some(&20)
        );
    }
}
