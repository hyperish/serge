//! GTK owns the menubar child and context menu; winit owns the application window.
use super::{DesktopError, Installed, Menu, menu::ContextMenu};
use gtk::{gdk, prelude::*};
use std::cell::Cell;
use winit::{
    event::{ElementState, WindowEvent},
    keyboard::{Key, ModifiersState},
    platform::{
        modifier_supplement::KeyEventExtModifierSupplement, scancode::PhysicalKeyExtScancode,
    },
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

struct Bar {
    menu: Menu,
    host: gtk::Window,
    content: gtk::Box,
    size: Cell<(i32, i32)>,
}
impl Drop for Bar {
    fn drop(&mut self) {
        let _ = self.menu.remove_for_gtk_window(&self.host);
        self.host.close();
    }
}
pub(crate) struct WindowMenus {
    parent: gdk::Window,
    bar: Option<Bar>,
    context: Option<Menu>,
    modifiers: Cell<ModifiersState>,
}

impl Installed<'_> {
    pub(crate) fn x11_menus(&self, window: &Window) -> Result<Option<WindowMenus>, DesktopError> {
        if self.bar.is_none() && self.context.is_none() {
            return Ok(None);
        }
        let raw = window
            .window_handle()
            .map_err(|e| DesktopError::Platform(e.to_string()))?;
        let xid = match raw.as_raw() {
            RawWindowHandle::Xlib(w) => w.window,
            RawWindowHandle::Xcb(w) => w.window.get().into(),
            _ => {
                return Err(DesktopError::Platform(
                    "Linux window menus require an X11 parent".into(),
                ));
            }
        };
        let display = gdk::Display::default()
            .and_then(|d| d.downcast::<gdkx11::X11Display>().ok())
            .ok_or_else(|| {
                DesktopError::Platform("Linux window menus require the GDK X11 backend".into())
            })?;
        let parent = gdkx11::X11Window::foreign_new_for_display(&display, xid).upcast();
        let bar = if let Some(menu) = &self.bar {
            let host = gtk::Window::new(gtk::WindowType::Popup);
            host.set_decorated(false);
            let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
            host.add(&content);
            menu.init_for_gtk_window(&host, Some(&content))
                .map_err(|e| DesktopError::Platform(e.to_string()))?;
            host.realize();
            let child = host.window().ok_or_else(|| {
                DesktopError::Platform("GTK could not realize the menubar".into())
            })?;
            child.reparent(&parent, 0, 0);
            host.show_all();
            child.raise();
            Some(Bar {
                menu: menu.clone(),
                host,
                content,
                size: Cell::default(),
            })
        } else {
            None
        };
        Ok(Some(WindowMenus {
            parent,
            bar,
            context: self.context.clone(),
            modifiers: Cell::default(),
        }))
    }
    pub(crate) fn pump_gtk(&self) -> bool {
        if self.bar.is_none() && self.context.is_none() && self._tray.is_none() {
            return false;
        }
        // The D-Bus tray schedules menu actions on GLib even without GTK windows.
        let events = gtk::glib::MainContext::default();
        for _ in 0..32 {
            if !events.iteration(false) {
                break;
            }
        }
        true
    }
}
impl WindowMenus {
    pub(crate) fn activate_shortcut(&self, event: &WindowEvent) -> bool {
        if let WindowEvent::ModifiersChanged(m) = event {
            self.modifiers.set(m.state());
        }
        let WindowEvent::KeyboardInput { event: key, .. } = event else {
            return false;
        };
        let Some(bar) = &self.bar else { return false };
        if key.state != ElementState::Pressed {
            return false;
        }
        let state = self.modifiers.get();
        let mut mods = gdk::ModifierType::empty();
        for (active, flag) in [
            (state.control_key(), gdk::ModifierType::CONTROL_MASK),
            (state.shift_key(), gdk::ModifierType::SHIFT_MASK),
            (state.alt_key(), gdk::ModifierType::MOD1_MASK),
            (state.super_key(), gdk::ModifierType::SUPER_MASK),
        ] {
            mods.set(flag, active);
        }
        let symbol = match key.key_without_modifiers() {
            Key::Character(text) => text
                .chars()
                .next()
                .filter(|c| c.len_utf8() == text.len())
                .map(|c| *gdk::keys::Key::from_unicode(c).to_lower()),
            // For non-text keys winit exposes evdev scancodes. XKB keycodes start at 8.
            _ => key.physical_key.to_scancode().and_then(|code| {
                gdk::Keymap::for_display(&self.parent.display())?
                    .translate_keyboard_state(code.checked_add(8)?, gdk::ModifierType::empty(), 0)
                    .map(|v| v.0)
            }),
        };
        symbol.is_some_and(|symbol| gtk::accel_groups_activate(&bar.host, symbol, mods))
    }
    pub(crate) fn viewport(&self, window: &Window) -> Option<serge_core::Rect> {
        let bar = self.bar.as_ref()?;
        let px = window.inner_size();
        let factor = f64::from(bar.host.scale_factor().max(1));
        let width = (f64::from(px.width) / factor).round().max(1.) as i32;
        let height = bar.content.preferred_height().1.max(1);
        if bar.size.replace((width, height)) != (width, height) {
            bar.host.resize(width, height);
            if let Some(child) = bar.host.window() {
                child.move_resize(0, 0, width, height);
            }
        }
        let scale = window.scale_factor();
        let top = f64::from(height) * factor / scale;
        Some(serge_core::Rect::new(
            0.,
            top as f32,
            (f64::from(px.width) / scale) as f32,
            (f64::from(px.height) / scale - top).max(0.) as f32,
        ))
    }
    pub(crate) fn context(&self, pointer: (f32, f32), window_scale: f64) {
        if let Some(menu) = &self.context {
            let scale = window_scale / f64::from(self.parent.scale_factor().max(1));
            let rect = gdk::Rectangle::new(
                (f64::from(pointer.0) * scale) as i32,
                (f64::from(pointer.1) * scale) as i32,
                1,
                1,
            );
            let mut trigger = gdk::Event::new(gdk::EventType::ButtonPress);
            trigger.set_device(
                self.parent
                    .display()
                    .default_seat()
                    .and_then(|seat| seat.pointer())
                    .as_ref(),
            );
            menu.gtk_context_menu().popup_at_rect(
                &self.parent,
                &rect,
                gdk::Gravity::SouthWest,
                gdk::Gravity::NorthWest,
                Some(&trigger),
            );
        }
    }
}
