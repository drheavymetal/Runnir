// Referencia completa de configuracion. Derivada de src/config.rs: cada opcion,
// su valor por defecto y una linea de descripcion. Archivo en
// ~/.config/runnir/runnir.toml (o runnir.json, que tiene prioridad).
export const CONFIG_GROUPS = [
  {
    group: '[font]',
    rows: [
      { k: 'family', v: '"JetBrainsMono Nerd Font Mono"', d: 'Familia de fuente monoespaciada. Se puede sobrescribir con la variable RUNNIR_FONT.' },
      { k: 'size', v: '16.0', d: 'Tamano base en puntos. Se limita al rango 4..200.' },
      { k: 'ligatures', v: 'true', d: 'Activar ligaturas (feature calt de la fuente).' },
    ],
  },
  {
    group: '[window]',
    rows: [
      { k: 'width', v: '1100.0', d: 'Ancho inicial de la ventana en pixeles.' },
      { k: 'height', v: '700.0', d: 'Alto inicial de la ventana en pixeles.' },
      { k: 'padding', v: '8.0', d: 'Margen interior en pixeles (0..200).' },
      { k: 'decorations', v: 'false', d: 'Mostrar los bordes/titulo de la ventana del sistema.' },
      { k: 'opacity', v: '1.0', d: 'Translucidez de la ventana (0.1..1.0; 1.0 = opaco). Necesita compositor.' },
      { k: 'status_bar', v: 'true', d: 'Barra inferior con cwd, rama de git y reloj. Cuesta una fila.' },
      { k: 'background', v: 'null', d: 'Ruta a una imagen dibujada detras del terminal. Necesita opacity < 1.' },
      { k: 'background_dim', v: '0.35', d: 'Cuanto se atenua la imagen de fondo (0 = negro, 1 = brillo completo).' },
      { k: 'minimap', v: 'false', d: 'Minimapa del historial en el borde del panel enfocado; clic para saltar.' },
    ],
  },
  {
    group: '[cursor]',
    rows: [
      { k: 'shape', v: '"block"', d: 'Forma del cursor: block | beam | underline.' },
      { k: 'blink', v: 'true', d: 'Parpadeo del cursor.' },
      { k: 'blink_interval', v: '600', d: 'Milisegundos por fase de parpadeo (minimo 50).' },
      { k: 'trail', v: 'false', d: 'Estela breve que se desvanece detras del cursor al saltar.' },
    ],
  },
  {
    group: '[scrollback]',
    rows: [
      { k: 'lines', v: '10000', d: 'Lineas de historial por panel (maximo 1.000.000).' },
    ],
  },
  {
    group: '[theme]',
    rows: [
      { k: 'foreground', v: '"#d4d6d9"', d: 'Color del texto.' },
      { k: 'background', v: '"#0d0d0f"', d: 'Color de fondo (negro casi puro).' },
      { k: 'cursor', v: '"#d4d6d9"', d: 'Color del cursor.' },
      { k: 'selection', v: '"#334466"', d: 'Color de la seleccion.' },
      { k: 'accent', v: '"#4c9fd4"', d: 'Acento de la UI propia (pestanas, paleta, paneles).' },
      { k: 'dim', v: '"#6a6d74"', d: 'Color tenue.' },
      { k: 'ansi', v: '[16 colores]', d: 'Las 16 colores ANSI: 0-7 normales, 8-15 brillantes. Acepta #rrggbb o #rgb.' },
    ],
  },
  {
    group: '[behaviour]',
    rows: [
      { k: 'copy_on_select', v: 'true', d: 'Copiar automaticamente al terminar una seleccion.' },
      { k: 'wheel_lines', v: '3.0', d: 'Lineas por muesca de la rueda (1..50).' },
      { k: 'context_tint', v: 'true', d: 'Tintar el fondo segun el proceso en primer plano (ssh / sudo / docker).' },
      { k: 'notify_after_secs', v: '20', d: 'Notificar cuando un comando mas largo que esto termine sin foco (0 desactiva).' },
      { k: 'confirm_close', v: 'true', d: 'Pedir confirmacion al cerrar.' },
      { k: 'restore_session', v: 'true', d: 'Restaurar la sesion previa (pestanas, layout, directorios, historial) al arrancar.' },
      { k: 'command_guardian', v: 'true', d: 'Confirmar comandos destructivos antes de ejecutarlos.' },
      { k: 'smooth_scroll', v: 'true', d: 'Animar los saltos de scroll con un deslizamiento suave.' },
    ],
  },
  {
    group: '[ai]',
    rows: [
      { k: 'default', v: '"claude"', d: 'Que entrada de "providers" usar por defecto.' },
      { k: 'timeout_secs', v: '120', d: 'Segundos antes de abandonar una peticion.' },
      { k: 'providers', v: 'claude, claude-yolo, openai, gemini, deepseek, zai', d: 'Proveedores predefinidos. claude/claude-yolo son subproceso (Claude Code, suscripcion); openai/gemini/deepseek/zai son APIs HTTP con la clave en api_key_env.' },
    ],
  },
  {
    group: '[keys]  (mapa acorde -> id de accion)',
    rows: [
      { k: '"ctrl+shift+t"', v: '"new_tab"', d: 'Ejemplo: reasignar un atajo. Se fusiona sobre los de fabrica.' },
      { k: 'Formato de acorde', v: '"ctrl+shift+X" / "alt+enter" / "super+1"', d: 'Modificadores: ctrl, shift, alt (opt/option), super (cmd/win/meta).' },
    ],
  },
  {
    group: '[[layouts]]  (workspaces con nombre)',
    rows: [
      { k: 'name', v: '"servers"', d: 'Nombre del layout, se lanza desde la paleta (Launch layout).' },
      { k: 'commands', v: '[ "ssh host1", "ssh host2", "htop" ]', d: 'Un panel por comando (mosaico). Comando vacio = shell normal. Se divide por espacios.' },
    ],
  },
]
