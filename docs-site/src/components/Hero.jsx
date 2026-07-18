export default function Hero() {
  return (
    <header className="hero">
      <img className="hero-logo" src="./logo.png" alt="runnir — terminal" />
      <h1 className="sr-only">runnir</h1>
      <p className="hero-tag">
        Un emulador de terminal <b>GPU</b>, <b>keyboard-first</b>, escrito desde cero en
        Rust para <b>Linux y macOS</b>. Rapido, con integracion de shell de verdad,
        asistente de IA dentro del terminal y detalles que no vas a encontrar en otro.
      </p>
      <div className="hero-meta">
        <span className="pill"><b>GPU</b> · una sola llamada de dibujo</span>
        <span className="pill"><b>Rust</b> · wgpu (Vulkan/Metal/DX12)</span>
        <span className="pill">en reposo <b>no consume</b> nada</span>
        <span className="pill">config <b>TOML/JSON</b> · recarga en caliente</span>
      </div>

      <div className="term">
        <div className="term-bar">
          <span className="term-dot" style={{ background: '#f14c4c' }} />
          <span className="term-dot" style={{ background: '#e5b510' }} />
          <span className="term-dot" style={{ background: '#0dbc79' }} />
          <span className="term-title">runnir — ~/projects/runnir</span>
        </div>
        <div className="term-body">
          <div><span className="c-p">~/projects/runnir</span> <span className="c-g">&rsaquo;</span> cargo run</div>
          <div>   <span className="c-d">Compiling runnir v0.1.0</span></div>
          <div>    <span className="c-g">Finished</span> in 2.41s</div>
          <div><span className="c-p">~/projects/runnir</span> <span className="c-g">&rsaquo;</span> <span className="c-a">runnir --quake</span>   <span className="c-d"># terminal desplegable</span></div>
          <div><span className="c-p">~/projects/runnir</span> <span className="c-g">&rsaquo;</span> <span className="c-a">Ctrl+Shift+P</span>  <span className="c-d">&rarr; la paleta: todo es buscable</span></div>
          <div><span className="c-p">~/projects/runnir</span> <span className="c-g">&rsaquo;</span> <span className="term-cur">▊</span></div>
        </div>
      </div>
    </header>
  )
}
