#![no_main]

use spel_framework::prelude::*;

risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "twap_oracle_core::Instruction")]
mod twap_oracle {
    #[allow(unused_imports)]
    use super::*;

    /// No-op instruction. Does nothing and returns no state changes.
    #[instruction]
    pub fn noop() -> SpelResult {
        Ok(spel_framework::SpelOutput::execute(twap_oracle_program::noop::noop(), vec![]))
    }
}
