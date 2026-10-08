import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

// Tests for the release scripts (plain Node, no DOM): `npm run test:release`.
export default defineConfig({
  root: fileURLToPath(new URL("..", import.meta.url)),
  test: {
    environment: "node",
    include: ["scripts/**/*.test.mjs"],
  },
});
