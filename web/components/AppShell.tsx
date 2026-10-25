"use client";

import { usePathname } from "next/navigation";
import type { ReactNode } from "react";
import { useSession } from "@/lib/session";
import { Header } from "./Header";

/**
 * The page chrome. Signed-in people (and every inner page) get the app header, a centred column
 * and a footer. A signed-out visitor on the home page gets the landing page instead, which is
 * full-width and brings its own navigation and footer, so the chrome steps aside for it.
 */
export function AppShell({ children }: { children: ReactNode }) {
  const path = usePathname();
  const session = useSession();

  // Signed out on the home page: the landing page. While the browser has not yet said whether
  // anyone is signed in, keep the landing's colour (rather than flashing the app chrome at a
  // first-time visitor) and still render the page, which draws nothing until it knows.
  const landing = path === "/" && (session === null || session === undefined);

  // The page must stay mounted in exactly the same place whichever layout is showing. Creating an
  // account signs you in at once, which flips this from the landing layout to the app layout; if
  // the page were rebuilt by that, the one-time "save your token" screen would be lost with it.
  // So only classes and the optional pieces around it change, never the element that holds it.
  return (
    <>
      {landing ? null : <Header />}
      <div
        role={landing ? undefined : "main"}
        className={
          landing
            ? "flex flex-1 flex-col bg-neutral-950"
            : path === "/"
              ? "mx-auto w-full max-w-6xl flex-1 px-4 py-6 sm:px-6 sm:py-8"
              : "mx-auto w-full max-w-3xl flex-1 px-4 py-6 sm:py-10"
        }
      >
        {children}
      </div>
      {landing ? null : (
        <footer className="px-4 py-6 text-center text-xs text-neutral-500">
          The page only shows what the escrow server reports. Funds are held by the Lightning network, not by
          this site.
        </footer>
      )}
    </>
  );
}
