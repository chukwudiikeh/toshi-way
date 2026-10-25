import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Optional same-origin proxy to the API (and the demo wallet helper), for when the page is opened
  // through a single forwarded port, as in a Codespace or behind a tunnel: the browser then only ever
  // talks to this server, so there are no cross-origin calls and no other port has to be exposed.
  // Off unless API_PROXY_TARGET is set (the demo environment sets it); then the app is started with
  // NEXT_PUBLIC_API_URL="" so the client uses relative URLs.
  async rewrites() {
    const api = process.env.API_PROXY_TARGET;
    if (!api) return [];
    const wallet = process.env.DEMO_WALLET_TARGET;
    return [
      { source: "/api/v1/:path*", destination: `${api}/api/v1/:path*` },
      { source: "/health", destination: `${api}/health` },
      ...(wallet ? [{ source: "/demo-wallet/:path*", destination: `${wallet}/:path*` }] : []),
    ];
  },
  /* config options here */
  // The "N" route indicator is dev-only noise on a projector; compile and runtime errors still show.
  devIndicators: false,
  cacheComponents: true,
  partialPrefetching: true,
  turbopack: {
    rules: {
      "*.css": {
        loaders: ["@tailwindcss/turbopack"],
        as: "*.css",
      },
    },
  },
};

export default nextConfig;
