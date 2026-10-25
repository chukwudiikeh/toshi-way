"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { formatSats } from "@/lib/clock";
import {
  filterEscrows,
  isActive,
  nextLine,
  roleCounts,
  shortDuration,
  shortId,
  timeAgo,
  type RoleFilter,
  type Tab,
} from "@/lib/dashboard";
import { formatFiat } from "@/lib/fiat";
import type { Escrow } from "@/lib/schemas";
import { StateBadge } from "../ui";
import { panel, RoleBadge } from "./Widgets";

const TABS: { id: Tab; label: string }[] = [
  { id: "active", label: "Active" },
  { id: "completed", label: "Completed" },
  { id: "all", label: "All" },
];

export function EscrowTable({ escrows, now }: { escrows: Escrow[]; now: number }) {
  const [tab, setTab] = useState<Tab>("active");
  const [role, setRole] = useState<RoleFilter>("all");

  const counts = useMemo(
    () => ({
      active: escrows.filter(isActive).length,
      completed: escrows.filter((e) => !isActive(e)).length,
      all: escrows.length,
    }),
    [escrows],
  );
  const roles = useMemo(() => roleCounts(escrows), [escrows]);
  const rows = useMemo(() => filterEscrows(escrows, tab, role), [escrows, tab, role]);
  // Only offer role filters for roles the person actually has.
  const roleChips = (["all", "buyer", "seller", "arbiter"] as RoleFilter[]).filter((r) => r === "all" || roles[r] > 0);

  return (
    <section className={panel} aria-labelledby="escrows-title">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-neutral-200 px-5 py-4 dark:border-neutral-800">
        <h2 id="escrows-title" className="font-semibold">
          My escrows
        </h2>
        <div role="tablist" aria-label="Which escrows" className="flex rounded-lg bg-neutral-100 p-1 text-sm dark:bg-neutral-800">
          {TABS.map((t) => (
            <button
              key={t.id}
              role="tab"
              type="button"
              aria-selected={tab === t.id}
              onClick={() => setTab(t.id)}
              className={`rounded-md px-3 py-1 font-medium transition-colors ${
                tab === t.id
                  ? "bg-white shadow-sm dark:bg-neutral-950"
                  : "text-neutral-600 hover:text-neutral-900 dark:text-neutral-400 dark:hover:text-neutral-100"
              }`}
            >
              {t.label} <span className="text-xs text-neutral-500">{counts[t.id]}</span>
            </button>
          ))}
        </div>
      </div>

      {roleChips.length > 2 && (
        <div className="flex flex-wrap gap-2 border-b border-neutral-200 px-5 py-3 text-xs dark:border-neutral-800">
          {roleChips.map((r) => (
            <button
              key={r}
              type="button"
              aria-pressed={role === r}
              onClick={() => setRole(r)}
              className={`rounded-full px-3 py-1 font-medium ring-1 transition-colors ${
                role === r
                  ? "bg-amber-500 text-black ring-amber-500"
                  : "text-neutral-600 ring-neutral-300 hover:bg-neutral-100 dark:text-neutral-400 dark:ring-neutral-700 dark:hover:bg-neutral-800"
              }`}
            >
              {r === "all" ? "All roles" : r[0].toUpperCase() + r.slice(1)} · {roles[r]}
            </button>
          ))}
        </div>
      )}

      {rows.length === 0 ? (
        <p className="px-5 py-10 text-center text-sm text-neutral-500">
          {escrows.length === 0
            ? "No escrows yet."
            : tab === "active"
              ? "Nothing active right now."
              : tab === "completed"
                ? "Nothing completed yet."
                : "Nothing matches that filter."}
        </p>
      ) : (
        <ul className="divide-y divide-neutral-200 dark:divide-neutral-800">
          {rows.map((e) => {
            const left = e.deadline_at - now;
            return (
              <li key={e.id}>
                <Link
                  href={`/escrows/${e.id}`}
                  className="grid items-center gap-x-4 gap-y-1 px-5 py-4 transition-colors hover:bg-neutral-50 md:grid-cols-[minmax(0,1fr)_auto_auto] dark:hover:bg-neutral-800/40"
                >
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="truncate font-medium">{e.description || "(no description)"}</span>
                      <span className="hidden shrink-0 font-mono text-xs text-neutral-400 sm:inline">#{shortId(e.id)}</span>
                    </div>
                    <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-neutral-500">
                      <RoleBadge role={e.your_role} />
                      <span>{nextLine(e)}</span>
                    </div>
                  </div>
                  <div className="md:text-right">
                    <div className="font-mono text-sm font-medium tabular-nums">{formatSats(e.amount_sats)}</div>
                    {e.fiat && (
                      <div className="text-xs text-neutral-500" title="Value when the offer was made. Display only.">
                        ≈ {formatFiat(e.fiat.amount_minor, e.fiat.currency, e.fiat.exponent)}
                      </div>
                    )}
                  </div>
                  <div className="flex items-center gap-3 md:min-w-44 md:flex-col md:items-end md:gap-1">
                    <StateBadge state={e.state} />
                    <span className="text-xs text-neutral-500">
                      {isActive(e) ? (left > 0 ? `${shortDuration(left)} left` : "deadline passed") : timeAgo(now, e.updated_at)}
                    </span>
                  </div>
                </Link>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
