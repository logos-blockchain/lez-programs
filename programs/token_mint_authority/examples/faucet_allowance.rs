//! Print the mint-allowance PDA for a `(recipient, token definition)` pair.
//!
//! `FaucetMint` rate-limits per `(recipient, definition)` by claiming a
//! [`MintAllowance`] account at [`compute_mint_allowance_pda`]. That address is a
//! required input to the instruction, so anything that submits a faucet mint (the
//! testnet setup script, an e2e test) needs to derive it up front. Like the
//! mint-authority PDA it depends on the deployed faucet binary's ImageID, so run
//! this against the *exact* `.bin` you deploy.
//!
//! Usage:
//!   cargo run -p token_mint_authority_program --example faucet_allowance -- \
//!     <path-to-token_mint_authority.bin> <recipient-base58> <definition-base58>

use std::{error::Error, str::FromStr};

use lee_core::{account::AccountId, program::ProgramId};
use risc0_binfmt::ProgramBinary;
use token_mint_authority_core::compute_mint_allowance_pda;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: cargo run -p token_mint_authority_program --example faucet_allowance -- \
         <token_mint_authority.bin> <recipient-base58> <definition-base58>";
    let path = args.next().ok_or(usage)?;
    let recipient_s = args.next().ok_or(usage)?;
    let definition_s = args.next().ok_or(usage)?;

    let bytes = std::fs::read(&path).map_err(|error| format!("failed to read {path}: {error}"))?;
    let binary = ProgramBinary::decode(&bytes)
        .map_err(|error| format!("failed to decode program binary: {error}"))?;
    let program_id: ProgramId = binary
        .compute_image_id()
        .map_err(|error| format!("failed to compute image id: {error}"))?
        .into();

    let recipient =
        AccountId::from_str(&recipient_s).map_err(|_| "recipient must be a base58 account id")?;
    let definition =
        AccountId::from_str(&definition_s).map_err(|_| "definition must be a base58 account id")?;

    let allowance = compute_mint_allowance_pda(program_id, recipient, definition);

    // Print only the base58 id on the last line so callers can grep it out easily.
    println!("recipient:        {recipient}");
    println!("definition:       {definition}");
    println!("base58:           {allowance}");

    Ok(())
}
