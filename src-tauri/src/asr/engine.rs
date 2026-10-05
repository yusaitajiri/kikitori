//! Owns the Whisper model: GPU first with a warm-up, CPU fallback (section 8).

use std::path::Path;

use whisper_rs::{WhisperContext, WhisperContextParameters, WhisperState};

use super::params;

pub struct Engine {
    ctx: WhisperContext,
    pub gpu: bool,
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadOutcome {
    Gpu,
    Cpu,
    /// GPU was attempted and failed; running on CPU.
    CpuAfterGpuFailure,
}

/// Whether this build has a GPU backend at all.
pub fn gpu_compiled() -> bool {
    cfg!(any(feature = "gpu-vulkan", feature = "gpu-cuda"))
}

/// Name of the GPU whisper.cpp will use (device 0), if any.
pub fn gpu_device_name() -> Option<String> {
    #[cfg(feature = "gpu-vulkan")]
    {
        whisper_rs::vulkan::list_devices().into_iter().next().map(|d| d.name.trim().to_string())
    }
    #[cfg(all(feature = "gpu-cuda", not(feature = "gpu-vulkan")))]
    {
        Some("CUDA".to_string())
    }
    #[cfg(not(any(feature = "gpu-vulkan", feature = "gpu-cuda")))]
    {
        None
    }
}

/// Before anything touches Vulkan: disable implicit layers (OBS, Steam overlays) that are
/// known to crash ggml, unless `KIKITORI_KEEP_VK_LAYERS=1`.
pub fn disable_vulkan_implicit_layers() {
    if std::env::var("KIKITORI_KEEP_VK_LAYERS").as_deref() != Ok("1") {
        // SAFETY: called at process start before any other thread exists.
        unsafe { std::env::set_var("VK_LOADER_LAYERS_DISABLE", "~implicit~") };
    }
}

impl Engine {
    fn create(path: &Path, use_gpu: bool) -> anyhow::Result<WhisperContext> {
        let mut p = WhisperContextParameters::default();
        p.use_gpu(use_gpu);
        p.flash_attn(use_gpu);
        WhisperContext::new_with_params(path, p).map_err(|e| anyhow::anyhow!("failed to load model: {e}"))
    }

    fn warm_up(ctx: &WhisperContext, gpu: bool) -> anyhow::Result<()> {
        let mut state = ctx.create_state().map_err(|e| anyhow::anyhow!("create state: {e}"))?;
        let opts = params::DecodeOptions {
            threads: params::thread_count(gpu),
            accuracy_first: false,
            audio_ctx_experimental: false,
        };
        let silence = vec![0.0f32; 16_000];
        let p = params::final_params(&opts, "ja", "", silence.len());
        state.full(p, &silence).map_err(|e| anyhow::anyhow!("warm-up failed: {e}"))?;
        Ok(())
    }

    /// Loads with the GPU when allowed, falling back to CPU on any failure.
    pub fn load(path: &Path, try_gpu: bool) -> anyhow::Result<(Engine, LoadOutcome)> {
        let device = gpu_device_name();
        if try_gpu && gpu_compiled() && device.is_some() {
            let attempt = Self::create(path, true).and_then(|ctx| Self::warm_up(&ctx, true).map(|_| ctx));
            match attempt {
                Ok(ctx) => return Ok((Engine { ctx, gpu: true, device_name: device }, LoadOutcome::Gpu)),
                Err(err) => tracing::warn!("GPU init failed, falling back to CPU: {err:#}"),
            }
            let ctx = Self::create(path, false)?;
            Self::warm_up(&ctx, false)?;
            return Ok((Engine { ctx, gpu: false, device_name: None }, LoadOutcome::CpuAfterGpuFailure));
        }
        let ctx = Self::create(path, false)?;
        Self::warm_up(&ctx, false)?;
        Ok((Engine { ctx, gpu: false, device_name: None }, LoadOutcome::Cpu))
    }

    pub fn create_state(&self) -> anyhow::Result<WhisperState> {
        self.ctx.create_state().map_err(|e| anyhow::anyhow!("create state: {e}"))
    }

    /// How sure Whisper is of each language (by language id) in the clip `state` just decoded:
    /// one decoder step on the start-of-transcript token over the encoder output the decode left
    /// in the state, which is how whisper.cpp detects a language, without encoding the audio
    /// again. `None` for a model that knows only English.
    pub fn languages(&self, state: &mut WhisperState) -> anyhow::Result<Option<Vec<f32>>> {
        if !self.ctx.is_multilingual() {
            return Ok(None);
        }
        state
            .decode(&[self.ctx.token_sot()], 0, self.threads().max(1) as usize)
            .map_err(|e| anyhow::anyhow!("language check: {e}"))?;
        let logits = state.get_logits().map_err(|e| anyhow::anyhow!("language check: {e}"))?;
        let lang: Vec<f32> = (0..=whisper_rs::get_lang_max_id())
            .map(|id| logits.get(self.ctx.token_lang(id) as usize).copied().unwrap_or(f32::NEG_INFINITY))
            .collect();
        Ok(Some(super::language::softmax(&lang)))
    }

    pub fn threads(&self) -> i32 {
        params::thread_count(self.gpu)
    }
}
