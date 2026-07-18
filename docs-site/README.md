# runnir — sitio de documentacion

Web de documentacion de **runnir** (emulador de terminal GPU, keyboard-first, en Rust
para Linux y macOS). Construida con Vite + React. Documenta todas las funciones —
disponibles y en desarrollo — en lenguaje llano (para cualquiera) y con una ficha
tecnica compacta (atajos, comandos de paleta, claves de config, secuencias de escape).
Contenido en espanol.

## Requisitos

- Node.js 18+ y npm.

## Comandos

```sh
npm install      # instalar dependencias (react, react-dom, vite, plugin-react)
npm run dev      # servidor de desarrollo con recarga en caliente (http://localhost:5173)
npm run build    # compila el sitio estatico a dist/
npm run preview  # sirve dist/ localmente para comprobarlo
```

## Deploy (GitHub Pages)

`vite.config.js` fija `base: './'`, asi que el sitio funciona en cualquier subruta.
El resultado de `npm run build` es la carpeta `dist/`, lista para publicar:

```sh
npm run build
# publica el contenido de dist/ en la rama gh-pages, por ejemplo:
npx gh-pages -d dist
# o, con GitHub Actions, sube dist/ como artefacto de Pages.
```

## Estructura

```
docs-site/
├── index.html
├── vite.config.js          # base: './' para GitHub Pages
├── public/
│   ├── favicon.svg
│   └── img/                # capturas REALES: runnir --render / --demo
└── src/
    ├── main.jsx
    ├── App.jsx             # vistas (Guia / Atajos / Config) + busqueda
    ├── styles.css          # tema oscuro estilo terminal (paleta runnir)
    ├── components/
    │   ├── Hero.jsx
    │   ├── Sidebar.jsx     # nav + buscador
    │   ├── FeatureCard.jsx # tarjeta por feature (llano + ficha tecnica)
    │   ├── TerminalDemo.jsx# maquetas CSS animadas (features dinamicas)
    │   ├── Kbd.jsx
    │   ├── KeybindingsPage.jsx
    │   └── ConfigPage.jsx
    └── data/
        ├── sections.js     # secciones
        ├── features.js     # TODAS las funciones documentadas
        ├── keybindings.js  # chuleta (de src/actions.rs + src/docs.rs)
        ├── config.js       # referencia de config (de src/config.rs)
        └── media.js        # mapa de capturas y demos por feature
```

## Fuentes de verdad

El contenido se derivo de: `docs/DEVLOG.md`, `src/docs.rs` (ayuda F1), `src/actions.rs`
(acciones + atajos + titulos de paleta) y `src/config.rs` (opciones + defaults). Las
capturas de `public/img/` se generaron con `runnir --render` y `runnir --demo`.

## Notas sobre las capturas

- `scene.png` — `runnir --demo`: escena multi-panel con la paleta de comandos.
- `colors.png`, `ligatures.png`, `boxdraw.png`, `underlines.png` — `runnir --render`.
- Funciones dinamicas (estela del cursor, campana, scroll suave, hover, gutter,
  minimapa) se ilustran con maquetas CSS animadas (`TerminalDemo`), porque el modo
  headless `--render` solo captura la rejilla, no el chrome ni la animacion.
