use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use tempfile::NamedTempFile;

const LIMIT_ERROR: &str = "input exceeds maximum size";

fn valid_log_line(message: &str) -> String {
    format!("2026-05-21T00:00:00Z INFO {message}\n")
}

#[test]
fn rejects_oversized_stdin_input() {
    let input = valid_log_line("stdin input that is deliberately over the configured limit");

    Command::cargo_bin("dredge")
        .unwrap()
        .arg("--max-input-bytes")
        .arg((input.len() - 1).to_string())
        .write_stdin(input)
        .assert()
        .failure()
        .stderr(predicate::str::contains(LIMIT_ERROR));
}

#[test]
fn accepts_boundary_sized_stdin_input() {
    let input = valid_log_line("stdin boundary input succeeds");

    Command::cargo_bin("dredge")
        .unwrap()
        .arg("--max-input-bytes")
        .arg(input.len().to_string())
        .write_stdin(input)
        .assert()
        .success();
}

#[test]
fn rejects_oversized_file_input() {
    let input = valid_log_line("file input that is deliberately over the configured limit");
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(input.as_bytes()).unwrap();

    Command::cargo_bin("dredge")
        .unwrap()
        .arg("--max-input-bytes")
        .arg((input.len() - 1).to_string())
        .arg(file.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(LIMIT_ERROR));
}

#[test]
fn accepts_boundary_sized_file_input() {
    let input = valid_log_line("file boundary input succeeds");
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(input.as_bytes()).unwrap();

    Command::cargo_bin("dredge")
        .unwrap()
        .arg("--max-input-bytes")
        .arg(input.len().to_string())
        .arg(file.path())
        .assert()
        .success();
}