import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// base: './' → relative asset paths, so the built site deploys to GitHub Pages
// (or any subpath) without further configuration.
export default defineConfig({
  base: './',
  plugins: [react()],
})
