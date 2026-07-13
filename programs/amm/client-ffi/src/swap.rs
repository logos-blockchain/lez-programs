use amm_core::Instruction;

/// Build the RISC0 instruction words for a SwapExactInput, via the exact
/// serializer the public-transaction path and the guest decoder share.
pub fn swap_exact_input_words(
    amount_in: u128,
    min_out: u128,
    deadline: u64,
) -> Result<Vec<u32>, String> {
    let instruction = Instruction::SwapExactInput {
        swap_amount_in: amount_in,
        min_amount_out: min_out,
        deadline,
    };
    risc0_zkvm::serde::to_vec(&instruction).map_err(|e| format!("{e:?}"))
}

#[cfg(test)]
mod tests {
    use amm_core::Instruction;

    use super::*;

    #[test]
    fn words_roundtrip_to_same_instruction() {
        let words = swap_exact_input_words(1000, 1, u64::MAX).unwrap();
        // Deserialize back through the exact serde the guest decoder uses.
        // `Instruction` derives neither `Debug` nor `PartialEq`, so assert
        // field-by-field instead of matching with a `{:?}` fallback arm.
        let decoded: Instruction = risc0_zkvm::serde::from_slice(&words).unwrap();
        match decoded {
            Instruction::SwapExactInput {
                swap_amount_in,
                min_amount_out,
                deadline,
            } => {
                assert_eq!(swap_amount_in, 1000);
                assert_eq!(min_amount_out, 1);
                assert_eq!(deadline, u64::MAX);
            }
            Instruction::Initialize { .. }
            | Instruction::UpdateConfig { .. }
            | Instruction::CreatePriceObservations { .. }
            | Instruction::CreateOraclePriceAccount { .. }
            | Instruction::NewDefinition { .. }
            | Instruction::AddLiquidity { .. }
            | Instruction::RemoveLiquidity { .. }
            | Instruction::SwapExactOutput { .. }
            | Instruction::SyncReserves => panic!("wrong variant: expected SwapExactInput"),
        }
    }
}
