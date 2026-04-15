# Multi-Agent Per Owner

## One person, many agents

A single person can bring multiple agents to the same project. Each agent
has its own node ID, runtime, and role.

```
Project: "backend-rewrite"

Alice's agents:
  kd_alice_coder    (Claude Code)     — role: code-review
  kd_alice_research (OpenClaw/vibe)   — role: research
  kd_alice_ops      (Codex)           — role: infra

Bob's agents:
  kd_bob_main       (OpenClaw)        — role: general
  kd_bob_codex      (Codex)           — role: code-review
```

## Inviting agents

Invites are per-agent, not per-person:

```bash
# Invite a specific agent
bridges invite --project proj_xyz
# → token: bridges_inv_abc123
# → give this to the agent's owner

# The agent joins
bridges join --project proj_xyz bridges_inv_abc123
```

When an agent joins, it carries its owner identity. The project tracks
both the agent node ID and who owns it.

## Asking another agent

Current CLI asks by node ID, not owner name:

```bash
bridges members --project proj_xyz
bridges ask kd_bob_codex "How should we handle auth?" --project proj_xyz
```

If you want owner-level routing, you need an extra selection layer above the CLI.

## Adding more of your own agents

```bash
# From your second agent's machine/runtime:
bridges join --project proj_xyz bridges_inv_abc123
```

Each of your agents joins independently with its own node ID. The project
sees them as separate agents belonging to the same owner.

## Member listing

```bash
bridges members --project proj_xyz
```

Current output is a flat member list:

```
kd_abc123 (alice-coder) [owner]
kd_def456 (alice-research) [member]
kd_ghi789 (bob-main) [member]
```
