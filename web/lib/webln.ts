// One-click payment through a WebLN wallet (for example a browser extension), when one is present.
// The invoice is handed to the user's own wallet; this app never learns whether the payment went
// through. Whether funds are locked is still decided by the API, from the node.

export interface WebLNProvider {
  enable(): Promise<void>;
  sendPayment(paymentRequest: string): Promise<unknown>;
}

declare global {
  interface Window {
    webln?: WebLNProvider;
  }
}

export function getWebLN(): WebLNProvider | null {
  if (typeof window === "undefined") return null;
  const w = window.webln;
  return w && typeof w.enable === "function" && typeof w.sendPayment === "function" ? w : null;
}

/**
 * Ask the wallet to pay `invoice`. Resolves once the wallet has accepted the request: a hold
 * invoice does not settle while it is locked, so a wallet may keep `sendPayment` pending until the
 * escrow ends. We therefore do not wait for it, and only report a wallet that refuses or fails
 * outright. Throws a user-readable Error.
 */
export async function payWithWebLN(invoice: string): Promise<void> {
  const webln = getWebLN();
  if (!webln) throw new Error("No WebLN wallet was found in this browser.");
  try {
    await webln.enable();
  } catch {
    throw new Error("The wallet did not allow this site to connect.");
  }
  const payment = webln.sendPayment(invoice);
  const outcome = await Promise.race([
    payment.then(
      () => "paid" as const,
      (err: unknown) => ({ failed: err }),
    ),
    new Promise<"pending">((resolve) => setTimeout(() => resolve("pending"), 3000)),
  ]);
  if (typeof outcome === "object") {
    const msg = outcome.failed instanceof Error ? outcome.failed.message : "";
    throw new Error(msg ? `The wallet could not pay: ${msg}` : "The wallet could not pay this invoice.");
  }
}
