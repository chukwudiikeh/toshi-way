"use client";

import { useEffect, useState } from "react";

const URL_BASE = process.env.NEXT_PUBLIC_DEMO_WALLET_URL;

/**
 * Demo only. When the app is started with NEXT_PUBLIC_DEMO_WALLET_URL pointing at
 * scripts/demo-wallet.mjs, this stands in for a WebLN wallet extension by handing invoices to that
 * local helper, which pays them from the regtest buyer node. It says so on screen. In any other
 * run the variable is unset and this renders nothing and installs nothing.
 */
export function DemoWallet() {
  const [active, setActive] = useState(false);

  useEffect(() => {
    if (!URL_BASE) return;
    // React runs effects twice in development, so install only once and always check afterwards
    // whether the wallet in place is ours (a real extension, if present, is left alone).
    if (!window.webln) {
      window.webln = Object.assign(
        {
          enable: async () => {},
          sendPayment: async (invoice: string) => {
            const res = await fetch(`${URL_BASE}/pay`, {
              method: "POST",
              headers: { "content-type": "application/json" },
              body: JSON.stringify({ invoice }),
            });
            if (!res.ok) throw new Error("The demo wallet refused the payment.");
            // A hold invoice keeps the payment pending while the funds are locked, like a real wallet.
            return new Promise(() => {});
          },
        },
        { isDemoWallet: true },
      );
    }
    const t = setTimeout(() => setActive((window.webln as { isDemoWallet?: boolean } | undefined)?.isDemoWallet === true), 0);
    return () => clearTimeout(t);
  }, []);

  if (!active) return null;
  return (
    <div className="fixed bottom-2 right-2 z-50 rounded-md bg-neutral-900/80 px-2 py-1 text-[10px] font-medium uppercase tracking-wide text-neutral-100">
      Demo wallet · regtest
    </div>
  );
}
