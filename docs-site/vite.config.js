import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import basicSsl from '@vitejs/plugin-basic-ssl'

// base: './' → relative asset paths, so the built site deploys to GitHub Pages
// (or any subpath) without further configuration.
//
// basicSsl + host: the receiver (#recibir) needs the camera, and getUserMedia
// does not exist at all on an insecure origin — localhost is exempt, a phone on
// the LAN is not. Without this, `npm run dev` is untestable on the one device
// the page is actually for. The certificate is self-signed, so the phone shows a
// warning once per session; that is the cost of testing before deploying.
export default defineConfig({
  base: './',
  plugins: [react(), basicSsl()],
  server: { host: true },
})
