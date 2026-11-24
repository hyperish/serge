//! The GPU path draws what the CPU path draws (`docs/05-implementation-plan.md`
//! §3, 렌더러 대칭): every primitive kind, translucency, nested fractional
//! clips and text, at 1× and 2×, within the fixed tolerance — in one draw
//! call.
#![allow(missing_docs)]

use serge_core::{Point, Rect, ScenePrimitive};
use serge_native_cpu::{CpuRenderer, Frame};
use serge_native_gpu::{Parity, test_renderer};
use serge_native_text::TextSystem;
use serge_style::Color;

fn scene(text: &mut TextSystem) -> Vec<ScenePrimitive> {
    let c = |hex: u32, a: u8| {
        let mut c = Color::hex(hex);
        c.a = a;
        c
    };
    let mut s = vec![
        ScenePrimitive::Quad {
            rect: Rect::new(0.0, 0.0, 420.0, 300.0),
            color: c(0xf8f9fa, 255),
            radius: 0.0,
        },
        ScenePrimitive::Quad {
            rect: Rect::new(10.5, 10.25, 180.0, 90.0),
            color: c(0xffffff, 255),
            radius: 12.0,
        },
        ScenePrimitive::Border {
            rect: Rect::new(10.5, 10.25, 180.0, 90.0),
            color: c(0xdee2e6, 255),
            width: 1.0,
            radius: 12.0,
        },
        ScenePrimitive::Border {
            rect: Rect::new(210.0, 12.0, 90.0, 40.0),
            color: c(0x3b5bdb, 255),
            width: 3.0,
            radius: 0.0,
        },
        ScenePrimitive::Quad {
            rect: Rect::new(230.0, 30.0, 120.0, 60.0),
            color: c(0xe03131, 128),
            radius: 6.0,
        },
        ScenePrimitive::Quad {
            rect: Rect::new(320.3, 10.7, 0.6, 50.0),
            color: c(0x212529, 255),
            radius: 0.0,
        },
        ScenePrimitive::PushClip(Rect::new(20.25, 120.5, 200.0, 80.0)),
        ScenePrimitive::Quad {
            rect: Rect::new(0.0, 100.0, 420.0, 120.0),
            color: c(0x74c0fc, 255),
            radius: 0.0,
        },
        ScenePrimitive::PushClip(Rect::new(60.0, 110.0, 300.0, 60.5)),
        ScenePrimitive::Quad {
            rect: Rect::new(40.0, 130.0, 100.0, 100.0),
            color: c(0x2f9e44, 200),
            radius: 30.0,
        },
        ScenePrimitive::PopClip,
        ScenePrimitive::PopClip,
        ScenePrimitive::Line {
            points: vec![
                Point::new(240.0, 130.0),
                Point::new(270.5, 180.0),
                Point::new(300.0, 140.25),
                Point::new(400.0, 190.0),
            ],
            color: c(0x7048e8, 255),
            width: 2.5,
        },
        ScenePrimitive::Polygon {
            // Concave.
            points: vec![
                Point::new(250.0, 200.0),
                Point::new(330.0, 210.0),
                Point::new(280.0, 230.0),
                Point::new(330.0, 270.0),
                Point::new(245.0, 260.0),
            ],
            color: c(0xf08c00, 180),
        },
    ];
    for (i, (line, size)) in [
        ("Count: 42 — Save", 14.0),
        ("Hide done · 테스트 漢字", 15.5),
        ("Exposure 3.00", 22.0),
    ]
    .into_iter()
    .enumerate()
    {
        let layout = text.layout(line, size, if i == 2 { 700 } else { 400 }, None);
        let origin = Point::new(16.3 + i as f32 * 0.37, 210.0 + i as f32 * 27.6);
        s.extend(layout.primitives(origin, c(0x212529, 255)));
    }
    // Text inside the card, under a clip.
    s.push(ScenePrimitive::PushClip(Rect::new(
        10.5, 10.25, 180.0, 90.0,
    )));
    let layout = text.layout(
        "Clipped label that runs past the card edge",
        14.0,
        400,
        None,
    );
    s.extend(layout.primitives(Point::new(24.0, 40.0), c(0x495057, 255)));
    s.push(ScenePrimitive::PopClip);
    s
}

fn dump(f: &Frame, name: &str) {
    if let Ok(dir) = std::env::var("SERGE_SNAPSHOT_DIR") {
        let mut out = format!("P6\n{} {}\n255\n", f.width, f.height).into_bytes();
        for p in f.rgba.as_chunks::<4>().0 {
            out.extend_from_slice(&p[..3]);
        }
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.ppm")), out).unwrap();
    }
}

#[test]
fn gpu_frames_match_cpu_frames() {
    let Some(mut gpu) = test_renderer() else {
        return;
    };
    let mut text = TextSystem::new();
    let scene = scene(&mut text);
    let mut cpu = CpuRenderer::new();
    for scale in [1.0_f32, 2.0] {
        let (w, h) = ((420.0 * scale) as u16, (300.0 * scale) as u16);
        let a = cpu.render(&scene, w, h, scale);
        let b = gpu.render(&scene, w, h, scale).expect("GPU frame");
        dump(&a, &format!("parity-cpu@{scale}x"));
        dump(&b, &format!("parity-gpu@{scale}x"));
        let p = Parity::of(&a, &b);
        eprintln!("{} @{scale}x: {p:?}", gpu.adapter());
        assert!(p.within_tolerance(), "@{scale}x: {p:?}");
        let stats = gpu.stats();
        assert_eq!(stats.draw_calls, 1, "one draw call per frame: {stats:?}");
        assert!(stats.instances > 40, "{stats:?}");
    }
    // A second frame hits the glyph cache.
    let _ = gpu.render(&scene, 840, 600, 2.0).unwrap();
    assert_eq!(gpu.stats().glyphs_rasterized, 0, "{:?}", gpu.stats());
}

#[test]
fn an_empty_scene_draws_nothing() {
    let Some(mut gpu) = test_renderer() else {
        return;
    };
    let f = gpu.render(&[], 16, 16, 1.0).unwrap();
    assert!(f.rgba.iter().all(|b| *b == 0));
    assert_eq!(gpu.stats().draw_calls, 0);
}

/// The fallback policy with real device creation: a backend that is not
/// built in fails on every machine, so an explicit `gpu` is an error and
/// automatic selection falls back to the CPU; with the real adapter list,
/// automatic selection agrees with whether a device exists.
#[test]
fn the_fallback_policy_with_real_devices() {
    use serge_native::{Backend, BackendError, choose_backend};
    use serge_native_gpu::{GpuRenderer, wgpu};
    let none = || {
        GpuRenderer::headless_with(wgpu::Backends::GL)
            .map(drop)
            .map_err(|e| e.0)
    };
    assert_eq!(choose_backend(None, false, none), Ok(Backend::Cpu));
    assert!(matches!(
        choose_backend(Some("gpu"), false, none),
        Err(BackendError::GpuUnavailable(_))
    ));
    assert_eq!(
        choose_backend(Some("cpu"), false, || panic!("not tried")),
        Ok(Backend::Cpu)
    );
    assert_eq!(
        choose_backend(None, true, || panic!("not tried")),
        Ok(Backend::Cpu)
    );
    let real = || GpuRenderer::headless().map(drop).map_err(|e| e.0);
    let available = GpuRenderer::headless().is_ok();
    let auto = choose_backend(None, false, real);
    assert_eq!(
        auto,
        Ok(if available {
            Backend::Gpu
        } else {
            Backend::Cpu
        })
    );
    if std::env::var("HYPERISH_TEST_GPU").as_deref() == Ok("require") {
        assert_eq!(auto, Ok(Backend::Gpu), "HYPERISH_TEST_GPU=require");
    }
}
