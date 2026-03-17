---
description: Get the program ID (Image ID) for a LEZ program. Use when the user asks for a program's ID, image ID, or program address (e.g. "what's the token program id", "get the amm program id").
---

# program-id

The program ID is the RISC Zero Image ID derived from the compiled guest ELF binary.

The program name corresponds to a top-level workspace directory. If none is specified, discover
available programs by looking for `<name>/methods/guest/Cargo.toml` and ask the user to pick one.

## Steps

1. **Check if the binary exists** at `<name>/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/<name>.bin`.
2. **If missing, build it first** using `cargo risczero build --manifest-path <name>/methods/guest/Cargo.toml`.
   - Docker must be running for this step. Fail fast if not.
3. **Inspect the binary** with `spel-cli inspect <path-to-binary>` and report the program ID to the user.
