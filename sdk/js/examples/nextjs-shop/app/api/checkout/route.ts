import { randomUUID } from "node:crypto";
import { ToshiError } from "@toshi-way/client";
import { update, type Order } from "@/lib/orders";
import { appUrl, checkoutLink, toshi } from "@/lib/toshi";

const ITEM = { name: "Parcel to Ikeja", amount_sats: 25_000 };

/** The customer pressed "Pay": make the order, make its escrow, and send them to toshi-way to pay. */
export async function POST() {
  const id = randomUUID().slice(0, 8);
  const order: Order = {
    id,
    item: ITEM.name,
    amount_sats: ITEM.amount_sats,
    status: "awaiting_payment",
    escrow_id: null,
    checkout_url: null,
    handled: [],
    created_at: Math.floor(Date.now() / 1000),
  };
  await update((all) => void (all[id] = order));

  try {
    const escrow = await toshi.escrows.create({
      amount_sats: order.amount_sats,
      description: `${order.item}\nOrder ${id}`,
      hold_secs: 3600,
      reference: id, // comes back on every escrow and every webhook
      return_url: `${appUrl()}/orders/${id}`,
      idempotencyKey: `order-${id}`, // a retry of this call can never make a second escrow
    });
    const link = checkoutLink(escrow);
    await update((all) => {
      all[id].escrow_id = escrow.id;
      all[id].checkout_url = link;
    });
    if (!link) {
      return new Response("No checkout link: the toshi-way server has no public web address, and TOSHI_WEB_URL is not set.", { status: 502 });
    }
    // 303 turns the POST into a GET on the other side.
    return Response.redirect(link, 303);
  } catch (e) {
    await update((all) => void (all[id].status = "cancelled"));
    if (e instanceof ToshiError) {
      console.error(`toshi-way refused: ${e.code} (${e.status}) request ${e.requestId}`);
      return new Response(`Could not start the payment: ${e.message}`, { status: 502 });
    }
    throw e;
  }
}

