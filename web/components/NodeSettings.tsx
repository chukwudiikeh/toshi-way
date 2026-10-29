"use client";

import { useEffect, useState, type FormEvent } from "react";
import { api, describeError } from "@/lib/api";
import { formatLocalTime } from "@/lib/clock";
import { NodeForm, type NodeInfo } from "@/lib/schemas";
import { useSession } from "@/lib/session";
import { SignIn } from "./Account";
import { KeyIcon } from "./landing/icons";
import { PageHeader, Step, StatusPanel } from "./PageParts";
import {
  Button,
  Card,
  ConfirmButton,
  CopyField,
  Field,
  inputClass,
  Notice,
  Spinner,
} from "./ui";

/** Where a seller connects their own Lightning node, so a release pays them directly. */
export function NodeSettings() {
  const session = useSession();
  const [info, setInfo] = useState<NodeInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!session || session.kind !== "user") return;
    let alive = true;
    api
      .node()
      .then((n) => alive && setInfo(n))
      .catch((err) => alive && setError(describeError(err)));
    return () => {
      alive = false;
    };
  }, [session]);

  if (session === undefined) return <Spinner />;
  if (!session)
    return <SignIn note="Sign in to connect your Lightning node." />;
  if (session.kind === "operator") {
    return (
      <Notice tone="info">Operators don&apos;t have a seller node.</Notice>
    );
  }
  if (error) return <Notice tone="error">{error}</Notice>;
  if (!info) return <Spinner />;

  return (
    <div className="space-y-6">
      <PageHeader
        icon={<KeyIcon className="size-5" />}
        title="Your Lightning node"
      >
        Connect your own node so released payments land with you directly and
        nobody else holds your sats.
      </PageHeader>
      <Status info={info} />
      {!info.own_nodes_enabled ? (
        <Notice tone="warn">
          This server isn&apos;t set up to take sellers&apos; own nodes.
        </Notice>
      ) : (
        <ConnectForm info={info} onChange={setInfo} />
      )}
    </div>
  );
}

function Status({ info }: { info: NodeInfo }) {
  if (info.mode === "own") {
    return (
      <StatusPanel tone="good" label="Connected">
        <p>
          <strong>Your node is connected.</strong> New escrows are issued on it,
          and a release pays you directly.
        </p>
        {info.address && (
          <p className="break-all font-mono text-xs text-neutral-600 dark:text-neutral-400">
            {info.address}
            {info.connected_at
              ? ` · connected ${formatLocalTime(info.connected_at)}`
              : ""}
          </p>
        )}
      </StatusPanel>
    );
  }
  if (info.mode === "shared") {
    return (
      <StatusPanel tone="warn" label="Shared node">
        <p>
          <strong>No node connected.</strong> Your escrows use the shared node,
          so the service receives the sats on release and owes them to you.
        </p>
      </StatusPanel>
    );
  }
  return (
    <StatusPanel tone="idle" label="Not connected">
      <p>
        <strong>No node connected.</strong> You need to connect a node before
        you can create an escrow, because this service doesn&apos;t hold
        sellers&apos; money.
      </p>
    </StatusPanel>
  );
}

function ConnectForm({
  info,
  onChange,
}: {
  info: NodeInfo;
  onChange: (n: NodeInfo) => void;
}) {
  const [values, setValues] = useState({
    address: "",
    tls_cert: "",
    macaroon: "",
  });
  const [errors, setErrors] = useState<
    Partial<Record<keyof typeof values, string>>
  >({});
  const [formError, setFormError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState(false);

  function set(key: keyof typeof values, value: string) {
    setValues((v) => ({ ...v, [key]: value }));
    setErrors((e) => ({ ...e, [key]: undefined }));
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    const parsed = NodeForm.safeParse(values);
    if (!parsed.success) {
      const next: typeof errors = {};
      for (const issue of parsed.error.issues) {
        const key = issue.path[0] as keyof typeof values;
        next[key] ??= issue.message;
      }
      setErrors(next);
      return;
    }
    setBusy(true);
    try {
      onChange(await api.connectNode(parsed.data));
      // The macaroon is a credential: don't keep it in the form once it has been handed over.
      setValues({ address: "", tls_cert: "", macaroon: "" });
    } catch (err) {
      setFormError(describeError(err));
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    setRemoving(true);
    setFormError(null);
    try {
      onChange(await api.disconnectNode());
    } catch (err) {
      setFormError(describeError(err));
    } finally {
      setRemoving(false);
    }
  }

  const locked = !info.can_change;
  return (
    <div className="mx-auto w-full max-w-2xl space-y-6">
      <Card
        title={info.mode === "own" ? "Replace your node" : "Connect your node"}
      >
        <form onSubmit={submit} className="space-y-4" noValidate>
          {locked && (
            <Notice tone="warn">
              You have escrows in progress on your node. You can change it again
              once they have finished.
            </Notice>
          )}
          <Field
            label="Node address"
            hint="Your node's gRPC address. It must be reachable from this service and support hold invoices."
            error={errors.address}
          >
            <input
              value={values.address}
              onChange={(e) => set("address", e.target.value)}
              className={`${inputClass} font-mono`}
              placeholder="https://mynode.example.com:10009"
              autoComplete="off"
              spellCheck={false}
              disabled={locked}
            />
          </Field>
          <Field
            label="TLS certificate"
            hint="Usually ~/.lnd/tls.cert. We only connect to a server presenting exactly this certificate."
            error={errors.tls_cert}
          >
            <textarea
              value={values.tls_cert}
              onChange={(e) => set("tls_cert", e.target.value)}
              className={`${inputClass} font-mono text-xs`}
              rows={4}
              placeholder="-----BEGIN CERTIFICATE-----"
              spellCheck={false}
              disabled={locked}
            />
          </Field>
          <Field
            label="Invoice macaroon (hex)"
            hint="Stored encrypted. It can create and cancel invoices on your node but cannot spend your funds."
            error={errors.macaroon}
          >
            <input
              type="password"
              value={values.macaroon}
              onChange={(e) => set("macaroon", e.target.value)}
              className={`${inputClass} font-mono`}
              autoComplete="off"
              spellCheck={false}
              disabled={locked}
            />
          </Field>
          {formError && <Notice tone="error">{formError}</Notice>}
          <div className="flex flex-wrap gap-2">
            <Button type="submit" busy={busy} disabled={locked}>
              Check and save
            </Button>
            {info.mode === "own" && (
              <ConfirmButton
                confirmText="Disconnect your node? New escrows will need a node again."
                confirmLabel="Yes, disconnect"
                variant="danger"
                busy={removing}
                onConfirm={remove}
                disabled={locked}
              >
                Disconnect
              </ConfirmButton>
            )}
          </div>
          <p className="text-xs text-neutral-500">
            Saving connects to your node, checks that it is synced, and makes
            and cancels a one-sat test invoice to prove the macaroon works.
            Nothing is saved if any step fails.
          </p>
        </form>
      </Card>
      <details
        open={info.mode !== "own"}
        className="group rounded-2xl border border-neutral-200 bg-white shadow-sm dark:border-neutral-800 dark:bg-neutral-900/60"
      >
        <summary className="flex cursor-pointer items-center justify-between px-5 py-4 font-semibold">
          How to get these details
          <span
            className="text-neutral-400 transition-transform group-open:rotate-180"
            aria-hidden
          >
            ▾
          </span>
        </summary>
        <div className="border-t border-neutral-200 px-5 py-5 dark:border-neutral-800">
          <ol className="space-y-5">
            <Step n={1}>
              Bake a macaroon that can <strong>only</strong> handle invoices
              (never share your admin macaroon):
              <CopyField value="lncli bakemacaroon invoices:read invoices:write info:read --save_to=toshi.macaroon" />
            </Step>
            <Step n={2}>
              Turn it into hex text and paste it into the form:
              <CopyField value="xxd -p -c 1000 toshi.macaroon" />
            </Step>
            <Step n={3}>
              Paste your node&apos;s TLS certificate (usually{" "}
              <code>~/.lnd/tls.cert</code>). We connect only to a server
              presenting exactly this certificate.
            </Step>
            <Step n={4}>
              Your node&apos;s gRPC address, as <code>https://host:10009</code>.
              It must be reachable from this service and must support hold
              invoices (LND does).
            </Step>
          </ol>
        </div>
      </details>
    </div>
  );
}
