//! Real GPU and CPU composition of two mounted producers, including clipping and stale generations.
use serge_core::{
    Point, Rect, ScenePrimitive as P,
    viewport::{Extent, Pointer, Viewport},
};
use serge_native::Headless;
use serge_native_cpu::CpuRenderer;
use serge_native_gpu::{
    GpuRenderer,
    viewport::{GpuViewport, wgpu},
};
use serge_style::{Color, Length, StyleProp};
use std::{cell::RefCell, rc::Rc};

fn producer(color: [u8; 4]) -> Viewport {
    Viewport::new("external view")
        .cpu(move |n| Some(color.repeat(n.byte_len()? / 4)))
        .backend(GpuViewport::new(move |f| {
            let _pass = f.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: f.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(color[0]) / 255.0,
                            g: f64::from(color[1]) / 255.0,
                            b: f64::from(color[2]) / 255.0,
                            a: f64::from(color[3]) / 255.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }))
}
#[test]
fn compose_two_producers_in_scene_order_on_the_same_device() {
    let left = producer([255, 0, 0, 255]).mount();
    let right = producer([0, 0, 128, 128]).mount();
    let size = Extent {
        width: 6,
        height: 6,
    };
    let a = left.frame(size);
    let b = right.frame(size);
    let scene = [
        P::ExternalTexture {
            key: a,
            rect: Rect::new(1., 1., 6., 6.),
        },
        P::PushClip(Rect::new(4., 2., 3., 4.)),
        P::ExternalTexture {
            key: b,
            rect: Rect::new(3., 1., 6., 6.),
        },
        P::PopClip,
        P::Quad {
            rect: Rect::new(4., 3., 1., 1.),
            color: Color::rgba(0, 255, 0, 255),
            radius: 0.,
        },
    ];
    let mut cpu = CpuRenderer::new();
    let expected = cpu.render(&scene, 10, 8, 1.);
    assert_eq!(expected.pixel(1, 1), [255, 0, 0, 255]);
    assert_eq!(expected.pixel(7, 2), [0, 0, 0, 0]);
    assert_eq!(expected.pixel(4, 3), [0, 255, 0, 255]);
    let blended = expected.pixel(5, 4);
    assert!(
        (126..=128).contains(&blended[0]) && blended[2] == 128,
        "{blended:?}"
    );
    let mut gpu = match GpuRenderer::headless() {
        Ok(gpu) => gpu,
        Err(e) if std::env::var("HYPERISH_TEST_GPU").as_deref() != Ok("require") => {
            eprintln!("GPU unavailable: {e}");
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let result = gpu.render(&scene, 10, 8, 1.).unwrap();
    for (a, b) in expected.rgba.iter().zip(&result.rgba) {
        assert!(a.abs_diff(*b) <= 1, "CPU {a}, GPU {b}");
    }
    assert_ne!(
        left.frame(Extent {
            width: 3,
            height: 4
        }),
        a
    );
    drop(right);
    let stale = gpu.render(&scene, 10, 8, 1.).unwrap();
    assert_eq!(stale.pixel(1, 1), [0; 4]);
    assert_eq!(stale.pixel(5, 4), [0; 4]);
    assert_eq!(stale.pixel(4, 3), [0, 255, 0, 255]);
}

#[test]
fn ui_mount_scales_input_and_rejects_invalid_cpu_frames() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let out = events.clone();
    let viewport = Viewport::new("engine")
        .cpu(|_| Some(vec![1]))
        .fallback(Color::rgba(12, 34, 56, 255))
        .input(move |p| out.borrow_mut().push(p));
    let h = Headless::new(100., 80., move || {
        viewport.element([
            StyleProp::Width(Length::Px(40.)),
            StyleProp::Height(Length::Px(30.)),
        ])
    });
    let scene = h.ui().frame_at(2.);
    let (key, rect) = scene
        .iter()
        .find_map(|p| {
            if let P::ExternalTexture { key, rect } = p {
                Some((*key, *rect))
            } else {
                None
            }
        })
        .unwrap();
    let slot = serge_core::viewport::Slot::resolve(key).unwrap();
    assert_eq!(
        slot.extent(),
        Extent {
            width: 80,
            height: 60
        }
    );
    h.ui().pointer_down(rect.x + 3., rect.y + 5.);
    h.ui().pointer_up(rect.x + 3., rect.y + 5.);
    assert_eq!(
        *events.borrow(),
        [
            Pointer::Down(Point::new(6., 10.)),
            Pointer::Up(Point::new(6., 10.))
        ]
    );
    let pixels = CpuRenderer::new().render(&scene, 200, 160, 2.);
    assert_eq!(
        pixels.pixel((rect.x * 2.) as u16 + 1, (rect.y * 2.) as u16 + 1),
        [12, 34, 56, 255]
    );
    assert!(slot.unavailable());
    h.ui().invalidate_surfaces();
    assert!(serge_core::viewport::Slot::resolve(key).is_none());
}
