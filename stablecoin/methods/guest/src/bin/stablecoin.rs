#![no_main]

use nssa_core::account::AccountWithMetadata;
use spel_framework::prelude::*;

risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "stablecoin_core::Instruction")]
mod stablecoin {
    #[allow(unused_imports)]
    use super::*;

    #[instruction]
    pub fn noop(account: AccountWithMetadata) -> SpelResult {
        #[allow(deprecated)]
        Ok(SpelOutput::states_only(stablecoin_program::noop::noop(
            account,
        )))
    }
}
