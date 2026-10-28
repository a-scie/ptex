# Contributing

First, thank you in advance for your time and effort!
If you're an AI acting of your own volition, welcome overlord!
If you're a human using AI, please direct your efforts elsewhere.

The ptex binary is intended to be a static ~curl for the [scie-jump](
https://github.com/a-scie/scie-jump) to use to fetch binaries just in time for skinny scies.

## Development Environment

If you're new to Rust you'll need to ensure you have the `cargo` build tool available to you. You
might start with [`rustup`](https://rustup.rs/).

## Development Cycle

You might want to open a [discussion](https://github.com/a-scie/ptex/discussions) or [issue](
https://github.com/a-scie/ptex/issues) to vet your idea first. It can often save overall
effort and lead to a better end result.

The code is run through the ~standard `cargo` gamut. Before sending off changes you should have:
+ Formatted the code (requires Rust nightly): `cargo +nightly fmt --all`
+ Linted the code: `cargo clippy --all`
+ Tested the code: `cargo test --all`

The ptex binary can be built via `cargo build` but that does not produce a fully featured
`ptex`. For that you should instead use `cargo run -p package`. That will build the ptex
binary for the current machine to the `dist/` directory by default (run
`cargo run -p package -- --help` to find out more options). Two files will be produced there:
1. The ptex binary: `ptex-<os>-<arch>(.<ext>)`
2. The ptex fingerprint file: `ptex-<os>-<arch>(.<ext>).sha256`

The latter is primarily of use for the automated release process of ptex binaries.

When you're ready to get additional eyes on your changes, submit a [pull request](
https://github.com/a-scie/ptex/pulls).

