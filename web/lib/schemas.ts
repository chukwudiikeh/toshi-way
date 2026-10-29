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
  buyer_id: z.string().min(1),
  seller_id: z.string().min(1),
  amount_sats: z.number().int().positive(),
  description: z.string(),
  payment_hash: hex64.nullable(),
  state: EscrowState,
  invoice_expires_at: unixSecs.nullable(),
  deadline_at: unixSecs,
  created_at: unixSecs,
  updated_at: unixSecs,
  your_role: z.enum(["buyer", "seller", "operator"]),
});
export type Escrow = z.infer<typeof Escrow>;

export const EscrowList = z.object({ escrows: z.array(Escrow) });

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
    buyer_id: z.string().trim().min(1, "Enter the buyer's user id"),
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
    return { buyer_id: f.buyer_id, amount_sats: f.amount_sats, description: f.description, hold_secs };
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
