// Cross-language proof: frames produced by runnir's Rust sender, decoded by
// decimen's own JavaScript receiver. If this passes, a runnir sender is
// decodable by the receiver that ships on runnir's website — and by every
// standalone decimen receiver anyone has already saved.
//
// Usage: node cross-check.mjs /path/to/vectors.json

import { readFileSync } from "node:fs";
import { LTDecoder } from "../src/receive/vendor/fountain.ts";
import { fnv1a, parseFrame, unpackFile } from "../src/receive/vendor/protocol.ts";

const cases = JSON.parse(readFileSync(process.argv[2], "utf8"));
let failed = 0;

function check(label, condition, detail = "") {
  if (condition) {
    console.log(`  ok   ${label}`);
  } else {
    console.log(`  FAIL ${label} ${detail}`);
    failed++;
  }
}

for (const c of cases) {
  console.log(`\n${c.name} — k=${c.k}, ${c.blockLen} B/frame, session ${c.sessionId}, ${c.compression}`);

  // Parse every frame with the JS parser, never with knowledge from the vector
  // file: the header has to be self-describing or the receiver cannot lock on.
  const parsed = c.frames.map((hex) =>
    parseFrame(Uint8Array.from(hex.match(/../g).map((b) => parseInt(b, 16)))),
  );
  check("every frame parses", parsed.every((p) => p !== null));

  const first = parsed[0].header;
  check("the header describes the stream", first.k === c.k && first.blockLen === c.blockLen &&
    first.totalLen === c.totalLen && first.sessionId === c.sessionId &&
    first.payloadFnv === c.payloadFnv, JSON.stringify(first));

  // Feed them back to front and skip every third, because a camera drops frames
  // and never sees them in order.
  const decoder = new LTDecoder(first.k, first.blockLen, first.sessionId, first.totalLen);
  const shuffled = [...parsed].reverse().filter((_, i) => i % 3 !== 0);
  for (const p of shuffled) {
    decoder.addFrame(p.header.seq, p.block);
    if (decoder.isComplete) break;
  }
  check(`decodes out of order with a third dropped (${decoder.framesNew} frames used)`, decoder.isComplete);
  if (!decoder.isComplete) continue;

  const container = decoder.assemble();
  check("the container checksum matches the header", fnv1a(container) === c.payloadFnv);

  const file = await unpackFile(container);
  check("the name survives", file.name === c.name, `got ${file.name}`);
  check("the media type survives", file.type === c.mediaType, `got ${file.type}`);
  check("the compression flag survives", file.compression === c.compression, `got ${file.compression}`);
  check("the length survives", file.bytes.length === c.originalSize, `got ${file.bytes.length}`);

  const sha = Buffer.from(
    await crypto.subtle.digest("SHA-256", Uint8Array.from(file.bytes)),
  ).toString("hex");
  check("the recovered bytes hash to what the sender promised", sha === c.sha256, `got ${sha}`);

  // The same six digits runnir shows beside the QR, derived on this side from
  // the bytes that actually arrived.
  const head = parseInt(sha.slice(0, 8), 16);
  const code = String(head % 1000000).padStart(6, "0");
  check(`the verification code agrees (${code})`, code === c.code, `sender said ${c.code}`);
}

console.log(failed === 0 ? "\nALL PASS — the Rust sender is decodable by the JS receiver" : `\n${failed} FAILED`);
process.exit(failed === 0 ? 0 : 1);
