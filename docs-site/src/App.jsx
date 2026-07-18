import { useEffect, useMemo, useState } from 'react'
import Sidebar from './components/Sidebar.jsx'
import Hero from './components/Hero.jsx'
import FeatureCard from './components/FeatureCard.jsx'
import KeybindingsPage from './components/KeybindingsPage.jsx'
import ConfigPage from './components/ConfigPage.jsx'
import { SECTIONS } from './data/sections.js'
import { FEATURES } from './data/features.js'

function matches(f, q) {
  if (!q) return true
  const hay = [
    f.title, f.natural, f.note, f.palette, f.example,
    ...(f.keys || []),
    ...(f.escape || []),
    ...((f.config || []).flatMap((c) => [c.k, c.v, c.d])),
  ].join(' ').toLowerCase()
  return hay.includes(q)
}

const VIEWS = ['guia', 'atajos', 'config']
function initialView() {
  const h = typeof window !== 'undefined' ? window.location.hash.replace('#', '') : ''
  return VIEWS.includes(h) ? h : 'guia'
}

export default function App() {
  const [view, setViewState] = useState(initialView)
  const setView = (v) => {
    setViewState(v)
    if (typeof window !== 'undefined') {
      window.location.hash = v === 'guia' ? '' : v
      window.scrollTo({ top: 0 })
    }
  }
  const [query, setQuery] = useState('')
  const [activeSection, setActiveSection] = useState(SECTIONS[0].id)
  const q = query.trim().toLowerCase()

  const filtered = useMemo(() => FEATURES.filter((f) => matches(f, q)), [q])

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

  // Scrollspy: resalta la seccion visible en la barra lateral.
  useEffect(() => {
    if (view !== 'guia') return
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

  const goHome = () => { setView('guia'); setQuery(''); window.scrollTo({ top: 0, behavior: 'smooth' }) }

  return (
    <div className="layout">
      <Sidebar
        view={view} setView={setView}
        query={query} setQuery={setQuery}
        counts={counts} activeSection={activeSection} goHome={goHome}
      />

      <main className="content">
        {view === 'guia' && (
          <div className="wrap">
            {!q && <Hero />}

            {q && filtered.length === 0 && (
              <p className="empty">Sin resultados para “{query}”. Prueba con otra palabra.</p>
            )}

            {SECTIONS.map((s) => {
              const items = bySection[s.id]
              if (items.length === 0) return null
              return (
                <section key={s.id}>
                  <div className="section-head" id={`sec-${s.id}`}>
                    <h2>{s.title}</h2>
                    <p className="blurb">{s.blurb}</p>
                  </div>
                  <hr className="section-rule" />
                  {items.map((f) => <FeatureCard key={f.title} f={f} />)}
                </section>
              )
            })}

            <p className="foot">
              runnir — {totals.total} funciones documentadas ({totals.shipped} disponibles,{' '}
              {totals.dev} en desarrollo). Contenido derivado de docs/DEVLOG.md, src/docs.rs,
              src/actions.rs y src/config.rs. Capturas generadas con runnir --render / --demo.
              <br />“rún” (susurro, en nordico antiguo) + “-nir” de Mjölnir. Un sitio donde susurrarle a la maquina.
            </p>
          </div>
        )}

        {view === 'atajos' && <KeybindingsPage query={query} />}
        {view === 'config' && <ConfigPage query={query} />}
      </main>
    </div>
  )
}
