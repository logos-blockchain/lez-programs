//! Build script that embeds the stablecoin RISC Zero guest ELF as host-side constants.
fn main() {
    risc0_build::embed_methods();
}
