//! Native dialogs, clipboard and a system shortcut on a dedicated X display.
#[cfg(target_os = "linux")]
fn main() {
    if std::env::var_os("SERGE_TEST_X11").is_none()
        && std::env::var_os("SERGE_TEST_WAYLAND_MENU").is_none()
    {
        return;
    }
    use hyve::Scope;
    use serge::{
        desktop::{
            WindowHandle, WindowOptions, WindowSpec,
            system::{
                self, DesktopSystem,
                menu::{Menu, MenuItem, Submenu},
            },
        },
        prelude::*,
        sys::{Clipboard, FileDialog, MessageDialog, MessageDialogResult},
    };
    use std::{
        cell::Cell,
        process::Command,
        rc::Rc,
        thread,
        time::{Duration, Instant},
    };
    if std::env::var_os("SERGE_TEST_WAYLAND_MENU").is_some() {
        let spec = WindowSpec::new(
            WindowOptions::default(),
            Scope::root(),
            || ui! {Text{"Menus"}},
        );
        let result = system::run(vec![spec], || {
            let mut system = DesktopSystem::new(|_| {});
            system.menu_bar = Some(Menu::new());
            Ok(system)
        });
        let error = result.expect_err("a native Wayland parent cannot embed the GTK/X11 menu");
        assert!(
            error
                .to_string()
                .contains("Linux window menus require an X11 parent"),
            "{error}"
        );
        println!("The pure Wayland menu request returned the documented X11-parent error");
        return;
    }

    fn xdo(args: &[&str]) {
        assert!(
            Command::new("xdotool")
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    fn visible(title: &str) -> Option<String> {
        let result = Command::new("xdotool")
            .args(["search", "--onlyvisible", "--name", title])
            .output()
            .unwrap();
        std::str::from_utf8(&result.stdout)
            .unwrap()
            .lines()
            .next()
            .map(str::to_owned)
    }
    fn window(title: &str) -> String {
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(id) = visible(title) {
                return id;
            }
            assert!(Instant::now() < end, "did not see {title}");
            thread::sleep(Duration::from_millis(60));
        }
    }

    // Inside Weston, send physical input to the compositor's seat. XWayland's
    // XTEST pointer cannot move the compositor pointer to an arbitrary surface.
    struct MenuInput {
        display: std::ffi::OsString,
        window: String,
        origin: (i32, i32),
    }
    impl MenuInput {
        fn new() -> Self {
            let id = window("^Serge X11 menus$");
            if let Some(display) = std::env::var_os("SERGE_TEST_WESTON_DISPLAY") {
                let info = Command::new("xwininfo")
                    .args(["-id", &id])
                    .env("LC_ALL", "C")
                    .output()
                    .unwrap();
                assert!(info.status.success());
                let info = String::from_utf8(info.stdout).unwrap();
                let coordinate = |field| -> i32 {
                    info.lines()
                        .find_map(|line| line.trim().strip_prefix(field))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap()
                };
                Self {
                    display,
                    window: std::env::var("SERGE_TEST_WESTON_WINDOW").unwrap(),
                    origin: (
                        coordinate("Absolute upper-left X:"),
                        coordinate("Absolute upper-left Y:"),
                    ),
                }
            } else {
                Self {
                    display: std::env::var_os("DISPLAY").unwrap(),
                    window: id,
                    origin: (0, 0),
                }
            }
        }
        fn send(&self, args: &[&str]) {
            assert!(
                Command::new("xdotool")
                    .env("DISPLAY", &self.display)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fn click(&self, x: i32, y: i32, button: &str) {
            self.send(&[
                "mousemove",
                "--window",
                &self.window,
                &(self.origin.0 + x).to_string(),
                &(self.origin.1 + y).to_string(),
                "click",
                button,
            ]);
        }
    }

    let (finish, watchdog) = std::sync::mpsc::channel();
    thread::spawn(move || {
        if watchdog.recv_timeout(Duration::from_secs(50)).is_err() {
            std::process::exit(1);
        }
    });
    fn check_dialogs_and_clipboard() {
        if std::env::var_os("SERGE_TEST_SECRET_SERVICE").is_some() {
            let name = format!("serge-test-{}", hyve::Ulid::new());
            let entry = serge::sys::Credential::new(&name, "fixture").unwrap();
            entry.set_secret(b"initial value").unwrap();
            entry.set_secret(b"updated value").unwrap();
            let restored = serge::sys::Credential::new(&name, "fixture").unwrap();
            assert_eq!(restored.get_secret().unwrap(), b"updated value");
            restored.delete_credential().unwrap();
            assert!(matches!(
                entry.get_secret(),
                Err(serge::sys::CredentialError::NoEntry)
            ));
            println!("Secret Service encrypted, replaced and removed the credential.");
        }
        let mut owner = Clipboard::new().unwrap();
        owner.set_text("Serge system clipboard").unwrap();
        let external = Command::new("xclip")
            .args(["-o", "-selection", "clipboard"])
            .output()
            .unwrap();
        assert!(external.status.success());
        assert_eq!(external.stdout, b"Serge system clipboard");
        assert_eq!(owner.get_text().unwrap(), "Serge system clipboard");
        assert!(serge::sys::data_directory("org", "../bad", "test").is_err());
        let data = serge::sys::data_directory("org", "Hyperish", "Picker").unwrap();
        assert!(data.starts_with(std::env::var_os("XDG_DATA_HOME").unwrap()));
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("input.txt");
        std::fs::write(&file, "example").unwrap();
        let selected = file.clone();
        let automate = thread::spawn(move || {
            // The native portal uses the supplied title. The Zenity file chooser uses its application name.
            let file_chooser = "^(Serge SDK picker|zenity)$";
            let picker = window(file_chooser);
            xdo(&["windowfocus", "--sync", &picker]);
            if visible("^zenity$").as_ref() == Some(&picker) {
                // Zenity selects the supplied file after realizing its directory model.
                thread::sleep(Duration::from_millis(400));
                xdo(&["key", "Return"]);
            } else {
                let deadline = Instant::now() + Duration::from_secs(12);
                loop {
                    assert!(
                        Instant::now() < deadline,
                        "the file chooser did not focus its location entry"
                    );
                    xdo(&["key", "--delay", "100", "ctrl+l", "ctrl+a"]);
                    xdo(&["type", "--clearmodifiers", selected.to_str().unwrap()]);
                    xdo(&["key", "--delay", "60", "ctrl+a", "ctrl+c"]);
                    let copied = Command::new("xclip")
                        .args(["-o", "-selection", "clipboard"])
                        .output()
                        .unwrap();
                    if copied.stdout == selected.as_os_str().as_encoded_bytes() {
                        break;
                    }
                }
                xdo(&["key", "Return"]);
            }
            let alert = window("^Serge SDK alert$");
            xdo(&["windowfocus", "--sync", &alert]);
            xdo(&["key", "Return"]);
        });
        let picked = FileDialog::new()
            .set_title("Serge SDK picker")
            .set_directory(dir.path())
            .set_file_name("input.txt")
            .pick_file()
            .unwrap();
        assert_eq!(picked, file);
        assert_eq!(
            MessageDialog::new()
                .set_title("Serge SDK alert")
                .set_description("Document selected")
                .show(),
            MessageDialogResult::Ok
        );
        automate.join().unwrap();
        println!("Native file and alert panels and external clipboard passed.");
    }
    if !std::env::args_os().any(|arg| arg == "--menus-only") {
        check_dialogs_and_clipboard();
    }

    let subscriber = zbus::blocking::Connection::session().unwrap();
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("com.canonical.Unity.LauncherEntry")
        .unwrap()
        .member("Update")
        .unwrap()
        .build();
    let mut updates =
        zbus::blocking::MessageIterator::for_match_rule(rule, &subscriber, Some(6)).unwrap();
    let (ready, frame) = std::sync::mpsc::channel::<WindowHandle>();
    let (commands, actions) = std::sync::mpsc::channel::<String>();
    let drive = thread::spawn(move || {
        let exercise = || {
            let handle = frame.recv_timeout(Duration::from_secs(15)).unwrap();
            let input = MenuInput::new();
            let expect = |name: &str| {
                let command = actions.recv_timeout(Duration::from_secs(8)).expect(name);
                assert_eq!(command, name);
                println!("Linux menu dispatched {command}");
            };
            input.send(&["windowfocus", "--sync", &input.window]);
            input.click(80, 80, "1");
            input.send(&["key", "--clearmodifiers", "ctrl+shift+F9"]);
            expect("show-panel");
            input.send(&["key", "F8"]);
            expect("menu-save");
            for (x, y, button, command) in
                [(30, 10, "1", "menu-save"), (80, 100, "3", "context-open")]
            {
                input.click(x, y, button);
                input.send(&["key", "--delay", "100", "Down", "Return"]);
                expect(command);
            }
            handle.close().unwrap();
        };
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(exercise)).is_err() {
            std::process::exit(1);
        }
    });
    let spec = WindowSpec::new(
        WindowOptions {
            title: "Serge X11 menus".into(),
            width: 480.,
            height: 320.,
        },
        Scope::root(),
        move || {
            let sent = Rc::new(Cell::new(false));
            ui! {Canvas {height:32px, paint(bounds,_ctx) => {
                if bounds.width > 0. && !sent.replace(true) {
                    WindowHandle::with_native(|window| {
                        for value in [Some(0), Some(64), None] {
                            system::set_task_progress(window, "org.hyperish.Playground.desktop", value).unwrap();
                        }
                    }).unwrap();
                    ready.send(WindowHandle::current().unwrap()).unwrap();
                }
            }}}
        },
    );
    system::run(vec![spec], move || {
        let mut sys = DesktopSystem::new(move |command| commands.send(command.to_owned()).unwrap());
        let bar = Menu::new();
        let project = Submenu::new("Project", true);
        project
            .append(&MenuItem::with_id(
                "menu-save",
                "Save",
                true,
                Some("F8".parse().unwrap()),
            ))
            .unwrap();
        bar.append(&project).unwrap();
        sys.menu_bar = Some(bar);
        let context = Menu::new();
        context
            .append(&MenuItem::with_id("context-open", "Open", true, None))
            .unwrap();
        sys.context_menu = Some(context);
        sys.shortcuts
            .push(("Ctrl+Shift+F9".parse().unwrap(), "show-panel".into()));
        Ok(sys)
    })
    .unwrap();
    drive.join().unwrap();
    for expected in [Some(0u8), Some(64u8), None] {
        let message = updates.next().unwrap().unwrap();
        let (uri, props): (
            String,
            std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        ) = message.body().deserialize().unwrap();
        assert_eq!(uri, "application://org.hyperish.Playground.desktop");
        assert_eq!(
            bool::try_from(&props["progress-visible"]).unwrap(),
            expected.is_some()
        );
        assert_eq!(
            f64::try_from(&props["progress"]).unwrap(),
            f64::from(expected.unwrap_or(0)) / 100.
        );
    }
    finish.send(()).unwrap();
    println!("Launcher progress, shortcut, F8, menubar and right-click menu passed.");
}
#[cfg(not(target_os = "linux"))]
fn main() {}
