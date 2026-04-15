# Sync Protocol

## Current model

Bridges sync is git-based.

- each project checkout is a git repo
- Gitea hosts the remote repository
- Bridges syncs managed project state from `.shared/`
- `.bridges/` stays local and is not git-synced

## What `bridges sync` does

When you run:

```bash
bridges sync --project proj_xxx
```

Bridges:

1. ensures `.shared/` exists
2. stages and commits managed local changes from `.shared/` and `.gitignore`
3. pushes to the remote when possible
4. fetches remote changes
5. merges remote changes if safe
6. pushes again after a successful merge when needed

## Managed vs unmanaged paths

Managed sync paths:

- `.shared/...`
- `.gitignore`

Unmanaged paths:

- everything else in the project worktree

Bridges does not overwrite unmanaged local work by default.

## Approval flow for risky sync

If unmanaged local or remote paths would be affected, Bridges does not merge immediately.

Instead it:

1. writes `.bridges/sync-approval.json`
2. prints a `SYNC WARNING`
3. waits for an explicit approved rerun

Approved rerun:

```bash
bridges sync --project proj_xxx --approve-unmanaged
```

On approved unmanaged sync:

- unmanaged local work is preserved in a git stash first
- Bridges attempts the merge
- if merge succeeds, it restores the stash if possible
- if merge conflicts, it keeps the stash and reports that to the user

## Conflicts

Conflicts are reported as conflicted file paths from git.

Bridges does not currently auto-create a Gitea issue for conflicts.

User-visible signals:

- `CONFLICTS: ...`
- `SYNC WARNING: ...`
- `.bridges/sync-approval.json` when approval is required

## Shared file guidance

Good shared content:

- project goals
- task lists
- decisions
- status updates
- artifacts

Do not store chat transcripts in `.shared/CHANGELOG.md`.

Conversation/session memory belongs in:

- `.bridges/conversation-memory/`
