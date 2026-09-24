//! The deployment script adapts i128 arguments for the pinned SPEL CLI, which
//! supports u128 but not i128. Verify the entire payload against the native ABI.

use lee_core::account::AccountId;
use risc0_zkvm::serde::to_vec;
use stablecoin_core::{math::FIXED_POINT_ONE, Instruction};

#[test]
fn unsigned_gain_transport_matches_initialize_program_abi() {
    for (proportional, integral) in [
        (0_i128, 0_i128),
        (-123_456_789_012_345_678_901_234_567, 42),
        (10_i128.pow(30), -10_i128.pow(27)),
        (-10_i128.pow(30), 10_i128.pow(27)),
    ] {
        let freeze_authority = AccountId::new([0x24; 32]);
        let ratio = FIXED_POINT_ONE * 3 / 2;
        let name = "Test 'stablecoin' 🪙";
        let native = Instruction::InitializeProgram {
            freeze_authority_account_id: freeze_authority,
            initial_stability_fee_per_millisecond: FIXED_POINT_ONE,
            initial_controller_proportional_gain: proportional,
            initial_controller_integral_gain: integral,
            initial_minimum_collateralization_ratio: ratio,
            minimum_milliseconds_between_rate_updates: 300_000,
            maximum_oracle_price_age_milliseconds: 900_000,
            initial_redemption_price: u128::MAX,
            stablecoin_name: name.to_owned(),
        };
        // SPEL serializes the IDL instruction index followed by its fields.
        // RISC Zero's i128 serializer writes exactly the same bits as u128.
        let transported = (
            0_u32,
            freeze_authority.to_string(),
            FIXED_POINT_ONE,
            u128::from_le_bytes(proportional.to_le_bytes()),
            u128::from_le_bytes(integral.to_le_bytes()),
            ratio,
            300_000_u64,
            900_000_u64,
            u128::MAX,
            name,
        );
        assert_eq!(
            to_vec(&transported).expect("serialize CLI transport"),
            to_vec(&native).expect("serialize native instruction"),
            "gain transport changed the instruction for Kp={proportional}, Ki={integral}",
        );
    }
}
