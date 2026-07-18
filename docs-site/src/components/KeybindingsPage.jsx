import { KEY_GROUPS } from '../data/keybindings.js'

// Divide "Ctrl+Shift+T" en <kbd> por cada clave del acorde.
function Chord({ spec }) {
  if (/^(Paleta|Clic|Ctrl\+Shift\+P ->)/.test(spec) || spec.includes(' ')) {
    // etiquetas descriptivas: mostrar tal cual
    if (spec.includes('+') && !spec.includes(' ')) {
      // fallthrough
    } else {
      return <kbd>{spec}</kbd>
    }
  }
  return (
    <span>
      {spec.split('+').map((k, i) => (
        <span key={i}>{i > 0 && <span className="plain"> + </span>}<kbd>{k}</kbd></span>
      ))}
    </span>
  )
}

export default function KeybindingsPage({ query }) {
  const q = query.trim().toLowerCase()
  const groups = KEY_GROUPS.map((g) => ({
    ...g,
    rows: g.rows.filter(
      (r) => !q || r.title.toLowerCase().includes(q) || r.id.toLowerCase().includes(q) || r.keys.join(' ').toLowerCase().includes(q)
    ),
  })).filter((g) => g.rows.length)

  return (
    <div className="wrap">
      <h1 className="page-title">Chuleta de atajos</h1>
      <p className="page-lede">
        Todos los atajos por defecto, sacados de <code>src/actions.rs</code> y{' '}
        <code>src/docs.rs</code>. La columna <b>id</b> es el identificador de la accion
        para reasignarla en <code>[keys]</code> del config. Los atajos propios usan
        siempre Ctrl+Shift o Super, nunca Ctrl+letra a secas (eso es del programa del panel).
      </p>

      {groups.length === 0 && <p className="empty">Sin resultados para “{query}”.</p>}

      {groups.map((g) => (
        <div className="tbl-wrap" key={g.group}>
          <table>
            <thead>
              <tr>
                <th style={{ width: '34%' }}>{g.group}</th>
                <th>Accion</th>
                <th style={{ width: '26%' }}>id (config)</th>
              </tr>
            </thead>
            <tbody>
              {g.rows.map((r, i) => (
                <tr key={i}>
                  <td>{r.keys.map((k, j) => <span key={j}>{j > 0 && <span className="plain" style={{ color: 'var(--fg-faint)' }}> / </span>}<Chord spec={k} /></span>)}</td>
                  <td>{r.title}</td>
                  <td><span className="id">{r.id || '—'}</span></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ))}
    </div>
  )
}
