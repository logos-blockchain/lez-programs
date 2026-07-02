use std::{env, ffi::OsString, fs, path::PathBuf};

use anyhow::{bail, Context, Result};
use risc0_binfmt::ProgramBinary;

fn main() -> Result<()> {
    let mut args = env::args_os();
    let command = args
        .next()
        .unwrap_or_else(|| OsString::from("risc0-packager"));
    let input = required_arg(&mut args, &command, "guest ELF")?;
    let output = required_arg(&mut args, &command, "output .bin")?;

    if args.next().is_some() {
        bail!(
            "usage: {} <guest-elf> <output-bin>",
            command.to_string_lossy()
        );
    }

    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let user_elf =
        fs::read(&input).with_context(|| format!("failed to read {}", input.display()))?;
    let kernel_elf = risc0_build::GuestOptions::default().kernel();
    let binary = ProgramBinary::new(&user_elf, &kernel_elf);

    fs::write(&output, binary.encode())
        .with_context(|| format!("failed to write {}", output.display()))?;

    Ok(())
}

fn required_arg(
    args: &mut impl Iterator<Item = OsString>,
    command: &OsString,
    name: &str,
) -> Result<PathBuf> {
    match args.next() {
        Some(value) => Ok(PathBuf::from(value)),
        None => bail!(
            "missing {name}; usage: {} <guest-elf> <output-bin>",
            command.to_string_lossy()
        ),
    }
}
