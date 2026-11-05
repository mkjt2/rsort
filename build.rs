//! Turns `cli.toml` into static Rust (`$OUT_DIR/spec.rs`, included by
//! `src/spec/mod.rs`), so the binary carries the interface's text without a
//! TOML parser and without parsing anything at startup.

#[path = "src/spec/model.rs"]
mod model;

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=cli.toml");
    println!("cargo:rerun-if-changed=src/spec/model.rs");
    let text = std::fs::read_to_string("cli.toml").expect("read cli.toml");
    let spec = model::Spec::parse(&text).unwrap_or_else(|e| panic!("cli.toml: {e}"));
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is set");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    std::fs::write(out.join("spec.rs"), spec.to_rust(&version)).expect("write spec.rs");
}
