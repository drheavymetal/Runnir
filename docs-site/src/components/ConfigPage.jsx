import { Fragment } from 'react'
import { CONFIG_GROUPS } from '../data/config.js'
import { useLang, UI } from '../i18n.jsx'

export default function ConfigPage({ query }) {
  const { t } = useLang()
  const q = query.trim().toLowerCase()
  const groups = CONFIG_GROUPS.map((g) => ({
    ...g,
    rows: g.rows.filter(
      (r) => !q || r.k.toLowerCase().includes(q) || t(r.d).toLowerCase().includes(q) || String(r.v).toLowerCase().includes(q) || g.group.toLowerCase().includes(q)
    ),
  })).filter((g) => g.rows.length)

  return (
    <div className="wrap">
      <h1 className="page-title">{t(UI.cfgTitle)}</h1>
      <p className="page-lede">{t(UI.cfgLede)}</p>

      <pre className="example" style={{ marginBottom: '24px' }}>{`# ~/.config/runnir/runnir.toml  (minimo, todo lo demas usa su valor por defecto)
[font]
size = 15.0

[window]
opacity = 0.92
minimap = true

[[layouts]]
name = "servers"
commands = [ "ssh 192.168.1.3", "ssh 192.168.1.7", "htop" ]

[keys]
"alt+enter" = "toggle_zoom"`}</pre>

      {groups.length === 0 && <p className="empty">{t(UI.emptyPrefix)} “{query}”.</p>}

      <div className="tbl-wrap">
        <table>
          <thead>
            <tr>
              <th style={{ width: '26%' }}>{t(UI.cfgColKey)}</th>
              <th style={{ width: '24%' }}>{t(UI.cfgColDefault)}</th>
              <th>{t(UI.cfgColDesc)}</th>
            </tr>
          </thead>
          <tbody>
            {groups.map((g) => (
              <Fragment key={g.group}>
                <tr className="grp-row">
                  <td colSpan={3}>{g.group}</td>
                </tr>
                {g.rows.map((r, i) => (
                  <tr key={g.group + i}>
                    <td className="k">{r.k}</td>
                    <td className="d">{r.v}</td>
                    <td className="desc">{t(r.d)}</td>
                  </tr>
                ))}
              </Fragment>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}
