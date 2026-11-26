//! Compile the optional Core Video to Metal bridge for Apple targets.
fn main() {
    #[cfg(feature = "native-metal")]
    if std::env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple") {
        build_metal();
    }
    #[cfg(feature = "camera")]
    if std::env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple") {
        println!("cargo:rerun-if-changed=apple/camera.m");
        cc::Build::new()
            .file("apple/camera.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("serge_camera");
        for framework in [
            "AVFoundation",
            "Foundation",
            "CoreMedia",
            "CoreVideo",
            "Metal",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}

#[cfg(feature = "native-metal")]
fn build_metal() {
    use naga::{
        ResourceBinding,
        back::msl,
        valid::{Capabilities, ValidationFlags, Validator},
    };
    println!("cargo:rerun-if-changed=src/shader.wgsl");
    println!("cargo:rerun-if-changed=apple/batch.m");
    let module = naga::front::wgsl::parse_str(include_str!("src/shader.wgsl")).expect("Serge WGSL");
    let checked = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .expect("valid shader");
    let mut resources = msl::EntryPointResources::default();
    for (binding, buffer, texture) in [(0, Some(0), None), (1, None, Some(0)), (2, None, Some(1))] {
        resources.resources.insert(
            ResourceBinding { group: 0, binding },
            msl::BindTarget {
                buffer,
                texture,
                ..Default::default()
            },
        );
    }
    let options = msl::Options {
        lang_version: (2, 3),
        fake_missing_bindings: false,
        per_entry_point_map: [
            ("vs_main".into(), resources.clone()),
            ("fs_main".into(), resources),
        ]
        .into(),
        ..Default::default()
    };
    let pipeline = msl::PipelineOptions {
        vertex_pulling_transform: false,
        ..Default::default()
    };
    let (source, translated) =
        msl::write_string(&module, &checked, &options, &pipeline).expect("Metal shader");
    for (entry, name) in module.entry_points.iter().zip(translated.entry_point_names) {
        println!(
            "cargo:rustc-env=SERGE_METAL_{}={}",
            entry.name,
            name.expect("bound shader entry point")
        );
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(out.join("scene.metal"), source).unwrap();
    cc::Build::new()
        .file("apple/batch.m")
        .flag("-fobjc-arc")
        .flag("-fblocks")
        .compile("serge_metal_batch");
    println!("cargo:rustc-link-lib=framework=Metal");
    println!("cargo:rustc-link-lib=framework=Foundation");
}
