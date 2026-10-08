import { defineConfig } from "vitest/config";

// Unit tests only (no Tauri): the DOM comes from happy-dom.
export default defineConfig({
  test: {
    environment: "happy-dom",
    include: ["src/**/*.test.ts"],
  },
});
