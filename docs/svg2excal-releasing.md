# Releasing svg2excal

The workspace publishes three crates with one shared version:

1. `svg2excal-core`, the library;
2. `svg2excal`, the CLI;
3. `svg2excal-server`, the loopback HTTP adapter.

Cargo resolves the workspace dependency order. The package names were
available on crates.io when checked on 2026-08-13, but name ownership is only
secured by the first successful publish.

## Prerequisites

- a clean `master` branch synchronized with `origin/master`;
- a crates.io account with a scoped publish token available to Cargo;
- `cargo-release` and `git-cliff` installed;
- initialized Git submodules; and
- no unreviewed generated or dependency changes.

Never put a registry token in the repository, command history, logs, or release
notes. Use Cargo's credential provider or an environment secret supplied by the
release runner.

## Release gate

Run the complete gate before publishing:

```bash
git submodule update --init --recursive
make verify
```

The gate includes package dry-runs from the generated `.crate` archives and
rejects an archive above crates.io's 10 MiB limit. Inspect the final diff and
confirm that `CHANGELOG.md`, `Cargo.lock`, the fixture pair, and all three
manifests describe the intended release.

## Publish

For the initial `0.1.0` release, the manifests already carry the desired
version, so run:

```bash
make release VERSION=release
```

For a later release, select a SemVer level or explicit version:

```bash
make release VERSION=patch
# or: make release VERSION=0.2.0
```

`cargo-release` keeps workspace versions aligned, refreshes the changelog,
creates one release commit and `v<version>` tag, publishes in dependency order,
and pushes the commit and tag to `origin`. The pushed tag triggers the GitHub
release workflow.

Publishing is permanent: do not run the release target as a probe. If a
published version is unusable, fix forward and yank the affected version with
`cargo yank --version <version> <crate>`; a yank does not delete the archive.
