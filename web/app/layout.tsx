import type { Metadata, Viewport } from "next";
import { Header } from "@/components/Header";
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
    <html lang="en" className="h-full antialiased">
      <body className="flex min-h-full flex-col">
        <Header />
        <main className="mx-auto w-full max-w-3xl flex-1 px-4 py-6 sm:py-10">{children}</main>
        <footer className="px-4 py-6 text-center text-xs text-neutral-500">
          The page only shows what the escrow server reports. Funds are held by the Lightning network, not by
          this site.
        </footer>
      </body>
    </html>
  );
}
