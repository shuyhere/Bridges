---
name: bridges
description: "Collaborate with other people and their agents through Bridges. Use this skill whenever the user mentions Bridges or `bridges`, or asks about setup, install, daemon/service health, runtime registration, Codex/Claude integration, projects, invites, joins, members, ask/debate/broadcast, optional shared-workspace sync, publish, sessions, peer connectivity, or debugging Bridges behavior."
allowed-tools: "Bash(bridges:*)"
---

# Bridges

Encrypted peer-to-peer collaboration between project members and their AI agents.

## Installation & Setup

### Step 1: Install the CLI

```bash
# Option A: Build from source (recommended for the current beta)
git clone https://github.com/shuyhere/Bridges.git bridges
cd bridges
cargo build --release --manifest-path cli/Cargo.toml
# Binary at: target/release/bridges
# Optionally symlink: ln -sf $(pwd)/target/release/bridges ~/.local/bin/bridges

# Option B: npm (only when a beta package is published)
# npm install -g bridges
```

### Step 2: Set up the CLI

```bash
bridges setup --coordination <COORDINATION_URL>
```

This:
- Generates Ed25519 keypairs locally (private key never leaves your machine)
- Registers your node with the coordination server
- Saves config to `~/.bridges/config.json`

### Step 3: Verify

```bash
bridges status
```

You should see your node ID and coordination server status.

### Step 4: Start the daemon

```bash
# Install as a background service (recommended)
bridges service install
bridges service start

# Or run in foreground for debugging
bridges daemon
```

## Adding Bridges as an Agent Skill

### For Pi Agent

Bridges installs itself as a skill when added to your project. To add it manually:

```bash
# The skill file is at: skills/bridges/SKILL.md (inside the bridges repo)
# Copy it to your agent's skills directory:
cp -r /path/to/bridges/skills/bridges ~/.agents/skills/bridges
```

If you are working from a local source checkout, copy from the repo:
```bash
cp -r /path/to/bridges/skills/bridges ~/.agents/skills/bridges
```

If a package release exists later, you can also copy from the installed npm package.

The skill gives the agent full knowledge of all Bridges commands, project workflows, optional shared-workspace sync behavior, and conversation session management.

### For Codex

```bash
bridges setup --coordination <COORDINATION_URL> --runtime codex
```

### For OpenClaw or Generic HTTP runtimes

```bash
bridges setup --coordination <COORDINATION_URL> \
  --runtime openclaw --endpoint http://<LOCAL_RUNTIME_HOST>:8080
```

## Quick Workflow

```bash
# Create a project
bridges create my-project --description "My agent collaboration"

# Invite a collaborator (share the token + project ID with them)
bridges invite -p proj_xxx

# They join with:
bridges join -p proj_xxx <INVITE_TOKEN>

# Talk to a peer
bridges ask kd_PEER_NODE_ID "What do you think about this design?" -p proj_xxx

# Run a debate with all members
bridges debate "Should we use microservices?" -p proj_xxx

# Optional: sync shared project files
bridges sync -p proj_xxx
```

## What Bridges Is

Bridges is a multi-user collaboration layer for humans and local coding agents.

Core model:

- each person has a local Bridges identity and a local daemon
- projects are coordinated through a central server
- agents talk to each other through `ask`, `debate`, `broadcast`, and `publish`
- optional shared project notes/files can be synchronized into `.shared/`
- local-only state stays under `.bridges/`

Think of Bridges as:

- a coordination server for membership, invites, peer keys, and transport routing
- a local daemon that receives messages and dispatches them into the user's runtime
- an optional shared-workspace sync layer for `.shared/` files
- a session memory layer for ongoing agent-to-agent conversations

When using this skill, reason about Bridges as a real collaboration system, not just a command wrapper:

- project membership matters
- sender identity matters
- transport can be direct or mailbox fallback
- `.shared/` is the source of shared project context
- `.bridges/` is local-only and should not be treated as shared state

## User Communication

Never tell the user to run `bridges` commands themselves. Run the commands and summarize the result naturally.

- Good: "Your project has two members. I can ask the other agent now."
- Good: "There's an optional shared-workspace sync involving unmanaged files. I generated an approval proposal and can apply it if you want."
- Bad: "Run `bridges invite`."

## Critical Rules

1. `--project` always takes a project ID starting with `proj_`, never the project slug.
2. After `bridges create`, save the returned `proj_...` ID and reuse it.
3. `ask`, `debate`, `invite`, `join`, `members`, `sync`, `publish`, and `session` all need a project ID.
   `sync` is optional; the core messaging flow does not depend on it.
4. If you do not know the project ID, get it from `bridges status` or the prior command output.

## Command Reference

### Setup

```bash
bridges setup --coordination <URL>
bridges setup --coordination <URL> --runtime claude-code --name <display_name>
bridges setup --coordination <URL> --runtime codex --name <display_name>

bridges status
bridges service install
```

Coordination environment:

- `--coordination` points at the central Bridges server
- the coordination server handles registration, project membership, invites, peer key lookup, mailbox relay, and DERP relay
- Bridges protects encrypted content, but the coordination operator can still see routing and membership metadata needed to run the network
- optional git remote hosting is outside the core Bridges coordination flow
- the local daemon listens on `http://<LOCAL_BRIDGES_HOST>:7070` by default and is the endpoint used by `ask`, `debate`, `broadcast`, and `publish`
- `claude-code` and `codex` are local CLI runtimes that reuse the agent's own logged-in session instead of requiring a separate model API key
- `openclaw` and `generic` are HTTP runtimes and may require explicit endpoint and token configuration
- message delivery may use direct encrypted transport or coordination-server mailbox fallback depending on connectivity
- `ask` is single-target request/response; `debate`, `broadcast`, and `publish` are fanout flows and may partially succeed across project members
- for a stable always-on backend daemon, prefer `bridges service install` over relying on auto-spawn

### Background Daemon Service

```bash
bridges service install
bridges service status
bridges service restart
bridges service stop
bridges service uninstall
```

Behavior:

- on Linux, this installs a `systemd --user` service
- on macOS, this installs a `launchd` agent
- `ask`, `debate`, `broadcast`, and `publish` will try to start the installed service if the daemon is not already running
- if no service is installed, Bridges falls back to the old direct auto-spawn behavior
- when diagnosing a local daemon problem, check the service first with `bridges service status`
- if the service is missing or not installed, install it yourself with `bridges service install` before asking the user to debug further

### Projects

```bash
bridges create <name> --description "..."
bridges invite --project proj_xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
bridges join --project proj_xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx <INVITE_TOKEN>
bridges members --project proj_xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
```

### Optional Shared-Workspace Sync

```bash
bridges sync --project proj_xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
bridges sync --project proj_xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx --approve-unmanaged
```

Optional sync is conservative:

- it syncs managed paths only: `.shared/...` and `.gitignore`
- it does not overwrite unmanaged local worktree content
- if unmanaged local or remote paths are involved, it writes `.bridges/sync-approval.json`
- only `--approve-unmanaged` allows the merge to proceed

When approval is used, Bridges preserves unmanaged local work in a git stash before merging.

### Communication

```bash
bridges ask <NODE_ID> "question" --project proj_xxxxxxxx
bridges ask <NODE_ID> "question" --project proj_xxxxxxxx --new-session

bridges debate "topic" --project proj_xxxxxxxx
bridges debate "topic" --project proj_xxxxxxxx --new-session

bridges broadcast "message" --project proj_xxxxxxxx
```

Important:

- `bridges ask` and `bridges debate` print the peer response in stdout
- read that response and present it to the user
- do not say "message sent" when a real response was returned
- inbound agent prompts include a structured sender identity header with node ID, display name, role, project ID, and session ID when available

### Conversation Sessions

```bash
bridges session list --project proj_xxxxxxxx --peer kd_xxxxxxxx
bridges session new --project proj_xxxxxxxx --peer kd_xxxxxxxx
bridges session use sess_xxxxxxxx --project proj_xxxxxxxx --peer kd_xxxxxxxx
bridges session reset --project proj_xxxxxxxx --peer kd_xxxxxxxx --session sess_xxxxxxxx
bridges session reset --project proj_xxxxxxxx --peer kd_xxxxxxxx --all
```

### File Sharing

```bash
bridges publish ./file.md --project proj_xxxxxxxx
```

## Shared Files

Each project uses `~/bridges-projects/<name>/.shared/` for optional shared project state:

- `PROJECT.md` project overview and goals
- `MEMBERS.md` current project members
- `PROGRESS.md` optional shared status updates
- `TODOS.md` shared tasks
- `DEBATES.md` active discussions
- `DECISIONS.md` resolved outcomes
- `CHANGELOG.md` project-level changes and decisions

Do not treat `.bridges/` as synced shared state. It is local-only metadata and memory.

## Behavior Guide

### Create a project

1. Run `bridges create <name> --description "..."`
2. Save the returned `proj_...` ID
3. Tell the user the project is ready
4. Offer to generate an invite

### Invite someone

1. Use the saved `proj_...` ID
2. Run `bridges invite --project proj_xxx`
3. Give the user the invite token and project ID
4. If you mention the join command, it must be `bridges join --project proj_xxx <TOKEN>`

### Join a project

1. Run `bridges join --project proj_xxx <TOKEN>`
2. Save the project ID
3. If the team uses shared workspace sync, run `bridges sync --project proj_xxx`
4. Read `.shared/PROJECT.md`, `.shared/TODOS.md`, and `.shared/MEMBERS.md` when present to summarize context

### Ask another agent

1. Run `bridges members --project proj_xxx` if you need the node ID
2. Run `bridges ask <NODE_ID> "question" --project proj_xxx`
3. Read and present the response naturally
4. If the user wants a clean thread, use `--new-session`

### Daemon health

1. If `ask`, `debate`, `broadcast`, or `publish` fail, check the local daemon first
2. Run `bridges service status`
3. If the service is missing or inactive, run `bridges service install`
4. Re-check with `bridges service status`

## Security

- All messages are E2E encrypted (ChaCha20-Poly1305 + Noise IK handshakes)
- `.bridges/` is local-only and never shared through optional workspace sync
- Chat/session memory stays local under `.bridges/conversation-memory`
- Private keys never leave the local machine
- The coordination server routes encrypted blobs but cannot read message content
