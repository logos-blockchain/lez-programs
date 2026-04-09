use std::{path::PathBuf, process};

fn main() {
    let path: PathBuf = match std::env::args().nth(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("Usage: idl-gen <source-file>");
            process::exit(1);
        }
    };

    match spel_framework_core::idl_gen::generate_idl_from_file(&path) {
        Ok(idl) => println!("{}", serde_json::to_string_pretty(&idl).unwrap()),
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    }
}
