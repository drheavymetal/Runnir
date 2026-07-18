import { SECTIONS } from '../data/sections.js'
import { useLang, UI } from '../i18n.jsx'

export default function Sidebar({ view, setView, query, setQuery, counts, activeSection, goHome }) {
  const { lang, setLang, t } = useLang()

  return (
    <aside className="sidebar">
      <div className="brand-row">
        <div className="brand" onClick={goHome} role="button" title="runnir">
          <img className="brand-mark" src="./icon.png" alt="runnir" width="30" height="30" />
          <span className="brand-name">runnir</span>
        </div>
        <div className="lang-toggle" role="group" aria-label={t(UI.langLabel)}>
          <button
            className={`lang-btn ${lang === 'es' ? 'active' : ''}`}
            onClick={() => setLang('es')}
            aria-pressed={lang === 'es'}
          >ES</button>
          <button
            className={`lang-btn ${lang === 'en' ? 'active' : ''}`}
            onClick={() => setLang('en')}
            aria-pressed={lang === 'en'}
          >EN</button>
        </div>
      </div>
      <p className="brand-sub">{t(UI.brandSub)}</p>

      <input
        className="search"
        type="search"
        placeholder={t(UI.searchPlaceholder)}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        aria-label={t(UI.searchPlaceholder)}
      />

      <div className="nav-tabs">
        <button className={`nav-tab ${view === 'guia' ? 'active' : ''}`} onClick={() => setView('guia')}>{t(UI.navGuide)}</button>
        <button className={`nav-tab ${view === 'instalacion' ? 'active' : ''}`} onClick={() => setView('instalacion')}>{t(UI.navInstall)}</button>
        <button className={`nav-tab ${view === 'atajos' ? 'active' : ''}`} onClick={() => setView('atajos')}>{t(UI.navShortcuts)}</button>
        <button className={`nav-tab ${view === 'config' ? 'active' : ''}`} onClick={() => setView('config')}>{t(UI.navConfig)}</button>
      </div>

      {view === 'guia' && (
        <nav>
          <div className="nav-section-label">{t(UI.navSections)}</div>
          <ul className="nav-list">
            {SECTIONS.map((s) => {
              const n = counts[s.id] ?? 0
              if (query && n === 0) return null
              return (
                <li key={s.id}>
                  <a
                    href={`#sec-${s.id}`}
                    className={activeSection === s.id ? 'active' : ''}
                    onClick={(e) => {
                      e.preventDefault()
                      document.getElementById(`sec-${s.id}`)?.scrollIntoView({ behavior: 'smooth' })
                    }}
                  >
                    {t(s.title)}
                    <span className="nav-count">{n}</span>
                  </a>
                </li>
              )
            })}
          </ul>
        </nav>
      )}

      {view !== 'guia' && (
        <p className="brand-sub" style={{ margin: '8px 6px' }}>
          {view === 'instalacion' ? t(UI.subInstall) : view === 'atajos' ? t(UI.subShortcuts) : t(UI.subConfig)}
        </p>
      )}
    </aside>
  )
}
