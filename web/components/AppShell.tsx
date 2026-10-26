"use client";

import { usePathname } from "next/navigation";
import type { ReactNode } from "react";
import { useSession } from "@/lib/session";
import { BoltIcon } from "./landing/icons";
import { Sidebar } from "./Sidebar";

/**
 * The page chrome. Signed-in people (and every inner page) get the app sidebar, a centred column
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
  // The dashboard and a single escrow use the full width; the other pages stay a narrow column.
  const wide = path === "/" || (path.startsWith("/escrows/") && path !== "/escrows/new");
  const footer = (
    <footer className="bg-neutral-950 px-4 py-8 text-neutral-400 sm:px-8">
      <div className="mx-auto flex max-w-6xl flex-col gap-3 text-xs sm:flex-row sm:items-center sm:justify-between">
        <span className="flex items-center gap-2 font-semibold text-neutral-200">
          <span className="grid size-6 place-items-center rounded-md bg-amber-500 text-black">
            <BoltIcon className="size-3" />
          </span>
          toshi-way
        </span>
        <span>Runs on a test network. No real money.</span>
        <span className="max-w-md">
          The page only shows what the escrow server reports. Funds are held by the Lightning network, not by this
          site.
        </span>
      </div>
    </footer>
  );

  return (
    <div className={`flex flex-1 ${landing ? "flex-col" : "flex-col lg:flex-row"}`}>
      {landing ? null : <Sidebar />}
      <div className="flex min-w-0 flex-1 flex-col">
        <div
          role={landing ? undefined : "main"}
          className={
            landing
              ? "flex flex-1 flex-col bg-neutral-950"
              : `mx-auto w-full flex-1 px-4 py-6 sm:px-8 sm:py-10 ${wide ? "max-w-6xl" : "max-w-3xl"}`
          }
        >
          {children}
        </div>
        {landing ? null : footer}
      </div>
    </div>
  );
}
