// Todas las funciones de runnir. Cada una tiene:
//   key      -> identificador estable (slug). Ancla del DOM y clave de MEDIA/DEMOS.
//   section  -> id de SECTIONS
//   title    -> { es, en }
//   status   -> 'shipped' (ya funciona) | 'dev' (en desarrollo, sin fusionar)
//   natural  -> { es, en }  explicación en prosa
//   keys[]   -> combinaciones de teclas. Cada entrada es un string (idéntico en
//               ambos idiomas) o { es, en } cuando lleva una pista traducible.
//   palette  -> string. Nombre del comando en la paleta; idéntico en ambos idiomas
//               (es la cadena literal del binario, en inglés).
//   config[] -> { k, v, d } donde d es { es, en }; k y v son idénticos.
//   escape[] -> secuencias de escape / protocolo, verbatim, idénticas.
//   example  -> string. Comando o snippet, verbatim, idéntico.
//   note     -> { es, en }
//
// English note: only prose (natural / note / config.d / key hints) is translated.
// Commands, keybinds, palette names, TOML keys and escape sequences stay identical.
//
// Escape sequences use String.raw to keep the backslashes:
//   \e = ESC (0x1b),  \a = BEL (0x07),  ST = string terminator (ESC \).
const R = String.raw

export const FEATURES = [
  // ------------------------------------------------------------------ NUCLEO
  {
    key: 'tabs', section: 'core', status: 'shipped',
    title: { es: 'Pestañas', en: 'Tabs' },
    natural: {
      es: 'Cada pestaña es una sesión de terminal independiente: su propia shell, su historial y su directorio. La barra se desplaza sola para mantener visible la activa.',
      en: 'Each tab is an independent terminal session: its own shell, scrollback and directory. The tab bar scrolls to keep the active tab visible.',
    },
    keys: [
      { es: 'Ctrl+Shift+T (nueva)', en: 'Ctrl+Shift+T (new)' },
      { es: 'Ctrl+Shift+W (cerrar)', en: 'Ctrl+Shift+W (close)' },
      { es: 'Ctrl+PageUp / Ctrl+PageDown (anterior / siguiente)', en: 'Ctrl+PageUp / Ctrl+PageDown (prev / next)' },
      { es: 'Leader 1..9 (ir a la pestaña N; leader = Alt+Shift+Space)', en: 'Leader 1..9 (jump to tab N; leader = Alt+Shift+Space)' },
      { es: 'Ctrl+Shift+R (renombrar)', en: 'Ctrl+Shift+R (rename)' },
      { es: 'Ctrl+Shift+Left / Right (mover en la barra)', en: 'Ctrl+Shift+Left / Right (move in the bar)' },
    ],
    palette: 'New tab / Close tab / Next tab / Previous tab / Rename tab / Move tab left / Move tab right',
  },
  {
    key: 'reopen-tab', section: 'core', status: 'shipped',
    title: { es: 'Reabrir pestaña cerrada', en: 'Reopen closed tab' },
    natural: {
      es: 'Recupera la última pestaña cerrada con su disposición de paneles, los directorios de cada uno y el historial. Los procesos no reviven; la estructura y lo que se vio, sí.',
      en: 'Brings back the last closed tab with its pane layout, each pane’s directory and its scrollback. Processes do not come back; the layout and what was on screen do.',
    },
    keys: ['Ctrl+Shift+U'],
    palette: 'Reopen closed tab',
  },
  {
    key: 'splits', section: 'core', status: 'shipped',
    title: { es: 'Splits (paneles)', en: 'Splits (panes)' },
    natural: {
      es: 'Una pestaña se divide en paneles; cada panel es su propia shell. Un split hereda el directorio del panel actual. El foco es geométrico: "foco a la derecha" va al panel que ves a la derecha, sin importar el orden en que hiciste los splits.',
      en: 'A tab splits into panes; each pane is its own shell. A split inherits the current pane’s directory. Focus is geometric: "focus right" goes to the pane you see on the right, whatever order you built the splits in.',
    },
    keys: [
      { es: 'Ctrl+Shift+D (dividir izq/der)', en: 'Ctrl+Shift+D (split left/right)' },
      { es: 'Ctrl+Shift+E (dividir arriba/abajo)', en: 'Ctrl+Shift+E (split up/down)' },
      { es: 'Ctrl+Shift+X (cerrar panel)', en: 'Ctrl+Shift+X (close pane)' },
      { es: 'Ctrl+Shift+H/J/K/L (mover foco, direcciones vim)', en: 'Ctrl+Shift+H/J/K/L (move focus, vim directions)' },
      { es: 'Leader h/j/k/l (mover foco, sin modificadores)', en: 'Leader h/j/k/l (move focus, no modifiers)' },
      { es: 'Alt+Shift+flechas, o Leader H/J/K/L o flechas (redimensionar)', en: 'Alt+Shift+arrows, or Leader H/J/K/L or arrows (resize)' },
    ],
    palette: 'Split pane left/right / Split pane up/down / Close pane',
  },
  {
    key: 'pane-zoom', section: 'core', status: 'shipped',
    title: { es: 'Zoom de panel', en: 'Pane zoom' },
    natural: {
      es: 'Amplía el panel enfocado hasta llenar la pestaña y vuelve con la misma tecla. Los demás paneles siguen vivos por debajo; sólo cambia lo que ves.',
      en: 'Blows the focused pane up to fill the tab; the same key toggles back. The other panes stay alive underneath — only what you see changes.',
    },
    keys: ['Ctrl+Shift+Z'],
    palette: 'Zoom / unzoom focused pane',
  },
  {
    key: 'sessions', section: 'core', status: 'shipped',
    title: { es: 'Sesiones (restaurar al arrancar)', en: 'Sessions (restore on startup)' },
    natural: {
      es: 'Al arrancar, runnir recupera la sesión anterior: pestañas, disposición de paneles, directorios y texto del historial. Los procesos no sobreviven a un reinicio; el layout y el historial, sí.',
      en: 'On startup runnir restores the previous session: tabs, pane layout, directories and scrollback text. Processes don’t survive a restart; the layout and history do.',
    },
    config: [{ k: 'behaviour.restore_session', v: 'true', d: { es: 'Restaurar la sesión previa al arrancar.', en: 'Restore the previous session on startup.' } }],
  },
  {
    key: 'project-session', section: 'core', status: 'shipped',
    title: { es: 'Sesión por proyecto', en: 'Per-project session' },
    natural: {
      es: 'Aparte de la sesión global, runnir recuerda la disposición de paneles y pestañas que usaste en un proyecto y la reconstruye al abrir el terminal ahí. El proyecto es el repositorio git más cercano por encima del directorio de trabajo (o ese directorio si estás fuera de un repo), así que un layout guardado en cualquier punto del repo vuelve para todo el repo. Sólo se restauran la forma de los splits y el cwd de cada panel — nunca el historial ni los procesos.',
      en: 'On top of the global session, runnir remembers the pane and tab layout you last used in a project and rebuilds it when you open the terminal there. The project is the nearest git repository above your working directory (or that directory itself when you are outside a repo), so a layout saved anywhere inside a repo comes back for the whole repo. Only the split shape and each pane’s working directory are restored — never the scrollback and never the running processes.',
    },
    palette: 'Save session for this project / Restore session for this project',
    config: [
      { k: 'behaviour.session_restore', v: 'false', d: { es: 'Reconstruir el layout guardado del proyecto al lanzar runnir dentro de él.', en: 'Rebuild the project’s saved layout when you launch runnir inside it.' } },
      { k: 'behaviour.session_auto_save', v: 'false', d: { es: 'Con session_restore activo, guardar también el layout al salir.', en: 'With session_restore on, also save the layout on exit.' } },
    ],
    example: '[behaviour]\nsession_restore = true\nsession_auto_save = true',
    note: {
      es: 'Ambos apagados por defecto. El store guarda los 50 proyectos más recientes en ~/.config/runnir/sessions.json, escrito de forma atómica.',
      en: 'Both off by default. The store keeps the 50 most recently saved projects in ~/.config/runnir/sessions.json, written atomically.',
    },
  },
  {
    key: 'quake', section: 'core', status: 'shipped',
    title: { es: 'Modo quake (desplegable)', en: 'Quake mode (drop-down)' },
    natural: {
      es: 'Arranca como terminal desplegable que cae desde arriba con una tecla global. Wayland no da atajos globales a las apps, así que el bind lo pone tu compositor; runnir sólo se marca con un app-id conocido para poder apuntarlo con reglas.',
      en: 'Starts as a drop-down terminal that slides down on a global key. Wayland doesn’t hand global shortcuts to apps, so the bind lives in your compositor; runnir just sets a known app-id so you can target it with rules.',
    },
    example: 'runnir --quake   # ventana sin bordes, app-id Wayland: runnir-quake',
    note: {
      es: 'Hyprland: reglas float/size/move + workspace especial y un bind a togglespecialworkspace. Bloque completo en el manual (F1).',
      en: 'Hyprland: float/size/move rules + a special workspace and a bind to togglespecialworkspace. Full block in the in-app manual (F1).',
    },
  },

  // ------------------------------------------------------------- RENDERIZADO
  {
    key: 'ligatures', section: 'rendering', status: 'shipped',
    title: { es: 'Ligaturas', en: 'Ligatures' },
    natural: {
      es: 'Secuencias como -> o != se dibujan como un solo glifo, como hacen de verdad las fuentes monoespaciadas, sin romper la rejilla: cada celda conserva su ancho.',
      en: 'Sequences like -> or != render as a single glyph, the way monospace fonts actually do it, without breaking the grid: every cell keeps its width.',
    },
    config: [{ k: 'font.ligatures', v: 'true', d: { es: 'Activar ligaturas (feature calt de la fuente).', en: 'Enable ligatures (the font’s calt feature).' } }],
    example: 'RUNNIR_NO_LIGATURES=1 runnir   # desactivar sin tocar el config',
    note: {
      es: 'Sólo secuencias ASCII. CJK y emoji mantienen su ruta por carácter.',
      en: 'ASCII sequences only. CJK and emoji stay on their per-character path.',
    },
  },
  {
    key: 'boxdraw', section: 'rendering', status: 'shipped',
    title: { es: 'Caracteres de dibujo de cajas', en: 'Box-drawing characters' },
    natural: {
      es: 'Las líneas y esquinas de htop, tmux y los marcos de TUIs se dibujan por código al tamaño exacto de la celda, no se toman de la fuente, así que las uniones encajan sin huecos. kitty y Ghostty hacen lo mismo.',
      en: 'The lines and corners in htop, tmux and TUI frames are drawn in code at the exact cell size rather than pulled from the font, so joins meet with no gaps. kitty and Ghostty do the same.',
    },
    note: {
      es: 'Incluye líneas de recuadro y bloques de sombreado; generados por boxdraw, no rasterizados de la tipografía.',
      en: 'Covers box lines and shading blocks; generated by boxdraw, not rasterized from the typeface.',
    },
  },
  {
    key: 'inline-images', section: 'rendering', status: 'shipped',
    title: { es: 'Imágenes en línea (protocolo gráfico kitty)', en: 'Inline images (kitty graphics protocol)' },
    natural: {
      es: 'runnir habla el protocolo gráfico de kitty, así que las herramientas que lo usan dibujan imágenes reales en la rejilla: previsualizaciones, gráficas de matplotlib, iconos. Se desplazan con su texto y se reciclan con el historial. runnir responde a la consulta de soporte para que las herramientas lo detecten.',
      en: 'runnir speaks the kitty graphics protocol, so tools that use it draw real images in the grid: previews, matplotlib plots, icons. They scroll with their text and are recycled with the scrollback. runnir answers the support query so tools detect it.',
    },
    example: 'kitten icat foto.png\nchafa --format kitty imagen.jpg',
  },
  {
    key: 'image-watch', section: 'rendering', status: 'shipped',
    title: { es: 'Autoprevisualización de imágenes', en: 'Image auto-preview' },
    natural: {
      es: 'Apunta runnir a un directorio donde escribe tu pipeline de imágenes (SDXL, ComfyUI, Wan) y cada archivo nuevo que suelta se previsualiza en línea en el panel enfocado, escalado para caber. Usa la misma ruta que el protocolo gráfico kitty, así que la vista es idéntica a la de un icat. Sólo disparan los archivos creados o modificados tras armar el watch, así que el contenido previo de la carpeta no inunda el panel; un archivo que aún se está escribiendo se retiene hasta que su tamaño se estabiliza, para no ver una imagen a medias; si llegan varias a la vez, sólo se muestra la más nueva.',
      en: 'Point runnir at a directory your image pipeline writes to (SDXL, ComfyUI, Wan) and every new file it drops is previewed inline in the focused pane, scaled to fit. It reuses the same path as the kitty graphics protocol, so the preview looks exactly like an icat one. Only files created or modified after you arm the watch fire, so the folder’s existing contents never flood the pane; a file still being written is held back until its size settles, so you never see a half-rendered image; when several land at once only the newest is shown.',
    },
    palette: 'Auto-preview images: toggle on this pane\'s dir',
    config: [
      { k: 'watch.enabled', v: 'false', d: { es: 'Armar el watcher al arrancar.', en: 'Arm the watcher at startup.' } },
      { k: 'watch.directory', v: 'null', d: { es: 'Directorio a vigilar; vacío = ninguno todavía (arma desde la paleta el cwd del panel).', en: 'Directory to watch; empty = none yet (arm the pane’s cwd from the palette).' } },
      { k: 'watch.extensions', v: '[ "png", "jpg", "jpeg", "webp" ]', d: { es: 'Extensiones a previsualizar. Lista vacía = cualquier archivo.', en: 'Extensions to preview. Empty list = any file.' } },
      { k: 'watch.max_width', v: '40', d: { es: 'Ancho máximo de la vista en celdas; una imagen mayor se reduce, una menor se deja igual.', en: 'Widest a preview is drawn, in cells; a bigger image scales down, a smaller one is left alone.' } },
    ],
    example: '[watch]\nenabled = true\ndirectory = "~/comfyui/output"\nextensions = [ "png", "jpg", "webp" ]\nmax_width = 40',
    note: {
      es: 'También desde la paleta: "Auto-preview images: set / clear watched dir" teclea un directorio (línea vacía = limpiar). La vista se salta mientras una app de pantalla completa (vim, htop) tiene el panel, y se retoma al salir.',
      en: 'Also from the palette: "Auto-preview images: set / clear watched dir" types a directory (empty line clears it). A preview is skipped while a full-screen app (vim, htop) holds the pane, and resumes once you leave it.',
    },
  },
  {
    key: 'cursor', section: 'rendering', status: 'shipped',
    title: { es: 'Cursor configurable', en: 'Configurable cursor' },
    natural: {
      es: 'Bloque, barra o subrayado, con parpadeo opcional y a la velocidad que quieras.',
      en: 'Block, beam or underline, with optional blink at the rate you set.',
    },
    config: [
      { k: 'cursor.shape', v: 'block', d: { es: 'Forma: block | beam | underline.', en: 'Shape: block | beam | underline.' } },
      { k: 'cursor.blink', v: 'true', d: { es: 'Parpadeo del cursor.', en: 'Cursor blink.' } },
      { k: 'cursor.blink_interval', v: '600', d: { es: 'Milisegundos por fase de parpadeo (mínimo 50).', en: 'Milliseconds per blink phase (min 50).' } },
    ],
  },
  {
    key: 'underline', section: 'rendering', status: 'shipped',
    title: { es: 'Subrayado normal', en: 'Plain underline' },
    natural: {
      es: 'Subrayado clásico vía SGR 4. Base sobre la que se añaden los subrayados de estilo y color (en desarrollo).',
      en: 'Classic underline via SGR 4. The base the styled/colored underlines build on (in development).',
    },
    escape: [R`\e[4m   subrayado on`, R`\e[24m  subrayado off`],
  },
  {
    key: 'underline-styled', section: 'rendering', status: 'dev',
    title: { es: 'Subrayados con estilo y color', en: 'Styled and colored underlines' },
    natural: {
      es: 'Amplía el subrayado a ondulado, punteado, discontinuo o doble, con un color propio distinto del texto. Es lo que hace que neovim o un LSP subrayen en zigzag rojo una palabra sin cambiar el color de la letra.',
      en: 'Extends underline to curly, dotted, dashed or double, with its own color separate from the text. It’s what lets neovim or an LSP draw a red squiggle under a word without recoloring the glyph.',
    },
    escape: [
      R`\e[4:1m  simple    \e[4:2m  doble`,
      R`\e[4:3m  ondulado  \e[4:4m  punteado  \e[4:5m  discontinuo`,
      R`\e[58:2::R:G:Bm  color de subrayado (truecolor)`,
      R`\e[59m  restablecer color de subrayado`,
    ],
    note: {
      es: 'En desarrollo: SGR 4:x y 58/59 (undercurl, dotted, dashed, double y color).',
      en: 'In development: SGR 4:x and 58/59 (undercurl, dotted, dashed, double and color).',
    },
  },

  // ------------------------------------------------------- ENTRADA Y SELECCION
  {
    key: 'leader', section: 'input', status: 'shipped',
    title: { es: 'Capa leader', en: 'Leader layer' },
    natural: {
      es: 'El compositor gana toda carrera de modificadores: Hyprland y GNOME se quedan casi toda la capa Super, y una tecla que ellos capturan no llega nunca a runnir. Por eso runnir monta su propia capa detrás de una tecla leader. Pulsa Alt+Shift+Space y suelta: la barra inferior saca LEADER (o un aviso «leader…» si la ocultaste) y un panel lista qué hace la siguiente tecla, así que la capa se enseña sola. Las teclas calientes actúan al momento; el resto abren un grupo que pide una tecla más. Los modificadores que sigas apretando se ignoran, o sea que puedes no soltar Alt+Shift en toda la secuencia. Esc o cualquier tecla sin atar sale sin filtrar nada al shell. La capa es un superconjunto estricto: todo acorde que exista sigue funcionando, pero muchas acciones (cambiar de pestaña, ciclar layout, copy mode, sesiones de proyecto, salir) solo viven aquí o en la paleta.',
      en: 'The compositor wins every modifier race: Hyprland and GNOME claim most of the Super layer, and a key they grab never reaches runnir. So runnir keeps its own layer behind a leader key. Press Alt+Shift+Space and let go: the bottom bar shows LEADER (or a “leader…” toast if you hid it) and a panel lists what the next key does, so the layer teaches itself. The hot keys act at once; the rest open a group that takes one more key. Modifiers you are still holding are ignored, so you can keep Alt+Shift down through the whole sequence. Esc or any unbound key backs out without leaking to the shell. The layer is a strict superset: every chord that exists still works, but plenty of actions (tab switching, cycle layout, copy mode, project sessions, quit) live only here or in the palette.',
    },
    keys: [
      { es: 'Alt+Shift+Space (armar la capa)', en: 'Alt+Shift+Space (arm the layer)' },
      { es: 'Leader 1..9 / hjkl / HJKL (pestaña, foco, redimensionar)', en: 'Leader 1..9 / hjkl / HJKL (tab, focus, resize)' },
      { es: 'Leader t p c f a r o s (grupos)', en: 'Leader t p c f a r o s (groups)' },
    ],
    config: [
      { k: 'leader', v: '"alt+shift+space"', d: { es: 'Acorde que arma la capa; "" la desactiva entera', en: 'Chord that arms the layer; "" turns it off entirely' } },
      { k: 'leader_timeout', v: '10', d: { es: 'Segundos de espera por paso; 0 = espera lo que haga falta', en: 'Seconds it waits per step; 0 = waits as long as you do' } },
    ],
    note: { es: 'En [keys], el prefijo leader+ ata tus acciones a la capa y el espacio separa los pasos: "leader+r c". Ojo: atar leader+t sustituye el grupo Pestañas entero por esa acción.', en: 'In [keys], a leader+ prefix binds your actions on the layer and a space separates the steps: "leader+r c". Careful: binding leader+t replaces the whole Tabs group with that action.' },
  },
  {
    key: 'keyboard-first', section: 'input', status: 'shipped',
    title: { es: 'Teclado primero', en: 'Keyboard first' },
    natural: {
      es: 'Casi todo tiene atajo; lo que no, vive en la paleta. Los atajos propios usan Ctrl+Shift, Alt+Shift o la capa leader (Alt+Shift+Space y luego una tecla suelta; mientras espera la segunda tecla, la barra inferior muestra LEADER, o un aviso «leader…» si tienes la barra oculta), nunca Ctrl+letra a secas, que pertenece al programa del panel (Ctrl+C, Ctrl+D). Tampoco Super: el compositor se queda esa capa antes de que la tecla llegue a runnir. Y tampoco Alt+Space pelado: es el menú de ventana en Windows y GNOME, y krunner en KDE. Así no se pisa ni lo que espera tu shell ni lo que espera tu escritorio.',
      en: 'Almost everything has a bind; the rest lives in the palette. runnir’s binds use Ctrl+Shift, Alt+Shift or the leader layer (Alt+Shift+Space, then one plain key; while it waits for that second key the bottom bar shows LEADER, or a “leader…” toast if you have the bar hidden), never a bare Ctrl+letter — that belongs to the program in the pane (Ctrl+C, Ctrl+D). Never Super either: the compositor grabs that layer before the key reaches runnir. Nor bare Alt+Space: that is the window menu on Windows and GNOME, and krunner on KDE. So runnir steps on neither what your shell expects nor what your desktop does.',
    },
    keys: [
      { es: 'Ctrl+Shift+P (paleta, todo buscable)', en: 'Ctrl+Shift+P (palette, everything searchable)' },
      { es: 'F1 (manual dentro del terminal)', en: 'F1 (manual inside the terminal)' },
    ],
    palette: 'Command palette',
  },
  {
    key: 'mouse-fullscreen', section: 'input', status: 'shipped',
    title: { es: 'Ratón en apps de pantalla completa', en: 'Mouse in full-screen apps' },
    natural: {
      es: 'Clics, arrastres y rueda se reenvían a los programas que piden el ratón (vim, tmux, htop, less), así que clicar un panel de tmux o un proceso de htop funciona. Mantén Shift para seleccionar texto por encima de la app.',
      en: 'Clicks, drags and the wheel go to programs that ask for the mouse (vim, tmux, htop, less), so clicking a tmux pane or an htop process works. Hold Shift to select text over the app instead.',
    },
    keys: [{ es: 'Shift+arrastrar (forzar selección)', en: 'Shift+drag (force selection)' }],
    escape: [R`\e[?1000h / \e[?1002h / \e[?1006h   el programa pide el ratón (X10 / motion / SGR)`],
  },
  {
    key: 'mouse-select', section: 'input', status: 'shipped',
    title: { es: 'Selección con ratón y copiar/pegar', en: 'Mouse selection and copy/paste' },
    natural: {
      es: 'Arrastra para seleccionar; al soltar se copia solo. Cualquier tecla devuelve la vista al output en vivo, para que no escribas en una pantalla desplazada preguntándote por qué no pasa nada.',
      en: 'Drag to select; the text is copied on release. Any keystroke snaps the view back to live output, so you never type into a scrolled-back screen and wonder why nothing happens.',
    },
    keys: [
      { es: 'Ctrl+Shift+C (copiar)', en: 'Ctrl+Shift+C (copy)' },
      { es: 'Ctrl+Shift+V (pegar)', en: 'Ctrl+Shift+V (paste)' },
    ],
    config: [{ k: 'behaviour.copy_on_select', v: 'true', d: { es: 'Copiar al terminar una selección.', en: 'Copy on completing a selection.' } }],
  },
  {
    key: 'primary-selection', section: 'input', status: 'shipped',
    title: { es: 'Selección primaria (clic central)', en: 'Primary selection (middle click)' },
    natural: {
      es: 'Al estilo Unix: lo último seleccionado queda en la selección primaria y el clic central lo pega. Es independiente del portapapeles, así puedes tener una cosa en Ctrl+Shift+C y otra en el central.',
      en: 'Unix-style: the last thing selected goes into the primary selection and middle click pastes it. Separate from the clipboard, so you can hold one thing in Ctrl+Shift+C and another in the middle button.',
    },
    keys: [{ es: 'Clic central (pega la selección primaria)', en: 'Middle click (pastes the primary selection)' }],
    note: { es: 'Usa wl-copy/wl-paste --primary en Wayland y PRIMARY en X11.', en: 'Uses wl-copy/wl-paste --primary on Wayland and PRIMARY on X11.' },
  },
  {
    key: 'clipboard-history', section: 'input', status: 'shipped',
    title: { es: 'Historial del portapapeles', en: 'Clipboard history' },
    natural: {
      es: 'Cada copia que hace runnir queda en un anillo en memoria, la más reciente primero: selecciones, Ctrl+Shift+C, yanks del copy mode, copiar la última salida, copias del hint mode y escrituras OSC 52 de los programas. Alt+Shift+V (o Leader V) abre un selector difuso; tecleas para filtrar, Enter pega la entrada resaltada en el panel enfocado por la ruta normal de pegado, Esc cierra. Recopiar una entrada la sube al principio en vez de duplicarla.',
      en: 'Every copy runnir makes lands in a small in-memory ring, newest first: selection copies, Ctrl+Shift+C, copy-mode yanks, copy-last-output, hint copies and OSC 52 writes from programs. Alt+Shift+V (or Leader V) opens a fuzzy picker; type to filter, Enter pastes the highlighted entry into the focused pane through the normal paste path, Esc closes. Re-copying an entry moves it to the top instead of duplicating it.',
    },
    keys: ['Alt+Shift+V', 'Leader V'],
    palette: 'Clipboard history',
    config: [
      { k: 'clipboard.capacity', v: '50', d: { es: 'Cuántas copias recientes guarda el historial.', en: 'How many recent copies the history keeps.' } },
      { k: 'clipboard.enabled', v: 'true', d: { es: 'Grabar las copias en el historial.', en: 'Record copies into the history.' } },
    ],
    note: {
      es: 'Sólo en memoria: nunca se escribe a disco, porque el portapapeles suele llevar secretos. Se pierde al cerrar.',
      en: 'In-memory only: never written to disk, since the clipboard often holds secrets. Gone when you close.',
    },
  },
  {
    key: 'copy-mode', section: 'input', status: 'shipped',
    title: { es: 'Copy mode (selección con teclado)', en: 'Copy mode (keyboard selection)' },
    natural: {
      es: 'Selecciona texto del historial sin ratón. Un cursor de teclado, teclas de vim, y la vista se desplaza sola para seguirlo, así seleccionas algo muy arriba sin buscar la rueda.',
      en: 'Select scrollback text with no mouse. A keyboard cursor, vim keys, and the view scrolls to follow it, so you can grab something far up the history without reaching for the wheel.',
    },
    keys: [
      { es: 'h j k l / flechas (mover)', en: 'h j k l / arrows (move)' },
      { es: '0 / $ (inicio / fin de línea)', en: '0 / $ (start / end of line)' },
      { es: 'g / G (arriba / abajo del todo)', en: 'g / G (top / bottom)' },
      { es: 'v o Espacio (empezar selección)', en: 'v or Space (start selection)' },
      { es: 'y o Enter (copiar y salir)', en: 'y or Enter (yank and exit)' },
      { es: 'Esc o q (salir)', en: 'Esc or q (exit)' },
    ],
    palette: 'Copy mode (keyboard select)',
  },
  {
    key: 'rect-selection', section: 'input', status: 'dev',
    title: { es: 'Selección rectangular (por bloque)', en: 'Rectangular (block) selection' },
    natural: {
      es: 'Selecciona un rectángulo en vez de líneas completas: una sola columna de una tabla sin arrastrar el resto de cada fila. Se activa manteniendo Alt (o Ctrl) al arrastrar.',
      en: 'Select a rectangle instead of whole lines: one column of a table without dragging the rest of each row. Hold Alt (or Ctrl) while dragging.',
    },
    keys: [{ es: 'Alt+arrastrar (o Ctrl+arrastrar)', en: 'Alt+drag (or Ctrl+drag)' }],
    note: { es: 'En desarrollo.', en: 'In development.' },
  },
  {
    key: 'hint-mode', section: 'input', status: 'shipped',
    title: { es: 'Hint mode (abrir/copiar sin ratón)', en: 'Hint mode (open/copy without the mouse)' },
    natural: {
      es: 'Etiqueta cada URL, ruta y hash de git en pantalla; tecleas la etiqueta y runnir abre la URL o copia la ruta o el hash.',
      en: 'Labels every URL, path and git hash on screen; type a label and runnir opens the URL or copies the path or hash.',
    },
    keys: ['Ctrl+Shift+Space'],
    palette: 'Hint mode (open/copy on screen)',
  },
  {
    key: 'hover-highlight', section: 'input', status: 'shipped',
    title: { es: 'Resaltado de URL/ruta al pasar por encima', en: 'URL/path highlight on hover' },
    natural: {
      es: 'Pasa el puntero sobre una URL o ruta y se subraya; Ctrl+clic la abre en el navegador o copia la ruta o el hash, sin entrar en hint mode. Respeta también los hyperlinks OSC 8 (ls --hyperlink, gcc, cargo): se abre exactamente el enlace que el programa declaró.',
      en: 'Hover a URL or path and it underlines; Ctrl+click opens it in the browser or copies the path or hash, without entering hint mode. It also honors OSC 8 hyperlinks (ls --hyperlink, gcc, cargo): the exact link the program declared is what opens.',
    },
    keys: [{ es: 'Ctrl+clic (abrir URL / copiar ruta o hash)', en: 'Ctrl+click (open URL / copy path or hash)' }],
  },

  // ---------------------------------------------------- INTEGRACION CON SHELL
  {
    key: 'osc133', section: 'shell', status: 'shipped',
    title: { es: 'Marcas OSC 133 (shell integration)', en: 'OSC 133 marks (shell integration)' },
    natural: {
      es: 'Si tu shell emite OSC 133, runnir sabe dónde empieza el prompt, la entrada y la salida de cada comando. Eso habilita el salto entre comandos, el gutter de estado, el prompt fijo y el plegado. Es la base de casi toda la integración.',
      en: 'If your shell emits OSC 133, runnir knows where each command’s prompt, input and output begin. That powers command jumps, the status gutter, the sticky prompt and folding. It’s the base of nearly all the integration.',
    },
    escape: [
      R`\e]133;A ST   inicio del prompt`,
      R`\e]133;B ST   fin del prompt / inicio de la entrada`,
      R`\e]133;C ST   comando enviado (inicio de la salida)`,
      R`\e]133;D;<codigo> ST   fin del comando, con su código de salida`,
    ],
    example: '# fish (config.fish):\nfunction runnir_prompt --on-event fish_prompt\n  printf \'\\e]133;A\\e\\\\\'\nend\nfunction runnir_preexec --on-event fish_preexec\n  printf \'\\e]133;C\\e\\\\\'\nend\nfunction runnir_postexec --on-event fish_postexec\n  printf \'\\e]133;D\\e\\\\\'\nend',
  },
  {
    key: 'jump-commands', section: 'shell', status: 'shipped',
    title: { es: 'Saltar entre comandos', en: 'Jump between commands' },
    natural: {
      es: 'Sube o baja al prompt del comando anterior o siguiente en vez de desplazarte línea a línea. Otro atajo copia directamente la salida del último comando.',
      en: 'Jump up or down to the previous or next command’s prompt instead of scrolling line by line. Another bind copies the last command’s output outright.',
    },
    keys: [
      { es: 'Ctrl+Shift+Up (comando anterior)', en: 'Ctrl+Shift+Up (previous command)' },
      { es: 'Ctrl+Shift+Down (comando siguiente)', en: 'Ctrl+Shift+Down (next command)' },
      { es: 'Ctrl+Shift+O (copiar la salida del último)', en: 'Ctrl+Shift+O (copy last command output)' },
    ],
    palette: 'Jump to previous command / Jump to next command / Copy last command output',
    note: { es: 'Necesita OSC 133.', en: 'Needs OSC 133.' },
  },
  {
    key: 'status-gutter', section: 'shell', status: 'shipped',
    title: { es: 'Gutter de estado por comando', en: 'Per-command status gutter' },
    natural: {
      es: 'Cada fila de prompt lleva a la izquierda una barra de color: verde si el comando salió con 0, roja si falló, tenue mientras corre. Un historial de éxitos y fallos de un vistazo.',
      en: 'Each prompt row gets a colored bar at the left edge: green if the command exited 0, red if it failed, dim while it runs. A glanceable pass/fail history.',
    },
    note: { es: 'Necesita OSC 133;D con código. Se oculta en la pantalla alternativa (vim, etc.).', en: 'Needs OSC 133;D with an exit code. Hidden on the alternate screen (vim, etc.).' },
  },
  {
    key: 'sticky-prompt', section: 'shell', status: 'shipped',
    title: { es: 'Prompt fijo (sticky)', en: 'Sticky prompt' },
    natural: {
      es: 'Al desplazarte hacia atrás, la línea de prompt del comando cuya salida estás leyendo se queda clavada arriba del panel, así no pierdes de vista qué comando produjo lo que miras.',
      en: 'As you scroll back, the prompt line of the command whose output you’re reading pins to the top of the pane, so you never lose track of which command produced what you’re looking at.',
    },
    note: { es: 'Necesita OSC 133.', en: 'Needs OSC 133.' },
  },
  {
    key: 'osc7-cwd', section: 'shell', status: 'shipped',
    title: { es: 'Directorio actual (OSC 7)', en: 'Current directory (OSC 7)' },
    natural: {
      es: 'Cuando la shell informa de su directorio con OSC 7, un split nuevo se abre ahí y la barra de estado lo muestra. Es la fuente portable del cwd: funciona también en macOS, donde no hay /proc.',
      en: 'When the shell reports its directory via OSC 7, a new split opens there and the status bar shows it. It’s the portable source of the cwd — works on macOS too, where there’s no /proc.',
    },
    escape: [R`\e]7;file://<host>/<ruta> ST   la shell informa de su directorio`],
  },
  {
    key: 'shell-inject', section: 'shell', status: 'dev',
    title: { es: 'Inyección automática de shell integration', en: 'Automatic shell-integration injection' },
    natural: {
      es: 'Inyectar las funciones OSC 133 por sí solo en bash, zsh y fish al arrancar la shell, para que el salto entre comandos, el gutter y el plegado funcionen sin configurar nada.',
      en: 'Injecting the OSC 133 hooks itself into bash, zsh and fish at shell startup, so command jumps, the gutter and folding work with no setup.',
    },
    note: { es: 'En desarrollo. Hoy la integración se añade a mano (ver Marcas OSC 133).', en: 'In development. Today the integration is added by hand (see OSC 133 marks).' },
  },

  // ------------------------------------------------------------ SCROLLBACK
  {
    key: 'search', section: 'scrollback', status: 'shipped',
    title: { es: 'Buscar en el historial', en: 'Search the scrollback' },
    natural: {
      es: 'Busca texto en todo el historial del panel, salta de coincidencia en coincidencia y te dice en cuál estás del total (N/M).',
      en: 'Search the whole pane history, step through matches and see which one you’re on out of the total (N/M).',
    },
    keys: [
      { es: 'Ctrl+Shift+F (buscar)', en: 'Ctrl+Shift+F (search)' },
      { es: 'Enter / Up (siguiente / anterior)', en: 'Enter / Up (next / previous)' },
      { es: 'Esc (cerrar)', en: 'Esc (close)' },
    ],
    palette: 'Search scrollback',
    config: [{ k: 'scrollback.lines', v: '10000', d: { es: 'Líneas de historial por panel (máx. 1.000.000).', en: 'Scrollback lines per pane (max 1,000,000).' } }],
  },
  {
    key: 'minimap', section: 'scrollback', status: 'shipped',
    title: { es: 'Minimapa del historial', en: 'Scrollback minimap' },
    natural: {
      es: 'Una tira estrecha en el borde derecho del panel enfocado que resume el historial, con la parte visible resaltada. Clic en cualquier punto para saltar ahí.',
      en: 'A narrow strip on the right edge of the focused pane summarizing the history, with the visible part highlighted. Click anywhere to jump there.',
    },
    config: [{ k: 'window.minimap', v: 'false', d: { es: 'Minimapa del historial en el borde del panel enfocado; clic para saltar.', en: 'Scrollback minimap on the focused pane’s edge; click to jump.' } }],
  },
  {
    key: 'fold-output', section: 'scrollback', status: 'shipped',
    title: { es: 'Plegar la salida de comandos', en: 'Fold command output' },
    natural: {
      es: 'Colapsa la salida de cada comando terminado en una línea de resumen, así una pantalla de ruido de compilación se vuelve una lista de comandos. Clic en un resumen para desplegar sólo ese. Es sólo vista: no altera el historial.',
      en: 'Collapses each finished command’s output into a one-line summary, so a screen of build noise becomes a list of commands. Click a summary to unfold just that one. View only: it doesn’t alter the history.',
    },
    palette: 'Fold / unfold all command output',
    note: { es: 'Necesita OSC 133.', en: 'Needs OSC 133.' },
  },
  {
    key: 'scrollback-editor', section: 'scrollback', status: 'shipped',
    title: { es: 'Abrir el historial en $EDITOR', en: 'Open scrollback in $EDITOR' },
    natural: {
      es: 'Vuelca el historial del panel a un temporal y lo abre en tu editor ($EDITOR, $VISUAL o vi) en un split nuevo, para buscar, copiar o guardar con tu editor en vez de pelearte con la selección.',
      en: 'Dumps the pane history to a temp file and opens it in your editor ($EDITOR, $VISUAL or vi) in a new split, so you search, copy or save with your editor instead of fighting terminal selection.',
    },
    keys: ['Ctrl+Shift+Q'],
    palette: 'Open scrollback in $EDITOR',
  },
  {
    key: 'pipe-output', section: 'scrollback', status: 'shipped',
    title: { es: 'Pasar la salida o el historial por un comando', en: 'Pipe scrollback / last output' },
    natural: {
      es: 'Desde la paleta, "Pipe last output through command..." abre una entrada donde tecleas un filtro — grep error, sort -u, jq . — y runnir lo corre en un split nuevo con la salida del último comando en stdin. "Pipe scrollback through command..." hace lo mismo pero con todo el historial del panel. El comando corre a través de sh, así que las tuberías y las redirecciones funcionan.',
      en: 'From the palette, "Pipe last output through command..." opens an input where you type a filter — grep error, sort -u, jq . — and runnir runs it in a new split with the last command output on stdin. "Pipe scrollback through command..." does the same but feeds the whole pane scrollback. The command runs through sh, so pipes and redirection work.',
    },
    palette: 'Pipe last output through command... / Pipe scrollback through command...',
    example: 'grep -i error\nsort | uniq -c | sort -rn',
    note: { es: 'La variante de última salida necesita OSC 133 para saber dónde empieza el bloque.', en: 'The last-output variant needs OSC 133 to know where the block starts.' },
  },

  // -------------------------------------------------------------------- IA
  {
    key: 'ai-panel', section: 'ai', status: 'shipped',
    title: { es: 'Panel de asistente IA', en: 'AI assistant panel' },
    natural: {
      es: 'Un asistente sin salir del terminal. Claude corre a través de la CLI de Claude Code contra tu suscripción, sin clave de API. Otros proveedores (OpenAI, Gemini, DeepSeek, Z.ai) usan sus APIs HTTP con la clave tomada de una variable de entorno que nombras en el config; nunca se guarda en el archivo.',
      en: 'An assistant without leaving the terminal. Claude runs through the Claude Code CLI against your subscription — no API key. Other providers (OpenAI, Gemini, DeepSeek, Z.ai) use their HTTP APIs, with the key read from an environment variable named in the config, never stored in the file.',
    },
    keys: [{ es: 'Ctrl+Shift+A (abrir/cerrar)', en: 'Ctrl+Shift+A (toggle)' }],
    palette: 'Toggle AI assistant',
    config: [
      { k: 'ai.default', v: '"claude"', d: { es: 'Qué proveedor usar por defecto.', en: 'Which provider to use by default.' } },
      { k: 'ai.timeout_secs', v: '120', d: { es: 'Segundos antes de abandonar una petición.', en: 'Seconds before giving up on a request.' } },
      { k: 'ai.providers', v: 'claude, openai, gemini, deepseek, zai', d: { es: 'Proveedores predefinidos. Claude Code es subproceso (suscripción); el resto son APIs HTTP (clave por api_key_env).', en: 'Predefined providers. Claude Code is a subprocess (subscription); the rest are HTTP APIs (key via api_key_env).' } },
    ],
  },
  {
    key: 'nl-to-command', section: 'ai', status: 'shipped',
    title: { es: 'Lenguaje natural a comando', en: 'Natural language to command' },
    natural: {
      es: 'Describe lo que quieres y el modelo escribe el comando y lo teclea en el prompt para que lo revises y lo ejecutes tú. No lo ejecuta: lo deja escrito. Útil para esos comandos de tar, ffmpeg o find que nunca recuerdas.',
      en: 'Describe what you want and the model writes the command and types it at the prompt for you to review and run. It doesn’t run it — it leaves it there. Handy for the tar, ffmpeg or find invocations you never remember.',
    },
    keys: ['Ctrl+Shift+M'],
    palette: 'AI: natural language to command',
  },
  {
    key: 'why-failed', section: 'ai', status: 'shipped',
    title: { es: 'Por qué ha fallado esto', en: 'Why did this fail' },
    natural: {
      es: 'Manda al modelo el último comando, su salida y su código de salida, y te explica por qué falló y cómo arreglarlo, sin copiar y pegar el error.',
      en: 'Sends the model the last command, its output and its exit code, and explains why it failed and how to fix it — no copy-pasting the error.',
    },
    keys: ['Ctrl+Shift+G'],
    palette: 'Ask AI: why did this fail?',
    note: { es: 'Necesita OSC 133 para delimitar el último comando y su salida.', en: 'Needs OSC 133 to delimit the last command and its output.' },
  },
  {
    key: 'ai-fix-run', section: 'ai', status: 'shipped',
    title: { es: 'IA: arreglar el último comando', en: 'AI fix-and-run' },
    natural: {
      es: 'Tras un comando fallido, manda al modelo el comando, su salida y su código de salida distinto de cero, y teclea un comando corregido en el prompt para que lo revises y lo ejecutes tú — nunca lo ejecuta. Por ejemplo, después de que mkdr foo falle, teclea mkdir foo en el prompt. Hermano de "por qué ha fallado esto": aquél explica, éste propone el arreglo listo para ejecutar.',
      en: 'After a failed command, it sends the model the command, its output and its non-zero exit code, then types a corrected command at the prompt for you to review and run — it never runs it. For example, after mkdr foo fails it types mkdir foo at the prompt. Sibling of "why did this fail": that one explains, this one proposes the fix ready to run.',
    },
    keys: ['Alt+Shift+G', 'Leader G'],
    palette: 'AI: fix the last failed command',
    note: { es: 'Necesita OSC 133 para delimitar el último comando y su salida.', en: 'Needs OSC 133 to delimit the last command and its output.' },
  },
  {
    key: 'explain-selection', section: 'ai', status: 'shipped',
    title: { es: 'Explicar la selección', en: 'Explain the selection' },
    natural: {
      es: 'Selecciona un trozo de salida, un comando o un fragmento de log y pídele que lo explique en el panel. Para descifrar esa línea de config críptica o un stack trace ajeno.',
      en: 'Select a chunk of output, a command or a log fragment and ask for an explanation in the panel. For decoding a cryptic config line or someone else’s stack trace.',
    },
    keys: ['Ctrl+Shift+Y'],
    palette: 'AI: explain the selection',
  },
  {
    key: 'summarize-session', section: 'ai', status: 'shipped',
    title: { es: 'Resumir la sesión', en: 'Summarize the session' },
    natural: {
      es: 'Un resumen de la sesión: qué comandos ejecutaste, qué resultados dieron, qué errores hubo y cómo se arreglaron.',
      en: 'A summary of the session: which commands you ran, what they returned, what errors came up and how they were fixed.',
    },
    keys: ['Ctrl+Shift+I'],
    palette: 'AI: summarize this session',
  },
  {
    key: 'launch-claude', section: 'ai', status: 'shipped',
    title: { es: 'Lanzar Claude Code', en: 'Launch Claude Code' },
    natural: {
      es: 'Abre Claude Code en un split nuevo para trabajar con el agente en paralelo, sin salir de la ventana.',
      en: 'Opens Claude Code in a new split to work with the agent alongside whatever you’re doing, without leaving the window.',
    },
    keys: ['Ctrl+Shift+N'],
    palette: 'Launch Claude Code',
  },
  {
    key: 'whisper', section: 'ai', status: 'shipped',
    title: { es: 'Whisper (dile al terminal qué hacer)', en: 'Whisper (tell the terminal what to do)' },
    natural: {
      es: 'Abre una barra y dices en lenguaje natural lo que quieres; un modelo lo convierte en acciones de runnir y las ejecuta. Controla al propio runnir, no sólo la shell: una instrucción puede partir paneles, abrir ssh, buscar o lanzar herramientas.',
      en: 'Opens a bar; you say what you want in plain language and a model turns it into runnir actions and runs them. It drives runnir itself, not just the shell: one instruction can split panes, open ssh, search or launch tools.',
    },
    keys: ['Ctrl+Shift+Enter'],
    palette: 'Whisper (tell the terminal what to do)',
    example: 'divide en cuatro y haz ssh a 192.168.1.3, .7, .9 y .188\nbusca la palabra panic en el historial\nhaz la fuente más grande y abre la ayuda',
    note: {
      es: 'Las acciones de runnir se ejecutan al momento; un comando de shell se teclea en el prompt para que lo revises, nunca se ejecuta por ti.',
      en: 'runnir actions run immediately; a shell command it decides on is typed at the prompt for you to review, never run for you.',
    },
  },

  // ------------------------------------------------------- FUNCIONES DISTINTIVAS
  {
    key: 'git-panel', section: 'distinctive', status: 'shipped',
    title: { es: 'Panel de git nativo', en: 'Native git panel' },
    natural: {
      es: 'Leader G abre un panel de git dentro de runnir: status, log, ramas y stashes, con el diff de lo seleccionado al lado. Espacio pone y quita del stage, c escribe el mensaje de commit, P empuja, p tira (solo fast-forward), f hace fetch. Las teclas act\u00faan al momento, sin confirmar \u2014 por eso no hay ninguna que pueda perder trabajo sin commitear: ni reset --hard, ni clean, ni descartar cambios, ni stash drop, ni branch -D. Eso se sigue tecleando en el prompt, donde el guardi\u00e1n pregunta.',
      en: 'Leader G opens a git panel inside runnir: status, log, branches and stashes, with the selection\u2019s diff beside them. Space stages and unstages, c writes the commit message, P pushes, p pulls (fast-forward only), f fetches. Keys act immediately, with no confirmation \u2014 which is why nothing that can lose uncommitted work is bound at all: no reset --hard, no clean, no discard, no stash drop, no branch -D. Those stay at the prompt, where the guardian asks.',
    },
    note: {
      es: 'Los diffs se dibujan con n\u00famero de l\u00ednea y la fila teida entera, no con una columna de + y -, as\u00ed el c\u00f3digo queda alineado con su contexto. Todo git corre en un worker: un push lento no congela la terminal.',
      en: 'Diffs are drawn with line numbers and a full-width tint per line rather than a +/- column, so the code stays aligned with its context. Every git call runs on a worker, so a slow push never freezes the terminal.',
    },
  },
  {
    key: 'file-explorer', section: 'distinctive', status: 'shipped',
    title: { es: 'Explorador de ficheros', en: 'File explorer sidebar' },
    natural: {
      es: 'Leader E abre un árbol del proyecto al lado de los paneles y pone el teclado en él. Es cromo, no una capa modal: sigue visible mientras trabajas en el panel de al lado, y sólo se queda las teclas mientras tiene el foco (Escape las devuelve). La raíz es el repositorio git del directorio del panel enfocado, y se re-ancla sólo cuando cambia el repositorio, nunca en cada cd. Cada fila lleva lo que git dice de ella: M modificado (amarillo) o en el índice (verde), A añadido, D borrado, ? sin seguimiento, ! en conflicto, y un punto en un directorio cuando algo de dentro cambió. Lo que git ignora se oculta, y el pie dice cuántas filas se están conteniendo.',
      en: 'Leader E opens a tree of the project beside the panes and puts the keyboard in it. It is chrome, not a modal layer: it stays up while you work in the pane next to it, and it only takes keys while it has focus (Escape gives them back). The root is the git repository of the focused pane’s directory, and it re-anchors only when that repository changes, never on every cd. Each row carries what git says about it: M modified (yellow) or staged (green), A added, D deleted, ? untracked, ! conflicted, and a dot on a directory when something below it changed. What git ignores is hidden, and the footer says how many rows that is.',
    },
    keys: [
      { es: 'Leader E (abrir y enfocar)', en: 'Leader E (open and focus)' },
      { es: 'j k / flechas (mover), h l (plegar / desplegar)', en: 'j k / arrows (move), h l (fold / unfold)' },
      { es: 'Enter (abrir), e ($EDITOR), o (abrir con el sistema)', en: 'Enter (open), e ($EDITOR), o (the desktop’s handler)' },
      { es: 'p propiedades y permisos, a crear, r renombrar, d borrar', en: 'p properties & permissions, a create, r rename, d delete' },
      { es: 's ordenar por nombre / por fecha, . ocultos, I ignorados por git', en: 's sort by name / by date, . hidden files, I files git ignores' },
      { es: 'y copiar la ruta, R releer, Esc o q volver al panel', en: 'y copy the path, R reread, Esc or q back to the pane' },
      { es: 'Leader (con el árbol enfocado): menú de verbos, sólo lo que la fila puede hacer', en: 'Leader (with the tree focused): a menu of verbs, only what the row can do' },
    ],
    palette: 'File explorer sidebar',
    config: [
      { k: 'explorer.side', v: '"left"', d: { es: 'Lado en el que se dibuja: left o right.', en: 'Which edge it sits on: left or right.' } },
      { k: 'explorer.width', v: '30', d: { es: 'Ancho en COLUMNAS, no en fracción: una fracción en un ultrapanorámico da un árbol de 90 columnas.', en: 'Width in COLUMNS, not a fraction: a fraction on an ultrawide gives a 90-column tree.' } },
      { k: 'explorer.show_hidden', v: 'false', d: { es: 'Mostrar los ficheros que empiezan por punto.', en: 'Show dotfiles.' } },
      { k: 'explorer.open_on_start', v: 'false', d: { es: 'Abrir la barra al arrancar, en cada pestaña.', en: 'Open the sidebar on start, in every tab.' } },
    ],
    note: {
      es: 'Sin editor propio: runnir es una terminal, así que su editor es el que corra en un panel. Nada que se ejecute se ejecuta con una tecla — un script ejecutable levanta un selector (ver / editar / ejecutar / abrir con el sistema) y un binario o un .desktop piden confirmación nombrando qué se lanzaría. Borrar un directorio cuenta lo que hay dentro antes de preguntar, y Enter no cuenta como sí.',
      en: 'No built-in editor: runnir is a terminal, so its editor is whatever runs in a pane. Nothing that RUNS is run by one keypress — an executable script raises a chooser (view / edit / run / open with the system) and a binary or a .desktop asks first, naming what would be launched. Deleting a directory counts what is inside before it asks, and Enter is not a yes.',
    },
  },
  {
    key: 'file-viewer', section: 'distinctive', status: 'shipped',
    title: { es: 'Visor de ficheros e imágenes', en: 'File and image viewer' },
    natural: {
      es: 'Enter sobre un fichero del árbol lo abre, y lo que hace depende de lo que el fichero ES, decidido por sus bytes y no por su nombre: un log sin extensión es texto, un .dat puede serlo y un PNG llamado .txt sigue siendo PNG. El texto se muestra de sólo lectura con números de línea y tabuladores expandidos; una imagen se dibuja como imagen de verdad —textura en la GPU, escalada en el hilo que la decodificó y centrada en el panel—, no como arte de caracteres.',
      en: 'Enter on a file in the tree opens it, and what that means comes from what the file IS, decided by its bytes and not by its name: a log with no extension is text, a .dat may well be, and a PNG called .txt is still a PNG. Text is shown read-only with line numbers and expanded tabs; an image is drawn as a real picture — a GPU texture, scaled on the worker that decoded it and centred in the panel — not as character art.',
    },
    keys: [
      { es: 'j k J K (scroll), h l (lateral), g G (extremos)', en: 'j k J K (scroll), h l (sideways), g G (ends)' },
      { es: 'e ($EDITOR), o (abrir con el sistema), y (copiar la ruta), Esc', en: 'e ($EDITOR), o (open with the system), y (copy the path), Esc' },
    ],
    note: {
      es: 'El arte de medios bloques sigue ahí como respaldo para un fichero que decodifica a arte pero no a píxeles. El visor tiene un tope de 4 MB de texto y lo dice cuando corta; una imagen mayor de 64 MB no se decodifica.',
      en: 'Half-block art is still the fallback for a file that decodes to art but not to pixels. The viewer stops at 4 MB of text and says so; an image file over 64 MB is not decoded at all.',
    },
  },
  {
    key: 'docker-panel', section: 'distinctive', status: 'shipped',
    title: { es: 'Panel de Docker nativo', en: 'Native Docker panel' },
    natural: {
      es: 'Leader D abre tres columnas: los hosts de docker (tus contextos, más Docker Hub como un host propio), los objetos del host seleccionado y el detalle de lo que está marcado. Los contenedores se agrupan por proyecto compose, porque esa es la unidad en la que se trabaja: nadie despliega un contenedor. La salud es una marca propia al lado del estado y nunca se funde con él — "arriba y con el healthcheck rojo" es justo el estado que hay que ver. Todo se lee por el socket del daemon en un hilo aparte, y un contexto remoto se alcanza por el mismo túnel que usa el CLI (ssh <host> docker system dial-stdio). Un host que no responde se dibuja como caído y nunca congela la ventana.',
      en: 'Leader D opens three columns: the docker hosts (your contexts, plus Docker Hub as a host of its own), the objects on the selected host, and the detail of what is selected. Containers are grouped by compose project, because that is the unit the work is done in: nobody deploys a container. Health is its own mark beside the state and is never folded into it — up-and-unhealthy is exactly the state worth seeing. Everything is read over the daemon’s socket on a worker, and a remote context is reached through the same tunnel the CLI uses (ssh <host> docker system dial-stdio). A host that cannot be reached is drawn as down and never freezes the window.',
    },
    keys: [
      { es: 'Leader D (abrir)', en: 'Leader D (open)' },
      { es: 'Tab / h l (columnas), j k (mover), C I V N (contenedores, imágenes, volúmenes, redes)', en: 'Tab / h l (columns), j k (move), C I V N (containers, images, volumes, networks)' },
      { es: 'Enter (plegar un proyecto o leer la selección), u L i (resumen, logs, inspect)', en: 'Enter (fold a project or read the selection), u L i (summary, logs, inspect)' },
      { es: 's x R p K (arrancar, parar, reiniciar, pausar, matar)', en: 's x R p K (start, stop, restart, pause, kill)' },
      { es: 'd (borrar: pregunta nombrando lo que se lleva), e (shell dentro), w (abrir su puerto)', en: 'd (remove: asks, naming what goes with it), e (a shell inside), w (open its port)' },
      { es: 'U W P T (compose up -d, down, pull, desplegar)', en: 'U W P T (compose up -d, down, pull, deploy)' },
      { es: 'Leader (dentro del panel): el mismo menú que el de git, filtrado por la fila', en: 'Leader (inside the panel): the same menu the git panel has, filtered by the row' },
    ],
    palette: 'Docker panel',
    note: {
      es: 'Las operaciones cortas corren sobre el socket y contestan en el pie. Lo que tarda minutos o pinta barras de progreso —una shell, compose up, compose pull— se va a un panel de terminal de verdad, porque un panel ya tiene color, Ctrl-C y scrollback. Cualquier cosa que llegue a otra máquina pregunta antes, nombrando el host.',
      en: 'Short operations run over the socket and answer in the footer. Anything that takes minutes or paints progress bars — a shell, compose up, compose pull — goes to a real terminal pane instead, because a pane already has colour, Ctrl-C and scrollback. Anything that reaches another machine asks first, with the host named.',
    },
  },
  {
    key: 'docker-hub', section: 'distinctive', status: 'shipped',
    title: { es: 'Docker Hub: diff local ↔ publicado y despliegue', en: 'Docker Hub: local ↔ published diff and deploy' },
    natural: {
      es: 'Docker Hub es el último host de la columna. Lista los repositorios que tu login puede ver; cuando la credencial guardada es un token de organización, la API web de Hub lo rechaza (el registry no), así que la lista cae de vuelta a los repositorios que nombran tus imágenes locales — y la cabecera dice cuál de las dos estás mirando. Enter lee los tags de un repositorio y cada tag dice cómo se compara con lo que hay aquí: la misma imagen, otra distinta, no descargada, o construida aquí y nunca publicada. Se compara por DIGEST, nunca por id: el id local es un hash de cómo esta máquina guardó la imagen y no dice nada de lo que tiene un registry.',
      en: 'Docker Hub is the last host in the column. It lists the repositories your login can see; when the stored credential is an organisation token, Hub’s web API refuses it (the registry does not), so the list falls back to the repositories your local images name — and the header says which of the two you are looking at. Enter reads a repository’s tags, and each tag says how it compares with what is here: the same image, a different one, not pulled here, or built here and never pushed. Compared by DIGEST, never by id: a local id is a hash of how this machine stored the image and says nothing about what a registry holds.',
    },
    keys: [
      { es: '> (publicar la imagen o el tag: docker push, pregunta antes)', en: '> (publish the image or tag: docker push, asks first)' },
      { es: 'T sobre un proyecto compose (desplegar: compose pull y luego up -d)', en: 'T on a compose project (deploy: compose pull, then up -d)' },
    ],
    note: {
      es: 'Las credenciales salen de ~/.docker/config.json y su credential helper primero: si ya hubo docker login no hay nada que runnir tenga que guardar. El despliegue es una sola línea encadenada con && a propósito — un up después de un pull fallido rearranca el proyecto con la imagen que ya corría, que es el despliegue que parece que funcionó.',
      en: 'Credentials come from ~/.docker/config.json and its credential helper first: if docker login already happened there is nothing for runnir to store. The deploy is one line chained with && on purpose — an up after a failed pull restarts the project on the image it was already running, which is the deploy that looks like it worked.',
    },
  },
  {
    key: 'command-guardian', section: 'distinctive', status: 'shipped',
    title: { es: 'Guardian de comandos', en: 'Command guardian' },
    natural: {
      es: 'Al pulsar Enter sobre un comando que coincide con un patrón destructivo conocido, runnir se detiene y pide confirmación. Enter confirma; Escape vuelve a la línea. Pilla rm -rf de una raíz o el home, dd sobre un dispositivo, mkfs, DROP/TRUNCATE, git push forzado y la fork bomb. También los git que borran lo que no está en ningún commit: reset --hard, clean -f, checkout de una ruta, restore, stash clear/drop, branch -D, push --delete/--mirror y tirar el reflog. Es una regla, no la IA: instantáneo y sin conexión.',
      en: 'Press Enter on a command matching a known destructive pattern and runnir stops to confirm. Enter runs it; Escape returns to the line. Catches rm -rf of a root or home, dd onto a device, mkfs, SQL DROP/TRUNCATE, git force-push and the fork bomb. Also the git commands that erase what no commit holds: reset --hard, clean -f, checkout of a path, restore, stash clear/drop, branch -D, push --delete/--mirror and dropping the reflog. It’s a rule, not AI: instant and offline.',
    },
    config: [{ k: 'behaviour.command_guardian', v: 'true', d: { es: 'Confirmar comandos destructivos antes de ejecutarlos.', en: 'Confirm destructive commands before running them.' } }],
    note: {
      es: 'Sólo se protege un Enter a secas en el prompt en vivo. Es una red conservadora, no una frontera de seguridad.',
      en: 'Only a bare Enter at the live prompt is guarded. A conservative safety net, not a security boundary.',
    },
  },
  {
    key: 'keyword-watch', section: 'distinctive', status: 'shipped',
    title: { es: 'Watch de palabra clave', en: 'Keyword watch' },
    natural: {
      es: 'Arma el panel enfocado con una palabra: cuando una línea posterior de su salida la contenga, runnir lanza una notificación de escritorio con esa línea. Apúntalo a un build, un deploy o un tail -f y vete a otra cosa.',
      en: 'Arm the focused pane with a word: when a later output line contains it, runnir fires a desktop notification with that line. Point it at a build, a deploy or a tail -f and walk away.',
    },
    palette: 'Watch pane for keyword',
    note: {
      es: 'Coincidencia por subcadena, sin regex. Empieza desde el fondo actual, así el historial viejo no dispara. Una palabra vacía limpia el watch.',
      en: 'Substring match, no regex. Starts from the current bottom, so old history doesn’t trigger. An empty word clears the watch.',
    },
  },
  {
    key: 'named-layouts', section: 'distinctive', status: 'shipped',
    title: { es: 'Layouts con nombre (workspaces)', en: 'Named layouts (workspaces)' },
    natural: {
      es: 'Defines disposiciones con nombre en el config y lanzas una desde la paleta: abre una pestaña nueva con un panel por comando, en mosaico. Un layout "servers" hace ssh a varias máquinas de un tirón.',
      en: 'Define named layouts in the config and launch one from the palette: it opens a fresh tab with one pane per command, tiled. A "servers" layout ssh’s into several machines at once.',
    },
    palette: 'Launch layout',
    config: [{ k: '[[layouts]]', v: 'name + commands[]', d: { es: 'Un panel por comando. Un comando vacío abre un shell normal.', en: 'One pane per command. An empty command opens a plain shell.' } }],
    example: '[[layouts]]\nname = "servers"\ncommands = [ "ssh 192.168.1.3", "ssh 192.168.1.7", "ssh 192.168.1.9", "htop" ]',
    note: {
      es: 'Los comandos se dividen por espacios (no es un parseo de shell completo), lo que cubre "ssh host", "journalctl -f" y similares.',
      en: 'Commands are split on whitespace (not a full shell parse), which covers "ssh host", "journalctl -f" and the like.',
    },
  },
  {
    key: 'snippets', section: 'distinctive', status: 'shipped',
    title: { es: 'Snippets de comandos', en: 'Command snippets' },
    natural: {
      es: 'Guarda en el config los comandos que repites como snippets con nombre y recupéralos desde la paleta (Insert command snippet), con Alt+Shift+S o con Leader O S. Tecleas para filtrar por nombre o descripción; al elegir uno, teclea su comando en el prompt para que lo revises y lo ejecutes tú — la misma regla de revisión previa que el escritor de comandos de la IA, nunca a tus espaldas. Un snippet con run_now = true se envía solo.',
      en: 'Save the commands you run often as named snippets in the config and recall them from the palette (Insert command snippet), with Alt+Shift+S or Leader O S. Type to filter on name or description; selecting one types its command at the prompt for you to check and run yourself — the same review-first rule as the AI command-writer, never behind your back. A snippet with run_now = true submits itself.',
    },
    keys: ['Alt+Shift+S', 'Leader O S'],
    palette: 'Insert command snippet',
    config: [{ k: '[[snippets]]', v: 'name + command + description + run_now', d: { es: 'description y run_now son opcionales; run_now por defecto false (se inserta, no se ejecuta).', en: 'description and run_now are optional; run_now defaults to false (inserted, not executed).' } }],
    example: '[[snippets]]\nname = "deploy"\ncommand = "git push && ssh server bin/deploy"\ndescription = "ship the current branch to prod"\n\n[[snippets]]\nname = "tail"\ncommand = "journalctl -fu runnir"\nrun_now = true',
  },
  {
    key: 'now-playing', section: 'distinctive', status: 'shipped',
    title: { es: 'Reproduciendo ahora (media)', en: 'Now playing (media)' },
    natural: {
      es: 'Ve y controla lo que suena sin salir del terminal. Alt+Shift+P (o Leader R M) abre un overlay con la carátula (medios bloques Unicode de color, así se ve en cualquier GPU), título, artista, álbum, el estado de reproducción y una forma de onda en vivo dibujada con barras Unicode (cava, en Linux). Las teclas multimedia XF86 del teclado (play/pausa, siguiente, anterior) funcionan en cualquier parte, y cada comando está en la paleta (Media: play / pause, etc.). Si no hay reproductor activo, un aviso breve lo dice.',
      en: 'See and control whatever is playing without leaving the terminal. Alt+Shift+P (or Leader R M) opens an overlay with the album art (coloured Unicode half-blocks, so it shows on any GPU), title, artist, album, the playback state and a live waveform drawn as Unicode bars (cava, on Linux). The XF86 media keys on your keyboard (play/pause, next, previous) work anywhere, and every command is in the palette (Media: play / pause, and so on). If no player is active, a brief toast says so.',
    },
    keys: [
      { es: 'Alt+Shift+P o Leader R M (abrir el overlay)', en: 'Alt+Shift+P or Leader R M (open the overlay)' },
      { es: 'Espacio (play / pausa)', en: 'Space (play / pause)' },
      { es: 'n / p (siguiente / anterior)', en: 'n / p (next / previous)' },
      { es: '+ / - (subir / bajar volumen)', en: '+ / - (volume up / down)' },
      { es: 'Esc o q (cerrar)', en: 'Esc or q (close)' },
    ],
    palette: 'Now playing (media overlay)',
    config: [
      { k: 'media.waveform', v: 'true', d: { es: 'Dibujar la onda de cava (no muestra nada si falta cava).', en: 'Draw the cava waveform (shows nothing if cava is absent).' } },
      { k: 'media.bars', v: '24', d: { es: 'Cuántas columnas de onda calcula y dibuja.', en: 'How many wave columns to compute and draw.' } },
      { k: 'media.art_cells', v: '18', d: { es: 'Ancho de la carátula en celdas.', en: 'Album-art width in cells.' } },
    ],
    note: {
      es: 'Requisitos: en Linux, playerctl (cualquier reproductor MPRIS: mpv, Spotify, navegadores, Music Assistant) para metadatos y control, y cava para la onda. En macOS, nowplaying-cli si está, o AppleScript contra Music o Spotify; ahí no hay carátula ni onda. Una herramienta ausente degrada a un aviso o un overlay más simple, nunca a un error.',
      en: 'Requirements: on Linux, playerctl (any MPRIS player: mpv, Spotify, browsers, Music Assistant) for metadata and control, and cava for the waveform. On macOS, nowplaying-cli if present, else AppleScript against Music or Spotify; no art or waveform there. A missing tool degrades to a toast or a plainer overlay, never an error.',
    },
  },
  {
    key: 'broadcast', section: 'distinctive', status: 'shipped',
    title: { es: 'Broadcast (entrada a varios paneles)', en: 'Broadcast (input to many panes)' },
    natural: {
      es: 'Con el broadcast activo, lo que escribes va a todos los paneles de la pestaña a la vez. Con grupos afinas: marcas los paneles concretos y el broadcast se limita a ellos, así emites a tres de cinco y dejas en paz un tail de logs.',
      en: 'With broadcast on, what you type goes to every pane in the tab at once. Groups narrow it: mark specific panes and broadcast limits to them, so you drive three of five and leave a log tail alone.',
    },
    keys: [{ es: 'Ctrl+Shift+B (activar/desactivar broadcast)', en: 'Ctrl+Shift+B (toggle broadcast)' }],
    palette: 'Toggle broadcast input / Toggle pane in broadcast group',
    note: { es: 'Sin miembros de grupo, el broadcast cubre todos los paneles.', en: 'With no group members, broadcast covers every pane.' },
  },
  {
    key: 'context-tint', section: 'distinctive', status: 'shipped',
    title: { es: 'Tintado por contexto (SSH / sudo / docker)', en: 'Context tinting (SSH / sudo / docker)' },
    natural: {
      es: 'runnir vigila el proceso en primer plano de cada panel. Cuando es ssh, tinta el panel de un color derivado del host: el mismo host es siempre el mismo tono, en cualquier máquina, sin configurar nada. sudo/root tintan rojo, docker azul. Lanza el ssh de verdad, así que tu ~/.ssh/config, los jump hosts y el agente de 1Password funcionan sin cambios.',
      en: 'runnir watches each pane’s foreground process. When it’s ssh, the pane is tinted a color derived from the host name: the same host is always the same shade, on any machine, with nothing to configure. sudo/root tint red, docker blue. It launches the real ssh, so your ~/.ssh/config, jump hosts and 1Password agent all work unchanged.',
    },
    keys: [{ es: 'Ctrl+Shift+S (conexión rápida: elige un host de ~/.ssh/config)', en: 'Ctrl+Shift+S (quick connect: pick a host from ~/.ssh/config)' }],
    palette: 'SSH quick connect',
    config: [{ k: 'behaviour.context_tint', v: 'true', d: { es: 'Tintar el fondo según el proceso en primer plano (ssh / sudo / docker).', en: 'Tint the background by foreground process (ssh / sudo / docker).' } }],
  },

  // ------------------------------------------------------------- APARIENCIA
  {
    key: 'transparency', section: 'appearance', status: 'shipped',
    title: { es: 'Transparencia y desenfoque', en: 'Transparency and blur' },
    natural: {
      es: 'Baja la opacidad por debajo de 1.0 y el fondo por defecto deja ver lo de detrás, así una regla de blur de tu compositor surte efecto. El texto y las celdas con fondo explícito se quedan opacos: sólo el fondo por defecto es translúcido.',
      en: 'Drop opacity below 1.0 and the default background lets what’s behind show through, so a blur rule in your compositor takes effect. Text and cells with an explicit background stay opaque: only the default background is translucent.',
    },
    config: [{ k: 'window.opacity', v: '1.0', d: { es: 'Translucidez de la ventana, 0.1..1.0 (necesita compositor; 1.0 = opaco).', en: 'Window translucency, 0.1..1.0 (needs a compositor; 1.0 = opaque).' } }],
    example: '# Hyprland:\ndecoration { blur = yes }\nwindowrulev2 = opacity 0.9, class:^(runnir)$',
    note: { es: 'Pasar la opacidad entre opaco y translúcido es el único ajuste que aún necesita reiniciar.', en: 'Switching opacity between opaque and translucent is the only setting that still needs a restart.' },
  },
  {
    key: 'background-image', section: 'appearance', status: 'shipped',
    title: { es: 'Imagen de fondo', en: 'Background image' },
    natural: {
      es: 'Una imagen detrás del terminal, atenuada al brillo que quieras para que el texto siga leyéndose. Se dibuja por debajo de todo y necesita algo de transparencia para asomar.',
      en: 'An image behind the terminal, dimmed to the brightness you want so text stays readable. Drawn under everything, and it needs some transparency to show through.',
    },
    config: [
      { k: 'window.background', v: 'null', d: { es: 'Ruta a una imagen detrás del terminal (necesita opacity < 1).', en: 'Path to an image behind the terminal (needs opacity < 1).' } },
      { k: 'window.background_dim', v: '0.35', d: { es: 'Cuánto se atenúa la imagen (0 = negro, 1 = brillo completo).', en: 'How much the image is dimmed (0 = black, 1 = full brightness).' } },
    ],
  },
  {
    key: 'themes', section: 'appearance', status: 'shipped',
    title: { es: 'Temas', en: 'Themes' },
    natural: {
      es: 'Un tema oscuro sobrio por defecto (fondo casi negro, acento verde), todo configurable: texto, fondo, cursor, selección, las 16 ANSI y el acento de la propia UI. Colores en hexadecimal, largo (#rrggbb) o corto (#rgb).',
      en: 'A sober dark theme by default (near-black background, green accent), all configurable: text, background, cursor, selection, the 16 ANSI colors and the UI’s own accent. Colors in hex, long (#rrggbb) or short (#rgb).',
    },
    config: [
      { k: 'theme.foreground', v: '#d4d6d9', d: { es: 'Color del texto.', en: 'Text color.' } },
      { k: 'theme.background', v: '#0d0d0f', d: { es: 'Color de fondo (negro casi puro).', en: 'Background color (near-pure black).' } },
      { k: 'theme.accent', v: '#4c9fd4', d: { es: 'Acento de la UI propia (pestañas, paleta, paneles).', en: 'Accent of runnir’s own UI (tabs, palette, panels).' } },
      { k: 'theme.ansi', v: '16 colores', d: { es: 'Las 16 ANSI: 0-7 normales, 8-15 brillantes. El verde 0dbc79 es el acento de marca.', en: 'The 16 ANSI colors: 0-7 normal, 8-15 bright. Green 0dbc79 is the brand accent.' } },
    ],
  },
  {
    key: 'theme-picker', section: 'appearance', status: 'shipped',
    title: { es: 'Selector de temas con vista previa', en: 'Theme picker with live preview' },
    natural: {
      es: 'Un selector con 74 temas incorporados y vista previa en vivo: escribes para filtrar, recorres la lista y ves cada tema aplicado al momento. Cada fila lleva una tira con su paleta. Enter lo guarda en el config, Esc restaura el que tenías.',
      en: 'A picker with 74 built-in themes and live preview: type to filter, scroll the list and see each theme applied on the spot. Every row carries a strip of its palette. Enter saves it to the config, Esc restores the one you had.',
    },
    note: { es: 'Leader O T, o "Theme picker" en la paleta. Las familias completas están: Catppuccin, Tokyo Night, Gruvbox, Rosé Pine, Kanagawa, Nightfox, Everforest, Ayu, Flexoki, Modus, Selenized.', en: 'Leader O T, or "Theme picker" in the palette. Whole families are bundled: Catppuccin, Tokyo Night, Gruvbox, Rosé Pine, Kanagawa, Nightfox, Everforest, Ayu, Flexoki, Modus, Selenized.' },
  },
  {
    key: 'tab-icons', section: 'appearance', status: 'shipped',
    title: { es: 'Iconos y avisos de pestaña', en: 'Tab icons and badges' },
    natural: {
      es: 'Cada pestaña muestra un icono de nerd-font según la app en primer plano y un aviso: punto ámbar si es una pestaña de fondo con salida sin ver, cruz roja si su último comando falló.',
      en: 'Each tab shows a nerd-font icon for its foreground app plus a badge: an amber dot for a background tab with unseen output, a red cross if its last command failed.',
    },
    note: { es: 'La barra de pestañas se desplaza para mantener visible la activa.', en: 'The tab bar scrolls to keep the active tab visible.' },
  },
  {
    key: 'status-bar', section: 'appearance', status: 'shipped',
    title: { es: 'Barra de estado', en: 'Status bar' },
    natural: {
      es: 'Una barra abajo con el directorio actual, la rama de git y el reloj. Cuesta una fila y se puede quitar.',
      en: 'A bar along the bottom with the current directory, git branch and clock. Costs one row and can be turned off.',
    },
    config: [{ k: 'window.status_bar', v: 'true', d: { es: 'Barra inferior (cwd, rama de git, reloj). Cuesta una fila.', en: 'Bottom bar (cwd, git branch, clock). Costs one row.' } }],
  },
  {
    key: 'progress-bar', section: 'appearance', status: 'shipped',
    title: { es: 'Barra de progreso (OSC 9;4)', en: 'Progress bar (OSC 9;4)' },
    natural: {
      es: 'Cuando una herramienta informa de su progreso con OSC 9;4 (descargas, builds, dd con status), runnir dibuja una barra en el borde inferior del panel.',
      en: 'When a tool reports progress via OSC 9;4 (downloads, builds, dd with status), runnir draws a bar along the pane’s bottom edge.',
    },
    escape: [R`\e]9;4;<estado>;<porcentaje> ST   (estado 1 = normal, 2 = error, 0 = limpiar)`],
  },
  {
    key: 'cursor-trail', section: 'appearance', status: 'shipped',
    title: { es: 'Estela del cursor', en: 'Cursor trail' },
    natural: {
      es: 'Al saltar, el cursor deja una breve estela que se desvanece. Adorno, apagado por defecto.',
      en: 'On a jump the cursor leaves a brief fading trail. Decoration, off by default.',
    },
    config: [{ k: 'cursor.trail', v: 'false', d: { es: 'Estela breve que se desvanece detrás del cursor.', en: 'Short fading trail behind the cursor.' } }],
  },
  {
    key: 'smooth-scroll', section: 'appearance', status: 'shipped',
    title: { es: 'Scroll suave', en: 'Smooth scroll' },
    natural: {
      es: 'Los saltos de scroll (al principio, al final, a un prompt) se animan con un deslizamiento en vez de teletransportarse, para que el ojo siga a dónde fue la vista. El scroll de touchpad acumula fracciones de línea para no perder los gestos lentos.',
      en: 'Scroll jumps (to top, to bottom, to a prompt) animate as a glide instead of teleporting, so your eye follows where the view went. Touchpad scroll accumulates sub-line fractions so slow gestures aren’t lost.',
    },
    config: [{ k: 'behaviour.smooth_scroll', v: 'true', d: { es: 'Animar los saltos de scroll con un deslizamiento.', en: 'Animate scroll jumps as a glide.' } }],
  },
  {
    key: 'live-font-size', section: 'appearance', status: 'shipped',
    title: { es: 'Tamaño de fuente en vivo', en: 'Live font size' },
    natural: {
      es: 'Agranda o reduce la fuente al vuelo, sin reiniciar, y vuelve al tamaño configurado cuando quieras.',
      en: 'Grow or shrink the font on the fly, no restart, and snap back to the configured size when you want.',
    },
    keys: [
      { es: 'Ctrl++ (o Ctrl+=) más grande', en: 'Ctrl++ (or Ctrl+=) larger' },
      { es: 'Ctrl+- más pequeña', en: 'Ctrl+- smaller' },
      { es: 'Ctrl+0 restablecer', en: 'Ctrl+0 reset' },
    ],
    palette: 'Increase font size / Decrease font size / Reset font size',
    config: [
      { k: 'font.family', v: '"JetBrainsMono Nerd Font Mono"', d: { es: 'Familia de fuente monoespaciada.', en: 'Monospace font family.' } },
      { k: 'font.size', v: '16.0', d: { es: 'Tamaño base en puntos (4..200).', en: 'Base size in points (4..200).' } },
    ],
  },
  {
    key: 'bell', section: 'appearance', status: 'shipped',
    title: { es: 'Campana visual y sonora', en: 'Visual and audible bell' },
    natural: {
      es: 'Cuando un programa hace sonar la campana (BEL), el panel destella en blanco; si la ventana no tiene el foco, además levanta el aviso de urgencia del compositor. Un build que termina en segundo plano te llama la atención sin robarte el foco.',
      en: 'When a program rings the bell (BEL), the pane flashes white; if the window isn’t focused it also raises the compositor’s urgency hint. A build finishing in the background gets your attention without stealing focus.',
    },
    escape: [R`\a   BEL (0x07): dispara el destello y, sin foco, la urgencia`],
  },

  // ------------------------------------------------------------- PROTOCOLOS
  {
    key: 'osc8', section: 'protocols', status: 'shipped',
    title: { es: 'Hyperlinks OSC 8', en: 'OSC 8 hyperlinks' },
    natural: {
      es: 'Los programas pueden marcar texto como un enlace con una URL (ls --hyperlink, gcc, cargo). runnir lo entiende: al pasar por encima se subraya el enlace exacto declarado y Ctrl+clic lo abre.',
      en: 'Programs can mark text as a link with a URL (ls --hyperlink, gcc, cargo). runnir honors it: hovering underlines the exact declared link and Ctrl+click opens it.',
    },
    escape: [R`\e]8;;https://ejemplo.com ST  texto del enlace  \e]8;; ST`],
  },
  {
    key: 'osc52', section: 'protocols', status: 'dev',
    title: { es: 'Portapapeles OSC 52', en: 'OSC 52 clipboard' },
    natural: {
      es: 'Deja que un programa (incluso por ssh o dentro de tmux) copie texto a tu portapapeles local con una secuencia de escape, sin plugins. runnir lo soporta sólo en escritura: los programas pueden poner texto, no leerlo.',
      en: 'Lets a program (even over ssh or inside tmux) copy text to your local clipboard via an escape sequence, no plugins. runnir supports write-only: programs can set the clipboard, not read it.',
    },
    escape: [R`\e]52;c;<texto-en-base64> ST   escribe <texto> en el portapapeles`],
    note: { es: 'En desarrollo. Sólo escritura, por seguridad: nunca se permite leer el portapapeles.', en: 'In development. Write-only, for safety: reading the clipboard is never allowed.' },
  },
  {
    key: 'osc94-progress', section: 'protocols', status: 'shipped',
    title: { es: 'Progreso OSC 9;4', en: 'OSC 9;4 progress' },
    natural: {
      es: 'El protocolo de progreso (de ConEmu y Windows Terminal) por el que una herramienta informa de su porcentaje. runnir lo pinta como barra en el borde inferior del panel.',
      en: 'The progress protocol (from ConEmu and Windows Terminal) a tool uses to report its percentage. runnir paints it as a bar on the pane’s bottom edge.',
    },
    escape: [R`\e]9;4;1;<0-100> ST  progreso normal`, R`\e]9;4;2;<0-100> ST  error`, R`\e]9;4;0 ST  limpiar`],
  },
  {
    key: 'osc99-notify', section: 'protocols', status: 'dev',
    title: { es: 'Notificaciones OSC 99 / OSC 777', en: 'OSC 99 / OSC 777 notifications' },
    natural: {
      es: 'Deja que un programa lance una notificación de escritorio con título y cuerpo. Soporta el formato moderno (OSC 99, de kitty) y el clásico (OSC 777, de urxvt/Windows Terminal), así funciona con lo que ya emiten muchas herramientas.',
      en: 'Lets a program raise a desktop notification with title and body. Supports the modern format (OSC 99, kitty’s) and the classic one (OSC 777, urxvt/Windows Terminal), so it works with what many tools already emit.',
    },
    escape: [R`\e]99;;<mensaje> ST            notificación (formato kitty)`, R`\e]777;notify;<titulo>;<cuerpo> ST   notificación (formato clásico)`],
    note: { es: 'En desarrollo.', en: 'In development.' },
  },
  {
    key: 'kitty-keyboard', section: 'protocols', status: 'dev',
    title: { es: 'Protocolo de teclado kitty (CSI u)', en: 'Kitty keyboard protocol (CSI u)' },
    natural: {
      es: 'El esquema de codificación moderno que piden neovim y las TUIs para distinguir teclas que el terminal clásico no puede (Esc de Ctrl+[, Tab de Ctrl+I) y reportar pulsaciones que antes se perdían. runnir lo implementa con los modos disambiguate y report-all.',
      en: 'The modern encoding scheme neovim and current TUIs ask for to tell apart keys a classic terminal can’t (Esc from Ctrl+[, Tab from Ctrl+I) and to report presses that used to be lost. runnir implements it with the disambiguate and report-all modes.',
    },
    escape: [R`\e[>1u   push: modo disambiguate`, R`\e[>15u  push: report-all`, R`\e[<u    pop: restaura el modo anterior`, R`\e[?u    consulta el modo activo`],
    note: { es: 'En desarrollo: CSI u para neovim y TUIs modernas.', en: 'In development: CSI u for neovim and modern TUIs.' },
  },
  {
    key: 'kitty-graphics', section: 'protocols', status: 'shipped',
    title: { es: 'Protocolo gráfico kitty', en: 'Kitty graphics protocol' },
    natural: {
      es: 'El protocolo con el que las herramientas dibujan imágenes en la rejilla. Detalle y ejemplos en "Imágenes en línea" (Renderizado).',
      en: 'The protocol tools use to draw images in the grid. Detail and examples under "Inline images" (Rendering).',
    },
    note: { es: 'Las imágenes se desplazan con su texto y se reciclan con el historial. runnir responde a la consulta de soporte.', en: 'Images scroll with their text and recycle with the scrollback. runnir answers the support query.' },
  },

  // ----------------------------------------------------------- AUTOMATIZACION
  {
    key: 'remote-control', section: 'automation', status: 'dev',
    title: { es: 'API de control remoto (runnir @)', en: 'Remote-control API (runnir @)' },
    natural: {
      es: 'Controla una instancia de runnir desde fuera, desde un script u otra terminal, con el subcomando runnir @: lanzar comandos en paneles nuevos, teclear texto, leer lo que hay en pantalla o cambiar de pestaña. Equivalente al remote control de kitty o al CLI de wezterm.',
      en: 'Drive a runnir instance from outside, from a script or another terminal, with the runnir @ subcommand: launch commands in new panes, type text, read what’s on screen or switch tabs. The equivalent of kitty’s remote control or wezterm’s CLI.',
    },
    example: 'runnir @ launch htop           # abre un comando en un panel nuevo\nrunnir @ send-text "ls -la\\n"   # teclea texto en el panel objetivo\nrunnir @ get-text               # lee el contenido visible del panel\nrunnir @ ls                     # lista pestañas y paneles\nrunnir @ focus-tab 2            # enfoca la pestaña N',
    note: { es: 'En desarrollo: subcomandos launch | send-text | get-text | ls | focus-tab.', en: 'In development: launch | send-text | get-text | ls | focus-tab subcommands.' },
  },
  {
    key: 'layout-modes', section: 'automation', status: 'dev',
    title: { es: 'Modos de layout (mosaicos)', en: 'Layout modes (tiling)' },
    natural: {
      es: 'Además de dividir a mano, ordenar los paneles automáticamente en modos de mosaico al estilo de un tiling WM: splits libres, stack, tall, fat y rejilla. Cambias de modo y los paneles se recolocan.',
      en: 'On top of manual splits, arrange panes automatically in tiling modes like a tiling WM: free splits, stack, tall, fat and grid. Switch mode and the panes re-tile.',
    },
    note: { es: 'En desarrollo: modos splits / stack / tall / fat / grid. Hoy los splits se crean y redimensionan a mano.', en: 'In development: splits / stack / tall / fat / grid modes. Today splits are made and resized by hand.' },
  },

  // ----------------------------------------------------------- CONFIGURACION
  {
    key: 'config-file', section: 'config', status: 'shipped',
    title: { es: 'Archivo de configuración (TOML / JSON)', en: 'Configuration file (TOML / JSON)' },
    natural: {
      es: 'Todo vive en ~/.config/runnir/runnir.toml (o un runnir.json, que tiene prioridad). Cada ajuste tiene un valor por defecto, así un archivo parcial o inexistente es normal. Un archivo con un fallo se avisa y se ignora: una errata en un color no te deja sin terminal. Las claves de API se referencian por nombre de variable de entorno, así el archivo es seguro para un repo de dotfiles.',
      en: 'Everything lives in ~/.config/runnir/runnir.toml (or a runnir.json, which wins). Every setting has a default, so a partial or missing file is normal. A broken file is warned about and ignored: a typo in a color never leaves you without a terminal. API keys are referenced by environment-variable name, so the file is safe for a dotfiles repo.',
    },
    example: 'runnir --write-config   # escribe un config por defecto totalmente comentado',
    config: [
      { k: 'window.width / height', v: '1100 / 700', d: { es: 'Tamaño inicial de la ventana en píxeles.', en: 'Initial window size in pixels.' } },
      { k: 'window.decorations', v: 'false', d: { es: 'Mostrar los bordes/título del sistema.', en: 'Show the system window border/title.' } },
      { k: 'behaviour.confirm_close', v: 'true', d: { es: 'Pedir confirmación al cerrar.', en: 'Ask for confirmation on close.' } },
    ],
  },
  {
    key: 'settings-panel', section: 'config', status: 'shipped',
    title: { es: 'Panel de ajustes', en: 'Settings panel' },
    natural: {
      es: 'Un panel interactivo para tocar cada opción sin editar el archivo: flechas para moverte, izquierda/derecha para cambiar un valor, Enter para editar un campo de texto y s para guardar. Al guardar escribe runnir.json (que se carga con preferencia sobre el TOML) y los cambios se aplican en vivo.',
      en: 'An interactive panel to change any option without editing the file: arrows to move, left/right to change a value, Enter to edit a text field, s to save. Saving writes runnir.json (loaded in preference to the TOML) and changes apply live.',
    },
    keys: [
      { es: 'flechas o j/k (mover)', en: 'arrows or j/k (move)' },
      { es: 'izquierda/derecha o h/l (cambiar valor)', en: 'left/right or h/l (change value)' },
      { es: 'Enter (editar campo)', en: 'Enter (edit field)' },
      { es: 's (guardar)', en: 's (save)' },
    ],
    palette: 'Settings',
  },
  {
    key: 'hot-reload', section: 'config', status: 'shipped',
    title: { es: 'Recarga en caliente', en: 'Hot reload' },
    natural: {
      es: 'Guarda el config y runnir aplica el nuevo tema, la fuente y los atajos en menos de un segundo, sin reiniciar. Ante un error de parseo conserva la configuración en uso en vez de saltar a los valores por defecto. El único cambio que aún necesita reiniciar es pasar la opacidad de opaco a translúcido.',
      en: 'Save the config and runnir applies the new theme, font and keybinds in under a second, no restart. On a parse error it keeps the config in use rather than falling back to defaults. The one change that still needs a restart is opacity from opaque to translucent.',
    },
  },
  {
    key: 'custom-keybinds', section: 'config', status: 'shipped',
    title: { es: 'Atajos personalizables', en: 'Custom keybindings' },
    natural: {
      es: 'Reasigna cualquier acción a la combinación que prefieras desde el config; tus atajos se fusionan sobre los de fábrica. Los acordes se escriben "ctrl+shift+t", "alt+enter", "alt+shift+v"; el prefijo "leader+" ata la tecla a la capa leader ("leader+v"), y los espacios separan los pasos de una secuencia ("leader+r c"). Ojo: atar una acción a una tecla de grupo ("leader+t") sustituye el grupo entero. Regla de oro: los atajos propios llevan Ctrl+Shift, Alt+Shift o leader, nunca Ctrl+letra a secas ni Super (esa capa la toma el compositor).',
      en: 'Rebind any action to the combo you like from the config; your binds merge over the defaults. Chords are written "ctrl+shift+t", "alt+enter", "alt+shift+v"; a "leader+" prefix binds the key on the leader layer ("leader+v"), and spaces separate the steps of a sequence ("leader+r c"). Careful: binding an action to a group key ("leader+t") replaces the whole group. Rule of thumb: your own binds use Ctrl+Shift, Alt+Shift or leader, never a bare Ctrl+letter and never Super (the compositor takes that layer).',
    },
    example: '[keys]\n"ctrl+shift+t" = "new_tab"\n"alt+enter" = "toggle_zoom"\n"leader+n" = "new_tab"\n\nleader = "alt+shift+space"   # "" turns the layer off',
    note: { es: 'Cada acción tiene un id estable (ver la página de atajos). go_to_tab_1..9 saltan a la pestaña N.', en: 'Each action has a stable id (see the shortcuts page). go_to_tab_1..9 jump to tab N.' },
  },

  // -------------------------------------------------------------- PLATAFORMA
  {
    key: 'linux-macos', section: 'platform', status: 'shipped',
    title: { es: 'Linux y macOS', en: 'Linux and macOS' },
    natural: {
      es: 'Terminal de GPU escrito desde cero en Rust para Linux y macOS. Necesita una GPU con Vulkan, Metal o DX12 y una fuente monoespaciada. Corre vim, htop y btop dentro. El renderizado es una sola llamada de dibujo (una instancia por celda) y en reposo no consume CPU: espera hasta que algo cambia.',
      en: 'A GPU terminal written from scratch in Rust for Linux and macOS. Needs a GPU capable of Vulkan, Metal or DX12 and a monospace font. Runs vim, htop and btop inside. Rendering is a single draw call (one instance per cell) and it idles at zero CPU: it really waits until something changes.',
    },
    example: 'cargo run                 # compilar y ejecutar\ncargo build --release     # binario optimizado',
    note: { es: 'Fuente por defecto: JetBrainsMono Nerd Font Mono; se sobrescribe con RUNNIR_FONT.', en: 'Default font: JetBrainsMono Nerd Font Mono; override with RUNNIR_FONT.' },
  },
  {
    key: 'headless', section: 'platform', status: 'shipped',
    title: { es: 'Modos headless de verificación', en: 'Headless verification modes' },
    natural: {
      es: 'Para probar y automatizar, runnir corre sin abrir ventana: vuelca la rejilla como texto o la renderiza a PNG. Están separados a propósito para que un fallo del parser nunca se disfrace de fallo de la GPU.',
      en: 'For testing and automation, runnir runs with no window: dump the grid as text or render it to a PNG. They’re kept separate on purpose so a parser bug never masquerades as a GPU bug.',
    },
    example: 'runnir --dump   "<cmd>"                 # corre cmd en un PTY real e imprime la rejilla como texto\nrunnir --render out.png "<cmd>" [ms]    # renderiza la rejilla a PNG sin ventana\nrunnir --demo out.png                   # captura de demostración',
  },

  // ---------------------------------------------------------------- ROADMAP
  {
    key: 'unicode', section: 'roadmap', status: 'dev',
    title: { es: 'Rigor Unicode / grafemas', en: 'Unicode / grapheme rigor' },
    natural: {
      es: 'Tratamiento más fino de los grafemas: emojis compuestos con modificadores (tono de piel, secuencias ZWJ), anchos en casos límite y combinaciones que hoy pueden descolocar la rejilla. Objetivo: que el ancho de cada cosa coincida con lo que esperan el resto de programas.',
      en: 'Finer grapheme handling: composed emoji with modifiers (skin tone, ZWJ sequences), edge-case widths and combinations that can throw off the grid today. Goal: the width of everything matches what other programs expect.',
    },
    note: { es: 'En cola, aún sin empezar.', en: 'Queued, not started yet.' },
  },
  {
    key: 'ime', section: 'roadmap', status: 'dev',
    title: { es: 'IME (métodos de entrada)', en: 'IME (input methods)' },
    natural: {
      es: 'Editores de método de entrada, imprescindibles para escribir chino, japonés o coreano y en general para la composición de caracteres con ventana de candidatos.',
      en: 'Input method editors, required to type Chinese, Japanese or Korean and character composition in general with a candidate window.',
    },
    note: { es: 'En cola, aún sin empezar.', en: 'Queued, not started yet.' },
  },
  {
    key: 'sixel', section: 'roadmap', status: 'dev',
    title: { es: 'Sixel', en: 'Sixel' },
    natural: {
      es: 'Otro protocolo de imágenes en el terminal, más antiguo que el de kitty pero que aún usan bastantes herramientas. Añadirlo amplía la compatibilidad.',
      en: 'Another terminal image protocol, older than kitty’s but still used by a fair number of tools. Adding it widens compatibility.',
    },
    note: { es: 'En cola, aún sin empezar.', en: 'Queued, not started yet.' },
  },
  {
    key: 'text-sizing', section: 'roadmap', status: 'dev',
    title: { es: 'Text sizing (tamaño de texto en línea)', en: 'Text sizing (inline text size)' },
    natural: {
      es: 'El protocolo que permite a un programa pedir texto más grande o más pequeño dentro de la misma pantalla (títulos, superíndices), para presentaciones y TUIs más expresivas.',
      en: 'The protocol that lets a program ask for larger or smaller text within the same screen (headings, superscripts), for presentations and more expressive TUIs.',
    },
    note: { es: 'En cola, aún sin empezar.', en: 'Queued, not started yet.' },
  },
  {
    key: 'triggers', section: 'roadmap', status: 'dev',
    title: { es: 'Triggers (reglas automáticas)', en: 'Triggers (automatic rules)' },
    natural: {
      es: 'Reglas "cuando aparezca este texto, haz esto": resaltar, notificar, lanzar un comando. Generaliza el keyword watch a un motor de reglas configurable.',
      en: 'Rules of the form "when this text appears, do this": highlight, notify, run a command. Generalizes keyword watch into a configurable rule engine.',
    },
    note: { es: 'En cola, aún sin empezar.', en: 'Queued, not started yet.' },
  },
  {
    key: 'command-blocks', section: 'roadmap', status: 'dev',
    title: { es: 'Bloques de comando navegables', en: 'Navigable command blocks' },
    natural: {
      es: 'Tratar cada comando y su salida como un bloque con el que interactuar: plegar, copiar, reejecutar, saltar entre ellos de forma más rica. La evolución del plegado y el salto actuales.',
      en: 'Treat each command and its output as a block to interact with: fold, copy, re-run, jump between them more richly. The evolution of today’s folding and jumping.',
    },
    note: { es: 'En cola, aún sin empezar.', en: 'Queued, not started yet.' },
  },
  {
    key: 'file-transfer', section: 'roadmap', status: 'dev',
    title: { es: 'Transferencia de archivos', en: 'File transfer' },
    natural: {
      es: 'Mover archivos por el propio canal del terminal, típico para traer o llevar ficheros a través de una sesión ssh sin abrir otra herramienta.',
      en: 'Move files over the terminal channel itself, typically to pull or push files across an ssh session without opening another tool.',
    },
    note: { es: 'En cola, aún sin empezar.', en: 'Queued, not started yet.' },
  },
]
