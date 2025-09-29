pub mod devices;
pub mod dsp;
pub mod graph;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use dsp::gain::Gain;
use graph::Chain;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::cell::UnsafeCell;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub input_name: Option<String>,       // match by substring (case-insensitive)
    pub output_name: Option<String>,
    pub input_index: Option<usize>,       // explicit index from device list
    pub output_index: Option<usize>,
    pub sample_rate: Option<u32>,         // e.g., 48000
    pub block_size: Option<u32>,          // frames per buffer (if backend supports)
    pub gain_db: f32,                     // simple test effect
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            input_name: None,
            output_name: None,
            input_index: None,
            output_index: None,
            sample_rate: None,
            block_size: None,
            gain_db: 0.0,
        }
    }
}

pub struct Engine {
    input_stream: Option<cpal::Stream>,
    output_stream: Option<cpal::Stream>,
    cfg: EngineConfig,
}

impl Engine {
    pub fn new(cfg: EngineConfig) -> Self {
        Self { input_stream: None, output_stream: None, cfg }
    }

    /// Start a simple chain (Gain) on input -> output.
    pub fn start(&mut self) -> Result<()> {
        let host = cpal::default_host();

        let in_dev  = pick_device(&host, true,  self.cfg.input_name.as_deref(), self.cfg.input_index)?
            .context("No input device matched (and no default available)")?;
        let out_dev = pick_device(&host, false, self.cfg.output_name.as_deref(), self.cfg.output_index)?
            .context("No output device matched (and no default available)")?;

        let in_cfg_any  = in_dev.default_input_config().context("No default input config")?;
        let out_cfg_any = out_dev.default_output_config().context("No default output config")?;

        let mut in_cfg  = in_cfg_any.config();
        let mut out_cfg = out_cfg_any.config();

        // Honor sample_rate/block_size if provided (best-effort)
        if let Some(sr) = self.cfg.sample_rate {
            in_cfg.sample_rate  = cpal::SampleRate(sr);
            out_cfg.sample_rate = cpal::SampleRate(sr);
        }
        if let Some(bs) = self.cfg.block_size {
            out_cfg.buffer_size = cpal::BufferSize::Fixed(bs);
            in_cfg.buffer_size  = cpal::BufferSize::Fixed(bs);
        }

        // Align channels/SR
        in_cfg.channels    = out_cfg.channels;
        in_cfg.sample_rate = out_cfg.sample_rate;

        let channels = out_cfg.channels as usize;
        let sr = out_cfg.sample_rate.0;

        // Capacity: choose a power-of-two ring >= 8 output buffers
        let cap_frames = match out_cfg.buffer_size {
            cpal::BufferSize::Fixed(n) => (n as usize) * 8,
            _ => 4096,
        };
        let cap = next_pow2(cap_frames * channels).max(1024);

        let ring = Arc::new(SpscRingF32::with_capacity(cap));
        let ring_tx = ring.clone();
        let ring_rx = ring.clone();

        // Build a serial chain: for now, just Gain.
        let mut chain = Chain::new(sr, out_cfg.channels);
        chain.push(Box::new(Gain::new(self.cfg.gain_db)));

        // Scratch buffer reused in the input callback (avoid allocs)
        let mut scratch = Vec::<f32>::with_capacity(cap);

        /* --------- INPUT (format-specific) --------- */
        let input_stream = match in_cfg_any.sample_format() {
            cpal::SampleFormat::F32 => {
                in_dev.build_input_stream::<f32, _, _>(
                    &in_cfg,
                    {
                        let mut chain = chain;
                        let ring = ring_tx;
                        move |data: &[f32], _| {
                            // reuse scratch
                            scratch.clear();
                            scratch.extend_from_slice(data);
                            chain.process(&mut scratch);
                            let _ = ring.push_slice(&scratch);
                        }
                    },
                    move |err| eprintln!("input stream error: {err}"),
                    None,
                )?
            }
            cpal::SampleFormat::I16 => {
                in_dev.build_input_stream::<i16, _, _>(
                    &in_cfg,
                    {
                        let mut chain = chain;
                        let ring = ring_tx;
                        move |data: &[i16], _| {
                            scratch.clear();
                            scratch.reserve(data.len());
                            for &s in data { scratch.push(s as f32 / 32768.0); }
                            chain.process(&mut scratch);
                            let _ = ring.push_slice(&scratch);
                        }
                    },
                    move |err| eprintln!("input stream error: {err}"),
                    None,
                )?
            }
            cpal::SampleFormat::U16 => {
                in_dev.build_input_stream::<u16, _, _>(
                    &in_cfg,
                    {
                        let mut chain = chain;
                        let ring = ring_tx;
                        move |data: &[u16], _| {
                            scratch.clear();
                            scratch.reserve(data.len());
                            for &s in data { scratch.push(((s as f32 / 65535.0) * 2.0) - 1.0); }
                            chain.process(&mut scratch);
                            let _ = ring.push_slice(&scratch);
                        }
                    },
                    move |err| eprintln!("input stream error: {err}"),
                    None,
                )?
            }
            other => return Err(anyhow!("Unsupported input format: {other:?}")),
        };

        /* --------- OUTPUT (format-specific) -------- */
        let output_stream = match out_cfg_any.sample_format() {
            cpal::SampleFormat::F32 => {
                out_dev.build_output_stream::<f32, _, _>(
                    &out_cfg,
                    move |out: &mut [f32], _| {
                        if !ring_rx.pop_into(out) {
                            out.fill(0.0);
                        }
                    },
                    move |err| eprintln!("output stream error: {err}"),
                    None,
                )?
            }
            cpal::SampleFormat::I16 => {
                out_dev.build_output_stream::<i16, _, _>(
                    &out_cfg,
                    move |out: &mut [i16], _| {
                        // read into a temp f32 stack buffer, then convert
                        // (stack buffer sized by out.len() is fine for typical < 4096)
                        let mut tmp = vec![0.0f32; out.len()];
                        if ring_rx.pop_into(&mut tmp) {
                            for (o, &v) in out.iter_mut().zip(tmp.iter()) {
                                *o = (v.clamp(-1.0, 1.0) * 32767.0) as i16;
                            }
                        } else {
                            out.fill(0);
                        }
                    },
                    move |err| eprintln!("output stream error: {err}"),
                    None,
                )?
            }
            cpal::SampleFormat::U16 => {
                out_dev.build_output_stream::<u16, _, _>(
                    &out_cfg,
                    move |out: &mut [u16], _| {
                        let mut tmp = vec![0.0f32; out.len()];
                        if ring_rx.pop_into(&mut tmp) {
                            for (o, &v) in out.iter_mut().zip(tmp.iter()) {
                                *o = (((v.clamp(-1.0, 1.0) + 1.0) * 0.5) * 65535.0) as u16;
                            }
                        } else {
                            out.fill(32768);
                        }
                    },
                    move |err| eprintln!("output stream error: {err}"),
                    None,
                )?
            }
            other => return Err(anyhow!("Unsupported output format: {other:?}")),
        };

        input_stream.play().context("Failed to play input stream")?;
        output_stream.play().context("Failed to play output stream")?;

        self.input_stream  = Some(input_stream);
        self.output_stream = Some(output_stream);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.input_stream  = None;
        self.output_stream = None;
    }
}

/* ---------- device picking (by name or index) ---------- */

fn pick_device(
    host: &cpal::Host,
    want_input: bool,
    name_substr: Option<&str>,
    index: Option<usize>,
) -> Result<Option<cpal::Device>> {
    // Try explicit index first
    if let Some(idx) = index {
        let mut i = 0usize;
        for dev in host.devices()? {
            // filter by input/output capability
            let ok = if want_input {
                dev.supported_input_configs().ok().is_some()
            } else {
                dev.supported_output_configs().ok().is_some()
            };
            if ok {
                if i == idx { return Ok(Some(dev)); }
                i += 1;
            }
        }
        // fallthrough to name/default if index not found
    }

    // Then try substring match
    let normalize = |s: &str| s.to_lowercase();
    if let Some(q) = name_substr {
        let qn = normalize(q);
        for dev in host.devices()? {
            let name = dev.name().unwrap_or_default();
            if normalize(&name).contains(&qn) {
                let ok = if want_input {
                    dev.supported_input_configs().ok().is_some()
                } else {
                    dev.supported_output_configs().ok().is_some()
                };
                if ok { return Ok(Some(dev)); }
            }
        }
    }

    // Fallback to default
    Ok(if want_input { host.default_input_device() } else { host.default_output_device() })
}

/* ---------- lock-free SPSC ring (power-of-two capacity) ---------- */

fn next_pow2(mut x: usize) -> usize {
    if x <= 1 { return 1; }
    x -= 1;
    x |= x >> 1;
    x |= x >> 2;
    x |= x >> 4;
    x |= x >> 8;
    x |= x >> 16;
    #[cfg(target_pointer_width = "64")]
    { x |= x >> 32; }
    x + 1
}

struct SpscRingF32 {
    // Interior mutability: single producer writes, single consumer reads.
    buf: UnsafeCell<Box<[f32]>>,
    mask: usize,
    write: AtomicUsize,
    read: AtomicUsize,
}

// Safety: we uphold SPSC discipline externally; only one writer and one reader exist.
// The writer only mutates indices [read..write) advancing write; the reader only
// reads and advances read. No aliasing writes occur.
unsafe impl Send for SpscRingF32 {}
unsafe impl Sync for SpscRingF32 {}

impl SpscRingF32 {
    fn with_capacity(cap: usize) -> Self {
        let cap_pow2 = next_pow2(cap);
        Self {
            buf: UnsafeCell::new(vec![0.0f32; cap_pow2].into_boxed_slice()),
            mask: cap_pow2 - 1,
            write: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
        }
    }

    #[inline]
    fn len(&self, w: usize, r: usize) -> usize {
        w.wrapping_sub(r) & self.mask
    }

    /// Producer: push entire slice; returns false if not enough space.
    fn push_slice(&self, data: &[f32]) -> bool {
        let r = self.read.load(Ordering::Acquire);
        let w = self.write.load(Ordering::Relaxed);
        let cap = unsafe { (&*self.buf.get()).len() };
        let free = cap - self.len(w, r) - 1;
        if free < data.len() { return false; }

        // Safe because: single producer thread writes, and we never write indices
        // that the consumer is reading (bounded by free-space check).
        let buf = unsafe { &mut *self.buf.get() };
        let mut wi = w;
        for &v in data {
            buf[wi & self.mask] = v;
            wi = wi.wrapping_add(1);
        }
        self.write.store(wi, Ordering::Release);
        true
    }

    /// Consumer: pop exactly out.len() samples into out; false if not enough data.
    fn pop_into(&self, out: &mut [f32]) -> bool {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Relaxed);
        let avail = self.len(w, r);
        if avail < out.len() { return false; }

        // Safe because: single consumer thread reads; producer only writes beyond `w`.
        let buf = unsafe { &*self.buf.get() };
        let mut ri = r;
        for o in out.iter_mut() {
            *o = buf[ri & self.mask];
            ri = ri.wrapping_add(1);
        }
        self.read.store(ri, Ordering::Release);
        true
    }
}
