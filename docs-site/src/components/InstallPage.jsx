import { useState } from 'react'
import {
  INSTALL_CMD, INSTALL_CMD_ALT, INSTALL_STEPS,
  INSTALL_NOTES, INSTALL_MAINTENANCE, INSTALL_PATHS, INSTALL_FLOWS,
} from '../data/install.js'
import { useLang, UI } from '../i18n.jsx'

// Bloque de comando copiable. / Copyable command block.
function Cmd({ code }) {
  const { t } = useLang()
  const [done, setDone] = useState(false)
  const copy = () => {
    navigator.clipboard?.writeText(code).then(() => {
      setDone(true)
      setTimeout(() => setDone(false), 1600)
    }).catch(() => {})
  }
  return (
    <div className="cmd-row">
      <pre className="example">{code}</pre>
      <button className="cmd-copy" onClick={copy} aria-label={t(UI.instCopy)}>
        {done ? t(UI.instCopied) : t(UI.instCopy)}
      </button>
    </div>
  )
}

export default function InstallPage({ query }) {
  const { t } = useLang()
  const q = query.trim().toLowerCase()
  const hit = (...parts) => !q || parts.filter(Boolean).join(' ').toLowerCase().includes(q)

  const steps = INSTALL_STEPS.filter((s) => hit(s.k, t(s.d)))
  const notes = INSTALL_NOTES.filter((n) => hit(t(n.title), t(n.body), n.code))
  const maint = INSTALL_MAINTENANCE.filter((m) => hit(t(m.title), t(m.body), m.code, t(m.note)))
  const paths = INSTALL_PATHS.filter((p) => hit(p.k, t(p.d)))
  const nothing = !steps.length && !notes.length && !maint.length && !paths.length

  return (
    <div className="wrap">
      <h1 className="page-title">{t(UI.instTitle)}</h1>
      <p className="page-lede">{t(UI.instLede)}</p>

      <Cmd code={INSTALL_CMD} />
      <p className="note">{t(UI.instAltLabel)}</p>
      <Cmd code={INSTALL_CMD_ALT} />

      {nothing && <p className="empty">{t(UI.emptyPrefix)} “{query}”.</p>}

      {steps.length > 0 && (
        <>
          <div className="section-head"><h2>{t(UI.instStepsTitle)}</h2></div>
          <hr className="section-rule" />
          <div className="tbl-wrap">
            <table>
              <thead>
                <tr>
                  <th style={{ width: '34%' }}>{t(UI.instColStep)}</th>
                  <th>{t(UI.instColWhat)}</th>
                </tr>
              </thead>
              <tbody>
                {steps.map((s) => (
                  <tr key={s.k}>
                    <td className="k">{s.k}</td>
                    <td className="desc">{t(s.d)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}

      {notes.length > 0 && (
        <>
          <div className="section-head"><h2>{t(UI.instReqTitle)}</h2></div>
          <hr className="section-rule" />
          {notes.map((n) => (
            <div className="card" key={n.id}>
              <div className="card-head"><h3>{t(n.title)}</h3></div>
              <p className="natural">{t(n.body)}</p>
              {n.code && <Cmd code={n.code} />}
              {n.note && <p className="note">{t(n.note)}</p>}
            </div>
          ))}
        </>
      )}

      {maint.length > 0 && (
        <>
          <div className="section-head"><h2>{t(UI.instMaintTitle)}</h2></div>
          <hr className="section-rule" />
          {maint.map((m) => (
            <div className="card" key={m.id}>
              <div className="card-head"><h3>{t(m.title)}</h3></div>
              <p className="natural">{t(m.body)}</p>
              <Cmd code={m.code} />
              {m.note && <p className="note">{t(m.note)}</p>}
            </div>
          ))}
        </>
      )}

      {paths.length > 0 && (
        <>
          <div className="section-head"><h2>{t(UI.instPathsTitle)}</h2></div>
          <hr className="section-rule" />
          <div className="tbl-wrap">
            <table>
              <thead>
                <tr>
                  <th style={{ width: '46%' }}>{t(UI.instColPath)}</th>
                  <th>{t(UI.instColWhat)}</th>
                </tr>
              </thead>
              <tbody>
                {paths.map((p) => (
                  <tr key={p.k}>
                    <td className="k">{p.k}</td>
                    <td className="desc">{t(p.d)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}

      {!q && (
        <p className="note">
          {t(UI.instFlowsNote)} <code className="pcmd">{INSTALL_FLOWS}</code>
        </p>
      )}
    </div>
  )
}
