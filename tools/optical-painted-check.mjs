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
// Usage, from a decimen checkout with its dependencies installed:
//   RUNNIR_PAINTED_FRAMES=/tmp/painted cargo test -- --ignored emit_painted_frames
//   node --import tsx optical-painted-check.mjs /tmp/painted

import { readFileSync, readdirSync } from "node:fs";
import { readBarcodes } from "zxing-wasm/reader";
import { LTDecoder } from "./shared/fountain.ts";
import { fnv1a, parseFrame, unpackFile } from "./shared/protocol.ts";

const dir = process.argv[2];
const meta = JSON.parse(readFileSync(`${dir}/index.json`, "utf8"));
const files = readdirSync(dir).filter((f) => f.endsWith(".rgba")).sort();
console.log(`${files.length} painted frames, ${meta.px}px, V${meta.version}, k=${meta.blocks}`);

let unreadable = 0;
let decoder = null;
const seen = new Set();

for (const file of files) {
  const raw = readFileSync(`${dir}/${file}`);
  const results = await readBarcodes(
    { data: new Uint8ClampedArray(raw), width: meta.px, height: meta.px },
    { formats: ["QRCode"], maxNumberOfSymbols: 1 },
  );
  const hit = results.find((r) => r.isValid && r.bytes.length > 0);
  if (!hit) {
    unreadable++;
    console.log(`  UNREADABLE ${file}`);
    continue;
  }
  const parsed = parseFrame(new Uint8Array(hit.bytes));
  if (!parsed) {
    unreadable++;
    console.log(`  read but did not parse: ${file}`);
    continue;
  }
  if (!decoder) {
    const h = parsed.header;
    decoder = new LTDecoder(h.k, h.blockLen, h.sessionId, h.totalLen);
    decoder.fnv = h.payloadFnv;
  }
  seen.add(parsed.header.seq);
  decoder.addFrame(parsed.header.seq, parsed.block);
  if (decoder.isComplete) break;
}

console.log(`  ${files.length - unreadable}/${files.length} frames read by zxing`);
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
