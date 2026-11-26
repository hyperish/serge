use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use winit::{
    dpi::{LogicalSize, PhysicalPosition},
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes, WindowLevel},
};

/// Initial behavior of a desktop window. Give each restorable window a distinct file.
#[derive(Debug, Default, Clone)]
pub struct WindowPlacement {
    /// Keep this window above ordinary windows, subject to the window manager.
    pub floating: bool,
    /// Restore on open and atomically persist normal bounds and maximized state on close.
    pub bounds_file: Option<PathBuf>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Bounds {
    width: f64,
    height: f64,
    // Physical desktop coordinates are meaningful only while that monitor still exists.
    position: Option<(i32, i32)>,
    maximized: bool,
}
pub(super) struct State {
    options: WindowPlacement,
    bounds: Option<Bounds>,
    outer: Option<PhysicalPosition<i32>>,
}
impl State {
    pub(super) fn new(options: WindowPlacement) -> Self {
        let bounds = options.bounds_file.as_deref().and_then(|path| {
            use std::io::Read;
            let file = fs::File::open(path).ok()?;
            if file.metadata().ok()?.len() > 4096 {
                return None;
            }
            let b: Bounds = serde_json::from_reader(file.take(4096)).ok()?;
            (b.width.is_finite() && b.height.is_finite() && b.width > 0. && b.height > 0.)
                .then_some(b)
        });
        Self {
            options,
            bounds,
            outer: None,
        }
    }
    pub(super) fn attributes(
        &mut self,
        el: &ActiveEventLoop,
        mut attrs: WindowAttributes,
    ) -> WindowAttributes {
        if self.options.floating {
            attrs = attrs.with_window_level(WindowLevel::AlwaysOnTop);
        }
        let Some(b) = self.bounds else {
            return attrs;
        };
        let original = b.position.and_then(|(x, y)| {
            el.available_monitors().find(|m| {
                let start = m.position();
                let size = m.size();
                let dx = i64::from(x) - i64::from(start.x);
                let dy = i64::from(y) - i64::from(start.y);
                dx >= 0
                    && dy >= 0
                    && dx + 32 <= i64::from(size.width)
                    && dy + 32 <= i64::from(size.height)
            })
        });
        if original.is_some()
            && let Some((x, y)) = b.position
        {
            let position = PhysicalPosition::new(x, y);
            attrs = attrs.with_position(position);
            self.outer = (!b.maximized).then_some(position);
        }
        let mut size = LogicalSize::new(b.width, b.height);
        if let Some(m) = original.or_else(|| el.primary_monitor()) {
            let screen = m.size().to_logical::<f64>(m.scale_factor());
            size.width = size.width.min(screen.width).max(1.);
            size.height = size.height.min(screen.height).max(1.);
        }
        attrs.with_inner_size(size).with_maximized(b.maximized)
    }
    pub(super) fn place(&self, window: &Window) {
        // Creation coordinates can refer to the content rectangle on AppKit. Apply
        // the saved outer-frame position once the OS knows the title-bar dimensions.
        if let Some(position) = self.outer {
            window.set_outer_position(position);
        }
    }
    pub(super) fn observe(&mut self, window: &Window) {
        if self.options.bounds_file.is_none()
            || window.is_minimized() == Some(true)
            || window.fullscreen().is_some()
        {
            return;
        }
        if window.is_maximized() {
            if let Some(b) = &mut self.bounds {
                b.maximized = true;
            }
            return;
        }
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let size = size.to_logical::<f64>(window.scale_factor());
        self.bounds = Some(Bounds {
            width: size.width,
            height: size.height,
            position: window.outer_position().ok().map(|p| (p.x, p.y)),
            maximized: false,
        });
    }
    pub(super) fn save(&mut self, window: &Window) -> io::Result<()> {
        self.observe(window);
        let (Some(file), Some(bounds)) = (&self.options.bounds_file, self.bounds) else {
            return Ok(());
        };
        let dir = file
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(dir)?;
        let mut temp = tempfile::NamedTempFile::new_in(dir)?;
        serde_json::to_writer(temp.as_file_mut(), &bounds).map_err(io::Error::other)?;
        temp.as_file().sync_all()?;
        temp.persist(file)?;
        Ok(())
    }
}
