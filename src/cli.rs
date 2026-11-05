//! Clap definitions: structure only.
//!
//! Help text, about strings and examples come from `cli.toml` (rendered into
//! [`crate::spec::SPEC`] at build time) and are applied in [`command`], so do
//! not add doc comments to these fields: clap would use them as help text.
//! `tests/conformance.rs` checks these definitions against `cli.toml`.

use crate::spec::{Interface, SPEC, split_flags};
use clap::builder::TypedValueParser;
use clap::{ArgAction, Args, CommandFactory, Parser, ValueEnum};

#[derive(Debug, Parser)]
// Like sort (getopt): a repeated option takes its last value, and an
// option's value may start with '-' (`-S -r` gives -S the value "-r").
#[command(
    version,
    propagate_version = true,
    disable_help_subcommand = true,
    args_override_self = true
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[arg(value_name = "FILE")]
    pub files: Vec<std::path::PathBuf>,

    #[arg(
        short,
        long,
        value_name = "MODE",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "diagnose-first",
        value_parser = ["diagnose-first", "silent", "quiet"],
        action = ArgAction::Append
    )]
    pub check: Vec<String>,

    #[arg(short = 'C', long)]
    pub check_silent: bool,

    #[arg(short, long)]
    pub merge: bool,

    #[arg(short, long, value_name = "FILE", allow_hyphen_values = true)]
    pub output: Option<std::path::PathBuf>,

    #[arg(short, long)]
    pub unique: bool,

    #[arg(short, long)]
    pub stable: bool,

    #[arg(short = 'b', long)]
    pub ignore_leading_blanks: bool,

    #[arg(short, long)]
    pub dictionary_order: bool,

    #[arg(short = 'f', long)]
    pub ignore_case: bool,

    #[arg(short = 'i', long)]
    pub ignore_nonprinting: bool,

    #[arg(short = 'g', long, conflicts_with_all = ["numeric_sort", "human_numeric_sort", "month_sort", "random_sort", "version_sort"])]
    pub general_numeric_sort: bool,

    #[arg(short = 'h', long, conflicts_with_all = ["numeric_sort", "general_numeric_sort", "month_sort", "random_sort", "version_sort"])]
    pub human_numeric_sort: bool,

    #[arg(short = 'M', long, conflicts_with_all = ["numeric_sort", "general_numeric_sort", "human_numeric_sort", "random_sort", "version_sort"])]
    pub month_sort: bool,

    #[arg(short, long, conflicts_with_all = ["general_numeric_sort", "human_numeric_sort", "month_sort", "random_sort", "version_sort"])]
    pub numeric_sort: bool,

    #[arg(short = 'R', long, conflicts_with_all = ["numeric_sort", "general_numeric_sort", "human_numeric_sort", "month_sort", "version_sort"])]
    pub random_sort: bool,

    #[arg(short = 'V', long, conflicts_with_all = ["numeric_sort", "general_numeric_sort", "human_numeric_sort", "month_sort", "random_sort"])]
    pub version_sort: bool,

    #[arg(long, value_name = "TYPE", value_parser = ["general-numeric", "human-numeric", "month", "numeric", "random"], allow_hyphen_values = true, action = ArgAction::Append)]
    pub sort: Vec<String>,

    #[arg(short, long)]
    pub reverse: bool,

    #[arg(long, value_name = "FILE", allow_hyphen_values = true)]
    pub random_source: Option<std::path::PathBuf>,

    #[arg(short, long, value_name = "POS1[,POS2]", allow_hyphen_values = true)]
    pub key: Vec<String>,

    #[arg(
        short = 't',
        long,
        value_name = "CHAR",
        allow_hyphen_values = true,
        value_parser = clap::builder::OsStringValueParser::new().try_map(separator)
    )]
    pub field_separator: Option<u8>,

    #[arg(short, long)]
    pub zero_terminated: bool,

    #[arg(long, value_name = "FILE", allow_hyphen_values = true)]
    pub files0_from: Option<std::path::PathBuf>,

    #[arg(short = 'S', long, value_name = "SIZE", allow_hyphen_values = true)]
    pub buffer_size: Option<String>,

    #[arg(short = 'T', long, value_name = "DIR", allow_hyphen_values = true)]
    pub temporary_directory: Option<std::path::PathBuf>,

    #[arg(long, value_name = "N", allow_hyphen_values = true)]
    pub parallel: Option<String>,

    #[arg(long, value_name = "N", allow_hyphen_values = true)]
    pub batch_size: Option<String>,

    #[arg(long, value_name = "PROGRAM", allow_hyphen_values = true)]
    pub compress_program: Option<String>,

    #[arg(long)]
    pub radixsort: bool,

    #[arg(long)]
    pub mergesort: bool,

    #[arg(long)]
    pub qsort: bool,

    #[arg(long)]
    pub heapsort: bool,

    #[arg(long)]
    pub mmap: bool,
}

/// `-t CHAR` as sort reads it: while more than one byte is left it must
/// start with a backslash, which is dropped, and a `0` right after a dropped
/// backslash is NUL; then the first byte (NUL when empty). So `\t` is `t`
/// and `\\` is a backslash. Like sort, every `-t` given is checked.
fn separator(arg: std::ffi::OsString) -> Result<u8, String> {
    use std::os::unix::ffi::OsStrExt;
    let mut s = arg.as_bytes();
    while s.len() > 1 {
        if s[0] != b'\\' {
            return Err("the field separator must be one character".into());
        }
        s = &s[1..];
        if s[0] == b'0' {
            return Ok(0);
        }
    }
    Ok(s.first().copied().unwrap_or(0))
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
    apply_spec(Cli::command(), &SPEC)
}

/// Apply about/help/examples and the help/version flags from `spec` to `cmd`
/// and its subcommands.
pub fn apply_spec(cmd: clap::Command, spec: &Interface) -> clap::Command {
    let mut cmd = cmd
        .name(spec.name)
        .bin_name(spec.name)
        .about(spec.about)
        .after_long_help(spec.after_long_help);
    if let Some(long) = spec.long_about {
        cmd = cmd.long_about(long);
    }
    for (id, help) in spec.help {
        cmd = set_help(cmd, id, help);
    }
    let cmd = apply_subcommands(cmd, spec, "");
    apply_help_and_version_flags(cmd, spec)
}

/// Replace clap's `-h/--help` and `-V/--version` when `cli.toml` declares others
/// (e.g. `help_flags = ["--help"]` frees `-h` for the CLI's own use).
fn apply_help_and_version_flags(mut cmd: clap::Command, spec: &Interface) -> clap::Command {
    let custom_help = spec.help_flags != ["-h", "--help"];
    let custom_version = spec.version_flags != ["-V", "--version"];
    if custom_help {
        cmd = disable_help_everywhere(cmd);
        cmd = add_flag(cmd, "help", spec.help_flags, ArgAction::Help, "Print help");
    }
    if custom_version {
        cmd = cmd.disable_version_flag(true);
        cmd = add_flag(
            cmd,
            "version",
            spec.version_flags,
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
    flags: &'static [&'static str],
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

fn apply_subcommands(mut cmd: clap::Command, spec: &Interface, prefix: &str) -> clap::Command {
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
            sub = sub.about(entry.about);
            if let Some(long) = entry.long_about {
                sub = sub.long_about(long);
            }
            if let Some(examples) = entry.after_help {
                sub = sub.after_help(examples);
            }
            for (id, help) in entry.help {
                sub = set_help(sub, id, help);
            }
            apply_subcommands(sub, spec, &path)
        });
    }
    cmd
}

/// Set an argument's help text. Arguments missing from the code are skipped
/// (the conformance test reports them) rather than panicking at runtime.
fn set_help(cmd: clap::Command, id: &str, help: &'static str) -> clap::Command {
    if cmd.get_arguments().any(|a| a.get_id() == id) {
        cmd.mut_arg(id, |a| a.help(help))
    } else {
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_with(
        help_flags: &'static [&'static str],
        version_flags: &'static [&'static str],
    ) -> Interface {
        Interface {
            help_flags,
            version_flags,
            ..SPEC
        }
    }

    fn parse(cmd: &clap::Command, args: &[&str]) -> Result<clap::ArgMatches, clap::Error> {
        cmd.clone()
            .try_get_matches_from(std::iter::once("x").chain(args.iter().copied()))
    }

    #[test]
    fn long_only_help_and_version_free_the_short_flags() {
        let spec = spec_with(&["--help"], &["--version"]);
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
        let spec = spec_with(&["-?", "--help"], &[]);
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
}
