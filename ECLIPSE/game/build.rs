use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo should set CARGO_MANIFEST_DIR"),
    );
    let source = manifest_dir.join("world").join("world.cfg");
    let workspace_root = manifest_dir
        .parent()
        .expect("game manifest directory should have a workspace parent");
    let release_config = workspace_root
        .join("release")
        .join("Eclipse")
        .join("world.cfg");

    println!("cargo:rerun-if-changed={}", source.display());

    let contents = fs::read(&source)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", source.display()));
    let should_copy = match fs::read(&release_config) {
        Ok(existing) => existing != contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => panic!("could not read {}: {error}", release_config.display()),
    };

    if should_copy {
        if let Some(parent) = release_config.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("could not create {}: {error}", parent.display()));
        }
        fs::write(&release_config, contents).unwrap_or_else(|error| {
            panic!("could not write {}: {error}", release_config.display())
        });
    }
}
