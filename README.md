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
      "https://github.com/indygreg/python-build-standalone/releases/download/20221106/cpython-3.10.8+20221106-x86_64-unknown-linux-gnu-install_only.tar.gz"
  }
}
```

In this example Linux x86_64 CPython 3.10 scie, the Linux x86_64 `ptex` binary found in the
[`ptex` Releases](https://github.com/a-scie/ptex/releases) is directly included. The CPython binary
distribution from [Python Build Standalone](https://github.com/indygreg/python-build-standalone) is
not directly included, but instead sourced via `ptex`. This is arranged with a binding command
configured to run `ptex` passing in the lift manifest itself which `ptex` uses to find the URL of
the CPython binary distribution when it needs to fetch it.

This results in a `skinny-scie` that is ~5.6MB. On 1st run on the target host you'll see some
information about the download updating on stderr:
```
$ rm -rf ~/.nce && time ./skinny-scie
Downloaded 27555856 of 27555856 bytes (100%) from https://github.com/indygreg/python-build-standalone/releases/download/20221106/cpython-3.10.8+20221106-x86_64-unknown-linux-gnu-install_only.tar.gz
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

The `ptex` binary is [released](https://github.com/a-scie/ptex/releases) for Linux (x86_64 &
aarch64), macOS (x86_64 & aarch64) and Windows (x86_64). If you'd like to build your own copy,
you'll need [Rust installed](https://rustup.rs/) at which point you can run `cargo run -p package`
and a binary for your current machine will be built in `dist/` along with a sha256 checksum file.
For more build options, you can run `cargo run -p package -- --help`. On some systems, builds will
require `cmake`, `make` and `perl` in order to build various `*-sys` crates. If you're missing
these, the build failures will point you in the right direction with some reading.
