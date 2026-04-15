"use client";

import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, CardTitle, CardDescription } from "@/components/ui/card";
import { listContacts, addContact, removeContact, listProjects, listMembers, type Contact, type Project, type Member } from "@/lib/api";

interface ProjectWithMembers extends Project {
  members: Member[];
}

export default function ContactsPage() {
  const [contacts, setContacts] = useState<Contact[]>([]);
  const [projects, setProjects] = useState<ProjectWithMembers[]>([]);
  const [loading, setLoading] = useState(true);
  const [nodeId, setNodeId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => { loadAll(); }, []);

  async function loadAll() {
    try {
      const [c, p] = await Promise.all([listContacts(), listProjects()]);
      setContacts(c);
      const withMembers = await Promise.all(
        p.map(async (proj) => {
          try { return { ...proj, members: await listMembers(proj.projectId) }; }
          catch { return { ...proj, members: [] }; }
        })
      );
      setProjects(withMembers);
    } catch {
      setError("Failed to load data");
    } finally { setLoading(false); }
  }

  async function handleAdd(e: React.FormEvent) {
    e.preventDefault();
    if (!nodeId.trim()) return;
    setAdding(true); setError("");
    try {
      const resp = await addContact(nodeId.trim(), displayName.trim() || undefined);
      if (!resp.ok) { setError(resp.message); }
      else { setNodeId(""); setDisplayName(""); await loadAll(); }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to add contact");
    } finally { setAdding(false); }
  }

  async function handleRemove(nid: string) {
    if (!confirm("Remove this contact?")) return;
    try { await removeContact(nid); await loadAll(); }
    catch { setError("Failed to remove contact"); }
  }

  // Collect all unique people: contacts + project members
  const contactIds = new Set(contacts.map((c) => c.nodeId));

  return (
    <div>
      <div className="mb-6">
        <h1 className="text-[22px] font-semibold tracking-[-0.02em]">Contacts</h1>
        <p className="text-[14px] text-muted mt-1">People and agents you can talk to</p>
      </div>

      {/* Add contact */}
      <Card className="mb-5">
        <CardTitle>Add Contact</CardTitle>
        <CardDescription className="mb-4">Enter a node ID to add an agent to your contacts</CardDescription>
        <form onSubmit={handleAdd} className="space-y-3">
          <div className="flex gap-3">
            <Input placeholder="kd_..." value={nodeId} onChange={(e) => setNodeId(e.target.value)} className="flex-1 font-mono" required />
            <Input placeholder="Name (optional)" value={displayName} onChange={(e) => setDisplayName(e.target.value)} className="w-[180px]" />
            <Button type="submit" loading={adding}>Add</Button>
          </div>
        </form>
      </Card>

      {error && (
        <div className="bg-danger/5 border border-danger/15 text-danger text-[13px] rounded-[6px] px-3.5 py-2.5 mb-4">{error}</div>
      )}

      {loading && <div className="text-[14px] text-muted py-8 text-center">Loading...</div>}

      {/* My Contacts */}
      {!loading && (
        <div className="mb-6">
          <div className="step-num mb-3">MY CONTACTS ({contacts.length})</div>
          {contacts.length === 0 ? (
            <div className="soft-card p-5 text-center text-[13px] text-muted">
              No contacts yet. Add a node ID above, or add from CLI: <code className="bg-surface px-1 py-0.5 rounded text-[12px]">bridges contact add kd_xxx</code>
            </div>
          ) : (
            <div className="space-y-2">
              {contacts.map((c) => {
                const name = c.displayName || c.registeredName;
                return (
                  <div key={c.nodeId} className="soft-card px-5 py-3.5 flex items-center justify-between">
                    <div className="flex items-center gap-3">
                      <div className="w-8 h-8 rounded-full bg-surface-alt flex items-center justify-center text-[12px] font-medium text-muted border border-border">
                        {(name || c.nodeId)[0].toUpperCase()}
                      </div>
                      <div>
                        {name && <div className="text-[14px] font-medium">{name}</div>}
                        <div className="text-[11px] text-muted font-mono">{c.nodeId}</div>
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="text-[11px] px-2 py-0.5 rounded-full bg-highlight text-foreground">contact</span>
                      <button onClick={() => handleRemove(c.nodeId)} className="text-[12px] text-muted hover:text-danger transition px-2 py-1 rounded-[6px] hover:bg-danger/5">
                        Remove
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* Project Members */}
      {!loading && projects.length > 0 && (
        <div>
          <div className="step-num mb-3">PROJECT MEMBERS</div>
          <div className="space-y-4">
            {projects.map((proj) => (
              <div key={proj.projectId}>
                <div className="text-[13px] font-medium mb-2 flex items-center gap-2">
                  {proj.displayName || proj.slug}
                  <span className="text-[11px] text-muted font-mono">{proj.projectId.slice(0, 18)}...</span>
                </div>
                <div className="space-y-1.5">
                  {proj.members.map((m) => {
                    const isContact = contactIds.has(m.nodeId);
                    return (
                      <div key={m.nodeId} className="soft-card px-5 py-3 flex items-center justify-between">
                        <div className="flex items-center gap-3">
                          <div className="w-8 h-8 rounded-full bg-surface-alt flex items-center justify-center text-[12px] font-medium text-muted border border-border">
                            {(m.displayName || m.nodeId)[0].toUpperCase()}
                          </div>
                          <div>
                            {m.displayName && <div className="text-[13px] font-medium">{m.displayName}</div>}
                            <div className="text-[11px] text-muted font-mono">{m.nodeId}</div>
                          </div>
                        </div>
                        <div className="flex items-center gap-2">
                          <span className="text-[11px] px-2 py-0.5 rounded-full bg-surface-alt text-muted">
                            {m.agentRole || "member"}
                          </span>
                          {!isContact && (
                            <button
                              onClick={async () => {
                                try {
                                  await addContact(m.nodeId, m.displayName || undefined);
                                  await loadAll();
                                } catch {}
                              }}
                              className="text-[11px] text-muted hover:text-foreground transition px-2 py-0.5 rounded-[6px] border border-border hover:bg-surface"
                            >
                              + Add
                            </button>
                          )}
                          {isContact && (
                            <span className="text-[11px] text-success">✓ contact</span>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* CLI ref */}
      <div className="soft-card p-4 mt-5">
        <div className="step-num mb-2">CLI</div>
        <div className="font-mono text-[11px] text-muted space-y-0.5">
          <div><span className="text-success">$</span> bridges contact add kd_XXXX --name &quot;Alice&quot;</div>
          <div><span className="text-success">$</span> bridges contact list</div>
          <div><span className="text-success">$</span> bridges ask kd_XXXX &quot;Hello!&quot; — direct chat, no project needed</div>
          <div><span className="text-success">$</span> bridges members -p proj_XXX — list project members</div>
        </div>
      </div>
    </div>
  );
}
