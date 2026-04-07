# Development

This project is still small enough that the easiest way to understand it is to run it and read the crates directly.

That is still the best advice here. The codebase is compact enough that reading the actual flow beats trying to memorize a big architecture doc.

## Build and test

From the workspace root:

```bash
cargo build
cargo run -- --help
cargo test
```

That should build the CLI and run the test suite across the workspace.

## Where to start reading

If you are new to the codebase, this path is usually the shortest:

1. `crates/syft-cli/src/main.rs`
2. `crates/syft-core/src/lib.rs`
3. `crates/syft-types/src/lib.rs`
4. `crates/syft-store/src/lib.rs`
5. `crates/syft-objects/src/lib.rs`

After that, read `syft-semantic` and `syft-validate` depending on which part you care about.

## What the tests cover

There are a few layers of tests in the workspace.

### Unit-level coverage

The lower crates cover things like:

- ID generation
- hash stability
- object-store round trips
- snapshot capture and materialization
- semantic extraction and semantic diff behavior
- validation detail persistence

### End-to-end CLI coverage

`crates/syft-cli/tests/e2e.rs` is the main integration harness.

It exercises real repo flows in temp directories, including:

- repo init
- Git import
- task creation
- current task handling
- snapshot capture
- change proposal
- validation
- promotion
- status and history
- snapshot and change read commands
- diff commands

If you change user-facing behavior, this file is worth updating first.

## Current constraints

There are a few important boundaries in the current build.

### Rust-only semantics

The semantic layer only understands Rust right now.

That means:

- symbol extraction is Rust-only
- public API summaries are Rust-only
- the `--symbol` history filter depends on Rust semantic data

This is fine for the bootstrap. It is also one of the clearest limits of the current system.

### Local-only execution

Validation runs on the local machine against temp materializations of snapshots.

There is no remote execution, sandboxing layer, or worker process yet.

### Git is still the compatibility layer

`syft` snapshots and change nodes are the internal model, but Git is still how we import a base state and how we can export a promoted one.

That is by design. It keeps the system usable while the model matures.

### The schema is still simple

SQLite stores mostly whole JSON objects. That makes the domain easy to evolve, but it also means query sophistication is limited for now.

If we need richer filtering or indexing later, that probably lands as a more deliberate query/index layer rather than by slowly overloading the current store.

## A couple of implementation details worth knowing

### `capture_snapshot` does not advance head

This one is easy to miss.

The current behavior is:

- importing a Git commit can establish or advance head
- promoting a change can advance head
- capturing the current worktree does not advance head

That is there so `change propose` can safely default its base snapshot to head while using a newly captured result snapshot.

### Validation stores full logs

Validation summaries are stored in metadata, but the full stdout/stderr payload is stored in the object store and referenced by `details_ref`.

That keeps the read model compact without throwing away evidence.

Validation also runs with a temp-local `CARGO_TARGET_DIR` and clears excluded paths like `target/` out of the materialized snapshot first. That matters because old build output can otherwise hide real failures.

### Worktree capture has safe defaults

Live worktree snapshots always skip:

- `.git`
- `.syft`
- `target`

After that, capture follows `.gitignore` by default.

If a repo needs `syft`-specific rules, it can add an optional `.syftignore`.

### The semantic model avoids a giant symbol-kind enum

This was a deliberate correction early on.

Symbols have:

- a stable identity
- a small category
- tags
- free-form attributes

That is a much better fit if the system grows into more than one language.

## Working on docs

The split is meant to stay simple:

- root `README.md` for the high-level picture and the design choices
- `docs/` for implementation details and usage

If you add new end-user behavior, update `docs/cli.md`.

If you change crate boundaries, storage layout, or the runtime model, update the matching docs file here.
