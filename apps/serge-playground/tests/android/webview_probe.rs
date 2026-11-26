use super::{Result, webview_document};
use android_activity::AndroidApp;
use hyve::{HyveError, Value};
use serge_adapter_webview::{LocalDocument, Rect, Slot};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

pub fn run(app: &AndroidApp) -> Result<()> {
    let js = include_bytes!("webview_codec.js");
    let wasm = include_bytes!("webview_codec_bg.wasm");
    let codecs = [None, Some((js.as_slice(), wasm.as_slice()))];
    for codec in codecs.into_iter().take(if wasm.is_empty() { 1 } else { 2 }) {
        let directory = app
            .internal_data_path()
            .ok_or("no private data")?
            .join("local-slot");
        fs::create_dir_all(&directory)?;
        let (bundle, hash) =
            webview_document::verified_page(&directory, include_str!("webview.js"), codec);
        assert!(LocalDocument::from_bundle(&bundle, "page.html", &"0".repeat(64)).is_err());
        let doc = LocalDocument::from_bundle(&bundle, "page.html", &hash)?;
        fs::remove_dir_all(directory)?;
        let (sent, result) = mpsc::sync_channel(1);
        let saved = Arc::new(AtomicUsize::new(0));
        let count = saved.clone();
        let slot = Slot::attach(
            app,
            Rect {
                x: 8,
                y: 8,
                width: 360,
                height: 210,
            },
            doc,
            16,
            0.into(),
            |p| {
                if p.get("op") == Some(&"denied".into()) {
                    Err(HyveError::domain("forbidden", "revoked"))
                } else {
                    Ok(())
                }
            },
            move |id, payload, slot| {
                let checked = (|| match payload.get("op") {
                    Some(Value::String(op))
                        if op == "save"
                            && id.seq == 1
                            && count.fetch_add(1, Ordering::SeqCst) == 0 =>
                    {
                        slot.update(1.into())?;
                        slot.reload()?;
                        slot.finish(id, Ok("saved".into()))?;
                        Ok(false)
                    }
                    Some(Value::String(op))
                        if op == "done"
                            && id.seq == 3
                            && payload.get("rendered") == Some(&"1".into())
                            && payload.get("guard") == Some(&"forbidden".into())
                            && payload.get("inline") == Some(&false.into()) =>
                    {
                        slot.finish(id, Ok(Value::Null))?;
                        Ok(true)
                    }
                    _ => Err(HyveError::internal(format!(
                        "unexpected Android WebView event {id:?} {payload:?}"
                    ))),
                })();
                match checked {
                    Ok(false) => {}
                    other => {
                        let _ = sent.try_send(other.map_err(|e| e.to_string()));
                    }
                }
            },
        )?;
        slot.set_bounds(Rect {
            x: 8,
            y: 12,
            width: 320,
            height: 200,
        })?;
        assert!(result.recv_timeout(Duration::from_secs(25))??);
        assert_eq!(saved.load(Ordering::SeqCst), 1);
        drop(slot);
        println!(
            "SERGE_WEBVIEW_ACCEPTED_{}",
            if codec.is_some() { "ARCHIVE" } else { "JSON" }
        );
    }
    Ok(())
}
