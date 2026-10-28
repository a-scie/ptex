// Copyright 2022 Science project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

const PTEX: &str = env!("CARGO_BIN_EXE_ptex");
const URL: &str = "https://github.com/a-scie/jump/releases/download/v0.2.1/scie-jump-linux-aarch64";

fn assert_fetched_buffer(buffer: &[u8]) {
    assert_eq!(1205568, buffer.len());
    assert_eq!(
        "937683255e98caf10745a674d7063bd38e9cbeb523b9f8ef4dbe8807abc35382".to_string(),
        hex::encode(Sha256::digest(buffer))
    );
}

fn isolated_command() -> Command {
    let mut command = Command::new(PTEX);
    command.env_clear();
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    command
}

#[test]
fn github_downloads_require_an_explicit_token_in_direct_and_manifest_modes() {
    let tempdir = tempfile::tempdir().unwrap();
    let manifest = tempdir.path().join("ptex.json");
    for url in [
        "https://api.github.com/repos/example/builds/actions/artifacts/123/zip",
        "https://github.com/example/builds/actions/runs/456/artifacts/123",
        "https://api.github.com/repos/example/builds/releases/assets/123",
    ] {
        std::fs::write(
            &manifest,
            serde_json::json!({"ptex": {"artifact": url}}).to_string(),
        )
        .unwrap();
        for args in [vec![url], vec![manifest.to_str().unwrap(), "artifact"]] {
            for token in [
                None,
                Some(""),
                Some("synthetic secret"),
                Some("synthetic\r\nsecret"),
            ] {
                let mut command = isolated_command();
                command
                    .args(["-s", "-D"])
                    .args(&args)
                    .env("HOME", tempdir.path())
                    .env("PTEX_DUMP_HEADERS", "1")
                    .env("GH_TOKEN", "synthetic-ambient-secret")
                    .env("GITHUB_TOKEN", "synthetic-ambient-secret")
                    .env(
                        "GOOGLE_APPLICATION_CREDENTIALS",
                        tempdir.path().join("absent.json"),
                    );
                if let Some(token) = token {
                    command.env("PTEX_GITHUB_TOKEN", token);
                }
                let output = command.output().unwrap();
                assert!(!output.status.success());
                assert!(output.stdout.is_empty());
                let error = String::from_utf8(output.stderr).unwrap();
                assert!(error.contains("PTEX_GITHUB_TOKEN"), "{error}");
                assert!(!error.contains("synthetic"), "{error}");
            }
        }
    }
}

#[test]
fn malformed_github_links_are_rejected_before_remote_name_creates_a_file() {
    let tempdir = tempfile::tempdir().unwrap();
    for url in [
        "https://api.github.com/repos/example/builds/actions/artifacts/123/zip?secret=synthetic",
        "https://synthetic@api.github.com/repos/example/builds/releases/assets/123",
        "https://api.github.com:443/repos/example/builds/releases/assets/123",
    ] {
        let output = isolated_command()
            .args(["-O", "-D", url])
            .current_dir(tempdir.path())
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(!error.contains("PTEX_GITHUB_TOKEN"), "{error}");
        assert!(!error.contains("synthetic"), "{error}");
    }
    assert_eq!(0, std::fs::read_dir(tempdir.path()).unwrap().count());
}

#[test]
fn file_downloads_ignore_github_tokens_and_preserve_binary_bytes() {
    let tempdir = tempfile::tempdir().unwrap();
    let source = tempdir.path().join("artifact");
    let bytes = b"binary\0\xffdata";
    std::fs::write(&source, bytes).unwrap();
    let output = isolated_command()
        .args(["-s", url::Url::from_file_path(source).unwrap().as_str()])
        .env("PTEX_GITHUB_TOKEN", "synthetic invalid token")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(bytes, output.stdout.as_slice());
}

#[test]
fn version() {
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        std::io::read_to_string(
            Command::new(PTEX)
                .arg("--version")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap()
                .stdout
                .unwrap()
        )
        .unwrap()
        .trim()
    );
}

#[test]
fn fetch_remote_name() {
    let tempdir = tempfile::tempdir().unwrap();
    assert!(
        Command::new(PTEX)
            .args(["-O", URL])
            .current_dir(&tempdir)
            .spawn()
            .unwrap()
            .wait()
            .unwrap()
            .success()
    );
    let local_file = tempdir.path().join("scie-jump-linux-aarch64");
    assert!(local_file.is_file());
    assert_fetched_buffer(std::fs::read(local_file).unwrap().as_slice());
}
