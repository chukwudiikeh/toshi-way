import { describe, expect, it } from "vitest";
import { lifecycle, splitDescription } from "@/lib/lifecycle";

const statuses = (state: Parameters<typeof lifecycle>[0], hadInvoice = true) =>
  lifecycle(state, hadInvoice).steps.map((s) => s.status);

describe("the four steps", () => {
  it("moves along as the escrow does", () => {
    expect(statuses("CREATED", false)).toEqual(["current", "upcoming", "upcoming", "upcoming"]);
    expect(statuses("INVOICE_ISSUED")).toEqual(["done", "current", "upcoming", "upcoming"]);
    expect(statuses("LOCKED")).toEqual(["done", "done", "current", "upcoming"]);
  });

  it("marks a dispute on the locked step and calls for attention", () => {
    const l = lifecycle("DISPUTED", true);
    expect(l.steps.map((s) => s.status)).toEqual(["done", "done", "current", "upcoming"]);
    expect(l.steps[2].note).toBe("In dispute");
    expect(l.tone).toBe("attention");
  });

  it("completes every step when the seller is paid", () => {
    const l = lifecycle("SETTLED", true);
    expect(l.steps.map((s) => s.status)).toEqual(["done", "done", "done", "done"]);
    expect(l.steps[3].label).toBe("Seller paid");
    expect(l.tone).toBe("success");
  });

  it("ends a refund after the lock, not as a success", () => {
    const l = lifecycle("REFUNDED", true);
    expect(l.steps.map((s) => s.status)).toEqual(["done", "done", "done", "ended"]);
    expect(l.steps[3].label).toBe("Buyer refunded");
    expect(l.tone).toBe("refund");
  });

  it("says where an unpaid escrow stopped", () => {
    // Withdrawn or lapsed before the buyer accepted: it ended at the offer.
    expect(statuses("EXPIRED_UNPAID", false)).toEqual(["ended", "upcoming", "upcoming", "upcoming"]);
    // The buyer accepted but never paid: it ended at the payment.
    expect(statuses("EXPIRED_UNPAID", true)).toEqual(["done", "ended", "upcoming", "upcoming"]);
    expect(lifecycle("EXPIRED_UNPAID", true).tone).toBe("stopped");
  });

  it("flags a failed escrow without pretending to know the step", () => {
    const l = lifecycle("FAILED", true);
    expect(l.steps[3]).toMatchObject({ status: "ended", label: "Needs attention" });
    expect(l.tone).toBe("attention");
  });

  it("always has the same four steps in the same order, with at most one current", () => {
    for (const state of ["CREATED", "INVOICE_ISSUED", "LOCKED", "DISPUTED", "SETTLED", "REFUNDED", "EXPIRED_UNPAID", "FAILED"] as const) {
      const steps = lifecycle(state, true).steps;
      expect(steps.map((s) => s.key)).toEqual(["offer", "payment", "locked", "outcome"]);
      expect(steps.filter((s) => s.status === "current").length).toBeLessThanOrEqual(1);
    }
  });
});

describe("splitting a description", () => {
  it("keeps a short one as the title", () => {
    expect(splitDescription("Parcel #4821 to Lekki, Lagos")).toEqual({ title: "Parcel #4821 to Lekki, Lagos", body: null });
  });

  it("takes the first line as the title and keeps the rest as the body", () => {
    const d = splitDescription("Cargo / goods shipment\nSeller ships:\n• goods and quantity\n• tracking");
    expect(d.title).toBe("Cargo / goods shipment");
    expect(d.body).toBe("Seller ships:\n• goods and quantity\n• tracking");
  });

  it("skips leading blank lines and trims", () => {
    expect(splitDescription("\n\n  Fabric order  \n\nDeliver to Abuja\n")).toEqual({ title: "Fabric order", body: "Deliver to Abuja" });
  });

  it("shortens a very long first line and keeps the whole text as the body", () => {
    const long = "x".repeat(200);
    const d = splitDescription(long);
    expect(d.title.length).toBe(90);
    expect(d.title.endsWith("…")).toBe(true);
    expect(d.body).toBe(long);
  });

  it("copes with nothing", () => {
    expect(splitDescription("")).toEqual({ title: "(no description)", body: null });
    expect(splitDescription("   \n  ")).toEqual({ title: "(no description)", body: null });
  });

  it("handles Windows line endings", () => {
    expect(splitDescription("Title\r\nBody line")).toEqual({ title: "Title", body: "Body line" });
  });
});
