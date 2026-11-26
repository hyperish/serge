//! Exercise an installed AppKit main menu and status-item menu on the UI thread.
#[cfg(target_os = "macos")]
fn main() {
    if std::env::var_os("SERGE_TEST_WINDOWS").is_none() {
        return;
    }
    use hyve::Scope;
    use objc2::{MainThreadMarker, rc::Retained};
    use objc2_app_kit::{
        NSApplication, NSButton, NSMenu, NSPopover, NSStatusBarButton, NSView, NSViewController,
    };
    use serge::{
        desktop::{
            NativeEvent, WindowEvent, WindowHandle, WindowOptions, WindowSpec,
            system::{
                self, DesktopSystem, Icon, MenuBarPopover, Tray,
                menu::{ContextMenu, Menu, MenuItem, Submenu},
            },
        },
        prelude::*,
    };
    use std::{
        cell::{Cell, OnceCell, RefCell},
        rc::Rc,
        time::Duration,
    };
    let (finished, watch) = std::sync::mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if watch.recv_timeout(Duration::from_secs(45)).is_err() {
            std::process::exit(1);
        }
    });
    let selected = Rc::new(RefCell::new(Vec::new()));
    let log = selected.clone();
    let tray_menu: Rc<RefCell<Option<Menu>>> = Rc::default();
    let show = tray_menu.clone();
    let popover_parts = Rc::new(RefCell::new(
        None::<(Retained<NSStatusBarButton>, Retained<NSPopover>)>,
    ));
    let inspect = popover_parts.clone();
    let step = Cell::new(0);
    let anchor = OnceCell::<Retained<NSButton>>::new();
    let pointer_driver = std::env::var_os("SERGE_TEST_STATUSBAR_CLICK");
    let pointer_process = RefCell::new(None::<std::process::Child>);
    let spec = WindowSpec::new(
        WindowOptions { title: "Serge menu acceptance".into(), ..Default::default() },
        Scope::root(),
        || ui! { Text{"Project"} },
    )
    .on_event(move |event| {
        if !matches!(event, NativeEvent::Window(WindowEvent::RedrawRequested)) || step.get() == 3 {
            return;
        }
        WindowHandle::with_native(|window| {
            let mut running = pointer_process.borrow_mut();
            if let Some(child) = running.as_mut() {
                match child.try_wait().unwrap() {
                    Some(status) => {
                        assert!(status.success(), "status-bar pointer driver exited {status}");
                        running.take();
                    }
                    None => {
                        window.request_redraw();
                        return;
                    }
                }
            }
            let parts = inspect.borrow();
            let (button, popover) = parts.as_ref().unwrap();
            let anchor = anchor.get_or_init(|| {
                if pointer_driver.is_some() || button.window().is_some_and(|w| w.frame().size.height > 0.) {
                    return Retained::from(&**button);
                }
                // A locked desktop reports a zero-height status window. Mount a real AppKit
                // button in our window and exercise the target installed on the status item.
                let mtm = MainThreadMarker::new().unwrap();
                let app = NSApplication::sharedApplication(mtm);
                let root = app.windows().iter().find(|w| w.title().to_string() == "Serge menu acceptance")
                    .and_then(|w| w.contentView()).unwrap();
                let proxy = NSButton::new(mtm);
                proxy.setFrame(button.bounds());
                root.addSubview(&proxy);
                // SAFETY: these are the installed button's target and selector. The owning
                // MenuBarPopover outlives both button clicks on this same thread.
                #[allow(unsafe_code)]
                unsafe {
                    proxy.setTarget(button.target().as_deref());
                    proxy.setAction(button.action());
                }
                println!("AppKit reported a zero-height status bar; exercised the installed action from the application window");
                proxy
            });
            let mut click = || {
                if let Some(driver) = &pointer_driver {
                    let status_window = button.window().expect("status button window");
                    let rect = status_window.convertRectToScreen(button.convertRect_toView(button.bounds(), None));
                    let x = rect.origin.x + rect.size.width / 2.;
                    let y = rect.origin.y + rect.size.height / 2.;
                    eprintln!("Clicking the status button at ({x}, {y}), frame {rect:?}");
                    // Let AppKit service accessibility queries and deliver the pointer event.
                    *running = Some(std::process::Command::new(driver)
                        .args([x.to_string(), y.to_string(), std::process::id().to_string()])
                        .spawn().unwrap());
                    return;
                }
                // SAFETY: this installed button invokes its own main-thread target and selector.
                #[allow(unsafe_code)]
                unsafe {
                    anchor.performClick(None)
                };
            };
            match step.get() {
                0 => {
                    assert_eq!(button.title().to_string(), "Projects");
                    assert!(!popover.isShown());
                    click();
                    step.set(1);
                }
                1 if popover.isShown() => {
                    eprintln!("AppKit displayed the status popover");
                    let content = popover.contentViewController().unwrap();
                    assert!(content.view().window().is_some());
                    assert_eq!(popover.contentSize().width, 240.);
                    click();
                    step.set(2);
                }
                2 if !popover.isShown() => {
                    step.set(3);
                    let app = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
                    for percent in [Some(0), Some(42), Some(100), None] {
                        system::set_task_progress(
                            window,
                            "org.hyperish.Playground.desktop",
                            percent,
                        )
                        .unwrap();
                        assert_eq!(
                            app.dockTile().badgeLabel().map(|s| s.to_string()),
                            percent.map(|n| format!("{n}%"))
                        );
                    }
                    assert!(system::set_task_progress(window, "", Some(101)).is_err());
                    let menu = app.mainMenu().expect("native main menu installed");
                    let project = (0..menu.numberOfItems())
                        .filter_map(|n| menu.itemAtIndex(n)?.submenu())
                        .find(|m| m.title().to_string() == "Project")
                        .expect("Project menu");
                    project.performActionForItemAtIndex(0);
                    let saved = show.borrow();
                    let native = saved.as_ref().unwrap().ns_menu().cast::<NSMenu>();
                    // SAFETY: this retained Menu owns its NSMenu during the synchronous UI action.
                    #[allow(unsafe_code)]
                    unsafe { native.as_ref().unwrap() }.performActionForItemAtIndex(0);
                    return;
                }
                _ => {}
            }
            window.request_redraw();
        })
        .unwrap();
    });
    system::run(vec![spec], move || {
        let menu = Menu::new();
        let app = Submenu::new("Serge", true);
        menu.append(&app).unwrap();
        let project = Submenu::new("Project", true);
        project
            .append(&MenuItem::with_id(
                "save",
                "Save",
                true,
                Some("CmdOrCtrl+S".parse().unwrap()),
            ))
            .unwrap();
        menu.append(&project).unwrap();
        let tray = Menu::new();
        tray.append(&MenuItem::with_id("tray-open", "Open", true, None))
            .unwrap();
        *tray_menu.borrow_mut() = Some(tray.clone());
        let mut system = DesktopSystem::new(move |id| {
            log.borrow_mut().push(id.to_owned());
            if id == "tray-open" {
                WindowHandle::current().unwrap().close().unwrap();
            }
        });
        system.menu_bar = Some(menu);
        let mtm = MainThreadMarker::new().unwrap();
        let content = NSViewController::new(mtm);
        content.setView(&NSView::new(mtm));
        let popover = MenuBarPopover::new("Projects", &content, 240., 120.)?;
        popover.popover().setAnimates(false);
        *popover_parts.borrow_mut() = Some((popover.button().into(), popover.popover().into()));
        system.menu_bar_popover = Some(popover);
        system.tray = Some(Tray {
            icon: Icon::from_rgba(vec![180; 16 * 16 * 4], 16, 16).unwrap(),
            tooltip: "Serge".into(),
            menu: tray,
        });
        Ok(system)
    })
    .unwrap();
    finished.send(()).unwrap();
    watchdog.join().unwrap();
    assert_eq!(*selected.borrow(), ["save", "tray-open"]);
    println!(
        "AppKit opened and dismissed the menu-bar popover, updated and cleared the Dock percentage, and delivered menu and status-item commands"
    );
}
#[cfg(target_os = "windows")]
fn main() {
    if std::env::var_os("SERGE_TEST_WINDOWS").is_none() {
        return;
    }
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use serge::{
        desktop::{
            NativeEvent, WindowEvent, WindowHandle, WindowOptions, WindowSpec,
            system::{
                self, DesktopSystem,
                menu::{Menu, MenuItem, Submenu},
            },
        },
        prelude::*,
    };
    use std::{cell::Cell, rc::Rc, time::Duration};
    use windows::Win32::{
        Foundation::{HWND, WPARAM},
        UI::WindowsAndMessaging::{GetMenu, GetSubMenu, PostMessageW, WM_KEYDOWN},
    };

    let (stop, wait) = std::sync::mpsc::channel();
    let watchdog = std::thread::spawn(move || {
        if wait.recv_timeout(Duration::from_secs(20)).is_err() {
            std::process::exit(1);
        }
    });
    let posted = Cell::new(false);
    let seen = Rc::new(Cell::new(false));
    let received = seen.clone();
    let spec = WindowSpec::new(
        WindowOptions::default(),
        hyve::Scope::root(),
        || ui! {Text{"Project"}},
    )
    .on_event(move |event| {
        if !matches!(event, NativeEvent::Window(WindowEvent::RedrawRequested))
            || posted.replace(true)
        {
            return;
        }
        WindowHandle::with_native(|w| {
            let RawWindowHandle::Win32(raw) = w.window_handle().unwrap().as_raw() else {
                panic!("expected HWND")
            };
            let hwnd = HWND(raw.hwnd.get() as _);
            for value in [Some(0), Some(42), Some(100), None] {
                system::set_task_progress(w, "", value).unwrap();
            }
            // SAFETY: this live HWND belongs to the current test window. F8 is queued only
            // to its message loop, so the installed native menu handles the shortcut.
            #[allow(unsafe_code)]
            unsafe {
                assert!(!GetSubMenu(GetMenu(hwnd), 0).is_invalid());
                PostMessageW(Some(hwnd), WM_KEYDOWN, WPARAM(0x77), Default::default()).unwrap();
            }
        })
        .unwrap();
    });
    system::run(vec![spec], move || {
        let menu = Menu::new();
        let project = Submenu::new("Project", true);
        project
            .append(&MenuItem::with_id(
                "save",
                "Save",
                true,
                Some("F8".parse().unwrap()),
            ))
            .unwrap();
        menu.append(&project).unwrap();
        let mut system = DesktopSystem::new(move |command| {
            assert_eq!(command, "save");
            assert!(!received.replace(true));
            WindowHandle::current().unwrap().close().unwrap();
        });
        system.menu_bar = Some(menu);
        Ok(system)
    })
    .unwrap();
    assert!(seen.get());
    stop.send(()).unwrap();
    watchdog.join().unwrap();
    println!("Windows updated the taskbar and dispatched F8 from the native menu");
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {}
