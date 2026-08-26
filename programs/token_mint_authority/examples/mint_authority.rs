//! Print the mint-authority account id for a built token-mint-authority guest binary.
//!
//! Every faucet token's `mint_authority` (set at `NewFungibleDefinition` time)
//! must be this program's singleton mint-authority PDA, which is derived from the
//! deployed binary's ImageID. This helper decodes the `.bin`, computes that
//! ImageID, and prints the resulting account id in base58 and hex so it can be
//! dropped straight into a token definition.
//!
//! Because the id depends on the ImageID, run this against the *exact* binary you
//! deploy (build with the release profile first — see the deployment notes).
//!
//! Usage:
//!   make build-programs   # produces target/guest/token_mint_authority.bin
//!   cargo run -p token_mint_authority_program --example mint_authority -- \
//!     target/guest/token_mint_authority.bin

use std::error::Error;

use lee_core::program::ProgramId;
use risc0_binfmt::ProgramBinary;
use token_mint_authority_core::compute_mint_authority_pda;

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or(
        "usage: cargo run -p token_mint_authority_program --example mint_authority -- \
         <path-to-token_mint_authority.bin>",
    )?;

    let bytes = std::fs::read(&path).map_err(|error| format!("failed to read {path}: {error}"))?;
    let binary = ProgramBinary::decode(&bytes)
        .map_err(|error| format!("failed to decode program binary: {error}"))?;
    let program_id: ProgramId = binary
        .compute_image_id()
        .map_err(|error| format!("failed to compute image id: {error}"))?
        .into();

    let authority = compute_mint_authority_pda(program_id);

    println!("program binary:   {path}");
    println!(
        "program id (hex): {}",
        hex::encode(program_id_bytes(program_id))
    );
    println!();
    println!("Set this as `mint_authority` on every faucet token definition:");
    println!("  base58: {authority}");
    println!("  hex:    {}", hex::encode(authority.to_bytes()));

    Ok(())
}

/// The 8×u32 ImageID as its 32-byte little-endian form (the ProgramId `spel inspect` reports).
fn program_id_bytes(program_id: ProgramId) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    for (chunk, word) in bytes.chunks_exact_mut(4).zip(program_id) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    bytes
}
