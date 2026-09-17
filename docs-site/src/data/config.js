// Referencia completa de configuración. Derivada de src/config.rs: cada opción,
// su valor por defecto y una línea de descripción. Archivo en
// ~/.config/runnir/runnir.toml (o runnir.json, que tiene prioridad).
// group, k, v son idénticos en ambos idiomas (cabeceras TOML, claves, valores).
// d (descripción) es un par { es, en }.
// group, k, v are identical in both languages (TOML headers, keys, values).
// d (description) is an { es, en } pair.
export const CONFIG_GROUPS = [
  {
    group: '[transfer]',
    rows: [
      { k: 'fps', v: '0', d: { es: 'Códigos por segundo, o 0 para automático: lo más rápido que ESTA máquina consigue pintar, con techo de 30. El techo es donde satura el receptor —un móvil lee unos 9 códigos por segundo, y 30 frames en color ya ponen 60 en la pantalla—, no la pantalla. El número está medido contra un móvil real, y ha estado mal tres veces: primero 30 razonado, luego 10 medido, y ese 10 resultó ser un artefacto de tener el receptor estrangulado. Un valor explícito se obedece aunque el pintor no llegue, y entonces el panel lo dice en vez de corregirte — la misma regla que tiles.', en: 'Codes per second, or 0 for automatic: the fastest THIS machine manages to paint, capped at 30. The cap is where the receiver saturates — a phone reads about 9 codes a second, and 30 frames in colour already put 60 on screen — not the screen. The number is measured against a real phone, and has been wrong three times: 30 reasoned, then 10 measured, and that 10 turned out to be an artifact of a throttled receiver. An explicit value is obeyed even when the painter cannot keep up, and the panel says so rather than overruling you — the same rule as tiles.' } },
      { k: 'tiles', v: '0', d: { es: 'Cuántos códigos a la vez, o 0 para poner los que quepan sin encoger los módulos. En una ventana 16:9 eso son dos, gratis: el código se dimensiona por el lado corto, así que el segundo ocupa espacio que era margen blanco. Un número mayor del que cabe cambia nitidez por cantidad — vale la pena de cerca y con buena cámara, y se pierde a distancia de brazo. El modo automático además se limita solo a lo que la máquina puede pintar a tiempo: un código cuesta unos 8 ms, casi todo el encode del QR.', en: 'How many codes at once, or 0 to fit as many as the window takes without shrinking the modules. On a 16:9 window that is two, for free: a code is sized by the short axis, so the second one sits in what was white margin. A number larger than what fits trades sharpness for count — worth it up close with a good camera, a loss at arm\u2019s length. The automatic answer also holds itself to what the machine can paint in time: a code costs about 8 ms, nearly all of it the QR encode.' } },
      { k: 'color', v: 'true', d: { es: 'Lleva un segundo código en el color del primero: rojo y verde pintan el QR de siempre, en blanco y negro, y el azul otro frame del mismo stream. Leído como brillo —que es lo que hace cualquier decodificador— el primero sigue siendo un QR corriente, así que no se añade nada al formato y un receptor que no sepa de color lee la mitad del stream a toda velocidad. Duplica lo que sale y también lo que el receptor tiene que decodificar, así que gana cuando el móvil no es ya el cuello de botella (pocas capturas tiradas en su línea de métricas) y no compra nada cuando lo es. Encendido por defecto: midió +73% contra un móvil real (22,5 KB/s con color a 30 fps, contra 13 como mucho sin él). runnir @ transfer --color 0 lo apaga por emisión, que es lo que conviene en un móvil cuya línea de métricas enseñe capturas tiradas.', en: 'Carry a second code in the colour of the first: red and green paint the usual black-and-white QR, blue carries another frame of the same stream. Read as brightness — which is what every decoder does — the first one is still an ordinary QR, so nothing is added to the format and a receiver that knows nothing about colour reads half the stream at full speed. It doubles what goes out and what the receiver has to decode, so it wins when the phone is not already the bottleneck (few dropped captures on its metrics line) and buys nothing when it is. On by default: it measured +73% against a real phone (22.5 KB/s in colour at 30 fps, against 13 at best without). runnir @ transfer --color 0 turns it off per stream, which is what a phone dropping captures on its metrics line wants.' } },
    ],
  },
  {
    group: '[tidal]',
    rows: [
      { k: 'client_id', v: '""', d: { es: 'Credencial de TIDAL. Sin client_id y client_secret el panel no existe. Nunca van compiladas en el binario: el repositorio es público.', en: 'TIDAL credential. Without client_id and client_secret the panel does not exist. Never compiled into the binary: this repository is public.' } },
      { k: 'client_secret', v: '""', d: { es: 'El secreto. Puede quedarse fuera del fichero: si client_secret_env nombra una variable de entorno con valor, esa gana.', en: 'The secret. It can stay out of the file: if client_secret_env names an environment variable that has a value, that one wins.' } },
      { k: 'client_secret_env', v: '"RUNNIR_TIDAL_SECRET"', d: { es: 'Variable de entorno de la que leer el secreto, para no dejarlo en un fichero que acaba en un repo de dotfiles.', en: 'Environment variable to read the secret from, so it need not sit in a file that ends up in a dotfiles repository.' } },
      { k: 'quality', v: '"hi_res_lossless"', d: { es: 'Calidad que se PIDE: hi_res_lossless, lossless o high. TIDAL responde con la mejor que pueda servir, que puede ser menor; la insignia dice lo que llegó, no lo que se pidió.', en: 'The tier ASKED for: hi_res_lossless, lossless or high. TIDAL answers with the best it can serve, which may be lower; the badge reports what arrived, not what was requested.' } },
      { k: 'output', v: '"auto"', d: { es: '"auto" recorre los dispositivos y elige el primero que acepte el stream intacto — un DAC USB va primero y una salida de pantalla no se elige nunca sola. Un nombre como "hw:2,0" lo fija. "default" entrega el audio a PipeWire/PulseAudio, que nunca es bit-perfect y nunca falla.', en: '"auto" walks the devices and takes the first that accepts the stream untouched — a USB DAC goes first and a display output is never chosen automatically. A name like "hw:2,0" pins one. "default" hands the audio to PipeWire/PulseAudio, which is never bit-perfect and never fails.' } },
      { k: 'bit_perfect', v: 'true', d: { es: 'Intentar el camino exclusivo hasta el DAC. Apagarlo no rompe nada: solo deja de intentarlo, útil cuando otra cosa necesita la tarjeta. Con él activo el volumen queda bloqueado, porque no hay nada entre el decodificador y el DAC que pueda escalar una muestra.', en: 'Try for the exclusive path to the DAC. Turning it off breaks nothing: it just stops trying, which helps when something else needs the card. With it on the volume is locked, because there is nothing between the decoder and the DAC that could scale a sample.' } },
      { k: 'volume_normalization', v: 'false', d: { es: 'ReplayGain. Apagado por defecto porque escala cada muestra, que es exactamente lo que bit-perfect promete no hacer; con bit_perfect activo se ignora.', en: 'ReplayGain. Off by default because it scales every sample, which is exactly what bit-perfect promises not to do; with bit_perfect on it is ignored.' } },
      { k: 'callback_port', v: '8747', d: { es: 'Puerto en el que escucha el retorno del inicio de sesión. Fijo y no aleatorio, porque un redirect_uri tiene que ser predecible para poder registrarse.', en: 'Port the sign-in callback listens on. Fixed rather than random, because a redirect URI has to be predictable to be registered.' } },
      { k: 'release_device', v: 'true', d: { es: 'Pedirle a PipeWire que suelte la tarjeta antes de abrirla en exclusivo. Encendido porque sin esto el camino exclusivo no consigue dispositivo en un escritorio normal: PipeWire retiene toda tarjeta que gestiona, esté sonando o no. Se le PIDE por el protocolo de reserva de dispositivo que implementa justo para esto, no se le quita, y la recupera al parar.', en: 'Ask PipeWire to let go of the sound card before opening it exclusively. On, because without it the exclusive path never gets a device on a normal desktop: PipeWire holds every card it manages, idle or not. It is ASKED through the Device Reservation protocol it implements for exactly this, not taken, and it gets the card back when playback stops.' } },
    ],
  },
  {
    group: '[spotify]',
    rows: [
      { k: 'client_id', v: '"65b7…87bd"', d: { es: 'El id de escritorio que trae librespot, público y sin registro, y el único que se sabe que abre sesión de reproducción contra el access point. Uno propio de developer.spotify.com sirve para la Web API y no está probado para reproducir. No hay client_secret: la autenticación es PKCE, que existe justamente para que un cliente que no puede guardar un secreto no tenga que tenerlo.', en: 'The desktop id librespot ships, public and registration-free, and the only one known to open a playback session against the access point. One of your own from developer.spotify.com is good for the Web API and unproven for playback. There is no client_secret: authentication is PKCE, which exists precisely so a client that cannot keep a secret does not need one.' } },
      { k: 'callback_port', v: '8898', d: { es: 'Retorno del inicio de sesión que REPRODUCE. No es una elección libre con el client_id por defecto: Spotify comprueba el redirect contra los registrados para ese cliente, y el de escritorio tiene http://127.0.0.1:8898/login. Cambiarlo sin poner también tu client_id devuelve INVALID_CLIENT, un error que no menciona el puerto.', en: 'Callback for the sign-in that PLAYS. Not a free choice with the default client_id: Spotify checks the redirect against the ones registered for that client, and the desktop one holds http://127.0.0.1:8898/login. Changing it without setting your own client_id gets INVALID_CLIENT back, an error that never mentions the port.' } },
      { k: 'api_client_id', v: '""', d: { es: 'Un client_id tuyo para el CATÁLOGO. Vacío significa usar el mismo de arriba, que funciona y luego deja de funcionar: el id de escritorio lo comparte todo programa basado en librespot y la Web API lo limita como un solo cliente (medido: 429 Retry-After: 40 en la tercera petición de una sesión recién abierta, con horas de por medio). La reproducción no se entera, porque nunca toca la Web API.', en: 'A client_id of your own for the CATALOGUE. Empty means it uses the one above, which works and then stops working: the desktop id is shared by every librespot-based program and the Web API rate-limits it as a single client (measured: 429 Retry-After: 40 on the third request of a fresh session, hours apart). Playback never notices, because it never touches the Web API.' } },
      { k: 'api_callback_port', v: '8899', d: { es: 'Retorno del inicio de sesión de catálogo. Este sí es libre, al contrario que el otro, porque el redirect lo registra quien es dueño del client_id — tú.', en: 'Callback for the catalogue sign-in. This one IS free to choose, unlike the other, because the redirect is registered by whoever owns the client id — you.' } },
      { k: 'output', v: '"auto"', d: { es: 'Misma cadena que TIDAL: "auto", un nombre como "hw:2,0", o "default" para PipeWire. Bit-perfect no se alcanza desde una fuente con pérdidas, pero un dispositivo exclusivo que no remuestrea sí.', en: 'The same chain as TIDAL: "auto", a name like "hw:2,0", or "default" for PipeWire. Bit-perfect is not reachable from a lossy source, but an exclusive device that does not resample is.' } },
      { k: 'bit_perfect', v: 'true', d: { es: 'Intentar el camino exclusivo. Con Spotify el techo honesto es exclusivo y sin remuestrear a 44,1/16, y la insignia nunca dice BIT-PERFECT ahí por muy bueno que sea el DAC.', en: 'Try for the exclusive path. On Spotify the honest ceiling is exclusive and unresampled at 44.1/16, and the badge never claims BIT-PERFECT there however good the DAC is.' } },
      { k: 'release_device', v: 'true', d: { es: 'Igual que en [tidal]: pedirle la tarjeta a PipeWire antes de abrirla en exclusivo.', en: 'As in [tidal]: ask PipeWire for the card before opening it exclusively.' } },
      { k: 'connect_device', v: 'true', d: { es: 'Anunciar la terminal a Spotify como dispositivo Connect. Encendido, porque sin esto el móvil no sabe que la terminal existe: suena por los altavoces y Spotify sigue sin enseñar nada en ningún sitio. Apagarlo devuelve el comportamiento anterior.', en: 'Announce the terminal to Spotify as a Connect device. On, because without it the phone has no idea the terminal exists: it plays out of the speakers and Spotify goes on showing nothing anywhere. Turning it off restores the old behaviour.' } },
      { k: 'device_name', v: '"runnir"', d: { es: 'El nombre que ve el móvil en la lista de dispositivos.', en: 'The name the phone sees in the device list.' } },
    ],
  },
  {
    group: 'raíz / top-level',
    rows: [
      { k: 'leader', v: '"alt+shift+space"', d: { es: 'Acorde que arma la capa leader: se pulsa, se suelta y luego una tecla directa (1..9; hjkl para el foco; HJKL y las flechas para redimensionar; v, g; z, Z, +, =, - y 0 para el tamaño de letra) o una de grupo (t, p, c, f, a, r, o, s) que pide una segunda. Mientras está armada la barra inferior muestra LEADER y un panel lista las opciones; con la barra oculta (status_bar = false) no hay chip y sale un aviso «leader…» en su lugar. Cadena vacía = capa desactivada. Evita ctrl+alt+space al reasignarlo: ctrl+alt es AltGr en la distribución española.', en: 'Chord that arms the leader layer: press it, release, then one direct key (1..9; hjkl for focus; HJKL and the arrows to resize; v, g; z, Z, +, =, - and 0 for font size) or a group key (t, p, c, f, a, r, o, s) that takes a second one. While armed the status bar shows LEADER and a panel lists the options; with the bar hidden (status_bar = false) there is no chip and a “leader…” toast stands in. An empty string turns the layer off. Avoid ctrl+alt+space when rebinding: ctrl+alt is AltGr on the Spanish layout.' } },
      { k: 'music_provider', v: '""', d: { es: 'La tienda con la que abre el panel de música: "tidal" o "spotify". Lo escribe el selector (Leader N P), así que rara vez se toca a mano; vacío significa la que esté configurada. Es una cadena y no un enum en el fichero a propósito: un valor desconocido degrada al de por defecto en vez de negarse a cargar el config entero — una errata aquí no debe costarte tus atajos.', en: 'The shop the music panel opens on: "tidal" or "spotify". The selector (Leader N P) writes it, so it is rarely set by hand; empty means whichever one is configured. Deliberately a string rather than an enum in the file: an unknown value degrades to the default instead of refusing to load the whole config — a typo here should not cost you your keybindings.' } },
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
      { k: 'screensaver_after_secs', v: '0', d: { es: 'Sacar el mapa solo tras este tiempo sin que ninguna tecla llegue a un panel; hace de salvapantallas, con lluvia de runas y la hora tallada en el centro. 0 nunca. Cualquier tecla lo quita, y esa tecla no hace nada más.', en: 'Put the map up by itself after this long with no keystroke reaching a pane; it doubles as a screensaver, with rune rain and the time carved in the middle. 0 never. Any key dismisses it, and that key does nothing else.' } },
      { k: 'confirm_close', v: 'true', d: { es: 'Pedir confirmación al cerrar.', en: 'Ask for confirmation on close.' } },
      { k: 'restore_session', v: 'true', d: { es: 'Restaurar la ventana que cerraste (pestañas, layout, directorios, historial) al abrir la siguiente — solo cuando no hay otra ventana de runnir abierta: una segunda ventana junto a una viva arranca limpia, porque heredar el layout de algo que sigue en pantalla es una copia que nadie pidió. En false, cada arranque empieza con una pestaña nueva.', en: 'Restore the window you closed (tabs, layout, directories, scrollback) into the next one you open — only when no other runnir window is running: a second window opened beside a live one starts clean, because inheriting the layout of something still on screen is a copy nobody asked for. false starts every launch with one fresh tab.' } },
      { k: 'command_guardian', v: 'true', d: { es: 'Confirmar comandos destructivos antes de ejecutarlos.', en: 'Confirm destructive commands before running them.' } },
      { k: 'shell_integration', v: 'true', d: { es: 'Inyectar la integración de shell (marcas de prompt OSC 133 y cwd por OSC 7) en fish, zsh y bash sin tocar tus ficheros rc. Es lo que alimenta el salto entre comandos, el gutter de acierto/fallo y el seguimiento del directorio. La detección hace lo que puede: una shell que no reconozca se lanza tal cual.', en: 'Inject shell integration (OSC 133 prompt marks and OSC 7 cwd) into fish, zsh and bash without touching your rc files. It is what powers command jumps, the pass/fail gutter and cwd tracking. Detection is best-effort: an unrecognised shell is spawned unchanged.' } },
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
    group: '[explorer]',
    rows: [
      { k: 'side', v: '"left"', d: { es: 'Lado en el que se dibuja la barra: "left" (donde la pone cualquier editor) o "right".', en: 'Which edge the sidebar sits on: "left" (where every editor puts it) or "right".' } },
      { k: 'width', v: '30', d: { es: 'Ancho en COLUMNAS, no en fracción de la ventana: una fracción en un ultrapanorámico da un árbol de 90 columnas. Se acota contra la ventana al dibujarlo, así que encogerla nunca deja la barra más ancha que la pestaña.', en: 'Width in COLUMNS, not a fraction of the window: a fraction on an ultrawide gives a 90-column tree. Clamped against the window when drawn, so shrinking it never leaves the sidebar wider than the tab.' } },
      { k: 'show_hidden', v: 'false', d: { es: 'Mostrar los ficheros que empiezan por punto. La tecla . lo cambia en caliente.', en: 'Show dotfiles. The . key toggles it live.' } },
      { k: 'open_on_start', v: 'false', d: { es: 'Abrir la barra al arrancar, en cada pestaña.', en: 'Open the sidebar on start, in every tab.' } },
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
