import { useLang, UI } from '../i18n.jsx'

export default function Hero() {
  const { t } = useLang()
  return (
    <header className="hero">
      <div className="hero-top">
        <img className="hero-logo" src="./logo-t.png" alt="runnir — terminal" />
        <div className="hero-intro">
          <h1 className="sr-only">runnir</h1>
          <p className="hero-tag">{t(UI.heroTag)}</p>
          <div className="hero-meta">
            {UI.heroPills.map((p, i) => <span className="pill" key={i}>{t(p)}</span>)}
          </div>
        </div>
      </div>

      <div className="term">
        <div className="term-bar">
          <span className="term-dot" style={{ background: '#f14c4c' }} />
          <span className="term-dot" style={{ background: '#e5b510' }} />
          <span className="term-dot" style={{ background: '#0dbc79' }} />
          <span className="term-title">runnir — ~/projects/runnir</span>
        </div>
        <div className="term-body">
          <div><span className="c-p">~/projects/runnir</span> <span className="c-g">&rsaquo;</span> cargo run</div>
          <div>   <span className="c-d">Compiling runnir v0.1.0</span></div>
          <div>    <span className="c-g">Finished</span> in 2.41s</div>
          <div><span className="c-p">~/projects/runnir</span> <span className="c-g">&rsaquo;</span> <span className="c-a">runnir --quake</span>   <span className="c-d">{t(UI.heroComment1)}</span></div>
          <div><span className="c-p">~/projects/runnir</span> <span className="c-g">&rsaquo;</span> <span className="c-a">Ctrl+Shift+P</span>  <span className="c-d">&rarr; {t(UI.heroComment2)}</span></div>
          <div><span className="c-p">~/projects/runnir</span> <span className="c-g">&rsaquo;</span> <span className="term-cur">▊</span></div>
        </div>
      </div>
    </header>
  )
}
