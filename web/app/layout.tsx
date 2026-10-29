import type { Metadata, Viewport } from "next";
import { Suspense } from "react";
import { DemoWallet } from "@/components/DemoWallet";
import { AppShell } from "@/components/AppShell";
import "./globals.css";

export const metadata: Metadata = {
  title: "toshi-way · Lightning escrow",
  description: "Escrow for buyers and sellers on Bitcoin Lightning, using hold invoices.",
  // Escrow pages are private; keep them out of search engines.
  robots: { index: false, follow: false },
  // The release secret travels in a URL fragment; never leak the page URL to other sites.
  referrer: "no-referrer",
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
};

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    <html lang="en" className="h-full antialiased" data-scroll-behavior="smooth">
      <body className="flex min-h-full flex-col">
        <DemoWallet />
        <Suspense>
          <AppShell>{children}</AppShell>
        </Suspense>
      </body>
    </html>
  );
}
