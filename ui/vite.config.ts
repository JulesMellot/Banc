import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [svelte()],
  server: {
    // `cargo xtask dev-server` listens there (dev/server.toml).
    proxy: { '/api': 'http://127.0.0.1:8080' },
  },
  test: {
    include: ['src/**/*.test.ts'],
  },
});
