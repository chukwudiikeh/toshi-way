"use client";

import { useEffect, useRef, useState } from "react";
import { createDetector } from "@/lib/scan";
import { Button, Notice } from "../ui";

/**
 * Camera QR scanner for the seller. Frames are analysed in the browser by the platform's
 * BarcodeDetector and never leave the device. Calls `onText` with each distinct decoded string and
 * stops the camera when closed or unmounted.
 */
export function Scanner({ onText, onClose }: { onText: (text: string) => void; onClose: () => void }) {
  const video = useRef<HTMLVideoElement>(null);
  const [error, setError] = useState<string | null>(null);
  const onTextRef = useRef(onText);
  useEffect(() => {
    onTextRef.current = onText;
  });

  useEffect(() => {
    let stream: MediaStream | null = null;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const detector = createDetector();

    async function start() {
      try {
        stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: "environment" }, audio: false });
      } catch {
        setError("The camera is not available. Allow camera access, or paste the secret instead.");
        return;
      }
      if (stopped) {
        stream.getTracks().forEach((t) => t.stop());
        return;
      }
      const el = video.current;
      if (!el || !detector) return;
      el.srcObject = stream;
      el.play().catch(() => {});
      let last = "";
      const tick = async () => {
        if (stopped) return;
        try {
          const found = await detector.detect(el);
          const text = found[0]?.rawValue;
          if (text && text !== last) {
            last = text;
            onTextRef.current(text);
          }
        } catch {
          // A frame that cannot be read is skipped.
        }
        timer = setTimeout(tick, 250);
      };
      tick();
    }
    start();

    return () => {
      stopped = true;
      clearTimeout(timer);
      stream?.getTracks().forEach((t) => t.stop());
    };
  }, []);

  return (
    <div className="space-y-3">
      {error ? (
        <Notice tone="error">{error}</Notice>
      ) : (
        <video ref={video} muted playsInline className="aspect-square w-full max-w-xs rounded-lg bg-black object-cover" />
      )}
      <Button variant="ghost" onClick={onClose}>
        Close camera
      </Button>
    </div>
  );
}
