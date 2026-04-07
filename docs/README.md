# Docs

This folder is for the nuts and bolts.

The root [`README.md`](/Users/mohamedachaq/rework/cronacl-saas/git-alrt/syft/README.md) is the high-level picture.

This folder is the practical part. It is about how the current build works, where things live, and what the CLI actually does today.

Start here:

- [`architecture.md`](/Users/mohamedachaq/rework/cronacl-saas/git-alrt/syft/docs/architecture.md) for the workspace layout and how the crates fit together
- [`storage.md`](/Users/mohamedachaq/rework/cronacl-saas/git-alrt/syft/docs/storage.md) for the `.syft/` directory, SQLite metadata, object storage, and core entities
- [`cli.md`](/Users/mohamedachaq/rework/cronacl-saas/git-alrt/syft/docs/cli.md) for commands and the current user workflow
- [`development.md`](/Users/mohamedachaq/rework/cronacl-saas/git-alrt/syft/docs/development.md) for local development, tests, and current constraints
- [`releasing.md`](/Users/mohamedachaq/rework/cronacl-saas/git-alrt/syft/docs/releasing.md) for CI, crates.io publishing, release binaries, and install scripts

All of this is based on the code that exists now.

There is a longer-term idea behind the project, sure, but these docs are meant to be useful when you are actually in the repo trying to understand the current system.
