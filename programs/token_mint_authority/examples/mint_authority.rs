//! Derive the mint authority for a faucet and token program deployment.
//!
//! Set the token definition's `mint_authority` to the printed account ID
//! when sending `token_core::Message::NewDefinition`.

use std::{error::Error, str::FromStr};

use lee_core::account::AccountId;
use token_mint_authority_core::compute_mint_authority_pda;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [faucet, token] = args.as_slice() else {
        return Err("usage: mint_authority <faucet-program-id> <token-program-id>".into());
    };
    let authority = compute_mint_authority_pda(
        AccountId::from_str(faucet).map_err(|_| "invalid faucet program id")?,
        AccountId::from_str(token).map_err(|_| "invalid token program id")?,
    );
    println!("base58: {authority}");
    println!("hex:    {}", hex::encode(authority.to_bytes()));
    Ok(())
}
