"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState, type ReactNode } from "react";
import { api } from "@/lib/api";
import { clearSession, useSession, useVaultState } from "@/lib/session";
import { BoltIcon, CodeIcon, KeyIcon, ScaleIcon, ShieldIcon, WalletIcon } from "./landing/icons";

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

const GridIcon = (p: React.SVGProps<SVGSVGElement>) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.7} strokeLinejoin="round" aria-hidden {...p}>
    <rect x="4" y="4" width="6.5" height="6.5" rx="1.5" />
    <rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5" />
    <rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5" />
    <rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5" />
  </svg>
);
const PlusIcon = (p: React.SVGProps<SVGSVGElement>) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" aria-hidden {...p}>
    <path d="M12 5v14M5 12h14" />
  </svg>
);

function NavLink({ href, icon, children, onClick }: { href: string; icon: ReactNode; children: ReactNode; onClick: () => void }) {
  const path = usePathname();
  const active = href === "/" ? path === "/" : path.startsWith(href);
  return (
    <Link
      href={href}
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={`flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-colors ${
        active ? "bg-white/10 text-white" : "text-neutral-400 hover:bg-white/5 hover:text-white"
      }`}
    >
      <span className="size-[18px] shrink-0 [&>svg]:size-full">{icon}</span>
      {children}
    </Link>
  );
}

/**
 * The app chrome's navigation: a dark rail down the left on wide screens, and a top bar that opens
 * the same rail as a drawer on narrow ones. Everything the signed-in person can reach lives here.
 */
export function Sidebar() {
  const session = useSession();
  const vault = useVaultState();
  const node = useNodeStatus();
  const [open, setOpen] = useState(false);
  const close = () => setOpen(false);

  return (
    <>
      <div className="sticky top-0 z-30 flex items-center justify-between bg-neutral-950 px-4 py-3 text-white lg:hidden">
        <Brand />
        <button
          type="button"
          aria-label={open ? "Close menu" : "Open menu"}
          aria-expanded={open}
          onClick={() => setOpen(!open)}
          className="grid size-10 place-items-center rounded-lg hover:bg-white/10"
        >
          <svg viewBox="0 0 24 24" className="size-6" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" aria-hidden>
            {open ? <path d="m6 6 12 12M18 6 6 18" /> : <path d="M4 7h16M4 12h16M4 17h16" />}
          </svg>
        </button>
      </div>
      {open && <div className="fixed inset-0 z-30 bg-black/50 lg:hidden" onClick={close} aria-hidden />}
      <aside
        className={`fixed inset-y-0 left-0 z-40 flex w-64 flex-col overflow-y-auto border-r border-white/10 bg-neutral-950 text-white transition-transform lg:sticky lg:top-0 lg:h-screen lg:shrink-0 lg:translate-x-0 ${
          open ? "translate-x-0" : "-translate-x-full"
        }`}
      >
        <div className="hidden border-b border-white/10 px-5 py-5 lg:block">
          <Brand />
        </div>
        <div className="space-y-2 p-4">
          {session?.kind === "user" ? (
            <Link
              href="/escrows/new"
              onClick={close}
              className="flex min-h-11 items-center justify-center gap-2 rounded-lg bg-amber-500 px-4 text-sm font-semibold text-black shadow-sm hover:bg-amber-400"
            >
              <PlusIcon className="size-4" /> Create escrow
            </Link>
          ) : !session ? (
            <Link
              href="/"
              onClick={close}
              className="flex min-h-11 items-center justify-center rounded-lg bg-amber-500 px-4 text-sm font-semibold text-black hover:bg-amber-400"
            >
              Create account
            </Link>
          ) : null}
        </div>
        <nav aria-label="Main" className="flex-1 px-3 pb-4">
          {session && (
            <>
              <div className="px-3 pb-2 pt-3 text-[11px] font-semibold uppercase tracking-[0.14em] text-neutral-500">Menu</div>
              <div className="space-y-1">
                {session.kind === "user" && (
                  <>
                    <NavLink href="/" icon={<GridIcon />} onClick={close}>Dashboard</NavLink>
                    <NavLink href="/escrows/new" icon={<BoltIcon />} onClick={close}>New escrow</NavLink>
                    <NavLink href="/balance" icon={<WalletIcon />} onClick={close}>Balance</NavLink>
                    <NavLink href="/node" icon={<KeyIcon />} onClick={close}>My node</NavLink>
                    <NavLink href="/arbiter" icon={<ScaleIcon />} onClick={close}>Arbiter</NavLink>
                    <NavLink href="/developers" icon={<CodeIcon />} onClick={close}>API</NavLink>
                    <NavLink href="/account" icon={<ShieldIcon />} onClick={close}>Account</NavLink>
                  </>
                )}
                {session.kind === "operator" && (
                  <NavLink href="/operator" icon={<ShieldIcon />} onClick={close}>Operator queue</NavLink>
                )}
              </div>
            </>
          )}
        </nav>
        <div className="space-y-3 border-t border-white/10 p-4 text-xs">
          {node && (
            <span className="flex items-center gap-2 text-neutral-400">
              <span className={`size-2 rounded-full ${statusText[node].dot}`} />
              {statusText[node].text}
            </span>
          )}
          {session && (
            <button
              type="button"
              onClick={clearSession}
              className="w-full rounded-lg px-3 py-2 text-left text-sm font-medium text-neutral-300 hover:bg-white/10 hover:text-white"
            >
              {vault === "unprotected" ? "Sign out" : "Lock"}
            </button>
          )}
        </div>
      </aside>
    </>
  );
}

function Brand() {
  return (
    <Link href="/" className="flex items-center gap-2 font-bold tracking-tight">
      <span className="grid size-8 place-items-center rounded-lg bg-amber-500 text-black">
        <BoltIcon className="size-4" />
      </span>
      toshi-way
    </Link>
  );
}
