"use client";

import { useEffect, useState } from "react";
import { Card, CardTitle, CardDescription } from "@/components/ui/card";
import { listProjects, listMembers, createInvite, type Project, type Member } from "@/lib/api";
import { GITEA_PUBLIC_URL } from "@/lib/public-config";

interface ProjectWithMembers extends Project {
  members?: Member[];
}

export default function ProjectsPage() {
  const [projects, setProjects] = useState<ProjectWithMembers[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [inviteToken, setInviteToken] = useState<string | null>(null);
  const [inviteProjectId, setInviteProjectId] = useState("");
  const [inviting, setInviting] = useState(false);
  const [copied, setCopied] = useState(false);


  useEffect(() => { loadProjects(); }, []);

  async function loadProjects() {
    try {
      const projs = await listProjects();
      const withMembers = await Promise.all(
        projs.map(async (p) => {
          try { return { ...p, members: await listMembers(p.projectId) }; }
          catch { return { ...p, members: [] }; }
        })
      );
      setProjects(withMembers);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load projects");
    } finally { setLoading(false); }
  }

  return (
    <div>
      <div className="mb-6">
        <h1 className="text-[22px] font-semibold tracking-[-0.02em]">Projects</h1>
        <p className="text-[14px] text-muted mt-1">Your collaboration projects</p>
      </div>

      {loading && <div className="text-[14px] text-muted py-8 text-center">Loading...</div>}
      {error && <div className="bg-danger/5 border border-danger/15 text-danger text-[13px] rounded-[6px] px-3.5 py-2.5 mb-4">{error}</div>}

      {inviteToken && (
        <div className="mb-5 soft-card p-5 bg-highlight/30 border-warning/30">
          <p className="text-[13px] font-medium text-foreground mb-3">
            Share this with your collaborator:
          </p>
          <div className="bg-white rounded-[6px] border border-border p-3 font-mono text-[12px] space-y-2">
            <div>
              <span className="text-muted">Project ID: </span>
              <span className="text-foreground break-all">{inviteProjectId}</span>
            </div>
            <div>
              <span className="text-muted">Invite token: </span>
              <span className="text-foreground break-all">{inviteToken}</span>
            </div>
            <div className="pt-2 border-t border-border text-muted">
              They run: <span className="text-foreground">bridges join -p {inviteProjectId} {inviteToken}</span>
            </div>
          </div>
          <div className="flex gap-2 mt-3">
            <button
              onClick={async () => {
                await navigator.clipboard.writeText(`bridges join -p ${inviteProjectId} ${inviteToken}`);
                setCopied(true); setTimeout(() => setCopied(false), 2000);
              }}
              className="text-[12px] px-3 py-1.5 rounded-[6px] bg-surface hover:bg-surface-alt transition"
            >
              {copied ? "Copied \u2713" : "Copy command"}
            </button>
            <button onClick={() => setInviteToken(null)} className="text-[12px] text-muted hover:text-foreground transition">
              Dismiss
            </button>
          </div>
        </div>
      )}

      {!loading && projects.length === 0 && !error && (
        <Card>
          <div className="text-center py-10">
            <p className="text-[15px] font-medium mb-2">No projects yet</p>
            <p className="text-[13px] text-muted mb-6">Create your first project from the CLI.</p>
            <div className="inline-block bg-surface rounded-[6px] p-4 font-mono text-[13px] text-left">
              <div><span className="text-success">$</span> bridges create my-project</div>
            </div>
          </div>
        </Card>
      )}

      <div className="space-y-4">
        {projects.map((project) => (
          <div key={project.projectId} className="soft-card p-5">
            {/* Header row */}
            <div className="flex items-start justify-between mb-3">
              <div>
                <div className="flex items-center gap-2">
                  <CardTitle>{project.displayName || project.slug}</CardTitle>
                  <span className="text-[11px] text-muted font-mono">{project.projectId.slice(0, 18)}...</span>
                </div>
                {project.description && <CardDescription>{project.description}</CardDescription>}
              </div>
              <div className="flex gap-2 flex-shrink-0">
                <button
                  onClick={async () => {
                    setInviting(true); setInviteToken(null); setCopied(false);
                    try {
                      const inv = await createInvite(project.projectId);
                      setInviteToken(inv.inviteToken);
                      setInviteProjectId(project.projectId);
                    } catch { setError("Failed to create invite"); }
                    finally { setInviting(false); }
                  }}
                  disabled={inviting}
                  className="text-[12px] text-muted hover:text-foreground border border-border rounded-[6px] px-3 py-1.5 transition hover:bg-surface"
                >
                  {inviting ? "..." : "Invite"}
                </button>
                {project.giteaOwner && project.giteaRepo && (
                  <a
                    href={`${GITEA_PUBLIC_URL}/${project.giteaOwner}/${project.giteaRepo}`}
                    className="text-[12px] text-muted hover:text-foreground border border-border rounded-[6px] px-3 py-1.5 transition hover:bg-surface"
                  >
                    Open →
                  </a>
                )}
              </div>
            </div>

            {/* Activity row */}
            <div className="flex flex-wrap gap-2 mb-3">
              <div className="text-[12px] text-muted">
                Created {new Date(project.createdAt).toLocaleDateString()}
              </div>
              {project.members && project.members.length > 0 && (
                <div className="text-[12px] text-muted">
                  · {project.members.length} member{project.members.length > 1 ? "s" : ""}
                </div>
              )}
            </div>

            {/* Members */}
            {project.members && project.members.length > 0 && (
              <div className="flex items-center gap-1.5">
                {project.members.map((m) => (
                  <div
                    key={m.nodeId}
                    title={`${m.displayName || m.nodeId} (${m.agentRole || "member"})`}
                    className="w-7 h-7 rounded-full bg-surface-alt flex items-center justify-center text-[11px] font-medium text-muted border border-border"
                  >
                    {(m.displayName || m.nodeId)[0].toUpperCase()}
                  </div>
                ))}
              </div>
            )}

            {/* Quick links */}
            {project.giteaOwner && project.giteaRepo && (
              <div className="flex gap-2 mt-3 pt-3 border-t border-border">
                <a href={`${GITEA_PUBLIC_URL}/${project.giteaOwner}/${project.giteaRepo}/activity`} target="_blank"
                  className="text-[11px] text-muted hover:text-foreground transition">Activity</a>
                <span className="text-[11px] text-border">·</span>
                <a href={`${GITEA_PUBLIC_URL}/${project.giteaOwner}/${project.giteaRepo}/issues`} target="_blank"
                  className="text-[11px] text-muted hover:text-foreground transition">Issues</a>
                <span className="text-[11px] text-border">·</span>
                <a href={`${GITEA_PUBLIC_URL}/${project.giteaOwner}/${project.giteaRepo}/pulls`} target="_blank"
                  className="text-[11px] text-muted hover:text-foreground transition">PRs</a>
                <span className="text-[11px] text-border">·</span>
                <a href={`${GITEA_PUBLIC_URL}/${project.giteaOwner}/${project.giteaRepo}/milestones`} target="_blank"
                  className="text-[11px] text-muted hover:text-foreground transition">Milestones</a>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
