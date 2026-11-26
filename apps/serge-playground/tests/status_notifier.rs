//! A desktop shell discovers the application's status item and activates its menu over D-Bus.
#[cfg(target_os = "linux")]
fn main() {
    if std::env::var_os("SERGE_TEST_STATUS_NOTIFIER").is_none() {
        return;
    }
    use hyve::Scope;
    use serge::{
        desktop::{
            WindowHandle, WindowOptions, WindowSpec,
            system::{
                self, DesktopSystem, Icon, Tray,
                menu::{Menu, MenuItem},
            },
        },
        prelude::*,
    };
    use std::{cell::Cell, collections::HashMap, rc::Rc, sync::mpsc, time::Duration};
    use zbus::{
        blocking::{Connection, Proxy, fdo::DBusProxy},
        zvariant::{OwnedObjectPath, OwnedValue, Value},
    };

    let (first_frame, window) = mpsc::channel::<WindowHandle>();
    let (activated, action) = mpsc::channel();
    let shell = std::thread::spawn(move || {
        let exercise = || {
            let handle = window.recv_timeout(Duration::from_secs(15)).unwrap();
            let bus = Connection::session().unwrap();
            let prefix = format!("org.kde.StatusNotifierItem-{}-", std::process::id());
            let service = DBusProxy::new(&bus)
                .unwrap()
                .list_names()
                .unwrap()
                .into_iter()
                .find(|name| name.as_str().starts_with(&prefix))
                .expect("registered status item");
            let item = Proxy::new(
                &bus,
                service.as_str(),
                "/StatusNotifierItem",
                "org.kde.StatusNotifierItem",
            )
            .unwrap();
            assert_eq!(item.get_property::<String>("Status").unwrap(), "Active");
            let path: OwnedObjectPath = item.get_property("Menu").unwrap();
            let menu = Proxy::new(
                &bus,
                service.as_str(),
                path.as_str(),
                "com.canonical.dbusmenu",
            )
            .unwrap();
            let items: Vec<(i32, HashMap<String, OwnedValue>)> = menu
                .call(
                    "GetGroupProperties",
                    &(Vec::<i32>::new(), ["label"].as_slice()),
                )
                .unwrap();
            let (id, _) = items
                .iter()
                .find(|(_, props)| {
                    props.get("label").and_then(|v| <&str>::try_from(v).ok())
                        == Some("Open project")
                })
                .expect("exported command");
            let _: () = menu
                .call("Event", &(*id, "clicked", Value::from(0i32), 0u32))
                .unwrap();
            assert_eq!(
                action.recv_timeout(Duration::from_secs(5)).unwrap(),
                "project-open"
            );
            handle.close().unwrap();
        };
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(exercise)).is_err() {
            std::process::exit(1);
        }
    });
    let spec = WindowSpec::new(WindowOptions::default(), Scope::root(), move || {
        let sent = Rc::new(Cell::new(false));
        ui! { Canvas { height: 32px, paint(bounds, _ctx) => {
            if bounds.width > 0. && !sent.replace(true) {
                first_frame.send(WindowHandle::current().unwrap()).unwrap();
            }
        } } }
    });
    system::run(vec![spec], move || {
        let mut system = DesktopSystem::new(move |id| activated.send(id.to_owned()).unwrap());
        let menu = Menu::new();
        menu.append(&MenuItem::with_id(
            "project-open",
            "Open project",
            true,
            None,
        ))
        .unwrap();
        system.tray = Some(Tray {
            icon: Icon::from_rgba(vec![255; 4 * 4 * 4], 4, 4).unwrap(),
            tooltip: "Project actions".into(),
            menu,
        });
        Ok(system)
    })
    .unwrap();
    shell.join().unwrap();
    println!("The D-Bus status item exported its menu and delivered the shell's command.");
}
#[cfg(not(target_os = "linux"))]
fn main() {}
