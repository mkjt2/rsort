//! `schema`: the machine-readable interface, generated from `cli.toml`.

use crate::ctx::Ctx;
use crate::error::AppError;
use crate::spec::Spec;
use serde_json::{Value, json};

/// Build the schema document for `spec`.
pub fn document(spec: &Spec) -> Value {
    let commands: Vec<Value> = spec
        .commands
        .iter()
        .map(|c| {
            let mut v = serde_json::to_value(c).expect("command serializes");
            v["usage"] = json!(format!("{} {}", spec.cli.name, c.path));
            v
        })
        .collect();
    json!({
        "schema_version": spec.schema_version,
        "name": spec.cli.name,
        "version": env!("CARGO_PKG_VERSION"),
        "about": spec.cli.about,
        "shape": if spec.is_single() { "single" } else { "subcommands" },
        "global_flags": spec.flags,
        "args": spec.args,
        "commands": commands,
        "exit_codes": spec.all_exit_codes(),
        "env": spec.env,
        "config_keys": spec.config_keys,
        "error_schema": {
            "description": "With --json, failures write one line to stderr with this shape",
            "type": "object",
            "required": ["error"],
            "properties": {
                "error": {
                    "type": "object",
                    "required": ["code", "exit_code", "message"],
                    "properties": {
                        "code": { "type": "string" },
                        "exit_code": { "type": "integer" },
                        "message": { "type": "string" },
                        "hint": { "type": "string" }
                    }
                }
            }
        }
    })
}

pub fn run(ctx: &mut Ctx) -> Result<(), AppError> {
    ctx.emit_json(&document(Spec::get()))
}
