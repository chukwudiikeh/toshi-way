import type { ReactNode } from "react";
import { panel } from "./dashboard/Widgets";

/** The top of a settings page: an icon tile, the title and a short plain description. */
export function PageHeader({ icon, title, children }: { icon: ReactNode; title: string; children: ReactNode }) {
  return (
    <header className={`${panel} flex items-start gap-4 p-5 sm:p-6`}>
      <span className="grid size-11 shrink-0 place-items-center rounded-xl bg-amber-500 text-black">{icon}</span>
      <div className="min-w-0">
        <h1 className="text-2xl font-bold tracking-tight">{title}</h1>
        <p className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">{children}</p>
      </div>
    </header>
  );
}

const tones = {
  good: { ring: "border-emerald-300 dark:border-emerald-700", dot: "bg-emerald-500", chip: "bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300" },
  warn: { ring: "border-amber-400 dark:border-amber-600", dot: "bg-amber-500", chip: "bg-amber-100 text-amber-800 dark:bg-amber-950 dark:text-amber-300" },
  idle: { ring: "border-neutral-200 dark:border-neutral-800", dot: "bg-neutral-400", chip: "bg-neutral-200 text-neutral-700 dark:bg-neutral-800 dark:text-neutral-300" },
};

/** A status at a glance: a coloured chip with the state, then what it means for the person. */
export function StatusPanel({
  tone,
  label,
  children,
}: {
  tone: keyof typeof tones;
  label: string;
  children: ReactNode;
}) {
  const t = tones[tone];
  return (
    <section className={`${panel} border-2 p-5 sm:p-6 ${t.ring}`}>
      <span className={`inline-flex items-center gap-2 rounded-full px-3 py-1 text-xs font-semibold uppercase tracking-wide ${t.chip}`}>
        <span className={`size-2 rounded-full ${t.dot}`} />
        {label}
      </span>
      <div className="mt-3 space-y-1 text-sm">{children}</div>
    </section>
  );
}

/** A numbered step in a how-to list. */
export function Step({ n, children }: { n: number; children: ReactNode }) {
  return (
    <li className="flex gap-3">
      <span className="grid size-6 shrink-0 place-items-center rounded-full bg-neutral-900 text-xs font-bold text-white dark:bg-neutral-100 dark:text-neutral-900">
        {n}
      </span>
      <div className="min-w-0 flex-1 space-y-2 text-sm">{children}</div>
    </li>
  );
}
