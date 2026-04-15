"use client";

import { useSearchParams } from "next/navigation";
import { Suspense } from "react";
import { GITEA_PUBLIC_URL } from "@/lib/public-config";

function GitFrame() {
  const searchParams = useSearchParams();
  const path = searchParams.get("path") || "/";
  const src = `${GITEA_PUBLIC_URL}${path}`;

  return (
    <div className="fixed inset-0 top-[57px] bg-background">
      <iframe
        src={src}
        className="w-full h-full border-none"
        title="Bridges Git"
      />
    </div>
  );
}

export default function GitPage() {
  return (
    <Suspense fallback={<div className="py-8 text-muted text-center text-[14px]">Loading...</div>}>
      <GitFrame />
    </Suspense>
  );
}
