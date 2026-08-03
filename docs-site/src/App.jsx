import { useEffect, useMemo, useState } from 'react'
import Sidebar from './components/Sidebar.jsx'
import Hero from './components/Hero.jsx'
import FeatureCard from './components/FeatureCard.jsx'
import KeybindingsPage from './components/KeybindingsPage.jsx'
import ConfigPage from './components/ConfigPage.jsx'
import InstallPage from './components/InstallPage.jsx'
import Intro from './components/Intro.jsx'
import ReceivePage from './components/ReceivePage.jsx'
import { SECTIONS } from './data/sections.js'
import { FEATURES } from './data/features.js'
import { useLang, UI } from './i18n.jsx'

// Construye el haystack de búsqueda en el idioma activo. / Search haystack in the active language.
function matches(f, q, t) {
  if (!q) return true
  const hay = [
    t(f.title), t(f.natural), t(f.note), f.palette, f.example,
    ...((f.keys || []).map(t)),
    ...(f.escape || []),
    ...((f.config || []).flatMap((c) => [c.k, c.v, t(c.d)])),
  ].filter(Boolean).join(' ').toLowerCase()
  return hay.includes(q)
}

// Identificadores de vista en INGLÉS, como el resto de nombres del proyecto (el
// texto se traduce; los nombres no). Aparecen en la URL, así que son parte de la
// interfaz pública del sitio.
//
// View identifiers in ENGLISH, like every other name in the project: text gets
// translated, names do not. They show up in the URL, so they are part of the
// site's public surface.
const VIEWS = ['guide', 'install', 'shortcuts', 'config', 'receive']

// Los nombres castellanos que tuvo el sitio antes. Un enlace guardado o pegado en
// un chat no debería llevar a la portada en silencio. / The Spanish names the
// site used to have: a saved or pasted link should not land on the front page
// without a word.
const RENAMED = {
  guia: 'guide',
  instalacion: 'install',
  atajos: 'shortcuts',
  recibir: 'receive',
}

function initialView() {
  const h = typeof window !== 'undefined' ? window.location.hash.replace('#', '') : ''
  if (VIEWS.includes(h)) return h
  return RENAMED[h] ?? 'guide'
}

export default function App() {
  const { t } = useLang()
  const [view, setViewState] = useState(initialView)
  const setView = (v) => {
    setViewState(v)
    if (typeof window !== 'undefined') {
      window.location.hash = v === 'guide' ? '' : v
      window.scrollTo({ top: 0 })
    }
  }
  const [query, setQuery] = useState('')
  const [activeSection, setActiveSection] = useState(SECTIONS[0].id)
  const q = query.trim().toLowerCase()

  const filtered = useMemo(() => FEATURES.filter((f) => matches(f, q, t)), [q, t])

  const bySection = useMemo(() => {
    const map = {}
    for (const s of SECTIONS) map[s.id] = filtered.filter((f) => f.section === s.id)
    return map
  }, [filtered])

  const counts = useMemo(() => {
    const c = {}
    for (const s of SECTIONS) c[s.id] = bySection[s.id].length
    return c
  }, [bySection])

  const totals = useMemo(() => {
    const shipped = FEATURES.filter((f) => f.status !== 'dev').length
    return { total: FEATURES.length, shipped, dev: FEATURES.length - shipped }
  }, [])

  // Scrollspy: resalta la sección visible en la barra lateral.
  useEffect(() => {
    if (view !== 'guide') return
    const obs = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) setActiveSection(e.target.id.replace('sec-', ''))
        }
      },
      { rootMargin: '-10% 0px -80% 0px', threshold: 0 }
    )
    SECTIONS.forEach((s) => {
      const el = document.getElementById(`sec-${s.id}`)
      if (el) obs.observe(el)
    })
    return () => obs.disconnect()
  }, [view, q])

  const goHome = () => { setView('guide'); setQuery(''); window.scrollTo({ top: 0, behavior: 'smooth' }) }

  return (
    <div className="layout">
      <Sidebar
        view={view} setView={setView}
        query={query} setQuery={setQuery}
        counts={counts} activeSection={activeSection} goHome={goHome}
      />

      <main className="content">
        {view === 'guide' && (
          <div className="wrap">
            {!q && <Hero onInstall={() => setView('install')} />}
            {!q && <Intro onInstall={() => setView('install')} />}

            {q && filtered.length === 0 && (
              <p className="empty">{t(UI.emptyPrefix)} “{query}”. {t(UI.emptySuffix)}</p>
            )}

            {SECTIONS.map((s) => {
              const items = bySection[s.id]
              if (items.length === 0) return null
              return (
                <section key={s.id}>
                  <div className="section-head" id={`sec-${s.id}`}>
                    <h2>{t(s.title)}</h2>
                    <p className="blurb">{t(s.blurb)}</p>
                  </div>
                  <hr className="section-rule" />
                  {items.map((f) => <FeatureCard key={f.key} f={f} />)}
                </section>
              )
            })}

            <p className="foot">
              runnir — {totals.total} {t({ es: 'funciones documentadas', en: 'documented features' })} ({totals.shipped}{' '}
              {t({ es: 'disponibles', en: 'shipped' })}, {totals.dev} {t({ es: 'en desarrollo', en: 'in development' })}).{' '}
              {t(UI.footTail)}
              <br />{t(UI.footEtymology)}
            </p>
          </div>
        )}

        {view === 'install' && <InstallPage query={query} />}
        {view === 'shortcuts' && <KeybindingsPage query={query} />}
        {view === 'config' && <ConfigPage query={query} />}
        {view === 'receive' && <ReceivePage />}
      </main>
    </div>
  )
}
