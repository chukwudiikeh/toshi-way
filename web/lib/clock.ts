// Server-corrected time. Deadlines come from the server's clock; if this device's clock is off,
// a naive countdown would lie. /health reports `server_time`, from which we keep an offset.
// The server's answer still wins: a countdown is only an indication.

import { useEffect, useState } from "react";
import { api } from "./api";

let offsetSecs = 0;
let synced: Promise<void> | null = null;

/** Measure the offset once per page load (again after a failure). */
export function syncClock(): Promise<void> {
  if (!synced) {
    synced = (async () => {
      const before = Date.now();
      const h = await api.health();
      const after = Date.now();
      if (typeof h.server_time === "number") {
        // Assume the server read its clock halfway through the round trip.
        offsetSecs = h.server_time - (before + after) / 2000;
      }
    })().catch(() => {
      synced = null;
    });
  }
  return synced;
}

export function serverNow(): number {
  return Date.now() / 1000 + offsetSecs;
}

/** Server-corrected unix seconds, updated every second. Null until mounted in the browser. */
export function useServerNow(): number | null {
  const [now, setNow] = useState<number | null>(null);
  useEffect(() => {
    let alive = true;
    const tick = () => alive && setNow(serverNow());
    syncClock().then(tick);
    tick();
    const t = setInterval(tick, 1000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);
  return now;
}

/** "1h 02m 05s", "4m 09s", "12s"; "0s" once passed. */
export function formatDuration(totalSecs: number): string {
  const s = Math.max(0, Math.floor(totalSecs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  if (h > 0) return `${h}h ${pad(m)}m ${pad(sec)}s`;
  if (m > 0) return `${m}m ${pad(sec)}s`;
  return `${sec}s`;
}

export function formatLocalTime(unixSecs: number): string {
  return new Date(unixSecs * 1000).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "medium",
  });
}

export function formatSats(sats: number): string {
  return `${sats.toLocaleString("en-US")} sats`;
}
