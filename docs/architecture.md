# Architecture

`syft` is a Rust workspace with one binary crate and a handful of focused library crates.

The shape is simple on purpose. The bootstrap version is trying to prove the model with as little machinery as possible.

## Workspace layout

The current workspace members are:

- `crates/syft-cli`
- `crates/syft-core`
- `crates/syft-git`
- `crates/syft-objects`
- `crates/syft-semantic`
- `crates/syft-store`
- `crates/syft-types`
- `crates/syft-validate`

`crates/syft-cli/src/main.rs` is the only binary entrypoint.

There is no root `src/main.rs`. Root-level `cargo run` works because the workspace resolves to the CLI binary cleanly.

## How the crates split up

### `syft-types`

Shared domain types.

This crate holds the serializable shapes used across the workspace: repos, snapshots, tasks, change nodes, validation records, promotions, semantic types, patch ops, and query/read models.

It also owns things like ID creation and hash helpers.

### `syft-store`

Persistence boundaries.

This crate defines the two store traits:

- `MetadataStore`
- `ObjectStore`

The current implementations are:

- SQLite for metadata
- filesystem-backed content-addressed object storage for raw bytes

The store layer is intentionally pretty plain. It stores whole JSON records in SQLite and keeps the logic in higher layers.

### `syft-objects`

Snapshot and tree handling.

This crate is where directory capture, tree serialization, materialization, and snapshot indexing live. It is the piece that turns a worktree into content-addressed objects and back again.

It also computes file-level patch ops between snapshots.

### `syft-git`

Git bridge in both directions.

This crate makes sure the repo is actually a Git repo, imports Git commits into snapshots, materializes snapshots into working directories, and exports promoted snapshots back into Git commits.

Right now this is how `syft` stays adoptable. You can keep using Git while `syft` tracks richer metadata on top.

### `syft-semantic`

Rust-only semantic extraction and diffing.

It parses Rust source with `syn`, extracts symbols and edges, and computes a semantic delta between two snapshots.

The model here is deliberately not built around a giant cross-language symbol enum. Symbols have a stable identity, a compact category, and flexible attributes. That makes the Rust side workable today and leaves room for more language adapters later.

### `syft-validate`

Local validation runner.

This crate materializes a snapshot into a temp directory and runs local `cargo` commands against it.

Today it supports:

- `cargo check`
- `cargo test`
- `cargo clippy -- -D warnings`

It stores both a summary artifact and the full stdout/stderr payload in object storage.

### `syft-core`

Application logic and orchestration.

This is the center of the bootstrap.

`SyftApp` lives here. It wires together the repo config, stores, semantic layer, validation runner, and Git bridge. It also owns the user-facing service traits:

- `RepoService`
- `TaskService`
- `ChangeService`
- `QueryService`

If you want to understand the actual behavior of the system, this is the first place to read.

### `syft-cli`

The command-line interface.

This crate is thin by design. It parses commands with `clap`, opens the repo, calls into `SyftApp`, and formats text or JSON output.

That keeps business logic out of the CLI and makes it easier to test flows at the core layer.

## Runtime shape

The bootstrap flow is synchronous and local.

There is no daemon. No API server. No background queue.

A typical command goes like this:

1. the CLI opens the repo by reading `.syft/repo.toml`
2. `SyftApp` opens the SQLite metadata store and the filesystem object store
3. the requested action runs
4. records are written to SQLite and objects are written to `.syft/objects`
5. the CLI prints text or JSON

That is it.

This is one of the nicer parts of the current design. You can understand a lot of the system without chasing a distributed control plane.

## Why the current boundaries look like this

The crate split mostly follows pressure points:

- types that need to be shared everywhere
- persistence that should be swappable later
- snapshot/object logic that should stay deterministic
- semantic analysis that will likely grow and change a lot
- validation that may eventually move to workers
- core orchestration that should stay readable
- a CLI that is replaceable

It is not the final architecture. It is a practical one for getting the first version working.

