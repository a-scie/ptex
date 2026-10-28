// Copyright 2026 Science project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

#[cfg(test)]
use std::future::Future;
use std::io::Write;
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, ensure};
use curl::easy::{Easy2, Handler, List, NetRc, WriteError};
use google_cloud_auth::credentials::{AccessToken, Builder};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

use crate::FetchHandler;

const STORAGE_SCOPE: &str = "https://www.googleapis.com/auth/devstorage.read_only";
const AUTH_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const ADC_GUIDANCE: &str = "Could not obtain a Google access token. Configure Application Default Credentials using GOOGLE_APPLICATION_CREDENTIALS, gcloud auth application-default login, or an attached service account with storage.objects.get permission.";

pub(crate) fn fetch<W: Write>(
    source: &str,
    output: W,
    headers: Vec<String>,
    show_headers: bool,
    show_progress: bool,
) -> Result<()> {
    let url = object_url(source)?;
    validate_headers(&headers)?;
    let token = access_token()?;
    fetch_with_token(
        &url,
        output,
        headers,
        show_headers,
        show_progress,
        &token.token,
    )
}

fn object_url(source: &str) -> Result<String> {
    let (bucket, path) = source
        .strip_prefix("gs://")
        .and_then(|reference| reference.split_once('/'))
        .context("Expected a GCS object URL: gs://bucket/object")?;
    ensure!(
        (3..=222).contains(&bucket.len())
            && bucket.split('.').all(|part| {
                !part.is_empty()
                    && part.len() <= 63
                    && part.starts_with(|c: char| c.is_ascii_alphanumeric())
                    && part.ends_with(|c: char| c.is_ascii_alphanumeric())
                    && part.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'
                    })
            }),
        "Invalid GCS bucket name; userinfo and ports are not supported"
    );
    ensure!(
        !path.contains(['?', '#']),
        "Encode '?' and '#' in GCS object names; query strings and fragments are not supported"
    );
    for (offset, byte) in path.bytes().enumerate() {
        if byte == b'%' {
            ensure!(
                path.as_bytes()
                    .get(offset + 1..offset + 3)
                    .is_some_and(|escape| escape.iter().all(u8::is_ascii_hexdigit)),
                "Invalid percent escape in GCS object name"
            );
        }
    }
    let object = percent_decode_str(path)
        .decode_utf8()
        .context("GCS object names must be UTF-8")?;
    ensure!(
        !object.is_empty()
            && object.len() <= 1024
            && object != "."
            && object != ".."
            && !object.chars().any(char::is_control),
        "Invalid GCS object name"
    );
    let object = utf8_percent_encode(&object, NON_ALPHANUMERIC);
    Ok(format!(
        "https://storage.googleapis.com/download/storage/v1/b/{bucket}/o/{object}?alt=media"
    ))
}

fn validate_headers(headers: &[String]) -> Result<()> {
    for header in headers {
        ensure!(
            !header.chars().any(char::is_control),
            "Control characters are not allowed in GCS request headers"
        );
        let name = header.split([':', ';']).next().unwrap_or("").trim();
        ensure!(
            !name.is_empty()
                && !["authorization", "proxy-authorization", "host"]
                    .iter()
                    .any(|reserved| name.eq_ignore_ascii_case(reserved)),
            "GCS requests cannot override authentication or host headers"
        );
    }
    Ok(())
}

fn validate_token(token: &str) -> Result<()> {
    ensure!(
        !token.is_empty()
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~+/=".contains(&b)),
        "Google returned an invalid access token"
    );
    Ok(())
}

fn access_token() -> Result<AccessToken> {
    run_with_deadline(
        || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("Failed to initialize Google authentication")?;
            runtime.block_on(async {
                let credentials = Builder::default()
                    .with_scopes([STORAGE_SCOPE])
                    .build_access_token_credentials()?;
                Ok(credentials.access_token().await?)
            })
        },
        AUTH_TIMEOUT,
    )
    .and_then(|token| {
        validate_token(&token.token)?;
        Ok(token)
    })
}

fn run_with_deadline<T, F>(operation: F, timeout: Duration) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    let (sender, receiver) = sync_channel(1);
    thread::spawn(move || {
        // Do not send the provider's error across the deadline boundary: SDK errors can contain
        // credential-provider response bodies. The caller only receives the sanitized guidance.
        let result = operation().map_err(|_| ());
        let _ = sender.send(result);
    });
    match receiver.recv_timeout(timeout) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(())) | Err(RecvTimeoutError::Disconnected) => Err(anyhow!(ADC_GUIDANCE)),
        Err(RecvTimeoutError::Timeout) => Err(anyhow!(
            "Timed out obtaining Google Application Default Credentials"
        )),
    }
}

#[cfg(test)]
async fn acquire_token(
    future: impl Future<Output = Result<AccessToken>>,
    timeout: Duration,
) -> Result<AccessToken> {
    let token = tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| anyhow!("Timed out obtaining Google Application Default Credentials"))?
        // SDK error chains may include credential responses; do not print them.
        .map_err(|_| anyhow!(ADC_GUIDANCE))?;
    validate_token(&token.token)?;
    Ok(token)
}

struct GcsHandler<W: Write> {
    inner: FetchHandler<W>,
    status: u32,
    output_error: Option<std::io::Error>,
}

impl<W: Write> Handler for GcsHandler<W> {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        if self.status != 200 {
            return Ok(data.len());
        }
        // A short write aborts libcurl without panicking or retrying a partial artifact.
        match self.inner.output.write_all(data) {
            Ok(()) => Ok(data.len()),
            Err(error) => {
                self.output_error = Some(error);
                Ok(0)
            }
        }
    }

    fn header(&mut self, data: &[u8]) -> bool {
        if let Ok(line) = std::str::from_utf8(data)
            && line.starts_with("HTTP/")
        {
            self.status = line
                .split_whitespace()
                .nth(1)
                .and_then(|code| code.parse().ok())
                .unwrap_or(0);
            // Only dump status, not arbitrary server headers that could echo credentials.
            if self.inner.show_headers {
                eprintln!("GCS HTTP status: {}", self.status);
            }
        }
        true
    }

    fn progress(&mut self, total: f64, now: f64, upload_total: f64, upload_now: f64) -> bool {
        self.inner.progress(total, now, upload_total, upload_now)
    }
}

fn fetch_with_token<W: Write>(
    url: &str,
    output: W,
    headers: Vec<String>,
    show_headers: bool,
    show_progress: bool,
    token: &str,
) -> Result<()> {
    validate_headers(&headers)?;
    validate_token(token)?;
    let mut easy = Easy2::new(GcsHandler {
        inner: FetchHandler::new(url, output, show_headers, show_progress),
        status: 0,
        output_error: None,
    });
    easy.url(url)?;
    easy.follow_location(false)?;
    easy.netrc(NetRc::Ignored)?;
    easy.connect_timeout(CONNECT_TIMEOUT)?;
    easy.low_speed_limit(1)?;
    easy.low_speed_time(CONNECT_TIMEOUT)?;
    easy.progress(true)?;
    easy.useragent(&format!("ptex/{}", env!("CARGO_PKG_VERSION")))?;
    let mut curl_headers = List::new();
    for header in headers {
        curl_headers
            .append(&header)
            .context("Invalid GCS request header")?;
    }
    curl_headers
        .append(&format!("Authorization: Bearer {token}"))
        .context("Failed to set GCS authentication")?;
    easy.http_headers(curl_headers)?;
    let perform = easy.perform();
    if let Some(error) = easy.get_mut().output_error.take() {
        return Err(error).context("Failed to write downloaded GCS object");
    }
    perform.context("Failed to download GCS object")?;
    let status = easy.response_code()?;
    ensure!(
        status == 200,
        "GCS download failed with HTTP {status}; verify the object exists and ADC has storage.objects.get permission. Redirects are not followed."
    );
    easy.get_mut()
        .inner
        .output
        .flush()
        .context("Failed to flush downloaded GCS object")
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    use super::*;

    fn serve(status: &str, headers: &str, body: &[u8]) -> (String, JoinHandle<String>) {
        let response = [
            format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
                body.len()
            )
            .into_bytes(),
            body.to_vec(),
        ]
        .concat();
        serve_response(response)
    }

    fn serve_response(response: Vec<u8>) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            loop {
                let count = stream.read(&mut buf).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buf[..count]);
                if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    break;
                }
            }
            stream.write_all(&response).unwrap();
            String::from_utf8(request).unwrap()
        });
        (url, thread)
    }

    #[test]
    fn streams_authenticated_binary_objects() {
        let body = [0, 255, 128, b'x'].repeat(262_144);
        let (url, server) = serve("200 OK", "", &body);
        let mut output = Vec::new();
        fetch_with_token(
            &url,
            &mut output,
            vec!["X-Test: test".into()],
            false,
            false,
            "synthetic.token",
        )
        .unwrap();
        assert_eq!(body, output);
        let request = server.join().unwrap();
        assert_eq!(1, request.matches("Authorization:").count());
        assert!(request.contains("Authorization: Bearer synthetic.token\r\n"));
        assert!(request.contains("X-Test: test\r\n"));
    }

    #[test]
    fn empty_objects_are_valid() {
        let (url, server) = serve("200 OK", "", b"");
        let mut output = Vec::new();
        fetch_with_token(&url, &mut output, vec![], false, false, "synthetic.token").unwrap();
        assert!(output.is_empty());
        server.join().unwrap();
    }

    #[test]
    fn errors_redirects_and_partial_responses_emit_no_object_bytes() {
        for status in [
            "301 Moved",
            "302 Found",
            "303 SeeOther",
            "307 Temporary",
            "308 Permanent",
            "401 Unauthorized",
            "403 Forbidden",
            "404 NotFound",
            "500 ServerError",
            "206 Partial",
        ] {
            let (url, server) = serve(
                status,
                "Location: http://127.0.0.1:1/never-fetch\r\nX-Echo: synthetic.token\r\n",
                b"secret error response",
            );
            let mut output = Vec::new();
            let error = fetch_with_token(&url, &mut output, vec![], true, false, "synthetic.token")
                .unwrap_err();
            assert!(output.is_empty());
            let message = format!("{error:#}");
            assert!(message.contains(&status[..3]), "{message}");
            assert!(!message.contains("synthetic.token"));
            assert!(!message.contains("secret error response"));
            server.join().unwrap();
        }
    }

    #[test]
    fn truncated_objects_fail_without_replaying_partial_bytes() {
        let (url, server) = serve_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\nincomplete"
                .to_vec(),
        );
        let mut output = Vec::new();
        let error = fetch_with_token(&url, &mut output, vec![], false, true, "synthetic.token")
            .unwrap_err();
        assert_eq!(b"incomplete", output.as_slice());
        assert!(error.to_string().contains("Failed to download"));
        assert!(!format!("{error:#}").contains("synthetic.token"));
        server.join().unwrap();
    }

    #[test]
    fn output_errors_are_reported_without_panicking() {
        struct BrokenWriter;
        impl Write for BrokenWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("closed output"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (url, server) = serve("200 OK", "", b"artifact");
        let error = fetch_with_token(&url, BrokenWriter, vec![], false, false, "synthetic.token")
            .unwrap_err();
        assert!(format!("{error:#}").contains("closed output"));
        server.join().unwrap();
    }

    #[test]
    fn flush_errors_are_reported_after_successful_downloads() {
        struct FlushErrorWriter;
        impl Write for FlushErrorWriter {
            fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
                Ok(data.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("flush failed"))
            }
        }
        let (url, server) = serve("200 OK", "", b"artifact");
        let error = fetch_with_token(
            &url,
            FlushErrorWriter,
            vec![],
            false,
            false,
            "synthetic.token",
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("flush failed"));
        server.join().unwrap();
    }

    #[test]
    fn blocking_authentication_work_respects_the_deadline() {
        let (release, blocker) = std::sync::mpsc::sync_channel(0);
        let error = run_with_deadline(
            move || {
                blocker.recv().unwrap();
                Ok(AccessToken {
                    token: "synthetic.token".into(),
                })
            },
            Duration::ZERO,
        )
        .unwrap_err();
        assert!(error.to_string().contains("Timed out"));
        release.send(()).unwrap();
    }

    #[test]
    fn authentication_worker_sanitizes_provider_errors() {
        let error = run_with_deadline(
            || Err::<AccessToken, _>(anyhow!("synthetic-provider-secret")),
            AUTH_TIMEOUT,
        )
        .unwrap_err();
        assert_eq!(ADC_GUIDANCE, error.to_string());
        assert!(!format!("{error:#}").contains("synthetic-provider-secret"));
        let token = run_with_deadline(
            || {
                Ok(AccessToken {
                    token: "synthetic.token".into(),
                })
            },
            AUTH_TIMEOUT,
        )
        .unwrap();
        assert_eq!("synthetic.token", token.token);
    }

    #[test]
    fn auth_errors_and_timeouts_do_not_disclose_provider_responses() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let error = runtime
            .block_on(acquire_token(
                async { Err(anyhow!("synthetic-refresh-secret")) },
                AUTH_TIMEOUT,
            ))
            .unwrap_err();
        assert_eq!(ADC_GUIDANCE, error.to_string());
        assert!(!format!("{error:#}").contains("synthetic-refresh-secret"));
        let error = runtime
            .block_on(acquire_token(std::future::pending(), Duration::ZERO))
            .unwrap_err();
        assert!(error.to_string().contains("Timed out"));
        let token = runtime
            .block_on(acquire_token(
                async {
                    Ok(AccessToken {
                        token: "synthetic.token".into(),
                    })
                },
                AUTH_TIMEOUT,
            ))
            .unwrap();
        assert_eq!("synthetic.token", token.token);
        assert!(
            runtime
                .block_on(acquire_token(
                    async {
                        Ok(AccessToken {
                            token: "invalid token".into(),
                        })
                    },
                    AUTH_TIMEOUT
                ))
                .is_err()
        );
    }

    #[test]
    fn google_auth_adapter_refreshes_user_credentials() {
        let (url, server) = serve(
            "200 OK",
            "Content-Type: application/json\r\n",
            br#"{"access_token":"synthetic.token","expires_in":3600,"token_type":"Bearer"}"#,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let token = runtime.block_on(async {
            let credentials = google_cloud_auth::credentials::user_account::Builder::new(serde_json::json!({
                "type": "authorized_user", "client_id": "synthetic-client", "client_secret": "synthetic-secret", "refresh_token": "synthetic-refresh"
            })).with_token_uri(url).with_scopes([STORAGE_SCOPE]).build_access_token_credentials().unwrap();
            acquire_token(async { Ok(credentials.access_token().await?) }, AUTH_TIMEOUT).await.unwrap()
        });
        assert_eq!("synthetic.token", token.token);
        assert!(server.join().unwrap().starts_with("POST "));
    }

    #[test]
    fn object_urls_preserve_object_names() {
        for (source, object) in [
            ("gs://test-bucket/pants.pex", "pants%2Epex"),
            (
                "gs://test-bucket/releases/a+b c.pex",
                "releases%2Fa%2Bb%20c%2Epex",
            ),
            ("gs://test-bucket/a%2Fb%252Fc", "a%2Fb%252Fc"),
            ("gs://test-bucket/a/../b", "a%2F%2E%2E%2Fb"),
            ("gs://test-bucket/a//b", "a%2F%2Fb"),
            ("gs://test-bucket/%3F%23", "%3F%23"),
            ("gs://test-bucket/caf\u{e9}", "caf%C3%A9"),
        ] {
            assert_eq!(
                format!(
                    "https://storage.googleapis.com/download/storage/v1/b/test-bucket/o/{object}?alt=media"
                ),
                object_url(source).unwrap()
            );
        }
    }

    #[test]
    fn invalid_object_urls_are_rejected() {
        for source in [
            "gs://",
            "gs://bucket",
            "gs:///object",
            "gs://bucket/",
            "gs://user@bucket/object",
            "gs://bucket:443/object",
            "gs://bucket/object?generation=1",
            "gs://bucket/object#fragment",
            "gs://bucket/%",
            "gs://bucket/%GG",
            "gs://bucket/%FF",
            "gs://bucket/%0a",
            "gs://bucket/line\nbreak",
            "gs://bucket/\tobject",
            "gs://BUCKET/object",
            "gs://-bucket/object",
            "gs://bucket../object",
            "gs://bucket/.",
            "gs://bucket/..",
            "gs://bucket/%2e",
            "https://storage.googleapis.com/bucket/object",
        ] {
            assert!(object_url(source).is_err(), "accepted {source:?}");
        }
        assert!(object_url(&format!("gs://bucket/{}", "a".repeat(1025))).is_err());
    }

    #[test]
    fn caller_cannot_override_credentials_or_destination() {
        for header in [
            "Authorization: Bearer caller",
            "authorization: caller",
            "AUTHORIZATION;",
            "Host: attacker.invalid",
            "Host;",
            "Proxy-Authorization: secret",
            ":authority: attacker.invalid",
            "X-Test: safe\r\nAuthorization: injected",
            "X-Test: a\0b",
        ] {
            let error = validate_headers(&[header.to_string()]).unwrap_err();
            assert!(!format!("{error:#}").contains(header));
        }
        validate_headers(&["Accept: application/octet-stream".into()]).unwrap();
    }

    #[test]
    fn bearer_values_are_validated_without_echoing_them() {
        for token in [
            "",
            "secret\r\nX-Test: injected",
            "secret token",
            "secret\0value",
            "secret\u{e9}",
        ] {
            let error = validate_token(token).unwrap_err();
            if !token.is_empty() {
                assert!(!format!("{error:#}").contains(token));
            }
        }
        validate_token("test.token-_~+/=").unwrap();
    }
}
