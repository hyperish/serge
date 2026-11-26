use hyve::Ulid;
use serge_web_ssr::{VerifiedBundle, digest};
use std::{fs, path::Path};

pub fn verified_page(
    directory: &Path,
    app_source: &str,
    codec: Option<(&[u8], &[u8])>,
) -> (VerifiedBundle, String) {
    let build = Ulid::new().to_string();
    let mut assets = vec![];
    let mut add = |name: &str, media: &str, bytes: &[u8]| {
        fs::write(directory.join(name), bytes).unwrap();
        let hash = digest(bytes);
        let url = format!("/assets/{hash}/{name}");
        assets.push(serde_json::json!({"url":url,"file":name,"mediaType":media,"bytes":bytes.len(),"sha256":hash}));
        url
    };
    let (imports, codec) = match codec {
        None => (String::new(), String::new()),
        Some((js_bytes, wasm_bytes)) => {
            let js = add("codec.js", "text/javascript", js_bytes);
            let wasm = add("codec.wasm", "application/wasm", wasm_bytes);
            (format!("import init, {{sergeWebviewEncode, sergeWebviewDecode}} from '{js}';\nawait init({{module_or_path:'{wasm}'}});"),"codec:{encode:sergeWebviewEncode,decode:sergeWebviewDecode},fingerprint:'0000000000000010',".into())
        }
    };
    let app = app_source
        .replace("__IMPORTS__", &imports)
        .replace("__BUILD__", &build)
        .replace("__CODEC__", &codec);
    let js = add("app.js", "text/javascript", app.as_bytes());
    let html = format!(
        r#"<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Local slot</title><output></output><script>window.inlineRan=true</script><script type="module" src="{js}"></script>"#
    );
    fs::write(directory.join("page.html"), &html).unwrap();
    fs::write(directory.join("serge-bundle.json"),serde_json::to_vec(&serde_json::json!({"version":1,"buildId":build,"assets":assets,"routes":[{"mode":"csr","pattern":"/","file":"page.html"}]})).unwrap()).unwrap();
    let bundle = VerifiedBundle::open(directory, &[]).unwrap();
    (bundle, digest(html.as_bytes()))
}
