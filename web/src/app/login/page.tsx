"use client";

import { useState, useEffect } from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { login, saveSession, ApiError } from "@/lib/api";
import { Suspense } from "react";

function LoginForm() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  // Handle OAuth redirect token
  useEffect(() => {
    const token = searchParams.get("token");
    if (token) {
      saveSession(token);
      router.replace("/dashboard");
    }
  }, [searchParams, router]);

  // Don't render the form if we're handling an OAuth redirect
  if (searchParams.get("token")) {
    return <div className="min-h-screen bg-background" />;
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    setLoading(true);
    try {
      const resp = await login(email, password);
      saveSession(resp.sessionToken);
      router.push("/dashboard");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Something went wrong.");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center px-4">
      <div className="w-full max-w-[380px]">
        <div className="text-center mb-8">
          <Link href="/" className="inline-block mb-6">
            <svg width="28" height="28" viewBox="0 0 26 26" fill="none">
              <path d="M18 11.8C16 11.8 14.3 10.2 14.3 8.1V0h-2.5v.04C11.8 6.6 6.5 11.9 0 11.9v2.4h8.1c2.1 0 3.7 1.7 3.7 3.7v8.1h2.5c0-6.5 5.3-11.8 11.8-11.8v-2.5H18z" fill="#3c3630"/>
            </svg>
          </Link>
          <h1 className="text-[22px] font-semibold tracking-[-0.02em]">Welcome back</h1>
          <p className="text-[14px] text-muted mt-1.5">Sign in to your Bridges account</p>
        </div>

        <div className="soft-card p-6">
          {/* Social login buttons */}
          <div className="space-y-2.5 mb-5">
            <a
              href="/v1/auth/github"
              className="flex items-center justify-center gap-2.5 w-full border border-border rounded-[6px] px-4 py-2.5 text-[13px] font-medium hover:bg-surface transition"
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="#3c3630">
                <path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"/>
              </svg>
              Continue with GitHub
            </a>
            <a
              href="/v1/auth/google"
              className="flex items-center justify-center gap-2.5 w-full border border-border rounded-[6px] px-4 py-2.5 text-[13px] font-medium hover:bg-surface transition"
            >
              <svg width="18" height="18" viewBox="0 0 24 24">
                <path d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 01-2.2 3.32v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.1z" fill="#4285F4"/>
                <path d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z" fill="#34A853"/>
                <path d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18A11.96 11.96 0 001 12c0 1.94.46 3.77 1.18 5.04l3.66-2.95z" fill="#FBBC05"/>
                <path d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z" fill="#EA4335"/>
              </svg>
              Continue with Google
            </a>
          </div>

          <div className="flex items-center gap-3 mb-5">
            <div className="flex-1 h-px bg-border" />
            <span className="text-[12px] text-muted">or</span>
            <div className="flex-1 h-px bg-border" />
          </div>

          <form onSubmit={handleSubmit} className="space-y-4">
            {error && (
              <div className="bg-danger/5 border border-danger/15 text-danger text-[13px] rounded-[6px] px-3.5 py-2.5">
                {error}
              </div>
            )}
            <Input id="email" label="Email" type="email" placeholder="you@example.com" value={email} onChange={(e) => setEmail(e.target.value)} required />
            <Input id="password" label="Password" type="password" placeholder="Your password" value={password} onChange={(e) => setPassword(e.target.value)} required />
            <Button type="submit" className="w-full" loading={loading}>Sign In</Button>
          </form>
        </div>

        <p className="text-center text-[13px] text-muted mt-5">
          Don&apos;t have an account?{" "}
          <Link href="/signup" className="text-foreground underline underline-offset-2">Sign up</Link>
        </p>
      </div>
    </div>
  );
}

export default function LoginPage() {
  return (
    <Suspense fallback={<div className="min-h-screen flex items-center justify-center text-muted">Loading...</div>}>
      <LoginForm />
    </Suspense>
  );
}
