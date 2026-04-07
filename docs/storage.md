# Storage and Data Model

The current repo model is local-first and file-based.

Everything `syft` needs lives under `.syft/` inside the Git repo.

That sounds simple because it is simple. Right now that is a good thing.

## Control directory layout

This is the layout today:

```text
.syft/
  repo.toml
  objects/
  state/
    metadata.db
    head
    current_task
  cache/
  index/
```

Some of those directories are mostly placeholders right now.

`cache/` and `index/` exist because they will matter later, but the main active pieces today are:

- `repo.toml`
- `objects/`
- `state/metadata.db`
- `state/head`
- `state/current_task`

## `repo.toml`

This is the repo config file.

It stores the repo ID, name, default lineage, object store mode, metadata mode, enabled semantic languages, whether the Git bridge is on, and any extra snapshot-capture exclusions for this repo.

It is small and human-readable on purpose.

`capture_excludes` is for repo-specific extra rules. The built-in safe defaults are always applied separately.

## SQLite metadata

The metadata store is SQLite, backed by `rusqlite`.

The schema is intentionally simple:

- `repos`
- `snapshots`
- `tasks`
- `change_nodes`
- `validation_artifacts`
- `promotions`

Each row stores the serialized domain object as JSON, plus a few indexed fields like `repo_id`, `created_at`, `task_id`, or `status` to keep common queries easy.

This is not a heavily normalized schema. That is fine for the current stage.

The goal right now is to keep the domain types moving without building a complicated relational model too early.

## Object storage

Large or immutable payloads go through the object store.

The current object store is just the local filesystem under `.syft/objects/`, keyed by a BLAKE3 content hash.

That store holds things like:

- blobs and trees captured from the worktree
- snapshot manifests and indexes
- validation details payloads

The object store trait is small on purpose:

- put bytes
- get bytes

That keeps it easy to swap later if the storage backend changes.

## Current state files

There are two little state files worth knowing about.

### `state/head`

This tracks the current head snapshot for the repo.

It is used as the default base when `syft change propose` is called without `--base`.

At the moment, imports and promotions advance head. Snapshot capture does not. That was a deliberate choice so you can capture a result snapshot and still propose it against the current head without accidentally diffing the snapshot against itself.

### `state/current_task`

This tracks the current task ID.

It is used as the default task when `syft change propose` is called without `--task`.

Nothing writes this implicitly. You set it with `syft task set-current <id>`.

That keeps task context explicit instead of surprising.

## Snapshot capture exclusions

Worktree snapshots always exclude a small built-in set:

- `.git`
- `.syft`
- `target`

That happens even if the repo forgot to ignore those paths in Git.

You can add extra repo-local exclusions through `capture_excludes` in `.syft/repo.toml`.

Those values are repo-root-relative path prefixes.

Examples:

- `dist`
- `.cache/build`
- `generated/schema.json`

## Core entities

These are the main records in the system today.

### `Repo`

One logical `syft` repo.

It stores the repo ID, repo name, root path, default lineage, and creation time.

### `Snapshot`

A materialized repository state captured into the object store.

A snapshot points at:

- its root tree hash
- zero or more parent snapshot IDs
- metadata like source and labels
- creation time

Snapshot sources matter because they tell you where a state came from. A snapshot imported from Git is a different thing from one captured from a live worktree.

### `Task`

A task is the intent record.

It holds:

- title
- description
- acceptance criteria
- constraints
- labels
- status
- priority
- timestamps

It is the thing a change node is attached to.

### `ChangeNode`

This is the main unit of work in the current model.

A change node ties together:

- the repo
- the task
- the base snapshot
- the result snapshot
- title and intent
- patch ops
- semantic delta
- provenance
- validation artifact IDs
- risk and status

This is the record that tries to answer, in one place, "what was the attempted change and what do we know about it?"

### `ValidationArtifact`

One validation run result.

It stores:

- kind
- pass/fail status
- summary
- metrics
- timestamps
- optional `details_ref`

`details_ref` points to a `ValidationDetails` payload in object storage. That payload stores the command, exit status, stdout, and stderr.

Validation runs also clean excluded paths out of the temp materialization before running commands. That keeps old generated output from changing the result.

### `PromotionRecord`

This records that a change node was promoted into a target lineage.

Right now promotion can also export the promoted snapshot back into Git.

## Diff data

There are two different diff shapes in play.

### Patch ops

These are file-level changes between snapshots.

They drive:

- `change diff`
- `snapshot diff`
- parts of `change show`

This is not a unified patch view yet. It is a stable, structured summary of add, modify, delete, and rename operations.

### Semantic delta

This is the semantic summary between two snapshots.

Right now it tracks:

- touched symbols
- added symbols
- removed symbols
- public API changes
- dependency changes
- changed files
- a short summary string

This is Rust-only today.

## What is intentionally simple right now

A few shortcuts are worth calling out:

- SQLite rows store full JSON blobs
- object storage is local only
- there is no migration framework yet beyond table creation
- there is no persistent symbol index
- there is no cache invalidation story because there is barely any cache

That is fine for the current phase. The design is trying to stay legible while the model settles down.
