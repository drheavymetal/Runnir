// Bilingue ES/EN. / Bilingual ES/EN.
//
// Modelo de datos: cada cadena traducible es un par { es, en }. Los datos
// (features, secciones, config, atajos) mezclan strings planos (identicos en
// ambos idiomas: comandos, teclas, claves TOML) con pares { es, en } (la prosa).
// `t()` resuelve cualquiera de los dos: string -> tal cual, par -> idioma activo.
//
// Data model: every translatable string is an { es, en } pair. The data files
// mix plain strings (identical in both languages: commands, keybinds, TOML keys)
// with { es, en } pairs (the prose). `t()` resolves either: string -> as-is,
// pair -> the active language.
import { createContext, useContext, useEffect, useState } from 'react'

const KEY = 'runnir-lang'
const DEFAULT = 'es'

export function translate(x, lang) {
  if (x == null) return x
  if (typeof x === 'string') return x
  if (typeof x === 'object' && ('es' in x || 'en' in x)) return x[lang] ?? x.es ?? x.en
  return x
}

const LangContext = createContext({ lang: DEFAULT, setLang: () => {}, t: (x) => translate(x, DEFAULT) })

export function LangProvider({ children }) {
  const [lang, setLangState] = useState(() => {
    if (typeof localStorage !== 'undefined') {
      const saved = localStorage.getItem(KEY)
      if (saved === 'es' || saved === 'en') return saved
    }
    return DEFAULT
  })

  const setLang = (l) => {
    setLangState(l)
    if (typeof localStorage !== 'undefined') localStorage.setItem(KEY, l)
  }

  useEffect(() => {
    document.documentElement.lang = lang
    document.title = translate(UI.docTitle, lang)
  }, [lang])

  const t = (x) => translate(x, lang)
  return <LangContext.Provider value={{ lang, setLang, t }}>{children}</LangContext.Provider>
}

export function useLang() {
  return useContext(LangContext)
}

// Cadenas de interfaz estaticas. / Static interface strings.
export const UI = {
  docTitle: { es: 'runnir — documentación', en: 'runnir — documentation' },
  metaDescription: {
    es: 'Documentación de runnir: un emulador de terminal GPU, keyboard-first, escrito en Rust para Linux y macOS.',
    en: 'runnir documentation: a keyboard-first, GPU-accelerated terminal emulator written in Rust for Linux and macOS.',
  },

  brandSub: { es: 'terminal GPU · Rust · docs', en: 'GPU terminal · Rust · docs' },
  searchPlaceholder: { es: 'Buscar (función, tecla, config)…', en: 'Search (feature, key, config)…' },

  navGuide: { es: 'Guía', en: 'Guide' },
  navInstall: { es: 'Instalación', en: 'Install' },
  navShortcuts: { es: 'Atajos', en: 'Shortcuts' },
  navConfig: { es: 'Config', en: 'Config' },
  navSections: { es: 'Secciones', en: 'Sections' },

  subInstall: { es: 'Cómo instalar runnir.', en: 'How to install runnir.' },
  subShortcuts: { es: 'Referencia de atajos.', en: 'Keybinding reference.' },
  subConfig: { es: 'Todas las opciones de configuración.', en: 'Every config option.' },

  langLabel: { es: 'Idioma', en: 'Language' },

  emptyPrefix: { es: 'Sin resultados para', en: 'No results for' },
  emptySuffix: { es: 'Prueba con otra palabra.', en: 'Try another term.' },

  badgeDev: { es: 'En desarrollo', en: 'In development' },
  badgeShipped: { es: 'Disponible', en: 'Shipped' },

  techShortcut: { es: 'Atajo', en: 'Keybind' },
  techPalette: { es: 'Paleta', en: 'Palette' },
  techConfig: { es: 'Config', en: 'Config' },
  techEscape: { es: 'Escape', en: 'Escape' },
  techExample: { es: 'Ejemplo', en: 'Example' },
  noteLabel: { es: 'Nota', en: 'Note' },

  // Hero
  heroTag: {
    es: 'Emulador de terminal por GPU, keyboard-first, escrito en Rust para Linux y macOS. Integración de shell real (OSC 133), asistente de IA sin salir del terminal y un puñado de detalles propios.',
    en: 'GPU-accelerated, keyboard-first terminal emulator written in Rust for Linux and macOS. Real shell integration (OSC 133), an in-terminal AI assistant, and a handful of features of its own.',
  },
  heroPills: [
    { es: 'GPU · una sola llamada de dibujo', en: 'GPU · single draw call' },
    { es: 'Rust · wgpu (Vulkan/Metal/DX12)', en: 'Rust · wgpu (Vulkan/Metal/DX12)' },
    { es: 'en reposo no consume CPU', en: 'idle: zero CPU' },
    { es: 'config TOML/JSON · recarga en caliente', en: 'TOML/JSON config · hot reload' },
  ],
  heroComment1: { es: '# terminal desplegable', en: '# drop-down terminal' },
  heroComment2: { es: 'la paleta: todo es buscable', en: 'the palette: everything is searchable' },
  heroCta: { es: 'Instalar runnir', en: 'Install runnir' },
  heroCtaHint: {
    es: 'Un comando, compila desde el código fuente, sin sudo.',
    en: 'One command, builds from source, no sudo.',
  },

  // Guide foot
  footTail: {
    es: 'Contenido derivado de src/docs.rs, src/actions.rs y src/config.rs. Capturas generadas con runnir --render / --demo.',
    en: 'Content derived from src/docs.rs, src/actions.rs and src/config.rs. Screenshots generated with runnir --render / --demo.',
  },
  footEtymology: {
    es: '"rún" (susurro, en nórdico antiguo) + "-nir" de Mjölnir. Un sitio donde susurrarle a la máquina.',
    en: '"rún" (a whisper, in Old Norse) + "-nir" from Mjölnir. A place to whisper to the machine.',
  },

  // Install page
  instTitle: { es: 'Instalación', en: 'Installation' },
  instLede: {
    es: 'Un solo comando compila runnir desde el código fuente y lo instala en ~/.local — sin sudo. El mismo install.sh gobierna los tres flujos (instalar, actualizar, desinstalar).',
    en: 'One command builds runnir from source and installs it into ~/.local — no sudo. The same install.sh drives all three flows (install, update, uninstall).',
  },
  instAltLabel: { es: 'Con wget:', en: 'With wget:' },
  instStepsTitle: { es: 'Qué hace', en: 'What it does' },
  instReqTitle: { es: 'Requisitos y variables', en: 'Requirements and overrides' },
  instMaintTitle: { es: 'Actualizar y desinstalar', en: 'Update and uninstall' },
  instPathsTitle: { es: 'Dónde queda cada cosa', en: 'Where everything lands' },
  instColPath: { es: 'Ruta', en: 'Path' },
  instColWhat: { es: 'Qué es', en: 'What it is' },
  instColStep: { es: 'Paso', en: 'Step' },
  instFlowsNote: {
    es: 'Desde un checkout del repositorio, sh install.sh --help enumera todas las opciones.',
    en: 'From a checkout of the repository, sh install.sh --help lists every option.',
  },
  instCopy: { es: 'Copiar', en: 'Copy' },
  instCopied: { es: 'Copiado', en: 'Copied' },

  // Keybindings page
  kbTitle: { es: 'Atajos de teclado', en: 'Keybindings' },
  kbLede: {
    es: 'Atajos por defecto, sacados de src/actions.rs y src/docs.rs. La columna id es el nombre de la acción para reasignarla en [keys]. Los atajos propios usan siempre Ctrl+Shift o Super, nunca Ctrl+letra a secas: eso pertenece al programa del panel.',
    en: 'Default keybinds, from src/actions.rs and src/docs.rs. The id column is the action name you rebind under [keys]. runnir’s own binds always use Ctrl+Shift or Super, never a bare Ctrl+letter — that belongs to the program in the pane.',
  },
  kbColKeys: { es: 'Teclas', en: 'Keys' },
  kbColAction: { es: 'Acción', en: 'Action' },
  kbColId: { es: 'id (config)', en: 'id (config)' },

  // Config page
  cfgTitle: { es: 'Referencia de configuración', en: 'Configuration reference' },
  cfgLede: {
    es: 'Cada opción, su valor por defecto y una línea de descripción, sacadas de src/config.rs. El archivo vive en ~/.config/runnir/runnir.toml (o runnir.json, que tiene prioridad). Todo tiene un valor por defecto: un archivo parcial o ausente es normal. Genera uno comentado con runnir --write-config.',
    en: 'Every option, its default and a one-line description, from src/config.rs. The file lives at ~/.config/runnir/runnir.toml (or runnir.json, which wins). Everything has a default; a partial or missing file is fine. Generate a commented one with runnir --write-config.',
  },
  cfgColKey: { es: 'Clave', en: 'Key' },
  cfgColDefault: { es: 'Por defecto', en: 'Default' },
  cfgColDesc: { es: 'Descripción', en: 'Description' },

  // TerminalDemo
  demoCaption: {
    es: 'Maqueta animada (CSS) del efecto — no es una captura del binario.',
    en: 'Animated CSS mock-up of the effect — not a screenshot of the binary.',
  },
}
