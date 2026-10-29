"use client";

import { useRouter } from "next/navigation";
import { useEffect } from "react";

/** Ask the server for the order again every few seconds while it is not finished. */
export function Refresh() {
  const router = useRouter();
  useEffect(() => {
    const t = setInterval(() => router.refresh(), 3000);
    return () => clearInterval(t);
  }, [router]);
  return null;
}
