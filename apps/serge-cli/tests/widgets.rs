//! Build a widget-only package through the same build stage invoked by hyve.
use std::{fs, process::Command};
#[test]
fn build_emits_platform_sources_and_keeps_a_valid_generation_on_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let src = root.join("src");
    fs::create_dir(&src).unwrap();
    fs::write(root.join("Cargo.toml"),"[package]\nname='widget-fixture'\nversion='0.1.0'\nedition='2024'\n[package.metadata.serge]\nwidgets=['src/lib.rs']\nandroid_package='org.example.status'\n[workspace]\n").unwrap();
    let file = src.join("lib.rs");
    let source = include_str!("../../serge-playground/src/platform_widgets.rs");
    fs::write(&file, source).unwrap();
    let build = || {
        Command::new(env!("CARGO_BIN_EXE_hyve-serge"))
            .args(["build", "--debug", "-p", "widget-fixture"])
            .current_dir(root)
            .env_remove("CARGO_TARGET_DIR")
            .output()
            .unwrap()
    };
    let done = build();
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let output = root.join("target/serge-widgets/widget-fixture");
    let pointer = output.join("current");
    let first = fs::read_to_string(&pointer).unwrap();
    let generation = output.join(first.trim());
    let swift_path = generation.join("ios/ExportIsland.swift");
    let swift = fs::read_to_string(&swift_path).unwrap();
    for declaration in [
        "ActivityConfiguration",
        "DynamicIslandExpandedRegion(.center)",
        "ProgressView",
        "case v1 = \"project_name\"",
    ] {
        assert!(swift.contains(declaration), "{swift}");
    }
    let home_path = generation.join("ios/ExportHome.swift");
    let home = fs::read_to_string(&home_path).unwrap();
    for expected in [
        "StaticConfiguration",
        ".accessoryRectangular",
        "struct ReadExportHomeIntent: AppIntent",
        "entry.state.",
        "reloadTimelines(ofKind:",
    ] {
        assert!(home.contains(expected), "{home}");
    }
    #[cfg(target_os = "macos")]
    assert!(
        Command::new("xcrun")
            .args(["swiftc", "-frontend", "-parse"])
            .arg(&swift_path)
            .arg(&home_path)
            .arg(generation.join("ios/host/Bridge.swift"))
            .arg(generation.join("ios/SergeWidgetBundle.swift"))
            .status()
            .unwrap()
            .success()
    );
    #[cfg(target_os = "macos")]
    if std::env::var_os("SERGE_REQUIRE_IOS_WIDGET_SDK").is_some() {
        let sdk = Command::new("xcrun")
            .args(["--sdk", "iphonesimulator", "--show-sdk-path"])
            .output()
            .unwrap();
        assert!(sdk.status.success(), "iOS simulator SDK is required");
        let sdk = String::from_utf8(sdk.stdout).unwrap();
        assert!(
            Command::new("xcrun")
                .args([
                    "swiftc",
                    "-sdk",
                    sdk.trim(),
                    "-target",
                    "arm64-apple-ios16.2-simulator",
                    "-parse-as-library",
                    "-typecheck"
                ])
                .arg(&swift_path)
                .arg(&home_path)
                .arg(generation.join("ios/host/Bridge.swift"))
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("xcrun")
                .args([
                    "swiftc",
                    "-sdk",
                    sdk.trim(),
                    "-target",
                    "arm64-apple-ios16.2-simulator",
                    "-parse-as-library",
                    "-application-extension",
                    "-typecheck"
                ])
                .arg(&swift_path)
                .arg(&home_path)
                .arg(generation.join("ios/SergeWidgetBundle.swift"))
                .status()
                .unwrap()
                .success()
        );
    }
    let kotlin = fs::read_to_string(
        generation.join("android/src/main/java/org/example/status/ExportWidget.kt"),
    )
    .unwrap();
    for declaration in [
        "GlanceAppWidgetReceiver",
        "updateAppWidgetState",
        "LinearProgressIndicator",
        "optString(\"project_name\"",
    ] {
        assert!(kotlin.contains(declaration), "{kotlin}");
    }
    let manifest =
        fs::read_to_string(generation.join("android/src/main/AndroidManifest.xml")).unwrap();
    assert!(manifest.contains("org.example.status.ExportWidgetReceiver"));
    assert!(manifest.contains("android.permission.POST_NOTIFICATIONS"));
    let notification = fs::read_to_string(
        generation.join("android/src/main/java/org/example/status/ExportProgress.kt"),
    )
    .unwrap();
    for expected in [
        "setOngoing(true)",
        "setContentTitle(state.optString",
        "setProgress(100",
        "manager.notify",
        ".cancel(",
        "checkSelfPermission",
    ] {
        assert!(notification.contains(expected), "{notification}");
    }
    assert!(
        generation
            .join("android/src/main/res/xml/serge_exportwidget.xml")
            .is_file()
    );
    if let Some(gradle) = std::env::var_os("SERGE_ANDROID_GRADLE") {
        let android = generation.join("android");
        assert!(
            Command::new(gradle)
                .args(["--no-daemon", "assembleDebug"])
                .current_dir(&android)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            android
                .join("build/outputs/aar/serge-widgets-debug.aar")
                .is_file()
        );
    }
    for invalid in ["Canvas { }", "Text { \"{state.project_name()}\" }"] {
        let edited = source.replace("Icon { \"cube\" }", invalid);
        fs::write(&file, edited).unwrap();
        assert!(!build().status.success());
        assert_eq!(fs::read_to_string(&pointer).unwrap(), first);
    }
}
