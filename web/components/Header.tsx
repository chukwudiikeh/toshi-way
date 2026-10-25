"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import { api } from "@/lib/api";
import { clearSession, useSession } from "@/lib/session";

type NodeStatus = "up" | "down" | "unreachable" | null;

function useNodeStatus(): NodeStatus {
  const [status, setStatus] = useState<NodeStatus>(null);
  useEffect(() => {
    let alive = true;
    const check = () =>
      api
        .health()
        .then((h) => alive && setStatus(h.node === "up" ? "up" : "down"))
        .catch(() => alive && setStatus("unreachable"));
    check();
    const t = setInterval(check, 15000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);
  return status;
}

const statusText = {
  up: { dot: "bg-emerald-500", text: "Node online" },
  down: { dot: "bg-amber-500", text: "Node unavailable" },
  unreachable: { dot: "bg-red-500", text: "Server unreachable" },
};

export function Header() {
  const session = useSession();
  const node = useNodeStatus();
  // On the dashboard the page is wider, so the header lines up with it.
  const wide = usePathname() === "/";

  return (
    <header className="border-b border-neutral-200 bg-white/80 backdrop-blur dark:border-neutral-800 dark:bg-neutral-950/80">
      <div className={`mx-auto flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3 ${wide ? "max-w-6xl sm:px-6" : "max-w-3xl"}`}>
        <Link href="/" className="flex items-center gap-2 font-bold tracking-tight">
          <span className="grid size-7 place-items-center rounded-md bg-amber-500 text-sm text-black">⚡</span>
          toshi-way
        </Link>
        <nav className="flex flex-wrap items-center gap-1 text-sm">
          {session?.kind === "user" && (
            <>
              <Link href="/" className="rounded-md px-2 py-1 hover:bg-neutral-100 dark:hover:bg-neutral-800">
                Dashboard
              </Link>
              <Link
                href="/escrows/new"
                className="rounded-md px-2 py-1 hover:bg-neutral-100 dark:hover:bg-neutral-800"
              >
                New escrow
              </Link>
              <Link href="/node" className="rounded-md px-2 py-1 hover:bg-neutral-100 dark:hover:bg-neutral-800">
                My node
              </Link>
              <Link href="/arbiter" className="rounded-md px-2 py-1 hover:bg-neutral-100 dark:hover:bg-neutral-800">
                Arbiter
              </Link>
            </>
          )}
          {session?.kind === "operator" && (
            <Link href="/operator" className="rounded-md px-2 py-1 hover:bg-neutral-100 dark:hover:bg-neutral-800">
              Operator queue
            </Link>
          )}
        </nav>
        <div className="ml-auto flex items-center gap-3 text-xs">
          {node && (
            <span className="flex items-center gap-1.5 text-neutral-500" title={statusText[node].text}>
              <span className={`size-2 rounded-full ${statusText[node].dot}`} />
              <span className="hidden sm:inline">{statusText[node].text}</span>
            </span>
          )}
          {session && (
            <button
              type="button"
              onClick={clearSession}
              className="rounded-md px-2 py-1 font-medium text-neutral-600 hover:bg-neutral-100 dark:text-neutral-300 dark:hover:bg-neutral-800"
            >
              Sign out
            </button>
          )}
        </div>
      </div>
    </header>
  );
}
