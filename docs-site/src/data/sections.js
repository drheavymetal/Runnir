// Metadatos de las secciones. El orden aquí es el orden en la barra lateral.
// title/blurb son pares { es, en } (ver src/i18n.jsx).
// Section metadata. Order here is the sidebar order. title/blurb are { es, en }.
export const SECTIONS = [
  {
    id: 'core',
    title: { es: 'Núcleo', en: 'Core' },
    blurb: { es: 'Pestañas, splits, sesiones y modo quake.', en: 'Tabs, splits, sessions and quake mode.' },
  },
  {
    id: 'rendering',
    title: { es: 'Renderizado', en: 'Rendering' },
    blurb: { es: 'Ligaturas, cajas, imágenes en línea, cursor y subrayados.', en: 'Ligatures, box drawing, inline images, cursor and underlines.' },
  },
  {
    id: 'input',
    title: { es: 'Entrada y selección', en: 'Input and selection' },
    blurb: { es: 'Teclado, ratón, copy-mode y pistas sin ratón.', en: 'Keyboard, mouse, copy mode and mouse-free hints.' },
  },
  {
    id: 'shell',
    title: { es: 'Integración con la shell', en: 'Shell integration' },
    blurb: { es: 'OSC 133: saltar entre comandos, gutter de estado, prompt fijo, cwd.', en: 'OSC 133: command jumps, status gutter, sticky prompt, cwd.' },
  },
  {
    id: 'scrollback',
    title: { es: 'Historial', en: 'Scrollback' },
    blurb: { es: 'Buscar, minimapa, plegado y volcar el historial al editor.', en: 'Search, minimap, folding and dumping history to the editor.' },
  },
  {
    id: 'ai',
    title: { es: 'IA', en: 'AI' },
    blurb: { es: 'Panel, lenguaje natural a comando, explicar, resumir y whisper.', en: 'Panel, natural-language-to-command, explain, summarize and whisper.' },
  },
  {
    id: 'distinctive',
    title: { es: 'Funciones propias', en: 'Signature features' },
    blurb: { es: 'Guardian de comandos, watch, layouts, broadcast y tintado por SSH.', en: 'Command guardian, watch, layouts, broadcast and SSH tinting.' },
  },
  {
    id: 'appearance',
    title: { es: 'Apariencia', en: 'Appearance' },
    blurb: { es: 'Opacidad, fondo, temas, iconos de pestaña, barra de estado, estela.', en: 'Opacity, background, themes, tab icons, status bar, cursor trail.' },
  },
  {
    id: 'protocols',
    title: { es: 'Protocolos', en: 'Protocols' },
    blurb: { es: 'Hyperlinks, portapapeles, progreso, notificaciones y teclado kitty.', en: 'Hyperlinks, clipboard, progress, notifications and the kitty keyboard.' },
  },
  {
    id: 'automation',
    title: { es: 'Automatización', en: 'Automation' },
    blurb: { es: 'Controlar runnir desde fuera con la API de control remoto.', en: 'Driving runnir from outside via the remote-control API.' },
  },
  {
    id: 'config',
    title: { es: 'Configuración', en: 'Configuration' },
    blurb: { es: 'TOML/JSON, panel de ajustes y recarga en caliente.', en: 'TOML/JSON, settings panel and hot reload.' },
  },
  {
    id: 'platform',
    title: { es: 'Plataforma', en: 'Platform' },
    blurb: { es: 'Dónde corre runnir y qué necesita.', en: 'Where runnir runs and what it needs.' },
  },
  {
    id: 'roadmap',
    title: { es: 'Hoja de ruta', en: 'Roadmap' },
    blurb: { es: 'Lo que viene después, aún sin empezar.', en: 'What comes next, not started yet.' },
  },
]
