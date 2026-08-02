// Read a QR frame back out of a RENDERED runnir window.
//
// The painted-frames check proves the raster is right. This one proves the whole
// path is: the demo scene goes through the real wgpu pipeline — the image quad,
// the linear sampler, the sRGB surface — and any of those could soften the code
// enough to stop a decoder, with nothing in the byte-level tests noticing.
//
// Usage, from the repository root (the deps live in docs-site, which is why
// this script does too):
//   runnir --demo /tmp/scene.png transfer
//   magick /tmp/scene.png -depth 8 RGBA:/tmp/scene.raw
//   node --import tsx docs-site/tools/optical-scene-check.mjs /tmp/scene.raw 1400 1000

import { readFileSync } from "node:fs";
import { readBarcodes } from "zxing-wasm/reader";
import { parseFrame } from "../src/receive/vendor/protocol.ts";

const [, , file, width, height] = process.argv;
if (!file || !width || !height) {
  console.error("usage: optical-scene-check.mjs <scene.raw> <width> <height>");
  process.exit(2);
}

const raw = readFileSync(file);
const results = await readBarcodes(
  { data: new Uint8ClampedArray(raw), width: +width, height: +height },
  { formats: ["QRCode"], maxNumberOfSymbols: 1 },
);
const hit = results.find((r) => r.isValid && r.bytes.length > 0);
if (!hit) {
  console.log("FAILED — no code found in the rendered window");
  process.exit(1);
}

const parsed = parseFrame(new Uint8Array(hit.bytes));
if (!parsed) {
  console.log("FAILED — a code was read, but it is not a runnir frame");
  process.exit(1);
}

const h = parsed.header;
console.log(
  `PASS — read a real frame off the rendered window: ` +
    `seq=${h.seq} k=${h.k} blockLen=${h.blockLen} session=${h.sessionId}`,
);
