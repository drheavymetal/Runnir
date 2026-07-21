// Capturas REALES renderizadas headless con `runnir --render` / `--demo`.
// Clave = key estable de la feature (ver features.js). Valor = { src, cap:{es,en} }.
// Real screenshots rendered headless with runnir --render / --demo.
// Key = the feature's stable key (see features.js). Value = { src, cap:{es,en} }.
export const MEDIA = {
  // Varias capturas: el panel which-key cambia por completo entre el nivel raíz y
  // un grupo, y una sola imagen no lo cuenta. Se generan con
  // `runnir --demo OUT <nivel>`, que dibuja el panel con las mismas entradas que
  // el Keymap real — una captura no puede prometer un atajo que no existe.
  leader: [
    { src: './img/leader.png', cap: { es: 'Captura real (runnir --demo): la capa recién armada. Chip LEADER en la barra y el panel which-key con todo el nivel raíz; en azul los grupos, en amarillo las teclas.', en: 'Real screenshot (runnir --demo): the layer just armed. The LEADER chip in the bar and the which-key panel with the whole root level; groups in blue, keys in yellow.' } },
    { src: './img/leader-t.png', cap: { es: 'Captura real: tras pulsar T, el panel se reduce al grupo Pestañas y la cabecera muestra dónde estás (LEADER t).', en: 'Real screenshot: after pressing T the panel narrows to the Tabs group and the header names where you are (LEADER t).' } },
    { src: './img/leader-f.png', cap: { es: 'Captura real: el grupo Buscar y scroll, con los alias (U/D y AvPág/RePág hacen lo mismo).', en: 'Real screenshot: the Find & scroll group, aliases included (U/D and PageUp/PageDown do the same).' } },
  ],
  tabs: { src: './img/scene.png', cap: { es: 'Captura real (runnir --demo): dos pestañas, varios paneles y la paleta de comandos abierta.', en: 'Real screenshot (runnir --demo): two tabs, several panes and the command palette open.' } },
  splits: { src: './img/scene.png', cap: { es: 'Captura real (runnir --demo): una pestaña dividida en paneles independientes.', en: 'Real screenshot (runnir --demo): a tab split into independent panes.' } },
  'keyboard-first': { src: './img/scene.png', cap: { es: 'Captura real (runnir --demo): la paleta de comandos con cada acción y su atajo.', en: 'Real screenshot (runnir --demo): the command palette with each action and its keybind.' } },
  'ai-panel': { src: './img/scene.png', cap: { es: 'Captura real (runnir --demo): escena multi-panel; el asistente vive en un panel más.', en: 'Real screenshot (runnir --demo): a multi-pane scene; the assistant lives in one more pane.' } },
  themes: { src: './img/colors.png', cap: { es: 'Captura real (runnir --render): las 16 colores ANSI y una rampa truecolor.', en: 'Real screenshot (runnir --render): the 16 ANSI colors and a truecolor ramp.' } },
  ligatures: { src: './img/ligatures.png', cap: { es: 'Captura real (runnir --render): ligaturas de fuente de código (->, =>, !=, >=, <=, ==).', en: 'Real screenshot (runnir --render): code-font ligatures (->, =>, !=, >=, <=, ==).' } },
  boxdraw: { src: './img/boxdraw.png', cap: { es: 'Captura real (runnir --render): recuadros de línea simple y doble más bloques de sombreado, al tamaño de celda.', en: 'Real screenshot (runnir --render): single- and double-line boxes plus shading blocks, at cell size.' } },
  // Estas cuatro son capturas de una VENTANA real (no headless): los paneles
  // hablan con git y con el daemon de docker de la máquina, así que una escena
  // sintética no podría enseñar lo que muestran.
  'file-explorer': { src: './img/file-explorer.png', cap: { es: 'Captura de una ventana real: el árbol del proyecto con los distintivos de git a la derecha de cada fila (M en amarillo = modificado) y el punto en src, que dice que algo de dentro cambió. El pie cuenta lo que se está ocultando.', en: 'Real window screenshot: the project tree with git badges at the right of each row (M in yellow = modified) and the dot on src, which says something below it changed. The footer counts what is being held back.' } },
  'file-viewer': { src: './img/file-viewer.png', cap: { es: 'Captura de una ventana real: una imagen abierta desde el árbol, dibujada como textura de GPU y centrada en el panel — no como arte de caracteres.', en: 'Real window screenshot: an image opened from the tree, drawn as a GPU texture and centred in the panel — not as character art.' } },
  'docker-panel': [
    { src: './img/docker-panel.png', cap: { es: 'Captura de una ventana real: hosts a la izquierda (con desktop-linux marcado como caído), los contenedores agrupados por proyecto compose con su marca de salud, y el resumen del seleccionado.', en: 'Real window screenshot: hosts on the left (with desktop-linux marked as down), containers grouped by compose project with their health mark, and the summary of the selected one.' } },
    { src: './img/docker-leader.png', cap: { es: 'Captura de una ventana real: el menú del leader dentro del panel, con el grupo Container. Sólo ofrece lo que la fila bajo el cursor puede hacer.', en: 'Real window screenshot: the leader menu inside the panel, showing the Container group. It only offers what the row under the cursor can do.' } },
  ],
  underline: { src: './img/underlines.png', cap: { es: 'Captura real (runnir --render): subrayado clásico (SGR 4). Los estilos ondulado/punteado/color son la parte en desarrollo.', en: 'Real screenshot (runnir --render): classic underline (SGR 4). The curly/dotted/colored styles are the in-development part.' } },
}

// Demos ANIMADOS en CSS para funciones dinámicas que una captura estática no
// transmite. Clave = key de la feature. Valor = kind que interpreta <TerminalDemo>.
// Animated CSS demos for dynamic features a static shot can't convey.
// Key = the feature's key. Value = the kind <TerminalDemo> renders.
export const DEMOS = {
  'cursor-trail': 'trail',
  bell: 'bell',
  'smooth-scroll': 'smooth',
  'hover-highlight': 'hover',
  'status-gutter': 'gutter',
  minimap: 'minimap',
}
