// Referencia de atajos. Derivada de src/actions.rs (default_bindings + default_hints)
// y src/docs.rs. id = identificador de la acción en el config ([keys]).
// group/title son pares { es, en }. keys[] mezcla strings (idénticos: acordes) con
// pares { es, en } cuando la "tecla" es una palabra descriptiva.
// Keybinding reference. group/title are { es, en }. keys[] mixes plain strings
// (identical chords) with { es, en } pairs when the "key" is a descriptive word.
export const KEY_GROUPS = [
  {
    group: { es: 'Pestañas', en: 'Tabs' },
    rows: [
      { keys: ['Ctrl+Shift+T'], id: 'new_tab', title: { es: 'Nueva pestaña', en: 'New tab' } },
      { keys: ['Ctrl+Shift+W'], id: 'close_tab', title: { es: 'Cerrar pestaña', en: 'Close tab' } },
      { keys: ['Ctrl+Tab', 'Ctrl+PageDown'], id: 'next_tab', title: { es: 'Pestaña siguiente', en: 'Next tab' } },
      { keys: ['Ctrl+Shift+Tab', 'Ctrl+PageUp'], id: 'prev_tab', title: { es: 'Pestaña anterior', en: 'Previous tab' } },
      { keys: ['Super+1 .. Super+9'], id: 'go_to_tab_N', title: { es: 'Ir a la pestaña N', en: 'Go to tab N' } },
      { keys: ['Ctrl+Shift+R'], id: 'rename_tab', title: { es: 'Renombrar pestaña', en: 'Rename tab' } },
      { keys: ['Ctrl+Shift+U'], id: 'reopen_closed', title: { es: 'Reabrir pestaña cerrada', en: 'Reopen closed tab' } },
      { keys: ['Ctrl+Shift+Left'], id: 'move_tab_left', title: { es: 'Mover pestaña a la izquierda', en: 'Move tab left' } },
      { keys: ['Ctrl+Shift+Right'], id: 'move_tab_right', title: { es: 'Mover pestaña a la derecha', en: 'Move tab right' } },
    ],
  },
  {
    group: { es: 'Paneles (splits)', en: 'Panes (splits)' },
    rows: [
      { keys: ['Ctrl+Shift+D'], id: 'split_horizontal', title: { es: 'Dividir izquierda/derecha', en: 'Split left/right' } },
      { keys: ['Ctrl+Shift+E'], id: 'split_vertical', title: { es: 'Dividir arriba/abajo', en: 'Split up/down' } },
      { keys: ['Ctrl+Shift+X'], id: 'close_pane', title: { es: 'Cerrar panel', en: 'Close pane' } },
      { keys: ['Ctrl+Shift+H'], id: 'focus_left', title: { es: 'Foco al panel de la izquierda', en: 'Focus left pane' } },
      { keys: ['Ctrl+Shift+J'], id: 'focus_down', title: { es: 'Foco al panel de abajo', en: 'Focus down pane' } },
      { keys: ['Ctrl+Shift+K'], id: 'focus_up', title: { es: 'Foco al panel de arriba', en: 'Focus up pane' } },
      { keys: ['Ctrl+Shift+L'], id: 'focus_right', title: { es: 'Foco al panel de la derecha', en: 'Focus right pane' } },
      { keys: ['Super+Left'], id: 'resize_left', title: { es: 'Encoger panel', en: 'Shrink pane' } },
      { keys: ['Super+Right'], id: 'resize_right', title: { es: 'Agrandar panel', en: 'Grow pane' } },
      { keys: ['Super+Up'], id: 'resize_up', title: { es: 'Agrandar panel hacia arriba', en: 'Grow pane upward' } },
      { keys: ['Super+Down'], id: 'resize_down', title: { es: 'Agrandar panel hacia abajo', en: 'Grow pane downward' } },
      { keys: ['Ctrl+Shift+Z'], id: 'toggle_zoom', title: { es: 'Zoom / des-zoom del panel enfocado', en: 'Zoom / unzoom focused pane' } },
    ],
  },
  {
    group: { es: 'Portapapeles e historial', en: 'Clipboard and scrollback' },
    rows: [
      { keys: ['Ctrl+Shift+C'], id: 'copy', title: { es: 'Copiar selección', en: 'Copy selection' } },
      { keys: ['Ctrl+Shift+V'], id: 'paste', title: { es: 'Pegar', en: 'Paste' } },
      { keys: [{ es: 'Clic central', en: 'Middle click' }], id: '(primaria)', title: { es: 'Pegar la selección primaria', en: 'Paste the primary selection' } },
      { keys: ['Ctrl+Shift+O'], id: 'copy_last_output', title: { es: 'Copiar la salida del último comando', en: 'Copy last command output' } },
      { keys: ['Ctrl+Shift+F'], id: 'search', title: { es: 'Buscar en el historial', en: 'Search the scrollback' } },
      { keys: ['Ctrl+Shift+Q'], id: 'open_scrollback_in_editor', title: { es: 'Abrir el historial en $EDITOR', en: 'Open scrollback in $EDITOR' } },
      { keys: ['Shift+PageUp'], id: 'scroll_page_up', title: { es: 'Subir una página', en: 'Scroll up a page' } },
      { keys: ['Shift+PageDown'], id: 'scroll_page_down', title: { es: 'Bajar una página', en: 'Scroll down a page' } },
      { keys: ['Ctrl+Shift+Home'], id: 'scroll_to_top', title: { es: 'Ir al principio', en: 'Jump to top' } },
      { keys: ['Ctrl+Shift+End'], id: 'scroll_to_bottom', title: { es: 'Ir al output en vivo', en: 'Jump to live output' } },
      { keys: ['Ctrl+Shift+Up'], id: 'jump_prev_prompt', title: { es: 'Saltar al comando anterior', en: 'Jump to previous command' } },
      { keys: ['Ctrl+Shift+Down'], id: 'jump_next_prompt', title: { es: 'Saltar al comando siguiente', en: 'Jump to next command' } },
    ],
  },
  {
    group: { es: 'Fuente', en: 'Font' },
    rows: [
      { keys: ['Ctrl++', 'Ctrl+='], id: 'font_bigger', title: { es: 'Aumentar tamaño de fuente', en: 'Increase font size' } },
      { keys: ['Ctrl+-'], id: 'font_smaller', title: { es: 'Reducir tamaño de fuente', en: 'Decrease font size' } },
      { keys: ['Ctrl+0'], id: 'font_reset', title: { es: 'Restablecer tamaño de fuente', en: 'Reset font size' } },
    ],
  },
  {
    group: { es: 'IA y whisper', en: 'AI and whisper' },
    rows: [
      { keys: ['Ctrl+Shift+A'], id: 'toggle_ai', title: { es: 'Abrir/cerrar el asistente IA', en: 'Toggle the AI assistant' } },
      { keys: ['Ctrl+Shift+G'], id: 'ask_ai_about_error', title: { es: 'IA: por qué ha fallado esto', en: 'AI: why did this fail' } },
      { keys: ['Ctrl+Shift+M'], id: 'ai_command', title: { es: 'IA: lenguaje natural a comando', en: 'AI: natural language to command' } },
      { keys: ['Ctrl+Shift+Y'], id: 'ai_explain', title: { es: 'IA: explicar la selección', en: 'AI: explain the selection' } },
      { keys: ['Ctrl+Shift+I'], id: 'summarize_session', title: { es: 'IA: resumir la sesión', en: 'AI: summarize the session' } },
      { keys: ['Ctrl+Shift+N'], id: 'launch_claude', title: { es: 'Lanzar Claude Code', en: 'Launch Claude Code' } },
      { keys: ['Ctrl+Shift+Enter'], id: 'whisper', title: { es: 'Whisper (dile al terminal qué hacer)', en: 'Whisper (tell the terminal what to do)' } },
    ],
  },
  {
    group: { es: 'Overlays y misceláneos', en: 'Overlays and misc' },
    rows: [
      { keys: ['Ctrl+Shift+P'], id: 'command_palette', title: { es: 'Paleta de comandos', en: 'Command palette' } },
      { keys: ['F1'], id: 'show_docs', title: { es: 'Mostrar el manual', en: 'Show the manual' } },
      { keys: ['Ctrl+Shift+Space'], id: 'hint_mode', title: { es: 'Hint mode (abrir/copiar en pantalla)', en: 'Hint mode (open/copy on screen)' } },
      { keys: ['Ctrl+Shift+S'], id: 'quick_connect', title: { es: 'SSH: conexión rápida', en: 'SSH: quick connect' } },
      { keys: ['Ctrl+Shift+B'], id: 'toggle_broadcast', title: { es: 'Activar/desactivar broadcast', en: 'Toggle broadcast' } },
    ],
  },
  {
    group: { es: 'Sólo en la paleta (sin atajo por defecto)', en: 'Palette only (no default keybind)' },
    rows: [
      { keys: ['Ctrl+Shift+P -> Settings'], id: 'open_config', title: { es: 'Panel de ajustes', en: 'Settings panel' } },
      { keys: [{ es: 'Paleta', en: 'Palette' }], id: 'history_search', title: { es: 'Insertar desde el historial de shell', en: 'Insert from shell history' } },
      { keys: [{ es: 'Paleta', en: 'Palette' }], id: 'watch_keyword', title: { es: 'Vigilar una palabra en el panel', en: 'Watch pane for keyword' } },
      { keys: [{ es: 'Paleta', en: 'Palette' }], id: 'launch_layout', title: { es: 'Lanzar un layout con nombre', en: 'Launch a named layout' } },
      { keys: [{ es: 'Paleta', en: 'Palette' }], id: 'copy_mode', title: { es: 'Copy mode (selección con teclado)', en: 'Copy mode (keyboard select)' } },
      { keys: [{ es: 'Paleta', en: 'Palette' }], id: 'fold_output', title: { es: 'Plegar / desplegar toda la salida', en: 'Fold / unfold all output' } },
      { keys: [{ es: 'Paleta', en: 'Palette' }], id: 'toggle_broadcast_group', title: { es: 'Marcar/desmarcar el panel en el grupo de broadcast', en: 'Toggle pane in the broadcast group' } },
      { keys: [{ es: 'Paleta', en: 'Palette' }], id: 'quit', title: { es: 'Salir de runnir', en: 'Quit runnir' } },
    ],
  },
  {
    group: { es: 'Copy mode (dentro del modo)', en: 'Copy mode (inside the mode)' },
    rows: [
      { keys: ['h j k l', { es: 'flechas', en: 'arrows' }], id: '', title: { es: 'Mover el cursor', en: 'Move the cursor' } },
      { keys: ['0', '$'], id: '', title: { es: 'Inicio / fin de línea', en: 'Start / end of line' } },
      { keys: ['g', 'G'], id: '', title: { es: 'Arriba / abajo del todo', en: 'Top / bottom' } },
      { keys: ['v', { es: 'Espacio', en: 'Space' }], id: '', title: { es: 'Empezar (o soltar) una selección', en: 'Start (or drop) a selection' } },
      { keys: ['y', 'Enter'], id: '', title: { es: 'Copiar la selección y salir', en: 'Yank the selection and exit' } },
      { keys: ['Esc', 'q'], id: '', title: { es: 'Salir del copy mode', en: 'Exit copy mode' } },
    ],
  },
]
