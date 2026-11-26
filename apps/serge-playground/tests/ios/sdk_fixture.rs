//! Installed as the main binary of a freshly generated iOS application by sdk.py.
mod platform_widgets;
use hyve::prelude::signal;
use platform_widgets::{ExportHome, ExportIsland, ExportStatus};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serge::{
    NativeEvent, WindowEvent,
    ios::{self, WindowHandle, WindowOptions, WindowSpec},
    prelude::*,
    sys::{
        AppleNotifications, IosDocumentResult, IosSystem, NotificationAction, NotificationCategory,
    },
};
use std::{fs, io, time::Duration};

fn record(name: &str, value: &str) {
    let file = std::env::temp_dir().join(format!("serge-{name}.txt"));
    let tmp = file.with_extension("pending");
    fs::write(&tmp, value).unwrap();
    fs::rename(tmp, file).unwrap();
}

fn system() -> IosSystem {
    WindowHandle::with_native(|window| {
        let RawWindowHandle::UiKit(raw) = window.window_handle().unwrap().as_raw() else {
            panic!("expected the live UIKit view")
        };
        // SAFETY: with_native lends the retained window's view on its UI thread.
        let view = unsafe { &*raw.ui_view.as_ptr().cast::<objc2_ui_kit::UIView>() };
        IosSystem::for_window(&view.window().unwrap())
    })
    .unwrap()
}

type Tick = Box<dyn FnMut()>;

fn result(name: String, mut read: impl FnMut() -> Option<String> + 'static) -> Tick {
    let mut recorded = false;
    Box::new(move || {
        if !recorded && let Some(value) = read() {
            record(&name, &value);
            recorded = true;
        }
    })
}

async fn widgets(live: bool) -> io::Result<String> {
    let state = |project: &str, progress| ExportStatus {
        project_name: project.into(),
        progress,
    };
    if live {
        let initial = ExportIsland(state("Started", 0.1));
        let id = ios::start_activity(&initial).await?;
        ios::update_activity(&ExportIsland(state("Rendering", 0.5)), &id).await?;
        ios::end_activity(&ExportIsland(state("Done", 1.)), &id).await?;
        return Ok(format!("ended {id}"));
    }
    ios::publish_widget(&ExportHome(state("First project", 0.2))).await?;
    ios::publish_widget(&ExportHome(state("Saved project", 0.75))).await?;
    Ok("published".into())
}

fn probe(mode: String, host: &IosSystem) -> io::Result<Tick> {
    match mode.as_str() {
        "intent" | "widget-families" => {
            unsafe extern "C" {
                fn serge_test_read_export_intent();
                fn serge_test_widget_families();
            }
            // SAFETY: these symbols are provided by the accompanying Swift test fixture.
            unsafe {
                if mode == "intent" {
                    serge_test_read_export_intent();
                } else {
                    serge_test_widget_families();
                }
            }
        }
        "biometric-none" => record(
            &mode,
            if serge::hw::Biometrics::available() {
                "enrolled"
            } else {
                "unavailable"
            },
        ),
        "biometric" => {
            let request = serge::hw::Biometrics::evaluate("Unlock this test project")?;
            record("biometric-ready", "evaluating");
            return Ok(result(mode, move || {
                request.try_result().map(|reply| match reply {
                    Ok(()) => "matched".into(),
                    Err(e) => format!("error: {e}"),
                })
            }));
        }
        "clipboard" => {
            host.set_clipboard_text("Serge system clipboard");
            assert_eq!(
                host.clipboard_text().as_deref(),
                Some("Serge system clipboard")
            );
            host.impact(0.5)?;
            record(&mode, "clipboard and haptic request");
        }
        "read-shared" => record(&mode, &host.clipboard_text().unwrap_or_default()),
        "widgets" | "activity" => {
            spawn(async move {
                match widgets(mode == "activity").await {
                    Ok(value) => record(&mode, &value),
                    Err(e) => record(&mode, &format!("error: {e}")),
                }
            });
        }
        "action" => {
            let choice = host.choose("Project actions", &["Archive project"], "Cancel")?;
            return Ok(result(mode, move || {
                choice.try_result().map(|v| format!("{v:?}"))
            }));
        }
        "date" | "time" => {
            let request = if mode == "date" {
                host.pick_date()?
            } else {
                host.pick_time()?
            };
            return Ok(result(mode, move || {
                request.try_result().map(|v| format!("{v:?}"))
            }));
        }
        "share" => {
            host.share_text("Serge shared project")?;
            record(&mode, "presented");
        }
        "import" | "import-cancel" | "export" => {
            let importing = mode == "import";
            let request = if mode != "export" {
                host.pick_document("public.plain-text")?
            } else {
                let path = std::env::temp_dir().join("serge-export.txt");
                fs::write(&path, "Serge document bytes")?;
                host.export_document(&path)?
            };
            return Ok(result(mode, move || {
                request
                    .try_result()
                    .unwrap()
                    .map(|selection| match selection {
                        IosDocumentResult::Cancelled => "cancelled".into(),
                        IosDocumentResult::Selected(path) => {
                            if importing {
                                fs::read_to_string(path).unwrap()
                            } else {
                                path.file_name().unwrap().to_string_lossy().into_owned()
                            }
                        }
                    })
            }));
        }
        "push" => {
            let action = NotificationAction {
                id: "archive",
                title: "Archive project",
            };
            let category = NotificationCategory {
                id: "project",
                actions: &[action],
            };
            let notifications = AppleNotifications::install(&[category], |response| {
                record("push-action", &response.action);
            })?;
            let permission = notifications.request_permission();
            return Ok(Box::new(move || {
                let _keep_delegate = &notifications;
                if let Ok(granted) = permission.try_recv() {
                    record(
                        "push",
                        if granted.unwrap() {
                            "allowed"
                        } else {
                            "denied"
                        },
                    );
                }
            }));
        }
        _ => return Err(io::Error::other("unknown probe")),
    }
    Ok(Box::new(|| {}))
}

fn main() {
    let mode = std::env::var("SERGE_IOS_PROBE").unwrap_or_else(|_| "clipboard".into());
    let mut started = false;
    let mut background = None;
    let visible_mode = mode.clone();
    let spec = WindowSpec::new(WindowOptions::default(), hyve::Scope::root(), move || {
        let status = signal("Project tools".to_owned());
        spawn(async move {
            let path = std::env::temp_dir().join(format!("serge-{visible_mode}.txt"));
            while !path.is_file() {
                sleep(Duration::from_millis(60)).await;
            }
            status.set(format!("Completed {visible_mode}"));
        });
        ui! { Text { "{status()}" } }
    })
    .on_event(move |event| match event {
        NativeEvent::Window(WindowEvent::RedrawRequested) if !started => {
            started = true;
            let host = system();
            if mode == "background" {
                background = Some(host.background_task("Finish project write").unwrap());
                record("background-ready", "active");
                return;
            }
            let name = mode.clone();
            spawn(async move {
                // UIKit completes its launch transition before presenting native sheets.
                sleep(Duration::from_millis(400)).await;
                match probe(name.clone(), &host) {
                    Ok(mut step) => loop {
                        step();
                        sleep(Duration::from_millis(50)).await;
                    },
                    Err(error) => record(&name, &format!("error: {error}")),
                }
            });
        }
        NativeEvent::Window(WindowEvent::Occluded(true))
            if background.as_ref().is_some_and(|task| !task.expired()) =>
        {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(400));
                record("background", "saved after entering background");
            });
        }
        _ => {}
    });
    ios::run_windows(vec![spec]).unwrap();
}
