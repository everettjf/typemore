#![allow(unexpected_cfgs)]

use serde::{Deserialize, Serialize};
use sherpa_rs::{paraformer::ParaformerConfig, paraformer::ParaformerRecognizer};
use std::{
    collections::HashMap,
    ffi::c_void,
    fs,
    io::{BufWriter, Cursor, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Listener, LogicalSize, Manager, PhysicalPosition, Position, Size};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutEvent, ShortcutState};
use walkdir::WalkDir;

const MODEL_ARCHIVE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-paraformer-trilingual-zh-cantonese-en.tar.bz2";
const MODEL_DIR_NAME: &str = "sherpa-model";
const EXTRACTED_DIR_NAME: &str = "extracted";
const RECORDINGS_DIR_NAME: &str = "recordings";
const TEMP_DIR_NAME: &str = "tmp";
const TRANSCRIPT_CACHE_FILE: &str = "transcript_cache.json";
const INIT_EVENT: &str = "model-init-progress";
const HOTKEY_EVENT: &str = "global-shortcut-triggered";
const OVERLAY_EVENT: &str = "overlay-state";
const HOTKEY_TOGGLE_DICTATION: &str = "CommandOrControl+Alt+Space";
const OVERLAY_WINDOW_LABEL: &str = "overlay";
const OVERLAY_WIDTH: f64 = 210.0;
const OVERLAY_HEIGHT: f64 = 25.0;
const OVERLAY_BOTTOM_MARGIN: i32 = 150;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RecordingItem {
    id: String,
    name: String,
    file_path: String,
    created_at_ms: u128,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelStatus {
    ready: bool,
    model_path: Option<String>,
    tokens_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccessibilityStatus {
    supported: bool,
    trusted: bool,
    ax_trusted: bool,
    tcc_allowed: Option<bool>,
    runtime_hint: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GlobalShortcutPayload {
    action: String,
    shortcut: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveAndTranscribeResult {
    recording: RecordingItem,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveRecordingPayload {
    suggested_name: Option<String>,
    wav_data: Vec<u8>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ModelInitStatus {
    running: bool,
    phase: String,
    progress: f32,
    message: String,
    ready: bool,
    error: Option<String>,
}

impl Default for ModelInitStatus {
    fn default() -> Self {
        Self {
            running: false,
            phase: "idle".into(),
            progress: 0.0,
            message: "模型尚未初始化".into(),
            ready: false,
            error: None,
        }
    }
}

struct AppState {
    init_status: Mutex<ModelInitStatus>,
    hotkeys: Mutex<HotkeyConfig>,
}

#[derive(Debug, Clone)]
struct HotkeyConfig {
    toggle: String,
    toggle_id: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HotkeySettings {
    toggle: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedHotkeySettings {
    toggle: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct OverlayStatePayload {
    phase: String,
    text: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            init_status: Mutex::new(ModelInitStatus::default()),
            hotkeys: Mutex::new(
                build_hotkey_config(HOTKEY_TOGGLE_DICTATION).expect("invalid default hotkeys"),
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedTranscript {
    text: String,
    updated_at_ms: u128,
}

type TranscriptCacheMap = HashMap<String, CachedTranscript>;

fn build_hotkey_config(toggle: &str) -> Result<HotkeyConfig, String> {
    let toggle_shortcut: tauri_plugin_global_shortcut::Shortcut = toggle
        .parse()
        .map_err(|e| format!("invalid toggle shortcut: {e}"))?;
    Ok(HotkeyConfig {
        toggle: toggle.to_string(),
        toggle_id: toggle_shortcut.id(),
    })
}

fn apply_toggle_shortcut(app: &AppHandle, new_config: HotkeyConfig) -> Result<HotkeyConfig, String> {
    let state = app.state::<AppState>();
    let old_config = {
        let lock = state
            .hotkeys
            .lock()
            .map_err(|_| "failed to read current hotkeys".to_string())?;
        lock.clone()
    };

    let manager = app.global_shortcut();
    if old_config.toggle != new_config.toggle && manager.is_registered(old_config.toggle.as_str()) {
        manager
            .unregister(old_config.toggle.as_str())
            .map_err(|e| format!("failed to unregister old toggle shortcut: {e}"))?;
    }

    if !manager.is_registered(new_config.toggle.as_str()) {
        manager
            .register(new_config.toggle.as_str())
            .map_err(|e| format!("failed to register toggle shortcut: {e}"))?;
    }

    {
        let mut lock = state
            .hotkeys
            .lock()
            .map_err(|_| "failed to update hotkey settings".to_string())?;
        *lock = new_config.clone();
    }

    Ok(new_config)
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))
}

fn hotkey_settings_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("hotkeys.json"))
}

fn load_persisted_hotkeys(app: &AppHandle) -> Result<Option<PersistedHotkeySettings>, String> {
    let path = hotkey_settings_file(app)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("failed to read hotkey settings: {e}"))?;
    let parsed = serde_json::from_str::<PersistedHotkeySettings>(&raw)
        .map_err(|e| format!("failed to parse hotkey settings: {e}"))?;
    Ok(Some(parsed))
}

fn save_persisted_hotkeys(app: &AppHandle, settings: &PersistedHotkeySettings) -> Result<(), String> {
    let path = hotkey_settings_file(app)?;
    let raw = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("failed to serialize hotkey settings: {e}"))?;
    fs::write(path, raw).map_err(|e| format!("failed to write hotkey settings: {e}"))
}

fn recordings_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join(RECORDINGS_DIR_NAME);
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create recordings dir: {e}"))?;
    Ok(dir)
}

fn transcript_cache_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(recordings_dir(app)?.join(TRANSCRIPT_CACHE_FILE))
}

fn temp_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join(TEMP_DIR_NAME);
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create temp dir: {e}"))?;
    Ok(dir)
}

fn model_root_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join(MODEL_DIR_NAME);
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create model dir: {e}"))?;
    Ok(dir)
}

fn file_stem_as_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("untitled")
        .replace('_', " ")
}

fn created_at_ms(path: &Path) -> u128 {
    let modified = fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .unwrap_or_else(SystemTime::now);
    modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn to_recording_item(path: &Path) -> Option<RecordingItem> {
    if path.extension().and_then(|x| x.to_str()) != Some("wav") {
        return None;
    }
    let id = path.file_name()?.to_string_lossy().to_string();
    Some(RecordingItem {
        id,
        name: file_stem_as_name(path),
        file_path: path.to_string_lossy().to_string(),
        created_at_ms: created_at_ms(path),
    })
}

fn collect_recordings(dir: &Path) -> Result<Vec<RecordingItem>, String> {
    let mut items = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("failed to list recordings: {e}"))? {
        let entry = entry.map_err(|e| format!("failed to read recording entry: {e}"))?;
        if let Some(item) = to_recording_item(&entry.path()) {
            items.push(item);
        }
    }
    items.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
    Ok(items)
}

fn sanitize_filename(name: &str) -> String {
    let filtered: String = name
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | ' ' => c,
            _ => '_',
        })
        .collect();
    let compact = filtered.trim().replace(' ', "_");
    if compact.is_empty() {
        "recording".into()
    } else {
        compact
    }
}

fn find_model_files(root: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut onnx_candidates: Vec<PathBuf> = Vec::new();
    let mut token_candidates: Vec<PathBuf> = Vec::new();

    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_lowercase();

        if file_name.ends_with(".onnx") {
            onnx_candidates.push(path.to_path_buf());
        }
        if file_name == "tokens.txt" || (file_name.contains("token") && file_name.ends_with(".txt"))
        {
            token_candidates.push(path.to_path_buf());
        }
    }

    onnx_candidates.sort_by_key(|p| {
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_lowercase();
        let mut score = 100;
        if name.contains("model") {
            score -= 40;
        }
        if name.contains("int8") {
            score -= 10;
        }
        score
    });
    token_candidates.sort();

    match (onnx_candidates.first(), token_candidates.first()) {
        (Some(model), Some(tokens)) => Some((model.clone(), tokens.clone())),
        _ => None,
    }
}

fn model_files_if_ready(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let extracted = model_root_dir(app)?.join(EXTRACTED_DIR_NAME);
    find_model_files(&extracted).ok_or_else(|| "model not initialized yet".into())
}

fn set_init_status(app: &AppHandle, status: ModelInitStatus) {
    {
        let state = app.state::<AppState>();
        if let Ok(mut lock) = state.init_status.lock() {
            *lock = status.clone();
        };
    }
    let _ = app.emit(INIT_EVENT, status);
}

fn get_init_status(app: &AppHandle) -> ModelInitStatus {
    let state = app.state::<AppState>();
    state
        .init_status
        .lock()
        .map(|s| s.clone())
        .unwrap_or_else(|_| ModelInitStatus::default())
}

fn load_transcript_cache(app: &AppHandle) -> Result<TranscriptCacheMap, String> {
    let cache_file = transcript_cache_file(app)?;
    if !cache_file.exists() {
        return Ok(HashMap::new());
    }
    let raw = fs::read_to_string(&cache_file)
        .map_err(|e| format!("failed to read transcript cache: {e}"))?;
    serde_json::from_str::<TranscriptCacheMap>(&raw)
        .map_err(|e| format!("failed to parse transcript cache: {e}"))
}

fn save_transcript_cache(app: &AppHandle, cache: &TranscriptCacheMap) -> Result<(), String> {
    let cache_file = transcript_cache_file(app)?;
    let raw = serde_json::to_string_pretty(cache)
        .map_err(|e| format!("failed to serialize transcript cache: {e}"))?;
    fs::write(cache_file, raw).map_err(|e| format!("failed to persist transcript cache: {e}"))
}

fn get_cached_transcript(app: &AppHandle, id: &str) -> Result<Option<String>, String> {
    let cache = load_transcript_cache(app)?;
    Ok(cache.get(id).map(|v| v.text.clone()))
}

fn put_cached_transcript(app: &AppHandle, id: &str, text: &str) -> Result<(), String> {
    let mut cache = load_transcript_cache(app)?;
    cache.insert(
        id.to_string(),
        CachedTranscript {
            text: text.to_string(),
            updated_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        },
    );
    save_transcript_cache(app, &cache)
}

fn remove_cached_transcript(app: &AppHandle, id: &str) -> Result<(), String> {
    let mut cache = load_transcript_cache(app)?;
    cache.remove(id);
    save_transcript_cache(app, &cache)
}

fn move_cached_transcript_key(app: &AppHandle, from_id: &str, to_id: &str) -> Result<(), String> {
    let mut cache = load_transcript_cache(app)?;
    if let Some(v) = cache.remove(from_id) {
        cache.insert(to_id.to_string(), v);
    }
    save_transcript_cache(app, &cache)
}

fn extract_model_archive(archive_path: &Path, output_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(output_dir).map_err(|e| format!("failed to create extract dir: {e}"))?;
    let status = Command::new("tar")
        .arg("-xjf")
        .arg(archive_path)
        .arg("-C")
        .arg(output_dir)
        .status()
        .map_err(|e| format!("failed to run tar: {e}"))?;
    if !status.success() {
        return Err("failed to extract model archive with tar".into());
    }
    Ok(())
}

fn download_file_with_progress(
    url: &str,
    output: &Path,
    on_progress: &mut dyn FnMut(f32, String),
) -> Result<(), String> {
    let client = reqwest::blocking::Client::new();
    let mut resp = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("failed to download model archive: {e}"))?;

    let total = resp.content_length();
    let mut writer = BufWriter::new(
        fs::File::create(output).map_err(|e| format!("failed to create archive file: {e}"))?,
    );

    let mut downloaded: u64 = 0;
    let mut buffer = [0u8; 64 * 1024];
    let mut last_emit = Instant::now();
    let mut last_ratio = 0.0f32;

    loop {
        let n = resp
            .read(&mut buffer)
            .map_err(|e| format!("failed while downloading model archive: {e}"))?;
        if n == 0 {
            break;
        }
        writer
            .write_all(&buffer[..n])
            .map_err(|e| format!("failed to write model archive: {e}"))?;

        downloaded += n as u64;
        let message = if let Some(all) = total {
            let p = (downloaded as f32 / all as f32).clamp(0.0, 1.0);
            // Throttle progress events to avoid UI flicker from overly frequent updates.
            let should_emit = p >= 1.0
                || (p - last_ratio) >= 0.003
                || last_emit.elapsed().as_millis() >= 120;
            if should_emit {
                on_progress(
                    80.0 * p,
                    format!(
                        "下载模型中... {:.1}% ({:.1} MB / {:.1} MB)",
                        p * 100.0,
                        downloaded as f32 / 1_048_576.0,
                        all as f32 / 1_048_576.0
                    ),
                );
                last_emit = Instant::now();
                last_ratio = p;
            }
            continue;
        } else {
            format!("下载模型中... {:.1} MB", downloaded as f32 / 1_048_576.0)
        };
        on_progress(40.0, message);
    }

    writer
        .flush()
        .map_err(|e| format!("failed to flush model archive: {e}"))?;
    Ok(())
}

fn run_model_init_job(app: &AppHandle) -> Result<(), String> {
    if model_files_if_ready(app).is_ok() {
        set_init_status(
            app,
            ModelInitStatus {
                running: false,
                phase: "done".into(),
                progress: 100.0,
                message: "模型已就绪".into(),
                ready: true,
                error: None,
            },
        );
        return Ok(());
    }

    let model_root = model_root_dir(app)?;
    let extracted = model_root.join(EXTRACTED_DIR_NAME);
    let archive_path = model_root.join("model.tar.bz2");

    if !archive_path.exists() {
        set_init_status(
            app,
            ModelInitStatus {
                running: true,
                phase: "download".into(),
                progress: 1.0,
                message: "开始下载模型".into(),
                ready: false,
                error: None,
            },
        );

        let mut progress_callback = |progress: f32, message: String| {
            set_init_status(
                app,
                ModelInitStatus {
                    running: true,
                    phase: "download".into(),
                    progress,
                    message,
                    ready: false,
                    error: None,
                },
            );
        };
        download_file_with_progress(MODEL_ARCHIVE_URL, &archive_path, &mut progress_callback)?;
    } else {
        set_init_status(
            app,
            ModelInitStatus {
                running: true,
                phase: "download".into(),
                progress: 80.0,
                message: "检测到已下载模型包，跳过下载".into(),
                ready: false,
                error: None,
            },
        );
    }

    set_init_status(
        app,
        ModelInitStatus {
            running: true,
            phase: "extract".into(),
            progress: 85.0,
            message: "正在解压模型".into(),
            ready: false,
            error: None,
        },
    );

    extract_model_archive(&archive_path, &extracted)?;

    set_init_status(
        app,
        ModelInitStatus {
            running: true,
            phase: "scan".into(),
            progress: 96.0,
            message: "正在校验模型文件".into(),
            ready: false,
            error: None,
        },
    );

    let _ = model_files_if_ready(app)?;

    set_init_status(
        app,
        ModelInitStatus {
            running: false,
            phase: "done".into(),
            progress: 100.0,
            message: "模型初始化完成".into(),
            ready: true,
            error: None,
        },
    );

    Ok(())
}

fn decode_wav_samples(wav_data: &[u8]) -> Result<(Vec<f32>, u32), String> {
    let mut reader = hound::WavReader::new(Cursor::new(wav_data))
        .map_err(|e| format!("invalid wav audio: {e}"))?;
    let spec = reader.spec();

    if spec.channels != 1 {
        return Err("wav must be mono channel".into());
    }

    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("failed to read float samples: {e}"))?,
        hound::SampleFormat::Int => {
            if spec.bits_per_sample <= 16 {
                reader
                    .samples::<i16>()
                    .map(|v| v.map(|s| s as f32 / i16::MAX as f32))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| format!("failed to read int16 samples: {e}"))?
            } else {
                reader
                    .samples::<i32>()
                    .map(|v| v.map(|s| s as f32 / i32::MAX as f32))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| format!("failed to read int32 samples: {e}"))?
            }
        }
    };

    Ok((samples, spec.sample_rate))
}

fn transcribe_samples(
    model_path: &Path,
    tokens_path: &Path,
    sample_rate: u32,
    samples: &[f32],
) -> Result<String, String> {
    let mut recognizer = ParaformerRecognizer::new(ParaformerConfig {
        model: model_path.to_string_lossy().to_string(),
        tokens: tokens_path.to_string_lossy().to_string(),
        ..Default::default()
    })
    .map_err(|e| format!("failed to initialize recognizer: {e}"))?;

    let result = recognizer.transcribe(sample_rate, samples);
    Ok(result.text)
}

#[tauri::command]
fn check_model_status(app: AppHandle) -> Result<ModelStatus, String> {
    Ok(match model_files_if_ready(&app) {
        Ok((model, tokens)) => ModelStatus {
            ready: true,
            model_path: Some(model.to_string_lossy().to_string()),
            tokens_path: Some(tokens.to_string_lossy().to_string()),
        },
        Err(_) => ModelStatus {
            ready: false,
            model_path: None,
            tokens_path: None,
        },
    })
}

#[tauri::command]
fn get_model_init_status(app: AppHandle) -> Result<ModelInitStatus, String> {
    let mut status = get_init_status(&app);
    if !status.running {
        status.ready = model_files_if_ready(&app).is_ok();
        if status.ready && status.phase == "idle" {
            status.phase = "done".into();
            status.progress = 100.0;
            status.message = "模型已就绪".into();
        }
    }
    Ok(status)
}

#[tauri::command]
fn init_model(app: AppHandle) -> Result<ModelInitStatus, String> {
    if model_files_if_ready(&app).is_ok() {
        let status = ModelInitStatus {
            running: false,
            phase: "done".into(),
            progress: 100.0,
            message: "模型已就绪".into(),
            ready: true,
            error: None,
        };
        set_init_status(&app, status.clone());
        return Ok(status);
    }

    let current = get_init_status(&app);
    if current.running {
        return Ok(current);
    }

    let started = ModelInitStatus {
        running: true,
        phase: "queued".into(),
        progress: 0.0,
        message: "初始化任务已启动".into(),
        ready: false,
        error: None,
    };
    set_init_status(&app, started.clone());

    let app_handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(err) = run_model_init_job(&app_handle) {
            set_init_status(
                &app_handle,
                ModelInitStatus {
                    running: false,
                    phase: "error".into(),
                    progress: 0.0,
                    message: "模型初始化失败".into(),
                    ready: false,
                    error: Some(err),
                },
            );
        }
    });

    Ok(started)
}

#[tauri::command]
fn list_recordings(app: AppHandle) -> Result<Vec<RecordingItem>, String> {
    let dir = recordings_dir(&app)?;
    collect_recordings(&dir)
}

#[tauri::command]
fn rename_recording(app: AppHandle, id: String, new_name: String) -> Result<RecordingItem, String> {
    let dir = recordings_dir(&app)?;
    let src = dir.join(&id);
    if !src.exists() {
        return Err("recording not found".into());
    }

    let sanitized = sanitize_filename(&new_name);
    let dst_name = format!("{sanitized}.wav");
    let dst = dir.join(dst_name);
    fs::rename(&src, &dst).map_err(|e| format!("failed to rename recording: {e}"))?;
    let dst_id = dst
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| "failed to build new recording id".to_string())?
        .to_string();
    move_cached_transcript_key(&app, &id, &dst_id)?;

    to_recording_item(&dst).ok_or_else(|| "failed to create recording metadata".into())
}

#[tauri::command]
fn delete_recording(app: AppHandle, id: String) -> Result<(), String> {
    let dir = recordings_dir(&app)?;
    let target = dir.join(&id);
    if target.exists() {
        fs::remove_file(target).map_err(|e| format!("failed to delete recording: {e}"))?;
    }
    remove_cached_transcript(&app, &id)?;
    Ok(())
}

#[tauri::command]
fn get_recording_cached_transcript(app: AppHandle, id: String) -> Result<Option<String>, String> {
    get_cached_transcript(&app, &id)
}

#[tauri::command]
fn transcribe_recording(app: AppHandle, id: String, force: Option<bool>) -> Result<String, String> {
    if !force.unwrap_or(false) {
        if let Some(text) = get_cached_transcript(&app, &id)? {
            return Ok(text);
        }
    }

    let dir = recordings_dir(&app)?;
    let path = dir.join(&id);
    if !path.exists() {
        return Err("recording not found".into());
    }

    let (model, tokens) = model_files_if_ready(&app)?;
    let wav_data = fs::read(&path).map_err(|e| format!("failed to read wav file: {e}"))?;
    let (samples, sample_rate) = decode_wav_samples(&wav_data)?;
    let text = transcribe_samples(&model, &tokens, sample_rate, &samples)?;
    put_cached_transcript(&app, &id, &text)?;
    Ok(text)
}

#[tauri::command]
fn save_recording_and_transcribe(
    app: AppHandle,
    payload: SaveRecordingPayload,
) -> Result<SaveAndTranscribeResult, String> {
    let dir = recordings_dir(&app)?;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let base_name = sanitize_filename(payload.suggested_name.as_deref().unwrap_or("recording"));
    let file_name = format!("{now_ms}_{base_name}.wav");
    let path = dir.join(file_name);

    fs::write(&path, &payload.wav_data).map_err(|e| format!("failed to save wav file: {e}"))?;

    let (model, tokens) = model_files_if_ready(&app)?;
    let (samples, sample_rate) = decode_wav_samples(&payload.wav_data)?;
    let text = transcribe_samples(&model, &tokens, sample_rate, &samples)?;

    let recording =
        to_recording_item(&path).ok_or_else(|| "failed to build recording metadata".to_string())?;
    put_cached_transcript(&app, &recording.id, &text)?;

    Ok(SaveAndTranscribeResult { recording, text })
}

#[tauri::command]
fn open_temp_dir(app: AppHandle) -> Result<String, String> {
    let dir = temp_dir(&app)?;
    let status = if cfg!(target_os = "macos") {
        Command::new("open").arg(&dir).status()
    } else if cfg!(target_os = "windows") {
        Command::new("explorer").arg(&dir).status()
    } else {
        Command::new("xdg-open").arg(&dir).status()
    }
    .map_err(|e| format!("failed to open temp dir: {e}"))?;

    if !status.success() {
        return Err("failed to open temp dir in file manager".into());
    }

    Ok(dir.to_string_lossy().to_string())
}

#[cfg(target_os = "macos")]
fn macos_is_accessibility_trusted() -> bool {
    // SAFETY: AXIsProcessTrusted is a pure system query function.
    let ax_trusted = unsafe { AXIsProcessTrusted() };
    if !ax_trusted {
        return false;
    }
    // Additional runtime probe: verify we can actually talk to System Events.
    // This avoids false positives where AX trust appears true but automation still fails.
    let probe = Command::new("osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to get name of first process whose frontmost is true")
        .status();
    matches!(probe, Ok(status) if status.success())
}

#[cfg(not(target_os = "macos"))]
fn macos_is_accessibility_trusted() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn macos_request_accessibility_permission() -> bool {
    use core_foundation::{base::TCFType, boolean::CFBoolean, dictionary::CFDictionary, string::CFString};

    let prompt_key = CFString::new("AXTrustedCheckOptionPrompt");
    let prompt_true = CFBoolean::true_value();
    let options = CFDictionary::from_CFType_pairs(&[(prompt_key.as_CFType(), prompt_true.as_CFType())]);

    // SAFETY: AXIsProcessTrustedWithOptions reads the provided CFDictionary options.
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const c_void) }
}

#[cfg(not(target_os = "macos"))]
fn macos_request_accessibility_permission() -> bool {
    false
}

fn shell_escape_single_quoted(input: &str) -> String {
    input.replace('\'', "''")
}

#[cfg(target_os = "macos")]
fn macos_tcc_accessibility_allowed(app: &AppHandle) -> Option<bool> {
    let home = std::env::var("HOME").ok()?;
    let db_path = format!("{home}/Library/Application Support/com.apple.TCC/TCC.db");
    if !Path::new(&db_path).exists() {
        return None;
    }

    let bundle_id = app.config().identifier.as_str();
    let exe_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_default();

    let bundle_esc = shell_escape_single_quoted(bundle_id);
    let exe_esc = shell_escape_single_quoted(&exe_path);
    let base_where = format!(
        "service='kTCCServiceAccessibility' AND (client='{bundle_esc}' OR client='{exe_esc}')"
    );

    let query_auth = format!(
        "SELECT auth_value FROM access WHERE {base_where} ORDER BY last_modified DESC LIMIT 1;"
    );
    let query_allowed =
        format!("SELECT allowed FROM access WHERE {base_where} ORDER BY last_modified DESC LIMIT 1;");

    let run_query = |sql: &str| -> Option<String> {
        let output = Command::new("sqlite3")
            .arg(&db_path)
            .arg(sql)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Some(raw)
    };

    if let Some(value) = run_query(&query_auth) {
        if value.is_empty() {
            return Some(false);
        }
        return Some(matches!(value.as_str(), "1" | "2"));
    }

    if let Some(value) = run_query(&query_allowed) {
        if value.is_empty() {
            return Some(false);
        }
        return Some(matches!(value.as_str(), "1"));
    }

    None
}

#[cfg(not(target_os = "macos"))]
fn macos_tcc_accessibility_allowed(_app: &AppHandle) -> Option<bool> {
    None
}

#[tauri::command]
fn get_accessibility_status(app: AppHandle) -> AccessibilityStatus {
    let ax_trusted = macos_is_accessibility_trusted();
    let tcc_allowed = macos_tcc_accessibility_allowed(&app);
    let current_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let runtime_hint = if cfg!(target_os = "macos")
        && !current_exe.contains(".app/Contents/MacOS/")
        && (current_exe.contains("/target/debug/") || current_exe.contains("/target/release/"))
    {
        Some("Running in dev binary mode; accessibility may be tied to Terminal/iTerm rather than a bundled .app".into())
    } else {
        None
    };
    let trusted = if cfg!(target_os = "macos") {
        ax_trusted && tcc_allowed.unwrap_or(false)
    } else {
        false
    };
    AccessibilityStatus {
        supported: cfg!(target_os = "macos"),
        trusted,
        ax_trusted,
        tcc_allowed,
        runtime_hint,
    }
}

#[tauri::command]
fn request_accessibility_permission(app: AppHandle) -> AccessibilityStatus {
    if cfg!(target_os = "macos") {
        let _ = macos_request_accessibility_permission();
    }
    let ax_trusted = macos_is_accessibility_trusted();
    let tcc_allowed = macos_tcc_accessibility_allowed(&app);
    let current_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let runtime_hint = if cfg!(target_os = "macos")
        && !current_exe.contains(".app/Contents/MacOS/")
        && (current_exe.contains("/target/debug/") || current_exe.contains("/target/release/"))
    {
        Some("Running in dev binary mode; accessibility may be tied to Terminal/iTerm rather than a bundled .app".into())
    } else {
        None
    };
    let trusted = if cfg!(target_os = "macos") {
        ax_trusted && tcc_allowed.unwrap_or(false)
    } else {
        false
    };
    AccessibilityStatus {
        supported: cfg!(target_os = "macos"),
        trusted,
        ax_trusted,
        tcc_allowed,
        runtime_hint,
    }
}

#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("accessibility settings are only supported on macOS".into());
    }

    let mut status = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .status()
        .map_err(|e| format!("failed to open accessibility settings: {e}"))?;

    if !status.success() {
        status = Command::new("open")
            .arg("/System/Library/PreferencePanes/Security.prefPane")
            .status()
            .map_err(|e| format!("failed to open Security preferences: {e}"))?;
        if !status.success() {
            return Err("failed to open accessibility settings".into());
        }
    }

    Ok(())
}

fn escape_applescript_text(input: &str) -> String {
    let mut escaped = String::new();
    for ch in input.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\" & return & \""),
            '\r' => {}
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[tauri::command]
fn type_text_to_focused_app(text: String) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("type_text_to_focused_app is currently only supported on macOS".into());
    }
    if !macos_is_accessibility_trusted() {
        return Err("accessibility permission not granted".into());
    }

    let script = format!(
        "tell application \"System Events\" to keystroke \"{}\"",
        escape_applescript_text(&text)
    );
    let status = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status()
        .map_err(|e| format!("failed to run osascript: {e}"))?;
    if !status.success() {
        return Err("failed to type text via System Events".into());
    }
    Ok(())
}

fn ensure_overlay_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        return Ok(window);
    }

    let overlay = tauri::WebviewWindowBuilder::new(
        app,
        OVERLAY_WINDOW_LABEL,
        tauri::WebviewUrl::App("index.html#/overlay".into()),
    )
    .title("TypeMore Overlay")
    .decorations(false)
    .resizable(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .focusable(false)
    .focused(false)
    .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
    .position(120.0, 120.0)
    .shadow(false)
    .transparent(true)
    .visible(false)
    .build()
    .map_err(|e| format!("failed to create overlay window: {e}"))?;

    #[cfg(target_os = "macos")]
    #[allow(unexpected_cfgs)]
    {
        use objc::{msg_send, sel, sel_impl};
        let ns_window_ptr = overlay
            .ns_window()
            .map_err(|e| format!("failed to access overlay ns_window: {e}"))?;
        let ns_window = ns_window_ptr as *mut objc::runtime::Object;
        unsafe {
            // Native level tuning on macOS to keep overlay window more reliably above regular windows.
            let _: () = msg_send![ns_window, setLevel: 25_i64];
            let behavior: u64 = 1 | (1 << 8);
            let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
        }
    }

    let _ = place_overlay_window(app, &overlay);

    Ok(overlay)
}

fn place_overlay_window(app: &AppHandle, overlay: &tauri::WebviewWindow) -> Result<(), String> {
    let _ = overlay.set_size(Size::Logical(LogicalSize::new(OVERLAY_WIDTH, OVERLAY_HEIGHT)));

    let monitor = app
        .get_webview_window("main")
        .and_then(|w| w.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());

    if let Some(monitor) = monitor {
        let monitor_size = monitor.size();
        let monitor_pos = monitor.position();
        let width = OVERLAY_WIDTH.round() as i32;
        let height = OVERLAY_HEIGHT.round() as i32;
        let x = monitor_pos.x + ((monitor_size.width as i32 - width) / 2).max(0);
        let y = monitor_pos.y + (monitor_size.height as i32 - height - OVERLAY_BOTTOM_MARGIN).max(0);
        let _ = overlay.set_position(Position::Physical(PhysicalPosition::new(x, y)));
    }

    Ok(())
}

fn emit_overlay_state(app: &AppHandle, phase: &str, text: Option<String>) -> Result<(), String> {
    let overlay = ensure_overlay_window(app)?;
    let payload = OverlayStatePayload {
        phase: phase.to_string(),
        text,
    };

    if phase == "hidden" {
        overlay
            .emit(OVERLAY_EVENT, payload.clone())
            .map_err(|e| format!("failed to emit overlay state to overlay window: {e}"))?;
        let _ = app.emit(OVERLAY_EVENT, payload);
        let _ = overlay.hide();
        return Ok(());
    } else {
        let _ = place_overlay_window(app, &overlay);
        let _ = overlay.unminimize();
        let _ = overlay.show();
        let _ = overlay.set_always_on_top(true);
        let _ = overlay.set_visible_on_all_workspaces(true);
    }

    overlay
        .emit(OVERLAY_EVENT, payload.clone())
        .map_err(|e| format!("failed to emit overlay state to overlay window: {e}"))?;
    app.emit(OVERLAY_EVENT, payload)
        .map_err(|e| format!("failed to emit overlay state to app: {e}"))?;
    Ok(())
}

#[tauri::command]
fn get_global_shortcuts(app: AppHandle) -> Result<HotkeySettings, String> {
    let state = app.state::<AppState>();
    let lock = state
        .hotkeys
        .lock()
        .map_err(|_| "failed to read hotkey settings".to_string())?;
    Ok(HotkeySettings {
        toggle: lock.toggle.clone(),
    })
}

#[tauri::command]
fn set_global_shortcuts(app: AppHandle, toggle: String) -> Result<HotkeySettings, String> {
    let new_config = build_hotkey_config(&toggle)?;
    let applied = apply_toggle_shortcut(&app, new_config)?;

    save_persisted_hotkeys(
        &app,
        &PersistedHotkeySettings {
            toggle: applied.toggle.clone(),
        },
    )?;

    Ok(HotkeySettings {
        toggle: applied.toggle,
    })
}

#[tauri::command]
fn set_overlay_state(app: AppHandle, phase: String, text: Option<String>) -> Result<(), String> {
    emit_overlay_state(&app, &phase, text)
}

#[tauri::command]
fn hide_overlay(app: AppHandle) -> Result<(), String> {
    emit_overlay_state(&app, "hidden", None)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event: ShortcutEvent| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    let state = app.state::<AppState>();
                    let Ok(lock) = state.hotkeys.lock() else {
                        return;
                    };
                    let action = if shortcut.id() == lock.toggle_id {
                        "toggle-dictation"
                    } else {
                        return;
                    };
                    let _ = app.emit(
                        HOTKEY_EVENT,
                        GlobalShortcutPayload {
                            action: action.to_string(),
                            shortcut: shortcut.to_string(),
                        },
                    );
                })
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let desired_cfg = match load_persisted_hotkeys(app.handle())? {
                Some(saved) => match build_hotkey_config(&saved.toggle) {
                    Ok(cfg) => cfg,
                    Err(err) => {
                        eprintln!(
                            "[typemore] invalid persisted hotkey '{}', fallback to default: {}",
                            saved.toggle, err
                        );
                        build_hotkey_config(HOTKEY_TOGGLE_DICTATION)?
                    }
                },
                None => build_hotkey_config(HOTKEY_TOGGLE_DICTATION)?,
            };

            if let Err(err) = apply_toggle_shortcut(app.handle(), desired_cfg.clone()) {
                eprintln!(
                    "[typemore] failed to register hotkey '{}': {}. fallback to default",
                    desired_cfg.toggle, err
                );
                let fallback = build_hotkey_config(HOTKEY_TOGGLE_DICTATION)?;
                apply_toggle_shortcut(app.handle(), fallback.clone())?;
                save_persisted_hotkeys(
                    app.handle(),
                    &PersistedHotkeySettings {
                        toggle: fallback.toggle,
                    },
                )?;
            } else if desired_cfg.toggle != HOTKEY_TOGGLE_DICTATION {
                let _ = save_persisted_hotkeys(
                    app.handle(),
                    &PersistedHotkeySettings {
                        toggle: desired_cfg.toggle,
                    },
                );
            }

            if let Ok(lock) = app.state::<AppState>().hotkeys.lock() {
                eprintln!("[typemore] active hotkey: {}", lock.toggle);
            }

            if let Err(err) = ensure_overlay_window(app.handle()) {
                eprintln!("[typemore] failed to create overlay window: {}", err);
            } else if let Some(overlay) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
                let _ = overlay.once("tauri://webview-created", |_| {
                    eprintln!("[typemore] overlay webview created");
                });
                let _ = overlay.once("tauri://error", |_| {
                    eprintln!("[typemore] overlay webview error");
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_model_status,
            get_model_init_status,
            init_model,
            list_recordings,
            rename_recording,
            delete_recording,
            get_recording_cached_transcript,
            transcribe_recording,
            save_recording_and_transcribe,
            open_temp_dir,
            get_accessibility_status,
            request_accessibility_permission,
            open_accessibility_settings,
            type_text_to_focused_app,
            get_global_shortcuts,
            set_global_shortcuts,
            set_overlay_state,
            hide_overlay
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
