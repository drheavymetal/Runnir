// Capturas REALES renderizadas headless con `runnir --render` / `--demo` en este
// repo. Clave = titulo exacto de la feature. Valor = { src, cap }.
export const MEDIA = {
  'Pestanas': { src: './img/scene.png', cap: 'Captura real (runnir --demo): dos pestanas, varios paneles y la paleta de comandos abierta.' },
  'Splits (paneles)': { src: './img/scene.png', cap: 'Captura real (runnir --demo): una pestana dividida en paneles independientes.' },
  'Teclado primero': { src: './img/scene.png', cap: 'Captura real (runnir --demo): la paleta de comandos con cada accion y su atajo.' },
  'Panel de asistente IA': { src: './img/scene.png', cap: 'Captura real (runnir --demo): escena multi-panel; el asistente vive en un panel mas.' },
  'Temas': { src: './img/colors.png', cap: 'Captura real (runnir --render): las 16 colores ANSI y una rampa truecolor.' },
  'Ligaturas': { src: './img/ligatures.png', cap: 'Captura real (runnir --render): ligaturas de fuente de codigo (->, =>, !=, >=, <=, ==).' },
  'Caracteres de dibujo de cajas': { src: './img/boxdraw.png', cap: 'Captura real (runnir --render): recuadros de linea simple y doble mas bloques de sombreado, dibujados al tamano de celda.' },
  'Subrayado normal': { src: './img/underlines.png', cap: 'Captura real (runnir --render): subrayado clasico (SGR 4). Los estilos ondulado/punteado/color son la parte En desarrollo.' },
}

// Demos ANIMADOS en CSS para funciones dinamicas que una captura estatica no
// transmite (parpadeo, estela, deslizamiento, subrayado al pasar el raton).
// Clave = titulo exacto. Valor = kind que interpreta <TerminalDemo>.
export const DEMOS = {
  'Estela del cursor': 'trail',
  'Campana visual y sonora': 'bell',
  'Scroll suave': 'smooth',
  'Resaltado de URL/ruta al pasar por encima': 'hover',
  'Gutter de estado por comando': 'gutter',
  'Minimapa del historial': 'minimap',
}
