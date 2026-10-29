"use client";

import Link from "next/link";
import type { ReactNode } from "react";
import { formatSats } from "@/lib/clock";
import { recentActivity, shortDuration, shortId, timeAgo, type Summary, type Todo } from "@/lib/dashboard";
import type { ArbiterKey, Escrow, NodeInfo } from "@/lib/schemas";
import { CopyField, StateBadge } from "../ui";
import { ArrowIcon, BoltIcon, CheckIcon, ClockIcon, KeyIcon, LockIcon, QrIcon, ScaleIcon, ShieldIcon } from "../landing/icons";

export const panel = "rounded-2xl border border-neutral-200 bg-white shadow-sm dark:border-neutral-800 dark:bg-neutral-900/60";

export function RoleBadge({ role }: { role: Escrow["your_role"] }) {
  const style: Record<string, string> = {
    buyer: "bg-sky-100 text-sky-800 dark:bg-sky-950 dark:text-sky-300",
    seller: "bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300",
    arbiter: "bg-violet-100 text-violet-800 dark:bg-violet-950 dark:text-violet-300",
    operator: "bg-neutral-200 text-neutral-700 dark:bg-neutral-800 dark:text-neutral-300",
  };
  return (
    <span className={`inline-flex rounded-md px-2 py-0.5 text-[11px] font-semibold uppercase tracking-wide ${style[role]}`}>
      {role}
    </span>
  );
}

// Stat cards

function Stat({
  label,
  value,
  sub,
  tone = "plain",
  icon,
}: {
  label: string;
  value: ReactNode;
  sub: ReactNode;
  tone?: "plain" | "attention";
  icon: ReactNode;
}) {
  return (
    <div
      className={`${panel} p-5 ${tone === "attention" ? "border-amber-400 bg-amber-50 ring-1 ring-amber-400/40 dark:border-amber-500/60 dark:bg-amber-500/10" : ""}`}
    >
      <div className="flex items-center justify-between text-neutral-500">
        <span className="text-xs font-semibold uppercase tracking-[0.12em]">{label}</span>
        <span className={tone === "attention" ? "text-amber-600 dark:text-amber-400" : ""}>{icon}</span>
      </div>
      <div className="mt-3 font-mono text-3xl font-semibold tabular-nums tracking-tight">{value}</div>
      <div className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">{sub}</div>
    </div>
  );
}

export function StatCards({ summary }: { summary: Summary }) {
  const parts = (...xs: (string | null)[]) => xs.filter(Boolean).join(" · ");
  return (
    <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
      <Stat
        label="Needs you"
        tone={summary.attention > 0 ? "attention" : "plain"}
        icon={<ClockIcon className="size-5" />}
        value={summary.attention}
        sub={summary.attention > 0 ? (summary.attention === 1 ? "move is yours" : "moves are yours") : "Nothing is waiting on you"}
      />
      <Stat
        label="Locked now"
        icon={<LockIcon className="size-5" />}
        value={formatSats(summary.lockedOut + summary.lockedIn).replace(" sats", "")}
        sub={
          summary.lockedCount === 0
            ? "No funds locked"
            : parts(
                `${summary.lockedCount} escrow${summary.lockedCount === 1 ? "" : "s"}`,
                summary.lockedOut ? `${formatSats(summary.lockedOut)} out` : null,
                summary.lockedIn ? `${formatSats(summary.lockedIn)} in` : null,
              )
        }
      />
      <Stat
        label="Completed"
        icon={<CheckIcon className="size-5" />}
        value={summary.settled.count}
        sub={
          summary.settled.count === 0
            ? "None yet"
            : parts(
                summary.settled.received ? `Received ${formatSats(summary.settled.received)}` : null,
                summary.settled.paid ? `Paid ${formatSats(summary.settled.paid)}` : null,
              ) || "Settled"
        }
      />
      <Stat
        label="Refunded"
        icon={<ShieldIcon className="size-5" />}
        value={summary.refunded.count}
        sub={summary.refunded.count === 0 ? "None" : `${formatSats(summary.refunded.sats)} returned`}
      />
    </div>
  );
}

// Needs your attention

const kindIcon = {
  accept: BoltIcon,
  pay: BoltIcon,
  handover: QrIcon,
  deliver: LockIcon,
  confirm: KeyIcon,
  decide: ScaleIcon,
} as const;

export function Attention({ items, now }: { items: Todo[]; now: number }) {
  return (
    <section className={panel} aria-labelledby="attention-title">
      <div className="flex items-center justify-between border-b border-neutral-200 px-5 py-4 dark:border-neutral-800">
        <h2 id="attention-title" className="font-semibold">
          Needs your attention
        </h2>
        {items.length > 0 && (
          <span className="rounded-full bg-amber-500 px-2 py-0.5 text-xs font-bold text-black">{items.length}</span>
        )}
      </div>
      {items.length === 0 ? (
        <div className="flex items-center gap-3 px-5 py-8 text-sm text-neutral-600 dark:text-neutral-400">
          <span className="grid size-9 place-items-center rounded-full bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">
            <CheckIcon className="size-5" />
          </span>
          You&apos;re all caught up. Nothing is waiting on you right now.
        </div>
      ) : (
        <ul className="divide-y divide-neutral-200 dark:divide-neutral-800">
          {items.map((t) => {
            const Icon = kindIcon[t.kind];
            const left = t.by === null ? null : t.by - now;
            const urgent = left !== null && left < 600;
            return (
              <li key={t.escrow.id}>
                <Link
                  href={`/escrows/${t.escrow.id}`}
                  className="group flex items-center gap-4 px-5 py-4 transition-colors hover:bg-neutral-50 dark:hover:bg-neutral-800/40"
                >
                  <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-amber-500/10 text-amber-600 ring-1 ring-amber-500/20 dark:text-amber-400">
                    <Icon className="size-5" />
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="font-medium">{t.title}</div>
                    <div className="truncate text-sm text-neutral-600 dark:text-neutral-400">
                      {t.escrow.description || "(no description)"} · <span className="font-mono">{formatSats(t.escrow.amount_sats)}</span>
                    </div>
                    <div className="text-xs text-neutral-500">{t.detail}</div>
                    {/* On a phone the right-hand column is hidden, so the time left goes here. */}
                    {left !== null && (
                      <div className={`mt-1 font-mono text-xs tabular-nums sm:hidden ${urgent ? "font-semibold text-red-600 dark:text-red-400" : "text-neutral-500"}`}>
                        {left > 0 ? `${shortDuration(left)} left` : "now"}
                      </div>
                    )}
                  </div>
                  <div className="hidden shrink-0 text-right sm:block">
                    <RoleBadge role={t.escrow.your_role} />
                    {left !== null && (
                      <div className={`mt-1 font-mono text-sm tabular-nums ${urgent ? "font-semibold text-red-600 dark:text-red-400" : "text-neutral-600 dark:text-neutral-400"}`}>
                        {left > 0 ? `${shortDuration(left)} left` : "now"}
                      </div>
                    )}
                  </div>
                  <ArrowIcon className="size-4 shrink-0 text-neutral-400 transition group-hover:translate-x-0.5 group-hover:text-amber-600" />
                </Link>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

// Side panels

function SideCard({ title, children, action }: { title: string; children: ReactNode; action?: ReactNode }) {
  return (
    <section className={`${panel} p-5`}>
      <div className="flex items-center justify-between">
        <h2 className="text-xs font-semibold uppercase tracking-[0.12em] text-neutral-500">{title}</h2>
        {action}
      </div>
      <div className="mt-3">{children}</div>
    </section>
  );
}

const linkClass = "text-sm font-semibold text-amber-700 hover:underline dark:text-amber-400";

export function AccountCard({ userId }: { userId: string | null }) {
  return (
    <SideCard title="Your account">
      {userId ? (
        <div className="space-y-2">
          <div className="text-sm text-neutral-600 dark:text-neutral-400">Your user id</div>
          <CopyField value={userId} />
          <p className="text-xs text-neutral-500">Not secret. Give it to a seller so they can make you an offer.</p>
        </div>
      ) : (
        <p className="text-sm text-neutral-600 dark:text-neutral-400">
          Your user id isn&apos;t stored on this device yet. It appears here once you have an escrow.
        </p>
      )}
    </SideCard>
  );
}

export function NodeCard({ node }: { node: NodeInfo | null }) {
  if (!node) return null;
  const own = node.mode === "own";
  return (
    <SideCard
      title="Lightning node"
      action={
        <Link href="/node" className={linkClass}>
          {own ? "Manage" : "Connect"}
        </Link>
      }
    >
      <div className="flex items-start gap-3">
        <span
          className={`mt-0.5 size-2.5 shrink-0 rounded-full ${own ? "bg-emerald-500" : node.mode === "shared" ? "bg-amber-500" : "bg-red-500"}`}
        />
        <div className="min-w-0 text-sm">
          {own ? (
            <>
              <div className="font-medium">Your own node is connected</div>
              <div className="truncate font-mono text-xs text-neutral-500">{node.address}</div>
              <div className="mt-1 text-xs text-neutral-500">Released payments land with you directly.</div>
            </>
          ) : node.mode === "shared" ? (
            <>
              <div className="font-medium">Using the shared node</div>
              <div className="mt-1 text-xs text-neutral-500">
                The service receives your sats and owes them to you. Connect your own node to be paid directly.
              </div>
            </>
          ) : (
            <>
              <div className="font-medium">No node connected</div>
              <div className="mt-1 text-xs text-neutral-500">Connect one to create escrows as a seller.</div>
            </>
          )}
        </div>
      </div>
    </SideCard>
  );
}

export function ArbiterCard({ key_, asArbiter }: { key_: ArbiterKey | null; asArbiter: number }) {
  if (!key_) return null;
  const ready = !!key_.fingerprint;
  return (
    <SideCard
      title="Arbiter"
      action={
        <Link href="/arbiter" className={linkClass}>
          {ready ? "Manage" : "Set up"}
        </Link>
      }
    >
      <div className="text-sm">
        {ready ? (
          <>
            <div className="font-medium">Ready to be named as an arbiter</div>
            <div className="mt-1 break-words font-mono text-[11px] text-neutral-500">{key_.fingerprint?.slice(0, 29)}…</div>
            {asArbiter > 0 && <div className="mt-1 text-xs text-neutral-500">{asArbiter} escrow{asArbiter === 1 ? "" : "s"} name you.</div>}
          </>
        ) : (
          <div className="text-neutral-600 dark:text-neutral-400">
            Be the trusted referee in other people&apos;s escrows. Set up a key to be named as an arbiter.
          </div>
        )}
      </div>
    </SideCard>
  );
}

export function Activity({ escrows, now }: { escrows: Escrow[]; now: number }) {
  const items = recentActivity(escrows, 5);
  if (items.length === 0) return null;
  return (
    <SideCard title="Recent activity">
      <ul className="space-y-3">
        {items.map((e) => (
          <li key={e.id}>
            <Link href={`/escrows/${e.id}`} className="group block">
              <div className="flex items-center justify-between gap-2">
                <span className="truncate text-sm font-medium group-hover:underline">{e.description || `#${shortId(e.id)}`}</span>
                <span className="shrink-0 text-xs text-neutral-500">{timeAgo(now, e.updated_at)}</span>
              </div>
              <div className="mt-1">
                <StateBadge state={e.state} />
              </div>
            </Link>
          </li>
        ))}
      </ul>
    </SideCard>
  );
}
