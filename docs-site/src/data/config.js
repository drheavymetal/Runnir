// Referencia completa de configuración. Derivada de src/config.rs: cada opción,
// su valor por defecto y una línea de descripción. Archivo en
// ~/.config/runnir/runnir.toml (o runnir.json, que tiene prioridad).
// group, k, v son idénticos en ambos idiomas (cabeceras TOML, claves, valores).
// d (descripción) es un par { es, en }.
// group, k, v are identical in both languages (TOML headers, keys, values).
// d (description) is an { es, en } pair.
export const CONFIG_GROUPS = [
  {
    group: 'raíz / top-level',
    rows: [
      { k: 'leader', v: '"alt+shift+space"', d: { es: 'Acorde que arma la capa leader: se pulsa, se suelta y luego una tecla suelta (1..9, hjkl, v, g) o una de grupo (t, p, c, f, a, r, o, s) que pide una segunda. Mientras está armada la barra inferior muestra LEADER y un panel lista las opciones. Cadena vacía = capa desactivada. Evita ctrl+alt+space al reasignarlo: ctrl+alt es AltGr en la distribución española.', en: 'Chord that arms the leader layer: press it, release, then one plain key (1..9, hjkl, v, g) or a group key (t, p, c, f, a, r, o, s) that takes a second one. While armed the status bar shows LEADER and a panel lists the options. An empty string turns the layer off. Avoid ctrl+alt+space when rebinding: ctrl+alt is AltGr on the Spanish layout.' } },
      { k: 'leader_timeout', v: '10', d: { es: 'Segundos que espera cada paso de la capa leader antes de caducar. 0 = no caduca nunca (estilo prefijo de tmux): entonces solo se sale con una acción, una tecla no ligada o Esc.', en: 'Seconds each leader step waits before lapsing. 0 = it never lapses (tmux-prefix style): the layer then leaves only on an action, an unbound key, or Esc.' } },
    ],
  },
  {
    group: '[font]',
    rows: [
      { k: 'family', v: '"JetBrainsMono Nerd Font Mono"', d: { es: 'Familia de fuente monoespaciada. Se sobrescribe con la variable RUNNIR_FONT.', en: 'Monospace font family. Overridden by the RUNNIR_FONT variable.' } },
      { k: 'size', v: '16.0', d: { es: 'Tamaño base en puntos. Se limita al rango 4..200.', en: 'Base size in points. Clamped to 4..200.' } },
      { k: 'ligatures', v: 'true', d: { es: 'Activar ligaturas (feature calt de la fuente).', en: 'Enable ligatures (the font’s calt feature).' } },
    ],
  },
  {
    group: '[window]',
    rows: [
      { k: 'width', v: '1100.0', d: { es: 'Ancho inicial de la ventana en píxeles.', en: 'Initial window width in pixels.' } },
      { k: 'height', v: '700.0', d: { es: 'Alto inicial de la ventana en píxeles.', en: 'Initial window height in pixels.' } },
      { k: 'padding', v: '8.0', d: { es: 'Margen interior en píxeles (0..200).', en: 'Inner padding in pixels (0..200).' } },
      { k: 'decorations', v: 'false', d: { es: 'Mostrar los bordes/título de la ventana del sistema.', en: 'Show the system window border/title.' } },
      { k: 'opacity', v: '1.0', d: { es: 'Translucidez de la ventana (0.1..1.0; 1.0 = opaco). Necesita compositor.', en: 'Window translucency (0.1..1.0; 1.0 = opaque). Needs a compositor.' } },
      { k: 'status_bar', v: 'true', d: { es: 'Barra inferior con cwd, rama de git y reloj. Cuesta una fila.', en: 'Bottom bar with cwd, git branch and clock. Costs one row.' } },
      { k: 'background', v: 'null', d: { es: 'Ruta a una imagen dibujada detrás del terminal. Necesita opacity < 1.', en: 'Path to an image drawn behind the terminal. Needs opacity < 1.' } },
      { k: 'background_dim', v: '0.35', d: { es: 'Cuánto se atenúa la imagen de fondo (0 = negro, 1 = brillo completo).', en: 'How much the background image is dimmed (0 = black, 1 = full brightness).' } },
      { k: 'minimap', v: 'false', d: { es: 'Minimapa del historial en el borde del panel enfocado; clic para saltar.', en: 'Scrollback minimap on the focused pane’s edge; click to jump.' } },
    ],
  },
  {
    group: '[cursor]',
    rows: [
      { k: 'shape', v: '"block"', d: { es: 'Forma del cursor: block | beam | underline.', en: 'Cursor shape: block | beam | underline.' } },
      { k: 'blink', v: 'true', d: { es: 'Parpadeo del cursor.', en: 'Cursor blink.' } },
      { k: 'blink_interval', v: '600', d: { es: 'Milisegundos por fase de parpadeo (mínimo 50).', en: 'Milliseconds per blink phase (min 50).' } },
      { k: 'trail', v: 'false', d: { es: 'Estela breve que se desvanece detrás del cursor al saltar.', en: 'Short fading trail behind the cursor on a jump.' } },
    ],
  },
  {
    group: '[scrollback]',
    rows: [
      { k: 'lines', v: '10000', d: { es: 'Líneas de historial por panel (máximo 1.000.000).', en: 'Scrollback lines per pane (max 1,000,000).' } },
    ],
  },
  {
    group: '[theme]',
    rows: [
      { k: 'foreground', v: '"#d4d6d9"', d: { es: 'Color del texto.', en: 'Text color.' } },
      { k: 'background', v: '"#0d0d0f"', d: { es: 'Color de fondo (negro casi puro).', en: 'Background color (near-pure black).' } },
      { k: 'cursor', v: '"#d4d6d9"', d: { es: 'Color del cursor.', en: 'Cursor color.' } },
      { k: 'selection', v: '"#334466"', d: { es: 'Color de la selección.', en: 'Selection color.' } },
      { k: 'accent', v: '"#4c9fd4"', d: { es: 'Acento de la UI propia (pestañas, paleta, paneles).', en: 'Accent of runnir’s own UI (tabs, palette, panels).' } },
      { k: 'dim', v: '"#6a6d74"', d: { es: 'Color tenue.', en: 'Dim color.' } },
      { k: 'ansi', v: '[16 colores]', d: { es: 'Las 16 colores ANSI: 0-7 normales, 8-15 brillantes. Acepta #rrggbb o #rgb.', en: 'The 16 ANSI colors: 0-7 normal, 8-15 bright. Accepts #rrggbb or #rgb.' } },
    ],
  },
  {
    group: '[behaviour]',
    rows: [
      { k: 'copy_on_select', v: 'true', d: { es: 'Copiar automáticamente al terminar una selección.', en: 'Copy automatically on completing a selection.' } },
      { k: 'wheel_lines', v: '3.0', d: { es: 'Líneas por muesca de la rueda (1..50).', en: 'Lines per wheel notch (1..50).' } },
      { k: 'context_tint', v: 'true', d: { es: 'Tintar el fondo según el proceso en primer plano (ssh / sudo / docker).', en: 'Tint the background by foreground process (ssh / sudo / docker).' } },
      { k: 'notify_after_secs', v: '20', d: { es: 'Notificar cuando un comando más largo que esto termine sin foco (0 desactiva).', en: 'Notify when a command longer than this finishes while unfocused (0 disables).' } },
      { k: 'confirm_close', v: 'true', d: { es: 'Pedir confirmación al cerrar.', en: 'Ask for confirmation on close.' } },
      { k: 'restore_session', v: 'true', d: { es: 'Restaurar la sesión previa (pestañas, layout, directorios, historial) al arrancar.', en: 'Restore the previous session (tabs, layout, directories, history) on startup.' } },
      { k: 'command_guardian', v: 'true', d: { es: 'Confirmar comandos destructivos antes de ejecutarlos.', en: 'Confirm destructive commands before running them.' } },
      { k: 'smooth_scroll', v: 'true', d: { es: 'Animar los saltos de scroll con un deslizamiento suave.', en: 'Animate scroll jumps as a smooth glide.' } },
    ],
  },
  {
    group: '[ai]',
    rows: [
      { k: 'default', v: '"claude"', d: { es: 'Qué entrada de "providers" usar por defecto.', en: 'Which "providers" entry to use by default.' } },
      { k: 'timeout_secs', v: '120', d: { es: 'Segundos antes de abandonar una petición.', en: 'Seconds before giving up on a request.' } },
      { k: 'providers', v: 'claude, openai, gemini, deepseek, zai', d: { es: 'Proveedores predefinidos. claude es subproceso (Claude Code, suscripción); el resto son APIs HTTP con la clave en api_key_env.', en: 'Predefined providers. claude is a subprocess (Claude Code, subscription); the rest are HTTP APIs with the key in api_key_env.' } },
    ],
  },
  {
    group: '[keys]',
    rows: [
      { k: '"ctrl+shift+t"', v: '"new_tab"', d: { es: 'Ejemplo: reasignar un atajo. Se fusiona sobre los de fábrica.', en: 'Example: rebind a shortcut. Merges over the defaults.' } },
      { k: 'formato de acorde', v: '"ctrl+shift+X" / "alt+enter" / "alt+shift+v"', d: { es: 'Modificadores: ctrl, shift, alt (opt/option), super (cmd/win/meta). Evita super: el compositor se queda esa capa antes de que la tecla llegue a runnir.', en: 'Modifiers: ctrl, shift, alt (opt/option), super (cmd/win/meta). Avoid super: the compositor grabs that layer before the key reaches runnir.' } },
      { k: '"leader+v"', v: '"clipboard_history"', d: { es: 'El prefijo leader+ ata la tecla a la capa leader, donde va sin modificadores.', en: 'A leader+ prefix binds the key on the leader layer, where it needs no modifiers.' } },
      { k: '"leader+r c"', v: '"launch_claude"', d: { es: 'Secuencia de dos teclas: el espacio separa los pasos. Si el primer paso no existe todavía, se crea como grupo nuevo.', en: 'A two-key sequence: the space separates the steps. If the first step does not exist yet it is created as a new group.' } },
    ],
  },
  {
    group: '[[layouts]]',
    rows: [
      { k: 'name', v: '"servers"', d: { es: 'Nombre del layout, se lanza desde la paleta (Launch layout).', en: 'Layout name, launched from the palette (Launch layout).' } },
      { k: 'commands', v: '[ "ssh host1", "ssh host2", "htop" ]', d: { es: 'Un panel por comando (mosaico). Comando vacío = shell normal. Se divide por espacios.', en: 'One pane per command (tiled). Empty command = plain shell. Split on whitespace.' } },
    ],
  },
]
