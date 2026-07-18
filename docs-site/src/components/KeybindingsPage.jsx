import { KEY_GROUPS } from '../data/keybindings.js'
import { useLang, UI } from '../i18n.jsx'

// Divide "Ctrl+Shift+T" en <kbd> por cada clave del acorde.
function Chord({ spec }) {
  if (/^(Paleta|Palette|Clic|Middle|Ctrl\+Shift\+P ->)/.test(spec) || spec.includes(' ')) {
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
  const { t } = useLang()
  const q = query.trim().toLowerCase()
  const groups = KEY_GROUPS.map((g) => ({
    ...g,
    rows: g.rows.filter(
      (r) => !q || t(r.title).toLowerCase().includes(q) || r.id.toLowerCase().includes(q) || r.keys.map(t).join(' ').toLowerCase().includes(q)
    ),
  })).filter((g) => g.rows.length)

  return (
    <div className="wrap">
      <h1 className="page-title">{t(UI.kbTitle)}</h1>
      <p className="page-lede">{t(UI.kbLede)}</p>

      {groups.length === 0 && <p className="empty">{t(UI.emptyPrefix)} “{query}”.</p>}

      {groups.map((g, gi) => (
        <div className="tbl-wrap" key={gi}>
          <table>
            <thead>
              <tr>
                <th style={{ width: '34%' }}>{t(g.group)}</th>
                <th>{t(UI.kbColAction)}</th>
                <th style={{ width: '26%' }}>{t(UI.kbColId)}</th>
              </tr>
            </thead>
            <tbody>
              {g.rows.map((r, i) => (
                <tr key={i}>
                  <td>{r.keys.map((k, j) => <span key={j}>{j > 0 && <span className="plain" style={{ color: 'var(--fg-faint)' }}> / </span>}<Chord spec={t(k)} /></span>)}</td>
                  <td>{t(r.title)}</td>
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
