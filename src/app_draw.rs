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

        // Drop a zoom that no longer holds (its pane closed or lost focus) so input
        // never lands on a pane the zoom hides.
        self.sync_zoom();

        // Lock every pane's grid up front; the render borrows them read-only.
        let area = self.active_area();
        let cell = self.renderer.cell_size();
        let rects = self.visible_rects(area);
        let focus = self.tabs[self.active].focus;

        // Build the chrome (tab bar, status bar) BEFORE locking the pane grids: the
        // tab badges read each pane's grid (last exit / dirty), which would re-lock a
        // grid this thread already holds via `guards` and deadlock. Chrome only needs
        // transient `&self` reads, so it is safe here.
        let chrome = self.build_chrome(config, screen);
        let status_holder = self.build_status(config, screen);
        let whichkey_holder = self.build_whichkey(screen);

        // Clear dirty flags so the next output marks a fresh redraw. (After chrome, so
        // the activity badge still reflects this frame's unseen output.)
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

        // The tab bar and any status chrome are grids too (built above), appended.
        for (grid, ox, oy) in &chrome {
            panes.push(PaneDraw { grid, selection: None, origin: (*ox, *oy), tint: None, focused: true, cursor: None, search: Default::default() });
        }
        if let Some((grid, ox, oy)) = &status_holder {
            panes.push(PaneDraw { grid, selection: None, origin: (*ox, *oy), tint: None, focused: true, cursor: None, search: Default::default() });
        }
        if let Some((grid, ox, oy)) = &whichkey_holder {
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
        let hint_grid = guards
            .iter()
            .find(|(id, ..)| *id == focus)
            .and_then(|(_, _, grid, ..)| self.build_hints(area, cell, focus, grid));
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

        // Command status gutter (D6): a short vertical bar at the left edge of each
        // command's prompt row — green for exit 0, red for a failure, dim grey while
        // it is unknown/running — so the pass/fail history is glanceable.
        for (id, r, grid, ..) in &guards {
            let _ = id;
            let (_, ch) = cell;
            for (row, exit) in grid.command_markers() {
                let color = match exit {
                    Some(0) => (0x3f, 0xb9, 0x50),
                    Some(_) => (0xe0, 0x4f, 0x4f),
                    None => (0x55, 0x58, 0x5f),
                };
                decorations.push(crate::render::SolidRect {
                    x: r.x,
                    y: r.y + row as f32 * ch + 1.0,
                    w: 2.0,
                    h: (ch - 2.0).max(1.0),
                    color,
                });
            }
        }

        // Cursor trail (D15, opt-in flair): leave fading ghosts where the focused
        // cursor was as it jumps. Ghosts fade toward the background, so they can be
        // drawn as opaque decorations (colour pre-blended by remaining life).
        if config.cursor.trail {
            let (cw, ch) = cell;
            // Only sample while following live output: a scrolled-back cursor is drawn
            // elsewhere (or not at all), so a ghost there would land on unrelated rows.
            let sampled = guards
                .iter()
                .find(|(id, ..)| *id == focus)
                .filter(|(_, _, grid, ..)| grid.display_offset() == 0)
                .map(|(_, r, grid, ..)| {
                    let (crow, ccol) = grid.cursor();
                    (r.x + ccol as f32 * cw, r.y + crow as f32 * ch, cw, ch)
                });
            match sampled {
                Some(rect) => {
                    // Push a ghost only for a move of the SAME pane's cursor, not for
                    // a focus/tab/resize jump.
                    if let Some((pid, px, py, pw, ph)) = self.last_cursor_rect {
                        if pid == focus && (px, py, pw, ph) != rect {
                            self.cursor_trail.push((px, py, pw, ph, std::time::Instant::now()));
                        }
                    }
                    self.last_cursor_rect = Some((focus, rect.0, rect.1, rect.2, rect.3));
                }
                None => self.last_cursor_rect = None,
            }
            let now = std::time::Instant::now();
            const LIFE_MS: f32 = 180.0;
            self.cursor_trail
                .retain(|g| now.saturating_duration_since(g.4).as_millis() as f32 <= LIFE_MS);
            let cc = config.theme.cursor;
            let bg = config.theme.background;
            for g in &self.cursor_trail {
                let age = now.saturating_duration_since(g.4).as_millis() as f32;
                let f = (1.0 - age / LIFE_MS).clamp(0.0, 1.0) * 0.45;
                let mix = |a: u8, b: u8| (a as f32 * (1.0 - f) + b as f32 * f) as u8;
                decorations.push(crate::render::SolidRect {
                    x: g.0,
                    y: g.1,
                    w: g.2,
                    h: g.3,
                    color: (mix(bg.0, cc.0), mix(bg.1, cc.1), mix(bg.2, cc.2)),
                });
            }
        } else if !self.cursor_trail.is_empty() {
            self.cursor_trail.clear();
            self.last_cursor_rect = None;
        }

        // Progress bar (OSC 9;4): a thin bar along the bottom edge of any pane whose
        // foreground reports progress (npm, cargo-style tools) — accent for normal,
        // red for error, amber for paused, full-width dim for indeterminate.
        for (_, r, grid, ..) in &guards {
            let Some((state, pct)) = grid.progress() else { continue };
            let a = config.theme.accent;
            let (color, frac) = match state {
                2 => ((0xe0, 0x4f, 0x4f), pct as f32 / 100.0),
                4 => ((0xe8, 0xb3, 0x39), pct as f32 / 100.0),
                3 => ((a.0, a.1, a.2), 1.0),
                _ => ((a.0, a.1, a.2), pct as f32 / 100.0),
            };
            decorations.push(crate::render::SolidRect {
                x: r.x,
                y: r.y + r.h - 2.0,
                w: (r.w * frac).max(1.0),
                h: 2.0,
                color,
            });
        }

        // Minimap: a slim overview of the whole scrollback on the focused pane's
        // right edge — one dim bar per sampled line (width = how full the line is),
        // with the current viewport window highlighted. Click it to jump.
        if config.window.minimap {
            if let Some((_, r, grid, ..)) = guards.iter().find(|(id, ..)| *id == focus).filter(|(_, r, ..)| r.w > crate::MINIMAP_W) {
                let total = grid.total_rows();
                let strip_w = crate::MINIMAP_W;
                let x0 = r.x + r.w - strip_w;
                let rows_px = (r.h as usize).max(1);
                // One screen pixel-row per map entry, sampling the scrollback.
                let steps = rows_px.min(total.max(1));
                let a = config.theme.accent;
                // Each row is drawn as its runs of ink, in the colour the text
                // actually has, so the strip reads as a shrunken picture of the
                // screen: indentation, blank lines and coloured output are all
                // recognisable. A run narrower than a pixel still renders — the quad
                // just covers part of one, which reads as a lighter mark.
                let cw = strip_w / grid.cols().max(1) as f32;
                let h = (r.h / steps as f32).max(1.0);
                let mut runs = Vec::new();
                for s in 0..steps {
                    let abs = s * total / steps.max(1);
                    grid.row_runs_into(abs, &mut runs);
                    let y = r.y + (s as f32 / steps as f32) * r.h;
                    for &(col, len, colour) in &runs {
                        let rgb = match colour {
                            crate::grid::Color::Default => config.theme.foreground,
                            crate::grid::Color::Rgb(cr, cg, cb) => crate::config::Rgb(cr, cg, cb),
                            crate::grid::Color::Indexed(i) => {
                                crate::render::xterm256(i, &config.theme.ansi)
                            }
                        };
                        decorations.push(crate::render::SolidRect {
                            x: x0 + col as f32 * cw,
                            y,
                            w: len as f32 * cw,
                            h,
                            color: (rgb.0, rgb.1, rgb.2),
                        });
                    }
                }
                // Highlight the visible window.
                let top = grid.total_rows() - grid.rows() - grid.display_offset();
                let vy = r.y + (top as f32 / total.max(1) as f32) * r.h;
                let vh = (grid.rows() as f32 / total.max(1) as f32 * r.h).max(3.0);
                decorations.push(crate::render::SolidRect {
                    x: x0 - 1.0,
                    y: vy,
                    w: 1.5,
                    h: vh,
                    color: (a.0, a.1, a.2),
                });
            }
        }

        // Scroll position indicator: a thin thumb on the right edge of any pane that
        // is scrolled back, sized and placed by the viewport's position in history.
        for (id, r, grid, ..) in &guards {
            // The focused pane's position is already shown by the minimap when on —
            // but only when the minimap actually drew (a pane too narrow for the strip
            // still needs its thumb).
            if config.window.minimap && *id == focus && r.w > crate::MINIMAP_W {
                continue;
            }
            let off = grid.display_offset();
            if off == 0 {
                continue;
            }
            let total = grid.total_rows().max(1) as f32;
            let rows = grid.rows() as f32;
            let sb = (grid.total_rows() - grid.rows()) as f32;
            let top = (sb - off as f32).max(0.0);
            let thumb_h = (rows / total * r.h).max(8.0);
            let thumb_y = r.y + (top / total) * r.h;
            let a = config.theme.accent;
            decorations.push(crate::render::SolidRect {
                x: r.x + r.w - 3.0,
                y: thumb_y.min(r.y + r.h - thumb_h),
                w: 3.0,
                h: thumb_h,
                color: (a.0, a.1, a.2),
            });
        }

        // Hover underline (D14): a thin accent line under the URL/path the pointer is
        // on, drawn as a decoration so it needs no per-cell plumbing.
        if let Some(h) = &self.hover_url {
            if let Some((_, r, grid, ..)) = guards.iter().find(|(id, ..)| *id == h.pane) {
                if let Some(local) = grid.screen_row_of(h.abs_row) {
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
            // Scroll the tab strip so the active tab is always visible when tabs
            // overflow the bar.
            let (offset, avail_end) = self.tab_scroll(cols);
            let mut x = 1usize;
            for i in 0..self.tabs.len() {
                // Use the same label tab_scroll/tab_bar_hit use (icon + badge), and
                // its display width, so drawing, scrolling and clicks all agree.
                let label = self.tab_label(i);
                let w = Self::label_w(&label);
                let drawn = x as isize - offset as isize;
                // Only draw tabs whose start is within the visible window.
                if drawn >= 1 && (drawn as usize) < avail_end {
                    let pen = if i == self.active {
                        Pen {
                            fg: Color::Rgb(0x0d, 0x0d, 0x0f),
                            bg: Color::Rgb(config.theme.accent.0, config.theme.accent.1, config.theme.accent.2),
                            ..Pen::default()
                        }
                    } else {
                        Pen { fg: Color::Rgb(0x9a, 0x9d, 0xa4), bg: Color::Rgb(0x15, 0x16, 0x1a), ..Pen::default() }
                    };
                    bar.write_str(0, drawn as usize, &label, pen);
                    // Recolour the badge glyph (last non-space cell) red/amber so a
                    // fail or activity dot stands out from the label text.
                    if let Some((ch, col)) = self.tab_badge(i) {
                        let bcol = drawn as usize + w - 2;
                        if bcol < avail_end {
                            bar.write_str(0, bcol, &ch.to_string(), Pen { fg: Color::Rgb(col.0, col.1, col.2), bg: pen.bg, ..Pen::default() });
                        }
                    }
                }
                x += w + 1;
            }
            // Chevrons hint at tabs scrolled off either edge.
            let chevron = Pen { fg: Color::Rgb(0x6a, 0x6d, 0x74), bg: Color::Rgb(0x15, 0x16, 0x1a), ..Pen::default() };
            if offset > 0 {
                bar.write_str(0, 0, "\u{25c2}", chevron);
            }
            if x.saturating_sub(offset) > avail_end {
                bar.write_str(0, avail_end.saturating_sub(1), "\u{25b8}", chevron);
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
                right = right.saturating_sub(Self::label_w(&tag) + 1);
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

    /// The bottom status bar: cwd (home-abbreviated) and git branch on the left, the
    /// clock on the right. `None` when disabled.
    fn build_status(&self, config: &Config, screen: (f32, f32)) -> Option<(Grid, f32, f32)> {
        if !self.status_bar {
            return None;
        }
        let (cw, ch) = self.renderer.cell_size();
        let cols = (screen.0 / cw).floor().max(1.0) as usize;
        let y = screen.1 - ch;
        let mut bar = Grid::new(cols, 1);
        let bg = Color::Rgb(0x12, 0x13, 0x17);
        bar.fill(Pen { bg, ..Pen::default() });

        let pane = self.tabs[self.active].focused_ref();
        let cwd = pane.cwd().map(|p| abbreviate_home(&p)).unwrap_or_default();
        // Two sources on purpose. The branch comes from `.git/HEAD`, one file read,
        // so it is correct the instant a checkout finishes. The counts come from the
        // worker's cache and lag by however long `git status` took — which is why
        // they are never the thing that names the branch.
        let repo = pane
            .cwd()
            .and_then(|p| crate::git::repo_root(&p))
            .and_then(|root| self.git_state.get(&root));
        let branch = pane.cwd().and_then(|p| crate::git::head_branch(&p)).map(|head| match repo {
            Some(state) => crate::git::status_text(&crate::git::RepoState { branch: head, ..state.clone() }),
            None => head,
        });
        let a = config.theme.accent;
        let dim = Pen { fg: Color::Rgb(0x8a, 0x8d, 0x94), bg, ..Pen::default() };
        let accent = Pen { fg: Color::Rgb(a.0, a.1, a.2), bg, ..Pen::default() };

        // Leader chip, leftmost so it lands where the eye already is when a modal
        // layer swallows the keyboard. Reversed accent, not just coloured text: the
        // point is "runnir is holding your next keystroke", which has to read at a
        // glance. Expiry is checked here too — `about_to_wait` repaints on the
        // deadline, but a repaint for any other reason must not draw a dead chip.
        let armed = self.leader_armed.is_some_and(|t| self.leader_timeout.is_none_or(|d| t.elapsed() < d));
        let mut x = 1;
        if armed {
            let chip = Pen {
                fg: Color::Rgb(0x12, 0x13, 0x17),
                bg: Color::Rgb(a.0, a.1, a.2),
                flags: crate::grid::Flags::BOLD,
                ..Pen::default()
            };
            let s = " LEADER ";
            bar.write_str(0, x, s, chip);
            x += s.chars().count() + 1;
        }

        bar.write_str(0, x, &cwd, dim);
        x += cwd.chars().count() + 2;
        if let Some(b) = &branch {
            let s = format!("\u{e0a0} {b}"); //  branch glyph
            bar.write_str(0, x, &s, accent);
            x += s.chars().count() + 2;
        }
        let _ = x;
        // Right: clock.
        if !self.clock.is_empty() {
            let s = format!("\u{f017} {} ", self.clock); //  clock glyph
            let right = cols.saturating_sub(s.chars().count());
            bar.write_str(0, right, &s, dim);
        }
        Some((bar, 0.0, y))
    }

    /// The which-key panel: what the armed leader layer will accept next, in
    /// columns across the bottom of the screen, just above the status bar.
    ///
    /// Drawn as chrome rather than an `Overlay` on purpose — an Overlay captures
    /// the keyboard, and the whole point here is that the next keystroke goes to
    /// the leader resolver. It also means no dimmed backdrop: this is a hint, not
    /// a modal, and the terminal stays readable behind it.
    fn build_whichkey(&self, screen: (f32, f32)) -> Option<(Grid, f32, f32)> {
        if self.leader_armed.is_none() || self.leader_entries.is_empty() {
            return None;
        }
        let (cw, ch) = self.renderer.cell_size();
        let cols = (screen.0 / cw).floor().max(20.0) as usize;
        let path: Vec<String> = self.leader_path.iter().map(|c| c.label()).collect();
        let grid = whichkey_grid(&self.leader_entries, &path, cols);

        // Sits directly on top of the status bar when there is one.
        let bar = if self.status_bar { ch } else { 0.0 };
        let y = screen.1 - bar - grid.rows() as f32 * ch;
        Some((grid, 0.0, y.max(0.0)))
    }

    fn build_hints(
        &self,
        area: Rect,
        cell: (f32, f32),
        focus: u64,
        src: &crate::grid::Grid,
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
            // Fold-aware placement: a hint on a folded (hidden) row is skipped.
            let Some(row) = src.screen_row_of(hint.abs_row) else { continue };
            if row >= rows {
                continue;
            }
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
        if config.behaviour.restore_session {
            let mut sess = session::Session::new(self.active);
            for tab in &self.tabs {
                let state = tab.to_session();
                sess.tabs.push(state);
            }
            if let Err(e) = sess.save() {
                eprintln!("runnir: could not save session: {e}");
            }
        }
        // Per-project layout auto-save (opt-in, independent of restore_session): record
        // this project's arrangement so reopening in it can restore the panes and cwds.
        if config.behaviour.session_auto_save {
            if let Err(e) = self.save_project_session() {
                eprintln!("runnir: could not save project session: {e}");
            }
        }
    }
}

/// Replaces a leading $HOME with `~` for the status bar.
fn abbreviate_home(p: &std::path::Path) -> String {
    let s = p.to_string_lossy();
    if let Some(home) = std::env::var_os("HOME") {
        let home = home.to_string_lossy();
        if let Some(rest) = s.strip_prefix(home.as_ref()) {
            return format!("~{rest}");
        }
    }
    s.into_owned()
}

/// Lays out the which-key panel for one level of the leader layer.
///
/// Free function rather than a `Gpu` method so the headless scene renderer
/// (`runnir --demo leader`) draws the exact same panel the app draws — a
/// screenshot of the layer can never drift from the layer itself.
///
/// `entries` is `(key, title, is_group)` as `Keymap::leader_entries` returns it,
/// `path` the keys pressed since the leader was armed (empty at the root).
fn whichkey_grid(entries: &[(String, String, bool)], path: &[String], cols: usize) -> Grid {
    // Column width from the widest entry, so nothing is truncated at the root
    // (the level with the most entries and the longest titles).
    let widest = entries.iter().map(|(k, t, _)| k.chars().count() + t.chars().count()).max().unwrap_or(10);
    let colw = (widest + 6).min(cols.saturating_sub(2)).max(12);
    let per_row = (cols.saturating_sub(2) / colw).max(1);
    let rows = entries.len().div_ceil(per_row);

    let bg = Color::Rgb(0x1a, 0x1c, 0x22);
    let mut grid = Grid::new(cols, rows + 2);
    grid.fill(Pen { bg, ..Pen::default() });

    let dim = Pen { fg: Color::Rgb(0x8a, 0x8d, 0x94), bg, ..Pen::default() };
    let key = Pen { fg: Color::Rgb(0xf5, 0xd5, 0x43), bg, flags: crate::grid::Flags::BOLD, ..Pen::default() };
    let text = Pen { fg: Color::Rgb(0xd4, 0xd6, 0xd9), bg, ..Pen::default() };
    // Groups are told apart by colour, not by a suffix: a trailing arrow costs
    // width in every column and this reads faster.
    let grp = Pen { fg: Color::Rgb(0x6b, 0xb1, 0xff), bg, ..Pen::default() };

    // Header names where you are: the keys pressed so far, or the root.
    let header = if path.is_empty() {
        "LEADER  —  Esc cancels".to_string()
    } else {
        format!("LEADER {}  —  Esc cancels", path.join(" "))
    };
    grid.write_str(0, 1, &header, dim);

    for (i, (k, title, is_group)) in entries.iter().enumerate() {
        let row = 1 + i / per_row;
        let col = 1 + (i % per_row) * colw;
        grid.write_str(row, col, k, key);
        let tcol = col + k.chars().count() + 2;
        if tcol + 1 < cols {
            let room = (col + colw).min(cols).saturating_sub(tcol + 1);
            let t: String = title.chars().take(room).collect();
            grid.write_str(row, tcol, &t, if *is_group { grp } else { text });
        }
    }
    grid
}
