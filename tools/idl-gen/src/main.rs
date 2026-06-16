use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

fn main() {
    let path: PathBuf = match std::env::args().nth(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("Usage: idl-gen <source-file>");
            process::exit(1);
        }
    };

    let dep_dirs = find_path_dep_dirs(&path);

    match spel_framework_core::idl_gen::generate_idl_from_file_with_deps(&path, &dep_dirs) {
        Ok(idl) => {
            // spel-framework emits the top-level `types` array in HashMap
            // iteration order, which is non-deterministic across processes.
            // Sort it by name so regenerated IDL is byte-stable regardless of
            // where it runs (local `make idl` vs CI vs another contributor).
            let mut value = match serde_json::to_value(&idl) {
                Ok(value) => value,
                Err(e) => {
                    eprintln!("Error converting IDL to JSON value: {e}");
                    process::exit(1);
                }
            };
            if let Some(types) = value.get_mut("types").and_then(|t| t.as_array_mut()) {
                types.sort_by(|a, b| {
                    let name = |v: &serde_json::Value| {
                        v.get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_owned()
                    };
                    name(a).cmp(&name(b))
                });
            }
            match serde_json::to_string_pretty(&value) {
                Ok(json) => println!("{json}"),
                Err(e) => {
                    eprintln!("Error serializing IDL to JSON: {e}");
                    process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    }
}

/// Return the crate-root directories of all `path = "..."` entries in the
/// `[dependencies]` table of the `Cargo.toml` nearest to `source_path`.
fn find_path_dep_dirs(source_path: &Path) -> Vec<PathBuf> {
    (|| -> Option<Vec<PathBuf>> {
        let manifest = find_crate_manifest(source_path)?;
        let content = fs::read_to_string(&manifest).ok()?;
        let value: toml::Value = toml::from_str(&content).ok()?;
        let manifest_dir = manifest.parent()?;

        let mut dirs = Vec::new();
        if let Some(table) = value.get("dependencies").and_then(|v| v.as_table()) {
            for (_name, dep) in table {
                if let Some(rel) = dep.get("path").and_then(|v| v.as_str()) {
                    let dep_dir = manifest_dir.join(rel);
                    if dep_dir.is_dir() {
                        dirs.push(dep_dir);
                    }
                }
            }
        }
        Some(dirs)
    })()
    .unwrap_or_default()
}

/// Walk up from `start` to find the nearest `Cargo.toml`.
fn find_crate_manifest(start: &Path) -> Option<PathBuf> {
    let mut dir: &Path = if start.is_file() {
        start.parent()?
    } else {
        start
    };
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}
