# Bridges identity lifecycle

Bridges node IDs are derived from the local Ed25519 identity key.

That means a real key change also changes the node ID.

## V1 contract

In Bridges V1, "key rotation" is modeled as **node replacement + revocation**:

- the replacement node generates a fresh keypair
- the replacement node registers normally and gets a new `kd_...` node ID
- the old node asks coordination to replace it with the new node
- project memberships migrate from the old node to the replacement node
- the old node is marked revoked
- revoked nodes stop authenticating for normal node API use
- revoked nodes stop appearing in key and endpoint lookups

## What `bridges identity rotate` does

`bridges identity rotate` currently:

1. generates a fresh local identity
2. registers the replacement node with coordination
3. asks coordination to migrate memberships to the replacement node
4. revokes the old node
5. saves the new keypair locally
6. updates `~/.bridges/config.json`
7. restarts the local daemon service when supported

## What `bridges identity revoke` does

`bridges identity revoke` marks the current node as revoked on the coordination server and clears the local API key from client config.

After revocation, the node should no longer be treated as an active Bridges identity.

## Current trust model

A revoked node:

- cannot continue authenticating with its old API key
- is hidden from key lookup
- is hidden from endpoint lookup
- may still appear in historical local state until local caches are refreshed or rewritten

A replacement node:

- must already be registered
- must currently be active
- must not already be participating in project memberships during replacement

## Current guarantees

Bridges currently guarantees that:

- replacement is coordinated through the server, not guessed by peers
- membership migration preserves the member role for migrated projects
- old node API keys are invalidated when the old node is revoked/replaced

## Current non-guarantees

Bridges does **not** yet guarantee:

- seamless continuity of in-flight sessions
- mailbox re-binding or replay across a replacement event
- automatic peer-side cache refresh without a fresh lookup/diagnostic cycle
- federation-wide revocation propagation
- preservation of the old node ID after key change

## Operational guidance

Use:

```bash
bridges identity status
bridges identity rotate
bridges identity revoke --reason "compromised"
bridges doctor
```

After a rotation or revocation, run `bridges doctor` to confirm local daemon health and coordination identity state.
