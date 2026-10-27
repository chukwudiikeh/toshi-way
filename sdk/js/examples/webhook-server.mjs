// A small server that receives toshi-way webhooks, checks them, and handles each event once.
//
//   TOSHI_URL=https://pay.example.com TOSHI_KEY=tk_... TOSHI_WHSEC=whsec_... node examples/webhook-server.mjs
import { createServer } from "node:http";
import { Toshi, verifyWebhook } from "../src/index.js";

const toshi = new Toshi({ baseUrl: process.env.TOSHI_URL, apiKey: process.env.TOSHI_KEY });

// In production keep these in a database. Here, in memory.
const handled = new Set();
const paidOrders = new Set();

/** Read the body as raw bytes: the signature covers the exact text that was sent. */
const readRaw = (req) =>
  new Promise((resolve, reject) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => resolve(Buffer.concat(chunks)));
    req.on("error", reject);
  });

createServer(async (req, res) => {
  if (req.method !== "POST" || req.url !== "/toshi-webhook") {
    res.statusCode = 404;
    return res.end();
  }

  let event;
  try {
    event = verifyWebhook({
      payload: await readRaw(req),
      signature: req.headers["toshi-signature"],
      secret: process.env.TOSHI_WHSEC,
    });
  } catch (e) {
    console.warn("Rejected a message:", e.message);
    res.statusCode = 400;
    return res.end();
  }

  // The same event can arrive twice. Answer 200 for a repeat without doing the work again.
  if (!handled.has(event.id)) {
    handled.add(event.id);
    try {
      await handle(event);
    } catch (e) {
      // Let it be retried: forget it, and answer with an error.
      handled.delete(event.id);
      console.error("Could not handle", event.id, e);
      res.statusCode = 500;
      return res.end();
    }
  }
  res.statusCode = 200;
  res.end("ok");
}).listen(Number(process.env.PORT ?? 8080), () => console.log("Listening on /toshi-webhook"));

async function handle(event) {
  switch (event.type) {
    case "webhook.test":
      console.log("Test message received. The signature check works.");
      break;
    case "escrow.locked":
      // The customer has paid and the money is locked. Time to deliver, but you are not paid yet.
      console.log(`Order ${event.data.escrow.reference}: paid and locked. Deliver it.`);
      break;
    case "escrow.settled": {
      // Read the escrow from the API before acting: the message is a signed nudge, the API is the truth.
      const escrow = await toshi.escrows.get(event.data.escrow.id);
      if (escrow.state === "SETTLED") {
        paidOrders.add(escrow.reference);
        console.log(`Order ${escrow.reference}: PAID. Mark it as paid.`);
      }
      break;
    }
    case "escrow.refunded":
    case "escrow.expired":
      console.log(`Order ${event.data.escrow.reference}: ${event.type}. Cancel it.`);
      break;
    default:
      console.log("Event", event.type, event.id);
  }
}
