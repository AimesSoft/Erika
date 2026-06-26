#[cfg(target_os = "windows")]
pub mod wasapi {
    use std::slice;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    };
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    use crossbeam_channel::{Receiver, Sender};
    use thiserror::Error;

    use crate::audio::{
        AudioClockSnapshot, AudioOutputBackend, AudioOutputState, AudioPushResult, AudioReadResult,
        AudioRingBuffer, AudioRingBufferConfig, AudioRingBufferStats, apply_volume,
        normalize_volume,
    };
    use crate::ffmpeg::{PcmAudioFrame, PcmFormat, PcmSampleFormat};

    use ::windows::Win32::Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioClient, IAudioRenderClient,
        IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, eConsole, eRender,
    };
    use ::windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };

    const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
    const WASAPI_BUFFER_DURATION_HNS: i64 = 1_000_000;
    const RENDER_POLL_INTERVAL: Duration = Duration::from_millis(5);

    #[derive(Debug, Error)]
    pub enum WasapiAudioOutputError {
        #[error("audio error: {0}")]
        Audio(#[from] crate::audio::AudioError),
        #[error("WASAPI {operation} failed: {message}")]
        Wasapi {
            operation: &'static str,
            message: String,
        },
        #[error("WASAPI output buffer is not configured")]
        NotConfigured,
        #[error("WASAPI output lock was poisoned")]
        LockPoisoned,
        #[error("WASAPI render thread stopped before initialization")]
        ThreadStopped,
        #[error("unsupported PCM format for WASAPI: {0:?}")]
        UnsupportedFormat(PcmSampleFormat),
        #[error("invalid WASAPI format: sample_rate={sample_rate}, channels={channels}")]
        InvalidFormat { sample_rate: u32, channels: u32 },
    }

    pub type Result<T> = std::result::Result<T, WasapiAudioOutputError>;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct WasapiAudioOutputConfig {
        pub ring_buffer: AudioRingBufferConfig,
    }

    impl Default for WasapiAudioOutputConfig {
        fn default() -> Self {
            Self {
                ring_buffer: AudioRingBufferConfig {
                    capacity_frames: 192_000,
                    drop_oldest_on_overflow: true,
                },
            }
        }
    }

    pub struct WasapiAudioOutput {
        state: AudioOutputState,
        format: Option<PcmFormat>,
        render_thread: Option<WasapiRenderThread>,
        buffer: Arc<Mutex<AudioRingBuffer>>,
        volume: Arc<AtomicU32>,
    }

    impl WasapiAudioOutput {
        pub fn new(config: WasapiAudioOutputConfig) -> Self {
            Self {
                state: AudioOutputState::Stopped,
                format: None,
                render_thread: None,
                buffer: Arc::new(Mutex::new(AudioRingBuffer::new(config.ring_buffer))),
                volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            }
        }

        pub fn configure(&mut self, format: PcmFormat) -> Result<()> {
            stop_render_thread(&mut self.render_thread);
            configure_buffer(&self.buffer, format)?;
            let render_thread = WasapiRenderThread::spawn(
                format,
                Arc::clone(&self.buffer),
                Arc::clone(&self.volume),
            )?;
            self.render_thread = Some(render_thread);
            self.format = Some(format);
            self.state = AudioOutputState::Stopped;
            Ok(())
        }

        pub fn set_volume(&mut self, volume: f32) {
            self.volume
                .store(normalize_volume(volume).to_bits(), Ordering::Relaxed);
        }

        pub fn volume(&self) -> f32 {
            f32::from_bits(self.volume.load(Ordering::Relaxed))
        }

        pub fn start(&mut self) -> Result<()> {
            let render_thread = self
                .render_thread
                .as_ref()
                .ok_or(WasapiAudioOutputError::NotConfigured)?;
            render_thread.send(WasapiRenderCommand::Start)?;
            self.state = AudioOutputState::Playing;
            Ok(())
        }

        pub fn pause(&mut self) -> Result<()> {
            if let Some(render_thread) = &self.render_thread {
                render_thread.send(WasapiRenderCommand::Pause)?;
            }
            self.state = AudioOutputState::Paused;
            Ok(())
        }

        pub fn stop(&mut self) -> Result<()> {
            if let Some(render_thread) = &self.render_thread {
                render_thread.send(WasapiRenderCommand::Stop)?;
            }
            clear_buffer(&self.buffer)?;
            self.state = AudioOutputState::Stopped;
            Ok(())
        }

        pub fn push(&mut self, frame: PcmAudioFrame) -> Result<AudioPushResult> {
            let mut buffer = self
                .buffer
                .lock()
                .map_err(|_| WasapiAudioOutputError::LockPoisoned)?;
            Ok(buffer.push_frame(frame)?)
        }

        pub fn read_for_test(&mut self, output: &mut [f32]) -> Result<AudioReadResult> {
            let mut buffer = self
                .buffer
                .lock()
                .map_err(|_| WasapiAudioOutputError::LockPoisoned)?;
            let result = buffer.read_interleaved(output)?;
            apply_volume(output, self.volume());
            Ok(result)
        }

        pub fn state(&self) -> AudioOutputState {
            self.state
        }

        pub fn stats(&self) -> Result<AudioRingBufferStats> {
            let buffer = self
                .buffer
                .lock()
                .map_err(|_| WasapiAudioOutputError::LockPoisoned)?;
            Ok(buffer.stats())
        }

        pub fn clock_snapshot(&self) -> Result<AudioClockSnapshot> {
            let buffer = self
                .buffer
                .lock()
                .map_err(|_| WasapiAudioOutputError::LockPoisoned)?;
            Ok(buffer.clock_snapshot())
        }

        pub fn format(&self) -> Option<PcmFormat> {
            self.format
        }
    }

    impl Default for WasapiAudioOutput {
        fn default() -> Self {
            Self::new(WasapiAudioOutputConfig::default())
        }
    }

    impl Drop for WasapiAudioOutput {
        fn drop(&mut self) {
            stop_render_thread(&mut self.render_thread);
        }
    }

    impl AudioOutputBackend for WasapiAudioOutput {
        fn configure(&mut self, format: PcmFormat) -> crate::audio::Result<()> {
            WasapiAudioOutput::configure(self, format)
                .map_err(|error| crate::audio::AudioError::Backend(error.to_string()))
        }

        fn start(&mut self) -> crate::audio::Result<()> {
            WasapiAudioOutput::start(self)
                .map_err(|error| crate::audio::AudioError::Backend(error.to_string()))
        }

        fn pause(&mut self) -> crate::audio::Result<()> {
            WasapiAudioOutput::pause(self)
                .map_err(|error| crate::audio::AudioError::Backend(error.to_string()))
        }

        fn stop(&mut self) -> crate::audio::Result<()> {
            WasapiAudioOutput::stop(self)
                .map_err(|error| crate::audio::AudioError::Backend(error.to_string()))
        }

        fn set_volume(&mut self, volume: f32) {
            WasapiAudioOutput::set_volume(self, volume);
        }

        fn volume(&self) -> f32 {
            WasapiAudioOutput::volume(self)
        }

        fn push(&mut self, frame: PcmAudioFrame) -> crate::audio::Result<AudioPushResult> {
            WasapiAudioOutput::push(self, frame)
                .map_err(|error| crate::audio::AudioError::Backend(error.to_string()))
        }

        fn state(&self) -> AudioOutputState {
            self.state
        }

        fn stats(&self) -> AudioRingBufferStats {
            self.stats().unwrap_or_default()
        }

        fn clock_snapshot(&self) -> Option<AudioClockSnapshot> {
            self.clock_snapshot().ok()
        }
    }

    struct WasapiRenderThread {
        commands: Sender<WasapiRenderCommand>,
        worker: Option<JoinHandle<()>>,
    }

    impl WasapiRenderThread {
        fn spawn(
            format: PcmFormat,
            buffer: Arc<Mutex<AudioRingBuffer>>,
            volume: Arc<AtomicU32>,
        ) -> Result<Self> {
            let (commands_tx, commands_rx) = crossbeam_channel::unbounded();
            let (init_tx, init_rx) = crossbeam_channel::bounded(1);
            let worker = thread::Builder::new()
                .name("erika-wasapi-render".to_string())
                .spawn(move || {
                    let result = run_render_thread(format, buffer, volume, commands_rx, init_tx);
                    if let Err(error) = result {
                        eprintln!("erika WASAPI render thread stopped: {error}");
                    }
                })
                .map_err(|error| WasapiAudioOutputError::Wasapi {
                    operation: "thread spawn",
                    message: error.to_string(),
                })?;

            match init_rx
                .recv()
                .map_err(|_| WasapiAudioOutputError::ThreadStopped)?
            {
                Ok(()) => Ok(Self {
                    commands: commands_tx,
                    worker: Some(worker),
                }),
                Err(error) => {
                    let _ = commands_tx.send(WasapiRenderCommand::Shutdown);
                    let _ = worker.join();
                    Err(error)
                }
            }
        }

        fn send(&self, command: WasapiRenderCommand) -> Result<()> {
            self.commands
                .send(command)
                .map_err(|_| WasapiAudioOutputError::ThreadStopped)
        }
    }

    impl Drop for WasapiRenderThread {
        fn drop(&mut self) {
            let _ = self.commands.send(WasapiRenderCommand::Shutdown);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum WasapiRenderCommand {
        Start,
        Pause,
        Stop,
        Shutdown,
    }

    fn run_render_thread(
        format: PcmFormat,
        buffer: Arc<Mutex<AudioRingBuffer>>,
        volume: Arc<AtomicU32>,
        commands: Receiver<WasapiRenderCommand>,
        init_tx: Sender<Result<()>>,
    ) -> Result<()> {
        let result = initialize_render_client(format);
        let mut render_state = match result {
            Ok(state) => {
                let _ = init_tx.send(Ok(()));
                state
            }
            Err(error) => {
                let _ = init_tx.send(Err(error));
                return Ok(());
            }
        };

        let mut playing = false;
        loop {
            for command in commands.try_iter() {
                match command {
                    WasapiRenderCommand::Start => {
                        if !playing {
                            render_state.client_start()?;
                            playing = true;
                        }
                    }
                    WasapiRenderCommand::Pause => {
                        if playing {
                            render_state.client_stop()?;
                            playing = false;
                        }
                    }
                    WasapiRenderCommand::Stop => {
                        if playing {
                            render_state.client_stop()?;
                            playing = false;
                        }
                        render_state.client_reset()?;
                    }
                    WasapiRenderCommand::Shutdown => {
                        if playing {
                            let _ = render_state.client_stop();
                        }
                        return Ok(());
                    }
                }
            }

            if playing {
                render_state.render_available_frames(&buffer, &volume)?;
            }
            thread::sleep(RENDER_POLL_INTERVAL);
        }
    }

    struct WasapiRenderState {
        _com: ComApartment,
        client: IAudioClient,
        render_client: IAudioRenderClient,
        buffer_frames: u32,
        channels: usize,
    }

    impl WasapiRenderState {
        fn client_start(&self) -> Result<()> {
            unsafe { self.client.Start() }
                .map_err(|error| wasapi_error("IAudioClient::Start", error))
        }

        fn client_stop(&self) -> Result<()> {
            unsafe { self.client.Stop() }.map_err(|error| wasapi_error("IAudioClient::Stop", error))
        }

        fn client_reset(&self) -> Result<()> {
            unsafe { self.client.Reset() }
                .map_err(|error| wasapi_error("IAudioClient::Reset", error))
        }

        fn render_available_frames(
            &mut self,
            buffer: &Arc<Mutex<AudioRingBuffer>>,
            volume: &Arc<AtomicU32>,
        ) -> Result<()> {
            let padding = unsafe { self.client.GetCurrentPadding() }
                .map_err(|error| wasapi_error("IAudioClient::GetCurrentPadding", error))?;
            let frames = self.buffer_frames.saturating_sub(padding);
            if frames == 0 {
                return Ok(());
            }

            let data = unsafe { self.render_client.GetBuffer(frames) }
                .map_err(|error| wasapi_error("IAudioRenderClient::GetBuffer", error))?;
            let sample_count = frames as usize * self.channels;
            let mut silent = false;
            let result = (|| {
                let output = unsafe { slice::from_raw_parts_mut(data.cast::<f32>(), sample_count) };
                let read_result = {
                    let mut buffer = buffer
                        .lock()
                        .map_err(|_| WasapiAudioOutputError::LockPoisoned)?;
                    buffer.read_interleaved(output)?
                };
                apply_volume(output, f32::from_bits(volume.load(Ordering::Relaxed)));
                silent = read_result.frames == 0;
                Ok(())
            })();

            let release_flags = if silent {
                AUDCLNT_BUFFERFLAGS_SILENT.0 as u32
            } else {
                0
            };
            let release_result = unsafe { self.render_client.ReleaseBuffer(frames, release_flags) }
                .map_err(|error| wasapi_error("IAudioRenderClient::ReleaseBuffer", error));
            result.and(release_result)
        }
    }

    fn initialize_render_client(format: PcmFormat) -> Result<WasapiRenderState> {
        let _com = ComApartment::new()?;
        let wave_format = wave_format(format)?;
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(|error| wasapi_error("CoCreateInstance(MMDeviceEnumerator)", error))?;
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
            .map_err(|error| wasapi_error("IMMDeviceEnumerator::GetDefaultAudioEndpoint", error))?;
        let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }
            .map_err(|error| wasapi_error("IMMDevice::Activate(IAudioClient)", error))?;
        unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                WASAPI_BUFFER_DURATION_HNS,
                0,
                &wave_format,
                None,
            )
        }
        .map_err(|error| wasapi_error("IAudioClient::Initialize", error))?;
        let buffer_frames = unsafe { client.GetBufferSize() }
            .map_err(|error| wasapi_error("IAudioClient::GetBufferSize", error))?;
        let render_client: IAudioRenderClient = unsafe { client.GetService() }
            .map_err(|error| wasapi_error("IAudioClient::GetService(IAudioRenderClient)", error))?;

        Ok(WasapiRenderState {
            _com,
            client,
            render_client,
            buffer_frames,
            channels: format.channels as usize,
        })
    }

    fn wave_format(format: PcmFormat) -> Result<WAVEFORMATEX> {
        match format.sample_format {
            PcmSampleFormat::F32Interleaved => {}
        }
        if format.sample_rate == 0 || format.channels == 0 || format.channels > u16::MAX as u32 {
            return Err(WasapiAudioOutputError::InvalidFormat {
                sample_rate: format.sample_rate,
                channels: format.channels,
            });
        }
        let channels = format.channels as u16;
        let block_align = channels
            .checked_mul(std::mem::size_of::<f32>() as u16)
            .ok_or(WasapiAudioOutputError::InvalidFormat {
                sample_rate: format.sample_rate,
                channels: format.channels,
            })?;
        Ok(WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
            nChannels: channels,
            nSamplesPerSec: format.sample_rate,
            nAvgBytesPerSec: format.sample_rate * block_align as u32,
            nBlockAlign: block_align,
            wBitsPerSample: 32,
            cbSize: 0,
        })
    }

    struct ComApartment;

    impl ComApartment {
        fn new() -> Result<Self> {
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .map_err(|error| wasapi_error("CoInitializeEx", error))?;
            Ok(Self)
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }

    fn configure_buffer(buffer: &Arc<Mutex<AudioRingBuffer>>, format: PcmFormat) -> Result<()> {
        let mut buffer = buffer
            .lock()
            .map_err(|_| WasapiAudioOutputError::LockPoisoned)?;
        Ok(buffer.configure(format)?)
    }

    fn clear_buffer(buffer: &Arc<Mutex<AudioRingBuffer>>) -> Result<()> {
        let mut buffer = buffer
            .lock()
            .map_err(|_| WasapiAudioOutputError::LockPoisoned)?;
        buffer.clear();
        Ok(())
    }

    fn stop_render_thread(render_thread: &mut Option<WasapiRenderThread>) {
        if let Some(render_thread) = render_thread.take() {
            drop(render_thread);
        }
    }

    fn wasapi_error(
        operation: &'static str,
        error: ::windows::core::Error,
    ) -> WasapiAudioOutputError {
        WasapiAudioOutputError::Wasapi {
            operation,
            message: error.to_string(),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn stereo_format() -> PcmFormat {
            PcmFormat {
                sample_rate: 48_000,
                channels: 2,
                sample_format: PcmSampleFormat::F32Interleaved,
            }
        }

        #[test]
        fn wave_format_uses_interleaved_float32_pcm() {
            let wave = wave_format(stereo_format()).unwrap();
            let tag = wave.wFormatTag;
            let channels = wave.nChannels;
            let sample_rate = wave.nSamplesPerSec;
            let block_align = wave.nBlockAlign;
            let avg_bytes_per_sec = wave.nAvgBytesPerSec;
            let bits_per_sample = wave.wBitsPerSample;

            assert_eq!(tag, WAVE_FORMAT_IEEE_FLOAT);
            assert_eq!(channels, 2);
            assert_eq!(sample_rate, 48_000);
            assert_eq!(block_align, 8);
            assert_eq!(avg_bytes_per_sec, 384_000);
            assert_eq!(bits_per_sample, 32);
        }

        #[test]
        fn volume_is_clamped_and_applied_to_pcm_samples() {
            let mut output = WasapiAudioOutput::default();

            assert_eq!(output.volume(), 1.0);
            output.set_volume(0.25);
            assert_eq!(output.volume(), 0.25);
            output.set_volume(-1.0);
            assert_eq!(output.volume(), 0.0);
            output.set_volume(f32::NAN);
            assert_eq!(output.volume(), 1.0);

            let mut samples = [1.0, -0.5, 0.25, 0.0];
            apply_volume(&mut samples, 0.5);
            assert_eq!(samples, [0.5, -0.25, 0.125, 0.0]);
        }
    }
}
