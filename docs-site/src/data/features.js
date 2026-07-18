// Todas las funciones de runnir, documentadas. Cada una tiene:
//   section  -> id de SECTIONS
//   title    -> titulo
//   status   -> 'shipped' (ya funciona) | 'dev' (en desarrollo, sin fusionar)
//   natural  -> explicacion en lenguaje llano (por que es util)
//   keys[]   -> combinaciones de teclas por defecto
//   palette  -> entrada del paleta de comandos (Ctrl+Shift+P)
//   config[] -> { k: clave, v: valor por defecto, d: descripcion }
//   escape[] -> secuencias de escape / protocolo, verbatim
//   example  -> ejemplo minimo (comando o snippet)
//   note     -> matiz o limitacion
//
// Las secuencias de escape usan String.raw para conservar las barras invertidas:
//   \e = ESC (0x1b),  \a = BEL (0x07),  ST = terminador de string (ESC \).
const R = String.raw

export const FEATURES = [
  // ------------------------------------------------------------------ NUCLEO
  {
    section: 'core', title: 'Pestanas', status: 'shipped',
    natural: 'Como en un navegador, cada pestana es una sesion de terminal independiente con su propio shell, su historial y su directorio. Abres las que necesites y saltas entre ellas sin perder nada. La barra de pestanas se desplaza sola para mantener visible la activa aunque tengas muchas abiertas.',
    keys: ['Ctrl+Shift+T (nueva)', 'Ctrl+Shift+W (cerrar)', 'Ctrl+PageUp / Ctrl+PageDown (anterior / siguiente)', 'Ctrl+Tab / Ctrl+Shift+Tab (siguiente / anterior)', 'Super+1..9 (ir a la pestana N)', 'Ctrl+Shift+R (renombrar)', 'Ctrl+Shift+Left / Right (mover en la barra)'],
    palette: 'New tab / Close tab / Next tab / Previous tab / Rename tab / Move tab left / Move tab right',
  },
  {
    section: 'core', title: 'Reabrir pestana cerrada', status: 'shipped',
    natural: 'Si cierras una pestana por error, la recuperas al instante con su disposicion de paneles, los directorios de trabajo de cada uno y el historial que tenia. Igual que reabrir una pestana en el navegador. Los procesos no reviven, pero la estructura y lo que se vio si.',
    keys: ['Ctrl+Shift+U'],
    palette: 'Reopen closed tab',
  },
  {
    section: 'core', title: 'Splits (paneles)', status: 'shipped',
    natural: 'Una pestana se divide en paneles y cada panel es su propio shell. Al dividir, el panel nuevo hereda el directorio en el que estabas, asi que se abre justo donde ya estas trabajando. El movimiento de foco es geometrico: "foco a la derecha" va al panel que ves a la derecha, sin importar en que orden creaste los splits.',
    keys: ['Ctrl+Shift+D (dividir izquierda/derecha)', 'Ctrl+Shift+E (dividir arriba/abajo)', 'Ctrl+Shift+X (cerrar panel)', 'Ctrl+Shift+H/J/K/L (mover foco izq/abajo/arriba/der, direcciones vim)', 'Super+flechas (redimensionar el panel)'],
    palette: 'Split pane left/right / Split pane up/down / Close pane',
  },
  {
    section: 'core', title: 'Zoom de panel', status: 'shipped',
    natural: 'Amplia el panel enfocado hasta ocupar toda la pestana para leer una salida larga o concentrarte en un proceso, y vuelve a la disposicion anterior con la misma tecla. Los demas paneles siguen vivos por debajo; solo cambia lo que ves.',
    keys: ['Ctrl+Shift+Z'],
    palette: 'Zoom / unzoom focused pane',
  },
  {
    section: 'core', title: 'Sesiones (restaurar al arrancar)', status: 'shipped',
    natural: 'Al abrir runnir recupera la sesion anterior: las pestanas, la disposicion de paneles, los directorios de trabajo y el texto del historial. Los procesos no sobreviven a un reinicio, pero la estructura de tu espacio de trabajo si, para que retomes justo donde lo dejaste.',
    config: [{ k: 'behaviour.restore_session', v: 'true', d: 'Restaurar la sesion previa (pestanas, layout, directorios, historial) al arrancar.' }],
    note: 'Se puede persistir la sesion de forma explicita; los procesos no reviven, solo el layout y el historial.',
  },
  {
    section: 'core', title: 'Modo quake (desplegable)', status: 'shipped',
    natural: 'Arranca runnir como un terminal desplegable que cae desde arriba de la pantalla al pulsar una tecla global, al estilo de la consola de Quake. Wayland no da atajos globales a las aplicaciones, asi que el atajo lo pone tu compositor; runnir solo se marca con un app-id conocido para que puedas apuntarlo con reglas.',
    example: 'runnir --quake   # ventana sin bordes, app-id Wayland: runnir-quake',
    note: 'Para Hyprland: reglas de ventana float/size/move + workspace especial y un bind a F12 (togglespecialworkspace). Ver el manual F1 para el bloque completo.',
  },

  // ------------------------------------------------------------- RENDERIZADO
  {
    section: 'rendering', title: 'Ligaturas', status: 'shipped',
    natural: 'En fuentes de programacion, ciertas secuencias como la flecha -> o el != se dibujan como un unico simbolo bonito en lugar de dos caracteres sueltos. runnir las implementa como lo hacen de verdad las fuentes monoespaciadas, sin romper la rejilla de caracteres: cada celda sigue ocupando su ancho exacto.',
    config: [{ k: 'font.ligatures', v: 'true', d: 'Activar ligaturas (feature calt de la fuente).' }],
    example: '# activadas por defecto; para desactivarlas sin tocar el config:\nRUNNIR_NO_LIGATURES=1 runnir',
    note: 'Solo secuencias ASCII, como en cualquier fuente de codigo. CJK y emoji mantienen su ruta por caracter.',
  },
  {
    section: 'rendering', title: 'Caracteres de dibujo de cajas', status: 'shipped',
    natural: 'Las lineas y esquinas que usan programas como htop, tmux o los marcos de las TUIs se dibujan a mano al tamano exacto de la celda, no se toman de la fuente. Asi las uniones encajan sin dejar huecos: los marcos quedan continuos y limpios. kitty y Ghostty hacen lo mismo por la misma razon.',
    note: 'Incluye las lineas de recuadro y bloques de sombreado; se generan por codigo (boxdraw), no se rasterizan de la tipografia.',
  },
  {
    section: 'rendering', title: 'Imagenes en linea (protocolo grafico kitty)', status: 'shipped',
    natural: 'runnir entiende el protocolo grafico de kitty, asi que las herramientas que lo hablan dibujan imagenes reales dentro del terminal: vistas previas de fotos, graficas de matplotlib, iconos. Las imagenes se desplazan con el texto que las coloco y desaparecen con el historial cuando este se recicla. runnir responde a la consulta de soporte para que las herramientas lo detecten solas.',
    example: 'kitten icat foto.png\nchafa --format kitty imagen.jpg\n# matplotlib, timg, etc.',
  },
  {
    section: 'rendering', title: 'Cursor configurable', status: 'shipped',
    natural: 'El cursor puede ser un bloque, una barra vertical o un subrayado, con parpadeo opcional y a la velocidad que quieras. Es puramente estetico y de comodidad: eliges la forma que mejor ves.',
    config: [
      { k: 'cursor.shape', v: 'block', d: 'Forma del cursor: block | beam | underline.' },
      { k: 'cursor.blink', v: 'true', d: 'Parpadeo del cursor.' },
      { k: 'cursor.blink_interval', v: '600', d: 'Milisegundos por fase de parpadeo (minimo 50).' },
    ],
  },
  {
    section: 'rendering', title: 'Subrayado normal', status: 'shipped',
    natural: 'runnir dibuja el subrayado clasico que piden los programas (SGR 4). Es la base sobre la que se anaden los subrayados de colores y estilos de la funcion en desarrollo.',
    escape: [R`\e[4m   subrayado activado`, R`\e[24m  subrayado desactivado`],
  },
  {
    section: 'rendering', title: 'Subrayados con estilo y color', status: 'dev',
    natural: 'Amplia el subrayado a los estilos modernos que usan los editores y las TUIs para marcar errores y avisos: ondulado (el clasico "corrector ortografico" en rojo), punteado, discontinuo o doble, y con un color propio distinto del texto. Es lo que hace que neovim o un LSP subrayen en zigzag rojo la palabra mal escrita sin cambiar el color de la letra.',
    escape: [
      R`\e[4:0m  sin subrayado`,
      R`\e[4:1m  simple    \e[4:2m  doble`,
      R`\e[4:3m  ondulado  \e[4:4m  punteado   \e[4:5m  discontinuo`,
      R`\e[58:2::R:G:Bm  color de subrayado (truecolor)   \e[58:5:Nm  (256 colores)`,
      R`\e[59m  restablecer el color de subrayado al del texto`,
    ],
    note: 'Marcado como En desarrollo: soporte de SGR 4:x y 58/59 (undercurl, dotted, dashed, double y color).',
  },

  // ------------------------------------------------------- ENTRADA Y SELECCION
  {
    section: 'input', title: 'Teclado primero', status: 'shipped',
    natural: 'runnir esta pensado para no soltar el teclado: casi todo tiene un atajo, y lo que no, vive en la paleta de comandos. Los atajos propios usan Ctrl+Shift o Super y nunca Ctrl+letra a secas, que pertenece al programa dentro del panel (Ctrl+C, Ctrl+D...). Asi runnir no pisa jamas lo que espera tu shell.',
    keys: ['Ctrl+Shift+P (paleta de comandos, todo es buscable)', 'F1 (manual completo dentro del terminal)'],
    palette: 'Command palette',
  },
  {
    section: 'input', title: 'Raton en aplicaciones de pantalla completa', status: 'shipped',
    natural: 'Los clics, arrastres y la rueda se reenvian a los programas que piden el raton (vim, tmux, htop, less), asi que clicar un panel en tmux o un proceso en htop funciona de verdad. Si en ese momento quieres seleccionar texto en vez de darle el clic al programa, manten Shift y runnir te deja seleccionar por encima de la aplicacion.',
    keys: ['Shift+arrastrar (forzar seleccion dentro de una app de raton)'],
    escape: [R`\e[?1000h / \e[?1002h / \e[?1006h  el programa activa el modo raton (X10 / motion / SGR)`],
  },
  {
    section: 'input', title: 'Seleccion con raton y copiar/pegar', status: 'shipped',
    natural: 'Arrastra para seleccionar; al soltar, el texto se copia solo. Pegar y copiar tienen sus atajos. Cualquier tecla que escribas devuelve la vista al output en vivo, para que nunca escribas dentro de una pantalla desplazada hacia atras y te preguntes por que no pasa nada.',
    keys: ['Ctrl+Shift+C (copiar)', 'Ctrl+Shift+V (pegar)'],
    config: [{ k: 'behaviour.copy_on_select', v: 'true', d: 'Copiar automaticamente al terminar una seleccion.' }],
  },
  {
    section: 'input', title: 'Seleccion primaria (clic central)', status: 'shipped',
    natural: 'Al estilo Unix de siempre: lo ultimo que seleccionas queda en la "seleccion primaria", y un clic con el boton central lo pega. Es independiente del portapapeles normal, asi que puedes tener una cosa copiada con Ctrl+Shift+C y otra distinta lista para pegar con el central.',
    keys: ['Clic central (pega la seleccion primaria)'],
    note: 'Usa wl-copy/wl-paste --primary en Wayland y PRIMARY de X11.',
  },
  {
    section: 'input', title: 'Copy mode (seleccion con teclado)', status: 'shipped',
    natural: 'Selecciona texto del historial sin tocar el raton. Arranca un cursor de teclado que mueves con las teclas de vim; la vista se desplaza sola para seguirlo, asi que puedes seleccionar algo muy arriba en la historia sin buscar la rueda. Ideal para copiar la salida de un comando de hace un rato.',
    keys: ['h j k l / flechas (mover)', '0 / $ (inicio / fin de linea)', 'g / G (arriba / abajo del todo)', 'v o Espacio (empezar seleccion)', 'y o Enter (copiar y salir)', 'Esc o q (salir)'],
    palette: 'Copy mode (keyboard select)',
  },
  {
    section: 'input', title: 'Seleccion rectangular (por bloque)', status: 'dev',
    natural: 'Selecciona un rectangulo de texto en lugar de lineas completas: perfecto para copiar una sola columna de una tabla o una lista alineada sin arrastrar el resto de cada fila. Se activa manteniendo Alt (o Ctrl) mientras arrastras.',
    keys: ['Alt+arrastrar (o Ctrl+arrastrar) para seleccion rectangular'],
    note: 'Marcado como En desarrollo.',
  },
  {
    section: 'input', title: 'Hint mode (abrir/copiar sin raton)', status: 'shipped',
    natural: 'Pone una etiqueta sobre cada URL, ruta y hash de git que hay en pantalla; tecleas la etiqueta y runnir abre la URL en el navegador o copia la ruta o el hash. Elimina casi todos los motivos para acercar la mano al raton.',
    keys: ['Ctrl+Shift+Space'],
    palette: 'Hint mode (open/copy on screen)',
  },
  {
    section: 'input', title: 'Resaltado de URL/ruta al pasar por encima', status: 'shipped',
    natural: 'Al pasar el puntero sobre una URL o una ruta, runnir la subraya, y con Ctrl+clic la abre en el navegador o copia la ruta o el hash, sin entrar en el modo hint. Tambien respeta los hyperlinks OSC 8 explicitos (los que emiten ls --hyperlink, gcc o cargo): se subraya y se abre exactamente el enlace que el programa declaro.',
    keys: ['Ctrl+clic (abrir URL / copiar ruta o hash)'],
  },

  // ---------------------------------------------------- INTEGRACION CON SHELL
  {
    section: 'shell', title: 'Marcas OSC 133 (shell integration)', status: 'shipped',
    natural: 'Si tu shell emite las marcas OSC 133, runnir sabe donde empieza el prompt de cada comando, donde empieza lo que tu escribes y donde empieza y acaba su salida. Ese conocimiento es lo que hace posibles el salto entre comandos, el gutter de estado, el prompt fijo y el plegado de salida. Es la base de casi toda la integracion con la shell.',
    escape: [
      R`\e]133;A ST   inicio del prompt`,
      R`\e]133;B ST   fin del prompt / inicio de la entrada del usuario`,
      R`\e]133;C ST   el comando se ha enviado (inicio de la salida)`,
      R`\e]133;D;<codigo> ST   fin del comando, con su codigo de salida`,
    ],
    example: '# fish (config.fish):\nfunction runnir_prompt --on-event fish_prompt\n  printf \'\\e]133;A\\e\\\\\'\nend\nfunction runnir_preexec --on-event fish_preexec\n  printf \'\\e]133;C\\e\\\\\'\nend\nfunction runnir_postexec --on-event fish_postexec\n  printf \'\\e]133;D\\e\\\\\'\nend',
  },
  {
    section: 'shell', title: 'Saltar entre comandos', status: 'shipped',
    natural: 'Sube o baja de golpe al prompt del comando anterior o siguiente, en vez de desplazarte linea a linea buscando donde empezo aquella orden. Y con un atajo copias directamente la salida del ultimo comando, sin seleccionar a mano.',
    keys: ['Ctrl+Shift+Up (comando anterior)', 'Ctrl+Shift+Down (comando siguiente)', 'Ctrl+Shift+O (copiar la salida del ultimo comando)'],
    palette: 'Jump to previous command / Jump to next command / Copy last command output',
    note: 'Necesita integracion OSC 133.',
  },
  {
    section: 'shell', title: 'Gutter de estado por comando', status: 'shipped',
    natural: 'Cada fila de prompt lleva a la izquierda una pequena barra de color: verde si el comando salio bien (codigo 0), roja si fallo, tenue mientras se ejecuta. De un vistazo ves el historial de exitos y fallos de la sesion sin leer un solo codigo de salida.',
    note: 'Necesita OSC 133;D con codigo. Se oculta en la pantalla alternativa (vim, etc.).',
  },
  {
    section: 'shell', title: 'Prompt fijo (sticky)', status: 'shipped',
    natural: 'Mientras te desplazas hacia atras por el historial, la linea de prompt del comando cuya salida estas leyendo se queda clavada arriba del panel. Asi nunca pierdes de vista que comando produjo lo que estas mirando. Es automatico.',
    note: 'Necesita integracion OSC 133.',
  },
  {
    section: 'shell', title: 'Directorio actual (OSC 7)', status: 'shipped',
    natural: 'Cuando la shell informa de su directorio de trabajo con OSC 7, runnir lo sabe y lo usa: por ejemplo, un split nuevo se abre en ese directorio y la barra de estado muestra donde estas. Es la fuente portable del directorio, funciona tambien en macOS, donde no existe /proc.',
    escape: [R`\e]7;file://<host>/<ruta> ST   la shell informa de su directorio`],
  },
  {
    section: 'shell', title: 'Inyeccion automatica de shell integration', status: 'dev',
    natural: 'Para no tener que pegar a mano las funciones de OSC 133 en la configuracion de tu shell, runnir inyecta esa integracion por si solo en bash, zsh y fish al arrancar el shell. Asi el saltar entre comandos, el gutter de estado y el plegado funcionan sin que configures nada.',
    note: 'Marcado como En desarrollo: inyeccion automatica para bash / zsh / fish. Hoy la integracion se anade a mano (ver Marcas OSC 133).',
  },

  // ------------------------------------------------------------ SCROLLBACK
  {
    section: 'scrollback', title: 'Buscar en el historial', status: 'shipped',
    natural: 'Busca cualquier texto en todo el historial del panel. Salta de una coincidencia a la siguiente y te dice en cual estas del total (N/M). Perfecto para encontrar aquel error o aquella URL que paso hace mil lineas.',
    keys: ['Ctrl+Shift+F (buscar)', 'Enter / Up (siguiente / anterior)', 'Esc (cerrar)'],
    palette: 'Search scrollback',
    config: [{ k: 'scrollback.lines', v: '10000', d: 'Cuantas lineas de historial guarda cada panel (maximo 1.000.000).' }],
  },
  {
    section: 'scrollback', title: 'Minimapa del historial', status: 'shipped',
    natural: 'Una tira estrecha en el borde derecho del panel enfocado que resume todo el historial de un vistazo, con la parte visible resaltada. Haz clic en cualquier punto para saltar ahi. Es como el minimapa de codigo de un editor, pero para tu terminal.',
    config: [{ k: 'window.minimap', v: 'false', d: 'Mostrar un minimapa del historial en el borde del panel enfocado; clic para saltar.' }],
  },
  {
    section: 'scrollback', title: 'Plegar la salida de comandos', status: 'shipped',
    natural: 'Colapsa la salida de cada comando terminado en una linea de resumen ("N lineas plegadas"), de modo que una pantalla llena de ruido de compilacion se convierte en una lista limpia de comandos. Haz clic en una linea de resumen para desplegar solo esa. Es solo vista: no altera el historial real.',
    palette: 'Fold / unfold all command output',
    note: 'Necesita OSC 133. Al hacer clic en el resumen se despliega ese bloque.',
  },
  {
    section: 'scrollback', title: 'Abrir el historial en $EDITOR', status: 'shipped',
    natural: 'Vuelca todo el historial del panel a un archivo temporal y lo abre en tu editor ($EDITOR, $VISUAL o vi) en un split nuevo. Asi puedes buscar, copiar o guardar la salida con todas las comodidades de tu editor en lugar de pelearte con la seleccion en el terminal.',
    keys: ['Ctrl+Shift+Q'],
    palette: 'Open scrollback in $EDITOR',
  },

  // -------------------------------------------------------------------- IA
  {
    section: 'ai', title: 'Panel de asistente IA', status: 'shipped',
    natural: 'runnir habla con un asistente sin salir del terminal. Claude corre a traves de la CLI de Claude Code contra tu suscripcion, sin clave de API. Otros proveedores (OpenAI, Gemini, DeepSeek, Z.ai) usan sus APIs HTTP, con la clave tomada de una variable de entorno que nombras en el config; la clave nunca se guarda en el archivo.',
    keys: ['Ctrl+Shift+A (abrir/cerrar el panel)'],
    palette: 'Toggle AI assistant',
    config: [
      { k: 'ai.default', v: '"claude"', d: 'Que proveedor de "providers" usar por defecto.' },
      { k: 'ai.timeout_secs', v: '120', d: 'Segundos antes de abandonar una peticion.' },
      { k: 'ai.providers', v: 'claude, claude-yolo, openai, gemini, deepseek, zai', d: 'Proveedores predefinidos. Claude Code es un subproceso (suscripcion); el resto son APIs HTTP (clave por variable de entorno via api_key_env).' },
    ],
  },
  {
    section: 'ai', title: 'Lenguaje natural a comando', status: 'shipped',
    natural: 'Describe en tu idioma lo que quieres hacer y el modelo escribe el comando y lo teclea en el prompt para que lo revises y lo ejecutes tu. No lo ejecuta por ti: lo deja escrito y tu decides. Ideal para esos comandos de tar, ffmpeg o find que nunca recuerdas de memoria.',
    keys: ['Ctrl+Shift+M'],
    palette: 'AI: natural language to command',
  },
  {
    section: 'ai', title: 'Por que ha fallado esto', status: 'shipped',
    natural: 'Envia al modelo el ultimo comando, su salida y su codigo de salida, y te explica por que fallo y como arreglarlo, sin que tengas que copiar y pegar el error en ningun sitio.',
    keys: ['Ctrl+Shift+G'],
    palette: 'Ask AI: why did this fail?',
    note: 'Necesita OSC 133 para delimitar el ultimo comando y su salida.',
  },
  {
    section: 'ai', title: 'Explicar la seleccion', status: 'shipped',
    natural: 'Selecciona un trozo de salida, un comando raro o un fragmento de log y pide que te lo explique en el panel del asistente. Util para descifrar esa linea de configuracion cryptica o ese stack trace ajeno.',
    keys: ['Ctrl+Shift+Y'],
    palette: 'AI: explain the selection',
  },
  {
    section: 'ai', title: 'Resumir la sesion', status: 'shipped',
    natural: 'Pide un resumen conciso de toda la sesion: que comandos ejecutaste, que resultados dieron, que errores hubo y como se arreglaron. Perfecto para dejar constancia de lo que hiciste o para retomar el hilo tras un rato.',
    keys: ['Ctrl+Shift+I'],
    palette: 'AI: summarize this session',
  },
  {
    section: 'ai', title: 'Lanzar Claude Code', status: 'shipped',
    natural: 'Abre Claude Code en un split nuevo directamente, para trabajar con el agente en paralelo a lo que estas haciendo, sin salir de la ventana.',
    keys: ['Ctrl+Shift+N'],
    palette: 'Launch Claude Code',
  },
  {
    section: 'ai', title: 'Whisper (dile al terminal que hacer)', status: 'shipped',
    natural: 'Abre una barra y dices en lenguaje natural lo que quieres; un modelo lo convierte en acciones de runnir y runnir las ejecuta. Y no controla solo la shell: controla al propio runnir. Una sola instruccion puede partir paneles, abrir sesiones ssh, buscar o lanzar herramientas. El nombre encaja: "run" es un susurro a la maquina.',
    keys: ['Ctrl+Shift+Enter'],
    palette: 'Whisper (tell the terminal what to do)',
    example: 'divide en cuatro y haz ssh a 192.168.1.3, .7, .9 y .188\nbusca la palabra panic en el historial\nhaz la fuente mas grande y abre la ayuda',
    note: 'Las acciones de runnir se ejecutan al momento; un comando de shell que decida se teclea en el prompt para que lo revises, nunca se ejecuta por ti.',
  },

  // ------------------------------------------------------- FUNCIONES DISTINTIVAS
  {
    section: 'distinctive', title: 'Command guardian', status: 'shipped',
    natural: 'Cuando pulsas Enter sobre un comando que coincide con un patron destructivo conocido, runnir se detiene y te pide confirmacion en vez de ejecutarlo a ciegas. Enter confirma y lo ejecuta; Escape te devuelve a la linea para corregirlo o cancelarlo. Pilla cosas como borrados recursivos forzados de una ruta raiz o del home, dd sobre un dispositivo, mkfs, DROP/TRUNCATE de SQL, git push forzado y la clasica fork bomb. Es una regla, no la IA: instantaneo y sin conexion.',
    config: [{ k: 'behaviour.command_guardian', v: 'true', d: 'Confirmar comandos que coinciden con un patron destructivo antes de ejecutarlos.' }],
    note: 'Solo se protege un Enter a secas en el prompt en vivo; editar el historial y las apps de pantalla completa quedan intactos. Es una red de seguridad conservadora, no una frontera de seguridad.',
  },
  {
    section: 'distinctive', title: 'Keyword watch (vigilar una palabra)', status: 'shipped',
    natural: 'Arma el panel enfocado con una palabra: cuando una linea posterior de su salida la contenga (sin distinguir mayusculas), runnir lanza una notificacion de escritorio con la linea que coincidio. Apuntalo a un build, un deploy o un tail -f y vete a otra cosa: te avisa en cuanto aparezca "deploy OK", "error", "panic" o lo que hayas puesto.',
    palette: 'Watch pane for keyword',
    note: 'Coincidencia por subcadena, sin regex. La vigilancia empieza desde el fondo actual, asi que el historial viejo no dispara. Una palabra vacia limpia la vigilancia.',
  },
  {
    section: 'distinctive', title: 'Named layouts (espacios de trabajo)', status: 'shipped',
    natural: 'Defines disposiciones con nombre en la configuracion y lanzas una desde la paleta: abre una pestana fresca dividida en un panel por comando, ordenados en mosaico. Perfecto para un layout "servers" que hace ssh a varias maquinas a la vez de un solo tiron.',
    palette: 'Launch layout',
    config: [{ k: '[[layouts]]', v: 'name + commands[]', d: 'Cada layout abre una pestana con un panel por comando. Un comando vacio abre un shell normal.' }],
    example: '[[layouts]]\nname = "servers"\ncommands = [ "ssh 192.168.1.3", "ssh 192.168.1.7", "ssh 192.168.1.9", "htop" ]',
    note: 'Los comandos se dividen por espacios (no es un parseo completo de shell), lo que cubre "ssh host", "journalctl -f" y similares.',
  },
  {
    section: 'distinctive', title: 'Broadcast (entrada a varios paneles)', status: 'shipped',
    natural: 'Activa el broadcast y lo que escribes va a todos los paneles de la pestana a la vez: util para pilotar varios servidores en paralelo. Y con los grupos afinas mas: marcas los paneles concretos que forman el grupo, y cuando hay algun miembro el broadcast se limita al grupo en vez de a toda la pestana. Asi puedes emitir a tres de cinco paneles y dejar en paz un tail de logs y un monitor.',
    keys: ['Ctrl+Shift+B (activar/desactivar broadcast)'],
    palette: 'Toggle broadcast input / Toggle pane in broadcast group',
    note: 'Sin miembros de grupo, el broadcast cubre todos los paneles.',
  },
  {
    section: 'distinctive', title: 'Tintado por contexto (SSH / sudo / docker)', status: 'shipped',
    natural: 'runnir vigila el proceso en primer plano de cada panel. Cuando es ssh, tinta el panel de un color derivado del nombre del host: el mismo host es siempre el mismo tono, en cualquier maquina, sin configurar nada. Los paneles sudo o root se tintan de rojo, docker de azul. De un vistazo sabes en que mundo esta cada panel y no ejecutas en produccion lo que creias local. Lanza el ssh de verdad, asi que tu ~/.ssh/config, los jump hosts y el agente de 1Password funcionan sin cambios.',
    keys: ['Ctrl+Shift+S (conexion rapida: elige un host de ~/.ssh/config)'],
    palette: 'SSH quick connect',
    config: [{ k: 'behaviour.context_tint', v: 'true', d: 'Tintar el fondo segun el proceso en primer plano (ssh / sudo / docker).' }],
  },

  // ------------------------------------------------------------- APARIENCIA
  {
    section: 'appearance', title: 'Transparencia y desenfoque', status: 'shipped',
    natural: 'Baja la opacidad de la ventana por debajo de 1.0 y el fondo por defecto deja ver lo que hay detras, asi que una regla de blur de tu compositor surte efecto tras runnir. El texto y las celdas con color de fondo explicito se quedan totalmente opacos y legibles: solo el fondo por defecto es translucido.',
    config: [{ k: 'window.opacity', v: '1.0', d: 'Translucidez de la ventana, 0.1..1.0 (necesita compositor; 1.0 = opaco).' }],
    example: '# Hyprland: para desenfocar detras de runnir\ndecoration { blur = yes }\nwindowrulev2 = opacity 0.9, class:^(runnir)$',
    note: 'Cambiar la opacidad entre opaco y translucido es el unico ajuste que aun necesita reiniciar.',
  },
  {
    section: 'appearance', title: 'Imagen de fondo', status: 'shipped',
    natural: 'Pon una imagen detras del terminal, atenuada al brillo que quieras para que el texto siga leyendose. Se dibuja la primera, por debajo de todo, y necesita algo de transparencia en el fondo para asomar.',
    config: [
      { k: 'window.background', v: 'null', d: 'Ruta a una imagen dibujada detras del terminal (necesita opacity < 1).' },
      { k: 'window.background_dim', v: '0.35', d: 'Cuanto se atenua la imagen de fondo (0 = negro, 1 = brillo completo).' },
    ],
  },
  {
    section: 'appearance', title: 'Temas', status: 'shipped',
    natural: 'runnir trae un tema oscuro sobrio (fondo casi negro, acento verde) y todo el es configurable: color de texto, de fondo, del cursor, de la seleccion, las 16 colores ANSI y el acento de la propia interfaz. Los colores se escriben en hexadecimal, en formato largo (#rrggbb) o corto (#rgb).',
    config: [
      { k: 'theme.foreground', v: '#d4d6d9', d: 'Color del texto.' },
      { k: 'theme.background', v: '#0d0d0f', d: 'Color de fondo (negro casi puro).' },
      { k: 'theme.cursor', v: '#d4d6d9', d: 'Color del cursor.' },
      { k: 'theme.selection', v: '#334466', d: 'Color de la seleccion.' },
      { k: 'theme.accent', v: '#4c9fd4', d: 'Acento de la UI propia (barra de pestanas, paleta, paneles).' },
      { k: 'theme.dim', v: '#6a6d74', d: 'Color tenue.' },
      { k: 'theme.ansi', v: '16 colores', d: 'Las 16 colores ANSI: 0-7 normales, 8-15 brillantes. El verde 0dbc79 es el acento de la marca.' },
    ],
  },
  {
    section: 'appearance', title: 'Selector de temas con vista previa', status: 'dev',
    natural: 'Un selector con entre 20 y 30 temas incorporados y vista previa en vivo: recorres la lista y ves cada tema aplicado al momento antes de quedarte con uno, sin editar el config a mano ni reiniciar.',
    note: 'Marcado como En desarrollo: picker con ~20-30 temas predefinidos y previsualizacion en directo. Hoy los colores se ajustan en theme.* del config.',
  },
  {
    section: 'appearance', title: 'Iconos y avisos de pestana', status: 'shipped',
    natural: 'Cada pestana muestra un icono (de nerd-font) segun la aplicacion en primer plano y un aviso: un punto ambar si es una pestana en segundo plano con salida sin ver, y una cruz roja si su ultimo comando fallo. Asi sabes de un vistazo cual pestana tiene algo nuevo o algo que reviso.',
    note: 'La barra de pestanas se desplaza para mantener visible la activa.',
  },
  {
    section: 'appearance', title: 'Barra de estado', status: 'shipped',
    natural: 'Una barra en el borde inferior con el directorio actual, la rama de git y el reloj. Cuesta una fila de altura y se puede quitar. De un vistazo sabes donde estas y en que rama.',
    config: [{ k: 'window.status_bar', v: 'true', d: 'Mostrar la barra inferior (cwd, rama de git, reloj). Cuesta una fila.' }],
  },
  {
    section: 'appearance', title: 'Barra de progreso (OSC 9;4)', status: 'shipped',
    natural: 'Cuando una herramienta informa de su progreso con OSC 9;4 (descargas, builds, dd con status), runnir dibuja una barra a lo largo del borde inferior del panel. Ves cuanto queda sin quedarte mirando numeros.',
    escape: [R`\e]9;4;<estado>;<porcentaje> ST   (estado 1 = normal, 2 = error, 0 = limpiar)`],
  },
  {
    section: 'appearance', title: 'Estela del cursor', status: 'shipped',
    natural: 'Un detalle estetico: al saltar el cursor deja una breve estela que se desvanece detras de el. Puro adorno, apagado por defecto.',
    config: [{ k: 'cursor.trail', v: 'false', d: 'Dibujar una breve estela que se desvanece detras del cursor.' }],
  },
  {
    section: 'appearance', title: 'Scroll suave', status: 'shipped',
    natural: 'Los saltos de scroll (al principio, al final, saltar a un prompt) se animan con un deslizamiento suave en vez de teletransportarse, para que el ojo siga a donde ha ido la vista. Ademas, el scroll de touchpad acumula fracciones de linea para que los gestos lentos no se pierdan.',
    config: [{ k: 'behaviour.smooth_scroll', v: 'true', d: 'Animar los saltos de scroll con un deslizamiento suave en vez de teletransportar.' }],
  },
  {
    section: 'appearance', title: 'Tamano de fuente en vivo', status: 'shipped',
    natural: 'Agranda o reduce la fuente al vuelo, sin reiniciar, y vuelve al tamano configurado cuando quieras. Util para leer algo de cerca o para caber mas en pantalla en una demo.',
    keys: ['Ctrl++ (o Ctrl+=) mas grande', 'Ctrl+- mas pequena', 'Ctrl+0 restablecer'],
    palette: 'Increase font size / Decrease font size / Reset font size',
    config: [
      { k: 'font.family', v: '"JetBrainsMono Nerd Font Mono"', d: 'Familia de fuente monoespaciada.' },
      { k: 'font.size', v: '16.0', d: 'Tamano base en puntos (4..200).' },
    ],
  },
  {
    section: 'appearance', title: 'Campana visual y sonora', status: 'shipped',
    natural: 'Cuando un programa hace sonar la campana (BEL), el panel destella brevemente en blanco; y si la ventana no tiene el foco, ademas levanta el aviso de urgencia del compositor. Asi un build que termina en segundo plano te llama la atencion sin robarte el foco.',
    escape: [R`\a   BEL (0x07): dispara el destello y, si esta sin foco, la urgencia`],
  },

  // ------------------------------------------------------------- PROTOCOLOS
  {
    section: 'protocols', title: 'Hyperlinks OSC 8', status: 'shipped',
    natural: 'Los programas pueden marcar un trozo de texto como un enlace real con una URL asociada (lo hacen ls --hyperlink, gcc, cargo...). runnir lo entiende: al pasar por encima se subraya el enlace exacto que el programa declaro y con Ctrl+clic se abre.',
    escape: [R`\e]8;;https://ejemplo.com ST  texto del enlace  \e]8;; ST`],
  },
  {
    section: 'protocols', title: 'Portapapeles OSC 52', status: 'dev',
    natural: 'Deja que un programa (incluso a traves de ssh o dentro de tmux) copie texto a tu portapapeles local mediante una secuencia de escape, sin plugins ni puentes raros. runnir lo soporta en modo escritura (write-only): los programas pueden poner cosas en tu portapapeles, pero no leerlo, que es lo seguro.',
    escape: [R`\e]52;c;<texto-en-base64> ST   escribe <texto> en el portapapeles`],
    note: 'Marcado como En desarrollo. Solo escritura, por seguridad: nunca se permite leer el portapapeles.',
  },
  {
    section: 'protocols', title: 'Progreso OSC 9;4', status: 'shipped',
    natural: 'El protocolo de progreso (originario de ConEmu y Windows Terminal) por el que una herramienta informa de su porcentaje. runnir lo pinta como una barra en el borde inferior del panel. Ver tambien "Barra de progreso" en Apariencia.',
    escape: [R`\e]9;4;1;<0-100> ST  progreso normal`, R`\e]9;4;2;<0-100> ST  estado de error`, R`\e]9;4;0 ST  limpiar`],
  },
  {
    section: 'protocols', title: 'Notificaciones OSC 99 / OSC 777', status: 'dev',
    natural: 'Deja que un programa lance una notificacion de escritorio con titulo y cuerpo mediante una secuencia de escape: "build terminado", "tests en verde", lo que sea. Soporta el formato moderno (OSC 99, el de kitty) y el clasico (OSC 777, el de urxvt/Windows Terminal), asi que funciona con lo que ya emiten muchas herramientas.',
    escape: [R`\e]99;;<mensaje> ST            notificacion (formato kitty)`, R`\e]777;notify;<titulo>;<cuerpo> ST   notificacion (formato clasico)`],
    note: 'Marcado como En desarrollo.',
  },
  {
    section: 'protocols', title: 'Protocolo de teclado kitty (CSI u)', status: 'dev',
    natural: 'El esquema de codificacion de teclado moderno que piden neovim y las TUIs actuales para distinguir teclas que el terminal clasico no puede diferenciar (por ejemplo Esc de Ctrl+[, o Tab de Ctrl+I) y para reportar pulsaciones que antes se perdian. runnir lo implementa con los modos de desambiguar (disambiguate) y reportar todo (report-all), asi neovim y compania reciben exactamente la tecla que pulsaste.',
    escape: [R`\e[>1u   push: modo desambiguar (disambiguate escape codes)`, R`\e[>15u  push: reportar todos los eventos (report-all)`, R`\e[<u    pop: restaura el modo anterior`, R`\e[?u    consulta el modo activo`],
    note: 'Marcado como En desarrollo: soporte de CSI u para neovim y TUIs modernas.',
  },
  {
    section: 'protocols', title: 'Protocolo grafico kitty', status: 'shipped',
    natural: 'El protocolo con el que las herramientas dibujan imagenes reales en la rejilla. Ver "Imagenes en linea" en Renderizado para el detalle y ejemplos.',
    note: 'Las imagenes se desplazan con su texto y se reciclan con el historial. runnir responde a la consulta de soporte.',
  },

  // ----------------------------------------------------------- AUTOMATIZACION
  {
    section: 'automation', title: 'API de control remoto (runnir @)', status: 'dev',
    natural: 'Controla una instancia de runnir desde fuera, desde un script o desde otra terminal, con un subcomando runnir @. Sirve para automatizar: lanzar comandos en paneles nuevos, teclear texto en un panel, leer lo que hay en pantalla o cambiar de pestana. Es el equivalente al "remote control" de kitty o al CLI de wezterm.',
    example: 'runnir @ launch htop          # abre un comando en un panel/pestana nuevo\nrunnir @ send-text "ls -la\\n"  # teclea texto en el panel objetivo\nrunnir @ get-text              # lee el contenido visible del panel\nrunnir @ ls                    # lista pestanas y paneles\nrunnir @ focus-tab 2           # enfoca la pestana N',
    note: 'Marcado como En desarrollo: subcomandos launch | send-text | get-text | ls | focus-tab.',
  },
  {
    section: 'automation', title: 'Modos de layout (mosaicos)', status: 'dev',
    natural: 'Ademas de dividir paneles a mano, runnir podra ordenarlos automaticamente en distintos modos de mosaico, al estilo de un gestor de ventanas de mosaico: splits libres, pila (uno grande y el resto apilados), "tall" (uno ancho a la izquierda), "fat" (uno alto arriba) y rejilla. Cambias de modo y los paneles se recolocan solos.',
    note: 'Marcado como En desarrollo: modos splits / stack / tall / fat / grid. Hoy los splits se crean y redimensionan a mano.',
  },

  // ----------------------------------------------------------- CONFIGURACION
  {
    section: 'config', title: 'Archivo de configuracion (TOML / JSON)', status: 'shipped',
    natural: 'Toda la configuracion vive en un archivo TOML en ~/.config/runnir/runnir.toml (o un JSON en runnir.json, que tiene prioridad). Cada ajuste tiene un valor por defecto que se sostiene solo, asi que un archivo parcial o inexistente es normal, no un error. Un archivo con un fallo se avisa y se ignora: una errata en un color jamas te deja sin terminal. Las claves de API se referencian por nombre de variable de entorno, asi que el archivo es seguro para un repo de dotfiles.',
    example: 'runnir --write-config   # escribe un config por defecto totalmente comentado',
    config: [
      { k: 'window.width / height', v: '1100 / 700', d: 'Tamano inicial de la ventana en pixeles.' },
      { k: 'window.padding', v: '8.0', d: 'Margen interior en pixeles (0..200).' },
      { k: 'window.decorations', v: 'false', d: 'Mostrar los bordes/titulo de la ventana del sistema.' },
      { k: 'behaviour.wheel_lines', v: '3.0', d: 'Lineas por muesca de la rueda (1..50).' },
      { k: 'behaviour.notify_after_secs', v: '20', d: 'Notificar cuando un comando mas largo que esto termine sin foco (0 desactiva).' },
      { k: 'behaviour.confirm_close', v: 'true', d: 'Pedir confirmacion al cerrar.' },
    ],
  },
  {
    section: 'config', title: 'Panel de ajustes', status: 'shipped',
    natural: 'Un panel interactivo para tocar cada opcion sin editar el archivo a mano: te mueves con las flechas, cambias un valor con izquierda/derecha, editas un campo de texto con Enter y guardas con s. Al guardar escribe ~/.config/runnir/runnir.json, que se carga con preferencia sobre el TOML, y los cambios se aplican en vivo mientras los haces.',
    keys: ['flechas o j/k (mover)', 'izquierda/derecha o h/l (cambiar valor)', 'Enter (editar campo de texto)', 's (guardar)'],
    palette: 'Settings',
  },
  {
    section: 'config', title: 'Recarga en caliente', status: 'shipped',
    natural: 'Guarda el archivo de configuracion y runnir aplica el nuevo tema, la fuente y los atajos en menos de un segundo, sin reiniciar. Comprueba la fecha de modificacion del archivo activo (el JSON si existe, si no el TOML). El unico cambio que aun necesita reiniciar es pasar la opacidad de opaco a translucido.',
    note: 'Ante un error de parseo se conserva la configuracion en uso en vez de saltar a los valores por defecto.',
  },
  {
    section: 'config', title: 'Atajos de teclado personalizables', status: 'shipped',
    natural: 'Puedes reasignar cualquier accion a la combinacion que prefieras desde el config, y tus atajos se fusionan sobre los de fabrica. Los acordes se escriben como "ctrl+shift+t", "alt+enter", "super+1". La regla de oro: los atajos propios llevan Ctrl+Shift o Super, nunca Ctrl+letra a secas, que es del programa dentro del panel.',
    example: '[keys]\n"ctrl+shift+t" = "new_tab"\n"alt+enter" = "toggle_zoom"',
    note: 'Cada accion tiene un id estable (ver la chuleta de atajos). go_to_tab_1..9 mapean a "ir a la pestana N".',
  },

  // -------------------------------------------------------------- PLATAFORMA
  {
    section: 'platform', title: 'Linux y macOS', status: 'shipped',
    natural: 'runnir es un terminal de GPU escrito desde cero en Rust para Linux y macOS. Necesita una GPU capaz de Vulkan, Metal o DX12 y una fuente monoespaciada. Corre vim, htop y btop correctamente dentro. Su renderizado es de una sola llamada de dibujo (una instancia por celda) y en reposo no consume nada: espera de verdad hasta que algo cambia.',
    example: 'cargo run                 # compilar y ejecutar\ncargo build --release     # binario optimizado',
    note: 'La fuente por defecto es JetBrainsMono Nerd Font Mono; se puede sobrescribir con la variable RUNNIR_FONT.',
  },
  {
    section: 'platform', title: 'Modos headless de verificacion', status: 'shipped',
    natural: 'Para probar y automatizar, runnir puede correr sin abrir ventana: vuelca la rejilla como texto o la renderiza a un PNG. Estan deliberadamente separados para que un fallo del parser nunca se disfrace de fallo de la GPU.',
    example: 'runnir --dump   "<cmd>"                 # corre cmd en un PTY real e imprime la rejilla como texto\nrunnir --render out.png "<cmd>" [ms]    # renderiza la rejilla a PNG sin ventana\nrunnir --demo out.png                   # captura de demostracion',
  },

  // ---------------------------------------------------------------- ROADMAP
  {
    section: 'roadmap', title: 'Rigor Unicode / grafemas', status: 'dev',
    natural: 'Tratamiento mas fino de los grafemas Unicode: emojis compuestos con modificadores (tono de piel, secuencias ZWJ), anchos de caracter en casos limite y combinaciones que hoy pueden descolocar la rejilla. El objetivo es que el ancho que ocupa cada cosa coincida siempre con lo que el resto de programas espera.',
    note: 'En cola, aun sin empezar.',
  },
  {
    section: 'roadmap', title: 'IME (metodos de entrada)', status: 'dev',
    natural: 'Soporte de editores de metodo de entrada, imprescindible para escribir idiomas como el chino, el japones o el coreano, y en general para la composicion de caracteres con la ventanita de candidatos.',
    note: 'En cola, aun sin empezar.',
  },
  {
    section: 'roadmap', title: 'Sixel', status: 'dev',
    natural: 'Otro protocolo de imagenes en el terminal, mas antiguo que el de kitty pero que aun usan bastantes herramientas. Anadirlo amplia la compatibilidad con programas que dibujan graficos en Sixel.',
    note: 'En cola, aun sin empezar.',
  },
  {
    section: 'roadmap', title: 'Text sizing (tamano de texto en linea)', status: 'dev',
    natural: 'El protocolo que permite a un programa pedir texto mas grande o mas pequeno dentro de la misma pantalla (titulos grandes, superindices), para presentaciones y TUIs mas expresivas.',
    note: 'En cola, aun sin empezar.',
  },
  {
    section: 'roadmap', title: 'Triggers (reglas automaticas)', status: 'dev',
    natural: 'Reglas del tipo "cuando aparezca este texto, haz esto": resaltar, notificar, lanzar un comando. Es la generalizacion del keyword watch a un motor de reglas configurable.',
    note: 'En cola, aun sin empezar.',
  },
  {
    section: 'roadmap', title: 'Bloques de comando navegables', status: 'dev',
    natural: 'Tratar cada comando y su salida como un bloque con el que interactuar: plegarlo, copiarlo, reejecutarlo, saltar entre ellos de forma mas rica. Es la evolucion del plegado y el salto entre comandos actuales.',
    note: 'En cola, aun sin empezar.',
  },
  {
    section: 'roadmap', title: 'Transferencia de archivos', status: 'dev',
    natural: 'Mover archivos por el propio canal del terminal, tipico para traer o llevar ficheros a traves de una sesion ssh sin abrir otra herramienta.',
    note: 'En cola, aun sin empezar.',
  },
]
