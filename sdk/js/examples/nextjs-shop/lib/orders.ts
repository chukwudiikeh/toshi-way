import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";

/**
 * Where an order stands in the shop's own words. Only `paid` may release goods or mark money received,
 * and only the escrow reaching SETTLED makes an order paid.
 */
export type OrderStatus = "awaiting_payment" | "paid_into_escrow" | "paid" | "cancelled";

export type Order = {
  id: string;
  item: string;
  amount_sats: number;
  status: OrderStatus;
  /** The escrow made for this order. */
  escrow_id: string | null;
  checkout_url: string | null;
  /** Webhook event ids already handled, so a repeat does nothing. */
  handled: string[];
  created_at: number;
};

// A JSON file keeps the example dependency-free. Use your database in production.
const FILE = resolve(process.cwd(), ".data/orders.json");

async function readAll(): Promise<Record<string, Order>> {
  try {
    return JSON.parse(await readFile(FILE, "utf8"));
  } catch {
    return {};
  }
}

// Requests can overlap, so writes go through one queue: read, change, write, then the next.
let queue: Promise<unknown> = Promise.resolve();

export function update<T>(change: (all: Record<string, Order>) => T): Promise<T> {
  const run = queue.then(async () => {
    const all = await readAll();
    const result = change(all);
    await mkdir(dirname(FILE), { recursive: true });
    await writeFile(`${FILE}.tmp`, JSON.stringify(all, null, 2));
    await rename(`${FILE}.tmp`, FILE);
    return result;
  });
  queue = run.catch(() => {});
  return run;
}

export async function getOrder(id: string): Promise<Order | null> {
  return (await readAll())[id] ?? null;
}

export async function findByEscrow(escrowId: string): Promise<Order | null> {
  return Object.values(await readAll()).find((o) => o.escrow_id === escrowId) ?? null;
}
