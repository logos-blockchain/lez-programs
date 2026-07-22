use std::env;

fn main() {
    let Ok(target_os) = env::var("CARGO_CFG_TARGET_OS") else {
        return;
    };

    // RISC Zero's host-side serde dependency contains guest syscall shims with exported C names.
    // They are implementation details of this cdylib and would otherwise leak beside the three
    // supported amm_client_* entry points.
    if matches!(
        target_os.as_str(),
        "android" | "dragonfly" | "freebsd" | "linux" | "netbsd" | "openbsd"
    ) {
        println!("cargo:rustc-cdylib-link-arg=-Wl,--exclude-libs,ALL");
    }
}
