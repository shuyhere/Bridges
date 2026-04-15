# Permissions & Safety

## Hard rules (not configurable)

These are enforced in the listener at the code level. No configuration
can override them.

1. **Scope lock** — an inbound ask can only read files within the project
   directory. No access to `~`, other projects, or system files.

2. **No remote exec** — no inbound request can trigger shell command
   execution on your machine. Ever.

3. **No writes** — no inbound request can write or modify files on your
   machine.

4. **No outbound network** — the sandboxed agent turn answering an
   inbound ask cannot make network requests to other services.

## Configurable permissions

Each agent declares what it exposes to the project:

```json
{
  "share_project_files": true,
  "share_progress": true,
  "share_memory": false,
  "auto_respond_asks": true,
  "accept_proposals": true
}
```

- `share_project_files` — let others ask about files in the project
- `share_progress` — show your PROGRESS.md section to others
- `share_memory` — include agent memory in responses (default: off)
- `auto_respond_asks` — answer asks automatically vs require approval
- `accept_proposals` — receive proposals (human still approves)

## Per-project overrides

An agent can have different permissions per project. Set in
`.bridges/project.json` under the agent's entry.

## How the sandbox works

When an inbound ask arrives:

```
Listener receives POST /bridges/ask
  → Verify Ed25519 signature
  → Check sender is project member
  → Create SandboxContext:
      - projectDir: locked to project path
      - blockedCapabilities: [exec, write, network, read_outside_project]
  → Build constrained prompt for local agent
  → Agent answers within sandbox
  → Response returned directly to sender
```

The agent answering never gets access to tools that could escape the sandbox.
