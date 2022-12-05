// Copyright 2022 Science project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::collections::BTreeMap;
use std::env;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use curl::easy::{Easy2, Handler, NetRc, WriteError};
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    ptex: BTreeMap<PathBuf, String>,
}

impl Config {
    fn parse<R: Read>(reader: R) -> Result<Self> {
        let config: Self =
            serde_json::from_reader(reader).context("Failed to parse ptex config")?;
        Ok(config)
    }
}

struct FetchHandler<W: Write> {
    output: W,
    progress: ProgressBar,
}

#[cfg(target_family = "windows")]
const NEWLINE: &str = "\r\n";

#[cfg(target_family = "unix")]
const NEWLINE: &str = "\n";

fn write(state: &ProgressState, w: &mut dyn std::fmt::Write) {
    write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
}

impl<W: Write> FetchHandler<W> {
    fn new(url: &str, output: W) -> Self {
        let progress = ProgressBar::new(0);
        progress.set_prefix(format!("Downloading {url}...{NEWLINE}"));
        progress.set_style(
            ProgressStyle::with_template(
                "{prefix}[{elapsed_precise}] [{bar:30}] {bytes}/{total_bytes} (eta: {eta})",
            )
            .unwrap()
            .with_key("eta", write)
            .progress_chars("#>-"),
        );
        Self { output, progress }
    }
}

impl<W: Write> Handler for FetchHandler<W> {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.output
            .write_all(data)
            .expect("Failed to write data to output");
        Ok(data.len())
    }

    fn progress(&mut self, dltotal: f64, dlnow: f64, _ultotal: f64, _ulnow: f64) -> bool {
        if dltotal > 0.0 {
            self.progress.set_length(dltotal as u64)
        }
        self.progress.set_position(dlnow as u64);
        true
    }
}

fn fetch_manifest<R: Read, W: Write>(lift_manifest: R, file_path: &Path, output: W) -> Result<()> {
    let config = Config::parse(lift_manifest)?;
    let url = config.ptex.get(file_path).with_context(|| {
        format!(
            "Did not find an URL mapping for file {path}.",
            path = file_path.display()
        )
    })?;
    fetch(url, output)
        .with_context(|| format!("Failed to source file {file}", file = file_path.display()))
}

fn fetch<W: Write>(url: &str, output: W) -> Result<()> {
    let mut easy = Easy2::new(FetchHandler::new(url, output));
    easy.follow_location(true)
        .context("Failed to configure re-direct following")?;
    easy.fail_on_error(true)
        .context("Failed to configure fail on error behavior")?;
    easy.netrc(NetRc::Optional)
        .context("Failed to enable ~/.netrc parsing")?;
    easy.url(url)
        .context("Failed to configure URL to fetch from as {url}")?;
    easy.progress(true)
        .context("Failed to enable progress meter")?;
    easy.perform()
        .with_context(|| format!("Failed to fetch {url}"))
}

fn usage() -> Result<()> {
    let current_exe = env::current_exe().context("Failed to determine current executable")?;
    eprintln!(
        r#"Usage:
    {current_exe} [lift manifest path] [file name]
    {current_exe} [URL]

    The `ptex` binary is a statically compiled URL fetcher based on
    libcurl. It supports the HTTP protocol up through HTTP/2, the FTP
    protocol and TLS via OpenSSL. It follows redirects, uses credentials
    from ~/.netrc if available and can perform NTLM authentication. It
    exits with a non-zero status if there was a network or protocol
    error.

{current_exe} [lift manifest path] [file name]

    For use in a scie file source binding. The first argument is the
    path to the scie lift manifest and the second argument is the file
    name to source. You configure this use in a scie by fully specifying
    file metadata, including size, hash and type and setting the source
    to the name of a binding command that uses `ptex` as its executable.

    The relevant parts of the lift manifest look like so:

    {{
      "scie": {{
        "lift": {{
          "files": [
            {{
              "name": "ptex-linux-x86_64"
              "executable": true
            }},
            {{
              "name": "some-file-to-be-fetched.tar.gz",
              "size": 123,
              "hash": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
              "type": "tar.gz",
              "source": "ptex-fetch"
            }},
          ]
          "boot": {{
            "bindings": {{
              "ptex-fetch": {{
                "exe": "{{ptex-linux-x86_64}}",
                "args": [
                  "{{scie.lift}}"
                ]
              }}
            }}
          }}
        }}
      }},
      "ptex": {{
        "some-file-to-be-fetched.tar.gz":
          "https://example.org/downloads/some-file-to-be-fetched.tar.gz"
      }}
    }}

    The file name is passed in as a second argument to the source
    binding by the `scie-jump` and `ptex` uses that file name to look up
    the URL to fetch the file from in the top-level "ptex" URL database
    object.

    See more documentation on scie packaging configuration here:
     https://github.com/a-scie/jump/blob/main/docs/packaging.md

{current_exe} [URL]

    For use as a fully self-contained curl-like binary. The given URL is
    fetched and the response is streamed to stdout.
"#,
        current_exe = current_exe.display()
    );
    Ok(())
}

trait OrExit<T> {
    fn or_exit(self) -> T;
}

impl<T> OrExit<T> for Result<T> {
    fn or_exit(self) -> T {
        match self {
            Ok(item) => item,
            Err(err) => {
                eprintln!("{:#}", err);
                std::process::exit(1)
            }
        }
    }
}

fn main() {
    if env::args().len() == 3 {
        let lift_manifest_path = PathBuf::from(
            env::args()
                .nth(1)
                .expect("We checked there were 3 args just above"),
        );
        let file_path = PathBuf::from(
            env::args()
                .nth(2)
                .expect("We checked there were 3 args just above"),
        );

        let lift_manifest = std::fs::File::open(&lift_manifest_path)
            .with_context(|| {
                format!(
                    "ailed to open lift manifest at {lift_manifest}",
                    lift_manifest = lift_manifest_path.display()
                )
            })
            .or_exit();
        fetch_manifest(&lift_manifest, &file_path, std::io::stdout()).or_exit()
    } else if env::args().len() == 2 {
        let url = env::args()
            .nth(1)
            .expect("We checked there were 2 args just above");

        fetch(url.as_str(), std::io::stdout()).or_exit()
    } else {
        usage().or_exit();
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::path::Path;

    use sha2::{Digest, Sha256};

    const URL: &str =
        "https://github.com/a-scie/jump/releases/download/v0.2.1/scie-jump-linux-aarch64";

    fn assert_fetched_buffer(buffer: &[u8]) {
        assert_eq!(1205568, buffer.len());
        assert_eq!(
            "937683255e98caf10745a674d7063bd38e9cbeb523b9f8ef4dbe8807abc35382".to_string(),
            format!("{digest:x}", digest = Sha256::digest(buffer))
        );
    }

    #[test]
    fn fetch_manifest() {
        let manifest = format!(
            r#"
{{
    "ptex": {{
        "scie-jump": "{URL}"
    }}
}}
"#
        );
        let mut buffer: Vec<u8> = Vec::new();
        super::fetch_manifest(Cursor::new(manifest), Path::new("scie-jump"), &mut buffer).unwrap();
        assert_fetched_buffer(buffer.as_slice());
    }

    #[test]
    fn fetch() {
        let mut buffer: Vec<u8> = Vec::new();
        super::fetch(URL, &mut buffer).unwrap();
        assert_fetched_buffer(buffer.as_slice());
    }
}
