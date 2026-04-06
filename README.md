# syft

`syft` is an experiment in version control built for the way code gets made now.

Git is still in the loop, but it is not the whole story. The point here is to treat a change as more than a patch. We want to keep the task, the intent, the result snapshot, the semantic impact, the validation evidence, and the promotion decision in one system.

That matters a lot more once AI is part of the workflow.

When a person writes one careful patch by hand, a commit is usually enough. When a human and one or more agents are exploring a task, running tools, generating variants, and validating results, a plain diff starts to feel too small. You still need the diff, but you also need the reason for the change, what it touched, what passed, what failed, and what actually got promoted.

That is the shape `syft` is aiming at.

## Why this exists

This project started from a simple frustration: Git is good at storing snapshots and patches, but it does not really model intent.

Most AI-assisted work is intent-first.

You start with a task. You try one approach, then another. You run tests. You compare outcomes. You may throw away half the attempts. In that flow, the useful unit is not just "here is the diff". It is closer to "here is the candidate change for this task, here is what it changed, here is the evidence, and here is whether we should keep it".

So `syft` puts a few different primitives up front:

- `Task`
- `Snapshot`
- `ChangeNode`
- `ValidationArtifact`
- `PromotionRecord`

That is the core bet.

## The design decisions that matter

### 1. Git stays underneath for now

This is not trying to replace Git in one shot.

The current bootstrap keeps Git as the bridge in and out. We can import a Git commit into a snapshot. We can export a promoted snapshot back into Git. That makes the system usable without asking anyone to throw away existing tooling.

It also keeps the early work honest. We are not hiding behind a big future architecture. We are proving the model in a real repo first.

### 2. The primary unit is a change node, not a commit

A `ChangeNode` ties a task to a base snapshot and a result snapshot. It also carries intent, provenance, semantic delta, validation records, risk, and status.

That sounds a bit heavier than a commit because it is heavier.

The point is to preserve the context that usually gets scattered across commit messages, chat logs, CI output, and somebody's memory.

### 3. Review should lean semantic-first

Raw text diffs still matter. We already expose file-level patch ops and snapshot diffs in the CLI.

But the longer-term direction is different. If a change modifies a public API, touches dependency edges, or changes a symbol signature, that should be obvious without making the reviewer reverse-engineer it from the patch.

The current semantic layer is Rust-only and still pretty small, but that is the path we are on.

### 4. Storage is local-first

Everything in the bootstrap is local.

Repo metadata lives under `.syft/`. Metadata is stored in SQLite. Content-addressed objects live on disk. Validation runs locally against materialized snapshots. There is no API service, no worker system, and no remote coordination yet.

That was intentional. The first problem was to make the workflow real, not distributed.

### 5. Branches are not the center of the model

Internally, this system cares more about snapshots, tasks, changes, and promotions than about branches.

Branches still matter when we export back to Git, but they are treated more like a compatibility surface than the main abstraction.

## What is built right now

The current workspace supports this end-to-end flow:

1. initialize a `syft` repo
2. import a Git commit into a snapshot
3. create a task
4. capture a result snapshot from the worktree
5. propose a change node against a base snapshot
6. run validation on the result snapshot
7. promote the change and optionally export it back to Git

It also has the first read-side commands you need to inspect what is going on:

- repo status
- history
- snapshot list, show, diff
- task list, show, current, set-current, changes
- change list, show, latest, diff

This is still a bootstrap. It is useful, but not finished.

## What this is not trying to be yet

Some things are very deliberately missing right now:

- no API layer
- no background workers
- no remote sync story
- no native Git replacement
- no multi-language semantic engine
- no composition or merge policy system
- no smart variant ranking

Those may come later. They are not needed to prove the core model.

## Why the AI angle changes the design

The main shift is volume and shape.

AI systems can generate a lot of plausible changes quickly. That is useful, but it creates a bookkeeping problem. You need a system that can keep multiple candidate implementations tied to one task, hold onto the evidence, and make promotion an explicit step.

Git can store the end result. It does not really help much with the rest.

`syft` is trying to cover that middle ground.

It is basically a version-control-shaped system for intent, evidence, and promotion, with Git still doing the transport job underneath.

## Reading the rest

The root README is meant to stay high level.

The technical details live in [`docs/README.md`](/Users/mohamedachaq/rework/cronacl-saas/git-alrt/syft/docs/README.md):

- architecture and crate layout
- repo layout and storage model
- CLI commands and expected workflows
- development notes and testing

