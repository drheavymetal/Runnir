import { SECTIONS } from '../data/sections.js'

export default function Sidebar({ view, setView, query, setQuery, counts, activeSection, goHome }) {
  return (
    <aside className="sidebar">
      <div className="brand" onClick={goHome} role="button" title="Inicio">
        <img className="brand-mark" src="./icon.png" alt="runnir" width="30" height="30" />
        <span className="brand-name">runnir</span>
      </div>
      <p className="brand-sub">terminal GPU · Rust · docs</p>

      <input
        className="search"
        type="search"
        placeholder="Buscar (feature, tecla, config)…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        aria-label="Buscar"
      />

      <div className="nav-tabs">
        <button className={`nav-tab ${view === 'guia' ? 'active' : ''}`} onClick={() => setView('guia')}>Guia</button>
        <button className={`nav-tab ${view === 'atajos' ? 'active' : ''}`} onClick={() => setView('atajos')}>Atajos</button>
        <button className={`nav-tab ${view === 'config' ? 'active' : ''}`} onClick={() => setView('config')}>Config</button>
      </div>

      {view === 'guia' && (
        <nav>
          <div className="nav-section-label">Secciones</div>
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
                    {s.title}
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
          {view === 'atajos' ? 'Chuleta completa de atajos.' : 'Todas las opciones de configuracion.'}
        </p>
      )}
    </aside>
  )
}
