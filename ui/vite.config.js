import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  base: './',
  build: {
    outDir: 'dist',
    assetsInlineLimit: 0,
    target: ['es2020', 'safari14', 'chrome90'],
    chunkSizeWarningLimit: 900,
  },
  server: { port: 5214, strictPort: true },
});
