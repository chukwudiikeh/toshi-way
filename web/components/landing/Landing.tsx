"use client";

import Link from "next/link";
import type { ComponentType, ReactNode, SVGProps } from "react";
import type { NewUser } from "@/lib/schemas";
import { CreateAccount, SignIn } from "../Account";
import { EscrowCard } from "./EscrowCard";
import {
  ArrowIcon,
  BoltIcon,
  CheckIcon,
  EyeIcon,
  KeyIcon,
  LockIcon,
  QrIcon,
  ScaleIcon,
  ShieldIcon,
  UndoIcon,
} from "./icons";

type IconType = ComponentType<SVGProps<SVGSVGElement>>;

const REPO = "https://github.com/yahia008/toshi-way";
const wrap = "mx-auto w-full max-w-6xl px-5 sm:px-8";

/** The landing page for someone who is not signed in. `onCreated` hands a new account to the token screen. */
export function Landing({ onCreated }: { onCreated: (user: NewUser) => void }) {
  return (
    <>
      <Hero />
      <main>
        <Sides />
        <HowItWorks />
        <Outcomes />
        <Trust />
        <Limits />
        <GetStarted onCreated={onCreated} />
      </main>
      <SiteFooter />
    </>
  );
}

// Hero

function Hero() {
  return (
    <section className="landing-hero relative isolate overflow-hidden bg-neutral-950 text-neutral-50">
      <div aria-hidden className="landing-grid absolute inset-0 -z-10" />
      <div aria-hidden className="landing-glow absolute -top-40 left-1/2 -z-10 h-[34rem] w-[34rem] -translate-x-1/2 rounded-full bg-amber-500/25 blur-3xl" />

      <header className={`${wrap} flex items-center justify-between gap-4 py-5`}>
        <Link href="/" className="flex items-center gap-2 font-bold tracking-tight">
          <span className="grid size-8 place-items-center rounded-lg bg-amber-500 text-black shadow-lg shadow-amber-500/30">
            <BoltIcon className="size-4" />
          </span>
          toshi-way
        </Link>
        <nav aria-label="Main" className="hidden items-center gap-7 text-sm text-neutral-300 md:flex">
          <a href="#how" className="transition-colors hover:text-white">How it works</a>
          <a href="#outcomes" className="transition-colors hover:text-white">Outcomes</a>
          <a href="#trust" className="transition-colors hover:text-white">Why trust it</a>
        </nav>
        <div className="flex items-center gap-2 text-sm">
          <a href="#start" className="rounded-lg px-3 py-2 font-medium text-neutral-200 transition-colors hover:text-white">
            Sign in
          </a>
          <a
            href="#start"
            className="rounded-lg bg-amber-500 px-4 py-2 font-semibold text-black shadow-lg shadow-amber-500/20 transition hover:bg-amber-400"
          >
            Get started
          </a>
        </div>
      </header>

      <div className={`${wrap} grid items-center gap-12 pb-20 pt-10 sm:pt-16 lg:grid-cols-[1.05fr_0.95fr] lg:gap-10 lg:pb-28`}>
        <div className="landing-rise">
          <span className="inline-flex items-center gap-2 rounded-full bg-white/5 px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.14em] text-amber-300 ring-1 ring-white/10">
            <span className="size-1.5 rounded-full bg-amber-400" />
            Non-custodial · Built on Lightning
          </span>
          <h1 className="mt-6 text-balance text-4xl font-bold leading-[1.05] tracking-tight sm:text-5xl lg:text-[3.5rem]">
            Escrow where{" "}
            <span className="bg-gradient-to-r from-amber-200 via-amber-400 to-orange-400 bg-clip-text text-transparent">
              nobody holds the money.
            </span>
          </h1>
          <p className="mt-6 max-w-xl text-lg leading-relaxed text-neutral-300">
            Lock a Bitcoin Lightning payment until the goods arrive. The seller is paid when the buyer hands over a
            secret. If they never do, the money goes back automatically.
          </p>
          <div className="mt-8 flex flex-wrap items-center gap-3">
            <a
              href="#start"
              className="inline-flex w-full items-center justify-center gap-2 rounded-xl bg-amber-500 px-6 py-3 text-base font-semibold text-black shadow-lg shadow-amber-500/25 transition hover:bg-amber-400 sm:w-auto"
            >
              Get started <ArrowIcon className="size-4" />
            </a>
            <a
              href="#how"
              className="inline-flex w-full items-center justify-center rounded-xl px-6 py-3 text-base font-semibold text-neutral-100 ring-1 ring-white/20 transition hover:bg-white/5 sm:w-auto"
            >
              See how it works
            </a>
          </div>
          <ul className="mt-8 flex flex-wrap gap-2.5">
            {["Hold invoices", "Automatic refunds", "Optional arbiter", "Your own node"].map((t) => (
              <li key={t} className="rounded-full bg-white/5 px-3 py-1 text-xs font-medium text-neutral-300 ring-1 ring-white/10">
                {t}
              </li>
            ))}
          </ul>
          <p className="mt-6 text-xs text-neutral-500">Runs on a test network today: there is no real money in it yet.</p>
        </div>

        <div className="landing-rise [animation-delay:120ms]">
          <EscrowCard />
        </div>
      </div>
    </section>
  );
}

// Building blocks

function Section({
  id,
  eyebrow,
  title,
  lead,
  children,
  tone = "plain",
}: {
  id?: string;
  eyebrow: string;
  title: ReactNode;
  lead?: ReactNode;
  children: ReactNode;
  tone?: "plain" | "tint";
}) {
  return (
    <section
      id={id}
      className={`scroll-mt-4 py-16 sm:py-24 ${tone === "tint" ? "bg-neutral-100/70 dark:bg-neutral-900/40" : ""}`}
    >
      <div className={wrap}>
        <div className="max-w-2xl">
          <div className="text-xs font-semibold uppercase tracking-[0.16em] text-amber-600 dark:text-amber-400">{eyebrow}</div>
          <h2 className="mt-3 text-3xl font-bold tracking-tight sm:text-4xl">{title}</h2>
          {lead && <p className="mt-4 text-lg leading-relaxed text-neutral-600 dark:text-neutral-400">{lead}</p>}
        </div>
        <div className="mt-12">{children}</div>
      </div>
    </section>
  );
}

const card =
  "rounded-2xl border border-neutral-200 bg-white p-6 shadow-sm dark:border-neutral-800 dark:bg-neutral-900/60";

function IconBadge({ icon: I }: { icon: IconType }) {
  return (
    <span className="grid size-11 place-items-center rounded-xl bg-amber-500/10 text-amber-600 ring-1 ring-amber-500/20 dark:text-amber-400">
      <I className="size-5" />
    </span>
  );
}

// Which side are you on

function Sides() {
  const sides = [
    {
      icon: LockIcon,
      who: "I'm paying",
      title: "Pay without handing the money over.",
      points: [
        "Your payment is locked, not sent.",
        "You release it with a QR code when you have the goods.",
        "If nothing happens, you are refunded at the deadline.",
      ],
      cta: "Pay safely",
    },
    {
      icon: BoltIcon,
      who: "I'm getting paid",
      title: "Start work knowing the money is there.",
      points: [
        "You see the payment is committed before you begin.",
        "It lands straight on your own Lightning node when released.",
        "Add an arbiter if a customer might not release.",
      ],
      cta: "Get paid safely",
    },
  ];
  return (
    <section className="relative -mt-10 pb-4 sm:-mt-14">
      <div className={`${wrap} grid gap-5 md:grid-cols-2`}>
        {sides.map((s) => (
          <div key={s.who} className={`${card} landing-lift flex flex-col`}>
            <div className="flex items-center gap-3">
              <IconBadge icon={s.icon} />
              <span className="text-xs font-semibold uppercase tracking-[0.14em] text-neutral-500">{s.who}</span>
            </div>
            <h3 className="mt-4 text-xl font-semibold tracking-tight">{s.title}</h3>
            <ul className="mt-4 space-y-2.5 text-[15px] text-neutral-600 dark:text-neutral-400">
              {s.points.map((p) => (
                <li key={p} className="flex gap-2.5">
                  <CheckIcon className="mt-0.5 size-4 shrink-0 text-emerald-600 dark:text-emerald-400" />
                  {p}
                </li>
              ))}
            </ul>
            <a
              href="#start"
              className="mt-6 inline-flex items-center gap-2 self-start text-sm font-semibold text-amber-700 transition hover:gap-3 dark:text-amber-400"
            >
              {s.cta} <ArrowIcon className="size-4" />
            </a>
          </div>
        ))}
      </div>
    </section>
  );
}

// How it works

function HowItWorks() {
  const steps = [
    { n: "01", icon: EyeIcon, title: "Make an offer", body: "The seller sets an amount, a description and a deadline, and shares a link. An arbiter is optional." },
    { n: "02", icon: LockIcon, title: "Pay into the lock", body: "The buyer accepts and pays a hold invoice. The payment is locked on the network, not sent to anyone." },
    { n: "03", icon: QrIcon, title: "Hand over", body: "The seller does the job knowing the money is committed. At handover, the buyer shows a QR code." },
    { n: "04", icon: UndoIcon, title: "Release or refund", body: "Scanning it pays the seller. If nobody does by the deadline, the buyer is refunded automatically." },
  ];
  return (
    <Section
      id="how"
      eyebrow="How it works"
      title="Four steps, and nobody in the middle."
      lead="The lock itself is the escrow. A Lightning hold invoice lets the seller's node receive a payment it cannot yet claim."
    >
      <ol className="grid gap-5 sm:grid-cols-2 lg:grid-cols-4">
        {steps.map((s) => (
          <li key={s.n} className={`${card} landing-lift relative`}>
            <div className="flex items-center justify-between">
              <IconBadge icon={s.icon} />
              <span className="font-mono text-sm font-semibold text-neutral-400 dark:text-neutral-600">{s.n}</span>
            </div>
            <h3 className="mt-5 text-lg font-semibold tracking-tight">{s.title}</h3>
            <p className="mt-2 text-[15px] leading-relaxed text-neutral-600 dark:text-neutral-400">{s.body}</p>
          </li>
        ))}
      </ol>
    </Section>
  );
}

// Outcomes

function Outcomes() {
  const outcomes = [
    {
      tone: "emerald",
      tag: "Most deals",
      title: "Paid on handover",
      body: "The buyer has the goods and shows the QR. The seller scans it and is paid straight to their own node.",
      foot: "No one else touches the money.",
    },
    {
      tone: "sky",
      tag: "If nobody shows up",
      title: "Refunded automatically",
      body: "The deadline passes and the lock is cancelled. The buyer gets every sat back, including fees, with no one lifting a finger.",
      foot: "No refund request, no approval.",
    },
    {
      tone: "violet",
      tag: "If they disagree",
      title: "Decided by an arbiter",
      body: "A referee both sides chose reads what each reported and picks one of two outcomes: pay the seller, or refund the buyer.",
      foot: "They decide. They never hold the funds.",
    },
  ] as const;
  const dot = {
    emerald: "bg-emerald-500",
    sky: "bg-sky-500",
    violet: "bg-violet-500",
  } as const;
  const tagText = {
    emerald: "text-emerald-700 dark:text-emerald-400",
    sky: "text-sky-700 dark:text-sky-400",
    violet: "text-violet-700 dark:text-violet-400",
  } as const;
  return (
    <Section
      id="outcomes"
      tone="tint"
      eyebrow="Every deal ends one of three ways"
      title="Built around what happens next."
      lead="Escrow is only useful if the unhappy paths are as solid as the happy one, so all three are first-class."
    >
      <div className="grid gap-5 lg:grid-cols-3">
        {outcomes.map((o) => (
          <article key={o.title} className={`${card} landing-lift flex flex-col`}>
            <div className={`inline-flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.14em] ${tagText[o.tone]}`}>
              <span className={`size-2 rounded-full ${dot[o.tone]}`} />
              {o.tag}
            </div>
            <h3 className="mt-4 text-xl font-semibold tracking-tight">{o.title}</h3>
            <p className="mt-3 flex-1 text-[15px] leading-relaxed text-neutral-600 dark:text-neutral-400">{o.body}</p>
            <p className="mt-5 border-t border-neutral-200 pt-4 text-sm font-medium dark:border-neutral-800">{o.foot}</p>
          </article>
        ))}
      </div>
    </Section>
  );
}

// Trust

function Trust() {
  const reasons = [
    {
      icon: ShieldIcon,
      title: "Nobody holds the money",
      body: "The payment is locked on the Lightning network. With your own node, it lands in your balance directly.",
    },
    {
      icon: BoltIcon,
      title: "The network decides, not the website",
      body: "An escrow only shows as locked when the Lightning node says so. A click on a page never changes what is real.",
    },
    {
      icon: KeyIcon,
      title: "Your secret stays yours",
      body: "The release secret is made in your browser. Only its fingerprint reaches the server, and the secret travels in a part of the link that browsers never send.",
    },
    {
      icon: ScaleIcon,
      title: "Arbitration without custody",
      body: "The buyer's secret is sealed so only the arbiter can open it. They can choose pay or refund, and nothing else.",
    },
  ];
  const facts = [
    { big: "256-bit", small: "release secret, made in your browser" },
    { big: "Auto", small: "refund at the deadline, no one acts" },
    { big: "6 h", small: "longest hold, set by how Lightning works" },
    { big: "Your node", small: "where a released payment lands" },
  ];
  return (
    <Section
      id="trust"
      eyebrow="Why you can trust it"
      title="Built so you don't have to trust us."
      lead="Most escrow asks you to trust the middleman. Here the guarantees come from the network and from cryptography, and the product is designed to hold as little power as possible."
    >
      <div className="grid gap-5 sm:grid-cols-2">
        {reasons.map((r) => (
          <div key={r.title} className={`${card} landing-lift flex gap-4`}>
            <div className="shrink-0">
              <IconBadge icon={r.icon} />
            </div>
            <div>
              <h3 className="text-lg font-semibold tracking-tight">{r.title}</h3>
              <p className="mt-2 text-[15px] leading-relaxed text-neutral-600 dark:text-neutral-400">{r.body}</p>
            </div>
          </div>
        ))}
      </div>
      <dl className="mt-10 grid grid-cols-2 gap-px overflow-hidden rounded-2xl border border-neutral-200 bg-neutral-200 dark:border-neutral-800 dark:bg-neutral-800 lg:grid-cols-4">
        {facts.map((f) => (
          <div key={f.big} className="bg-white px-5 py-6 dark:bg-neutral-950">
            <dt className="text-2xl font-bold tracking-tight sm:text-3xl">{f.big}</dt>
            <dd className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">{f.small}</dd>
          </div>
        ))}
      </dl>
    </Section>
  );
}

// Honest limits

function Limits() {
  const limits = [
    { title: "It doesn't judge the work", body: "Releasing proves the buyer chose to release, not that the job was good. Quality disputes still need a person." },
    { title: "Holds are short", body: "A payment can only be held for up to six hours, which is as long as Lightning can safely hold it." },
    { title: "A test network, for now", body: "Today this runs on a test network with stand-in coins. It has not yet been tried with real phone wallets." },
  ];
  return (
    <section className="py-16 sm:py-20">
      <div className={wrap}>
        <div className="rounded-3xl border border-amber-500/30 bg-amber-500/5 p-6 sm:p-10">
          <div className="text-xs font-semibold uppercase tracking-[0.16em] text-amber-700 dark:text-amber-400">Honest limits</div>
          <h2 className="mt-3 max-w-xl text-2xl font-bold tracking-tight sm:text-3xl">What it doesn&apos;t do, said plainly.</h2>
          <div className="mt-8 grid gap-6 md:grid-cols-3">
            {limits.map((l) => (
              <div key={l.title}>
                <h3 className="font-semibold">{l.title}</h3>
                <p className="mt-2 text-[15px] leading-relaxed text-neutral-600 dark:text-neutral-400">{l.body}</p>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

// Closing call to action

function GetStarted({ onCreated }: { onCreated: (user: NewUser) => void }) {
  return (
    <section id="start" className="landing-hero relative isolate scroll-mt-0 overflow-hidden bg-neutral-950 py-20 text-neutral-50 sm:py-28">
      <div aria-hidden className="landing-grid absolute inset-0 -z-10 opacity-60" />
      <div aria-hidden className="absolute -bottom-48 left-1/2 -z-10 h-[30rem] w-[30rem] -translate-x-1/2 rounded-full bg-amber-500/20 blur-3xl" />
      <div className={wrap}>
        <div className="mx-auto max-w-2xl text-center">
          <h2 className="text-balance text-3xl font-bold tracking-tight sm:text-5xl">Start your first protected deal.</h2>
          <p className="mt-5 text-lg leading-relaxed text-neutral-300">
            Make an account in a second, or sign back in with the token you saved.
          </p>
        </div>
        {/* The forms are light or dark cards of their own, so they must not inherit this section's white text. */}
        <div className="mx-auto mt-12 grid max-w-4xl gap-6 text-neutral-900 md:grid-cols-2 dark:text-neutral-50">
          <CreateAccount onCreated={onCreated} />
          <SignIn />
        </div>
      </div>
    </section>
  );
}

// Footer

function SiteFooter() {
  const cols: { title: string; links: { label: string; href: string; external?: boolean }[] }[] = [
    {
      title: "Product",
      links: [
        { label: "How it works", href: "#how" },
        { label: "Outcomes", href: "#outcomes" },
        { label: "Why trust it", href: "#trust" },
        { label: "Get started", href: "#start" },
      ],
    },
    {
      title: "Docs",
      links: [
        { label: "Demo script", href: `${REPO}/blob/main/docs/demo.md`, external: true },
        { label: "Product spec", href: `${REPO}/blob/main/docs/project.md`, external: true },
        { label: "Work log", href: `${REPO}/blob/main/docs/flow.md`, external: true },
      ],
    },
    {
      title: "Project",
      links: [
        { label: "Source code", href: REPO, external: true },
        { label: "Honest limits", href: "#trust" },
      ],
    },
  ];
  return (
    <footer className="border-t border-white/10 bg-neutral-950 text-neutral-300">
      <div className={`${wrap} grid gap-10 py-14 md:grid-cols-[1.4fr_2fr]`}>
        <div>
          <div className="flex items-center gap-2 font-bold tracking-tight text-white">
            <span className="grid size-8 place-items-center rounded-lg bg-amber-500 text-black">
              <BoltIcon className="size-4" />
            </span>
            toshi-way
          </div>
          <p className="mt-4 max-w-sm text-sm leading-relaxed text-neutral-400">
            Escrow on Bitcoin Lightning, without a middleman. Payments are locked by the network and released by the
            people involved.
          </p>
        </div>
        <div className="grid grid-cols-2 gap-8 sm:grid-cols-3">
          {cols.map((c) => (
            <nav key={c.title} aria-label={c.title}>
              <div className="text-xs font-semibold uppercase tracking-[0.14em] text-neutral-500">{c.title}</div>
              <ul className="mt-4 space-y-2.5 text-sm">
                {c.links.map((l) => (
                  <li key={l.label}>
                    <a
                      href={l.href}
                      {...(l.external ? { target: "_blank", rel: "noopener noreferrer" } : {})}
                      className="text-neutral-300 transition-colors hover:text-white"
                    >
                      {l.label}
                    </a>
                  </li>
                ))}
              </ul>
            </nav>
          ))}
        </div>
      </div>
      <div className="border-t border-white/10">
        <div className={`${wrap} flex flex-col gap-2 py-6 text-xs text-neutral-500 sm:flex-row sm:items-center sm:justify-between`}>
          <span>Runs on a test network. No real money.</span>
          <span>The page only shows what the escrow server reports. Funds are held by the Lightning network, not by this site.</span>
        </div>
      </div>
    </footer>
  );
}
