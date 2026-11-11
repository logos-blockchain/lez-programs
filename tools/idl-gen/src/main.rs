//! Generates a program's IDL from its core crate with spel's generator, the same output as
//! `spel generate-idl`, so CI can check `artifacts/` without installing spel.
//!
//! Usage: idl-gen <CRATE_DIR> --message <ENUM> [--state <TYPE>] [--events <MODULE>]

use std::{path::PathBuf, process};

use spel_framework_core::idl_gen::{generate_idl, GenOptions};

const USAGE: &str =
    "Usage: idl-gen <CRATE_DIR> --message <ENUM> [--state <TYPE>] [--events <MODULE>]";

fn parse_args(args: &[String]) -> Result<GenOptions, String> {
    let mut options = GenOptions::default();
    let mut crate_dir = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |flag: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "--message" => options.message = value("--message")?,
            "--state" => options.state = Some(value("--state")?),
            "--events" => options.events = Some(value("--events")?),
            flag if flag.starts_with('-') => return Err(format!("unknown option '{flag}'")),
            dir if crate_dir.is_none() => crate_dir = Some(PathBuf::from(dir)),
            extra => return Err(format!("unexpected argument '{extra}'")),
        }
    }
    options.crate_dir = crate_dir.ok_or("missing <CRATE_DIR>")?;
    if options.message.is_empty() {
        return Err("missing --message <ENUM>".into());
    }
    Ok(options)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = parse_args(&args).unwrap_or_else(|e| {
        eprintln!("error: {e}\n{USAGE}");
        process::exit(1);
    });
    let idl = generate_idl(&options, |w| eprintln!("warning: {w}")).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });
    match idl.to_json_pretty() {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}
