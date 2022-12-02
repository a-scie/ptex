# Release Process

## Preparation

### Version Bump and Changelog

1. Bump the version in [`Cargo.toml`](Cargo.toml).
2. Run `cargo run -p package` to update [`Cargo.lock`](Cargo.lock) with the new version
   and as a sanity check on the state of the project.
3. Update [`CHANGES.md`](CHANGES.md) with any changes that are likely to be useful to consumers.
4. Open a PR with these changes and land it on https://github.com/a-scie/ptex main.

## Release

### Push Release Tag

Sync a local branch with https://github.com/a-scie/ptex main and confirm it has the version bump
and changelog update as the tip commit:

```
$ git log --stat -1 HEAD
commit c946ee09cb2ea6d34b894b4c5d92abae0c98d731 (HEAD -> main)
Author: John Sirois <john.sirois@gmail.com>
Date:   Thu Nov 24 07:56:16 2022 -0800

    Prepare the 0.1.0 release.

 .circleci/config.yml          |  2 +-
 .github/workflows/release.yml | 13 +++++++------
 CHANGES.md                    |  5 +++++
 3 files changed, 13 insertions(+), 7 deletions(-)
```

Tag the release as `v<version>` and push the tag to https://github.com/a-scie/ptex main:

```
$ git tag --sign -am 'Release 0.1.0' v0.1.0
$ git push --tags https://github.com/a-scie/ptex HEAD:main
```

The release is automated and will create a GitHub Release page at
[https://github.com/a-scie/ptex/releases/tag/v&lt;version&gt;](
https://github.com/a-scie/ptex/releases) with binaries for Linux, Mac and Windows.

