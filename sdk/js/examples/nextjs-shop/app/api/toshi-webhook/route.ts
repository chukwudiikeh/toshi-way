import { verifyWebhook, type WebhookEvent } from "@toshi-way/client";
import { findByEscrow, update } from "@/lib/orders";
import { toshi, webhookSecret } from "@/lib/toshi";

/**
 * toshi-way tells us when an escrow changes. Every message is checked, handled once, and answered
 * quickly. The message is a signed nudge: before marking an order paid we read the escrow from the API.
 */
export async function POST(request: Request) {
  // The signature covers the exact bytes sent, so read the raw text. Never request.json() here.
  const raw = await request.text();

  let event: WebhookEvent;
  try {
    event = verifyWebhook({ payload: raw, signature: request.headers.get("toshi-signature"), secret: webhookSecret() });
  } catch (e) {
    console.warn("Rejected a webhook:", e instanceof Error ? e.message : e);
    return new Response("invalid signature", { status: 400 });
  }

  if (event.type === "webhook.test") return new Response("ok");

  const escrowId = event.data.escrow?.id;
  const order = escrowId ? await findByEscrow(escrowId) : null;
  if (!order) return new Response("ok"); // not one of ours: nothing to do, and no reason to retry

  try {
    switch (event.type) {
      case "escrow.locked":
        // The customer paid and the money is locked. Deliver now. It is NOT yours yet.
        await mark(order.id, event.id, "paid_into_escrow");
        break;
      case "escrow.settled": {
        const escrow = await toshi.escrows.get(escrowId!);
        if (escrow.state === "SETTLED") await mark(order.id, event.id, "paid");
        break;
      }
      case "escrow.refunded":
      case "escrow.expired":
        await mark(order.id, event.id, "cancelled");
        break;
    }
  } catch (e) {
    console.error("Could not handle", event.id, e);
    return new Response("try again", { status: 500 }); // toshi-way will retry
  }
  return new Response("ok");
}

/** Change the order's status, once per event, and never move a finished order backwards. */
function mark(orderId: string, eventId: string, status: "paid_into_escrow" | "paid" | "cancelled") {
  const rank = { awaiting_payment: 0, paid_into_escrow: 1, paid: 2, cancelled: 2 } as const;
  return update((all) => {
    const o = all[orderId];
    if (o.handled.includes(eventId)) return;
    o.handled.push(eventId);
    if (rank[status] > rank[o.status]) o.status = status;
  });
}
