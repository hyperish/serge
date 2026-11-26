use block2::RcBlock;
use hyve::SharedRingBuf;
use objc2::{rc::Retained, runtime::Bool};
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
use objc2_avf_audio::{
    AVAudioCommonFormat, AVAudioEngine, AVAudioInputNode, AVAudioPCMBuffer, AVAudioTime,
};
use objc2_foundation::{NSBundle, NSString};
use std::{io, ptr::NonNull, sync::mpsc};

/// Microphone input on Apple platforms. The ring holds the first input channel as signed 16-bit PCM.
pub struct Microphone {
    engine: Retained<AVAudioEngine>,
    input: Retained<AVAudioInputNode>,
    ring: SharedRingBuf<i16>,
    sample_rate: u32,
}

impl Microphone {
    /// Asks the OS for permission using the app's NSMicrophoneUsageDescription.
    pub fn request_permission() -> io::Result<mpsc::Receiver<bool>> {
        let usage = NSBundle::mainBundle()
            .objectForInfoDictionaryKey(&NSString::from_str("NSMicrophoneUsageDescription"))
            .and_then(|v| v.downcast::<NSString>().ok());
        if usage.is_none_or(|s| s.is_empty()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "set NSMicrophoneUsageDescription",
            ));
        }
        let (tx, rx) = mpsc::channel();
        let reply = RcBlock::new(move |granted: Bool| {
            let _ = tx.send(granted.as_bool());
        });
        // SAFETY: the usage description is present. AVFoundation copies the block.
        #[allow(unsafe_code)]
        unsafe {
            let audio =
                AVMediaTypeAudio.ok_or_else(|| io::Error::other("audio capture unavailable"))?;
            AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &reply);
        }
        Ok(rx)
    }

    /// Activates a play-and-record session for apps that do not manage their own iOS audio session.
    /// The session is shared with playback and remains under application control.
    #[cfg(target_os = "ios")]
    pub fn configure_recording_session() -> io::Result<()> {
        use objc2_avf_audio::{AVAudioSession, AVAudioSessionCategoryPlayAndRecord};
        // SAFETY: the shared AVAudioSession accepts this public system category.
        #[allow(unsafe_code)]
        unsafe {
            let session = AVAudioSession::sharedInstance();
            let category = AVAudioSessionCategoryPlayAndRecord
                .ok_or_else(|| io::Error::other("recording session unavailable"))?;
            session
                .setCategory_error(category)
                .map_err(|e| io::Error::other(e.to_string()))?;
            session
                .setActive_error(true)
                .map_err(|e| io::Error::other(e.to_string()))
        }
    }

    /// Opens an authorized microphone. On iOS configure an input-capable audio session first.
    pub fn open(capacity: usize) -> io::Result<Self> {
        if !(1..=1_048_576).contains(&capacity) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid microphone ring capacity",
            ));
        }
        // SAFETY: we own the engine and its only tap. The OS owns callback buffers
        // for the duration of a call. The callback captures its own ring handle.
        #[allow(unsafe_code)]
        unsafe {
            let audio =
                AVMediaTypeAudio.ok_or_else(|| io::Error::other("audio capture unavailable"))?;
            if AVCaptureDevice::authorizationStatusForMediaType(audio)
                != AVAuthorizationStatus::Authorized
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "microphone permission required",
                ));
            }
            let engine = AVAudioEngine::new();
            let input = engine.inputNode();
            let format = input.outputFormatForBus(0);
            let hz = format.sampleRate();
            if format.channelCount() == 0
                || !(1.0..=f64::from(u32::MAX)).contains(&hz)
                || format.commonFormat() != AVAudioCommonFormat::PCMFormatFloat32
            {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "no active floating-point input route",
                ));
            }
            let ring = SharedRingBuf::new(capacity);
            let producer = ring.clone();
            let callback = RcBlock::new(
                move |buffer: NonNull<AVAudioPCMBuffer>, _: NonNull<AVAudioTime>| {
                    copy_first_channel(buffer.as_ref(), &producer)
                },
            );
            input.installTapOnBus_bufferSize_format_block(
                0,
                (hz * 0.1) as u32,
                None,
                RcBlock::as_ptr(&callback),
            );
            if let Err(error) = engine.startAndReturnError() {
                input.removeTapOnBus(0);
                return Err(io::Error::other(error.to_string()));
            }
            Ok(Self {
                engine,
                input,
                ring,
                sample_rate: hz.round() as u32,
            })
        }
    }
    /// Input sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    /// The most recent single-channel PCM samples.
    pub fn samples(&self) -> &SharedRingBuf<i16> {
        &self.ring
    }
}

fn copy_first_channel(buffer: &AVAudioPCMBuffer, ring: &SharedRingBuf<i16>) {
    // SAFETY: floatChannelData is null for other formats. AVFAudio guarantees
    // frameLength valid samples in each channel, spaced stride elements apart.
    #[allow(unsafe_code)]
    unsafe {
        let channels = buffer.floatChannelData();
        if channels.is_null() {
            return;
        }
        let first = *channels;
        let stride = buffer.stride();
        for i in 0..buffer.frameLength() as usize {
            let sample = first.as_ptr().add(i * stride).read();
            ring.push((sample * 32768.0).round() as i16);
        }
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        // SAFETY: only a successfully started, owned engine reaches Drop.
        #[allow(unsafe_code)]
        unsafe {
            self.engine.stop();
            self.input.removeTapOnBus(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::AnyThread;
    use objc2_avf_audio::AVAudioFormat;

    #[test]
    fn native_pcm_buffers_preserve_the_first_channel_across_ring_wrap() {
        // SAFETY: a locally owned buffer provides exactly five writable frames.
        #[allow(unsafe_code)]
        unsafe {
            for interleaved in [false, true] {
                let format = AVAudioFormat::initWithCommonFormat_sampleRate_channels_interleaved(
                    AVAudioFormat::alloc(),
                    AVAudioCommonFormat::PCMFormatFloat32,
                    48_000.0,
                    2,
                    interleaved,
                )
                .unwrap();
                let samples = AVAudioPCMBuffer::initWithPCMFormat_frameCapacity(
                    AVAudioPCMBuffer::alloc(),
                    &format,
                    5,
                )
                .unwrap();
                samples.setFrameLength(5);
                let channels = samples.floatChannelData();
                for (i, value) in [-1.0, -0.5, 0.0, 0.5, 1.0].into_iter().enumerate() {
                    (*channels).as_ptr().add(i * samples.stride()).write(value);
                    (*channels.add(1))
                        .as_ptr()
                        .add(i * samples.stride())
                        .write(-1.0);
                }
                let ring = SharedRingBuf::new(4);
                copy_first_channel(&samples, &ring);
                let mut out = [0i16; 4];
                assert_eq!(ring.read_latest_snapshot(&mut out), 4);
                assert_eq!(out, [-16_384, 0, 16_384, 32_767]);
            }
        }
    }
}
