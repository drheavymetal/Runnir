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
//
// A capture may hold SEVERAL codes: a runnir window wide enough puts a mosaic on
// screen, each tile a different frame of the same stream. They all come back
// from one scan, because the scan cost is the image, not the symbol count.

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

// How many symbols to look for is the CALLER's decision, because it is not free
// and the common case is one. Measured on a laptop, one 2400x1000 capture:
//
//   maxNumberOfSymbols=1   16.8 ms      =2   16.9 ms
//   maxNumberOfSymbols=4   24.9 ms      =16  26.2 ms
//
// A phone is several times slower than that, and every millisecond here is
// capture throughput — which IS transfer speed. So the page asks for one symbol
// normally and probes wider now and then; see `mosaic.ts`.
ctx.onmessage = async (e: MessageEvent) => {
  const { id, buf, bitmap, w, h, max } = e.data as {
    id: number;
    buf?: ArrayBuffer;
    bitmap?: ImageBitmap;
    w: number;
    h: number;
    max?: number;
  };
  try {
    // A bitmap arrives by TRANSFER, so the megabytes never crossed the main
    // thread: at 1920x1440 a capture is 11 MB, and copying that per frame in the
    // event loop was throttling the whole receiver. Drawing it here spreads the
    // cost across the pool instead. Older Safari has no OffscreenCanvas inside a
    // worker, so the page still sends raw pixels when that is all it can do.
    const img = bitmap ? drawToImageData(bitmap) : new ImageData(new Uint8ClampedArray(buf!), w, h);
    const results = await readBarcodes(img, {
      formats: ["QRCode"],
      maxNumberOfSymbols: Math.max(1, max ?? 1),
    });
    const symbols = results.filter((x) => x.isValid && x.bytes.length > 0).map((x) => x.bytes);
    ctx.postMessage({ id, symbols });
  } catch {
    ctx.postMessage({ id, symbols: [] });
  }
};

function drawToImageData(bitmap: ImageBitmap): ImageData {
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const c2d = canvas.getContext("2d", { willReadFrequently: true }) as OffscreenCanvasRenderingContext2D;
  c2d.drawImage(bitmap, 0, 0);
  const out = c2d.getImageData(0, 0, bitmap.width, bitmap.height);
  // Frees the backing memory now rather than at the next GC. At several frames a
  // second these are the largest allocations the receiver makes.
  bitmap.close();
  return out;
}

// Warm the WASM so the first real frame does not pay instantiation.
void readBarcodes(new ImageData(8, 8), { formats: ["QRCode"] })
  .catch(() => undefined)
  .then(() => ctx.postMessage({ id: -1, bytes: null }));
