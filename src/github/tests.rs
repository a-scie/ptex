// Copyright 2026 Science project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::io::Read;
use std::net::TcpListener;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::*;

const ARTIFACT: &str = "https://api.github.com/repos/example/builds/actions/artifacts/123/zip";
const BROWSER: &str = "https://github.com/example/builds/actions/runs/456/artifacts/123";
const ASSET: &str = "https://api.github.com/repos/example/builds/releases/assets/123";

fn client() -> Client<'static> {
    Client {
        token: "synthetic.token",
        show_headers: true,
        show_progress: true,
    }
}

fn allow_loopback(url: &str) -> Result<()> {
    let url = Url::parse(url)?;
    ensure!(
        url.scheme() == "http" && url.host_str() == Some("127.0.0.1"),
        "Expected loopback fixture"
    );
    Ok(())
}

fn serve(status: &str, headers: &str, body: &[u8]) -> (String, JoinHandle<String>) {
    serve_response(
        [
            format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
                body.len()
            )
            .into_bytes(),
            body.to_vec(),
        ]
        .concat(),
    )
}

fn serve_response(response: Vec<u8>) -> (String, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buf = [0; 1024];
        while !request.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
            let read = stream.read(&mut buf).unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buf[..read]);
            assert!(request.len() < 16384);
        }
        // The client deliberately stops reading errors and oversized responses.
        if let Err(error) = stream.write_all(&response) {
            assert!(matches!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
            ));
        }
        String::from_utf8(request).unwrap()
    });
    (url, server)
}

#[test]
fn api_redirects_stream_exact_bytes_without_forwarding_credentials() {
    for kind in [Kind::Artifact, Kind::ReleaseAsset] {
        let bytes = b"PK\x03\x04synthetic-archive\0\xff".repeat(100_000);
        let (storage, storage_server) = serve("200 OK", "X-Echo: synthetic.token\r\n", &bytes);
        let location = format!("{storage}/payload%2Bname?sig=a%2Fb+c&se=x&sp=r");
        let (api, api_server) = serve(
            "302 Found",
            &format!("lOcAtIoN: {location}\r\nSet-Cookie: synthetic-cookie\r\n"),
            b"secret redirect body",
        );
        let mut output = Vec::new();
        client()
            .download(&api, &mut output, kind, allow_loopback)
            .unwrap();
        assert_eq!(bytes, output);
        let request = api_server.join().unwrap().to_lowercase();
        assert!(request.starts_with("get / http/"));
        assert_eq!(1, request.matches("authorization:").count());
        assert!(request.contains("authorization: bearer synthetic.token\r\n"));
        assert!(request.contains("x-github-api-version: 2022-11-28\r\n"));
        let accept = if kind == Kind::Artifact {
            "application/vnd.github+json"
        } else {
            "application/octet-stream"
        };
        assert!(request.contains(&format!("accept: {accept}\r\n")));
        let request = storage_server.join().unwrap();
        assert!(request.starts_with("GET /payload%2Bname?sig=a%2Fb+c&se=x&sp=r HTTP/"));
        let request = request.to_lowercase();
        for secret_header in [
            "authorization:",
            "cookie:",
            "referer:",
            "x-github-api-version:",
            "synthetic.token",
        ] {
            assert!(
                !request.contains(secret_header),
                "unexpected header {secret_header}"
            );
        }
    }
}

#[test]
fn release_assets_can_stream_directly_including_empty_files() {
    for body in [b"".as_slice(), b"#!raw-pex\0\xff".as_slice()] {
        let (api, server) = serve("200 OK", "", body);
        let mut output = Vec::new();
        client()
            .download(&api, &mut output, Kind::ReleaseAsset, allow_loopback)
            .unwrap();
        assert_eq!(body, output);
        server.join().unwrap();
    }
}

#[test]
fn binary_streams_have_no_fixed_total_deadline() {
    assert_eq!(Duration::ZERO, Phase::ReleaseAsset.total_timeout());
    assert_eq!(Duration::ZERO, Phase::Storage.total_timeout());
    assert_eq!(API_TIMEOUT, Phase::Metadata.total_timeout());
    assert_eq!(API_TIMEOUT, Phase::Artifact.total_timeout());
}

#[test]
fn api_errors_and_unexpected_statuses_never_emit_bodies() {
    for status in [
        200, 204, 206, 301, 303, 304, 307, 308, 400, 401, 403, 404, 410, 422, 429, 500,
    ] {
        let (api, server) = serve(
            &format!("{status} synthetic.token"),
            "Location: https://attacker.invalid/signed-secret\r\nX-Echo: synthetic.token\r\n",
            b"secret API body",
        );
        let mut output = Vec::new();
        let error = client()
            .download(&api, &mut output, Kind::Artifact, allow_loopback)
            .unwrap_err();
        assert!(output.is_empty());
        let error = format!("{error:#}");
        assert!(error.contains(&status.to_string()));
        assert!(!error.contains("synthetic.token"));
        assert!(!error.contains("signed-secret"));
        assert!(!error.contains("secret API body"));
        server.join().unwrap();
    }
}

#[test]
fn missing_duplicate_and_unapproved_locations_fail_closed() {
    for headers in [
        "",
        "Location: \r\n",
        "Location: https://release-assets.githubusercontent.com/a\r\nLocation: https://release-assets.githubusercontent.com/b\r\n",
        "Location: https://attacker.invalid/signed-secret\r\n",
        "Location: /signed-secret\r\n",
        "Location: http://127.0.0.1:1/signed-secret\r\n",
    ] {
        let (api, server) = serve("302 Found", headers, b"secret redirect body");
        let mut output = Vec::new();
        let error = client()
            .download(&api, &mut output, Kind::Artifact, validate_location)
            .unwrap_err();
        assert!(output.is_empty());
        let error = format!("{error:#}");
        assert!(!error.contains("signed-secret"));
        assert!(!error.contains("secret redirect body"));
        server.join().unwrap();
    }
}

#[test]
fn storage_errors_and_further_redirects_never_emit_bodies() {
    for status in [
        204, 206, 301, 302, 303, 304, 307, 308, 401, 403, 404, 410, 429, 500,
    ] {
        let (storage, storage_server) = serve(
            &format!("{status} synthetic.token"),
            "Location: http://127.0.0.1:1/signed-secret\r\n",
            b"secret storage body",
        );
        let (api, api_server) = serve(
            "302 Found",
            &format!("Location: {storage}?sig=signed-secret\r\n"),
            b"",
        );
        let mut output = Vec::new();
        let error = client()
            .download(&api, &mut output, Kind::Artifact, allow_loopback)
            .unwrap_err();
        assert!(output.is_empty());
        let error = format!("{error:#}");
        assert!(error.contains("storage"));
        assert!(error.contains(&status.to_string()));
        assert!(!error.contains("synthetic.token"));
        assert!(!error.contains("signed-secret"));
        assert!(!error.contains("secret storage body"));
        api_server.join().unwrap();
        storage_server.join().unwrap();
    }
}

#[test]
fn truncated_downloads_fail_without_replaying_partial_bytes() {
    let (storage, storage_server) = serve_response(
        b"HTTP/1.1 200 OK\r\nContent-Length: 30\r\nConnection: close\r\n\r\nincomplete".to_vec(),
    );
    let (api, api_server) = serve(
        "302 Found",
        &format!("Location: {storage}?sig=signed-secret\r\n"),
        b"",
    );
    let mut output = Vec::new();
    let error = client()
        .download(&api, &mut output, Kind::Artifact, allow_loopback)
        .unwrap_err();
    assert_eq!(b"incomplete", output.as_slice());
    assert!(error.to_string().contains("curl"));
    assert!(!format!("{error:#}").contains("signed-secret"));
    api_server.join().unwrap();
    storage_server.join().unwrap();
}

#[test]
fn output_write_and_flush_failures_are_preserved() {
    struct BrokenWriter(bool);
    impl Write for BrokenWriter {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            if self.0 {
                Ok(data.len())
            } else {
                Err(std::io::Error::other("closed output"))
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("flush failed"))
        }
    }
    for (flush, expected) in [(false, "closed output"), (true, "flush failed")] {
        let (api, server) = serve("200 OK", "", b"artifact");
        let error = client()
            .download(
                &api,
                BrokenWriter(flush),
                Kind::ReleaseAsset,
                allow_loopback,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(expected));
        server.join().unwrap();
    }
}

#[test]
fn browser_links_verify_artifact_and_workflow_run_ids() {
    for (body, valid) in [
        (br#"{"id":123,"workflow_run":{"id":456},"archive_download_url":"https://attacker.invalid/ignored"}"#.as_slice(), true),
        (br#"{"id":124,"workflow_run":{"id":456}}"#.as_slice(), false),
        (br#"{"id":123,"workflow_run":{"id":457}}"#.as_slice(), false),
        (br#"{"id":123}"#.as_slice(), false),
        (br#"{"id":123,"workflow_run":null}"#.as_slice(), false),
        (b"synthetic-private-body".as_slice(), false),
    ] {
        let (api, server) = serve("200 OK", "", body);
        let result = client().verify_run(&api, 123, 456);
        assert_eq!(valid, result.is_ok());
        if let Err(error) = result {
            assert!(!format!("{error:#}").contains("synthetic-private-body"));
        }
        let request = server.join().unwrap();
        assert!(request.contains("Authorization: Bearer synthetic.token"));
        assert!(request.contains("Accept: application/vnd.github+json"));
    }
}

#[test]
fn metadata_and_headers_have_bounded_sizes() {
    let (api, server) = serve("200 OK", "", &vec![b'x'; MAX_METADATA_BYTES + 1]);
    let error = client().verify_run(&api, 123, 456).unwrap_err();
    assert!(error.to_string().contains("metadata"));
    server.join().unwrap();
    let (api, server) = serve(
        "302 Found",
        &format!("X-Large: {}\r\n", "x".repeat(MAX_HEADER_BYTES)),
        b"",
    );
    let error = client()
        .download(&api, Vec::new(), Kind::Artifact, allow_loopback)
        .unwrap_err();
    assert!(error.to_string().contains("headers"));
    server.join().unwrap();
}

#[test]
fn informational_headers_do_not_supply_the_download_location() {
    let (storage, storage_server) = serve("200 OK", "", b"artifact");
    let (api, api_server) = serve_response(format!("HTTP/1.1 103 Early Hints\r\nLink: <https://example.com/ignored>; rel=preload\r\n\r\nHTTP/1.1 302 Found\r\nLocation: {storage}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes());
    let mut output = Vec::new();
    client()
        .download(&api, &mut output, Kind::Artifact, allow_loopback)
        .unwrap();
    assert_eq!(b"artifact", output.as_slice());
    api_server.join().unwrap();
    storage_server.join().unwrap();
}

#[test]
fn each_response_header_block_resets_the_location() {
    let mut easy = client()
        .handle(ARTIFACT, Vec::new(), Phase::Artifact)
        .unwrap();
    let handler = easy.get_mut();
    assert!(handler.header(b"HTTP/1.1 103 Early Hints\r\n"));
    assert!(handler.header(b"Location: https://attacker.invalid/ignored\r\n"));
    assert!(handler.header(b"HTTP/1.1 302 Found\r\n"));
    assert_eq!(302, handler.status);
    assert!(handler.location.is_none());
    assert!(handler.header(b"Location: https://release-assets.githubusercontent.com/file\r\n"));
    assert_eq!(
        Some("https://release-assets.githubusercontent.com/file"),
        handler.location.as_deref()
    );
    assert!(handler.response_error.is_none());
}

#[test]
fn metadata_errors_fail_before_a_download() {
    let (api, server) = serve("404 Not Found", "", b"synthetic-private-body");
    let error = client().verify_run(&api, 123, 456).unwrap_err();
    assert!(error.to_string().contains("HTTP 404"));
    assert!(!format!("{error:#}").contains("synthetic-private-body"));
    server.join().unwrap();
}

#[test]
fn github_authorization_is_not_sent_to_proxy_connect() {
    let (proxy, server) = serve("407 Proxy Authentication Required", "", b"");
    let mut easy = client()
        .handle(ARTIFACT, Vec::new(), Phase::Artifact)
        .unwrap();
    easy.proxy(&proxy).unwrap();
    easy.noproxy("").unwrap();
    assert!(easy.perform().is_err());
    let request = server.join().unwrap().to_lowercase();
    assert!(request.starts_with("connect api.github.com:443 http/"));
    assert!(!request.contains("authorization:"));
    assert!(!request.contains("synthetic.token"));
}

#[test]
fn progress_never_contains_the_signed_location() {
    let easy = client()
        .handle(
            "https://release-assets.githubusercontent.com/private?sig=synthetic-secret",
            Vec::new(),
            Phase::Storage,
        )
        .unwrap();
    let prefix = easy.get_ref().inner.progress.prefix();
    assert!(prefix.contains("GitHub artifact"));
    assert!(!prefix.contains("synthetic-secret"));
}

#[test]
fn custom_headers_are_rejected_before_credentials_are_read() {
    for header in [
        "Accept: application/json",
        "Authorization: synthetic.token",
        "Cookie: secret",
        "Host;",
        "X-Test: secret\r\n",
    ] {
        let error = fetch(ARTIFACT, Vec::new(), vec![header.into()], true, true).unwrap_err();
        assert!(error.to_string().contains("Custom headers"));
        assert!(!format!("{error:#}").contains(header));
    }
}

#[test]
fn supported_links_identify_immutable_resources() {
    for (url, kind, run_id) in [
        (ARTIFACT, Kind::Artifact, None),
        (BROWSER, Kind::Artifact, Some(456)),
        (ASSET, Kind::ReleaseAsset, None),
    ] {
        assert!(recognizes(url));
        assert_eq!(
            Download {
                repository: "example/builds".into(),
                id: 123,
                run_id,
                kind,
            },
            Download::parse(url).unwrap()
        );
    }
    let source =
        "https://api.github.com/repos/Example-Org/Repo.name_1/releases/assets/18446744073709551615";
    assert_eq!(u64::MAX, Download::parse(source).unwrap().id);
}

#[test]
fn unrelated_urls_do_not_enable_github_authentication() {
    for url in [
        "https://api.github.com/repos/example/builds/releases/latest",
        "https://api.github.com/repos/example/builds/actions/artifacts",
        "https://api.github.com/repos/example/builds/actions/artifacts/123",
        "https://api.github.com/repos/example/builds/releases/assets/123/labels",
        "https://api.github.com/repos/example/builds/releases?next=/actions/artifacts/123/zip",
        "https://github.com/example/builds/issues/1?next=/actions/runs/456/artifacts/123",
        "https://github.com/example/builds/releases/download/v1/pants.pex",
        "https://github.com/example/builds/actions/runs/456",
        "https://api.github.com.attacker.invalid/repos/example/builds/actions/artifacts/123/zip",
        "https://example.com/repos/example/builds/actions/artifacts/123/zip",
        "file:///tmp/artifact",
        "gs://bucket/artifact",
        "not a URL",
    ] {
        assert!(!recognizes(url), "recognized {url}");
    }
}

#[test]
fn malformed_download_routes_still_use_strict_validation() {
    for source in [
        ARTIFACT.replace("https://", "http://"),
        ARTIFACT.replace("api.github.com", "synthetic-secret@api.github.com"),
        ARTIFACT.replace("api.github.com", "api.github.com:443"),
        format!("{ARTIFACT}?secret=synthetic-secret"),
        format!("{ARTIFACT}#synthetic-secret"),
        ARTIFACT.replace("/123/", "/0/"),
        ARTIFACT.replace("/123/", "/%31/"),
    ] {
        assert!(recognizes(&source));
        let error = validate_source(&source).unwrap_err();
        assert!(!format!("{error:#}").contains("synthetic-secret"));
    }
}

#[test]
fn invalid_sources_fail_without_echoing_input() {
    for source in [
        "http://api.github.com/repos/example/builds/actions/artifacts/123/zip",
        "https://synthetic-secret@api.github.com/repos/example/builds/actions/artifacts/123/zip",
        "https://api.github.com:443/repos/example/builds/actions/artifacts/123/zip",
        "https://api.github.com/repos/example/builds/actions/artifacts/123/zip?secret=synthetic-secret",
        "https://api.github.com/repos/example/builds/actions/artifacts/123/zip#synthetic-secret",
        "https://api.github.com/repos/example/builds/actions/artifacts/0/zip",
        "https://api.github.com/repos/example/builds/actions/artifacts/-1/zip",
        "https://api.github.com/repos/example/builds/actions/artifacts/+1/zip",
        "https://api.github.com/repos/example/builds/actions/artifacts/1e3/zip",
        "https://api.github.com/repos/example/builds/actions/artifacts/18446744073709551616/zip",
        "https://api.github.com/repos/example/builds/actions/artifacts/123/tar",
        "https://api.github.com/repos/example/builds/actions/artifacts/123/zip/extra",
        "https://api.github.com/repos/example/builds/actions/artifacts//zip",
        "https://api.github.com/repos/example/builds/actions/artifacts/%31/zip",
        "https://api.github.com/repos/example/../actions/artifacts/123/zip",
        "https://api.github.com/repos/example/%2e%2e/actions/artifacts/123/zip",
        "https://api.github.com/repos/example/a%2fb/actions/artifacts/123/zip",
        "https://api.github.com/repos/example//actions/artifacts/123/zip",
        "https://api.github.com/repos/example/builds\\a/actions/artifacts/123/zip",
        "https://api.github.com/repos/example/builds/actions/artifacts/12\t3/zip",
        "https://github.com/example/builds/actions/runs/0/artifacts/123",
        "https://github.com/example/builds/actions/runs/456/artifacts/123/",
    ] {
        let error = Download::parse(source).unwrap_err();
        assert!(!format!("{error:#}").contains("synthetic-secret"));
    }
    let source = format!(
        "https://api.github.com/repos/example/{}/releases/assets/123",
        "x".repeat(101)
    );
    assert!(Download::parse(&source).is_err());
}

#[test]
fn bearer_validation_is_bounded_and_secret_safe() {
    for token in [
        "",
        "synthetic secret",
        "synthetic\r\nsecret",
        "synthetic\0secret",
        "synthetic\u{e9}",
    ] {
        let error = validate_token(token).unwrap_err();
        assert!(!format!("{error:#}").contains("synthetic"));
    }
    assert!(validate_token(&"x".repeat(8193)).is_err());
    validate_token("synthetic.token-_~+/=").unwrap();
}

#[test]
fn only_documented_storage_services_accept_signed_downloads() {
    for location in [
        "https://productionresultssa0.blob.core.windows.net/container/pants.pex?sig=a%2Fb+c&se=x",
        "https://release-assets.githubusercontent.com/github-production-release-asset/pants.pex?sig=synthetic",
        "https://objects.githubusercontent.com/github-production-release-asset/pants.pex?sig=synthetic",
    ] {
        validate_location(location).unwrap();
    }
    for location in [
        "http://productionresultssa0.blob.core.windows.net/object?sig=synthetic",
        "https://attacker.invalid/object?sig=synthetic",
        "https://api.github.com/repos/example/builds?sig=synthetic",
        "https://localhost/object?sig=synthetic",
        "https://127.0.0.1/object?sig=synthetic",
        "https://[::1]/object?sig=synthetic",
        "https://169.254.169.254/object?sig=synthetic",
        "https://productionresultssa0.blob.core.windows.net.attacker.invalid/object?sig=synthetic",
        "https://attacker.productionresultssa0.blob.core.windows.net/object?sig=synthetic",
        "https://blob.core.windows.net/object?sig=synthetic",
        "https://synthetic@release-assets.githubusercontent.com/object",
        "https://release-assets.githubusercontent.com:444/object?sig=synthetic",
        "https://release-assets.githubusercontent.com/object#synthetic",
        "https://release-assets.githubusercontent.com/object?sig=synthetic\n",
        "https://release-assets.githubusercontent.com\\@attacker.invalid/object?sig=synthetic",
        "https://release-assets.githubusercontent.com/object?sig=%GGsynthetic",
        "file:///synthetic",
        "//release-assets.githubusercontent.com/synthetic",
        "/synthetic",
        "",
    ] {
        let error = validate_location(location).unwrap_err();
        assert!(!format!("{error:#}").contains("synthetic"));
    }
}
