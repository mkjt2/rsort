//! Conformance: the clap definitions in src/cli.rs must match cli.toml exactly,
//! and the error catalog in src/error.rs must match its exit codes.
//!
//! When this fails, decide which side is wrong. cli.toml is the approved
//! design: change the code unless the spec itself needs to change (and then
//! re-approve it with the user).

mod common;

use clap::{ArgAction, CommandFactory};
use common::spec::{Arg, Flag, FlagKind, Spec};
use rsort::cli::Cli;
use std::collections::BTreeSet;

fn kind_of(arg: &clap::Arg) -> Option<FlagKind> {
    match arg.get_action() {
        ArgAction::SetTrue | ArgAction::SetFalse => Some(FlagKind::Switch),
        ArgAction::Count => Some(FlagKind::Count),
        ArgAction::Set => Some(FlagKind::Value),
        ArgAction::Append => Some(FlagKind::Values),
        _ => None,
    }
}

fn possible_values(arg: &clap::Arg) -> Vec<String> {
    arg.get_possible_values()
        .iter()
        .filter(|p| !p.is_hide_set())
        .map(|p| p.get_name().to_string())
        .collect()
}

fn defaults(arg: &clap::Arg) -> Option<String> {
    let vals: Vec<String> = arg
        .get_default_values()
        .iter()
        .map(|v| v.to_string_lossy().into_owned())
        .collect();
    (!vals.is_empty()).then(|| vals.join(","))
}

fn value_name(arg: &clap::Arg) -> Option<String> {
    arg.get_value_names()
        .and_then(|names| names.first())
        .map(|n| n.to_string())
}

fn check_flag(problems: &mut Vec<String>, owner: &str, flag: &Flag, arg: &clap::Arg) {
    let mut diff = |field: &str, spec: String, code: String| {
        if spec != code {
            problems.push(format!(
                "{owner} --{}: {field} is {code} in code but {spec} in cli.toml",
                flag.long
            ));
        }
    };
    diff(
        "long",
        format!("{:?}", Some(&flag.long)),
        format!("{:?}", arg.get_long()),
    );
    diff(
        "short",
        format!("{:?}", flag.short),
        format!("{:?}", arg.get_short()),
    );
    diff(
        "kind",
        format!("{:?}", Some(flag.kind)),
        format!("{:?}", kind_of(arg)),
    );
    diff(
        "global",
        flag.global.to_string(),
        arg.is_global_set().to_string(),
    );
    diff(
        "required",
        flag.required.to_string(),
        arg.is_required_set().to_string(),
    );
    diff(
        "env",
        format!("{:?}", flag.env),
        format!(
            "{:?}",
            arg.get_env().map(|e| e.to_string_lossy().into_owned())
        ),
    );
    if matches!(flag.kind, FlagKind::Value | FlagKind::Values) {
        diff(
            "default",
            format!("{:?}", flag.default),
            format!("{:?}", defaults(arg)),
        );
        diff(
            "possible_values",
            format!("{:?}", flag.possible_values),
            format!("{:?}", possible_values(arg)),
        );
        let expected = flag
            .value_name
            .clone()
            .unwrap_or_else(|| flag.id.to_uppercase());
        diff(
            "value_name",
            format!("{:?}", Some(expected)),
            format!("{:?}", value_name(arg)),
        );
        // clap has no getter for default-missing values; its Debug output does show them.
        let optional = arg.get_num_args().is_some_and(|r| r.min_values() == 0);
        let debug = format!("{arg:?}");
        let missing = flag
            .default_missing
            .as_ref()
            .map(|v| debug.contains(&format!("default_missing_vals: [{v:?}]")));
        diff(
            "optional value (default_missing)",
            flag.default_missing.is_some().to_string(),
            optional.to_string(),
        );
        if let Some(found) = missing {
            diff("default_missing value", "true".into(), found.to_string());
            diff(
                "require_equals",
                "true".into(),
                arg.is_require_equals_set().to_string(),
            );
        }
    }
    let mut aliases: Vec<String> = arg
        .get_all_aliases()
        .unwrap_or_default()
        .into_iter()
        .map(String::from)
        .collect();
    aliases.sort();
    let mut spec_aliases = flag.aliases.clone();
    spec_aliases.sort();
    diff(
        "aliases",
        format!("{spec_aliases:?}"),
        format!("{aliases:?}"),
    );
    let mut shorts = arg.get_all_short_aliases().unwrap_or_default();
    shorts.sort();
    let mut spec_shorts = flag.short_aliases.clone();
    spec_shorts.sort();
    diff(
        "short_aliases",
        format!("{spec_shorts:?}"),
        format!("{shorts:?}"),
    );
}

fn check_positional(problems: &mut Vec<String>, owner: &str, spec: &Arg, arg: &clap::Arg) {
    let mut diff = |field: &str, s: String, c: String| {
        if s != c {
            problems.push(format!(
                "{owner} <{}>: {field} is {c} in code but {s} in cli.toml",
                spec.id
            ));
        }
    };
    let multiple = matches!(arg.get_action(), ArgAction::Append)
        || arg.get_num_args().is_some_and(|r| r.max_values() > 1);
    diff(
        "required",
        spec.required.to_string(),
        arg.is_required_set().to_string(),
    );
    diff("multiple", spec.multiple.to_string(), multiple.to_string());
    diff(
        "default",
        format!("{:?}", spec.default),
        format!("{:?}", defaults(arg)),
    );
    diff(
        "possible_values",
        format!("{:?}", spec.possible_values),
        format!("{:?}", possible_values(arg)),
    );
    let expected = spec
        .value_name
        .clone()
        .unwrap_or_else(|| spec.id.to_uppercase());
    diff(
        "value_name",
        format!("{:?}", Some(expected)),
        format!("{:?}", value_name(arg)),
    );
}

fn check_args(
    problems: &mut Vec<String>,
    owner: &str,
    cmd: &clap::Command,
    flags: &[Flag],
    positionals: &[Arg],
) {
    let code_args: Vec<&clap::Arg> = cmd
        .get_arguments()
        .filter(|a| !matches!(a.get_id().as_str(), "help" | "version"))
        .collect();
    let code_ids: BTreeSet<String> = code_args.iter().map(|a| a.get_id().to_string()).collect();
    let spec_ids: BTreeSet<String> = flags
        .iter()
        .map(|f| f.id.clone())
        .chain(positionals.iter().map(|a| a.id.clone()))
        .collect();
    for missing in spec_ids.difference(&code_ids) {
        problems.push(format!(
            "{owner}: `{missing}` is in cli.toml but not in code"
        ));
    }
    for extra in code_ids.difference(&spec_ids) {
        problems.push(format!("{owner}: `{extra}` is in code but not in cli.toml"));
    }
    for arg in code_args {
        let id = arg.get_id().as_str();
        if let Some(flag) = flags.iter().find(|f| f.id == id) {
            if arg.is_positional() {
                problems.push(format!(
                    "{owner}: `{id}` is positional in code but a flag in cli.toml"
                ));
            } else {
                check_flag(problems, owner, flag, arg);
            }
        } else if let Some(pos) = positionals.iter().find(|p| p.id == id) {
            if arg.is_positional() {
                check_positional(problems, owner, pos, arg);
            } else {
                problems.push(format!(
                    "{owner}: `{id}` is a flag in code but positional in cli.toml"
                ));
            }
        }
    }
}

fn check_command(problems: &mut Vec<String>, spec: &Spec, cmd: &clap::Command, path: &str) {
    if !path.is_empty() {
        let Some(entry) = spec.command(path) else {
            problems.push(format!("command `{path}` is in code but not in cli.toml"));
            return;
        };
        check_args(
            problems,
            &format!("`{path}`"),
            cmd,
            &entry.flags,
            &entry.args,
        );
    }
    let code_children: BTreeSet<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    let spec_children: BTreeSet<String> = spec
        .commands
        .iter()
        .filter_map(|c| match c.path.rsplit_once(' ') {
            Some((parent, name)) if parent == path => Some(name.to_string()),
            None if path.is_empty() => Some(c.path.clone()),
            _ => None,
        })
        .collect();
    for missing in spec_children.difference(&code_children) {
        let full = if path.is_empty() {
            missing.clone()
        } else {
            format!("{path} {missing}")
        };
        problems.push(format!("command `{full}` is in cli.toml but not in code"));
    }
    for sub in cmd.get_subcommands() {
        let child = if path.is_empty() {
            sub.get_name().to_string()
        } else {
            format!("{path} {}", sub.get_name())
        };
        if !spec_children.contains(sub.get_name()) {
            problems.push(format!("command `{child}` is in code but not in cli.toml"));
            continue;
        }
        check_command(problems, spec, sub, &child);
    }
}

#[test]
fn clap_definitions_match_cli_toml() {
    let spec = Spec::get();
    let root = Cli::command();
    let mut problems = Vec::new();
    check_args(&mut problems, "<root>", &root, &spec.flags, &spec.args);
    check_command(&mut problems, spec, &root, "");
    assert!(
        problems.is_empty(),
        "src/cli.rs and cli.toml disagree:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn cli_toml_is_valid() {
    let problems = Spec::get().validate();
    assert!(
        problems.is_empty(),
        "cli.toml problems:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn cli_name_matches_package() {
    assert_eq!(Spec::get().cli.name, env!("CARGO_PKG_NAME"));
}

#[test]
fn error_catalog_matches_exit_codes() {
    let declared = Spec::get().error_catalog();
    let code: BTreeSet<(String, i32)> = rsort::error::EXIT_CODES
        .iter()
        .map(|(n, c)| ((*n).to_string(), *c))
        .collect();
    assert_eq!(
        code, declared,
        "src/error.rs EXIT_CODES must match cli.toml: usage = 2, error = [cli].error_exit_code, \
         plus every [[exit_code]]"
    );
}

#[test]
fn final_command_is_consistent() {
    // Panics on clashes clap can't resolve, e.g. a flag using -h while
    // `help_flags` still claims it.
    rsort::cli::command().debug_assert();
}

#[test]
fn help_and_version_flags_match_cli_toml() {
    use rsort::spec::split_flags;
    let spec = Spec::get();
    let cmd = rsort::cli::command();
    for (id, flags, auto) in [
        ("help", &spec.cli.help_flags, ["-h", "--help"]),
        ("version", &spec.cli.version_flags, ["-V", "--version"]),
    ] {
        let mut probe = cmd.clone();
        probe.build();
        let arg = probe.get_arguments().find(|a| a.get_id() == id);
        let (short, long) = split_flags(flags);
        if flags.as_slice() == auto {
            continue; // clap's built-in flag
        }
        assert_eq!(arg.and_then(|a| a.get_short()), short, "{id} short flag");
        assert_eq!(arg.and_then(|a| a.get_long()), long, "{id} long flag");
    }
}

#[test]
fn every_flag_and_command_has_help_text_applied() {
    let cmd = rsort::cli::command();
    fn walk(cmd: &clap::Command, path: &str, problems: &mut Vec<String>) {
        if cmd.get_about().is_none() {
            problems.push(format!("`{path}` has no about"));
        }
        for arg in cmd.get_arguments() {
            if !matches!(arg.get_id().as_str(), "help" | "version") && arg.get_help().is_none() {
                problems.push(format!("`{path}` arg `{}` has no help", arg.get_id()));
            }
        }
        for sub in cmd.get_subcommands() {
            walk(sub, format!("{path} {}", sub.get_name()).trim(), problems);
        }
    }
    let mut problems = Vec::new();
    walk(&cmd, "", &mut problems);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn generated_interface_matches_cli_toml() {
    // build.rs renders cli.toml into rsort::spec; this catches a stale or
    // wrong rendering.
    let spec = Spec::get();
    let built = &rsort::spec::SPEC;
    assert_eq!(built.name, spec.cli.name);
    assert_eq!(built.about, spec.cli.about);
    assert_eq!(built.long_about, spec.cli.long_about.as_deref());
    assert_eq!(built.after_long_help, spec.root_after_help());
    assert_eq!(built.help_flags, spec.cli.help_flags);
    assert_eq!(built.version_flags, spec.cli.version_flags);
    assert_eq!(built.error_exit_code, spec.cli.error_exit_code);
    assert_eq!(built.help, spec.root_help());
    assert!(spec.commands.is_empty(), "rsort has no subcommands");
    let schema: serde_json::Value = serde_json::from_str(rsort::spec::SCHEMA).unwrap();
    assert_eq!(schema, spec.schema_document(env!("CARGO_PKG_VERSION")));
}

// The cli.toml model itself.

#[test]
fn validate_reports_problems() {
    let spec = Spec::parse(
        r#"
        schema_version = 2
        [cli]
        name = "x"
        about = " "
        error_exit_code = 9
        [[exit_code]]
        code = 0
        name = "zero"
        description = "out of range"
        [[exit_code]]
        code = 7
        name = "a"
        description = "first"
        [[exit_code]]
        code = 7
        name = "b"
        description = "second"
        [[arg]]
        id = "file"
        help = "mixed with commands"
        [[case]]
        name = "c"
        stdout = "x"
        stdout_file = "x.out"
        exit_code = 99
        [[case]]
        name = "c"
        [[flag]]
        id = "f"
        long = "f"
        kind = "switch"
        value_name = "V"
        help = ""
        [[command]]
        path = "a b"
        about = ""
        mutates = true
        exit_codes = [99]
        [[command.arg]]
        id = "x"
        help = ""
        [[command]]
        path = "a b"
        about = "again"
        "#,
    )
    .unwrap();
    let problems = spec.validate().join("\n");
    for needle in [
        "schema_version",
        "cli.about",
        "exit code 7 declared twice",
        "must be in 1..=125",
        "cannot be mixed",
        "error_exit_code 9",
        "case `c` declared twice",
        "both stdout and stdout_file",
        "undeclared exit code 99",
        "declared twice",
        "empty about",
        "no parent entry `a`",
        "--dry-run",
        "arg `x`",
        "empty help",
        "takes no value",
    ] {
        assert!(
            problems.contains(needle),
            "missing `{needle}` in:\n{problems}"
        );
    }
}

#[test]
fn unknown_fields_are_rejected() {
    let err =
        Spec::parse("schema_version = 1\n[cli]\nname='x'\nabout='y'\ntypo = 1\n").unwrap_err();
    assert!(err.to_string().contains("typo"));
}

#[test]
fn exit_codes_are_sorted_and_include_builtins() {
    let codes: Vec<i32> = Spec::get()
        .all_exit_codes()
        .iter()
        .map(|c| c.code)
        .collect();
    let mut sorted = codes.clone();
    sorted.sort();
    assert_eq!(codes, sorted);
    assert!(codes.contains(&0) && codes.contains(&2) && codes.contains(&130));
}

#[test]
fn root_help_mentions_schema_only_when_there_is_a_schema_command() {
    let spec = Spec::parse("schema_version = 1\n[cli]\nname = \"x\"\nabout = \"y\"\n").unwrap();
    assert!(!spec.root_after_help().contains("schema"));
    assert!(spec.root_after_help().starts_with("Exit codes:"));
    let commands = "[[command]]\npath = \"schema\"\nabout = \"z\"\n";
    let spec = Spec::parse(&format!(
        "schema_version = 1\n[cli]\nname = \"x\"\nabout = \"y\"\n{commands}"
    ))
    .unwrap();
    assert!(spec.root_after_help().contains("Run `x schema`"));
}

#[test]
fn rendered_rust_escapes_text() {
    let spec = Spec::parse("schema_version = 1\n[cli]\nname = \"x\"\nabout = 'say \"hi\" \\ é'\n")
        .unwrap();
    let rust = spec.to_rust("1.2.3");
    assert!(rust.contains(r#"about: "say \"hi\" \\ é""#), "{rust}");
}
