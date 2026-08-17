// recorder/audio.rs – WASAPI loopback capture
//
// Records whatever is coming out of the default playback device, which is what
// "include the audio that's playing" means in practice: game sound, plus
// anything else mixing into the same endpoint.
//
// The encoder's audio clock is driven purely by how many samples it has been
// handed, so the stream has to be continuous. Loopback capture doesn't help
// there — an idle endpoint delivers nothing at all rather than silence — so the
// capture thread pads with silence to keep the sample count tracking wall
// clock. Without that, audio would run progressively ahead of the video.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::Media::Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};

use super::Error;

/// Bytes per sample the encoder is fed. 16-bit PCM is what its stream
/// descriptor is built with.
const BYTES_PER_SAMPLE: usize = 2;

/// Channels handed to the encoder. Anything wider is folded down, because AAC
/// in an MP4 is expected to be stereo here.
const CHANNELS: usize = 2;

/// How often the capture thread drains the endpoint.
const POLL_INTERVAL: Duration = Duration::from_millis(8);

/// Cap on buffered audio, in case video frames stop being delivered and stop
/// draining it. Old audio is dropped rather than growing without bound.
const MAX_BUFFERED: Duration = Duration::from_secs(5);

/// `WAVE_FORMAT_*` tags, spelled out because the generated constants for them
/// sit in a different module to the rest of the WASAPI bindings.
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// Wraps a Win32 failure with what was being attempted.
fn audio_error(context: &str, error: windows::core::Error) -> Error {
    Error::Audio(format!("{context}: {error}"))
}

/// PCM format the encoder should be configured for.
#[derive(Debug, Clone, Copy)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u32,
    pub bits_per_sample: u32,
}

impl AudioFormat {
    /// Bytes per frame, i.e. one sample across all channels.
    fn block_align(&self) -> usize {
        self.channels as usize * BYTES_PER_SAMPLE
    }
}

/// A running loopback capture.
pub struct AudioCapture {
    format: AudioFormat,
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// State the capture thread and the consumer both touch.
struct Shared {
    /// Interleaved 16-bit PCM waiting to be handed to the encoder.
    pcm: Mutex<Vec<u8>>,
    /// Set once video starts keeping frames. Audio captured before this is
    /// discarded so both streams begin at the same instant.
    armed: AtomicBool,
    /// Tells the capture thread to wind up.
    stop: AtomicBool,
}

impl AudioCapture {
    /// Opens the default playback device for loopback capture.
    ///
    /// Returns an error if there's no usable endpoint, which the caller is
    /// expected to treat as "record video only" rather than as fatal.
    pub fn start() -> Result<Self, Error> {
        let shared = Arc::new(Shared {
            pcm: Mutex::new(Vec::new()),
            armed: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        });

        // The device is opened on the capture thread so every COM call happens
        // in the apartment that thread initialises. The format comes back here
        // because the encoder has to be configured to match.
        let (sender, receiver) = std::sync::mpsc::channel::<Result<AudioFormat, Error>>();

        let thread = std::thread::spawn({
            let shared = shared.clone();
            move || capture_thread(&shared, &sender)
        });

        match receiver.recv() {
            Ok(Ok(format)) => Ok(Self {
                format,
                shared,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(Error::Audio("音频线程意外结束".to_string()))
            }
        }
    }

    /// Format the encoder must be configured for.
    pub fn format(&self) -> AudioFormat {
        self.format
    }

    /// Starts keeping audio. Called when the first video frame is encoded so
    /// the two streams share a zero point.
    pub fn arm(&self) {
        self.shared.armed.store(true, Ordering::Release);
    }

    /// Asks the capture thread to wind up.
    ///
    /// Called before the final drain so the tail stops growing while the file
    /// is being closed out. Whatever is already buffered stays available.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Release);
    }

    /// Takes everything captured so far, or `None` if there's nothing yet.
    pub fn take_pcm(&self) -> Option<Vec<u8>> {
        let mut buffer = self.shared.pcm.lock().ok()?;
        if buffer.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut buffer))
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Capture thread
// ---------------------------------------------------------------------------

fn capture_thread(shared: &Shared, sender: &std::sync::mpsc::Sender<Result<AudioFormat, Error>>) {
    // MTA, because nothing here needs a message pump.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if com.is_err() {
        let _ = sender.send(Err(Error::Audio("COM 初始化失败".to_string())));
        return;
    }

    let result = run_capture(shared, sender);

    if let Err(error) = result {
        // If setup already reported a format, the consumer has moved on and
        // there is nobody left to tell; losing audio mid-recording just means
        // the tail is padded with silence.
        let _ = sender.send(Err(error));
    }

    unsafe { CoUninitialize() };
}

fn run_capture(
    shared: &Shared,
    sender: &std::sync::mpsc::Sender<Result<AudioFormat, Error>>,
) -> Result<(), Error> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|error| audio_error("枚举音频设备失败", error))?;

        // eRender + LOOPBACK is what turns a playback endpoint into a capture
        // source; eCapture would pick up the microphone instead.
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|error| audio_error("未找到默认播放设备", error))?;

        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|error| audio_error("激活音频客户端失败", error))?;

        let mix_format = client
            .GetMixFormat()
            .map_err(|error| audio_error("读取音频格式失败", error))?;
        let source = SourceFormat::from_wave_format(mix_format)?;

        // A 200ms endpoint buffer leaves plenty of slack for the poll interval.
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                200 * 10_000,
                0,
                mix_format,
                None,
            )
            .map_err(|error| audio_error("初始化音频采集失败", error))?;

        let capture: IAudioCaptureClient = client
            .GetService()
            .map_err(|error| audio_error("获取音频采集接口失败", error))?;

        let format = AudioFormat {
            sample_rate: source.sample_rate,
            channels: CHANNELS as u32,
            bits_per_sample: (BYTES_PER_SAMPLE * 8) as u32,
        };

        client
            .Start()
            .map_err(|error| audio_error("启动音频采集失败", error))?;

        // Setup succeeded; the encoder can now be built against this format.
        let _ = sender.send(Ok(format));

        let block_align = format.block_align();
        let max_buffered =
            (format.sample_rate as usize * MAX_BUFFERED.as_secs() as usize) * block_align;

        let mut armed_at: Option<Instant> = None;
        let mut frames_produced: u64 = 0;
        let mut staging: Vec<u8> = Vec::new();

        while !shared.stop.load(Ordering::Acquire) {
            let is_armed = shared.armed.load(Ordering::Acquire);
            if is_armed && armed_at.is_none() {
                armed_at = Some(Instant::now());
            }

            staging.clear();
            drain_endpoint(&capture, &source, is_armed, &mut staging)?;

            if let Some(armed_at) = armed_at {
                frames_produced += (staging.len() / block_align) as u64;

                // Loopback goes quiet rather than producing silence when
                // nothing is playing, so top up to wall clock. Without this the
                // audio track would end up shorter than the video and drift out
                // of sync as it went.
                let expected =
                    (armed_at.elapsed().as_secs_f64() * f64::from(format.sample_rate)) as u64;
                if let Some(missing) = expected.checked_sub(frames_produced) {
                    // A frame or two of jitter isn't worth patching; only fill a
                    // real gap.
                    if missing > u64::from(format.sample_rate) / 100 {
                        staging.resize(staging.len() + missing as usize * block_align, 0);
                        frames_produced += missing;
                    }
                }

                if !staging.is_empty() {
                    if let Ok(mut buffer) = shared.pcm.lock() {
                        buffer.extend_from_slice(&staging);

                        // Nothing is draining us; keep the newest audio.
                        if buffer.len() > max_buffered {
                            let excess = buffer.len() - max_buffered;
                            buffer.drain(..excess);
                        }
                    }
                }
            }

            std::thread::sleep(POLL_INTERVAL);
        }

        let _ = client.Stop();
    }

    Ok(())
}

/// Pulls every queued packet out of the endpoint, converting as it goes.
unsafe fn drain_endpoint(
    capture: &IAudioCaptureClient,
    source: &SourceFormat,
    keep: bool,
    out: &mut Vec<u8>,
) -> Result<(), Error> {
    loop {
        let packet = capture
            .GetNextPacketSize()
            .map_err(|error| Error::Audio(format!("读取音频包失败: {error}")))?;
        if packet == 0 {
            return Ok(());
        }

        let mut data = std::ptr::null_mut();
        let mut frames = 0u32;
        let mut flags = 0u32;

        capture
            .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
            .map_err(|error| Error::Audio(format!("获取音频缓冲失败: {error}")))?;

        if keep && frames > 0 {
            let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
            if silent || data.is_null() {
                out.resize(out.len() + frames as usize * CHANNELS * BYTES_PER_SAMPLE, 0);
            } else {
                let bytes = std::slice::from_raw_parts(
                    data,
                    frames as usize * source.block_align as usize,
                );
                source.convert_into(bytes, out);
            }
        }

        capture
            .ReleaseBuffer(frames)
            .map_err(|error| Error::Audio(format!("释放音频缓冲失败: {error}")))?;
    }
}

// ---------------------------------------------------------------------------
// Format conversion
// ---------------------------------------------------------------------------

/// What the endpoint hands us, and how to turn it into 16-bit stereo PCM.
struct SourceFormat {
    sample_rate: u32,
    channels: usize,
    block_align: u16,
    kind: SampleKind,
}

enum SampleKind {
    /// 32-bit IEEE float, which is what a shared-mode mix format almost always
    /// is on current Windows.
    F32,
    /// Integer PCM of the given width in bytes.
    Int(usize),
}

impl SourceFormat {
    unsafe fn from_wave_format(format: *const WAVEFORMATEX) -> Result<Self, Error> {
        let wave = &*format;

        // Shared-mode mix formats are almost always extensible float32, but the
        // plain tag shows up on some drivers.
        let is_float = if wave.wFormatTag == WAVE_FORMAT_EXTENSIBLE {
            // WAVEFORMATEXTENSIBLE is byte-packed, so the GUID has to be read
            // out unaligned rather than borrowed.
            let extensible = format as *const WAVEFORMATEXTENSIBLE;
            std::ptr::addr_of!((*extensible).SubFormat).read_unaligned()
                == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
        } else {
            wave.wFormatTag == WAVE_FORMAT_IEEE_FLOAT
        };

        let bytes = (wave.wBitsPerSample / 8) as usize;
        if bytes == 0 || wave.nChannels == 0 {
            return Err(Error::Audio("音频格式无效".to_string()));
        }

        let kind = if is_float {
            if bytes != 4 {
                return Err(Error::Audio(format!("不支持的浮点位深: {bytes}")));
            }
            SampleKind::F32
        } else {
            SampleKind::Int(bytes)
        };

        Ok(Self {
            sample_rate: wave.nSamplesPerSec,
            channels: wave.nChannels as usize,
            block_align: wave.nBlockAlign,
            kind,
        })
    }

    /// Converts interleaved source frames into interleaved 16-bit stereo.
    ///
    /// Extra channels are dropped rather than mixed; a surround endpoint's
    /// front pair is the sensible stereo approximation, and mixing would need
    /// per-layout coefficients to avoid sounding wrong.
    fn convert_into(&self, bytes: &[u8], out: &mut Vec<u8>) {
        let frame_bytes = self.block_align as usize;
        if frame_bytes == 0 {
            return;
        }

        for frame in bytes.chunks_exact(frame_bytes) {
            for channel in 0..CHANNELS {
                // Mono sources feed both output channels.
                let index = channel.min(self.channels - 1);
                let sample = self.sample_at(frame, index);
                out.extend_from_slice(&sample.to_le_bytes());
            }
        }
    }

    /// Reads one channel of a frame and scales it to `i16`.
    fn sample_at(&self, frame: &[u8], channel: usize) -> i16 {
        match self.kind {
            SampleKind::F32 => {
                let at = channel * 4;
                let Some(raw) = frame.get(at..at + 4) else {
                    return 0;
                };
                let value = f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
                // Clamp before scaling: shared-mode mixes can exceed unity.
                (value.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
            }
            SampleKind::Int(width) => {
                let at = channel * width;
                let Some(raw) = frame.get(at..at + width) else {
                    return 0;
                };
                match width {
                    1 => (i16::from(raw[0]) - 128) << 8,
                    2 => i16::from_le_bytes([raw[0], raw[1]]),
                    // Keep the top two bytes of 24- and 32-bit samples.
                    3 => i16::from_le_bytes([raw[1], raw[2]]),
                    _ => i16::from_le_bytes([raw[width - 2], raw[width - 1]]),
                }
            }
        }
    }
}
