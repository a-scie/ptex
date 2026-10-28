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

#[test]
fn invalid_gcs_urls_fail_before_credentials_are_loaded() {
    let tempdir = tempfile::tempdir().unwrap();
    for url in [
        "gs://bucket/",
        "gs://user@bucket/object",
        "gs://bucket/object?secret=synthetic",
    ] {
        let output = Command::new(PTEX)
            .args(["-s", url])
            .env_clear()
            .env("HOME", tempdir.path())
            .env(
                "GOOGLE_APPLICATION_CREDENTIALS",
                tempdir.path().join("absent.json"),
            )
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            !error.contains("Application Default Credentials"),
            "{error}"
        );
        assert!(!error.contains("synthetic"), "{error}");
    }
}

#[test]
fn invalid_adc_fails_in_direct_and_manifest_modes_without_disclosing_secrets() {
    let tempdir = tempfile::tempdir().unwrap();
    let credentials = tempdir.path().join("adc.json");
    std::fs::write(&credentials, r#"{"type":"synthetic-private-key-secret"}"#).unwrap();
    let manifest = tempdir.path().join("ptex.json");
    std::fs::write(
        &manifest,
        r#"{"ptex":{"artifact":"gs://private-artifacts/object"}}"#,
    )
    .unwrap();
    for args in [
        vec!["gs://private-artifacts/object"],
        vec![manifest.to_str().unwrap(), "artifact"],
    ] {
        let output = Command::new(PTEX)
            .args(["-s", "-D"])
            .args(args)
            .env_clear()
            .env("HOME", tempdir.path())
            .env("GOOGLE_APPLICATION_CREDENTIALS", &credentials)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("Application Default Credentials"), "{error}");
        assert!(!error.contains("synthetic-private-key-secret"), "{error}");
    }
}

#[test]
fn file_downloads_do_not_consult_adc() {
    let tempdir = tempfile::tempdir().unwrap();
    let source = tempdir.path().join("artifact");
    let bytes = b"binary\0\xffdata";
    std::fs::write(&source, bytes).unwrap();
    let output = Command::new(PTEX)
        .args(["-s", url::Url::from_file_path(source).unwrap().as_str()])
        .env_clear()
        .env(
            "GOOGLE_APPLICATION_CREDENTIALS",
            tempdir.path().join("absent.json"),
        )
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
