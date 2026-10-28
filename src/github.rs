// Copyright 2026 Science project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::env;
use std::io::Write;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail, ensure};
use curl::easy::{Easy2, Handler, List, NetRc, WriteError};
use serde::Deserialize;
use url::Url;

use crate::FetchHandler;

const API_ORIGIN: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const API_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_METADATA_BYTES: usize = 64 * 1024;
const MAX_LOCATION_BYTES: usize = 16 * 1024;
const TOKEN_GUIDANCE: &str = "Set PTEX_GITHUB_TOKEN to a repository-scoped GitHub token with Actions: read (artifacts) or Contents: read (release assets).";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Artifact,
    ReleaseAsset,
}

#[derive(Debug, PartialEq)]
struct Download {
    repository: String,
    id: u64,
    run_id: Option<u64>,
    kind: Kind,
}

impl Download {
    fn parse(source: &str) -> Result<Self> {
        ensure!(
            source.len() <= 2048
                && source.bytes().all(|b| b.is_ascii_graphic())
                && !source.contains(['%', '?', '#', '@', '\\']),
            "Invalid GitHub download URL; queries, fragments and escaped paths are not supported"
        );
        let (host, path) = source
            .strip_prefix("https://")
            .and_then(|rest| rest.split_once('/'))
            .context("Expected an HTTPS GitHub artifact or release-asset link")?;
        let segments: Vec<_> = path.split('/').collect();
        let (owner, repo, id, run, kind) =
            match (host.to_ascii_lowercase().as_str(), segments.as_slice()) {
                ("api.github.com", ["repos", owner, repo, "actions", "artifacts", id, "zip"]) => {
                    (*owner, *repo, *id, None, Kind::Artifact)
                }
                ("api.github.com", ["repos", owner, repo, "releases", "assets", id]) => {
                    (*owner, *repo, *id, None, Kind::ReleaseAsset)
                }
                ("github.com", [owner, repo, "actions", "runs", run, "artifacts", id]) => {
                    (*owner, *repo, *id, Some(*run), Kind::Artifact)
                }
                _ => bail!(
                    "Unsupported GitHub link; use an Actions artifact link or release-asset API URL"
                ),
            };
        for component in [owner, repo] {
            ensure!(
                !component.is_empty()
                    && component.len() <= 100
                    && !matches!(component, "." | "..")
                    && component
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
                "Invalid GitHub repository name"
            );
        }
        Ok(Self {
            repository: format!("{owner}/{repo}"),
            id: positive_id(id)?,
            run_id: run.map(positive_id).transpose()?,
            kind,
        })
    }

    fn api_path(&self) -> String {
        let resource = match self.kind {
            Kind::Artifact => "actions/artifacts",
            Kind::ReleaseAsset => "releases/assets",
        };
        format!("/repos/{}/{resource}/{}", self.repository, self.id)
    }
}

fn positive_id(value: &str) -> Result<u64> {
    ensure!(
        value.bytes().all(|b| b.is_ascii_digit()),
        "Invalid GitHub resource ID"
    );
    let id = value
        .parse::<u64>()
        .map_err(|_| anyhow!("Invalid GitHub resource ID"))?;
    ensure!(id > 0, "Invalid GitHub resource ID");
    Ok(id)
}

pub(crate) fn recognizes(source: &str) -> bool {
    let Ok(url) = Url::parse(source) else {
        return false;
    };
    let path: Vec<_> = url.path().split('/').collect();
    matches!(
        (url.host_str(), path.as_slice()),
        (
            Some("api.github.com"),
            ["", "repos", _, _, "actions", "artifacts", _, "zip"]
        ) | (
            Some("api.github.com"),
            ["", "repos", _, _, "releases", "assets", _]
        ) | (
            Some("github.com"),
            ["", _, _, "actions", "runs", _, "artifacts", _]
        )
    )
}

pub(crate) fn validate_source(source: &str) -> Result<()> {
    Download::parse(source).map(|_| ())
}

pub(crate) fn fetch<W: Write>(
    source: &str,
    output: W,
    headers: Vec<String>,
    show_headers: bool,
    show_progress: bool,
) -> Result<()> {
    let download = Download::parse(source)?;
    ensure!(
        headers.is_empty(),
        "Custom headers are not supported for GitHub downloads"
    );
    let token = env::var("PTEX_GITHUB_TOKEN").map_err(|_| anyhow!(TOKEN_GUIDANCE))?;
    validate_token(&token)?;
    let client = Client {
        token: &token,
        show_headers,
        show_progress,
    };
    let api_url = format!("{API_ORIGIN}{}", download.api_path());
    if let Some(run_id) = download.run_id {
        client.verify_run(&api_url, download.id, run_id)?;
    }
    let url = match download.kind {
        Kind::Artifact => format!("{api_url}/zip"),
        Kind::ReleaseAsset => api_url,
    };
    client.download(&url, output, download.kind, validate_location)
}

fn validate_token(token: &str) -> Result<()> {
    ensure!(
        !token.is_empty()
            && token.len() <= 8192
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~+/=".contains(&b)),
        "Invalid PTEX_GITHUB_TOKEN; expected a nonempty, header-safe bearer token"
    );
    Ok(())
}

fn validate_location(location: &str) -> Result<()> {
    ensure!(
        location.len() <= MAX_LOCATION_BYTES
            && location.bytes().all(|b| b.is_ascii_graphic())
            && !location.contains(['\\', '#']),
        "Invalid GitHub download destination"
    );
    let authority = location
        .strip_prefix("https://")
        .and_then(|rest| rest.split_once('/'))
        .map(|(authority, _)| authority)
        .context("GitHub download destination must be an absolute HTTPS URL")?;
    let url = Url::parse(location).map_err(|_| anyhow!("Invalid GitHub download destination"))?;
    let host = url.host_str().context("Invalid GitHub download host")?;
    ensure!(
        authority.eq_ignore_ascii_case(host)
            || authority.eq_ignore_ascii_case(&format!("{host}:443")),
        "GitHub download destination cannot contain userinfo or non-HTTPS ports"
    );
    let azure = host
        .strip_suffix(".blob.core.windows.net")
        .is_some_and(|account| {
            (3..=24).contains(&account.len())
                && account
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
    ensure!(
        azure
            || matches!(
                host,
                "release-assets.githubusercontent.com" | "objects.githubusercontent.com"
            ),
        "Unrecognized GitHub storage host; refusing the download destination"
    );
    for (index, byte) in location.bytes().enumerate() {
        if byte == b'%' {
            ensure!(
                location
                    .as_bytes()
                    .get(index + 1..index + 3)
                    .is_some_and(|escape| escape.iter().all(u8::is_ascii_hexdigit)),
                "Invalid escaping in GitHub download destination"
            );
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Metadata,
    Artifact,
    ReleaseAsset,
    Storage,
}

impl Phase {
    fn total_timeout(self) -> Duration {
        match self {
            Self::Metadata | Self::Artifact => API_TIMEOUT,
            Self::ReleaseAsset | Self::Storage => Duration::ZERO,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Metadata => "metadata",
            Self::Artifact | Self::ReleaseAsset => "API",
            Self::Storage => "storage",
        }
    }
}

struct Client<'a> {
    token: &'a str,
    show_headers: bool,
    show_progress: bool,
}

struct Response {
    status: u32,
    location: Option<String>,
}

impl Client<'_> {
    fn verify_run(&self, url: &str, artifact_id: u64, run_id: u64) -> Result<()> {
        #[derive(Deserialize)]
        struct Run {
            id: u64,
        }
        #[derive(Deserialize)]
        struct Artifact {
            id: u64,
            workflow_run: Option<Run>,
        }
        let mut metadata = Vec::new();
        let response = self.request(url, &mut metadata, Phase::Metadata)?;
        if response.status != 200 {
            return Err(status_error(Phase::Metadata, response.status));
        }
        let artifact: Artifact = serde_json::from_slice(&metadata)
            .map_err(|_| anyhow!("Invalid GitHub artifact metadata"))?;
        ensure!(
            artifact.id == artifact_id && artifact.workflow_run.is_some_and(|run| run.id == run_id),
            "GitHub artifact does not belong to the workflow run in the link"
        );
        Ok(())
    }

    fn download<W: Write>(
        &self,
        url: &str,
        mut output: W,
        kind: Kind,
        check_location: impl Fn(&str) -> Result<()>,
    ) -> Result<()> {
        let phase = match kind {
            Kind::Artifact => Phase::Artifact,
            Kind::ReleaseAsset => Phase::ReleaseAsset,
        };
        let response = self.request(url, &mut output, phase)?;
        if response.status == 302 {
            let location = response
                .location
                .context("GitHub API did not provide a download Location")?;
            check_location(&location)?;
            // Never reuse the authenticated handle or generic fetch(): storage receives no auth,
            // cookies, custom headers or netrc credentials, and its signed URL is never displayed.
            let response = self.request(&location, &mut output, Phase::Storage)?;
            if response.status != 200 {
                return Err(status_error(Phase::Storage, response.status));
            }
        } else if response.status != 200 || kind != Kind::ReleaseAsset {
            return Err(status_error(phase, response.status));
        }
        output.flush().context("Failed to flush GitHub download")
    }

    fn request<W: Write>(&self, url: &str, output: W, phase: Phase) -> Result<Response> {
        let mut easy = self
            .handle(url, output, phase)
            .map_err(|error| transport_error(phase, error))?;
        let result = easy.perform();
        let handler = easy.get_mut();
        if let Some(error) = handler.output_error.take() {
            return Err(error).context("Failed to write GitHub download");
        }
        if let Some(error) = handler.response_error {
            bail!(error);
        }
        if let Err(error) = result
            && !(handler.discarded_body && error.is_write_error())
        {
            return Err(transport_error(phase, error));
        }
        Ok(Response {
            status: handler.status,
            location: handler.location.take(),
        })
    }

    fn handle<W: Write>(
        &self,
        url: &str,
        output: W,
        phase: Phase,
    ) -> std::result::Result<Easy2<GithubHandler<W>>, curl::Error> {
        let mut easy = Easy2::new(GithubHandler {
            inner: FetchHandler::new(
                "GitHub artifact",
                output,
                self.show_headers,
                self.show_progress && phase != Phase::Metadata,
            ),
            phase,
            status: 0,
            location: None,
            header_bytes: 0,
            body_bytes: 0,
            output_error: None,
            response_error: None,
            discarded_body: false,
        });
        easy.url(url)?;
        easy.path_as_is(true)?;
        easy.follow_location(false)?;
        easy.unrestricted_auth(false)?;
        easy.netrc(NetRc::Ignored)?;
        easy.proxy_headers(List::new())?;
        easy.ssl_verify_peer(true)?;
        easy.ssl_verify_host(true)?;
        easy.connect_timeout(CONNECT_TIMEOUT)?;
        easy.low_speed_limit(1)?;
        easy.low_speed_time(CONNECT_TIMEOUT)?;
        easy.progress(true)?;
        easy.useragent(&format!("ptex/{}", env!("CARGO_PKG_VERSION")))?;
        easy.timeout(phase.total_timeout())?;
        if phase != Phase::Storage {
            let mut headers = List::new();
            headers.append(&format!("Authorization: Bearer {}", self.token))?;
            headers.append(&format!("X-GitHub-Api-Version: {API_VERSION}"))?;
            headers.append(if phase == Phase::ReleaseAsset {
                "Accept: application/octet-stream"
            } else {
                "Accept: application/vnd.github+json"
            })?;
            easy.http_headers(headers)?;
        }
        Ok(easy)
    }
}

fn transport_error(phase: Phase, error: curl::Error) -> anyhow::Error {
    // libcurl's error buffer can contain a signed URL. Do not retain it in the error chain.
    anyhow!(
        "GitHub {} request failed (curl error {})",
        phase.label(),
        error.code()
    )
}

fn status_error(phase: Phase, status: u32) -> anyhow::Error {
    let hint = match (phase, status) {
        (Phase::Storage, _) => {
            "The signed download failed; no further redirects or retries are attempted."
        }
        (_, 401) => "Check PTEX_GITHUB_TOKEN.",
        (_, 403 | 429) => "Check repository permissions, SSO authorization and API rate limits.",
        (_, 404) => "The resource may be missing or inaccessible to this token.",
        (_, 410) => "The artifact is gone or expired; check its retention period.",
        _ => "Expected a documented GitHub download response.",
    };
    anyhow!("GitHub {} failed with HTTP {status}. {hint}", phase.label())
}

struct GithubHandler<W: Write> {
    inner: FetchHandler<W>,
    phase: Phase,
    status: u32,
    location: Option<String>,
    header_bytes: usize,
    body_bytes: usize,
    output_error: Option<std::io::Error>,
    response_error: Option<&'static str>,
    discarded_body: bool,
}

impl<W: Write> Handler for GithubHandler<W> {
    fn write(&mut self, data: &[u8]) -> std::result::Result<usize, WriteError> {
        if self.status != 200 || self.phase == Phase::Artifact {
            // Stop rather than consuming arbitrarily large API/error bodies.
            self.discarded_body = true;
            return Ok(0);
        }
        if self.phase == Phase::Metadata && data.len() > MAX_METADATA_BYTES - self.body_bytes {
            self.response_error = Some("GitHub metadata exceeds the size limit");
            return Ok(0);
        }
        self.body_bytes += data.len();
        match self.inner.output.write_all(data) {
            Ok(()) => Ok(data.len()),
            Err(error) => {
                self.output_error = Some(error);
                Ok(0)
            }
        }
    }

    fn header(&mut self, data: &[u8]) -> bool {
        self.header_bytes += data.len();
        if self.header_bytes > MAX_HEADER_BYTES {
            self.response_error = Some("GitHub response headers exceed the size limit");
            return false;
        }
        if data.starts_with(b"HTTP/") {
            self.status = std::str::from_utf8(data)
                .ok()
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|code| code.parse().ok())
                .unwrap_or(0);
            self.location = None;
            if self.inner.show_headers {
                eprintln!("GitHub {} HTTP status: {}", self.phase.label(), self.status);
            }
        } else if data
            .get(..9)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"location:"))
        {
            let location = std::str::from_utf8(&data[9..])
                .ok()
                .map(|value| value.trim_matches([' ', '\t', '\r', '\n']));
            match location {
                Some(value)
                    if !value.is_empty()
                        && value.len() <= MAX_LOCATION_BYTES
                        && self.location.is_none() =>
                {
                    self.location = Some(value.to_string())
                }
                _ => {
                    self.response_error = Some("Invalid or duplicate GitHub download Location");
                    return false;
                }
            }
        }
        true
    }

    fn progress(&mut self, total: f64, now: f64, upload_total: f64, upload_now: f64) -> bool {
        self.inner.progress(total, now, upload_total, upload_now)
    }
}

#[cfg(test)]
mod tests;
