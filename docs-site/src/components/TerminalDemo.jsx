import { useLang, UI } from '../i18n.jsx'

// Demos animados en CSS para funciones dinámicas que una captura estática no
// puede transmitir. Son maquetas que imitan el efecto real de runnir.
// Animated CSS demos for dynamic features a static shot can't convey.
export default function TerminalDemo({ kind }) {
  const { t } = useLang()
  return (
    <figure className="shot demo">
      <div className={`demo-term demo-${kind}`}>{render(kind, t)}</div>
      <figcaption>{t(UI.demoCaption)}</figcaption>
    </figure>
  )
}

function render(kind, t) {
  switch (kind) {
    case 'trail':
      return (
        <div className="d-body">
          <div className="d-line"><span className="c-p">~/proj &rsaquo;</span> deploy</div>
          <div className="d-trailwrap">
            <span className="d-ghost g1" /><span className="d-ghost g2" /><span className="d-ghost g3" />
            <span className="d-cursor moving" />
          </div>
          <div className="d-line c-d">{t({ es: 'el cursor deja una estela que se desvanece', en: 'the cursor leaves a fading trail' })}</div>
        </div>
      )
    case 'bell':
      return (
        <div className="d-body">
          <div className="d-flash" />
          <div className="d-line"><span className="c-p">~/proj &rsaquo;</span> make &amp;&amp; echo done</div>
          <div className="d-line c-g">Finished — <span className="c-d">{t({ es: 'BEL: el panel destella', en: 'BEL: the pane flashes' })}</span></div>
          <div className="d-line"><span className="c-cur">&#9608;</span></div>
        </div>
      )
    case 'smooth':
      return (
        <div className="d-body d-smoothwrap">
          <div className="d-scroller">
            {Array.from({ length: 14 }).map((_, i) => (
              <div className="d-line" key={i}><span className="c-d">{String(i + 1).padStart(2, '0')}</span> {t({ es: 'línea de salida', en: 'output line' })} {i + 1}</div>
            ))}
          </div>
          <div className="d-hint c-d">{t({ es: 'Ctrl+Shift+Home / End: la vista se desliza suave, no salta', en: 'Ctrl+Shift+Home / End: the view glides, it doesn’t jump' })}</div>
        </div>
      )
    case 'hover':
      return (
        <div className="d-body">
          <div className="d-line">clone: <span className="d-url">https://github.com/drheavymetal/Runnir</span></div>
          <div className="d-line">log: <span className="c-d">/var/log/</span><span className="d-path">deploy.log</span></div>
          <div className="d-hint c-d">{t({ es: 'al pasar el ratón se subraya; Ctrl+clic abre o copia', en: 'hover underlines; Ctrl+click opens or copies' })}</div>
        </div>
      )
    case 'gutter':
      return (
        <div className="d-body d-gutterwrap">
          <div className="d-grow"><span className="gut ok" /><span className="c-p">~/proj &rsaquo;</span> make</div>
          <div className="d-grow d-out c-d"><span className="gut cont" />  {t({ es: 'Compilando... ok', en: 'Compiling... ok' })}</div>
          <div className="d-grow"><span className="gut fail" /><span className="c-p">~/proj &rsaquo;</span> ./run</div>
          <div className="d-grow d-out c-d"><span className="gut cont" />  Segmentation fault</div>
          <div className="d-grow"><span className="gut run" /><span className="c-p">~/proj &rsaquo;</span> tail -f log</div>
          <div className="d-hint c-d">{t({ es: 'verde = código 0, rojo = fallo, tenue = en curso', en: 'green = exit 0, red = failed, dim = running' })}</div>
        </div>
      )
    case 'minimap':
      return (
        <div className="d-body d-minimapwrap">
          <div className="d-mmtext">
            {Array.from({ length: 12 }).map((_, i) => (
              <div className="d-line" key={i}><span className="c-d">{String(i + 1).padStart(2, '0')}</span> {t({ es: 'salida', en: 'output' })} {i + 1}</div>
            ))}
          </div>
          <div className="d-minimap">
            {Array.from({ length: 24 }).map((_, i) => (
              <span className={`mm-row ${i > 6 && i < 12 ? 'view' : ''}`} style={{ width: `${20 + ((i * 37) % 60)}%` }} key={i} />
            ))}
          </div>
        </div>
      )
    default:
      return null
  }
}
