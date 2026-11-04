//! Clap definitions: structure only.
//!
//! Help text, about strings and examples come from `cli.toml` and are applied
//! in [`command`], so do not add doc comments to these fields: clap would use
//! them as help text. `tests/conformance.rs` checks these definitions against
//! `cli.toml`.

use crate::spec::{Example, Spec, split_flags};
use clap::{ArgAction, Args, CommandFactory, Parser, ValueEnum};

#[derive(Debug, Parser)]
#[command(version, propagate_version = true, disable_help_subcommand = true)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
}

#[derive(Debug, Args)]
pub struct GlobalArgs {}

/// Effective global options. A field whose flag was removed with its kit
/// feature keeps its default.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub verbose: u8,
    pub quiet: bool,
    pub color: Option<ColorWhen>,
}

impl GlobalArgs {
    #[allow(clippy::needless_update)] // the update supplies defaults for removed flags
    pub fn settings(&self) -> Settings {
        Settings {
            ..Settings::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorWhen {
    Auto,
    Always,
    Never,
}

/// The clap command with all text from `cli.toml` applied.
pub fn command() -> clap::Command {
    apply_spec(Cli::command(), Spec::get())
}

/// Apply about/help/examples and the help/version flags from `spec` to `cmd`
/// and its subcommands.
pub fn apply_spec(cmd: clap::Command, spec: &Spec) -> clap::Command {
    let mut cmd = cmd
        .name(spec.cli.name.clone())
        .bin_name(spec.cli.name.clone())
        .about(spec.cli.about.clone())
        .after_long_help(root_after_help(spec));
    if let Some(long) = &spec.cli.long_about {
        cmd = cmd.long_about(long.clone());
    }
    for flag in &spec.flags {
        cmd = set_help(cmd, &flag.id, &flag.help);
    }
    for arg in &spec.args {
        cmd = set_help(cmd, &arg.id, &arg.help);
    }
    let cmd = apply_subcommands(cmd, spec, "");
    apply_help_and_version_flags(cmd, spec)
}

/// Replace clap's `-h/--help` and `-V/--version` when `cli.toml` declares others
/// (e.g. `help_flags = ["--help"]` frees `-h` for the CLI's own use).
fn apply_help_and_version_flags(mut cmd: clap::Command, spec: &Spec) -> clap::Command {
    let custom_help = spec.cli.help_flags != ["-h", "--help"];
    let custom_version = spec.cli.version_flags != ["-V", "--version"];
    if custom_help {
        cmd = disable_help_everywhere(cmd);
        cmd = add_flag(
            cmd,
            "help",
            &spec.cli.help_flags,
            ArgAction::Help,
            "Print help",
        );
    }
    if custom_version {
        cmd = cmd.disable_version_flag(true);
        cmd = add_flag(
            cmd,
            "version",
            &spec.cli.version_flags,
            ArgAction::Version,
            "Print version",
        );
    }
    cmd
}

fn disable_help_everywhere(cmd: clap::Command) -> clap::Command {
    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    let mut cmd = cmd.disable_help_flag(true);
    for name in names {
        cmd = cmd.mut_subcommand(name, disable_help_everywhere);
    }
    cmd
}

fn add_flag(
    cmd: clap::Command,
    id: &'static str,
    flags: &[String],
    action: ArgAction,
    help: &'static str,
) -> clap::Command {
    let (short, long) = split_flags(flags);
    if short.is_none() && long.is_none() {
        return cmd;
    }
    let mut arg = clap::Arg::new(id).action(action).help(help).global(true);
    if let Some(s) = short {
        arg = arg.short(s);
    }
    if let Some(l) = long {
        arg = arg.long(l);
    }
    cmd.arg(arg)
}

fn apply_subcommands(mut cmd: clap::Command, spec: &Spec, prefix: &str) -> clap::Command {
    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    for name in names {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix} {name}")
        };
        let Some(entry) = spec.command(&path) else {
            // Missing entries are reported by the conformance test.
            continue;
        };
        cmd = cmd.mut_subcommand(name.as_str(), |mut sub| {
            sub = sub.about(entry.about.clone());
            if let Some(long) = &entry.long_about {
                sub = sub.long_about(long.clone());
            }
            if !entry.examples.is_empty() {
                sub = sub.after_help(examples_section(&entry.examples));
            }
            for arg in &entry.args {
                sub = set_help(sub, &arg.id, &arg.help);
            }
            for flag in &entry.flags {
                sub = set_help(sub, &flag.id, &flag.help);
            }
            apply_subcommands(sub, spec, &path)
        });
    }
    cmd
}

/// Set an argument's help text. Arguments missing from the code are skipped
/// (the conformance test reports them) rather than panicking at runtime.
fn set_help(cmd: clap::Command, id: &str, help: &str) -> clap::Command {
    if cmd.get_arguments().any(|a| a.get_id() == id) {
        cmd.mut_arg(id, |a| a.help(help.to_string()))
    } else {
        cmd
    }
}

fn examples_section(examples: &[Example]) -> String {
    let mut out = String::from("Examples:\n");
    for ex in examples {
        out.push_str(&format!("  # {}\n  $ {}\n", ex.description, ex.cmd));
    }
    out.trim_end().to_string()
}

fn root_after_help(spec: &Spec) -> String {
    let mut out = String::new();
    if !spec.cli.examples.is_empty() {
        out.push_str(&examples_section(&spec.cli.examples));
        out.push_str("\n\n");
    }
    out.push_str("Exit codes:\n");
    for code in spec.all_exit_codes() {
        out.push_str(&format!("  {:>3}  {}\n", code.code, code.description));
    }
    if spec.command("schema").is_some() {
        out.push_str(&format!(
            "\nRun `{} schema` for the full machine-readable interface.",
            spec.cli.name
        ));
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_with(cli_extra: &str) -> Spec {
        Spec::parse(&format!(
            "schema_version = 1\n[cli]\nname = \"x\"\nabout = \"y\"\n{cli_extra}\n"
        ))
        .unwrap()
    }

    fn parse(cmd: &clap::Command, args: &[&str]) -> Result<clap::ArgMatches, clap::Error> {
        cmd.clone()
            .try_get_matches_from(std::iter::once("x").chain(args.iter().copied()))
    }

    #[test]
    fn long_only_help_and_version_free_the_short_flags() {
        let spec = spec_with("help_flags = [\"--help\"]\nversion_flags = [\"--version\"]");
        let cmd = apply_spec(Cli::command(), &spec);
        cmd.clone().debug_assert();
        use clap::error::ErrorKind;
        let kind = |args: &[&str]| parse(&cmd, args).err().map(|e| e.kind());
        assert_eq!(kind(&["--help"]), Some(ErrorKind::DisplayHelp));
        assert_eq!(kind(&["--version"]), Some(ErrorKind::DisplayVersion));
        // -h/-V are free: unknown here, or the CLI's own flags.
        assert_ne!(kind(&["-h"]), Some(ErrorKind::DisplayHelp));
        assert_ne!(kind(&["-V"]), Some(ErrorKind::DisplayVersion));
    }

    #[test]
    fn help_flags_reach_subcommands_and_version_can_be_removed() {
        let spec = spec_with("help_flags = [\"-?\", \"--help\"]\nversion_flags = []");
        let cmd = apply_spec(Cli::command(), &spec);
        cmd.clone().debug_assert();
        assert_eq!(
            parse(&cmd, &["-?"]).unwrap_err().kind(),
            clap::error::ErrorKind::DisplayHelp
        );
        assert_eq!(
            parse(&cmd, &["--version"]).unwrap_err().kind(),
            clap::error::ErrorKind::UnknownArgument
        );
        let sub = cmd
            .get_subcommands()
            .next()
            .map(|s| s.get_name().to_string());
        if let Some(sub) = sub {
            assert_eq!(
                parse(&cmd, &[sub.as_str(), "-?"]).unwrap_err().kind(),
                clap::error::ErrorKind::DisplayHelp
            );
        }
    }

    #[test]
    fn root_help_mentions_schema_only_when_there_is_a_schema_command() {
        let spec = spec_with("");
        assert!(!root_after_help(&spec).contains("schema"));
        assert!(root_after_help(&spec).starts_with("Exit codes:"));
    }
}
