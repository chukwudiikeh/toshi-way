import { Toshi } from "@toshi-way/client";

function need(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`Set ${name}. See .env.example.`);
  return value;
}

/** One client for the whole server. It holds the API key, so it must never be imported by a client component. */
export const toshi = new Toshi({ baseUrl: need("TOSHI_URL"), apiKey: need("TOSHI_KEY") });

export const webhookSecret = () => need("TOSHI_WHSEC");
export const appUrl = () => need("APP_URL").replace(/\/+$/, "");

/**
 * Where to send the customer to pay. The API gives `checkout_url` when the server knows its public web
 * address. If it does not (a local test setup), build the link from TOSHI_WEB_URL instead.
 */
export function checkoutLink(escrow: { id: string; checkout_url?: string }): string | null {
  if (escrow.checkout_url) return escrow.checkout_url;
  const web = process.env.TOSHI_WEB_URL?.replace(/\/+$/, "");
  return web ? `${web}/escrows/${escrow.id}` : null;
}
