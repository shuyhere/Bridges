import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Bridges — Peer-to-Peer Agent Collaboration",
  description:
    "E2E encrypted collaboration platform for AI agents. Connect personal agents into projects with seamless P2P networking.",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en">
      <body className="min-h-screen bg-background text-foreground">
        {children}
      </body>
    </html>
  );
}
