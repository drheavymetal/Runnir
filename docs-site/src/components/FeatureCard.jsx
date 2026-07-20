import Kbd from './Kbd.jsx'
import TerminalDemo from './TerminalDemo.jsx'
import { MEDIA, DEMOS } from '../data/media.js'
import { useLang, UI } from '../i18n.jsx'

export default function FeatureCard({ f }) {
  const { t } = useLang()
  // Una feature puede traer una captura o varias (la capa leader necesita ver el
  // nivel raíz y un grupo para entenderse). / One shot or several.
  const media = [MEDIA[f.key]].flat().filter(Boolean)
  const demo = DEMOS[f.key]
  const hasTech = f.keys || f.palette || (f.config && f.config.length) || (f.escape && f.escape.length) || f.example
  return (
    <article className="card" id={f.key}>
      <div className="card-head">
        <h3>{t(f.title)}</h3>
        <span className={`badge ${f.status === 'dev' ? 'dev' : 'shipped'}`}>
          {f.status === 'dev' ? t(UI.badgeDev) : t(UI.badgeShipped)}
        </span>
      </div>

      <p className="natural">{t(f.natural)}</p>

      {media.map((m, i) => (
        <figure className="shot" key={i}>
          <img src={m.src} alt={t(f.title)} loading="lazy" />
          <figcaption>{t(m.cap)}</figcaption>
        </figure>
      ))}

      {demo && <TerminalDemo kind={demo} />}

      {hasTech && (
        <div className="tech">
          {f.keys && (
            <div className="tech-row">
              <div className="tech-label">{t(UI.techShortcut)}</div>
              <div className="tech-val">
                {f.keys.map((k, i) => <Kbd key={i} text={t(k)} />)}
              </div>
            </div>
          )}
          {f.palette && (
            <div className="tech-row">
              <div className="tech-label">{t(UI.techPalette)}</div>
              <div className="tech-val">
                {f.palette.split(' / ').map((p, i) => <span className="pcmd" key={i}>{p}</span>)}
              </div>
            </div>
          )}
          {f.config && f.config.length > 0 && (
            <div className="tech-row">
              <div className="tech-label">{t(UI.techConfig)}</div>
              <div className="tech-val">
                {f.config.map((c, i) => (
                  <span className="line" key={i}>
                    <span className="cfg-key">{c.k}</span>
                    {c.v && <> = <span className="cfg-def">{c.v}</span></>}
                    <span className="cfg-desc"> — {t(c.d)}</span>
                  </span>
                ))}
              </div>
            </div>
          )}
          {f.escape && f.escape.length > 0 && (
            <div className="tech-row">
              <div className="tech-label">{t(UI.techEscape)}</div>
              <div className="tech-val">
                {f.escape.map((e, i) => <code className="esc" key={i}>{e}</code>)}
              </div>
            </div>
          )}
          {f.example && (
            <div className="tech-row">
              <div className="tech-label">{t(UI.techExample)}</div>
              <div className="tech-val">
                <pre className="example">{f.example}</pre>
              </div>
            </div>
          )}
        </div>
      )}

      {f.note && <p className="note"><b>{t(UI.noteLabel)}:</b> {t(f.note)}</p>}
    </article>
  )
}
