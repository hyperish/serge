//! Zero-copy Camera2 frames produced into an external GLES texture.
use super::{AndroidSystem, services::class};
use jni::{
    jni_sig, jni_str,
    objects::{JFloatArray, JIntArray, JObject, JObjectArray, JString, JValue},
    refs::Global,
};
use std::{io, marker::PhantomData, rc::Rc};

/// Metadata for the latest image bound to the external texture. Pixels stay in the OS buffers.
pub struct CameraFrame {
    /// Producer timestamp, in nanoseconds.
    pub timestamp_ns: i64,
    /// Column-major transform from nominal UVs to the producer's crop and orientation.
    pub transform: [f32; 16],
}

/// Camera2 output bound to an app-owned GL_TEXTURE_EXTERNAL_OES name.
/// Acquire frames on the same current GLES context. Drop before deleting the texture.
pub struct CameraTexture {
    system: AndroidSystem,
    stream: Global<JObject<'static>>,
    matrix: Global<JFloatArray<'static>>,
    _render_thread: PhantomData<Rc<()>>,
}

impl CameraTexture {
    /// Camera identifiers visible to this application.
    pub fn cameras(system: &AndroidSystem) -> io::Result<Vec<String>> {
        system.access(|env, a| {
            let api = class(env, a)?;
            let list = env
                .call_static_method(
                    api,
                    jni_str!("cameraIds"),
                    jni_sig!("(Landroid/app/Activity;)[Ljava/lang/String;"),
                    &[JValue::Object(a)],
                )?
                .l()?;
            let list = env.cast_local::<JObjectArray<JString>>(list)?;
            let n = list.len(env)?;
            (0..n)
                .map(|i| Ok(list.get_element(env, i)?.to_string()))
                .collect()
        })
    }
    /// The available native texture sizes for one camera.
    pub fn sizes(system: &AndroidSystem, id: &str) -> io::Result<Vec<(u32, u32)>> {
        system.access(|env, a| {
            let api = class(env, a)?;
            let id = env.new_string(id)?;
            let sizes = env
                .call_static_method(
                    api,
                    jni_str!("cameraSizes"),
                    jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)[I"),
                    &[JValue::Object(a), JValue::Object(&id)],
                )?
                .l()?;
            let sizes = env.cast_local::<JIntArray>(sizes)?;
            let mut values = vec![0; sizes.len(env)?];
            sizes.get_region(env, 0, &mut values)?;
            Ok(values
                .as_chunks::<2>()
                .0
                .iter()
                .map(|wh| (wh[0] as u32, wh[1] as u32))
                .collect())
        })
    }
    /// Start an authorized stream using a texture name allocated by the caller's current GLES context.
    pub fn open(
        system: &AndroidSystem,
        camera: &str,
        size: (u32, u32),
        texture: u32,
    ) -> io::Result<Self> {
        let (stream,matrix)=system.access(|env,a| {
            let api=class(env,a)?;
            let id=env.new_string(camera)?;
            let matrix=JFloatArray::new(env,16)?;
            let matrix=env.new_global_ref(matrix)?;
            let stream=env.call_static_method(api,jni_str!("camera"),jni_sig!("(Landroid/app/Activity;Ljava/lang/String;III)Lorg/hyperish/serge/CameraTexture;"),
                &[JValue::Object(a),JValue::Object(&id),JValue::Int(size.0 as i32),JValue::Int(size.1 as i32),JValue::Int(texture as i32)])?.l()?;
            Ok((env.new_global_ref(stream)?,matrix))
        })?;
        Ok(Self {
            system: system.clone(),
            stream,
            matrix,
            _render_thread: PhantomData,
        })
    }
    /// Bind the newest producer buffer, returning None when there is no new image.
    pub fn try_update(&self) -> io::Result<Option<CameraFrame>> {
        self.system.access(|env, _| {
            let timestamp_ns = env
                .call_method(
                    &self.stream,
                    jni_str!("update"),
                    jni_sig!("([F)J"),
                    &[JValue::Object(&self.matrix)],
                )?
                .j()?;
            if timestamp_ns < 0 {
                return Ok(None);
            }
            let mut transform = [0.; 16];
            self.matrix.get_region(env, 0, &mut transform)?;
            Ok(Some(CameraFrame {
                timestamp_ns,
                transform,
            }))
        })
    }
}
impl Drop for CameraTexture {
    fn drop(&mut self) {
        let _ = self.system.access(|env, _| {
            env.call_method(&self.stream, jni_str!("close"), jni_sig!("()V"), &[])?;
            Ok(())
        });
    }
}
