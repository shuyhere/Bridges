"use client";

import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, CardTitle, CardDescription } from "@/components/ui/card";
import { listTokens, createToken, deleteToken, type Token, type CreateTokenResp } from "@/lib/api";
import { COORDINATION_EXAMPLE_URL } from "@/lib/public-config";

export default function TokensPage() {
  const [tokens, setTokens] = useState<Token[]>([]);
  const [newTokenName, setNewTokenName] = useState("");
  const [creating, setCreating] = useState(false);
  const [newToken, setNewToken] = useState<CreateTokenResp | null>(null);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => { loadTokens(); }, []);

  async function loadTokens() {
    try { setTokens(await listTokens()); } catch { setError("Failed to load tokens"); }
  }

  async function handleCreate(e: React.FormEvent) {
    e.preventDefault();
    if (!newTokenName.trim()) return;
    setCreating(true); setError("");
    try {
      const resp = await createToken(newTokenName.trim());
      setNewToken(resp); setNewTokenName(""); await loadTokens();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create token");
    } finally { setCreating(false); }
  }

  async function handleDelete(tokenId: string) {
    if (!confirm("Revoke this token? Any CLI using it will stop working.")) return;
    try { await deleteToken(tokenId); await loadTokens(); } catch { setError("Failed to delete token"); }
  }

  async function handleCopy(text: string) {
    await navigator.clipboard.writeText(text);
    setCopied(true); setTimeout(() => setCopied(false), 2000);
  }

  return (
    <div>
      <div className="mb-8">
        <h1 className="text-[22px] font-semibold tracking-[-0.02em]">API Tokens</h1>
        <p className="text-[14px] text-muted mt-1">
          Generate tokens to authenticate your CLI and agents
        </p>
      </div>

      {/* Create */}
      <Card className="mb-5">
        <CardTitle>Create New Token</CardTitle>
        <CardDescription className="mb-4">Give it a name like &quot;My Laptop&quot; or &quot;CI Server&quot;</CardDescription>
        <form onSubmit={handleCreate} className="flex gap-3">
          <Input placeholder="Token name" value={newTokenName} onChange={(e) => setNewTokenName(e.target.value)} className="flex-1" required />
          <Button type="submit" loading={creating}>Generate</Button>
        </form>
      </Card>

      {/* New token alert */}
      {newToken && (
        <div className="mb-5 soft-card p-5 border-warning/30 bg-highlight/30">
          <p className="text-[13px] font-medium text-foreground mb-3">
            ⚠ Copy your token now — it won&apos;t be shown again
          </p>
          <div className="flex items-center gap-2 bg-white rounded-[6px] border border-border p-3">
            <code className="flex-1 text-[12px] text-foreground break-all font-mono">{newToken.token}</code>
            <button onClick={() => handleCopy(newToken.token)} className="flex-shrink-0 px-3 py-1.5 text-[12px] rounded-[6px] bg-surface hover:bg-surface-alt transition">
              {copied ? "Copied ✓" : "Copy"}
            </button>
          </div>
          <div className="mt-3 bg-white rounded-[6px] border border-border p-3">
            <p className="text-[11px] text-muted mb-1">Setup your CLI with this token:</p>
            <code className="text-[12px] text-foreground font-mono break-all">
              bridges setup --coordination {COORDINATION_EXAMPLE_URL} --token {newToken.token}
            </code>
          </div>
          <button onClick={() => setNewToken(null)} className="mt-2 text-[12px] text-muted hover:text-foreground transition underline">
            Dismiss
          </button>
        </div>
      )}

      {error && (
        <div className="mb-5 bg-danger/5 border border-danger/15 text-danger text-[13px] rounded-[6px] px-3.5 py-2.5">
          {error}
        </div>
      )}

      {/* Token list */}
      <div className="space-y-2">
        {tokens.length === 0 ? (
          <Card>
            <div className="text-center py-10 text-muted text-[14px]">
              <p>No API tokens yet</p>
              <p className="text-[13px] mt-1">Create one above to connect your CLI</p>
            </div>
          </Card>
        ) : (
          tokens.map((token) => (
            <div key={token.tokenId} className="flex items-center justify-between soft-card px-5 py-4">
              <div>
                <div className="text-[14px] font-medium">{token.name}</div>
                <div className="flex items-center gap-3 mt-1 text-[12px] text-muted font-mono">
                  <span>{token.prefix}</span>
                  <span>Created {new Date(token.createdAt).toLocaleDateString()}</span>
                  {token.lastUsedAt && <span>Used {new Date(token.lastUsedAt).toLocaleDateString()}</span>}
                </div>
              </div>
              <button
                onClick={() => handleDelete(token.tokenId)}
                className="text-[12px] text-muted hover:text-danger transition px-3 py-1.5 rounded-[6px] hover:bg-danger/5"
              >
                Revoke
              </button>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
