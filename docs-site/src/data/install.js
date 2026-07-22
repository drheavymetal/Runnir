// Instalación. Derivado de README.md ("Install") e install.sh — el instalador es
// la única fuente de verdad: no añadir pasos que el script no haga.
// Los textos son pares { es, en } (ver src/i18n.jsx); los comandos y rutas son
// strings planos, idénticos en ambos idiomas.
//
// Install. Derived from README.md ("Install") and install.sh — the installer is
// the single source of truth; don't document steps the script doesn't do.
// Prose is { es, en } pairs; commands and paths are plain strings.

export const INSTALL_CMD = 'curl -fsSL https://raw.githubusercontent.com/drheavymetal/Runnir/main/install.sh | sh'

export const INSTALL_CMD_ALT = 'wget -qO- https://raw.githubusercontent.com/drheavymetal/Runnir/main/install.sh | sh'

// Lo que hace el instalador, en orden. / What the installer does, in order.
export const INSTALL_STEPS = [
  {
    k: 'git clone --depth 1',
    d: {
      es: 'Clona el repositorio en ~/.local/share/runnir/src (o lo actualiza si ya está).',
      en: 'Clones the repository into ~/.local/share/runnir/src (or updates it if already there).',
    },
  },
  {
    k: 'cargo build --release',
    d: {
      es: 'Compila runnir desde el código fuente. Puede tardar unos minutos la primera vez.',
      en: 'Builds runnir from source. Can take a few minutes the first time.',
    },
  },
  {
    k: '~/.local/bin/runnir',
    d: {
      es: 'Instala el binario en $PREFIX/bin/runnir.',
      en: 'Installs the binary to $PREFIX/bin/runnir.',
    },
  },
  {
    k: 'runnir-update · runnir-uninstall',
    d: {
      es: 'Deja dos comandos auxiliares junto al binario; ambos reejecutan la copia guardada de install.sh.',
      en: 'Drops two helper commands next to the binary; both re-exec the saved copy of install.sh.',
    },
  },
  {
    k: 'runnir.desktop + icono',
    d: {
      es: 'Solo en Linux: entrada .desktop e icono, para que runnir aparezca en el lanzador de aplicaciones. En macOS este paso se omite (el binario se instala igual).',
      en: 'Linux only: a .desktop entry and icon so runnir shows up in your app launcher. On macOS this step is skipped (the binary still installs).',
    },
  },
]

// Requisitos y variables de entorno. / Requirements and environment overrides.
export const INSTALL_NOTES = [
  {
    id: 'rust',
    title: { es: 'Toolchain de Rust', en: 'Rust toolchain' },
    body: {
      es: 'runnir se compila desde el código fuente, así que cargo tiene que estar disponible. Si no lo está, el instalador te remite a rustup y, cuando se ejecuta de forma interactiva, se ofrece a instalarlo por ti — nunca en silencio. Las instalaciones por tubería (curl … | sh) se detienen con instrucciones en vez de instalar un toolchain sin tu consentimiento.',
      en: 'runnir builds from source, so cargo must be available. If it isn’t, the installer points you at rustup and, when run interactively, offers to install it for you — never silently. Piped installs (curl … | sh) stop with instructions rather than installing a toolchain without your consent.',
    },
    code: "curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | sh",
  },
  {
    id: 'prefix',
    title: { es: 'PREFIX', en: 'PREFIX' },
    body: {
      es: 'PREFIX cambia el prefijo de instalación (por defecto $HOME/.local); el binario acaba en $PREFIX/bin/runnir. Con el valor por defecto no hace falta sudo. Una instalación para todo el sistema es el único caso en el que se permite ejecutar como root: el instalador rechaza root si no se ha fijado PREFIX explícitamente.',
      en: 'PREFIX overrides the install prefix (default $HOME/.local); the binary lands in $PREFIX/bin/runnir. With the default, no sudo is needed. A system-wide install is the only case where running as root is allowed: the installer refuses root unless PREFIX was set explicitly.',
    },
    code: 'PREFIX=/usr/local sh install.sh',
  },
  {
    id: 'path',
    title: { es: 'PATH', en: 'PATH' },
    body: {
      es: 'Si ~/.local/bin no está en tu PATH, el instalador te dice cómo añadirlo. Para fish:',
      en: 'If ~/.local/bin isn’t on your PATH, the installer tells you how to add it. For fish:',
    },
    code: 'fish_add_path ~/.local/bin',
    note: {
      es: 'Para bash/zsh/sh: export PATH="$HOME/.local/bin:$PATH" en el rc de tu shell.',
      en: 'For bash/zsh/sh: export PATH="$HOME/.local/bin:$PATH" in your shell rc.',
    },
  },
  {
    id: 'git',
    title: { es: 'git', en: 'git' },
    body: {
      es: 'El instalador clona el repositorio, así que git tiene que estar instalado. runnir se instala en Linux y macOS; cualquier otro sistema se rechaza.',
      en: 'The installer clones the repository, so git must be installed. runnir installs on Linux and macOS; any other system is rejected.',
    },
  },
]

// Mantenimiento: actualizar y desinstalar. / Maintenance: update and uninstall.
export const INSTALL_MAINTENANCE = [
  {
    id: 'update',
    title: { es: 'Actualizar', en: 'Update' },
    code: 'runnir-update',
    body: {
      es: 'Trae el último commit, recompila y reinstala. Tu configuración en ~/.config/runnir queda intacta.',
      en: 'Fetches the latest commit, rebuilds and reinstalls. Your config in ~/.config/runnir is left untouched.',
    },
  },
  {
    id: 'uninstall',
    title: { es: 'Desinstalar', en: 'Uninstall' },
    code: 'runnir-uninstall',
    body: {
      es: 'Borra el binario, los comandos auxiliares y la entrada .desktop. Conserva tu configuración y el código fuente cacheado (pregunta antes de borrar la caché). Con --purge se lleva también la caché y la configuración.',
      en: 'Removes the binary, the helper commands and the .desktop entry. Keeps your config and the cached source (it asks before removing the cache). Pass --purge to take the cache and the config too.',
    },
    note: {
      es: 'runnir-uninstall --purge borra además ~/.local/share/runnir y ~/.config/runnir.',
      en: 'runnir-uninstall --purge also removes ~/.local/share/runnir and ~/.config/runnir.',
    },
  },
]

// Dónde queda cada cosa. / Where everything ends up.
export const INSTALL_PATHS = [
  { k: '$PREFIX/bin/runnir', d: { es: 'El binario (por defecto ~/.local/bin/runnir).', en: 'The binary (by default ~/.local/bin/runnir).' } },
  { k: '$PREFIX/bin/runnir-update', d: { es: 'Comando auxiliar de actualización.', en: 'Update helper command.' } },
  { k: '$PREFIX/bin/runnir-uninstall', d: { es: 'Comando auxiliar de desinstalación.', en: 'Uninstall helper command.' } },
  { k: '~/.local/share/runnir/src', d: { es: 'El checkout de git desde el que se compila.', en: 'The git checkout it builds from.' } },
  { k: '~/.local/share/runnir/install.sh', d: { es: 'Copia del instalador que reejecutan los auxiliares.', en: 'Copy of the installer the helpers re-exec.' } },
  { k: '~/.local/share/applications/runnir.desktop', d: { es: 'Entrada del lanzador (solo Linux).', en: 'Launcher entry (Linux only).' } },
  { k: '~/.local/share/icons/hicolor/256x256/apps/runnir.png', d: { es: 'Icono de la aplicación (solo Linux).', en: 'Application icon (Linux only).' } },
  { k: '~/.config/runnir', d: { es: 'Tu configuración. El instalador nunca la toca salvo con --purge.', en: 'Your config. The installer never touches it unless you pass --purge.' } },
]

// El mismo install.sh sirve los tres flujos. / One install.sh drives all three flows.
export const INSTALL_FLOWS = 'sh install.sh --help'
