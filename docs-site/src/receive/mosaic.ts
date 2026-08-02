// Several codes out of one capture, without touching the vendored pool.
//
// A runnir window wide enough shows a MOSAIC: two or more QR codes side by side,
// each carrying a different frame of the same stream. One camera capture holds
// all of them, which is the only way past the frame-rate ceiling — a phone
// delivers 30 or 60 captures a second no matter how fast the screen changes.
//
// `vendor/worker-pool.ts` is decimen's, verbatim, and its contract is one decode
// per submitted frame: the worker answers with `bytes`, the pool frees the slot
// and hands them on. Rather than edit a vendored file to widen that contract,
// this wraps a worker in something the pool still recognises — the interface is
// structural, so an object with the three members is a worker as far as it is
// concerned. Extra symbols go straight to the same sink the pool would have used;
// the first one travels the normal path, which is what releases the slot.
//
// The upstream file therefore stays byte-identical and can be re-vendored at any
// time without replaying this change.

import type { PoolWorker } from './vendor/worker-pool.ts'

interface MosaicMessage {
  id: number
  symbols?: Uint8Array[]
}

/** Symbols to ask for on a probe capture, and how often to spend one. */
export const PROBE_SYMBOLS = 4
export const PROBE_EVERY = 12

/**
 * How many symbols the next capture should ask zxing for.
 *
 * One, until a mosaic is actually seen. Looking for more costs real time — 25 ms
 * against 17 ms on a laptop for a capture with two codes in it, and a phone is
 * several times slower — and that time is capture throughput, which is transfer
 * speed. So the common case pays nothing, and a mosaic is FOUND rather than
 * assumed: every twelfth capture looks wider, and once more than one code comes
 * back the receiver keeps asking for that many.
 *
 * A probe that costs one slow capture in twelve is cheap; guessing wrong in
 * either direction for the whole transfer is not.
 */
export function symbolsToRequest(found: number, frameId: number): number {
  if (found > 1) return found
  return frameId % PROBE_EVERY === 0 ? PROBE_SYMBOLS : 1
}

/**
 * Wrap a decode worker so every code in a capture reaches `onDecoded`.
 *
 * @param spawn creates the underlying worker
 * @param onDecoded receives every symbol beyond the first; the first goes
 *   through the pool so its slot accounting stays exactly as vendored
 * @param seen latched high-water mark of codes seen in one capture, which is
 *   what {@link symbolsToRequest} reads to stop probing
 */
export function mosaicWorker(
  spawn: () => Worker,
  onDecoded: (bytes: Uint8Array) => void,
  seen: { current: number },
): PoolWorker {
  const real = spawn()
  const adapter: PoolWorker = {
    onmessage: null,
    postMessage: (message: unknown, transfer: Transferable[]) => real.postMessage(message, transfer),
    terminate: () => real.terminate(),
  }
  real.onmessage = (event: MessageEvent) => {
    const { id, symbols } = event.data as MosaicMessage
    const list = symbols ?? []
    if (list.length > seen.current) seen.current = list.length
    // Extras first, then the one that frees the slot: by the time the pool can
    // hand this worker another capture, everything from this one is already in
    // the decoder.
    for (let i = 1; i < list.length; i++) onDecoded(list[i]!)
    adapter.onmessage?.({ data: { id, bytes: list[0] ?? null } } as MessageEvent)
  }
  return adapter
}
