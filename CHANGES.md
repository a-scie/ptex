# Release Notes

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
