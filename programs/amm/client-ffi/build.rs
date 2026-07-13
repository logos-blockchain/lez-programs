fn main() {
    let crate_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    cbindgen::generate(&crate_dir)
        .expect("cbindgen")
        .write_to_file(format!("{crate_dir}/amm_client_ffi.h"));
    println!("cargo:rerun-if-changed=src/lib.rs");
}
