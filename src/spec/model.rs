//! Typed view of `cli.toml`, the source of truth for this CLI's interface.
//!
//! This file is not part of the binary. `build.rs` compiles it to turn
//! `cli.toml` into static Rust (`Spec::to_rust`), which `src/spec/mod.rs`
//! includes, and the tests compile it to check the code against `cli.toml`
//! (`tests/conformance.rs`, `tests/cases.rs`). So nothing parses TOML at run
//! time.
#![allow(dead_code)] // build.rs and each test use a different part

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Exit codes every generated CLI uses unless `cli.toml` redefines them.
/// `error` moves to `[cli].error_exit_code` when that is set.
pub const BUILTIN_EXIT_CODES: &[(i32, &str, &str)] = &[
    (0, "ok", "Success"),
    (1, "error", "General error"),
    (2, "usage", "Invalid arguments or usage"),
    (130, "interrupted", "Interrupted (Ctrl-C)"),
];

/// Usage errors always exit with this code.
pub const USAGE_EXIT_CODE: i32 = 2;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub schema_version: u32,
    pub cli: CliInfo,
    #[serde(default)]
    pub project: Project,
    #[serde(default)]
    pub kit: Kit,
    #[serde(default, rename = "exit_code")]
    pub exit_codes: Vec<ExitCode>,
    #[serde(default)]
    pub env: Vec<EnvVar>,
    #[serde(default, rename = "config_key")]
    pub config_keys: Vec<ConfigKey>,
    #[serde(default, rename = "flag")]
    pub flags: Vec<Flag>,
    #[serde(default, rename = "arg")]
    pub args: Vec<Arg>,
    #[serde(default, rename = "command")]
    pub commands: Vec<Command>,
    #[serde(default, rename = "case")]
    pub cases: Vec<Case>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CliInfo {
    pub name: String,
    pub about: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_about: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<Example>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Flags that print help, e.g. `["-h", "--help"]` (the default) or `["--help"]`.
    #[serde(default = "default_help_flags")]
    pub help_flags: Vec<String>,
    /// Flags that print the version; `[]` for none.
    #[serde(default = "default_version_flags")]
    pub version_flags: Vec<String>,
    /// How errors are shown to humans.
    #[serde(default)]
    pub error_style: ErrorStyle,
    /// Exit code for general errors (default 1). Usage errors always exit 2.
    #[serde(default = "default_error_exit_code")]
    pub error_exit_code: i32,
}

fn default_help_flags() -> Vec<String> {
    vec!["-h".into(), "--help".into()]
}

fn default_version_flags() -> Vec<String> {
    vec!["-V".into(), "--version".into()]
}

fn default_error_exit_code() -> i32 {
    1
}

/// Human error rendering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorStyle {
    /// miette diagnostics: code, message and hint.
    #[default]
    Rich,
    /// `<name>: <message>`, plus a `hint:` line when there is one.
    Plain,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub homebrew_tap: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Kit {
    #[serde(default = "yes")]
    pub config: bool,
    #[serde(default)]
    pub self_update: bool,
    #[serde(default = "yes")]
    pub json: bool,
    #[serde(default = "yes")]
    pub color: bool,
    #[serde(default = "yes")]
    pub prompts: bool,
    #[serde(default = "yes")]
    pub verbosity: bool,
}

fn yes() -> bool {
    true
}

impl Default for Kit {
    fn default() -> Self {
        Kit {
            config: true,
            self_update: false,
            json: true,
            color: true,
            prompts: true,
            verbosity: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExitCode {
    pub code: i32,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvVar {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigKey {
    pub key: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub possible_values: Vec<String>,
}

/// How an option-style argument takes values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagKind {
    /// Boolean presence flag (`--force`).
    Switch,
    /// Repeatable counter (`-vvv`).
    Count,
    /// Takes one value (`--out FILE`).
    Value,
    /// Takes a value and may repeat (`--tag a --tag b`).
    Values,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Flag {
    pub id: String,
    pub long: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<char>,
    pub kind: FlagKind,
    pub help: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_name: Option<String>,
    #[serde(default)]
    pub global: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub possible_values: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts_with: Vec<String>,
    /// Value used when the flag is given without a value (`--color` = `--color=always`).
    /// Makes the value optional; it must then be attached with `=`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_missing: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub short_aliases: Vec<char>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Arg {
    pub id: String,
    pub help: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_name: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub possible_values: Vec<String>,
}

/// Whether a command honours `--json`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonMode {
    /// Human output by default, JSON with `--json`.
    #[default]
    Supported,
    /// Always emits JSON.
    Always,
    /// Output is not data (scripts, man pages); `--json` only affects errors.
    Never,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Example {
    pub cmd: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub path: String,
    pub about: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_about: Option<String>,
    #[serde(default)]
    pub json: JsonMode,
    #[serde(default)]
    pub mutates: bool,
    #[serde(default)]
    pub stdin: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exit_codes: Vec<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<Example>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(default, rename = "arg", skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<Arg>,
    #[serde(default, rename = "flag", skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<Flag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<toml::Table>,
}

/// An executable example: run the CLI with `args` (and `stdin`/`files`/`env`) and
/// expect `stdout` (or the contents of `tests/cases/<stdout_file>`) and `exit_code`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
    /// Files created in the working directory before running, by relative path.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub files: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_contains: Option<String>,
    #[serde(default)]
    pub exit_code: i32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl Spec {
    /// Parse a spec from TOML text.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// This crate's `cli.toml`, for tests.
    #[cfg(test)]
    pub fn get() -> &'static Spec {
        static SPEC: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| {
            Spec::parse(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/cli.toml"
            )))
            .expect("cli.toml is valid")
        })
    }

    /// Look up a command by its space-separated path.
    pub fn command(&self, path: &str) -> Option<&Command> {
        self.commands.iter().find(|c| c.path == path)
    }

    /// True for a single command with operands (no `[[command]]` entries).
    pub fn is_single(&self) -> bool {
        self.commands.is_empty()
    }

    /// Exit codes as documented: built-ins (unless redefined by a declared code, or
    /// `error` moved by `error_exit_code`) plus declared codes, sorted by code.
    pub fn all_exit_codes(&self) -> Vec<ExitCode> {
        let declared = |code: i32| self.exit_codes.iter().any(|c| c.code == code);
        let mut codes: Vec<ExitCode> = BUILTIN_EXIT_CODES
            .iter()
            .filter(|(code, name, _)| {
                !declared(*code) && !(*name == "error" && self.cli.error_exit_code != *code)
            })
            .map(|(code, name, description)| ExitCode {
                code: *code,
                name: (*name).to_string(),
                description: (*description).to_string(),
            })
            .chain(self.exit_codes.iter().cloned())
            .collect();
        codes.sort_by_key(|c| c.code);
        codes
    }

    /// (name, code) pairs `src/error.rs` must produce: usage, error and every
    /// declared code.
    pub fn error_catalog(&self) -> std::collections::BTreeSet<(String, i32)> {
        let mut set: std::collections::BTreeSet<(String, i32)> = self
            .exit_codes
            .iter()
            .map(|c| (c.name.clone(), c.code))
            .collect();
        set.insert(("usage".into(), USAGE_EXIT_CODE));
        set.insert(("error".into(), self.cli.error_exit_code));
        set
    }

    /// Semantic checks beyond what serde enforces. Returns every problem found.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.schema_version != 1 {
            problems.push(format!(
                "unsupported schema_version {}",
                self.schema_version
            ));
        }
        if self.cli.about.trim().is_empty() {
            problems.push("cli.about must not be empty".into());
        }
        let mut seen_codes = std::collections::BTreeSet::new();
        for code in self.all_exit_codes() {
            if !seen_codes.insert(code.code) {
                problems.push(format!("exit code {} declared twice", code.code));
            }
        }
        for code in &self.exit_codes {
            if code.code < 1 || code.code > 125 {
                problems.push(format!(
                    "exit code {} ({}) must be in 1..=125",
                    code.code, code.name
                ));
            }
        }
        let mut paths = std::collections::BTreeSet::new();
        for cmd in &self.commands {
            if !paths.insert(cmd.path.as_str()) {
                problems.push(format!("command `{}` declared twice", cmd.path));
            }
            if cmd.about.trim().is_empty() {
                problems.push(format!("command `{}` has an empty about", cmd.path));
            }
            if let Some((parent, _)) = cmd.path.rsplit_once(' ')
                && self.command(parent).is_none()
            {
                problems.push(format!(
                    "command `{}` has no parent entry `{parent}`",
                    cmd.path
                ));
            }
            for code in &cmd.exit_codes {
                if !seen_codes.contains(code) {
                    problems.push(format!(
                        "command `{}` lists undeclared exit code {code}",
                        cmd.path
                    ));
                }
            }
            if cmd.mutates && !cmd.flags.iter().any(|f| f.id == "dry_run") {
                problems.push(format!(
                    "mutating command `{}` must have a --dry-run flag",
                    cmd.path
                ));
            }
            for arg in &cmd.args {
                if arg.help.trim().is_empty() {
                    problems.push(format!("arg `{}` of `{}` has empty help", arg.id, cmd.path));
                }
            }
            for flag in &cmd.flags {
                check_flag(&mut problems, flag, &cmd.path);
            }
        }
        for flag in &self.flags {
            check_flag(&mut problems, flag, "<root>");
        }
        if !self.args.is_empty() && !self.commands.is_empty() {
            problems.push("root [[arg]]s and [[command]]s cannot be mixed".into());
        }
        if !seen_codes.contains(&self.cli.error_exit_code) {
            problems.push(format!(
                "error_exit_code {} is not a documented exit code",
                self.cli.error_exit_code
            ));
        }
        let mut case_names = std::collections::BTreeSet::new();
        for case in &self.cases {
            if !case_names.insert(case.name.as_str()) {
                problems.push(format!("case `{}` declared twice", case.name));
            }
            if case.stdout.is_some() && case.stdout_file.is_some() {
                problems.push(format!(
                    "case `{}` sets both stdout and stdout_file",
                    case.name
                ));
            }
            if case.exit_code != 0 && !seen_codes.contains(&case.exit_code) {
                problems.push(format!(
                    "case `{}` expects undeclared exit code {}",
                    case.name, case.exit_code
                ));
            }
        }
        problems
    }
}

fn check_flag(problems: &mut Vec<String>, flag: &Flag, owner: &str) {
    if flag.help.trim().is_empty() {
        problems.push(format!("flag `--{}` of {owner} has empty help", flag.long));
    }
    let takes_value = matches!(flag.kind, FlagKind::Value | FlagKind::Values);
    if !takes_value
        && (flag.value_name.is_some()
            || !flag.possible_values.is_empty()
            || flag.default_missing.is_some())
    {
        problems.push(format!(
            "flag `--{}` of {owner} takes no value but sets value_name/possible_values/default_missing",
            flag.long
        ));
    }
}

// Build-time rendering: everything the binary needs from cli.toml, as text.

impl Spec {
    /// The `--help` footer: examples, exit codes and a pointer to `schema`.
    pub fn root_after_help(&self) -> String {
        let mut out = String::new();
        if !self.cli.examples.is_empty() {
            out.push_str(&examples_section(&self.cli.examples));
            out.push_str("\n\n");
        }
        out.push_str("Exit codes:\n");
        for code in self.all_exit_codes() {
            out.push_str(&format!("  {:>3}  {}\n", code.code, code.description));
        }
        if self.command("schema").is_some() {
            out.push_str(&format!(
                "\nRun `{} schema` for the full machine-readable interface.",
                self.cli.name
            ));
        }
        out.trim_end().to_string()
    }

    /// The machine-readable interface printed by `--generate schema`.
    pub fn schema_document(&self, version: &str) -> Value {
        let commands: Vec<Value> = self
            .commands
            .iter()
            .map(|c| {
                let mut v = serde_json::to_value(c).expect("command serializes");
                v["usage"] = json!(format!("{} {}", self.cli.name, c.path));
                v
            })
            .collect();
        json!({
            "schema_version": self.schema_version,
            "name": self.cli.name,
            "version": version,
            "about": self.cli.about,
            "shape": if self.is_single() { "single" } else { "subcommands" },
            "global_flags": self.flags,
            "args": self.args,
            "commands": commands,
            "exit_codes": self.all_exit_codes(),
            "env": self.env,
            "config_keys": self.config_keys,
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

    /// Rust source defining `SPEC` and `SCHEMA` for `src/spec/mod.rs`.
    pub fn to_rust(&self, version: &str) -> String {
        let root_help: Vec<(&str, &str)> = self
            .flags
            .iter()
            .map(|f| (f.id.as_str(), f.help.as_str()))
            .chain(self.args.iter().map(|a| (a.id.as_str(), a.help.as_str())))
            .collect();
        let commands: Vec<String> = self
            .commands
            .iter()
            .map(|c| {
                let help: Vec<(&str, &str)> = c
                    .args
                    .iter()
                    .map(|a| (a.id.as_str(), a.help.as_str()))
                    .chain(c.flags.iter().map(|f| (f.id.as_str(), f.help.as_str())))
                    .collect();
                let after_help = (!c.examples.is_empty()).then(|| examples_section(&c.examples));
                format!(
                    "CommandText {{ path: {:?}, about: {:?}, long_about: {}, after_help: {}, help: {} }}",
                    c.path,
                    c.about,
                    option(c.long_about.as_deref()),
                    option(after_help.as_deref()),
                    pairs(&help),
                )
            })
            .collect();
        let style = match self.cli.error_style {
            ErrorStyle::Rich => "Rich",
            ErrorStyle::Plain => "Plain",
        };
        let schema = self.schema_document(version).to_string();
        format!(
            "// Generated by build.rs from cli.toml. Do not edit.\n\
             pub static SPEC: Interface = Interface {{\n\
             \x20   name: {name:?},\n\
             \x20   about: {about:?},\n\
             \x20   long_about: {long_about},\n\
             \x20   after_long_help: {after:?},\n\
             \x20   help_flags: {help_flags},\n\
             \x20   version_flags: {version_flags},\n\
             \x20   help: {help},\n\
             \x20   commands: &[{commands}],\n\
             \x20   error_style: ErrorStyle::{style},\n\
             \x20   error_exit_code: {code},\n\
             \x20   json: {json},\n\
             }};\n\
             pub static SCHEMA: &str = {schema:?};\n",
            name = self.cli.name,
            about = self.cli.about,
            long_about = option(self.cli.long_about.as_deref()),
            after = self.root_after_help(),
            help_flags = strs(&self.cli.help_flags),
            version_flags = strs(&self.cli.version_flags),
            help = pairs(&root_help),
            commands = commands.join(", "),
            code = self.cli.error_exit_code,
            json = self.kit.json,
        )
    }
}

/// An `Examples:` help section.
pub fn examples_section(examples: &[Example]) -> String {
    let mut out = String::from("Examples:\n");
    for ex in examples {
        out.push_str(&format!("  # {}\n  $ {}\n", ex.description, ex.cmd));
    }
    out.trim_end().to_string()
}

fn option(s: Option<&str>) -> String {
    s.map_or_else(|| "None".to_string(), |s| format!("Some({s:?})"))
}

fn strs(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|s| format!("{s:?}")).collect();
    format!("&[{}]", items.join(", "))
}

fn pairs(items: &[(&str, &str)]) -> String {
    let items: Vec<String> = items
        .iter()
        .map(|(a, b)| format!("({a:?}, {b:?})"))
        .collect();
    format!("&[{}]", items.join(", "))
}
