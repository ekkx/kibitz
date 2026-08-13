import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

// The Rust server listens on 127.0.0.1:7777 (see docs/API.md).
// With VITE_MOCK=1 nothing reaches this proxy — the mock transport answers in-process.
//
// `@/…` resolves to `src/`. It exists because shadcn/ui generates components that
// import `@/lib/utils`; the alias is declared here for the bundler *and* in
// tsconfig.json for the type checker, and the two must be kept in step. vitest
// reads this same file, so tests resolve `@/…` without a config of their own.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    port: 5273,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:7777',
        changeOrigin: false,
      },
    },
  },
});
