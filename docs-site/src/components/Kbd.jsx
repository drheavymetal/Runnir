// Renderiza una cadena de tecla como <kbd>, separando un parentesis explicativo.
// "Ctrl+Shift+T (nueva)" -> <kbd>Ctrl+Shift+T</kbd> <span>(nueva)</span>
export default function Kbd({ text }) {
  const m = text.match(/^(.*?)\s*(\(.*\))\s*$/)
  const chord = m ? m[1].trim() : text
  const hint = m ? m[2] : null
  return (
    <span className="kbd-wrap">
      <kbd>{chord}</kbd>
      {hint && <span className="plain" style={{ color: 'var(--fg-faint)', fontSize: '12px' }}> {hint}</span>}
    </span>
  )
}
