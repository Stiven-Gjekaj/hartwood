<div align="center">
  <a href="../README.md"><b>hartwood</b></a>
</div>

# Releasing

A release is a decision that a person makes. The workflow does the rest, and
it does nothing until a tag tells it to.

## Before a release

Every item has to be true, and not only probably true.

- CI is green on the commit that the tag will name: the format, the lints,
  the tests on macOS and on Linux, the wasm32 build, the web build, and the
  244 riscv-tests programs.
- The images workflow is green on that commit: xv6 and Linux boot on
  hartwood, and Linux boots on QEMU.
- The version in the root `Cargo.toml` is the version of the release.
- [CHANGELOG.md](../CHANGELOG.md) has a section for the version, such as
  `## 0.1.0 - 2026-11-13`. The workflow makes the notes of the release from
  it, and it stops when there is none.

## Cut a release

1. Set `version` under `[workspace.package]` in the root `Cargo.toml`, and
   run `cargo check` so `Cargo.lock` follows. Commit both. The commit subject
   names no version number.
2. Push, and wait for CI and the images workflow to pass on that commit.
3. Tag the commit and push the tag:

       git tag v0.1.0
       git push origin v0.1.0

4. The [release workflow](../.github/workflows/release.yml) builds the
   command line for five targets and the web frontend, hashes each file,
   attests each one, and opens a **draft** release. It refuses a tag that
   disagrees with the version in `Cargo.toml`.
5. Open the draft. Read the list of assets: five binaries, the web archive
   and `SHA256SUMS`. Publish it.

## The web demo

The [pages workflow](../.github/workflows/pages.yml) runs after each green
run of the images workflow on `main`, and puts the site with its images on
GitHub Pages. A release does not change the site. To deploy by hand:

    gh workflow run pages.yml
