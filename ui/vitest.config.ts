import { defineConfig } from "vitest/config";

// Vitest 4 reads test config from its own file (vite 8 no longer types the `test`
// field on vite's config).
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
