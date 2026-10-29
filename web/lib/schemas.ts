import { z } from "zod";

// API responses. Unknown extra fields are allowed so a newer server doesn't break the app.

export const ESCROW_STATES = [
  "CREATED",
  "INVOICE_ISSUED",
  "LOCKED",
  "SETTLED",
  "REFUNDED",
  "EXPIRED_UNPAID",
  "DISPUTED",
  "FAILED",
] as const;
export const EscrowState = z.enum(ESCROW_STATES);
export type EscrowState = z.infer<typeof EscrowState>;

export const FINAL_STATES: ReadonlySet<EscrowState> = new Set([
  "SETTLED",
  "REFUNDED",
  "EXPIRED_UNPAID",
  "FAILED",
]);

const hex64 = z.string().regex(/^[0-9a-f]{64}$/i, "must be 64 hex characters");
const unixSecs = z.number().int();

export const Escrow = z.object({
  id: z.string().min(1),
  /** `null` for an open offer that nobody has accepted yet. */
  buyer_id: z.string().min(1).nullable(),
  seller_id: z.string().min(1),
  amount_sats: z.number().int().positive(),
  description: z.string(),
  payment_hash: hex64.nullable(),
  state: EscrowState,
  invoice_expires_at: unixSecs.nullable(),
  deadline_at: unixSecs,
  created_at: unixSecs,
  updated_at: unixSecs,
  your_role: z.enum(["buyer", "seller", "arbiter", "operator"]),
  /**
   * Whose Lightning node holds the invoice. "own": the seller's, so a release pays the seller
   * directly. "shared": the service's, so the service receives the sats and owes them to the seller.
   */
  seller_node: z.enum(["own", "shared"]).default("shared"),
  /** A trusted third party who decides disputes, when the seller named one. */
  arbiter_id: z.string().nullable().default(null),
  /** Their public key as recorded when the escrow was created: what the buyer encrypts to. */
  arbiter_key: z.string().nullable().default(null),
  arbiter_fingerprint: z.string().nullable().default(null),
  /** Whether the buyer has sealed the release secret to the arbiter (the blob itself is never shown). */
  arbiter_sealed: z.boolean().default(false),
  /** When the arbiter confirmed they can open it; until then the seller should not rely on arbitration. */
  arbiter_confirmed_at: unixSecs.nullable().default(null),
  /**
   * What the sum was worth in a currency when the offer was made: display only, locked at creation.
   * Amounts are in the currency's minor unit (cents, kobo, ...).
   */
  fiat: z
    .object({
      currency: z.string().min(3).max(3),
      exponent: z.number().int().min(0).max(2),
      amount_minor: z.number().int().nonnegative(),
      rate_minor_per_btc: z.number().int().positive(),
      quoted_at: unixSecs,
      source: z.string(),
    })
    .nullable()
    .default(null),
});
export type Escrow = z.infer<typeof Escrow>;

/** Counts and sums over a person's whole history, which can be longer than the list the server sends. */
export const Totals = z.object({
  escrows: z.number(),
  settled: z.object({ count: z.number(), received: z.number(), paid: z.number() }),
  refunded: z.object({ count: z.number(), sats: z.number() }),
});
export type Totals = z.infer<typeof Totals>;

export const EscrowList = z.object({ escrows: z.array(Escrow), totals: Totals.optional() });

export const AuditEntry = z.object({
  actor: z.string(),
  action: z.string(),
  from_state: z.string().nullable(),
  to_state: z.string().nullable(),
  reason: z.string().nullable(),
  at: unixSecs,
});
export type AuditEntry = z.infer<typeof AuditEntry>;

export const NodeEvent = z.object({ node_invoice_state: z.string(), at: unixSecs });
export type NodeEvent = z.infer<typeof NodeEvent>;

export const EscrowEvents = z.object({ audit: z.array(AuditEntry), node_events: z.array(NodeEvent) });
export type EscrowEvents = z.infer<typeof EscrowEvents>;

export const Invoice = z.object({ payment_request: z.string().min(1), expires_at: unixSecs.nullable() });
export type Invoice = z.infer<typeof Invoice>;

export const Me = z.object({ id: z.string().min(1), role: z.string() });
export type Me = z.infer<typeof Me>;

export const API_KEY_SCOPES = ["escrows:read", "escrows:write"] as const;
export type ApiKeyScope = (typeof API_KEY_SCOPES)[number];

/** A key as listed: never carries the key itself. */
export const ApiKeyInfo = z.object({
  id: z.string(),
  name: z.string(),
  last_chars: z.string(),
  scopes: z.array(z.string()),
  created_at: unixSecs,
  last_used_at: unixSecs.nullable(),
  revoked_at: unixSecs.nullable(),
});
export type ApiKeyInfo = z.infer<typeof ApiKeyInfo>;

export const ApiKeyList = z.object({ api_keys: z.array(ApiKeyInfo) });
/** A new key: the only time the secret is seen. */
export const NewApiKey = ApiKeyInfo.extend({ key: z.string() });
export type NewApiKey = z.infer<typeof NewApiKey>;

export const NodeInfo = z.object({
  mode: z.enum(["own", "shared", "none"]),
  address: z.string().nullable(),
  connected_at: unixSecs.nullable(),
  shared_available: z.boolean(),
  own_nodes_enabled: z.boolean(),
  can_change: z.boolean(),
});
export type NodeInfo = z.infer<typeof NodeInfo>;

/** What a seller pastes to connect their own node. */
export const NodeForm = z.object({
  address: z
    .string()
    .trim()
    .regex(/^https:\/\/[^\s/?#@]+:\d{1,5}$/, "Use https://host:port, for example https://mynode.example.com:10009"),
  tls_cert: z
    .string()
    .trim()
    .refine((v) => v.startsWith("-----BEGIN CERTIFICATE-----"), "Paste the certificate text, starting with -----BEGIN CERTIFICATE-----"),
  macaroon: z
    .string()
    .trim()
    .toLowerCase()
    .regex(/^(?:[0-9a-f]{2})+$/, "The macaroon is hex text (see the command above)")
    .max(4096, "That is too long to be a macaroon"),
});
export type NodeForm = z.infer<typeof NodeForm>;

export const ArbiterKey = z.object({ public_key: z.string().nullable(), fingerprint: z.string().nullable() });
export type ArbiterKey = z.infer<typeof ArbiterKey>;

/** What an arbiter fetches to judge: the sealed secret and the hash it must match. */
export const Arbitration = z.object({
  sealed_secret: z.string().min(1),
  payment_hash: hex64.nullable(),
  arbiter_key: z.string().nullable(),
  state: EscrowState,
});
export type Arbitration = z.infer<typeof Arbitration>;

export const RatesInfo = z.object({
  enabled: z.boolean(),
  source: z.string(),
  currencies: z.array(z.object({ code: z.string(), exponent: z.number().int().min(0).max(2) })),
});
export type RatesInfo = z.infer<typeof RatesInfo>;

export const RateQuote = z.object({
  currency: z.string(),
  exponent: z.number().int().min(0).max(2),
  rate_minor_per_btc: z.number().int().positive(),
  quoted_at: unixSecs,
  source: z.string(),
});
export type RateQuote = z.infer<typeof RateQuote>;

export const NewUser = z.object({ id: z.string().min(1), role: z.string(), token: z.string().min(1) });
export type NewUser = z.infer<typeof NewUser>;

export const Health = z.object({
  status: z.string(),
  node: z.string(),
  server_time: unixSecs.optional(),
});
export type Health = z.infer<typeof Health>;

export const ApiErrorBody = z.object({ error: z.object({ code: z.string(), message: z.string() }) });

// Forms. Limits mirror the server's; the server checks them again regardless.

export const MAX_AMOUNT_SATS = 1_000_000;
export const MIN_HOLD_SECS = 60;
export const MAX_HOLD_SECS = 6 * 3600;
export const MAX_DESCRIPTION = 500;

export const HOLD_UNITS = { minutes: 60, hours: 3600 } as const;

export const NewEscrowForm = z
  .object({
    /** Optional: someone both sides trust to decide a dispute. */
    arbiter_id: z.string().trim().default(""),
    /** Optional: also show the amount in this currency (display only). Empty means none. */
    fiat_currency: z.string().trim().default(""),
    /** Optional: leave empty for an open offer that the first user to accept becomes the buyer of. */
    buyer_id: z.string().trim(),
    amount_sats: z.coerce
      .number({ error: "Enter an amount in sats" })
      .int("Whole sats only")
      .min(1, "At least 1 sat")
      .max(MAX_AMOUNT_SATS, `At most ${MAX_AMOUNT_SATS.toLocaleString("en-US")} sats`),
    description: z
      .string()
      .trim()
      .min(1, "Describe what is being sold")
      .max(MAX_DESCRIPTION, `At most ${MAX_DESCRIPTION} characters`),
    hold_amount: z.coerce.number({ error: "Enter a hold time" }).positive("Enter a hold time"),
    hold_unit: z.enum(["minutes", "hours"]),
  })
  .transform((f, ctx) => {
    const hold_secs = Math.round(f.hold_amount * HOLD_UNITS[f.hold_unit]);
    if (hold_secs < MIN_HOLD_SECS || hold_secs > MAX_HOLD_SECS) {
      ctx.addIssue({
        code: "custom",
        path: ["hold_amount"],
        message: "The hold must be between 1 minute and 6 hours",
      });
      return z.NEVER;
    }
    return {
      ...(f.buyer_id ? { buyer_id: f.buyer_id } : {}),
      ...(f.arbiter_id ? { arbiter_id: f.arbiter_id } : {}),
      ...(f.fiat_currency ? { fiat_currency: f.fiat_currency } : {}),
      amount_sats: f.amount_sats,
      description: f.description,
      hold_secs,
    };
  });
export type NewEscrowRequest = z.output<typeof NewEscrowForm>;

export const SecretInput = z
  .string()
  .trim()
  .toLowerCase()
  .regex(/^[0-9a-f]{64}$/, "The secret is exactly 64 hex characters (0-9, a-f)");

export const DisputeForm = z
  .string()
  .trim()
  .min(1, "Say what went wrong")
  .max(500, "At most 500 characters");
