use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use curl::easy::{Easy2, Handler, NetRc, WriteError};
use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    ptex: BTreeMap<PathBuf, String>,
}

impl Config {
    fn parse<R: Read>(reader: R) -> Result<Self, String> {
        let config: Self = serde_json::from_reader(reader)
            .map_err(|e| format!("Failed to parse ptex config: {e}"))?;
        Ok(config)
    }
}

struct FetchHandler<'a, W: Write> {
    url: &'a str,
    output: W,
    last_seen: f64,
}

impl<'a, W: Write> FetchHandler<'a, W> {
    fn new(url: &'a str, output: W) -> Self {
        Self {
            url,
            output,
            last_seen: 0.0,
        }
    }
}

impl<'a, W: Write> Handler for FetchHandler<'a, W> {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.output
            .write_all(data)
            .expect("Failed to write data to output");
        Ok(data.len())
    }

    fn progress(&mut self, dltotal: f64, dlnow: f64, _ultotal: f64, _ulnow: f64) -> bool {
        if dlnow != self.last_seen {
            eprint!(
                "\rDownloaded {dlnow} of {total} from {url}",
                total = if dltotal > 0.0 {
                    format!(
                        "{dltotal} bytes ({percent:>3.0}%)",
                        percent = dlnow / dltotal * 100.0
                    )
                } else {
                    "unknown".to_string()
                },
                url = self.url,
            );
            self.last_seen = dlnow;
            if dlnow == dltotal {
                eprintln!()
            }
        }
        true
    }
}

fn fetch<R: Read, W: Write>(lift_manifest: R, file_path: &Path, output: W) -> Result<(), String> {
    let config = Config::parse(lift_manifest)?;
    let url = config.ptex.get(file_path).ok_or_else(|| {
        format!(
            "Did not find an URL mapping for file {path}.",
            path = file_path.display()
        )
    })?;

    let mut easy = Easy2::new(FetchHandler::new(url, output));
    easy.follow_location(true)
        .map_err(|e| format!("Failed to configure re-direct following: {e}"))?;
    easy.fail_on_error(true)
        .map_err(|e| format!("Failed to configure fail on error behavior: {e}"))?;
    easy.netrc(NetRc::Optional)
        .map_err(|e| format!("Failed to enable ~/.netrc parsing: {e}"))?;
    easy.url(url)
        .map_err(|e| format!("Failed to configure URL to fetch from as {url}: {e}"))?;
    easy.progress(true)
        .map_err(|e| format!("Failed to enable progress meter: {e}"))?;
    easy.perform().map_err(|e| {
        format!(
            "Failed to fetch {file} from {url}: {e}",
            file = file_path.display()
        )
    })
}

fn usage() -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Failed to determine current executable: {e}"))?;
    eprintln!(
        "Usage: {current_exe} [lift manifest path] [file path]",
        current_exe = current_exe.display()
    );
    Ok(())
}

fn main() -> Result<(), String> {
    if std::env::args().len() != 3 {
        usage()?;
        std::process::exit(1);
    }
    let lift_manifest_path = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("We checked there were 3 args just above"),
    );
    let file_path = PathBuf::from(
        std::env::args()
            .nth(2)
            .expect("We checked there were 3 args just above"),
    );

    let lift_manifest = std::fs::File::open(&lift_manifest_path).map_err(|e| {
        format!(
            "ailed to open lift manifest at {lift_manifest}: {e}",
            lift_manifest = lift_manifest_path.display()
        )
    })?;
    fetch(&lift_manifest, &file_path, std::io::stdout())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::path::Path;

    use sha2::{Digest, Sha256};

    #[test]
    fn fetch() {
        let manifest = r#"
{
    "ptex": {
        "scie-jump":
            "https://github.com/a-scie/jump/releases/download/v0.2.1/scie-jump-linux-aarch64"
    }
}
"#;
        let mut buffer: Vec<u8> = Vec::new();
        super::fetch(Cursor::new(manifest), Path::new("scie-jump"), &mut buffer).unwrap();
        assert_eq!(1205568, buffer.len());
        assert_eq!(
            "937683255e98caf10745a674d7063bd38e9cbeb523b9f8ef4dbe8807abc35382".to_string(),
            format!("{digest:x}", digest = Sha256::digest(buffer))
        );
    }
}
