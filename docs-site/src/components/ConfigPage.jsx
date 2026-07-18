import { Fragment } from 'react'
import { CONFIG_GROUPS } from '../data/config.js'

export default function ConfigPage({ query }) {
  const q = query.trim().toLowerCase()
  const groups = CONFIG_GROUPS.map((g) => ({
    ...g,
    rows: g.rows.filter(
      (r) => !q || r.k.toLowerCase().includes(q) || r.d.toLowerCase().includes(q) || String(r.v).toLowerCase().includes(q) || g.group.toLowerCase().includes(q)
    ),
  })).filter((g) => g.rows.length)

  return (
    <div className="wrap">
      <h1 className="page-title">Referencia de configuracion</h1>
      <p className="page-lede">
        Cada opcion, su valor por defecto y una linea de descripcion, sacadas de{' '}
        <code>src/config.rs</code>. El archivo vive en{' '}
        <code>~/.config/runnir/runnir.toml</code> (o <code>runnir.json</code>, que tiene
        prioridad). Todo tiene un valor por defecto: un archivo parcial o ausente es normal.
        Genera uno comentado con <code>runnir --write-config</code>.
      </p>

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

      {groups.length === 0 && <p className="empty">Sin resultados para “{query}”.</p>}

      <div className="tbl-wrap">
        <table>
          <thead>
            <tr>
              <th style={{ width: '26%' }}>Clave</th>
              <th style={{ width: '24%' }}>Por defecto</th>
              <th>Descripcion</th>
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
                    <td className="desc">{r.d}</td>
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
