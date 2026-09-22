//! Microphone ownership stays on a worker thread. Stopping drains delivered
//! samples before finalizing the WAV; dropping a recorder also stops capture.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;

pub(super) struct Audio {
    pub file: NamedTempFile,
    pub duration: Duration,
    pub warning: Option<String>,
}

pub(super) struct Recorder {
    stop: Arc<AtomicBool>,
    pub ready: Arc<AtomicBool>,
    pub level: Arc<AtomicU32>,
    result: mpsc::Receiver<Result<Audio, String>>,
}

impl Recorder {
    pub fn start() -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(AtomicBool::new(false));
        let level = Arc::new(AtomicU32::new(0));
        let (tx, result) = mpsc::channel();
        let (stopped, started, meter) = (stop.clone(), ready.clone(), level.clone());
        std::thread::Builder::new()
            .name("voice-capture".into())
            .spawn(move || {
                let result = capture(stopped, started, meter).map_err(|e| e.to_string());
                let _ = tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            stop,
            ready,
            level,
            result,
        })
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    pub fn poll(&self) -> Option<Result<Audio, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Microphone worker stopped unexpectedly".into())),
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop();
    }
}

type Error = Box<dyn std::error::Error + Send + Sync>;

struct Block {
    pcm: Vec<i16>,
    captured_through: Instant,
}

fn capture(stop: Arc<AtomicBool>, ready: Arc<AtomicBool>, level: Arc<AtomicU32>) -> Result<Audio, Error> {
    // X11 tests substitute only the microphone boundary; WAV upload and the
    // entire application lifecycle still use production code.
    if std::env::var_os("LST_X11_STATE_TRACE_FILE").is_some() {
        if let Some(path) = std::env::var_os("LST_TEST_VOICE_AUDIO") {
            let mut source = std::fs::File::open(path)?;
            let mut file = NamedTempFile::new()?;
            std::io::copy(&mut source, &mut file)?;
            let reader = hound::WavReader::open(file.path())?;
            let duration = Duration::from_secs_f64(reader.duration() as f64 / reader.spec().sample_rate as f64);
            ready.store(true, Ordering::Release);
            while !stop.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(10));
            }
            return Ok(Audio {
                file,
                duration,
                warning: None,
            });
        }
    }
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("No default microphone available")?;
    let supported = device.default_input_config()?;
    let config: cpal::StreamConfig = supported.clone().into();
    let rate = config.sample_rate.0;
    let channels = usize::from(config.channels);
    if channels == 0 || rate == 0 {
        return Err("Invalid microphone configuration".into());
    }
    let (tx, rx) = mpsc::sync_channel::<Block>(32);
    let (error_tx, error_rx) = mpsc::channel();
    let overflow = Arc::new(AtomicBool::new(false));
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, tx, error_tx, level, overflow.clone()),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, tx, error_tx, level, overflow.clone()),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, tx, error_tx, level, overflow.clone()),
        format => return Err(format!("Unsupported microphone sample format: {format}").into()),
    }?;
    let mut file = NamedTempFile::new()?;
    let mut writer = hound::WavWriter::new(
        file.as_file_mut(),
        hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    stream.play()?;
    ready.store(true, Ordering::Release);
    let mut samples = 0u64;
    let mut stopping_at = None;
    let mut captured_through = Instant::now();
    let mut warning = None;
    loop {
        if stop.load(Ordering::Acquire) {
            // Drain until device timestamps cover the stop request, rather
            // than assuming a fixed hardware latency. Bound a stalled device.
            let stop_time = *stopping_at.get_or_insert_with(Instant::now);
            if captured_through >= stop_time {
                break;
            }
            if stop_time.elapsed() >= Duration::from_secs(2) {
                warning = Some("Microphone did not finish draining; captured audio was retained.".into());
                break;
            }
        }
        if let Ok(error) = error_rx.try_recv() {
            warning = Some(error);
            break;
        }
        if overflow.load(Ordering::Acquire) {
            warning = Some("Microphone overrun; recording stopped. Captured audio was retained.".into());
            break;
        }
        if samples >= u64::from(rate) * 15 * 60 {
            warning = Some("15-minute segment limit reached. Resume to continue.".into());
            break;
        }
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(block) => {
                captured_through = block.captured_through;
                samples += block.pcm.len() as u64;
                for sample in block.pcm {
                    writer.write_sample(sample)?;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                warning = Some("Microphone disconnected".into());
                break;
            }
        }
    }
    drop(stream);
    for block in rx.try_iter() {
        samples += block.pcm.len() as u64;
        for sample in block.pcm {
            writer.write_sample(sample)?;
        }
    }
    writer.finalize()?;
    Ok(Audio {
        file,
        duration: Duration::from_secs_f64(samples as f64 / f64::from(rate)),
        warning,
    })
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: mpsc::SyncSender<Block>,
    error_tx: mpsc::Sender<String>,
    level: Arc<AtomicU32>,
    overflow: Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = usize::from(config.channels);
    let rate = config.sample_rate.0;
    device.build_input_stream(
        config,
        move |data: &[T], info| {
            let now = Instant::now();
            let (pcm, meter) = downmix(data, channels);
            let timestamp = info.timestamp();
            let latency = timestamp
                .callback
                .duration_since(&timestamp.capture)
                .unwrap_or_default();
            let duration = Duration::from_secs_f64(pcm.len() as f64 / f64::from(rate));
            let captured_through = now.checked_sub(latency.saturating_sub(duration)).unwrap_or(now);
            level.store(meter, Ordering::Relaxed);
            if tx.try_send(Block { pcm, captured_through }).is_err() {
                overflow.store(true, Ordering::Release);
            }
        },
        move |error| {
            let _ = error_tx.send(format!("Microphone: {error}"));
        },
        None,
    )
}

fn downmix<T>(data: &[T], channels: usize) -> (Vec<i16>, u32)
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let mut energy = 0.0f32;
    let pcm: Vec<i16> = data
        .chunks_exact(channels)
        .map(|frame| {
            let sample = frame
                .iter()
                .map(|v| <f32 as cpal::Sample>::from_sample(*v))
                .sum::<f32>()
                / channels as f32;
            let sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            energy += sample * sample;
            (sample * 32767.0) as i16
        })
        .collect();
    let rms = (energy / pcm.len().max(1) as f32).sqrt();
    let meter = ((20.0 * rms.max(1e-6).log10() + 50.0) / 40.0).clamp(0.0, 1.0);
    (pcm, (meter * 100.0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_preserves_frames_and_bounds_pcm_and_meter() {
        let (pcm, level) = downmix(&[1.0f32, -1.0, 0.5, 0.5, 2.0, 2.0, f32::NAN, 0.0], 2);
        assert_eq!(pcm, [0, 16383, 32767, 0]);
        assert_eq!(level, 100);
        assert_eq!(downmix(&[0i16; 8], 1), (vec![0; 8], 0));
        assert_eq!(downmix(&[32768u16; 8], 2), (vec![0; 4], 0));
        assert_eq!(downmix(&[] as &[f32], 1), (vec![], 0));
    }
}
