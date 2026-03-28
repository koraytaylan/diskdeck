/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  server: {
    port: 1420,
    strictPort: true,
  },
  css: {
    modules: {
      localsConvention: "camelCaseOnly",
    },
  },
  build: {
    target: "esnext",
  },
  test: {
    environment: "happy-dom",
    globals: true,
  },
});
