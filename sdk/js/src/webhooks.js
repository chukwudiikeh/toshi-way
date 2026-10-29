import { createHmac, timingSafeEqual } from "node:crypto";

/** Thrown when a webhook cannot be trusted. `reason` says why, for your logs (not for the caller). */
export class WebhookSignatureError extends Error {
  /** @param {string} reason */
  constructor(reason) {
    super(`Invalid webhook: ${reason}`);
    this.name = "WebhookSignatureError";
    this.reason = reason;
  }
}

/** `t=<unix seconds>,v1=<hex>[,v1=<hex>...]` into its parts, or null if it is not that shape. */
function parseHeader(header) {
  if (typeof header !== "string") return null;
  let t = null;
  const v1 = [];
  for (const part of header.split(",")) {
    const eq = part.indexOf("=");
    if (eq < 1) return null;
    const key = part.slice(0, eq).trim();
    const value = part.slice(eq + 1).trim();
    if (key === "t") t = value;
    else if (key === "v1") v1.push(value);
  }
  if (t === null || !/^\d{1,12}$/.test(t) || v1.length === 0) return null;
  return { t: Number(t), v1 };
}

/**
 * Check that a webhook really came from toshi-way, and return the event.
 *
 * Pass the body EXACTLY as it arrived (the raw bytes or string). Do not parse it and serialise it
 * again: the signature covers the original text, and any change to spacing or key order breaks it.
 *
 * @param {object} args
 * @param {string | Buffer | Uint8Array} args.payload the raw request body
 * @param {string | null | undefined} args.signature the `Toshi-Signature` header
 * @param {string} args.secret your endpoint's signing secret (`whsec_...`)
 * @param {number} [args.toleranceSecs] how old a message may be, in seconds (default 300)
 * @param {number} [args.now] the current unix time in seconds (for tests)
 * @returns {any} the parsed event
 */
export function verifyWebhook({ payload, signature, secret, toleranceSecs = 300, now = Date.now() / 1000 }) {
  if (typeof secret !== "string" || secret === "") throw new WebhookSignatureError("no signing secret given");
  const body = typeof payload === "string" ? payload : Buffer.from(payload ?? []).toString("utf8");
  const parsed = parseHeader(signature);
  if (!parsed) throw new WebhookSignatureError("the signature header is missing or malformed");

  const expected = createHmac("sha256", secret).update(`${parsed.t}.${body}`).digest();
  const ok = parsed.v1.some((candidate) => {
    if (!/^[0-9a-f]{64}$/i.test(candidate)) return false;
    return timingSafeEqual(expected, Buffer.from(candidate, "hex"));
  });
  if (!ok) throw new WebhookSignatureError("the signature does not match");

  // Only after the signature is known good: a replayed old message carries a valid signature.
  if (Math.abs(now - parsed.t) > toleranceSecs) throw new WebhookSignatureError("the message is too old (or the clock is wrong)");

  try {
    return JSON.parse(body);
  } catch {
    throw new WebhookSignatureError("the body is not JSON");
  }
}
