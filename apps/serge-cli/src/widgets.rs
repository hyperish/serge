//! Publish a complete set of platform sources for each opted-in app.
use serge_web_dev::Package;
use serge_widget_codegen::{Platform, discover};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn insert(
    files: &mut BTreeMap<String, String>,
    path: String,
    contents: String,
) -> Result<(), String> {
    if files.insert(path.clone(), contents).is_some() {
        return Err(format!("duplicate generated widget file {path}"));
    }
    Ok(())
}
pub(super) fn generate(app: &Package, target: &Path) -> Result<Option<PathBuf>, String> {
    let Some(input) = app.delivery.get("widgets") else {
        return Ok(None);
    };
    let sources = input
        .as_array()
        .ok_or("package.metadata.serge.widgets must be an array of Rust file paths")?;
    let package = app.delivery["android_package"].as_str().unwrap_or("");
    let root = app.dir.canonicalize().map_err(|e| e.to_string())?;
    let mut files = BTreeMap::new();
    let mut receivers = String::new();
    let (mut updates, mut notices, mut cancels) = (String::new(), String::new(), String::new());
    let mut android = false;
    let mut notifications = false;
    let mut swift = Vec::new();
    let mut app_group = false;
    let mut live = false;
    let (mut home_cases, mut live_cases) = (String::new(), String::new());
    for file in sources {
        let name = file.as_str().ok_or("expected a widget source file")?;
        let path = root
            .join(name)
            .canonicalize()
            .map_err(|e| format!("{name}: {e}"))?;
        if !path.starts_with(&root) {
            return Err(format!("widget input outside package: {name}"));
        }
        let src = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let widgets = discover(&src).map_err(|e| format!("{name}: {e}"))?;
        if widgets.is_empty() {
            return Err(format!("no platform widget declarations in {name}"));
        }
        for w in widgets {
            let contents = w.source(package).map_err(|e| format!("{name}: {e}"))?;
            let ty = w.name();
            match w.platform() {
                Platform::LiveActivity | Platform::WidgetKit => {
                    insert(&mut files, format!("ios/{ty}.swift"), contents)?;
                    if w.platform() == Platform::WidgetKit {
                        home_cases.push_str(&format!("            case (\"{ty}\", 0): try {ty}Store.publish(data); value = kind\n"));
                    } else {
                        live_cases.push_str(&format!("            case (\"{ty}\", 1...3): value = try await runActivity({ty}Attributes(), operation: operation, data: data, id: id)\n"));
                    }
                    swift.push(ty);
                    app_group |= w.platform() == Platform::WidgetKit;
                    live |= w.platform() == Platform::LiveActivity;
                }
                Platform::Glance | Platform::Ongoing => {
                    android = true;
                    notifications |= w.platform() == Platform::Ongoing;
                    if w.platform() == Platform::Glance {
                        updates.push_str(&format!(r#"                    "{ty}" -> {{
                        val ids = manager.getGlanceIds({ty}::class.java).filter {{ id == -1 || manager.getAppWidgetId(it) == id }}
                        if (id != -1) require(ids.size == 1) {{ "Unknown app widget ID" }}
                        ids.forEach {{ {ty}.updateState(app, it, json) }}
                        ids.size
                    }}
"#));
                    } else {
                        notices.push_str(&format!("            \"{ty}\" -> {ty}.publish(context, id, channel, label, smallIcon, json, null)\n"));
                        cancels.push_str(&format!(
                            "            \"{ty}\" -> {ty}.cancel(context, id)\n"
                        ));
                    }
                    insert(
                        &mut files,
                        format!(
                            "android/src/main/java/{}/{ty}.kt",
                            package.replace('.', "/")
                        ),
                        contents,
                    )?;
                    for (path, data) in w.android_resources(package).map_err(|e| e.to_string())? {
                        if path == "receiver" {
                            receivers.push_str(&data);
                        } else {
                            insert(&mut files, format!("android/src/main/{path}"), data)?;
                        }
                    }
                }
            }
        }
    }
    if android {
        let namespace = package
            .split('.')
            .map(|s| format!("`{s}`"))
            .collect::<Vec<_>>()
            .join(".");
        let bridge = include_str!("../templates/widgets/bridge.kt")
            .replace("{{package}}", &namespace)
            .replace("{{updates}}", &updates)
            .replace("{{notifications}}", &notices)
            .replace("{{cancels}}", &cancels);
        insert(
            &mut files,
            format!(
                "android/src/main/java/{}/SergeWidgetBridge.kt",
                package.replace('.', "/")
            ),
            bridge,
        )?;
        let permission = if notifications {
            "  <uses-permission android:name=\"android.permission.POST_NOTIFICATIONS\" />\n"
        } else {
            ""
        };
        files.insert(
            "android/settings.gradle.kts".into(),
            include_str!("../templates/widgets/settings.gradle.kts").into(),
        );
        files.insert(
            "android/build.gradle.kts".into(),
            include_str!("../templates/widgets/build.gradle.kts").replace("{{namespace}}", package),
        );
        files.insert(
            "android/gradle.properties".into(),
            "android.useAndroidX=true\n".into(),
        );
        insert(
            &mut files,
            "android/src/main/AndroidManifest.xml".into(),
            format!(
                "<manifest xmlns:android=\"http://schemas.android.com/apk/res/android\">\n{permission}  <application>\n    <meta-data android:name=\"org.hyperish.serge.widgetBridge\" android:value=\"{package}.SergeWidgetBridge\" />\n{receivers}  </application>\n</manifest>\n"
            ),
        )?;
    }
    if !swift.is_empty() {
        let bridge = include_str!("../templates/widgets/host.swift")
            .replace("{{home}}", &home_cases)
            .replace("{{live}}", &live_cases);
        insert(&mut files, "ios/host/Bridge.swift".into(), bridge)?;
        insert(
            &mut files,
            "ios/requirements.json".into(),
            serde_json::json!({"appGroup":app_group,"liveActivities":live}).to_string(),
        )?;
        let list = swift
            .iter()
            .map(|ty| format!("        {ty}()\n"))
            .collect::<String>();
        insert(
            &mut files,
            "ios/SergeWidgetBundle.swift".into(),
            format!(
                "import WidgetKit\nimport SwiftUI\n\n@available(iOS 16.2, *)\n@main\nstruct SergeWidgetBundle: WidgetBundle {{\n    var body: some Widget {{\n{list}    }}\n}}\n"
            ),
        )?;
    }
    // Never overwrite a previous valid generation on a parse or write error.
    // An immutable generation and atomically replaced pointer also permit
    // concurrent builds and let an ongoing host build finish reading its files.
    let dir = target.join("serge-widgets").join(&app.name);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stage = tempfile::Builder::new()
        .prefix("generation-")
        .tempdir_in(&dir)
        .map_err(|e| e.to_string())?;
    for (path, data) in files {
        let path = stage.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(path, data).map_err(|e| e.to_string())?;
    }
    let mut pointer = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
    use std::io::Write;
    writeln!(
        pointer,
        "{}",
        stage.path().file_name().unwrap().to_string_lossy()
    )
    .map_err(|e| e.to_string())?;
    let published = stage.keep();
    if let Err(error) = pointer.persist(dir.join("current")) {
        let _ = fs::remove_dir_all(published);
        return Err(error.to_string());
    }
    eprintln!(
        "hyve-serge: {} platform widgets → {}",
        app.name,
        published.display()
    );
    Ok(Some(published))
}
