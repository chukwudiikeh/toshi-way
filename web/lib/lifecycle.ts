// Where an escrow is in its life, as four steps, and how a long description splits into a title
// and the rest. Both are derived from what the server reports, for display only.

import type { EscrowState } from "./schemas";

export type StepStatus = "done" | "current" | "upcoming" | "ended";
export type StepKey = "offer" | "payment" | "locked" | "outcome";

export type Step = {
  key: StepKey;
  label: string;
  status: StepStatus;
  /** A short remark under the step, for example "In dispute". */
  note: string | null;
};

export type Tone = "progress" | "success" | "refund" | "stopped" | "attention";

export type Lifecycle = { steps: Step[]; tone: Tone };

const ORDER: StepKey[] = ["offer", "payment", "locked", "outcome"];

/**
 * The four steps for a state. `hadInvoice` says whether the buyer got as far as accepting (an
 * escrow that ends unpaid either lapsed as an offer or while waiting for payment, and the state
 * alone cannot say which).
 */
export function lifecycle(state: EscrowState, hadInvoice: boolean): Lifecycle {
  const labels: Record<StepKey, string> = { offer: "Offer", payment: "Payment", locked: "Funds locked", outcome: "Paid out" };
  // How many steps are complete, and which (if any) is the one in progress.
  let done = 0;
  let current: StepKey | null = null;
  let tone: Tone = "progress";
  let ended: StepKey | null = null;
  let note: Partial<Record<StepKey, string>> = {};

  switch (state) {
    case "CREATED":
      current = "offer";
      break;
    case "INVOICE_ISSUED":
      done = 1;
      current = "payment";
      break;
    case "LOCKED":
      done = 2;
      current = "locked";
      break;
    case "DISPUTED":
      done = 2;
      current = "locked";
      tone = "attention";
      note = { locked: "In dispute" };
      break;
    case "SETTLED":
      done = 4;
      tone = "success";
      labels.outcome = "Seller paid";
      break;
    case "REFUNDED":
      done = 3;
      ended = "outcome";
      tone = "refund";
      labels.outcome = "Buyer refunded";
      break;
    case "EXPIRED_UNPAID":
      tone = "stopped";
      labels.outcome = "Ended";
      if (hadInvoice) {
        done = 1;
        ended = "payment";
      } else {
        ended = "offer";
      }
      break;
    case "FAILED":
      tone = "attention";
      labels.outcome = "Needs attention";
      ended = "outcome";
      note = { outcome: "The operator will look at it" };
      break;
  }

  const steps = ORDER.map((key, i): Step => {
    let status: StepStatus;
    if (key === ended) status = "ended";
    else if (i < done) status = "done";
    else if (key === current) status = "current";
    else status = "upcoming";
    return { key, label: labels[key], status, note: note[key] ?? null };
  });
  return { steps, tone };
}

const TITLE_MAX = 90;

/**
 * A long, multi-line description (an agreed checklist, say) makes a poor page title. The title is
 * the first non-empty line, shortened if need be; the body is everything else, or the whole text
 * when the first line had to be cut. `body` is null when the title already says it all.
 */
export function splitDescription(description: string): { title: string; body: string | null } {
  const text = description.trim();
  if (!text) return { title: "(no description)", body: null };
  const lines = text.split(/\r?\n/);
  const first = lines.find((l) => l.trim() !== "")!.trim();
  const rest = lines.slice(lines.findIndex((l) => l.trim() !== "") + 1).join("\n").trim();
  if (first.length > TITLE_MAX) {
    return { title: first.slice(0, TITLE_MAX - 1).trimEnd() + "…", body: text };
  }
  return { title: first, body: rest || null };
}
