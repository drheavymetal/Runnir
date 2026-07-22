import { useLang, UI } from '../i18n.jsx'

// La introducción: qué es runnir y por qué existe, antes del catálogo de funciones.
// Va justo bajo el hero y solo cuando no hay búsqueda activa — quien busca algo
// concreto no quiere leer una tesis.
//
// The introduction: what runnir is and why, ahead of the feature catalogue. Sits
// under the hero, and only when no search is active — someone searching for one
// thing does not want an essay.
export default function Intro({ onInstall }) {
  const { t } = useLang()
  return (
    <section className="intro">
      <p className="intro-lead">{t(UI.introLead)}</p>
      <p className="intro-body">{t(UI.introBet)}</p>

      <div className="intro-grid">
        {UI.introPoints.map((p, i) => (
          <div className="intro-card" key={i}>
            <h3>{t(p.title)}</h3>
            <p>{t(p.body)}</p>
          </div>
        ))}
      </div>

      <div className="intro-split">
        <div className="intro-col">
          <h3 className="intro-col-head intro-yes">{t(UI.introForTitle)}</h3>
          <ul className="intro-list">
            {UI.introFor.map((l, i) => <li key={i}>{t(l)}</li>)}
          </ul>
        </div>
        <div className="intro-col">
          <h3 className="intro-col-head intro-no">{t(UI.introNotTitle)}</h3>
          <ul className="intro-list">
            {UI.introNot.map((l, i) => <li key={i}>{t(l)}</li>)}
          </ul>
        </div>
      </div>

      <p className="intro-foot">
        {t(UI.introFoot)}{' '}
        <button className="intro-link" onClick={onInstall}>{t(UI.introFootCta)}</button>
      </p>
    </section>
  )
}
