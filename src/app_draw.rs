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

        // A bell flashes the whole window briefly.
        self.check_bells();

        // Window title tracks the focused pane.
        let title = self.tabs[self.active].title();
        self.window.set_title(if title.is_empty() { "runnir" } else { &title });

        // Lock every pane's grid up front; the render borrows them read-only.
        let area = self.active_area();
        let cell = self.renderer.cell_size();
        let rects = self.visible_rects(area);
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

        // Search-match highlight for the focused pane, when a search is open.
        let search = match &self.overlay {
            Some(Overlay::Search(s)) => crate::render::SearchHighlight {
                matches: &s.matches,
                len: s.query.chars().count(),
                current: s.current_match(),
            },
            _ => Default::default(),
        };

        let mut panes: Vec<PaneDraw> = guards
            .iter()
            .map(|(id, r, grid, tint, focused)| PaneDraw {
                grid,
                selection: self.tabs[self.active].panes[id].selection.as_ref(),
                origin: (r.x, r.y),
                tint: *tint,
                focused: *focused,
                cursor: (*focused && cursor_on && self.overlay.is_none()).then_some(shape),
                search: if *focused { search } else { Default::default() },
            })
            .collect();

        // The tab bar and any status chrome are grids too, appended as panes.
        let chrome = self.build_chrome(config, screen);
        for (grid, ox, oy) in &chrome {
            panes.push(PaneDraw { grid, selection: None, origin: (*ox, *oy), tint: None, focused: true, cursor: None, search: Default::default() });
        }

        // Sticky prompt: while scrolled back, pin the focused command's prompt line
        // at the top of its pane so you always see what you're reading. Built into a
        // local (not self) so it does not conflict with the guards' borrow. Read
        // from the already-held guard to avoid re-locking (would deadlock).
        let sticky_holder: Option<(Grid, f32, f32)> =
            guards.iter().find(|(id, ..)| *id == focus).and_then(|(_, r, grid, ..)| {
                grid.sticky_prompt().map(|text| {
                    let cols = (r.w / cell.0).floor().max(1.0) as usize;
                    let mut bar = Grid::new(cols, 1);
                    let bg = Color::Rgb(0x2a, 0x2c, 0x33);
                    bar.fill(Pen { bg, ..Pen::default() });
                    bar.write_str(0, 0, &text, Pen { fg: Color::Rgb(0xd4, 0xd6, 0xd9), bg, ..Pen::default() });
                    (bar, r.x, r.y)
                })
            });
        if let Some((grid, ox, oy)) = &sticky_holder {
            panes.push(PaneDraw { grid, selection: None, origin: (*ox, *oy), tint: None, focused: true, cursor: None, search: Default::default() });
        }

        // Hint labels annotate the focused pane, drawn as a top chrome grid. The
        // focused grid is already locked in `guards`, so read its top row from
        // there — locking it again on this thread would deadlock (Mutex is not
        // reentrant).
        let focus_top = guards
            .iter()
            .find(|(id, ..)| *id == focus)
            .map(|(_, _, grid, ..)| grid.abs_row(0));
        let hint_grid = focus_top.and_then(|top| self.build_hints(area, cell, focus, top));
        if let Some((grid, ox, oy)) = &hint_grid {
            panes.push(PaneDraw { grid, selection: None, origin: (*ox, *oy), tint: None, focused: true, cursor: None, search: Default::default() });
        }

        // Status toast (e.g. "whispering…"), centred near the top, with a spinner
        // so a pending AI request is unmistakably working.
        let toast: Option<(Grid, f32, f32)> = self.status.as_ref().map(|msg| {
            const SPIN: [char; 4] = ['⠋', '⠙', '⠹', '⠸'];
            let frame = (self.start.elapsed().as_millis() / 120) as usize % SPIN.len();
            let text = format!(" {} {} ", SPIN[frame], msg);
            let w = text.chars().count();
            let mut g = Grid::new(w, 1);
            let bg = Color::Rgb(config.theme.accent.0, config.theme.accent.1, config.theme.accent.2);
            g.fill(Pen { bg, ..Pen::default() });
            g.write_str(0, 0, &text, Pen { fg: Color::Rgb(0x0d, 0x0d, 0x0f), bg, ..Pen::default() });
            // Bottom-right, one row up from the very edge, clear of the prompt.
            let x = screen.0 - w as f32 * cell.0 - cell.0;
            let y = screen.1 - 2.0 * cell.1;
            (g, x.max(0.0), y.max(0.0))
        });
        if let Some((grid, ox, oy)) = &toast {
            panes.push(PaneDraw { grid, selection: None, origin: (*ox, *oy), tint: None, focused: true, cursor: None, search: Default::default() });
        }

        // The search bar draws as chrome (undimmed) so matches stay visible.
        let theme = self.renderer_theme();
        let search_bar: Option<(Grid, f32, f32)> = match &self.overlay {
            Some(Overlay::Search(s)) => {
                let cols = (screen.0 / cell.0).floor().max(1.0) as usize;
                let rows = (screen.1 / cell.1).floor().max(1.0) as usize;
                s.render(cols, rows, &theme)
                    .into_iter()
                    .next()
                    .map(|p| (p.grid, p.col as f32 * cell.0, p.row as f32 * cell.1))
            }
            _ => None,
        };
        if let Some((grid, ox, oy)) = &search_bar {
            panes.push(PaneDraw { grid, selection: None, origin: (*ox, *oy), tint: None, focused: true, cursor: None, search: Default::default() });
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
                    search: Default::default(),
                })
                .collect(),
        });

        // Pane dividers: a thin border on each pane when there is more than one, so
        // splits are visible. The focused pane gets the accent colour.
        let mut decorations: Vec<crate::render::SolidRect> = Vec::new();
        if rects.len() > 1 {
            let a = config.theme.accent;
            let accent = (a.0, a.1, a.2);
            let dim = (0x33, 0x36, 0x3d);
            for (id, r) in &rects {
                let color = if *id == focus { accent } else { dim };
                let t = 1.5;
                // Four edges of the pane.
                decorations.push(crate::render::SolidRect { x: r.x, y: r.y, w: r.w, h: t, color });
                decorations.push(crate::render::SolidRect { x: r.x, y: r.y + r.h - t, w: r.w, h: t, color });
                decorations.push(crate::render::SolidRect { x: r.x, y: r.y, w: t, h: r.h, color });
                decorations.push(crate::render::SolidRect { x: r.x + r.w - t, y: r.y, w: t, h: r.h, color });
            }
        }

        // Hover underline (D14): a thin accent line under the URL/path the pointer is
        // on, drawn as a decoration so it needs no per-cell plumbing.
        if let Some(h) = &self.hover_url {
            if let Some((_, r, grid, ..)) = guards.iter().find(|(id, ..)| *id == h.pane) {
                let top = grid.abs_row(0);
                if h.abs_row >= top {
                    let local = h.abs_row - top;
                    if local < grid.rows() {
                        let (cw, ch) = cell;
                        let a = config.theme.accent;
                        decorations.push(crate::render::SolidRect {
                            x: r.x + h.col as f32 * cw,
                            y: r.y + (local as f32 + 1.0) * ch - 2.0,
                            w: h.len as f32 * cw,
                            h: 1.5,
                            color: (a.0, a.1, a.2),
                        });
                    }
                }
            }
        }

        let flash = self.bell_alpha();
        self.renderer.render(
            &self.device,
            &self.queue,
            &mut encoder,
            &view,
            &panes,
            &decorations,
            overlay.as_ref(),
            flash,
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
            // Right-aligned indicators: broadcast, then the focused pane's context
            // (ssh host / root / docker), so the tab bar always says where you are.
            let mut right = cols;
            if self.broadcast {
                let tag = " BROADCAST ";
                right = right.saturating_sub(tag.len() + 1);
                bar.write_str(0, right, tag, Pen {
                    fg: Color::Rgb(0x0d, 0x0d, 0x0f),
                    bg: Color::Rgb(0xf1, 0x4c, 0x4c),
                    ..Pen::default()
                });
            }
            if let Some(label) = self.tabs[self.active].focused_ref().context.label() {
                let tag = format!(" {label} ");
                right = right.saturating_sub(tag.chars().count() + 1);
                bar.write_str(0, right, &tag, Pen {
                    fg: Color::Rgb(0xd4, 0xd6, 0xd9),
                    bg: Color::Rgb(0x2a, 0x2c, 0x33),
                    ..Pen::default()
                });
            }
            out.push((bar, 0.0, 0.0));
            let _ = ch;
        }
        out
    }

    fn build_hints(
        &self,
        area: Rect,
        cell: (f32, f32),
        focus: u64,
        top_abs: usize,
    ) -> Option<(Grid, f32, f32)> {
        let Some(Overlay::Hints(h)) = &self.overlay else { return None };
        let rect = self.tabs[self.active].layout(area).into_iter().find(|(id, _)| *id == focus)?.1;
        let (cw, ch) = cell;
        let cols = (rect.w / cw).floor().max(1.0) as usize;
        let rows = (rect.h / ch).floor().max(1.0) as usize;
        let mut grid = Grid::new(cols, rows);
        // Transparent-ish base: draw only labels, over a faint dim done by the
        // label cells themselves.
        grid.fill(Pen { bg: Color::Default, ..Pen::default() });

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
        // Hints and search draw as chrome (no dimmed backdrop) so the pane stays
        // fully visible behind them.
        if matches!(overlay, Overlay::Hints(_) | Overlay::Search(_)) {
            return None;
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
