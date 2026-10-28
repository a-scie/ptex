# ptex

[![GitHub](https://img.shields.io/github/license/a-scie/ptex)](LICENSE)
[![Github Actions CI](https://github.com/a-scie/ptex/actions/workflows/ci.yml/badge.svg)](https://github.com/a-scie/ptex/actions/workflows/ci.yml)
[![Discord](https://img.shields.io/discord/1113502044922322954)](https://scie.app/discord)

Ship gouged out scies that repair their base on 1st run by fetching missing files.

## Why use `ptex`?

Shipping your interpreted-language application as a native executable [scie](
https://github.com/a-scie/jump) can be attractive. Users need not install an interpreter or even
know what language your application is written in at all. As you continue to ship updates to your
application each binary scie release will contain the same embedded interpreter though and these can
be quite large. This is unfortunate since the interpreter likely already exists in the user's local
`~/.nce` cache. For this reason and others, you may want to ship a scie without all the files in its
base filled in, but instead downloaded to the end users machine only if needed. This saves
storage space on your servers and the end user's machine as well as bandwidth transferring your
application scie between them. The `ptex` binary can help you do this.

## Private GitHub downloads

Set `PTEX_GITHUB_TOKEN` to a repository-scoped token. ptex obtains it directly from
its environment, without Python, `gh`, browser cookies or a credential helper.
`GH_TOKEN`, `GITHUB_TOKEN` and launcher-specific GitHub token settings are **not**
read implicitly. For example, explicitly expose a workflow token to ptex:

```console
PTEX_GITHUB_TOKEN="$GITHUB_TOKEN" ptex \
  https://github.com/example/builds/actions/runs/456/artifacts/123 > build-artifacts.zip
```

These GitHub.com link forms work directly and as string values in the `ptex` map:

- Actions browser link: `https://github.com/OWNER/REPO/actions/runs/RUN_ID/artifacts/ARTIFACT_ID`
- Actions API download: `https://api.github.com/repos/OWNER/REPO/actions/artifacts/ARTIFACT_ID/zip`
- Release asset API download: `https://api.github.com/repos/OWNER/REPO/releases/assets/ASSET_ID`

Browser links are resolved through the API, not scraped; metadata must identify the
specified artifact and workflow run. Repository names and positive numeric IDs are
validated before credentials are read. Source queries, fragments, escaped paths,
userinfo and explicit ports are not supported. GitHub Enterprise, release browser
links (`/releases/download/TAG/NAME`), discovery by name/tag and "latest" lookup are
not authenticated by this provider. Other URLs retain their existing behavior.

Use a fine-grained PAT or GitHub App token restricted to the relevant repository:
**Actions: read** for artifacts, or **Contents: read** for release assets. A workflow's
`GITHUB_TOKEN` normally covers only that workflow's repository; cross-repository
access needs an appropriately scoped identity. Classic PATs generally require the
broader `repo` scope. A missing, empty, invalid or rejected token fails without
trying another identity or falling back to public content.

### Bytes, archives and bootstrap

ptex returns downloaded bytes **unchanged**. Default Actions uploads are ZIP archives,
even for one uploaded file; ptex does not unwrap them or select an archive member.
A mapping for a raw PEX or Python tarball must return that exact file, not a ZIP
wrapper. Use a release asset, or a producer version of
[`actions/upload-artifact`](https://github.com/actions/upload-artifact#inputs) that
supports `archive: false` for a single raw file. The API route is still `/zip` for
those raw uploads. `-O` keeps normal URL-basename behavior (e.g. `zip` for the API
archive route), not the filename in a response header.

Artifacts expire or are deleted according to workflow retention. Stable artifact
IDs do not guarantee permanent availability; release assets or GCS are usually
better for long-lived bootstrap distributions. Keep stable GitHub links in the
manifest, **not** the short-lived signed storage URLs returned by the API.

### Credential and redirect boundary

Only recognized links enable this provider. It rejects caller `-H` headers and
ignores `.netrc` on both requests. The bearer goes only to the fixed HTTPS GitHub API
origin. Actions requires the documented 302 redirect; release assets support 200 or
302. A fresh, credential-free request then downloads the signed URL, requiring 200.
It does not forward cookies, API headers or credentials, and does not follow further
redirects. The allowed storage hosts are Azure storage accounts under
`.blob.core.windows.net`, `release-assets.githubusercontent.com` and
`objects.githubusercontent.com`. Unknown destinations fail closed; GitHub does not
promise this storage-host list will never change.

Headers and metadata are size-bounded; connections and stalled transfers time out.
Metadata and Actions artifact resolution have a 60-second deadline; binary downloads
have no fixed total deadline. Diagnostics and `-D` / `PTEX_DUMP_HEADERS` show only
safe stage/status information, never signed URLs or arbitrary response headers/bodies.
A failed transfer can leave partial output; check the exit status before using it.
No failed transfer is retried into the existing output stream.

Authentication is not provenance: trust the producer/workflow, restrict repository
access, and keep independently trusted hashes for executable downloads. The provider
is not a sandbox for untrusted manifests; local proxy, DNS and CA settings remain
trusted configuration. Environment tokens are also visible to same-user processes
and may be inherited by applications started by a parent launcher.

API contracts: [Actions artifacts](https://docs.github.com/en/rest/actions/artifacts#download-an-artifact)
and [release assets](https://docs.github.com/en/rest/releases/assets#get-a-release-asset).

## How `ptex` works

The [`scie-jump`](
https://github.com/a-scie/jump/blob/main/docs/packaging.md#optional-fields) supports assigning a
`source` binding command to any file entry in the lift manifest. That source binding command then
becomes responsible for providing the content of that file when needed. The `ptex` binary can be
used as the executable for these binding commands to provide a robust, self-contained means of
fetching scie files just in time, on an as-needed basis. The `ptex` just needs to be told what URL
each file it is responsible for materializing comes from. Configuration looks like so:
```json
{
  "scie": {
    "lift": {
      "name": "skinny-scie",
      "files": [
        {
          "name": "ptex-linux-x86_64",
          "key": "ptex",
          "executable": true
        },
        {
          "name": "cpython-3.10.8+20221106-x86_64-unknown-linux-gnu-install_only.tar.gz",
          "key": "cpython",
          "size": 27555856,
          "hash": "6c8db44ae0e18e320320bbaaafd2d69cde8bfea171ae2d651b7993d1396260b7",
          "type": "tar.gz",
          "source": "ptex-fetch"
        }
      ],
      "boot": {
        "commands": {
          "": {
            "exe": "{cpython}/python/bin/python3.10",
            "args": [
              "-c",
              "print('Hello World!')"
            ]
          }
        },
        "bindings": {
          "ptex-fetch": {
            "exe": "{ptex}",
            "args": [
              "{scie.lift}"
            ]
          }
        }
      }
    }
  },
  "ptex": {
    "cpython-3.10.8+20221106-x86_64-unknown-linux-gnu-install_only.tar.gz":
      "https://github.com/astral-sh/python-build-standalone/releases/download/20221106/cpython-3.10.8+20221106-x86_64-unknown-linux-gnu-install_only.tar.gz"
  }
}
```

In this example Linux x86_64 CPython 3.10 scie, the Linux x86_64 `ptex` binary found in the
[`ptex` Releases](https://github.com/a-scie/ptex/releases) is directly included. The CPython binary
distribution from [Python Build Standalone](https://github.com/astral-sh/python-build-standalone) is
not directly included, but instead sourced via `ptex`. This is arranged with a binding command
configured to run `ptex` passing in the lift manifest itself which `ptex` uses to find the URL of
the CPython binary distribution when it needs to fetch it.

This results in a `skinny-scie` that is ~5.6MB. On 1st run on the target host you'll see some
information about the download updating on stderr:
```
$ rm -rf ~/.nce && time ./skinny-scie
Downloaded 27555856 of 27555856 bytes (100%) from https://github.com/astral-sh/python-build-standalone/releases/download/20221106/cpython-3.10.8+20221106-x86_64-unknown-linux-gnu-install_only.tar.gz
Hello World!

real    0m3.888s
user    0m0.305s
sys     0m0.212s
```
On subsequent runs on that host, or any host where a scie has already used that CPython
distribution, you'll find:
```
$ time ./skinny-scie
Hello World!

real    0m0.018s
user    0m0.018s
sys     0m0.000s
```

Since the CPython distribution was already downloaded and extracted (to
`~/.nce/6c8db44ae0e18e320320bbaaafd2d69cde8bfea171ae2d651b7993d1396260b7` in this case) it is not
fetched again, but instead ready for use immediately. Any other scie that uses this same CPython
distribution with the same hash, whether it uses ptex or not, will also enjoy this same cache-hit
and speedy run.

For comparison, the equivalent self-contained scie with a pre-downloaded and embedded CPython
distribution is ~28M:
```json
{
  "scie": {
    "lift": {
      "name": "skinny-scie",
      "files": [
        {
          "name": "cpython-3.10.8+20221106-x86_64-unknown-linux-gnu-install_only.tar.gz",
          "key": "cpython"
        }
      ],
      "boot": {
        "commands": {
          "": {
            "exe": "{cpython}/python/bin/python3.10",
            "args": [
              "-c",
              "print('Hello World!')"
            ]
          }
        }
      }
    }
  }
}
```

## Building `ptex`

The `ptex` binary is [released](https://github.com/a-scie/ptex/releases) for Linux (x86_64,
aarch64, armv7l, powerpc64le, riscv64 & s390x), macOS (x86_64 & aarch64) and Windows
(x86_64 & aarch64). If you'd like to build your own copy, you'll need
[Rust installed](https://rustup.rs/) at which point you can run `cargo run -p package` and a binary
for your current machine will be built in `dist/` along with a sha256 checksum file. For more build
options, you can run `cargo run -p package -- --help`. On some systems, builds will require `cmake`,
`make` and `perl` in order to build various `*-sys` crates. If you're missing these, the build
failures will point you in the right direction with some reading.
