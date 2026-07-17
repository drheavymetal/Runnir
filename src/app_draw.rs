// Rendering for `Gpu`. Included into main.rs.

impl Gpu {
    fn render(&mut self, config: &Config) {
        use wgpu::CurrentSurfaceTexture as Cst;
        let frame = match self.surface.get_current_texture() {
            Cst::Success(f) | Cst::Suboptimal(f) => f,
            Cst::Outdated | Cst::Lost => {
                self.surface.configure(&self.device, &self.surface_config);
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
        let screen = (self.surface_config.width as f32, self.surface_config.height as f32);

        // Window title tracks the focused pane.
        let title = self.tabs[self.active].title();
        self.window.set_title(if title.is_empty() { "runnir" } else { &title });

        // Lock every pane's grid up front; the render borrows them read-only.
        let area = self.active_area();
        let cell = self.renderer.cell_size();
        let rects = self.tabs[self.active].layout(area);
        let focus = self.tabs[self.active].focus;

        // Clear dirty flags so the next output marks a fresh redraw.
        for pane in self.tabs[self.active].panes.values() {
            pane.grid.lock().unwrap().dirty = false;
        }

        let guards: Vec<(u64, Rect, std::sync::MutexGuard<Grid>, Option<(u8, u8, u8)>, bool)> = rects
            .iter()
            .map(|(id, r)| {
                let pane = &self.tabs[self.active].panes[id];
                let grid = pane.grid.lock().unwrap();
                let tint = pane.context.tint();
                (*id, *r, grid, tint, *id == focus)
            })
            .collect();

        // The cursor shows on the focused pane only, off during the blink's dark
        // phase. An overlay owns input, so the terminal cursor is hidden then.
        let cursor_on = self.cursor_on(config);
        let shape = config.cursor.shape;

        let mut panes: Vec<PaneDraw> = guards
            .iter()
            .map(|(id, r, grid, tint, focused)| PaneDraw {
                grid,
                selection: self.tabs[self.active].panes[id].selection.as_ref(),
                origin: (r.x, r.y),
                tint: *tint,
                focused: *focused,
                cursor: (*focused && cursor_on && self.overlay.is_none()).then_some(shape),
            })
            .collect();

        // The tab bar and any status chrome are grids too, appended as panes.
        let chrome = self.build_chrome(config, screen);
        for (grid, ox, oy) in &chrome {
            panes.push(PaneDraw { grid, selection: None, origin: (*ox, *oy), tint: None, focused: true, cursor: None });
        }

        // Hint labels annotate the focused pane, drawn as a top chrome grid.
        let hint_grid = self.build_hints(area, cell, focus);
        if let Some((grid, ox, oy)) = &hint_grid {
            panes.push(PaneDraw { grid, selection: None, origin: (*ox, *oy), tint: None, focused: true, cursor: None });
        }

        // Overlay panels, on a dimmed backdrop.
        let overlay_grids = self.build_overlay(screen, cell);
        let overlay = overlay_grids.as_ref().map(|panels| OverlayDraw {
            dim: 0.55,
            panels: panels
                .iter()
                .map(|(g, ox, oy)| PaneDraw {
                    grid: g,
                    selection: None,
                    origin: (*ox, *oy),
                    tint: None,
                    focused: true,
                    cursor: None,
                })
                .collect(),
        });

        self.renderer.render(
            &self.device,
            &self.queue,
            &mut encoder,
            &view,
            &panes,
            overlay.as_ref(),
            screen,
        );

        drop(guards);
        self.queue.submit(Some(encoder.finish()));
        self.window.pre_present_notify();
        self.queue.present(frame);
    }

    /// The tab bar plus a broadcast/scroll indicator, as owned grids the caller
    /// draws. Empty when there is a single tab and nothing to indicate.
    fn build_chrome(&self, config: &Config, screen: (f32, f32)) -> Vec<(Grid, f32, f32)> {
        let (cw, ch) = self.renderer.cell_size();
        let cols = (screen.0 / cw).floor().max(1.0) as usize;
        let mut out = Vec::new();

        let multi = self.tabs.len() > 1;
        if multi {
            let mut bar = Grid::new(cols, 1);
            bar.fill(Pen { bg: Color::Rgb(0x15, 0x16, 0x1a), ..Pen::default() });
            let mut x = 1;
            for (i, tab) in self.tabs.iter().enumerate() {
                let label = format!(" {} {} ", i + 1, tab.title());
                let pen = if i == self.active {
                    Pen {
                        fg: Color::Rgb(0x0d, 0x0d, 0x0f),
                        bg: Color::Rgb(config.theme.accent.0, config.theme.accent.1, config.theme.accent.2),
                        ..Pen::default()
                    }
                } else {
                    Pen { fg: Color::Rgb(0x9a, 0x9d, 0xa4), bg: Color::Rgb(0x15, 0x16, 0x1a), ..Pen::default() }
                };
                bar.write_str(0, x, &label, pen);
                x += label.chars().count() + 1;
                if x >= cols {
                    break;
                }
            }
            // Broadcast indicator, right-aligned.
            if self.broadcast {
                let tag = " BROADCAST ";
                let pen = Pen {
                    fg: Color::Rgb(0x0d, 0x0d, 0x0f),
                    bg: Color::Rgb(0xf1, 0x4c, 0x4c),
                    ..Pen::default()
                };
                bar.write_str(0, cols.saturating_sub(tag.len() + 1), tag, pen);
            }
            out.push((bar, 0.0, 0.0));
            let _ = ch;
        }
        out
    }

    fn build_hints(&self, area: Rect, cell: (f32, f32), focus: u64) -> Option<(Grid, f32, f32)> {
        let Some(Overlay::Hints(h)) = &self.overlay else { return None };
        let rect = self.tabs[self.active].layout(area).into_iter().find(|(id, _)| *id == focus)?.1;
        let (cw, ch) = cell;
        let cols = (rect.w / cw).floor().max(1.0) as usize;
        let rows = (rect.h / ch).floor().max(1.0) as usize;
        let mut grid = Grid::new(cols, rows);
        // Transparent-ish base: draw only labels, over a faint dim done by the
        // label cells themselves.
        grid.fill(Pen { bg: Color::Default, ..Pen::default() });

        let pane = &self.tabs[self.active].panes[&focus];
        let g = pane.grid.lock().unwrap();
        let top_abs = g.abs_row(0);
        for hint in &h.hints {
            if hint.abs_row < top_abs || hint.abs_row >= top_abs + rows {
                continue;
            }
            let row = hint.abs_row - top_abs;
            let pen = Pen {
                fg: Color::Rgb(0x0d, 0x0d, 0x0f),
                bg: Color::Rgb(0xf5, 0xd5, 0x43),
                flags: crate::grid::Flags::BOLD,
                ..Pen::default()
            };
            grid.write_str(row, hint.col, &hint.label, pen);
        }
        Some((grid, rect.x, rect.y))
    }

    fn build_overlay(&self, screen: (f32, f32), cell: (f32, f32)) -> Option<Vec<(Grid, f32, f32)>> {
        let overlay = self.overlay.as_ref()?;
        if matches!(overlay, Overlay::Hints(_)) {
            return None; // Hints are drawn as chrome, not a modal.
        }
        let (cw, ch) = cell;
        let cols = (screen.0 / cw).floor().max(1.0) as usize;
        let rows = (screen.1 / ch).floor().max(1.0) as usize;
        let panels = overlay.render(cols, rows, &self.renderer_theme());
        Some(
            panels
                .into_iter()
                .map(|p| (p.grid, p.col as f32 * cw, p.row as f32 * ch))
                .collect(),
        )
    }

    fn renderer_theme(&self) -> crate::config::Theme {
        crate::config::Theme::default()
    }

    /// Whether the cursor is in its visible phase. Steady when blink is off; else a
    /// square wave over `blink_interval`. Time comes from the process start so it
    /// needs no per-frame state.
    fn cursor_on(&self, config: &Config) -> bool {
        if !config.cursor.blink {
            return true;
        }
        let ms = self.start.elapsed().as_millis() as u64;
        let interval = config.cursor.blink_interval.max(50);
        (ms / interval) % 2 == 0
    }

    /// Writes the current layout and scrollback to the session file. Called before
    /// exit and on a slow autosave timer, so a crash still leaves a recent state.
    pub fn save_session(&self, config: &Config) {
        if !config.behaviour.restore_session {
            return;
        }
        let mut sess = session::Session::new(self.active);
        for tab in &self.tabs {
            let state = tab.to_session();
            sess.tabs.push(state);
        }
        if let Err(e) = sess.save() {
            eprintln!("runnir: could not save session: {e}");
        }
    }
}
