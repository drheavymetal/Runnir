import Kbd from './Kbd.jsx'
import TerminalDemo from './TerminalDemo.jsx'
import { MEDIA, DEMOS } from '../data/media.js'

function slug(s) {
  return s.toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '')
    .replace(/[^a-z0-9]+/g, '-').replace(/(^-|-$)/g, '')
}

export default function FeatureCard({ f }) {
  const media = MEDIA[f.title]
  const demo = DEMOS[f.title]
  const hasTech = f.keys || f.palette || (f.config && f.config.length) || (f.escape && f.escape.length) || f.example
  return (
    <article className="card" id={slug(f.title)}>
      <div className="card-head">
        <h3>{f.title}</h3>
        <span className={`badge ${f.status === 'dev' ? 'dev' : 'shipped'}`}>
          {f.status === 'dev' ? 'En desarrollo' : 'Disponible'}
        </span>
      </div>

      <p className="natural">{f.natural}</p>

      {media && (
        <figure className="shot">
          <img src={media.src} alt={f.title} loading="lazy" />
          <figcaption>{media.cap}</figcaption>
        </figure>
      )}

      {demo && <TerminalDemo kind={demo} />}

      {hasTech && (
        <div className="tech">
          {f.keys && (
            <div className="tech-row">
              <div className="tech-label">Atajo</div>
              <div className="tech-val">
                {f.keys.map((k, i) => <Kbd key={i} text={k} />)}
              </div>
            </div>
          )}
          {f.palette && (
            <div className="tech-row">
              <div className="tech-label">Paleta</div>
              <div className="tech-val">
                {f.palette.split(' / ').map((p, i) => <span className="pcmd" key={i}>{p}</span>)}
              </div>
            </div>
          )}
          {f.config && f.config.length > 0 && (
            <div className="tech-row">
              <div className="tech-label">Config</div>
              <div className="tech-val">
                {f.config.map((c, i) => (
                  <span className="line" key={i}>
                    <span className="cfg-key">{c.k}</span>
                    {c.v && <> = <span className="cfg-def">{c.v}</span></>}
                    <span className="cfg-desc"> — {c.d}</span>
                  </span>
                ))}
              </div>
            </div>
          )}
          {f.escape && f.escape.length > 0 && (
            <div className="tech-row">
              <div className="tech-label">Escape</div>
              <div className="tech-val">
                {f.escape.map((e, i) => <code className="esc" key={i}>{e}</code>)}
              </div>
            </div>
          )}
          {f.example && (
            <div className="tech-row">
              <div className="tech-label">Ejemplo</div>
              <div className="tech-val">
                <pre className="example">{f.example}</pre>
              </div>
            </div>
          )}
        </div>
      )}

      {f.note && <p className="note"><b>Nota:</b> {f.note}</p>}
    </article>
  )
}

export { slug }
