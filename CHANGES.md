# Release Notes

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
