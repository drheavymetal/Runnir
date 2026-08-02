// QR decode worker: zxing-cpp compiled to WASM.
//
// Adapted from decimen-optical-transfer (MIT) — the only change is that the
// wasm URL is imported here rather than through a build-swapped module, since
// this site has no standalone build to swap for.
//
// WASM rather than the browser's own BarcodeDetector because Safari has never
// shipped it (WebKit bug 281848), and a receiver that does not work on an
// iPhone is not a receiver.
//
// One frame in flight per worker; the page drops frames when all workers are
// busy. Frames are disposable — the fountain does not care.

import { prepareZXingModule, readBarcodes } from "zxing-wasm/reader";
import wasmUrl from "zxing-wasm/reader/zxing_reader.wasm?url";

prepareZXingModule({
  overrides: {
    locateFile: (path: string, prefix: string) =>
      path.endsWith(".wasm") ? wasmUrl : prefix + path,
  },
});

const ctx = self as unknown as {
  onmessage: ((e: MessageEvent) => void) | null;
  postMessage(msg: unknown, transfer?: Transferable[]): void;
};

ctx.onmessage = async (e: MessageEvent) => {
  const { id, buf, w, h } = e.data as { id: number; buf: ArrayBuffer; w: number; h: number };
  try {
    const img = new ImageData(new Uint8ClampedArray(buf), w, h);
    const results = await readBarcodes(img, { formats: ["QRCode"], maxNumberOfSymbols: 1 });
    const r = results.find((x) => x.isValid && x.bytes.length > 0);
    ctx.postMessage({ id, bytes: r ? r.bytes : null });
  } catch {
    ctx.postMessage({ id, bytes: null });
  }
};

// Warm the WASM so the first real frame does not pay instantiation.
void readBarcodes(new ImageData(8, 8), { formats: ["QRCode"] })
  .catch(() => undefined)
  .then(() => ctx.postMessage({ id: -1, bytes: null }));
