//! Discoverability of the command-line interface: aliases, help, completions.

use assert_cmd::Command;
use predicates::prelude::*;

fn ctfd() -> Command {
    Command::cargo_bin("ctfdc").expect("ctfdc binary should exist")
}

#[test]
fn no_arguments_shows_quick_start() {
    ctfd()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Quick start"))
        .stderr(predicate::str::contains("ctfdc init"));
}

#[test]
fn help_lists_aliases_and_examples() {
    ctfd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("dl"))
        .stdout(predicate::str::contains("lb"))
        .stdout(predicate::str::contains("Quick start"));
}

#[test]
fn subcommand_help_has_examples() {
    for cmd in ["dl", "s", "ls", "init"] {
        ctfd()
            .args([cmd, "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Examples:"));
    }
}

#[test]
fn completions_are_generated() {
    ctfd()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ctfdc"));
}

#[test]
fn missing_config_points_to_init() {
    let temp = tempfile::tempdir().unwrap();
    ctfd()
        .arg("ls")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(temp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("ctfdc init"));
}
