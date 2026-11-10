fn main() {
    println!("cargo:rerun-if-changed=src/oracle.rs");
    risc0_build::embed_methods();
}
