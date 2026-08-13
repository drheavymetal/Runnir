// The other half of the proof: can a QR decoder actually READ what runnir paints?
//
// The wire-format cross-check settles that runnir's BYTES are decodable. This one
// takes the PIXELS — the exact RGBA the renderer uploads as a texture — and hands
// them to zxing-wasm, the same decoder the receiver runs in the browser, then
// pushes whatever comes back through the fountain decoder to recover the file.
//
// A failure here means a code was painted wrong (scale, quiet zone, contrast,
// version), which is invisible to every byte-level test.
//
// Usage, from the repository root (the deps live in docs-site, which is why
// this script does too):
//   RUNNIR_PAINTED_FRAMES=/tmp/painted cargo test -- --ignored emit_painted_frames
//   node --import tsx docs-site/tools/optical-painted-check.mjs /tmp/painted

import { readFileSync, readdirSync } from "node:fs";
import { readBarcodes } from "zxing-wasm/reader";
import { LTDecoder } from "../src/receive/vendor/fountain.ts";
import { fnv1a, parseFrame, unpackFile } from "../src/receive/vendor/protocol.ts";

const dir = process.argv[2];
const meta = JSON.parse(readFileSync(`${dir}/index.json`, "utf8"));
const files = readdirSync(dir).filter((f) => f.endsWith(".rgba")).sort();
// A drawn image may hold a MOSAIC of codes; older index files only had `px`.
const width = meta.w ?? meta.px;
const height = meta.h ?? meta.px;
const tiles = meta.tiles ?? 1;
console.log(
  `${files.length} painted images, ${width}x${height}, ${meta.grid ?? "1x1"} codes each, V${meta.version}, k=${meta.blocks}`,
);

let unreadable = 0;
let codes = 0;
let blueCodes = 0;
let decoder = null;
const seen = new Set();

// A colour stream carries a SECOND code per tile in the blue channel, and the
// claim that makes it safe is that the first one is still an ordinary QR. So the
// pixels are read exactly as before — no special case, no hint that colour is in
// use — and the extra codes come from a second pass over the blue channel alone,
// which is what the receiver's worker does.
const color = meta.color === true;

for (const file of files) {
  const raw = readFileSync(`${dir}/${file}`);
  const options = { formats: ["QRCode"], maxNumberOfSymbols: tiles };
  const pixels = new Uint8ClampedArray(raw);
  const results = await readBarcodes({ data: pixels, width, height }, options);
  const hits = results.filter((r) => r.isValid && r.bytes.length > 0);
  const baseCount = hits.length;
  if (color) {
    // In place, over the same buffer, once the first read has finished with it.
    for (let i = 0; i < pixels.length; i += 4) {
      pixels[i] = pixels[i + 2];
      pixels[i + 1] = pixels[i + 2];
    }
    const extra = (await readBarcodes({ data: pixels, width, height }, options)).filter(
      (r) => r.isValid && r.bytes.length > 0,
    );
    blueCodes += extra.length;
    if (extra.length < tiles) {
      console.log(`  PARTIAL BLUE ${file}: ${extra.length}/${tiles} codes`);
    }
    hits.push(...extra);
  }
  if (hits.length === 0) {
    unreadable++;
    console.log(`  UNREADABLE ${file}`);
    continue;
  }
  // Every tile in the image has to come back, not just one: a mosaic that reads
  // as a single code is a mosaic that bought nothing. Counted on the ordinary
  // scan alone, which is the one every decoder in the world performs.
  if (baseCount < tiles) {
    console.log(`  PARTIAL ${file}: ${baseCount}/${tiles} codes`);
  }
  for (const hit of hits) {
    const parsed = parseFrame(new Uint8Array(hit.bytes));
    if (!parsed) {
      console.log(`  read but did not parse: ${file}`);
      continue;
    }
    codes++;
    if (!decoder) {
      const h = parsed.header;
      decoder = new LTDecoder(h.k, h.blockLen, h.sessionId, h.totalLen);
      decoder.fnv = h.payloadFnv;
    }
    seen.add(parsed.header.seq);
    decoder.addFrame(parsed.header.seq, parsed.block);
  }
  if (decoder?.isComplete) break;
}

console.log(`  ${files.length - unreadable}/${files.length} images read by zxing, ${codes} codes total`);
if (color) console.log(`  ${blueCodes} of them came off the blue channel`);
if (unreadable > 0) console.log(`  ${unreadable} unreadable`);

if (!decoder?.isComplete) {
  console.log("\nFAILED — the painted frames did not rebuild the file");
  process.exit(1);
}

const container = decoder.assemble();
const file = await unpackFile(container);
const sha = Buffer.from(await crypto.subtle.digest("SHA-256", Uint8Array.from(file.bytes))).toString("hex");
const code = String(parseInt(sha.slice(0, 8), 16) % 1000000).padStart(6, "0");

const checks = [
  ["the container checksum matches", fnv1a(container) === decoder.fnv],
  ["the name survives", file.name === meta.name],
  ["the length survives", file.bytes.length === meta.size],
  ["the hash matches what the sender promised", Buffer.from(file.sha256).toString("hex") === sha],
  [`the verification code agrees (${code})`, code === meta.code],
];
let failed = 0;
for (const [label, ok] of checks) {
  console.log(`  ${ok ? "ok  " : "FAIL"} ${label}`);
  if (!ok) failed++;
}

console.log(
  failed === 0
    ? "\nALL PASS — a QR decoder reads runnir's painted frames and rebuilds the file"
    : `\n${failed} FAILED`,
);
process.exit(failed === 0 ? 0 : 1);
