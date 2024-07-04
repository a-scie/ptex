# Release Notes

## 1.1.1

This release fixes missing attestations for Linux ARM64 artifacts.

## 1.1.0

This release updates various dependencies as well as upgrading to Rust
1.79.0. In addition, this is the first release to include artifact
attestations in Sigstore.

## 1.0.0

This release updates various dependencies as well as upgrading to Rust
1.78.0 and dropping support for Windows versions prior to Windows 10.

## 0.7.1

This release updates various dependencies, most notably updating from
curl 8.0.1 (March 20, 2023) to 8.6.0 (January 31, 2024).
See:
+ https://curl.se/changes.html
+ https://curl.se/docs/releases.html
+ https://curl.se/docs/vulnerabilities.html

## 0.7.0

This release updates various dependencies, most notably updating from
curl 7.86.0 (October 26, 2022) to 8.0.1 (March 20, 2023).
See:
+ https://curl.se/changes.html
+ https://curl.se/docs/releases.html
+ https://curl.se/docs/vulnerabilities.html

## 0.6.0

This release add support for passing custom headers to the server via
one or more `-H|--header` arguments.

## 0.5.0

This release begins sending a User-Agent header of `ptex/<version>` and
also adds support for dumping received headers to stderr via either the
`-D|--dump-header` switches or a non-empty `PTEX_DUMP_HEADERS`
environment variable.

## 0.4.0

This release adds support for saving fetched content to a file by
specifying the `-O` or `--remote-name` option when in direct URL mode.
The ptex version can also e queried with `-V` or `--version`.

## 0.3.0

This release adds support for direct use by passing a single URL to
fetch and improves command line help to fully explain both the existing
scie binding file source mode and the new direct URL mode.

## 0.2.0

This release brings fully static binaries for Linux with zero runtime
linkage by switching the Linux targets to use musl. As part of this
switch, the Rust toolchain used is stabilized to stable / 1.65.0.

## 0.1.15

This release completes as much static linking as possible on Linux using
gnu by bringing zlib into the fold.

## 0.1.14

This release fixes a bug downloading large files and isolates the ptex
binary more fully from the end user system by statically linking as much
as possible.

## 0.1.13

The 1st public release of the project.
