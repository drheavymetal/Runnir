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
  // ---- introducción / introduction --------------------------------------
  introLead: {
    es: 'runnir es un emulador de terminal escrito desde cero en Rust: parser VT propio, renderizador propio por GPU (wgpu + winit), ninguna librería de terminal por debajo. Se usa a diario en Linux/Wayland.',
    en: 'runnir is a terminal emulator written from scratch in Rust: its own VT parser, its own GPU renderer (wgpu + winit), no terminal library underneath. It is a daily driver on Linux/Wayland.',
  },
  introBet: {
    es: 'La mayoría de terminales son un rectángulo rápido dentro del cual ejecutas las TUIs de otros. runnir apuesta al revés: cuando el renderizador es tuyo, el cliente de git, el árbol de ficheros, el panel de contenedores y el visor de imágenes pueden ser paneles nativos en vez de procesos peleándose por las mismas 80×24. Comparten el tema, el keymap y la capa which-key, dibujan a velocidad de GPU, y saben cosas que una TUI no puede saber: qué panel está en un prompt, sobre qué fichero está el cursor, a qué host estás conectado por ssh.',
    en: 'Most terminals are a fast rectangle you run other people’s TUIs inside. runnir bets the other way: when the renderer is yours, the git client, the file tree, the container dashboard and the image viewer can be native panels instead of processes fighting over the same 80×24. They share the theme, the keymap and the which-key layer, they draw at GPU speed, and they know things a TUI cannot — which pane is sitting at a prompt, which file the cursor is on, which host you are ssh’d into.',
  },
  introPoints: [
    {
      title: { es: 'Paneles nativos, no TUIs', en: 'Native panels, not TUIs' },
      body: {
        es: 'Cliente de git completo (estado, log con grafo, ramas, stashes, blame, staging por líneas, rebase interactivo), panel de Docker con Docker Hub y despliegue, explorador de ficheros con badges de git, y visor de texto e imágenes. Todo dibujado por runnir, no procesos externos.',
        en: 'A full git client (status, graph log, branches, stashes, blame, line-by-line staging, interactive rebase), a Docker panel with Docker Hub and deploys, a file explorer with git badges, and a text and image viewer. All drawn by runnir, not external processes.',
      },
    },
    {
      title: { es: 'Una capa que se enseña sola', en: 'A layer that teaches itself' },
      body: {
        es: 'El compositor gana toda carrera de modificadores, así que runnir monta su propia capa detrás de una tecla leader. Al armarla, un panel which-key lista qué hace la siguiente tecla: no hay tabla que memorizar. Cada panel tiene su propio leader, filtrado por lo que la fila bajo el cursor puede hacer.',
        en: 'The compositor wins every modifier race, so runnir keeps its own layer behind a leader key. Arming it shows a which-key panel listing what the next key does: no table to memorise. Every panel has its own leader, filtered by what the row under the cursor can actually do.',
      },
    },
    {
      title: { es: 'Sabe qué está pasando', en: 'It knows what is going on' },
      body: {
        es: 'Con integración de shell por OSC 133/7, la sesión está segmentada por comando: saltar entre comandos, gutter de estado, prompt fijo al hacer scroll, splits que heredan el directorio, avisos cuando termina algo largo, y un guardián que frena un rm -rf / antes de que Enter llegue a la shell.',
        en: 'With OSC 133/7 shell integration the session is segmented per command: jump between commands, a status gutter, a sticky prompt while scrolled back, splits that inherit the cwd, notifications when something long finishes, and a guardian that stops an rm -rf / before Enter reaches the shell.',
      },
    },
    {
      title: { es: 'Se puede conducir desde fuera', en: 'It can be driven from outside' },
      body: {
        es: 'runnir @ ejecuta acciones, pulsa teclas, hace clic y mueve la rueda en el propio terminal — no en el proceso hijo — y cada respuesta trae el estado de la UI en JSON. Es como se prueban los paneles, sin capturas de pantalla, y sirve igual para scriptar tu propio flujo.',
        en: 'runnir @ runs actions, presses keys, clicks and turns the wheel in the terminal itself — not in the child process — and every reply carries the UI state as JSON. It is how the panels are tested, without screenshots, and it works just as well for scripting your own workflow.',
      },
    },
  ],
  introForTitle: { es: 'Para quién es', en: 'Who it is for' },
  introFor: [
    { es: 'Trabajas en la terminal todo el día y abres git, docker y un explorador junto a ella.', en: 'You live in the terminal all day and keep git, docker and a file browser open beside it.' },
    { es: 'Prefieres el teclado y te molesta memorizar tablas de atajos.', en: 'You prefer the keyboard and resent memorising shortcut tables.' },
    { es: 'Quieres poder scriptar tu terminal, no solo lo que corre dentro.', en: 'You want to script your terminal, not just what runs inside it.' },
    { es: 'Linux con Wayland (probado a diario en Hyprland) o macOS.', en: 'Linux on Wayland (a daily driver on Hyprland) or macOS.' },
  ],
  introNotTitle: { es: 'Qué no es', en: 'What it is not' },
  introNot: [
    { es: 'No es un multiplexor: no hay sesiones que sobrevivan al cierre ni attach remoto. Para eso, tmux dentro de runnir funciona.', en: 'Not a multiplexer: no sessions surviving a close, no remote attach. tmux inside runnir works fine for that.' },
    { es: 'No es un editor. El visor de ficheros es de solo lectura a propósito: su editor es el que corras en un panel.', en: 'Not an editor. The file viewer is deliberately read-only: its editor is whatever you run in a pane.' },
    { es: 'No hay soporte de Windows.', en: 'No Windows support.' },
    { es: 'No hay binarios precompilados: se compila desde el código en tu máquina.', en: 'No prebuilt binaries: it builds from source on your machine.' },
  ],
  introFoot: {
    es: 'Todo lo que sigue está documentado función a función, con sus atajos y sus opciones de configuración.',
    en: 'Everything below is documented feature by feature, with its keybindings and config options.',
  },
  introFootCta: { es: 'Ver cómo se instala', en: 'See how to install it' },

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
    es: 'Atajos por defecto, sacados de src/actions.rs y src/docs.rs. La columna id es el nombre de la acción para reasignarla en [keys]. Los atajos propios usan siempre Ctrl+Shift, Alt+Shift o la capa leader, nunca Ctrl+letra a secas: eso pertenece al programa del panel. Tampoco Super: el compositor se queda esa capa antes de que runnir vea la tecla.',
    en: 'Default keybinds, from src/actions.rs and src/docs.rs. The id column is the action name you rebind under [keys]. runnir’s own binds always use Ctrl+Shift, Alt+Shift or the leader layer, never a bare Ctrl+letter — that belongs to the program in the pane. Never Super either: the compositor grabs that layer before runnir sees the key.',
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
