# Releasing

There are two release paths now.

- crates.io publishing for the workspace crates
- GitHub releases for the `syft` binary

The GitHub Actions setup lives in:

- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`

`ci.yml` runs the test suite on Linux, macOS, and Windows. It also checks the workspace metadata and packages `syft-types`, which is the only crate that can be fully package-checked before the rest of the workspace exists on crates.io.

The dependent crates get their real publish validation in the release job itself, in publish order, with retries for crates.io index lag.

`release.yml` runs on tags that look like `v*`.

It does four things:

1. Checks that the tag matches the workspace version.
2. Builds release binaries for:
   - Linux x86_64
   - macOS x86_64
   - macOS arm64
   - Windows x86_64
3. Publishes a GitHub release with archives and a `SHA256SUMS.txt` file.
4. Publishes the crates to crates.io in dependency order.

## Required secret

The crates publish job needs this secret:

- `CARGO_REGISTRY_TOKEN`

If that secret is missing, the binary release still runs. The crates.io publish job just gets skipped.

## Install from releases

There are two install scripts in `scripts/`.

- `scripts/install.sh`
- `scripts/install.ps1`

They download the latest release by default. You can also pass a version.

Examples:

```bash
./scripts/install.sh
./scripts/install.sh v0.1.0
```

```powershell
./scripts/install.ps1
./scripts/install.ps1 v0.1.0
```

By default the scripts install into a user-local bin directory.

- Unix: `$HOME/.local/bin`
- Windows: `%USERPROFILE%\.local\bin`

You can override that with `SYFT_INSTALL_DIR`.
