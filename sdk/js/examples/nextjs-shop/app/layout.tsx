import type { ReactNode } from "react";

export const metadata = { title: "Example shop · toshi-way" };

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body style={{ fontFamily: "system-ui, sans-serif", maxWidth: 560, margin: "3rem auto", padding: "0 1rem", lineHeight: 1.5 }}>
        {children}
      </body>
    </html>
  );
}
