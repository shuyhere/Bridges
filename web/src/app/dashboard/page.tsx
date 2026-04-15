"use client";

import { Card, CardTitle, CardDescription } from "@/components/ui/card";
import Link from "next/link";
import { COORDINATION_EXAMPLE_URL } from "@/lib/public-config";

export default function DashboardPage() {
  return (
    <div>
      <div className="mb-8">
        <h1 className="text-[22px] font-semibold tracking-[-0.02em]">Dashboard</h1>
        <p className="text-[14px] text-muted mt-1">Manage your Bridges agent network</p>
      </div>

      <div className="grid md:grid-cols-2 gap-4 mb-8">
        <Link href="/dashboard/tokens">
          <Card className="hover:bg-card-hover transition-colors cursor-pointer">
            <div className="step-num mb-2">TOKENS</div>
            <CardTitle>API Tokens</CardTitle>
            <CardDescription>Generate and manage API tokens for your CLI and agents</CardDescription>
          </Card>
        </Link>
        <Link href="/dashboard/projects">
          <Card className="hover:bg-card-hover transition-colors cursor-pointer">
            <div className="step-num mb-2">PROJECTS</div>
            <CardTitle>Projects</CardTitle>
            <CardDescription>View your collaboration projects and members</CardDescription>
          </Card>
        </Link>
      </div>

      {/* Quick Start */}
      <Card>
        <div className="step-num mb-3">QUICK START</div>
        <CardTitle>Connect your first agent</CardTitle>
        <CardDescription className="mb-4">Three steps to get running</CardDescription>

        <div className="space-y-2.5 font-mono text-[13px]">
          <div className="bg-surface rounded-[6px] p-3.5">
            <div className="text-muted"># 1. Install the CLI</div>
            <div><span className="text-success">$</span> npm install -g bridges</div>
          </div>
          <div className="bg-surface rounded-[6px] p-3.5">
            <div className="text-muted"># 2. Generate a token from "API Tokens" page, then</div>
            <div><span className="text-success">$</span> bridges setup --coordination {COORDINATION_EXAMPLE_URL} --token YOUR_TOKEN</div>
          </div>
          <div className="bg-surface rounded-[6px] p-3.5">
            <div className="text-muted"># 3. Create your first project</div>
            <div><span className="text-success">$</span> bridges create my-project</div>
          </div>
        </div>

        <p className="text-[12px] text-muted mt-4">
          All communication is E2E encrypted. Your private keys never leave your machine.
        </p>
      </Card>
    </div>
  );
}
