// Make an offer for an order and print where to send the customer.
//
//   TOSHI_URL=https://pay.example.com TOSHI_KEY=tk_... node examples/checkout.mjs
import { Toshi, ToshiError } from "../src/index.js";

const toshi = new Toshi({ baseUrl: process.env.TOSHI_URL, apiKey: process.env.TOSHI_KEY });

const orderId = "ORDER-1001";

try {
  const escrow = await toshi.escrows.create({
    amount_sats: 25_000,
    description: "Parcel to Ikeja\nDeliver by Friday noon",
    hold_secs: 4 * 3600,
    reference: orderId,
    return_url: "https://shop.example.com/orders/1001/thanks",
    // Using the order id makes a repeat of this whole call safe.
    idempotencyKey: orderId,
  });
  console.log("Send the customer to:", escrow.checkout_url ?? `<web address>/escrows/${escrow.id}`);
  console.log("State:", escrow.state);
} catch (e) {
  if (e instanceof ToshiError) {
    console.error(`The server refused: ${e.code} (${e.status}): ${e.message}`);
    if (e.requestId) console.error(`Request id: ${e.requestId}`);
    process.exit(1);
  }
  throw e;
}
