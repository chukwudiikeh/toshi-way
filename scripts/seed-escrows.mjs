#!/usr/bin/env node
// Put a realistic mix of escrows on the demo cast, so a dashboard has something to show:
// two locked (the courier must deliver, the customer must hand over), one invoice waiting to be
// paid, two offers waiting on a buyer, and a short-fused locked one. Needs `demo-env.sh up` first.
//
//   node scripts/seed-escrows.mjs

import { createHash, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const API = process.env.API ?? "http://127.0.0.1:3000";
const WALLET = process.env.WALLET ?? "http://127.0.0.1:3100";
const cast = JSON.parse(readFileSync(resolve(repo, ".demo/accounts.json"), "utf8"));
const { courier, customer } = cast;

async function call(method, path, token, body) {
  const res = await fetch(API + path, {
    method,
    headers: { authorization: `Bearer ${token}`, ...(body ? { "content-type": "application/json" } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  const json = await res.json().catch(() => null);
  if (!res.ok) throw new Error(`${method} ${path}: ${JSON.stringify(json)}`);
  return json;
}

const offer = (description, sats, holdSecs, named = true) =>
  call("POST", "/api/v1/escrows", courier.token, {
    description,
    amount_sats: sats,
    hold_secs: holdSecs,
    ...(named ? { buyer_id: customer.id } : {}),
  });

// The customer accepts with a fresh secret's hash. The secret itself is thrown away: these sample
// escrows are for looking at, not for releasing.
const accept = (e) =>
  call("POST", `/api/v1/escrows/${e.id}/accept`, customer.token, {
    payment_hash: createHash("sha256").update(randomBytes(32)).digest("hex"),
  });

async function lock(e) {
  await accept(e);
  const { payment_request } = await call("GET", `/api/v1/escrows/${e.id}/invoice`, customer.token);
  const res = await fetch(`${WALLET}/pay`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ invoice: payment_request }),
  });
  if (!res.ok) throw new Error("the demo wallet refused the payment (is it running?)");
}

await lock(await offer("Parcel #5001 to Ikeja, Lagos", 12_000, 5_400));
await lock(await offer("Documents to Victoria Island", 3_500, 1_500));
await accept(await offer("Laptop pickup, Yaba", 45_000, 7_200)); // invoice issued, never paid
await offer("Groceries delivery, Surulere", 2_500, 3_600, false); // an open offer: a link, no buyer yet
await offer("Fabric order to Abuja", 8_000, 10_800);

await new Promise((r) => setTimeout(r, 3000));
const mine = await call("GET", "/api/v1/escrows", courier.token);
console.log(`Sample escrows are in place (${mine.escrows.length} for the courier):`);
for (const e of mine.escrows) console.log(`  ${e.state.padEnd(15)} ${String(e.amount_sats).padStart(6)} sats  ${e.description}`);
