// El receptor óptico: cámara → decodificación QR en workers → fountain → fichero.
// The optical receiver: camera → QR decode in workers → fountain → file.
//
// La lógica está vendorizada de decimen-optical-transfer (MIT) sin tocar una
// línea, porque es formato de cable: ver src/receive/vendor/. Lo que se
// reescribe aquí es sólo la presentación.
//
// The logic is vendored verbatim from decimen-optical-transfer (MIT) because it
// is wire format: see src/receive/vendor/. Only the presentation is rewritten.

import { useCallback, useEffect, useRef, useState } from 'react'
import { LTDecoder } from '../receive/vendor/fountain.ts'
import { fnv1a, parseFrame, streamIdentity, unpackFile, verifyFile } from '../receive/vendor/protocol.ts'
import { isSnippet, snippetText } from '../receive/vendor/snippet.ts'
import { estimateTransferProgress, expectedFountainOverhead, formatDuration } from '../receive/vendor/progress.ts'
import { NoSignalHintTimer } from '../receive/vendor/no-signal.ts'
import { DecodeWorkerPool } from '../receive/vendor/worker-pool.ts'
import { mosaicWorker, symbolsToRequest } from '../receive/mosaic.ts'
import { useLang } from '../i18n.jsx'

// Diez segundos de cámara sin un solo frame decodificado: el emisor casi seguro
// va demasiado denso para esta cámara. / Ten seconds of camera and not one
// decoded frame: the sender is almost certainly too dense for this camera.
const NO_SIGNAL_AFTER_MS = 10_000
const STATS_WINDOW_MS = 2000
const DEFAULT_WORKERS = Math.min(4, Math.max(1, (navigator.hardwareConcurrency || 4) - 1))

const T = {
  title: { es: 'Recibir por la cámara', en: 'Receive with the camera' },
  lede: {
    es: 'Apunta la cámara de este dispositivo a una ventana de runnir que esté emitiendo (leader q). El fichero llega por la luz: no hay red entre los dos, ni emparejamiento, ni cuenta.',
    en: 'Point this device’s camera at a runnir window that is sending (leader q). The file arrives through light: no network between the two, no pairing, no account.',
  },
  start: { es: 'Encender la cámara', en: 'Start camera' },
  starting: { es: 'Encendiendo…', en: 'Starting…' },
  insecure: {
    es: 'La cámara necesita un contexto seguro: esta página tiene que servirse por https.',
    en: 'The camera needs a secure context: this page must be served over https.',
  },
  denied: {
    es: 'Permiso de cámara denegado. Concédelo y vuelve a pulsar.',
    en: 'Camera permission denied. Allow it, then press again.',
  },
  searching: { es: 'buscando una emisión…', en: 'searching for a stream…' },
  verified: { es: 'SHA-256 verificado', en: 'SHA-256 verified' },
  save: { es: 'Guardar', en: 'Save' },
  again: { es: 'Recibir otro', en: 'Receive another' },
  retry: { es: 'Reintentar', en: 'Try again' },
  textGot: { es: 'Texto recibido', en: 'Text received' },
  copy: { es: 'Copiar', en: 'Copy' },
  copied: { es: 'Copiado', en: 'Copied' },
  done: { es: 'Transferencia completa', en: 'Transfer complete' },
  failed: { es: 'La transferencia falló', en: 'Transfer failed' },
  failedNote: {
    es: 'De esa emisión no salió nada aprovechable. Reinicia el emisor y vuelve a apuntar: una transferencia a medias no cuesta más que el tiempo.',
    en: 'Nothing usable came out of that stream. Restart the sender and point again — a partial transfer costs nothing but the time.',
  },
  badSum: { es: 'La suma de comprobación de la emisión no cuadra.', en: 'The optical stream checksum did not match.' },
  badHash: { es: 'El fichero recuperado no pasó la verificación SHA-256.', en: 'The recovered file failed SHA-256 verification.' },
  noSignalTitle: { es: 'Nada decodificado todavía — prueba esto', en: 'Nothing decoded yet — try this' },
  noSignal: {
    es: [
      'Acerca el móvil hasta que el código llene la pantalla, y apóyalo en algo: el autofoco persiguiendo el pulso es la causa habitual.',
      'Sube al máximo el brillo de la pantalla que emite.',
      'En el emisor, baja los fps: cada frame se queda más tiempo en pantalla.',
      'Si la ventana de runnir es pequeña, agrándala: lo que importa es el tamaño del código, no el del fichero.',
    ],
    en: [
      'Move closer until the code fills the view, and prop the phone against something — autofocus hunting from hand tremor is the usual culprit.',
      'Turn the sending screen’s brightness all the way up.',
      'Drop the sender’s fps: each frame then stays on screen longer.',
      'If the runnir window is small, make it bigger — what matters is the size of the code, not of the file.',
    ],
  },
  dismiss: { es: 'Cerrar', en: 'Dismiss' },
  blocks: { es: 'bloques', en: 'blocks' },
  frames: { es: 'frames', en: 'frames' },
  estimating: { es: 'Estimando…', en: 'Estimating…' },
  decoding: { es: 'decodificando', en: 'decoding' },
  about: { es: 'Faltan unos', en: 'About' },
  total: { es: 'en total', en: 'total' },
  code: { es: 'código', en: 'code' },
  codeNote: {
    es: 'Los mismos seis dígitos que runnir enseña junto al código. La verificación no depende de que los mires: el SHA-256 completo se comprueba solo.',
    en: 'The same six digits runnir shows beside the code. Verification does not depend on you checking them: the full SHA-256 is verified automatically.',
  },
  camera: { es: 'cámara', en: 'camera' },
  workers: { es: 'workers', en: 'workers' },
}

/** Los seis dígitos, derivados de los bytes que realmente llegaron. */
async function verificationCode(bytes) {
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', Uint8Array.from(bytes)))
  const head = ((digest[0] << 24) >>> 0) + (digest[1] << 16) + (digest[2] << 8) + digest[3]
  return String(head % 1000000).padStart(6, '0')
}

export default function ReceivePage() {
  const { t, lang } = useLang()
  const videoRef = useRef(null)
  const canvasRef = useRef(null)

  const [phase, setPhase] = useState('idle') // idle | starting | live | done | failed
  const [error, setError] = useState('')
  const [status, setStatus] = useState('')
  const [camera, setCamera] = useState('')
  const [progress, setProgress] = useState(null)
  const [result, setResult] = useState(null)
  const [noSignal, setNoSignal] = useState(false)
  const [copied, setCopied] = useState(false)

  // Todo lo que no debe provocar un re-render vive en refs: esto corre a la
  // velocidad de la cámara. / Anything that must not trigger a re-render lives
  // in refs: this runs at camera speed.
  const state = useRef({
    stream: null,
    pool: null,
    // Cuántos códigos ha traído la mejor captura hasta ahora: 1 hasta que se vea
    // un mosaico de verdad. Es un objeto para que el worker envuelto y el bucle
    // de captura miren el mismo valor sin re-crear el pool.
    symbolsSeen: { current: 1 },
    // Sacar los píxeles del hilo principal necesita las dos cosas: bitmaps aquí
    // y OffscreenCanvas dentro del worker. Safari viejo no tiene la segunda.
    canBitmap:
      typeof createImageBitmap === 'function' && typeof OffscreenCanvas === 'function',
    decoder: null,
    streamKey: '',
    startTs: 0,
    gen: 0,
    done: false,
    frameId: 0,
    captureTimes: [],
    decodeTimes: [],
    // Capturas tiradas porque todos los workers estaban ocupados. Sin este
    // número, "30 capturas y 7 lecturas" es ambiguo entre 23 ilegibles y 23 ni
    // intentadas, y los dos casos piden arreglos opuestos.
    dropTimes: [],
    noSignal: new NoSignalHintTimer(NO_SIGNAL_AFTER_MS),
    statsTimer: undefined,
  })

  const teardown = useCallback(() => {
    const s = state.current
    s.gen++
    s.stream?.getTracks().forEach((track) => track.stop())
    s.stream = null
    clearInterval(s.statsTimer)
    s.statsTimer = undefined
    // Cada worker lleva su propia instancia WASM de ~940 KB: en un móvil merece
    // la pena soltarla en cuanto entra el último frame.
    s.pool?.resize(0)
  }, [])

  useEffect(() => teardown, [teardown])

  const finish = useCallback(async (container, hashOk, seconds) => {
    const s = state.current
    s.done = true
    teardown()
    try {
      if (!hashOk) throw new Error(t(T.badSum))
      const file = await unpackFile(container)
      if (!(await verifyFile(file))) throw new Error(t(T.badHash))
      const code = await verificationCode(file.bytes)
      const rate = (container.length / 1024 / seconds).toFixed(1)
      if (isSnippet(file)) {
        setResult({ kind: 'text', text: snippetText(file), seconds, rate, code, gzip: file.compression === 'gzip' })
      } else {
        const url = URL.createObjectURL(new Blob([file.bytes], { type: file.type }))
        setResult({
          kind: 'file',
          name: file.name,
          type: file.type,
          size: file.bytes.length,
          url,
          seconds,
          rate,
          code,
          gzip: file.compression === 'gzip',
        })
      }
      setPhase('done')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      setPhase('failed')
    }
  }, [t, teardown])

  const onDecoded = useCallback((bytes) => {
    const s = state.current
    s.decodeTimes.push(performance.now())
    const parsed = parseFrame(bytes)
    if (!parsed || s.done) return
    const { header, block } = parsed
    if (s.noSignal.frameDecoded()) setNoSignal(false)
    // streamIdentity cubre TODOS los campos que deben mantenerse constantes, no
    // sólo el id de sesión: dos emisiones distintas metidas en el mismo
    // decodificador lo corrompen en silencio.
    const identity = streamIdentity(header)
    if (!s.decoder || s.streamKey !== identity) {
      s.decoder = new LTDecoder(header.k, header.blockLen, header.sessionId, header.totalLen)
      s.streamKey = identity
      s.startTs = performance.now()
    }
    s.decoder.addFrame(header.seq, block)
    if (s.decoder.isComplete) {
      const payload = s.decoder.assemble()
      const seconds = (performance.now() - s.startTs) / 1000
      void finish(payload, fnv1a(payload) === header.payloadFnv, seconds)
    }
  }, [finish])

  const captureFrame = useCallback(() => {
    const s = state.current
    const video = videoRef.current
    if (!video) return
    const vw = video.videoWidth
    const vh = video.videoHeight
    if (!vw || !vh) return
    s.captureTimes.push(performance.now())
    // Todos los workers ocupados: se tira el frame. El fountain lo absorbe como
    // cualquier otro fallo, y un frame viejo vale menos que el siguiente.
    if (s.pool.busyCount === s.pool.size) {
      s.dropTimes.push(performance.now())
      return
    }
    // Buscar más de un símbolo cuesta tiempo real, y ese tiempo ES la velocidad
    // de la transferencia. Se pide uno salvo que ya se haya visto un mosaico, y
    // una captura de cada doce mira más ancho por si lo hay.
    const max = symbolsToRequest(s.symbolsSeen.current, s.frameId)
    const id = s.frameId++

    // Los píxeles NO pasan por el hilo principal cuando el navegador sabe hacer
    // bitmaps: a 1920x1440 una captura son 11 MB, y copiarlos aquí en cada frame
    // estrangulaba el bucle de captura entero — que es el techo real de la
    // transferencia. `createImageBitmap` no copia, y el bitmap viaja al worker
    // por transferencia, así que el dibujado y la lectura los paga el pool.
    if (s.canBitmap) {
      createImageBitmap(video).then(
        (bitmap) => {
          if (!s.pool || !s.pool.submit({ id, bitmap, w: vw, h: vh, max }, [bitmap])) bitmap.close()
        },
        () => {
          // Un navegador que dice que sabe y luego falla: se vuelve al camino
          // lento en vez de dejar de recibir.
          s.canBitmap = false
        },
      )
      return
    }

    const grab = canvasRef.current
    if (grab.width !== vw || grab.height !== vh) {
      grab.width = vw
      grab.height = vh
    }
    const ctx = grab.getContext('2d', { willReadFrequently: true })
    ctx.drawImage(video, 0, 0)
    const img = ctx.getImageData(0, 0, vw, vh)
    s.pool.submit({ id, buf: img.data.buffer, w: vw, h: vh, max }, [img.data.buffer])
  }, [])

  const scheduleFrame = useCallback((gen) => {
    const s = state.current
    if (s.done || gen !== s.gen) return
    const video = videoRef.current
    const next = () => {
      // Las cadenas de requestVideoFrameCallback sobreviven a un stream parado y
      // reviven con el siguiente: el contador de generación mata al zombi.
      if (state.current.done || gen !== state.current.gen) return
      captureFrame()
      scheduleFrame(gen)
    }
    if (video?.requestVideoFrameCallback) video.requestVideoFrameCallback(next)
    else requestAnimationFrame(next)
  }, [captureFrame])

  const tickStats = useCallback(() => {
    const s = state.current
    if (s.done) return
    const now = performance.now()
    const prune = (a) => { while (a.length > 0 && a[0] < now - STATS_WINDOW_MS) a.shift() }
    prune(s.captureTimes)
    prune(s.decodeTimes)
    prune(s.dropTimes)
    if (s.noSignal.tick(now)) setNoSignal(true)
    if (!s.decoder) {
      setProgress(null)
      return
    }
    const elapsed = Math.max(0, (now - s.startTs) / 1000)
    // El progreso sigue los frames RECOGIDOS, no los bloques resueltos: la
    // cascada del peeling se acumula al final, así que una barra por bloques
    // parece atascada y luego teletransporta.
    const estimate = estimateTransferProgress(s.decoder.k, s.decoder.framesNew, elapsed, s.decoder.solvedCount)
    const goodput =
      (s.decoder.framesNew * s.decoder.blockLen) /
      expectedFountainOverhead(s.decoder.k) / 1024 / Math.max(0.1, elapsed)
    setProgress({
      percent: estimate.fraction * 100,
      eta: estimate.etaSeconds,
      phase: estimate.phase,
      solved: s.decoder.solvedCount,
      k: s.decoder.k,
      frames: s.decoder.framesNew,
      dup: s.decoder.framesDup,
      goodput,
      elapsed,
      payload: s.decoder.totalLen,
      capture: s.captureTimes.length / (STATS_WINDOW_MS / 1000),
      decode: s.decodeTimes.length / (STATS_WINDOW_MS / 1000),
      drop: s.dropTimes.length / (STATS_WINDOW_MS / 1000),
    })
  }, [])

  const start = useCallback(async () => {
    const s = state.current
    setError('')
    // En un origen inseguro la API no existe SIQUIERA: es el caso de http por
    // la LAN. localhost está exento; cualquier otro host necesita https.
    if (!navigator.mediaDevices?.getUserMedia) {
      setError(t(T.insecure))
      return
    }
    setPhase('starting')
    // Resolución: la palanca más grande del receptor. Un símbolo V40 son 185
    // módulos con zona tranquila, así que a 960 px de alto y con el código
    // ocupando media pantalla salen ~2,5 px por módulo — justo donde zxing
    // empieza a fallar, y un fallo aquí es una captura entera tirada. Pedir
    // 1920x1440 dobla los píxeles por módulo con el mismo encuadre. `ideal`
    // degrada solo en la cámara que no llegue.
    const base = { facingMode: 'environment', width: { ideal: 1920 }, height: { ideal: 1440 } }
    try {
      try {
        // iOS trata `ideal` como una sugerencia y entrega 30. Pedir `exact`
        // primero, y caer a `ideal` en lo que lo rechace.
        s.stream = await navigator.mediaDevices.getUserMedia({ audio: false, video: { ...base, frameRate: { exact: 60 } } })
      } catch {
        s.stream = await navigator.mediaDevices.getUserMedia({ audio: false, video: { ...base, frameRate: { ideal: 60 } } })
      }
    } catch (err) {
      const denied = err instanceof DOMException && err.name === 'NotAllowedError'
      setError(denied ? t(T.denied) : `${t(T.camera)}: ${err instanceof Error ? err.message : String(err)}`)
      setPhase('idle')
      return
    }

    const video = videoRef.current
    video.srcObject = s.stream
    await video.play().catch(() => undefined)
    // Enfoque continuo cuando el navegador lo deje: el motivo número uno de una
    // captura ilegible es el autofocus buscando por el temblor de la mano, y a
    // 30 capturas por segundo cada rebusca se lleva decenas de frames.
    try {
      const track = s.stream.getVideoTracks()[0]
      if (track?.getCapabilities?.().focusMode?.includes('continuous')) {
        await track.applyConstraints({ advanced: [{ focusMode: 'continuous' }] })
      }
    } catch {
      // No en todos los navegadores, y una restricción rechazada no es motivo
      // para no recibir.
    }
    const settings = s.stream.getVideoTracks()[0]?.getSettings()
    setCamera(`${settings?.width}×${settings?.height} @ ${Math.round(settings?.frameRate ?? 0)} fps · ${DEFAULT_WORKERS} ${t(T.workers)}`)
    setStatus(t(T.searching))

    if (!s.pool) {
      s.pool = new DecodeWorkerPool(
        () =>
          mosaicWorker(
            () => new Worker(new URL('../receive/worker.ts', import.meta.url), { type: 'module' }),
            onDecoded,
            s.symbolsSeen,
          ),
        onDecoded,
      )
    }
    s.pool.resize(DEFAULT_WORKERS)
    s.done = false
    s.noSignal.cameraStarted(performance.now())
    s.gen++
    scheduleFrame(s.gen)
    s.statsTimer = setInterval(tickStats, 500)
    setPhase('live')
    // Una pantalla que se apaga a mitad de transferencia mata la cámara.
    try {
      await navigator.wakeLock?.request('screen')
    } catch {
      // No en todos los navegadores, y no vale la pena fallar por ello.
    }
  }, [onDecoded, scheduleFrame, t, tickStats])

  const restart = () => window.location.reload()

  return (
    <div className="wrap receive">
      <h1>{t(T.title)}</h1>
      <p className="blurb">{t(T.lede)}</p>

      {phase === 'idle' && (
        <button className="receive-start" onClick={start}>{t(T.start)}</button>
      )}
      {phase === 'starting' && <button className="receive-start" disabled>{t(T.starting)}</button>}

      {error && <p className="receive-error">{error}</p>}

      <div className="receive-stage" style={{ display: phase === 'live' ? 'block' : 'none' }}>
        <video ref={videoRef} className="receive-video" playsInline muted />
        <p className="receive-camera">{camera} · {status}</p>
      </div>
      <canvas ref={canvasRef} style={{ display: 'none' }} />

      {phase === 'live' && progress && (
        <div className="receive-progress">
          {/* Los dos números que se miran mientras sostienes el móvil, y por eso
              van primero y grandes: cuánto queda, y si mover el móvil mejora la
              velocidad. Enterrados en una línea de detalle no se leen a un
              brazo de distancia. */}
          <div className="receive-headline">
            <span className="receive-pct">
              {progress.percent < 10 ? progress.percent.toFixed(1) : progress.percent.toFixed(0)}
              <span className="receive-unit">%</span>
            </span>
            <span className="receive-rate">
              {progress.frames >= 4 ? progress.goodput.toFixed(1) : '—'}
              <span className="receive-unit">KB/s</span>
            </span>
          </div>
          <div className="receive-bar"><span style={{ width: `${progress.percent.toFixed(1)}%` }} /></div>
          <div className="receive-progress-row">
            <span>{progress.solved}/{progress.k} {t(T.blocks)}</span>
            <span>
              {progress.eta === undefined
                ? progress.phase === 'decoding'
                  ? `${progress.frames} ${t(T.frames)} · ${t(T.decoding)}`
                  : t(T.estimating)
                : `${t(T.about)} ${formatDuration(progress.eta)} · ${progress.frames} ${t(T.frames)}`}
            </span>
          </div>
          <p className="receive-metrics">
            {progress.capture.toFixed(0)} cap · {progress.decode.toFixed(1)} dec ·{' '}
            {progress.drop.toFixed(0)} drop · {progress.frames}/{progress.dup} · k={progress.k} ·{' '}
            {Math.round(progress.payload / 1024)} KB
          </p>
        </div>
      )}

      {noSignal && phase === 'live' && (
        <div className="receive-hint" role="status">
          <strong>{t(T.noSignalTitle)}</strong>
          <ul>{T.noSignal[lang].map((line) => <li key={line}>{line}</li>)}</ul>
          <button className="receive-text-button" onClick={() => { state.current.noSignal.dismiss(performance.now()); setNoSignal(false) }}>
            {t(T.dismiss)}
          </button>
        </div>
      )}

      {phase === 'done' && result && (
        <div className="receive-result">
          <div className="receive-done">{result.kind === 'text' ? t(T.textGot) : t(T.done)}</div>
          <p className="receive-summary">
            {result.kind === 'file' && `${Math.round(result.size / 1024)} KB · `}
            {result.seconds.toFixed(1)} s · {result.rate} KB/s
            {result.gzip && ' · gzip'} · {t(T.verified)} ✓
          </p>
          <p className="receive-code">{t(T.code)} <strong>{result.code}</strong></p>
          <p className="receive-code-note">{t(T.codeNote)}</p>

          {result.kind === 'text' ? (
            <>
              <pre className="receive-snippet">{result.text}</pre>
              <div className="receive-actions">
                <button
                  className="receive-text-button"
                  onClick={async () => {
                    try {
                      await navigator.clipboard.writeText(result.text)
                      setCopied(true)
                      setTimeout(() => setCopied(false), 1500)
                    } catch { /* nada que hacer si el portapapeles está bloqueado */ }
                  }}
                >{copied ? t(T.copied) : t(T.copy)}</button>
                <button className="receive-text-button" onClick={restart}>{t(T.again)}</button>
              </div>
            </>
          ) : (
            <>
              <div className="receive-actions">
                <a className="receive-download" href={result.url} download={result.name}>{t(T.save)} {result.name}</a>
                <button className="receive-text-button" onClick={restart}>{t(T.again)}</button>
              </div>
              {result.type.startsWith('image/') && (
                <img className="receive-preview" src={result.url} alt={result.name} />
              )}
            </>
          )}
        </div>
      )}

      {phase === 'failed' && (
        <div className="receive-result">
          <div className="receive-failed">{t(T.failed)}</div>
          <p className="receive-summary">{t(T.failedNote)}</p>
          <div className="receive-actions">
            <button className="receive-text-button" onClick={restart}>{t(T.retry)}</button>
          </div>
        </div>
      )}
    </div>
  )
}
