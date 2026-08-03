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
//
// Add `color` as a fourth argument for a window rendered with
// `RUNNIR_DEMO_COLOR=1`: the blue channel then carries a second code per tile,
// and this reads it the way the receiver's worker does. It is opt-in because a
// PLAIN window's blue channel is its base code again — reading it unasked would
// return every frame twice and look like a stream sending duplicates.

import { readFileSync } from "node:fs";
import { readBarcodes } from "zxing-wasm/reader";
import { parseFrame } from "../src/receive/vendor/protocol.ts";

const [, , file, width, height, mode] = process.argv;
if (!file || !width || !height) {
  console.error("usage: optical-scene-check.mjs <scene.raw> <width> <height> [color]");
  process.exit(2);
}
const color = mode === "color";

const raw = readFileSync(file);
// A wide window shows a MOSAIC — several codes, each a different frame of the
// same stream. Read them all: the point of the mosaic is that ONE capture
// carries several frames, so a harness that stops at the first proves nothing
// about the second.
const pixels = new Uint8ClampedArray(raw);
const options = { formats: ["QRCode"], maxNumberOfSymbols: 16 };
const results = await readBarcodes({ data: pixels, width: +width, height: +height }, options);
const hits = results.filter((r) => r.isValid && r.bytes.length > 0);
if (hits.length === 0) {
  console.log("FAILED — no code found in the rendered window");
  process.exit(1);
}
if (color) {
  // The same in-place channel copy the worker does, after the ordinary read has
  // taken its copy. This is the pass that would catch the sRGB surface or the
  // linear sampler pulling the four colours toward each other.
  for (let i = 0; i < pixels.length; i += 4) {
    pixels[i] = pixels[i + 2];
    pixels[i + 1] = pixels[i + 2];
  }
  const extra = (
    await readBarcodes({ data: pixels, width: +width, height: +height }, options)
  ).filter((r) => r.isValid && r.bytes.length > 0);
  if (extra.length === 0) {
    console.log("FAILED — nothing readable in the blue channel of a colour window");
    process.exit(1);
  }
  hits.push(...extra);
}

const frames = [];
for (const hit of hits) {
  const parsed = parseFrame(new Uint8Array(hit.bytes));
  if (!parsed) {
    console.log("FAILED — a code was read, but it is not a runnir frame");
    process.exit(1);
  }
  frames.push(parsed.header);
}

// Every tile must carry a DIFFERENT frame. Two tiles showing the same sequence
// number would look identical on screen and double nothing.
const seqs = new Set(frames.map((h) => h.seq));
if (seqs.size !== frames.length) {
  console.log(`FAILED — ${frames.length} codes but only ${seqs.size} distinct sequence numbers`);
  process.exit(1);
}
const sessions = new Set(frames.map((h) => h.sessionId));
if (sessions.size !== 1) {
  console.log("FAILED — the codes on screen are not from the same stream");
  process.exit(1);
}

const h = frames[0];
console.log(
  `PASS — read ${frames.length} real frame(s) off the rendered window: ` +
    `seq=${frames.map((f) => f.seq).join(",")} k=${h.k} blockLen=${h.blockLen} session=${h.sessionId}`,
);
