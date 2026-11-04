//! Behavior cases: every `[[case]]` in cli.toml runs here. A case gives the
//! arguments (plus optional stdin, input files and environment) and the
//! expected stdout (inline, or `tests/cases/<stdout_file>`), stderr fragment
//! and exit code.

mod common;

use std::path::Path;

#[test]
fn cases_from_cli_toml() {
    let spec = rsort::spec::Spec::get();
    let mut failures = Vec::new();
    for case in &spec.cases {
        let dir = tempfile::tempdir().unwrap();
        for (name, content) in &case.files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
        let mut c = common::cmd(dir.path());
        c.current_dir(dir.path()).args(&case.args);
        for (key, value) in &case.env {
            c.env(key, value);
        }
        if let Some(stdin) = &case.stdin {
            c.write_stdin(stdin.as_bytes());
        }
        let out = c.output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let mut problems = Vec::new();
        if out.status.code() != Some(case.exit_code) {
            problems.push(format!(
                "exit code {:?}, expected {}",
                out.status.code(),
                case.exit_code
            ));
        }
        let expected = match (&case.stdout, &case.stdout_file) {
            (Some(s), _) => Some(s.clone()),
            (None, Some(file)) => Some(
                std::fs::read_to_string(Path::new("tests/cases").join(file))
                    .unwrap_or_else(|e| panic!("case `{}`: tests/cases/{file}: {e}", case.name)),
            ),
            (None, None) => None,
        };
        if let Some(expected) = expected
            && stdout != expected
        {
            problems.push(format!(
                "stdout differs\n--- expected\n{expected}\n--- actual\n{stdout}"
            ));
        }
        if let Some(fragment) = &case.stderr_contains
            && !stderr.contains(fragment.as_str())
        {
            problems.push(format!("stderr lacks {fragment:?}:\n{stderr}"));
        }
        if !problems.is_empty() {
            failures.push(format!(
                "case `{}` ({:?}): {}",
                case.name,
                case.args,
                problems.join("; ")
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
