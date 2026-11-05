//! `--help` snapshot for every command in cli.toml (plus the root).
//! Accept changes with `just snapshots`, then review `git diff tests/snapshots`.

mod common;

#[test]
fn help_snapshots() {
    let home = tempfile::tempdir().unwrap();
    let spec = common::spec::Spec::get();
    let Some(help_flag) = spec.cli.help_flags.iter().find(|f| f.starts_with("--")) else {
        return; // no long help flag
    };
    let help_flag = help_flag.as_str();
    let paths = std::iter::once(String::new()).chain(spec.commands.iter().map(|c| c.path.clone()));
    for path in paths {
        let mut args: Vec<&str> = path.split_whitespace().collect();
        args.push(help_flag);
        let output = common::cmd(home.path()).args(&args).output().unwrap();
        assert!(output.status.success(), "`{path} {help_flag}` failed");
        assert!(
            output.stderr.is_empty(),
            "`{path} {help_flag}` wrote to stderr"
        );
        let name = if path.is_empty() {
            "root".to_string()
        } else {
            path.replace(' ', "-")
        };
        insta::assert_snapshot!(name, String::from_utf8(output.stdout).unwrap());
    }
}
