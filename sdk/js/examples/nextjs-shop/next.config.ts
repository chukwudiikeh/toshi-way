import { fileURLToPath } from "node:url";
import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // The client is linked from the folder two levels up (`file:../..`), so the bundler's root has to
  // include it. Once the client is installed from npm or a tarball, delete this block.
  turbopack: { root: fileURLToPath(new URL("../..", import.meta.url)) },
};

export default nextConfig;
