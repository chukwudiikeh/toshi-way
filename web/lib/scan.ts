import { parseReleaseFragment, parseSecret } from "./secret";

export type ScanResult = { secret: string } | { error: string };

/**
 * Turn the text of a scanned QR code into a release secret. Accepts the release link the buyer's
 * page shows (`.../escrows/<id>#release=<64 hex>`), checking that it is for this escrow, or a bare
 * 64-hex secret. Anything else, including short codes, is refused.
 */
export function secretFromScan(text: string, escrowId: string): ScanResult {
  const trimmed = text.trim();
  const bare = parseSecret(trimmed);
  if (bare) return { secret: bare };

  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    return { error: "That QR code is not a release code." };
  }
  const secret = parseReleaseFragment(url.hash);
  if (!secret) return { error: "That QR code is not a release code." };
  const target = url.pathname.split("/").filter(Boolean).pop();
  if (target !== escrowId) return { error: "That release code is for a different escrow." };
  return { secret };
}

interface BarcodeDetectorLike {
  detect(source: CanvasImageSource): Promise<{ rawValue: string }[]>;
}
type BarcodeDetectorCtor = new (opts: { formats: string[] }) => BarcodeDetectorLike;

function detectorCtor(): BarcodeDetectorCtor | null {
  if (typeof window === "undefined") return null;
  const ctor = (window as unknown as { BarcodeDetector?: BarcodeDetectorCtor }).BarcodeDetector;
  return ctor ?? null;
}

/** The scanner uses the browser's own QR detection and camera; no library or third party. */
export function scannerSupported(): boolean {
  return detectorCtor() !== null && typeof navigator !== "undefined" && !!navigator.mediaDevices?.getUserMedia;
}

export function createDetector(): BarcodeDetectorLike | null {
  const Ctor = detectorCtor();
  return Ctor ? new Ctor({ formats: ["qr_code"] }) : null;
}
