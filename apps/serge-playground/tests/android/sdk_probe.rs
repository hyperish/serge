//! Runs inside the disposable NativeActivity used by sdk.py.
mod vulkan_probe;
mod webview_document;
mod webview_probe;
use android_activity::{AndroidApp, MainEvent, PollEvent};
use rustls_platform_verifier::BuilderVerifierExt;
use serge_native_sys::{
    AndroidSystem,
    hw::{HapticPulse, Microphone, Sensor, SensorKind, SensorSample},
};
use std::{
    io::{self, Read, Write},
    net::TcpStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn camera_frames(system: &AndroidSystem) -> Result<()> {
    use serge_native_sys::hw::CameraTexture;
    unsafe extern "C" {
        fn probe_camera_context() -> u32;
        fn probe_camera_draw(transform: *const f32) -> bool;
        fn probe_camera_finish();
    }
    let ids = CameraTexture::cameras(system)?;
    let id = ids.first().ok_or("emulator has no configured camera")?;
    let sizes = CameraTexture::sizes(system, id)?;
    let size = sizes
        .into_iter()
        .filter(|(w, h)| *w >= 160 && *h >= 120)
        .min_by_key(|(w, h)| u64::from(*w) * u64::from(*h))
        .ok_or("no texture size")?;
    let texture = unsafe { probe_camera_context() };
    if texture == 0 {
        return Err("no GLES external camera texture".into());
    }
    let stream = CameraTexture::open(system, id, size, texture)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last = None;
    loop {
        if Instant::now() >= deadline {
            return Err("no changing camera texture".into());
        }
        if let Some(frame) = stream.try_update()? {
            if !frame.transform.iter().all(|n| n.is_finite())
                || !unsafe { probe_camera_draw(frame.transform.as_ptr()) }
            {
                return Err("external camera texture did not render".into());
            }
            if last.is_some_and(|t| t < frame.timestamp_ns) {
                break;
            }
            last = Some(frame.timestamp_ns);
        }
        thread::sleep(Duration::from_millis(15));
    }
    drop(stream);
    unsafe { probe_camera_finish() };
    Ok(())
}

fn device_ports(sys: &AndroidSystem) -> Result<()> {
    use serge_native_sys::hw::{
        ble::api::{Central, Peripheral, ScanFilter, WriteType},
        midi::{MidiInput, MidiOutput},
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let adapters = sys.bluetooth_adapters().await?;
        let adapter = adapters
            .first()
            .ok_or_else(|| io::Error::other("missing emulator BLE central"))?;
        let peer = option_env!("SERGE_PROBE_GATT");
        let service = peer
            .unwrap_or("21eefd75-0000-4b83-9b1b-16bf6d6e7f21")
            .parse()?;
        let scan = ScanFilter {
            services: vec![service],
        };
        adapter.start_scan(scan).await?;
        if peer.is_some() {
            use futures_util::StreamExt;
            let exchange = async {
                let peripheral = 'discovery: loop {
                    for p in adapter.peripherals().await? {
                        if p.properties()
                            .await?
                            .is_some_and(|s| s.services.contains(&service))
                        {
                            break 'discovery p;
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                };
                peripheral.connect().await?;
                peripheral.discover_services().await?;
                let id: uuid::Uuid = "d1fcf31e-253b-43da-91fc-4bc7d8848211".parse()?;
                let ch = peripheral
                    .characteristics()
                    .into_iter()
                    .find(|c| c.uuid == id && c.service_uuid == service)
                    .ok_or("missing GATT characteristic")?;
                assert_eq!(peripheral.read(&ch).await?, [1, 2, 3]);
                let mut notifications = peripheral.notifications().await?;
                peripheral.subscribe(&ch).await?;
                let payload = b"serge-test";
                peripheral
                    .write(&ch, payload, WriteType::WithResponse)
                    .await?;
                let event = notifications
                    .next()
                    .await
                    .ok_or("GATT notification channel closed")?;
                assert_eq!(event.uuid, id);
                assert_eq!(event.value, payload);
                assert_eq!(peripheral.read(&ch).await?, payload);
                peripheral.unsubscribe(&ch).await?;
                peripheral.disconnect().await?;
                Ok::<_, Box<dyn std::error::Error>>(())
            };
            tokio::time::timeout(Duration::from_secs(20), exchange).await??;
        } else {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        adapter.stop_scan().await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    let input = MidiInput::new("Serge SDK input")?;
    let output = MidiOutput::new("Serge SDK output")?;
    let source = input
        .ports()
        .into_iter()
        .find(|p| {
            input
                .port_name(p)
                .is_ok_and(|n| n.contains("Serge test output"))
        })
        .ok_or("no virtual MIDI source")?;
    let sink = output
        .ports()
        .into_iter()
        .find(|p| {
            output
                .port_name(p)
                .is_ok_and(|n| n.contains("Serge test input"))
        })
        .ok_or("no virtual MIDI sink")?;
    let (tx, rx) = std::sync::mpsc::channel();
    let _receiver = input.connect(
        &source,
        "probe",
        move |_, data, _| {
            let _ = tx.send(data.to_vec());
        },
        (),
    )?;
    let mut sender = output.connect(&sink, "probe")?;
    for note in [[0x90, 60, 90], [0x80, 60, 0]] {
        sender.send(&note)?;
        if rx.recv_timeout(Duration::from_secs(4))? != note {
            return Err("MIDI loopback bytes changed".into());
        }
    }
    Ok(())
}

fn tls(port: u16, host: &'static str, root: Option<&[u8]>) -> Result<String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?;
    let client = if let Some(der) = root {
        let mut store = rustls::RootCertStore::empty();
        store.add(rustls::pki_types::CertificateDer::from(der.to_vec()))?;
        builder.with_root_certificates(store)
    } else {
        builder.with_platform_verifier()?
    }
    .with_no_client_auth();
    let conn = rustls::ClientConnection::new(Arc::new(client), host.try_into()?)?;
    let raw = TcpStream::connect(("127.0.0.1", port))?;
    raw.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut stream = rustls::StreamOwned::new(conn, raw);
    write!(
        stream,
        "GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = [0; 512];
    let n = stream.read(&mut response)?;
    Ok(String::from_utf8_lossy(&response[..n]).into_owned())
}
fn background_work(sys: &AndroidSystem, resumed: &AtomicBool) -> Result<()> {
    use serge_native_sys::BackgroundStatus;
    fn wait(condition: impl Fn() -> bool) -> Result<()> {
        let end = Instant::now() + Duration::from_secs(15);
        while !condition() {
            if Instant::now() >= end {
                return Err("Android background lifecycle timed out".into());
            }
            thread::sleep(Duration::from_millis(25));
        }
        Ok(())
    }
    let work = sys.background_task(
        "Saving the project",
        "Completing the current save",
        Duration::from_secs(35),
    )?;
    wait(|| work.status().ok() == Some(BackgroundStatus::Active))?;
    let short = sys.background_task("Index", "Updating the index", Duration::from_millis(200))?;
    assert_eq!(short.status()?, BackgroundStatus::Active);
    wait(|| short.status().ok() == Some(BackgroundStatus::Ended))?;
    drop(short);
    println!("SERGE_BACKGROUND_READY");
    wait(|| !resumed.load(Ordering::Acquire))?;
    assert_eq!(work.status()?, BackgroundStatus::Active);
    let file = sys.data_directory()?.join("background-save.txt");
    std::fs::write(&file, b"saved while the activity was stopped")?;
    println!("SERGE_BACKGROUND_SAVED={}", file.display());
    wait(|| resumed.load(Ordering::Acquire))?;
    drop(work);
    Ok(())
}

fn pen_strokes(sys: &AndroidSystem) -> Result<()> {
    use serge_native_sys::hw::{StylusArea, StylusPhase};
    let pad = sys.stylus_pad(StylusArea {
        x: 64,
        y: 64,
        width: 360,
        height: 410,
    })?;
    let end = Instant::now() + Duration::from_secs(20);
    while !pad.is_ready()? {
        if Instant::now() >= end {
            return Err("stylus pad did not open".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
    println!("SERGE_PEN_READY");
    let mut samples = Vec::new();
    loop {
        let batch = pad.drain()?;
        assert_eq!(batch.dropped, 0);
        samples.extend(batch.samples);
        if samples
            .last()
            .is_some_and(|s| s.phase == StylusPhase::Cancel)
        {
            break;
        }
        if Instant::now() >= end {
            return Err(format!("missing pen stroke: {samples:?}").into());
        }
        thread::sleep(Duration::from_millis(25));
    }
    let downs: Vec<_> = samples
        .iter()
        .filter(|s| s.phase == StylusPhase::Down)
        .collect();
    assert_eq!(downs.len(), 2, "{samples:?}");
    assert!(!downs[0].eraser && downs[1].eraser);
    for down in &downs {
        assert!((0.23..0.27).contains(&down.pressure) && (0.18..0.22).contains(&down.tilt));
        assert!(down.position[0] > 0. && down.position[1] > 0. && down.timestamp_ns > 0);
        assert!(
            samples.iter().any(|s| s.phase == StylusPhase::Move
                && s.eraser == down.eraser
                && s.pointer_id == down.pointer_id
                && s.timestamp_ns >= down.timestamp_ns
                && (0.73..0.77).contains(&s.pressure)
                && (0.58..0.62).contains(&s.tilt)
                && (s.position[0] - down.position[0] - 12.).abs() < 1.
                && (s.position[1] - down.position[1] - 8.).abs() < 1.),
            "{samples:?}"
        );
    }
    assert!(
        samples
            .iter()
            .any(|s| s.phase == StylusPhase::Up && !s.eraser)
    );
    assert!(pad.drain()?.samples.is_empty());
    println!("SERGE_PEN_ACCEPTED");
    Ok(())
}

fn notification_action(sys: &AndroidSystem) -> Result<()> {
    use serge_native_sys::{NotificationAction, NotificationCategory, NotificationResponse};
    let actions = [NotificationAction {
        id: "archive",
        title: "Archive project",
    }];
    let category = NotificationCategory {
        id: "project",
        actions: &actions,
    };
    assert!(sys.take_notification_action()?.is_none());
    assert!(sys.post_local_notification("project-17", "First revision", "Draft", category)?);
    sys.cancel_local_notification("project-17")?;
    assert!(sys.post_local_notification("project-17", "Preparing project", "Working", category)?);
    assert!(sys.post_local_notification(
        "project-17",
        "Serge project ready",
        "Project 17",
        category
    )?);
    println!("SERGE_NOTIFICATION_READY");
    let end = Instant::now() + Duration::from_secs(45);
    loop {
        if let Some(response) = sys.take_notification_action()? {
            assert_eq!(
                response,
                NotificationResponse {
                    notification: "project-17".into(),
                    action: "archive".into()
                }
            );
            break;
        }
        if Instant::now() >= end {
            return Err("missing notification action".into());
        }
        thread::sleep(Duration::from_millis(80));
    }
    assert!(sys.take_notification_action()?.is_none());
    println!("SERGE_NOTIFICATION_ACCEPTED");
    Ok(())
}

fn check(app: &AndroidApp, resumed: &AtomicBool) -> Result<()> {
    serge_adapter_client::android::initialize_platform_verifier(app)?;
    let ok = tls(
        env!("SERGE_PROBE_GOOD").parse()?,
        "localhost",
        Some(include_bytes!("ca.der")),
    )?;
    if !ok.starts_with("HTTP/1.0 200") {
        return Err("trusted loopback endpoint did not return 200".into());
    }
    let rejected = tls(env!("SERGE_PROBE_BAD").parse()?, "localhost", None).unwrap_err();
    if !rejected.to_string().contains("certificate") {
        return Err(format!("expected TLS certificate refusal, got {rejected}").into());
    }
    if let Some((host, port)) = option_env!("SERGE_PROBE_PUBLIC").and_then(|s| s.split_once(':')) {
        let response = tls(port.parse()?, host, None)?;
        if !response.starts_with("HTTP/") {
            return Err("system-trusted TLS endpoint did not respond".into());
        }
    }
    let sys = AndroidSystem::new(app.clone());
    assert_eq!(sys.package_name()?, "org.hyperish.sdkprobe");
    assert_eq!(
        sys.launch_uri()?.as_deref(),
        Some("serge-probe://projects/17?tab=notes")
    );
    let dir = sys.data_directory()?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("probe.txt"), "private")?;
    assert_eq!(std::fs::read_to_string(dir.join("probe.txt"))?, "private");
    assert!(sys.is_granted("android.permission.RECORD_AUDIO")?);
    sys.set_clipboard_text("probe", "Serge Android clipboard")?;
    assert_eq!(
        sys.clipboard_text()?.as_deref(),
        Some("Serge Android clipboard")
    );
    let vibrator = sys.vibrate(40)?;
    let pattern = [
        HapticPulse {
            millis: 24,
            amplitude: 96,
        },
        HapticPulse {
            millis: 16,
            amplitude: 0,
        },
        HapticPulse {
            millis: 32,
            amplitude: 255,
        },
    ];
    assert_eq!(sys.vibrate_waveform(&pattern)?, vibrator);
    assert_eq!(
        sys.vibrate_waveform(&[]).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    pen_strokes(&sys)?;
    notification_action(&sys)?;
    device_ports(&sys)?;
    camera_frames(&sys)?;
    vulkan_probe::run(&sys)?;
    let sensor = Sensor::open(
        &sys,
        SensorKind::Accelerometer,
        Duration::from_millis(20),
        32,
    )?;
    let audio = Microphone::open(&sys, 1024)?;
    let end = Instant::now() + Duration::from_secs(8);
    while sensor.samples().total_pushed() < 2 || audio.samples().total_pushed() < 512 {
        if Instant::now() >= end {
            return Err(io::Error::other(format!(
                "sensor={} microphone={}",
                sensor.samples().total_pushed(),
                audio.samples().total_pushed()
            ))
            .into());
        }
        thread::sleep(Duration::from_millis(30));
    }
    let mut samples = [SensorSample::default(); 2];
    assert_eq!(sensor.samples().read_latest_snapshot(&mut samples), 2);
    assert!(samples[1].timestamp_ns > samples[0].timestamp_ns);
    assert!(samples[1].xyz.iter().all(|n| n.is_finite()));
    assert!(audio.sample_rate() > 0);
    let ring = sensor.samples().clone();
    drop(sensor);
    let n = ring.total_pushed();
    thread::sleep(Duration::from_millis(80));
    assert_eq!(ring.total_pushed(), n);
    drop(audio);
    webview_probe::run(app)?;
    let credential = sys.credential("org.hyperish.sdkprobe", "fixture")?;
    assert!(credential.get_secret()?.is_none());
    credential.set_secret(b"first")?;
    credential.set_secret(b"updated")?;
    assert_eq!(
        sys.credential("org.hyperish.sdkprobe", "fixture")?
            .get_secret()?,
        Some(b"updated".to_vec())
    );
    credential.delete()?;
    assert!(credential.get_secret()?.is_none());
    use serge_native_sys::hw::SigningKey;
    let first = SigningKey::open(&sys, "device-key", false)?;
    let public = first.public_key()?;
    let signature = first.sign(b"device message")?;
    let reopened = SigningKey::open(&sys, "device-key", false)?;
    assert_eq!(public, reopened.public_key()?);
    assert!(reopened.verify(b"device message", &signature)?);
    assert!(!reopened.verify(b"changed", &signature)?);
    assert_eq!(
        SigningKey::open(&sys, "device-key", true).is_ok(),
        first.hardware_backed()?
    );
    reopened.delete()?;
    let replacement = SigningKey::open(&sys, "device-key", false)?;
    assert_ne!(replacement.public_key()?, public);
    assert!(first.delete().is_err());
    replacement.delete()?;
    fn wait_prompt(prompt: serge_native_sys::Prompt) -> Result<serge_native_sys::PromptResult> {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(result) = prompt.try_result()? {
                return Ok(result);
            }
            if Instant::now() >= end {
                return Err("native prompt timed out".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn selected(prompt: serge_native_sys::Prompt) -> Result<String> {
        match wait_prompt(prompt)? {
            serge_native_sys::PromptResult::Selected(value) => Ok(value),
            serge_native_sys::PromptResult::Cancelled => Err("native prompt cancelled".into()),
        }
    }
    if option_env!("SERGE_PROBE_FINGERPRINT").is_some() {
        let prompt = sys.authenticate("Unlock project", "Confirm with your fingerprint")?;
        println!("SERGE_BIOMETRIC_WAIT");
        assert_eq!(selected(prompt)?, "authenticated");
        let cancel = sys.authenticate("Cancel verification", "Dismiss this request")?;
        println!("SERGE_BIOMETRIC_CANCEL");
        assert_eq!(wait_prompt(cancel)?, serge_native_sys::PromptResult::Cancelled);
        println!("SERGE_BIOMETRIC_ACCEPTED");
    }
    println!("SERGE_PROBE_DIALOG");
    assert_eq!(
        selected(sys.choose("SDK options", &["First", "Second"])?)?,
        "1"
    );
    assert_eq!(
        selected(sys.alert("SDK alert", "Saved successfully")?)?,
        "ok"
    );
    let date = selected(sys.pick_date()?)?;
    assert!(date.len() == 10 && date.as_bytes()[4] == b'-');
    let time = selected(sys.pick_time()?)?;
    assert!(time.len() == 5 && time.as_bytes()[2] == b':');
    let filename = env!("SERGE_PROBE_DOCUMENT");
    println!("SERGE_PROBE_DOCUMENT_CREATE");
    let uri = selected(sys.create_document("text/plain", filename)?)?;
    sys.write_document(&uri)?
        .write_all(b"Serge selected document")?;
    println!("SERGE_PROBE_DOCUMENT_OPEN");
    let selection = selected(sys.pick_document("text/plain")?)?;
    let mut text = String::new();
    sys.read_document(&selection)?.read_to_string(&mut text)?;
    assert_eq!(text, "Serge selected document");
    println!("SERGE_PROBE_DOCUMENT_CANCEL");
    let cancel = sys.pick_document("text/plain")?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if cancel.try_result()? == Some(serge_native_sys::PromptResult::Cancelled) {
            break;
        }
        if Instant::now() > deadline {
            return Err("document picker did not cancel".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
    let posture = sys.window_posture()?;
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if let Some(folds) = posture.latest()? {
            for fold in &folds {
                let [left, top, right, bottom] = fold.bounds;
                assert!(right >= left && bottom >= top);
            }
            if option_env!("SERGE_PROBE_FOLD").is_some()
                && !folds
                    .iter()
                    .any(|f| f.state == serge_native_sys::FoldState::Flat)
            {
                if Instant::now() >= deadline {
                    return Err(format!("no flat hinge: {folds:?}").into());
                }
                thread::sleep(Duration::from_millis(30));
                continue;
            }
            break;
        }
        if Instant::now() >= deadline {
            return Err("no AndroidX window layout delivery".into());
        }
        thread::sleep(Duration::from_millis(30));
    }
    if option_env!("SERGE_PROBE_FOLD").is_some() {
        let hinge = Sensor::open(&sys, SensorKind::HingeAngle, Duration::from_millis(20), 8)?;
        println!("SERGE_PROBE_FOLD_WAIT");
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut samples = [SensorSample::default(); 8];
        loop {
            let n = hinge.samples().read_latest_snapshot(&mut samples);
            let right_angle = samples[..n].iter().any(|s| (s.xyz[0] - 90.).abs() < 2.);
            let folds = posture.latest()?.unwrap_or_default();
            if right_angle
                && folds
                    .iter()
                    .any(|f| f.state == serge_native_sys::FoldState::HalfOpened && f.separating)
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err(format!("hinge did not move: {folds:?}, {:?}", &samples[..n]).into());
            }
            thread::sleep(Duration::from_millis(30));
        }
    }
    drop(posture);
    background_work(&sys, resumed)?;
    assert!(!sys.in_picture_in_picture()?);
    assert_eq!(selected(sys.enter_picture_in_picture(16, 9)?)?, "entered");
    let deadline = Instant::now() + Duration::from_secs(6);
    while !sys.in_picture_in_picture()? {
        if Instant::now() >= deadline {
            return Err("activity never entered PiP".into());
        }
        thread::sleep(Duration::from_millis(30));
    }
    Ok(())
}
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    let mut focused = false;
    let until = Instant::now() + Duration::from_secs(15);
    while !focused && Instant::now() < until {
        app.poll_events(Some(Duration::from_millis(200)), |event| {
            if matches!(event, PollEvent::Main(MainEvent::GainedFocus)) {
                focused = true
            }
        });
    }
    let host = app.clone();
    let resumed = Arc::new(AtomicBool::new(focused));
    let lifecycle = resumed.clone();
    let worker = thread::spawn(move || check(&host, &lifecycle).map_err(|error| error.to_string()));
    while !worker.is_finished() {
        app.poll_events(Some(Duration::from_millis(30)), |event| match event {
            PollEvent::Main(MainEvent::Stop) => resumed.store(false, Ordering::Release),
            PollEvent::Main(MainEvent::Resume { .. }) => resumed.store(true, Ordering::Release),
            _ => {}
        });
    }
    match worker.join().expect("SDK worker") {
        Ok(()) => println!("SERGE_SDK_PROBE_OK"),
        Err(e) => println!("SERGE_SDK_PROBE_FAILED {e}"),
    }
}
