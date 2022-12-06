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
        format!("{digest:x}", digest = Sha256::digest(buffer))
    );
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
    assert!(Command::new(PTEX)
        .args(["-O", URL])
        .current_dir(&tempdir)
        .spawn()
        .unwrap()
        .wait()
        .unwrap()
        .success());
    let local_file = tempdir.path().join("scie-jump-linux-aarch64");
    assert!(local_file.is_file());
    assert_fetched_buffer(std::fs::read(local_file).unwrap().as_slice());
}
