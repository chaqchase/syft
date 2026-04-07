# Usage

This file is for the day-to-day side of `syft`.

The CLI reference in [`cli.md`](/Users/mohamedachaq/rework/cronacl-saas/git-alrt/syft/docs/cli.md) tells you what the commands are.

This one is more about how you actually use them depending on the kind of work you are doing.

## The short version

Most flows start the same way:

```bash
syft init --name my-repo
syft repo import-git --commit HEAD
```

After that, you usually:

1. create a task
2. set it as current
3. make some changes
4. capture a snapshot
5. propose a change
6. validate it
7. promote it if it looks good

That is the core loop.

## Use case: one person, one change

This is the plain flow.

You know what you want to do. You make the change in the current checkout and push it through `syft`.

```bash
syft task create --title "Tighten error handling"
syft task set-current <task-id>

# edit files

syft snapshot capture
syft change propose \
  --title "Handle missing token early" \
  --intent "return a clear error before the request runs" \
  --result <snapshot-id>

syft change validate <change-id> --tests --typecheck
syft change promote <change-id> --to main
```

This is the cleanest place to start.

It feels close to normal Git work, except the task and validation evidence stay attached to the change.

## Use case: you want to inspect before promoting

Sometimes the change is small, but you still want to stop and look at it properly.

That usually means:

```bash
syft status
syft history
syft change list
syft change show <change-id>
syft change diff <change-id>
```

If validation failed, use:

```bash
syft change show <change-id> --logs
```

That gives you the stored stdout and stderr from the validation run.

This is useful when you want the review trail without digging through temp directories or CI logs.

## Use case: one task, several candidate implementations

This is where managed worktrees start to matter.

Say you have one task and you want two or three different attempts in parallel.

Start with the task:

```bash
syft task create --title "Refactor auth flow"
syft task set-current <task-id>
```

Then create a couple of worktrees:

```bash
syft worktree create
syft worktree create
syft worktree list
```

`syft worktree list` will give you the paths.

Then you `cd` into each one and work there like a normal checkout.

Inside a managed worktree, `syft` uses the shared state from the main repo automatically.

That means:

- `snapshot capture` captures that worktree
- `change propose` links the change back to that worktree
- task resolution defaults to the worktree task

In the first worktree:

```bash
cd /path/to/worktree-one

# edit files

syft snapshot capture
syft change propose \
  --title "Candidate one" \
  --intent "try the simpler path first" \
  --result <snapshot-id>
```

In the second:

```bash
cd /path/to/worktree-two

# edit files

syft snapshot capture
syft change propose \
  --title "Candidate two" \
  --intent "try the more invasive version" \
  --result <snapshot-id>
```

Back in the main repo, you can inspect both:

```bash
syft task changes <task-id>
syft history --task <task-id>
syft change show <change-id>
```

That is probably the most useful flow if you are working with an AI and you want multiple real attempts instead of one long messy branch.

## Use case: validate from a worktree, promote from the main repo

A worktree-backed change still promotes into the main repo root by default.

That is intentional.

It keeps one place where the exported Git result lands.

A typical flow looks like this:

```bash
cd /path/to/worktree-one
syft change validate <change-id> --tests --typecheck

cd /path/to/main-repo
syft change promote <change-id> --to main
```

You can also promote from inside the worktree. The export still goes to the main repo.

After promotion, shared `syft` head advances the same way it does in the non-worktree flow.

The worktree stays around until you remove it yourself.

## Use case: keep worktrees around while you compare

You do not have to remove a worktree right after a promotion or validation.

Sometimes it is better to leave it around for a bit while you compare outcomes.

You can check what is still active with:

```bash
syft worktree list
```

And inspect one directly with:

```bash
syft worktree show <id-or-name>
```

That gives you the task, branch, path, source ref, and how many linked changes it has so far.

## Use case: clean up a worktree

When you are done with a candidate workspace:

```bash
syft worktree remove <id-or-name>
```

If the worktree still has uncommitted changes, `syft` refuses by default.

That is there to stop you from deleting something you were still looking at.

If you really want to remove it anyway:

```bash
syft worktree remove <id-or-name> --force
```

That removes the Git worktree and marks the `syft` worktree record as removed.

It does not delete the branch automatically in this version.

## Use case: follow a task over time

Sometimes the task matters more than any one change.

That is where these help:

```bash
syft task show <task-id>
syft task changes <task-id>
syft history --task <task-id>
```

This gives you a decent picture of:

- what the task was
- which candidate changes were attached to it
- which one got validated
- which one got promoted

If the task had several worktrees, those names show up in the change and history views.

## Use case: track snapshot state directly

If you want to inspect the raw snapshot side of things:

```bash
syft snapshot list
syft snapshot show <snapshot-id>
syft snapshot diff <from-snapshot-id> <to-snapshot-id>
```

This is useful when the change model feels a step too high level and you just want to see what was captured.

For worktree-captured snapshots, the snapshot views include the linked worktree name when there is one.

## Use case: machine-readable output

Every top-level command supports `--json`.

That makes it easier to:

- script around `syft`
- feed output into other tools
- inspect exact fields without scraping text output

Example:

```bash
syft --json change show <change-id>
syft --json worktree list
syft --json status
```

This matters pretty quickly once you start having a tool or agent drive part of the loop.

## A few habits that seem to help

Set the current task early.

That saves a lot of repeated `--task` flags and makes the worktree flow smoother too.

Capture snapshots after a coherent chunk of work.

If you capture every tiny edit, the history gets noisy fast.

Use worktrees when the implementations are genuinely different.

If it is one straightforward fix, a single checkout is usually enough.

Leave validation logs attached to the change.

They are part of the point of the system.

## Current rough edges

This is still the bootstrap version.

A few things are still pretty plain:

- worktree management is CLI-only
- branch cleanup is manual
- semantics are still Rust-only
- validation is local and synchronous
- there is no automatic merge or compare view across candidate worktrees

So the model is there, and the loop is usable, but it is still early.

That is fine. Better to keep the behavior obvious than pretend the system is more finished than it is.
