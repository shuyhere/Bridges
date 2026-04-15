"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { cn } from "@/lib/cn";
import { clearSession } from "@/lib/api";
import { AuthProvider, useAuth } from "@/lib/auth-context";

const navItems = [
  { href: "/dashboard", label: "Overview" },
  { href: "/dashboard/tokens", label: "API Tokens" },
  { href: "/dashboard/projects", label: "Projects" },
  { href: "/dashboard/contacts", label: "Contacts" },
  { href: "/dashboard/settings", label: "Settings" },
];

function DashboardShell({ children }: { children: React.ReactNode }) {
  const router = useRouter();
  const pathname = usePathname();
  const { user, loading } = useAuth();

  if (loading) {
    return (
      <div className="min-h-screen bg-background" />
    );
  }

  if (!user) return null;

  return (
    <div className="min-h-screen">
      <header className="border-b border-border bg-white">
        <div className="max-w-[1000px] mx-auto px-6 h-14 flex items-center justify-between">
          <div className="flex items-center gap-6">
            <Link href="/" className="flex items-center gap-2">
              <svg width="20" height="20" viewBox="0 0 26 26" fill="none">
                <path d="M18 11.8C16 11.8 14.3 10.2 14.3 8.1V0h-2.5v.04C11.8 6.6 6.5 11.9 0 11.9v2.4h8.1c2.1 0 3.7 1.7 3.7 3.7v8.1h2.5c0-6.5 5.3-11.8 11.8-11.8v-2.5H18z" fill="#3c3630"/>
              </svg>
              <span className="font-semibold text-[14px]">Bridges</span>
            </Link>
            <nav className="flex items-center gap-1">
              {navItems.map((item) => {
                const isActive = pathname === item.href || (item.href !== "/dashboard" && pathname.startsWith(item.href));
                return (
                  <Link
                    key={item.href}
                    href={item.href}
                    className={cn(
                      "px-3 py-1.5 rounded-[6px] text-[13px] transition-colors duration-150",
                      isActive ? "bg-surface-alt text-foreground font-medium" : "text-muted hover:text-foreground hover:bg-surface",
                    )}
                  >
                    {item.label}
                  </Link>
                );
              })}
            </nav>
          </div>
          <div className="flex items-center gap-3">
            <span className="text-[13px] text-muted">{user.displayName || user.email}</span>
            <button
              onClick={() => { clearSession(); router.push("/login"); }}
              className="text-[13px] text-muted hover:text-foreground transition-colors duration-150"
            >
              Log out
            </button>
          </div>
        </div>
      </header>

      <main className="max-w-[1000px] mx-auto px-6 py-8">
        {children}
      </main>
    </div>
  );
}

export default function DashboardLayout({ children }: { children: React.ReactNode }) {
  return (
    <AuthProvider>
      <DashboardShell>{children}</DashboardShell>
    </AuthProvider>
  );
}
