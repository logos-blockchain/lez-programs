//! Derive the allowance account to declare when submitting a faucet claim.

use std::{error::Error, str::FromStr};

use lee_core::account::AccountId;
use token_mint_authority_core::compute_mint_allowance_pda;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [faucet, token, recipient, definition] = args.as_slice() else {
        return Err("usage: faucet_allowance <faucet-program-id> <token-program-id> <recipient> <definition>".into());
    };
    let allowance = compute_mint_allowance_pda(
        AccountId::from_str(faucet).map_err(|_| "invalid faucet program id")?,
        AccountId::from_str(token).map_err(|_| "invalid token program id")?,
        AccountId::from_str(recipient).map_err(|_| "invalid recipient account id")?,
        AccountId::from_str(definition).map_err(|_| "invalid definition account id")?,
    );
    println!("base58: {allowance}");
    Ok(())
}
