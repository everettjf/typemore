//! ASR (automatic speech recognition) provider abstraction.
//!
//! Local mode (Sherpa-ONNX) is dispatched in `lib.rs` because it needs the model files
//! resolved from the Tauri app data dir. Cloud providers all follow the OpenAI
//! `/audio/transcriptions` request shape, so they share `transcribe_via_openai_compatible`.

use std::io::Cursor;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AsrProviderKind {
    LocalSherpa,
    OpenaiWhisper,
    GroqWhisper,
    OpenaiCompatible,
}

impl Default for AsrProviderKind {
    fn default() -> Self {
        Self::LocalSherpa
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrSettings {
    #[serde(default)]
    pub provider: AsrProviderKind,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default = "default_asr_model")]
    pub model: String,
}

impl Default for AsrSettings {
    fn default() -> Self {
        Self {
            provider: AsrProviderKind::default(),
            api_key: String::new(),
            base_url: String::new(),
            model: default_asr_model(),
        }
    }
}

pub fn default_asr_model() -> String {
    "whisper-1".to_string()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestAsrResult {
    pub ok: bool,
    pub message: String,
}

const OPENAI_DEFAULT_BASE: &str = "https://api.openai.com/v1";
const GROQ_DEFAULT_BASE: &str = "https://api.groq.com/openai/v1";

/// Resolve the API base URL for cloud providers, falling back to vendor defaults
/// when the user-supplied override is empty. `OpenaiCompatible` requires an explicit URL.
pub fn resolve_base_url(kind: AsrProviderKind, override_url: &str) -> Result<String, String> {
    let override_trim = override_url.trim();
    let url = match (kind, override_trim.is_empty()) {
        (AsrProviderKind::OpenaiWhisper, true) => OPENAI_DEFAULT_BASE.to_string(),
        (AsrProviderKind::GroqWhisper, true) => GROQ_DEFAULT_BASE.to_string(),
        (AsrProviderKind::OpenaiCompatible, true) => {
            return Err("base url is required for OpenAI-compatible provider".into());
        }
        _ => override_trim.to_string(),
    };
    Ok(url.trim_end_matches('/').to_string())
}

/// Encode `f32` samples (range -1..1) into a PCM16 mono WAV byte buffer.
pub fn encode_wav_pcm16(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, String> {
    let mut buffer: Vec<u8> = Vec::with_capacity(44 + samples.len() * 2);
    {
        let cursor = Cursor::new(&mut buffer);
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(cursor, spec)
            .map_err(|e| format!("failed to start wav writer: {e}"))?;
        for sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            let val = (clamped * i16::MAX as f32) as i16;
            writer
                .write_sample(val)
                .map_err(|e| format!("failed to write wav sample: {e}"))?;
        }
        writer
            .finalize()
            .map_err(|e| format!("failed to finalize wav: {e}"))?;
    }
    Ok(buffer)
}

/// POST audio to an OpenAI-compatible `/audio/transcriptions` endpoint and return the
/// recognized text.
pub fn transcribe_via_openai_compatible(
    samples: &[f32],
    sample_rate: u32,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err("api key is missing".into());
    }
    if model.trim().is_empty() {
        return Err("model is missing".into());
    }
    let wav_bytes = encode_wav_pcm16(samples, sample_rate)?;

    let endpoint = format!("{}/audio/transcriptions", base_url);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("http client init failed: {e}"))?;

    let part = reqwest::blocking::multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("multipart mime failed: {e}"))?;
    let form = reqwest::blocking::multipart::Form::new()
        .text("model", model.to_string())
        .text("response_format", "json".to_string())
        .part("file", part);

    let response = client
        .post(&endpoint)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .map_err(|e| format!("request failed: {e}"))?;

    let status = response.status();
    let body = response
        .text()
        .map_err(|e| format!("read response body failed: {e}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status.as_u16(), body));
    }

    let parsed: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("invalid JSON response: {e} (body: {body})"))?;
    if let Some(text) = parsed.get("text").and_then(|v| v.as_str()) {
        return Ok(text.to_string());
    }
    Err(format!("no `text` field in response: {body}"))
}
