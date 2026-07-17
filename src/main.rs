mod boxdraw;
mod config;
mod font;
mod grid;
mod keys;
mod layout;
mod pty;
mod render;
mod selection;

use std::sync::{Arc, Mutex};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::font::FontAtlas;
use crate::grid::Grid;
use crate::keys::KeyMode;
use crate::pty::Pty;
use crate::render::{Renderer, Viewport};
use crate::selection::{Mode as SelMode, Selection};

/// Matches the `scrollback_lines`/`copy_on_select` habits from the user's kitty
/// config; real configuration lands in M6.
const WHEEL_LINES: f32 = 3.0;

const FONT_PX: f32 = 16.0;

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    grid: Arc<Mutex<Grid>>,
    pty: Pty,
    selection: Option<Selection>,
    selecting: bool,
    cursor_px: PhysicalPosition<f64>,
    clipboard: Option<arboard::Clipboard>,
}

impl Gpu {
    fn new(window: Arc<Window>) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).unwrap();

        // LowPower picks the iGPU on hybrid laptops. Waking the dGPU to draw text
        // costs battery and buys nothing.
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no suitable GPU adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("runnir"),
            ..Default::default()
        }))
        .expect("failed to create device");

        let size = window.inner_size();
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("adapter does not support this surface");
        surface.configure(&device, &config);

        let info = adapter.get_info();
        println!("runnir: {} ({:?})", info.name, info.backend);

        let font = FontAtlas::new(FONT_PX).expect("font");
        let renderer = Renderer::new(&device, config.format, font);

        let (cols, rows) = renderer.cells_for(config.width as f32, config.height as f32);
        let grid = Arc::new(Mutex::new(Grid::new(cols, rows)));

        let waker = window.clone();
        let pty = Pty::spawn(grid.clone(), None, move || waker.request_redraw())
            .expect("failed to spawn pty");

        Self {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            grid,
            pty,
            selection: None,
            selecting: false,
            cursor_px: PhysicalPosition::new(0.0, 0.0),
            clipboard: arboard::Clipboard::new().ok(),
        }
    }

    /// Pixel position -> absolute grid point, clamped to the viewport.
    fn point_at(&self, px: PhysicalPosition<f64>) -> selection::Point {
        let grid = self.grid.lock().unwrap();
        let col = (px.x as f32 / self.renderer.font.cell_w).floor().max(0.0) as usize;
        let row = (px.y as f32 / self.renderer.font.cell_h).floor().max(0.0) as usize;
        let row = row.min(grid.rows() - 1);
        (grid.abs_row(row), col.min(grid.cols() - 1))
    }

    fn copy_selection(&mut self) {
        let Some(sel) = self.selection else { return };
        let text = {
            let grid = self.grid.lock().unwrap();
            if sel.is_empty(&grid) {
                return;
            }
            sel.text(&grid)
        };
        if let Some(cb) = self.clipboard.as_mut() {
            let _ = cb.set_text(text);
        }
    }

    fn paste(&mut self) {
        let Some(text) = self.clipboard.as_mut().and_then(|cb| cb.get_text().ok()) else {
            return;
        };
        let bracketed = self.grid.lock().unwrap().bracketed_paste;
        if bracketed {
            // Without the brackets, a pasted newline runs the line immediately and
            // an editor auto-indents every line of the paste.
            self.pty.write(b"\x1b[200~");
            self.pty.write(text.as_bytes());
            self.pty.write(b"\x1b[201~");
        } else {
            self.pty.write(text.as_bytes());
        }
    }

    fn clear_selection(&mut self) {
        if self.selection.take().is_some() {
            self.renderer.invalidate();
            self.window.request_redraw();
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);

        let (cols, rows) = self.renderer.cells_for(self.config.width as f32, self.config.height as f32);
        self.grid.lock().unwrap().resize(cols, rows);
        // The child only learns the new size from the PTY, not from the window.
        self.pty.resize(cols as u16, rows as u16);
    }

    fn render(&mut self) {
        use wgpu::CurrentSurfaceTexture as Cst;
        let frame = match self.surface.get_current_texture() {
            Cst::Success(frame) | Cst::Suboptimal(frame) => frame,
            Cst::Outdated | Cst::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Cst::Timeout | Cst::Occluded => return,
            Cst::Validation => {
                eprintln!("runnir: surface validation error");
                return;
            }
        };

        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let screen = (self.config.width as f32, self.config.height as f32);

        {
            let mut grid = self.grid.lock().unwrap();
            // The PTY thread sets `dirty` from another thread; this is where we
            // notice and rebuild.
            if grid.dirty {
                grid.dirty = false;
                self.renderer.invalidate();
            }
            self.window.set_title(if grid.title.is_empty() { "runnir" } else { &grid.title });
            self.renderer.draw(
                &self.device,
                &self.queue,
                &mut encoder,
                &view,
                &grid,
                self.selection.as_ref(),
                Viewport { x: 0.0, y: 0.0, w: screen.0, h: screen.1 },
                screen,
            );
        }

        self.queue.submit(Some(encoder.finish()));
        self.window.pre_present_notify();
        self.queue.present(frame);
    }
}

#[derive(Default)]
struct App {
    gpu: Option<Gpu>,
    mods: ModifiersState,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("runnir")
            .with_inner_size(LogicalSize::new(960.0, 600.0));
        let window = Arc::new(event_loop.create_window(attrs).unwrap());
        self.gpu = Some(Gpu::new(window));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => gpu.resize(size.width, size.height),
            WindowEvent::RedrawRequested => gpu.render(),
            WindowEvent::ModifiersChanged(mods) => self.mods = mods.state(),

            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * WHEEL_LINES,
                    MouseScrollDelta::PixelDelta(p) => {
                        p.y as f32 / gpu.renderer.font.cell_h
                    }
                };
                if gpu.grid.lock().unwrap().scroll_display(lines.round() as isize) {
                    gpu.renderer.invalidate();
                    gpu.window.request_redraw();
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                gpu.cursor_px = position;
                if gpu.selecting {
                    let point = gpu.point_at(position);
                    if let Some(sel) = gpu.selection.as_mut() {
                        sel.update(point);
                        gpu.renderer.invalidate();
                        gpu.window.request_redraw();
                    }
                }
            }

            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => {
                    let point = gpu.point_at(gpu.cursor_px);
                    gpu.selection = Some(Selection::new(point, SelMode::Char));
                    gpu.selecting = true;
                    gpu.renderer.invalidate();
                    gpu.window.request_redraw();
                }
                ElementState::Released => {
                    gpu.selecting = false;
                    // copy_on_select, as in the user's kitty config.
                    gpu.copy_selection();
                }
            },

            // Middle click pastes the primary selection on X11/Wayland. arboard
            // gives us the clipboard, which is close enough until M6 config.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Middle,
                ..
            } => gpu.paste(),

            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                use winit::keyboard::{Key, NamedKey};

                let ctrl_shift = self.mods.control_key() && self.mods.shift_key();

                // Shift+PageUp/Down scrolls the view instead of reaching the child.
                if self.mods.shift_key() {
                    let scroll = match event.logical_key {
                        Key::Named(NamedKey::PageUp) => Some(gpu.grid.lock().unwrap().rows() as isize),
                        Key::Named(NamedKey::PageDown) => {
                            Some(-(gpu.grid.lock().unwrap().rows() as isize))
                        }
                        _ => None,
                    };
                    if let Some(delta) = scroll {
                        if gpu.grid.lock().unwrap().scroll_display(delta) {
                            gpu.renderer.invalidate();
                            gpu.window.request_redraw();
                        }
                        return;
                    }
                }

                if ctrl_shift {
                    match event.logical_key.as_ref() {
                        Key::Character("C") | Key::Character("c") => {
                            gpu.copy_selection();
                            return;
                        }
                        Key::Character("V") | Key::Character("v") => {
                            gpu.paste();
                            return;
                        }
                        _ => {}
                    }
                }

                let mode = KeyMode { app_cursor: gpu.grid.lock().unwrap().app_cursor };
                if let Some(bytes) = keys::encode(&event, self.mods, mode) {
                    // Typing into a scrolled-back view and seeing nothing happen is
                    // maddening; snap to the live output first.
                    if gpu.grid.lock().unwrap().scroll_to_bottom() {
                        gpu.renderer.invalidate();
                    }
                    gpu.clear_selection();
                    gpu.pty.write(&bytes);
                }
            }
            _ => {}
        }
    }
}

/// Runs `cmd` on a real PTY and prints the resulting grid as text. Checks the
/// parser without involving the GPU.
fn dump(cmd: &str) {
    let grid = Arc::new(Mutex::new(Grid::new(80, 24)));
    let mut pty = Pty::spawn(grid.clone(), Some(cmd), || {}).expect("failed to spawn pty");
    pty.wait();

    let grid = grid.lock().unwrap();
    println!("{}", grid.dump());
    let (row, col) = grid.cursor();
    eprintln!("--- cursor: row {row} col {col} | title: {:?}", grid.title);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--dump") => return dump(args.get(2).map(String::as_str).unwrap_or("echo hola")),
        Some("--render") => {
            let path = args.get(2).map(String::as_str).unwrap_or("/tmp/runnir.png");
            let cmd = args.get(3).map(String::as_str).unwrap_or("echo hola");
            let delay = args.get(4).and_then(|s| s.parse().ok());
            return render::offscreen(path, cmd, FONT_PX, delay);
        }
        _ => {}
    }

    let event_loop = EventLoop::new().unwrap();
    // Wait, not Poll: redraw only on demand. An idle terminal must not burn a core.
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut App::default()).unwrap();
}
