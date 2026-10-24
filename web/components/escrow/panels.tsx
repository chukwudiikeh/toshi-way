"use client";

import { QRCodeSVG } from "qrcode.react";
import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { api, ApiError, describeError } from "@/lib/api";
import { formatSats } from "@/lib/clock";
import { DisputeForm, SecretInput, type Escrow, type Invoice } from "@/lib/schemas";
import { buildReleaseLink, generateSecret, hashSecret, loadSecret, saveSecret } from "@/lib/secret";
import { Button, Card, ConfirmButton, CopyButton, CopyField, Countdown, Field, inputClass, Notice, Spinner } from "../ui";
import { useStoredSecret } from "./hooks";

type PanelProps = {
  escrow: Escrow;
  setEscrow: (e: Escrow) => void;
  refresh: () => Promise<void>;
  /** A release secret taken from a `#release=` link, if this page was opened with one. */
  linkSecret: string | null;
};

type Run = (name: string, fn: () => Promise<Escrow>) => Promise<boolean>;
type Actions = { run: Run; busy: string | null };

/** Base URL for links shown to the other party; set NEXT_PUBLIC_APP_URL when phones can't reach localhost. */
function appUrl(): string {
  return (process.env.NEXT_PUBLIC_APP_URL || window.location.origin).replace(/\/+$/, "");
}

export function EscrowPanel({ escrow, setEscrow, refresh, linkSecret }: PanelProps) {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // Every action returns the escrow as the server now reports it; that, not the click, drives the UI.
  const run: Run = async (name, fn) => {
    setBusy(name);
    setError(null);
    setNotice(null);
    try {
      setEscrow(await fn());
      return true;
    } catch (err) {
      if (err instanceof ApiError && (err.code === "invalid_state" || err.code === "too_late")) {
        await refresh();
        setNotice(
          err.code === "too_late"
            ? describeError(err)
            : "The escrow changed in the meantime. This is its latest state.",
        );
      } else {
        setError(describeError(err));
      }
      return false;
    } finally {
      setBusy(null);
    }
  };
  const actions: Actions = { run, busy };
  const role = escrow.your_role;

  let body: ReactNode;
  if (role === "operator") body = <OperatorPanel escrow={escrow} actions={actions} />;
  else if (role === "buyer") body = <BuyerPanel escrow={escrow} actions={actions} linkSecret={linkSecret} />;
  else body = <SellerPanel escrow={escrow} actions={actions} linkSecret={linkSecret} />;

  const releasable = escrow.state === "LOCKED" || escrow.state === "DISPUTED";

  return (
    <Card>
      <div className="space-y-5">
        <NextSteps escrow={escrow} />
        {linkSecret && !releasable && role !== "operator" && (
          <Notice tone="info">
            You opened a release link, but there is nothing to release while the escrow is in this state.
          </Notice>
        )}
        {body}
        {notice && <Notice tone="info">{notice}</Notice>}
        {error && <Notice tone="error">{error}</Notice>}
      </div>
    </Card>
  );
}

// What happens next, per state and role.

function NextSteps({ escrow }: { escrow: Escrow }) {
  const buyer = escrow.your_role === "buyer";
  const seller = escrow.your_role === "seller";
  const text: Record<Escrow["state"], string> = {
    CREATED: buyer
      ? "Accepting creates a secret on this device and a Lightning invoice for you to pay. Nothing is paid yet."
      : seller
        ? "The buyer has to accept the offer. Send them the link below."
        : "Waiting for the buyer to accept.",
    INVOICE_ISSUED: buyer
      ? "Pay the invoice before it expires. Your payment is locked, not sent: the seller only gets it when you hand over your secret. If you don't, you are refunded automatically at the deadline."
      : "Waiting for the buyer to pay. If the invoice expires unpaid, the escrow ends and nothing is owed.",
    LOCKED: buyer
      ? "Your payment is locked. When you have what you paid for, show the seller your release QR. If you never do, you are refunded automatically at the deadline."
      : seller
        ? "The buyer's payment is locked, so you can start the work. On delivery, get the buyer's release QR (or 64-character secret) and submit it before the deadline. After the deadline the buyer is refunded automatically and the payment can no longer be claimed."
        : "Funds are locked. They settle if the secret is submitted before the deadline, otherwise they are refunded.",
    DISPUTED:
      "A dispute is open and the operator will review it. The deadline still applies: if the secret isn't submitted before it, the buyer is refunded automatically. The parties can still settle it themselves (release with the secret) or the seller can give a refund.",
    SETTLED: buyer ? "Done. The seller has been paid." : seller ? "Done. You were paid." : "Settled: the seller was paid.",
    REFUNDED: buyer
      ? "Done. Your payment was cancelled and the funds are back in your wallet."
      : seller
        ? "Cancelled. The buyer was refunded."
        : "Refunded to the buyer.",
    EXPIRED_UNPAID: "This offer ended without a payment: it was withdrawn, declined or its time ran out. Nothing was paid and nothing is owed.",
    FAILED: "Something went wrong between the escrow and the Lightning node. An operator needs to look at it.",
  };
  return <p className="text-sm leading-relaxed text-neutral-700 dark:text-neutral-300">{text[escrow.state]}</p>;
}

function Deadline({ escrow }: { escrow: Escrow }) {
  return (
    <Countdown
      until={escrow.deadline_at}
      label="Release deadline"
      endedText="Passed: refunding the buyer"
    />
  );
}

function FinalSummary({ escrow }: { escrow: Escrow }) {
  switch (escrow.state) {
    case "SETTLED":
      return <Notice tone="success">{formatSats(escrow.amount_sats)} settled to the seller.</Notice>;
    case "REFUNDED":
      return <Notice tone="success">{formatSats(escrow.amount_sats)} returned to the buyer.</Notice>;
    case "EXPIRED_UNPAID":
      return <Notice tone="info">Ended without a payment.</Notice>;
    case "FAILED":
      return <Notice tone="error">Needs operator attention. Your funds are governed by the Lightning node.</Notice>;
    default:
      return null;
  }
}

// Buyer

function BuyerPanel({ escrow, actions, linkSecret }: { escrow: Escrow; actions: Actions; linkSecret: string | null }) {
  // Bumped after the secret is (re)generated so the stored-secret check runs again.
  const [secretVersion, setSecretVersion] = useState(0);
  const stored = useStoredSecret(escrow.id, escrow.payment_hash, secretVersion);

  async function accept() {
    const ok = await actions.run("accept", async () => {
      // Store P before sending H: if the response is lost, the secret is still here.
      let p = loadSecret(escrow.id);
      if (!p) {
        p = generateSecret();
        saveSecret(escrow.id, p);
      }
      try {
        return await api.accept(escrow.id, await hashSecret(p));
      } catch (err) {
        if (!(err instanceof ApiError) || err.code !== "hash_in_use") throw err;
        p = generateSecret();
        saveSecret(escrow.id, p);
        return await api.accept(escrow.id, await hashSecret(p));
      }
    });
    if (ok) setSecretVersion((v) => v + 1);
  }

  switch (escrow.state) {
    case "CREATED":
      return (
        <div className="space-y-3">
          <Notice tone="warn">
            Your release secret will be stored <strong>only in this browser</strong>. If this browser&apos;s data
            is cleared, the secret is lost and the escrow can only end in a refund. Use the browser you will have
            with you at delivery.
          </Notice>
          <Button onClick={accept} busy={actions.busy === "accept"}>
            Accept offer of {formatSats(escrow.amount_sats)}
          </Button>
          {escrow.buyer_id ? (
            <ConfirmButton
              confirmText="Decline this offer? Nothing has been paid."
              confirmLabel="Yes, decline"
              variant="danger"
              busy={actions.busy === "cancel"}
              onConfirm={() => actions.run("cancel", () => api.cancel(escrow.id))}
            >
              Decline
            </ConfirmButton>
          ) : (
            <p className="text-xs text-neutral-500">
              Accepting makes you the buyer of this offer. If you are not the person it was meant for, close this page.
            </p>
          )}
        </div>
      );
    case "INVOICE_ISSUED":
      return (
        <div className="space-y-4">
          <SecretHealth stored={stored} />
          <InvoiceBox escrow={escrow} />
          <p className="flex items-center gap-2 text-sm text-neutral-500">
            <span className="size-2 animate-pulse rounded-full bg-sky-500" />
            After you pay, this page updates by itself once the Lightning node confirms the funds are locked.
          </p>
          <ConfirmButton
            confirmText="Cancel this escrow? Nothing has been paid yet."
            confirmLabel="Yes, cancel"
            variant="danger"
            busy={actions.busy === "cancel"}
            onConfirm={() => actions.run("cancel", () => api.cancel(escrow.id))}
          >
            Cancel
          </ConfirmButton>
        </div>
      );
    case "LOCKED":
    case "DISPUTED":
      return (
        <div className="space-y-4">
          <Deadline escrow={escrow} />
          {linkSecret ? (
            <ReleaseForm escrow={escrow} actions={actions} initial={linkSecret} fromLink />
          ) : stored.status === "ok" ? (
            <ReleaseHandover escrow={escrow} secret={stored.secret} actions={actions} />
          ) : stored.status === "checking" ? (
            <Spinner label="Checking your secret…" />
          ) : (
            <>
              <SecretHealth stored={stored} />
              <ReleaseForm escrow={escrow} actions={actions} />
            </>
          )}
          {escrow.state === "LOCKED" && <DisputeBox escrow={escrow} actions={actions} />}
          <p className="text-xs text-neutral-500">
            Once the funds are locked only the seller can cancel. If nothing happens, you are refunded at the
            deadline.
          </p>
        </div>
      );
    default:
      return <FinalSummary escrow={escrow} />;
  }
}

function SecretHealth({ stored }: { stored: ReturnType<typeof useStoredSecret> }) {
  if (stored.status === "missing") {
    return (
      <Notice tone="warn">
        This browser doesn&apos;t hold the release secret for this escrow (it was accepted on another device, or
        the browser data was cleared). Without it the payment can&apos;t be released and will be refunded at the
        deadline. If you have the 64-character secret elsewhere, paste it to release.
      </Notice>
    );
  }
  if (stored.status === "mismatch") {
    return (
      <Notice tone="error">
        The secret stored in this browser doesn&apos;t match this escrow&apos;s payment hash, so it can&apos;t
        release it.
      </Notice>
    );
  }
  return null;
}

function InvoiceBox({ escrow }: { escrow: Escrow }) {
  const [invoice, setInvoice] = useState<Invoice | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .invoice(escrow.id)
      .then((inv) => alive && setInvoice(inv))
      .catch((err) => alive && setError(describeError(err)));
    return () => {
      alive = false;
    };
  }, [escrow.id, escrow.payment_hash]);

  if (error) return <Notice tone="error">{error}</Notice>;
  if (!invoice) return <Spinner label="Fetching the invoice…" />;

  const expires = invoice.expires_at ?? escrow.invoice_expires_at;
  return (
    <div className="space-y-4">
      <div className="flex flex-col items-center gap-3">
        {/* Generated locally: the invoice is never sent to a QR service. Upper case makes a smaller code. */}
        <div className="rounded-xl bg-white p-3">
          <QRCodeSVG
            value={`lightning:${invoice.payment_request.toUpperCase()}`}
            size={232}
            marginSize={2}
            className="h-auto w-full max-w-[232px]"
            title="Lightning invoice"
          />
        </div>
        <div className="text-sm font-medium">Scan with a Lightning wallet to pay {formatSats(escrow.amount_sats)}</div>
      </div>
      <CopyField value={invoice.payment_request} label="Copy invoice" />
      {expires && <Countdown until={expires} label="Pay within" endedText="Expired" />}
      {process.env.NODE_ENV === "development" && (
        <details className="text-xs text-neutral-500">
          <summary className="cursor-pointer">Paying on regtest</summary>
          <p className="mt-2">From the repository root:</p>
          <CopyField
            value={`docker compose -f regtest/docker-compose.yml exec -T buyer lncli --network=regtest payinvoice --force ${invoice.payment_request}`}
          />
        </details>
      )}
    </div>
  );
}

/** The buyer's release QR: a link with the secret in the fragment. Hidden until revealed. */
function ReleaseHandover({ escrow, secret, actions }: { escrow: Escrow; secret: string; actions: Actions }) {
  const [shown, setShown] = useState(false);
  const link = buildReleaseLink(appUrl(), escrow.id, secret);

  if (!shown) {
    return (
      <div className="space-y-3 rounded-lg border border-neutral-200 p-4 dark:border-neutral-800">
        <div className="font-semibold">Release QR</div>
        <p className="text-sm text-neutral-600 dark:text-neutral-400">
          Showing this QR is how you pay the seller. Anyone who scans it can release the payment, so only reveal it
          when you have what you paid for.
        </p>
        <ConfirmButton
          confirmText="Have you received what you paid for? Whoever scans the QR can release the payment to the seller."
          confirmLabel="Yes, show the QR"
          onConfirm={() => setShown(true)}
          variant="primary"
        >
          Reveal release QR
        </ConfirmButton>
      </div>
    );
  }

  return (
    <div className="space-y-4 rounded-lg border border-amber-400 p-4 dark:border-amber-700">
      <div className="flex flex-col items-center gap-3">
        <div className="rounded-xl bg-white p-3">
          <QRCodeSVG value={link} size={256} marginSize={2} className="h-auto w-full max-w-[256px]" title="Release QR" />
        </div>
        <p className="text-center text-sm">
          Let the seller scan this with their phone camera. It opens this escrow with the secret filled in.
        </p>
      </div>
      <div className="flex flex-wrap gap-2">
        <CopyButton value={link} label="Copy release link" />
        <CopyButton value={secret} label="Copy secret only" />
        <Button variant="ghost" onClick={() => setShown(false)}>
          Hide
        </Button>
      </div>
      <div className="border-t border-neutral-200 pt-3 dark:border-neutral-800">
        <ConfirmButton
          confirmText={`Release ${formatSats(escrow.amount_sats)} to the seller now? This can't be undone.`}
          confirmLabel="Release now"
          busy={actions.busy === "release"}
          onConfirm={() => actions.run("release", () => api.release(escrow.id, secret))}
        >
          Or release it yourself from here
        </ConfirmButton>
      </div>
    </div>
  );
}

// Seller

function SellerPanel({ escrow, actions, linkSecret }: { escrow: Escrow; actions: Actions; linkSecret: string | null }) {
  const refund = (
    <ConfirmButton
      confirmText={
        escrow.state === "CREATED" || escrow.state === "INVOICE_ISSUED"
          ? "Withdraw this offer? The buyer won't be able to pay."
          : `Give the buyer their ${formatSats(escrow.amount_sats)} back? You will not be paid. This can't be undone.`
      }
      confirmLabel={escrow.state === "CREATED" || escrow.state === "INVOICE_ISSUED" ? "Withdraw offer" : "Refund the buyer"}
      variant="danger"
      busy={actions.busy === "cancel"}
      onConfirm={() => actions.run("cancel", () => api.cancel(escrow.id))}
    >
      {escrow.state === "CREATED" || escrow.state === "INVOICE_ISSUED" ? "Withdraw offer" : "Give refund"}
    </ConfirmButton>
  );

  switch (escrow.state) {
    case "CREATED":
      return (
        <div className="space-y-3">
          <div className="text-sm font-medium">Link for the buyer</div>
          <CopyField value={`${appUrl()}/escrows/${escrow.id}`} label="Copy link" />
          <p className="text-xs text-neutral-500">
            {escrow.buyer_id ? (
              <>
                The buyer must be signed in as <span className="font-mono">{escrow.buyer_id}</span> to open it.
              </>
            ) : (
              <>
                This is an open offer: <strong>the first person to open the link and accept becomes the buyer</strong>,
                so send it only to the person you are dealing with.
              </>
            )}{" "}
            The offer lapses on its own if nobody has accepted by the deadline.
          </p>
          {refund}
        </div>
      );
    case "INVOICE_ISSUED":
      return (
        <div className="space-y-4">
          <p className="flex items-center gap-2 text-sm">
            <span className="size-2 animate-pulse rounded-full bg-sky-500" />
            Waiting for the buyer&apos;s payment.
          </p>
          {escrow.invoice_expires_at && (
            <Countdown until={escrow.invoice_expires_at} label="Invoice expires in" endedText="Expired" />
          )}
          {refund}
        </div>
      );
    case "LOCKED":
    case "DISPUTED":
      return (
        <div className="space-y-4">
          {escrow.state === "LOCKED" && (
            <Notice tone="success">
              <strong>Funds are locked: you can start the work.</strong>
            </Notice>
          )}
          <Deadline escrow={escrow} />
          <ReleaseForm
            key={linkSecret ?? "manual"}
            escrow={escrow}
            actions={actions}
            initial={linkSecret ?? ""}
            fromLink={!!linkSecret}
          />
          <div className="flex flex-wrap gap-2">
            {refund}
          </div>
          {escrow.state === "LOCKED" && <DisputeBox escrow={escrow} actions={actions} />}
        </div>
      );
    default:
      return <FinalSummary escrow={escrow} />;
  }
}

/** Paste or confirm the 64-hex secret and release. Never releases without a button press. */
function ReleaseForm({
  escrow,
  actions,
  initial = "",
  fromLink = false,
}: {
  escrow: Escrow;
  actions: Actions;
  initial?: string;
  fromLink?: boolean;
}) {
  const [value, setValue] = useState(initial);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    const parsed = SecretInput.safeParse(value);
    if (!parsed.success) {
      setError(parsed.error.issues[0]?.message ?? "Invalid secret");
      return;
    }
    setError(null);
    await actions.run("release", () => api.release(escrow.id, parsed.data));
  }

  return (
    <form onSubmit={submit} className="space-y-3 rounded-lg border border-neutral-200 p-4 dark:border-neutral-800">
      <div className="font-semibold">Release payment</div>
      {fromLink && (
        <Notice tone="info">
          The secret was filled in from the release link. Check the handover is complete, then press Release.
        </Notice>
      )}
      <Field
        label="Release secret"
        hint="Scan the buyer's release QR with your phone camera, or paste the 64-character secret."
        error={error ?? undefined}
      >
        <input
          value={value}
          onChange={(e) => {
            setValue(e.target.value);
            setError(null);
          }}
          className={`${inputClass} font-mono`}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck={false}
          placeholder="64 hex characters"
          maxLength={80}
        />
      </Field>
      <Button type="submit" busy={actions.busy === "release"}>
        Release {formatSats(escrow.amount_sats)}
      </Button>
    </form>
  );
}

function DisputeBox({ escrow, actions }: { escrow: Escrow; actions: Actions }) {
  const [open, setOpen] = useState(false);
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);

  if (!open) {
    return (
      <Button variant="ghost" onClick={() => setOpen(true)}>
        Open a dispute…
      </Button>
    );
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    const parsed = DisputeForm.safeParse(reason);
    if (!parsed.success) {
      setError(parsed.error.issues[0]?.message ?? "Invalid reason");
      return;
    }
    if (await actions.run("dispute", () => api.dispute(escrow.id, parsed.data))) setOpen(false);
  }

  return (
    <form onSubmit={submit} className="space-y-3 rounded-lg border border-neutral-200 p-4 dark:border-neutral-800">
      <div className="font-semibold">Open a dispute</div>
      <p className="text-sm text-neutral-600 dark:text-neutral-400">
        Flags the escrow for the operator. It does not extend the deadline: unless the secret is submitted, the buyer
        is still refunded when it passes.
      </p>
      <Field label="What went wrong?" error={error ?? undefined}>
        <textarea
          value={reason}
          onChange={(e) => {
            setReason(e.target.value);
            setError(null);
          }}
          rows={3}
          maxLength={500}
          className={inputClass}
        />
      </Field>
      <div className="flex flex-wrap gap-2">
        <Button type="submit" variant="danger" busy={actions.busy === "dispute"}>
          Open dispute
        </Button>
        <Button variant="ghost" onClick={() => setOpen(false)}>
          Never mind
        </Button>
      </div>
    </form>
  );
}

// Operator

function OperatorPanel({ escrow, actions }: { escrow: Escrow; actions: Actions }) {
  const cancellable = ["INVOICE_ISSUED", "LOCKED", "DISPUTED"].includes(escrow.state);
  return (
    <div className="space-y-4">
      {(escrow.state === "LOCKED" || escrow.state === "DISPUTED") && <Deadline escrow={escrow} />}
      {cancellable ? (
        <ConfirmButton
          confirmText={`Cancel and refund ${formatSats(escrow.amount_sats)} to the buyer? This can't be undone.`}
          confirmLabel="Cancel and refund"
          variant="danger"
          busy={actions.busy === "cancel"}
          onConfirm={() => actions.run("cancel", () => api.cancel(escrow.id))}
        >
          Cancel escrow
        </ConfirmButton>
      ) : (
        <FinalSummary escrow={escrow} />
      )}
    </div>
  );
}
