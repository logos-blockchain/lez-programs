//! Print the mint-authority account id for a deployed token-mint-authority program.
//!
//! Every faucet token's `mint_authority` (set at `NewFungibleDefinition` time) must be this
//! program's singleton mint-authority PDA.
//!
//! Since LEZ v0.2.5 a program is addressed by the account id of its deployed `ProgramHeader`,
//! not by its ImageID, and PDAs derive from that account id. Header addresses are chosen by
//! whoever deploys, so this takes the deployed program's account id as its argument — it can
//! no longer be computed from a `.bin`. Re-run it whenever the program is deployed to a new
//! address; a rebuild alone no longer changes the PDA.
//!
//! Usage:
//!   cargo run -p token_mint_authority_program --example mint_authority -- \
//!     <token-mint-authority-program-account-id-base58>

use std::{error::Error, str::FromStr};

use lee_core::account::AccountId;
use token_mint_authority_core::compute_mint_authority_pda;

fn main() -> Result<(), Box<dyn Error>> {
    let arg = std::env::args().nth(1).ok_or(
        "usage: cargo run -p token_mint_authority_program --example mint_authority -- \
         <token-mint-authority-program-account-id-base58>",
    )?;
    let program_account_id =
        AccountId::from_str(&arg).map_err(|_| "program account id must be base58")?;

    let authority = compute_mint_authority_pda(program_account_id);

    println!("program account id: {program_account_id}");
    println!();
    println!("Set this as `mint_authority` on every faucet token definition:");
    println!("  base58: {authority}");
    println!("  hex:    {}", hex::encode(authority.to_bytes()));

    Ok(())
}
