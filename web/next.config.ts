import type { NextConfig } from "next";

const nextConfig: NextConfig = {
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
