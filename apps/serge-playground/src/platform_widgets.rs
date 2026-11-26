//! The same export status projected into an iOS Live Activity and an Android widget.
use serge::prelude::*;

/// Fields published by the exporting application.
pub struct ExportStatus {
    /// User-visible project title.
    pub project_name: String,
    /// Completion fraction.
    pub progress: f32,
}

#[allow(non_snake_case)]
#[ios::live_activity]
/// Compact Dynamic Island regions and an expanded export view.
pub fn ExportIsland(state: ExportStatus) -> LiveActivity {
    live_activity! {
        compact_leading: ui! { Icon { "cube" } },
        compact_trailing: ui! { Text { "{(state.progress * 100.0) as u32}%" } },
        expanded: ui! {
            Stack { gap: 8px,
                Text { "{state.project_name} 내보내는 중..." }
                ProgressBar { value: state.progress }
            }
        }
    }
}

#[allow(non_snake_case)]
#[android::widget]
/// Export status on the Android home screen.
pub fn ExportWidget(state: ExportStatus) -> PlatformWidget {
    ui! {
        Stack { gap: 8px,
            Text { "{state.project_name} 내보내는 중..." }
            ProgressBar { value: state.progress }
        }
    }
}

#[allow(non_snake_case)]
#[ios::widget]
/// Export progress on the home or lock screen and in Shortcuts.
pub fn ExportHome(state: ExportStatus) -> PlatformWidget {
    ui! { Column { Text { "{state.project_name}" } ProgressBar { value: state.progress } } }
}

#[allow(non_snake_case)]
#[android::ongoing_notification]
/// A system notification the app updates or cancels when an export completes.
pub fn ExportProgress(state: ExportStatus) -> PlatformWidget {
    ongoing_notification! {
        title: ui! { Text { "{state.project_name}" } },
        body: ui! { Text { "{(state.progress * 100.0) as u32}%" } },
        progress: ui! { ProgressBar { value: state.progress } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_hosts_receive_the_projected_widget_state() {
        let state = || ExportStatus {
            project_name: "메모".into(),
            progress: 0.75,
        };
        let ios = ExportIsland(state());
        let android = ExportWidget(state());
        assert_eq!(ios.kind, "ExportIsland");
        assert_eq!(android.kind, "ExportWidget");
        assert_eq!(ios.state_json, android.state_json);
        assert_eq!(ExportHome(state()).state_json, ios.state_json);
        assert_eq!(ExportProgress(state()).state_json, ios.state_json);
        let value: hyve::Value = hyve::serde::from_json(&ios.state_json).unwrap();
        assert_eq!(value.get("project_name"), Some(&hyve::Value::from("메모")));
        assert_eq!(value.get("progress"), Some(&hyve::Value::from(0.75)));
    }
}
