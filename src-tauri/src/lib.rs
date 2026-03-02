#![allow(unexpected_cfgs)]

use serde::{Deserialize, Serialize};
use sherpa_rs::{paraformer::ParaformerConfig, paraformer::ParaformerRecognizer};
use std::{
    collections::HashMap,
    ffi::c_void,
    fs,
    io::{BufWriter, Cursor, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
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
const HOTKEY_TOGGLE_TRANSLATION: &str = "CommandOrControl+Alt+Enter";
const OVERLAY_WINDOW_LABEL: &str = "overlay";
const OVERLAY_WIDTH: f64 = 210.0;
const OVERLAY_HEIGHT: f64 = 25.0;
const OVERLAY_BOTTOM_MARGIN: i32 = 150;
const OVERLAY_TOP_MARGIN: i32 = 90;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
}

#[cfg(target_os = "macos")]
fn start_macos_fn_key_monitor(app: &AppHandle) -> Result<(), String> {
    let app_handle = app.clone();
    std::thread::spawn(move || {
        // kCGEventSourceStateHIDSystemState
        const STATE_HID_SYSTEM: i32 = 1;
        // kCGEventSourceStateCombinedSessionState
        const STATE_COMBINED_SESSION: i32 = 0;
        // macOS virtual keycode for Fn/Globe.
        const KEYCODE_FN: u16 = 63;
        const KEYCODE_LEFT_SHIFT: u16 = 56;
        const KEYCODE_RIGHT_SHIFT: u16 = 60;

        let mut was_down = false;
        let mut active_action: Option<&'static str> = None;
        loop {
            let is_down = unsafe {
                CGEventSourceKeyState(STATE_HID_SYSTEM, KEYCODE_FN)
                    || CGEventSourceKeyState(STATE_COMBINED_SESSION, KEYCODE_FN)
            };
            let shift_down = unsafe {
                CGEventSourceKeyState(STATE_HID_SYSTEM, KEYCODE_LEFT_SHIFT)
                    || CGEventSourceKeyState(STATE_COMBINED_SESSION, KEYCODE_LEFT_SHIFT)
                    || CGEventSourceKeyState(STATE_HID_SYSTEM, KEYCODE_RIGHT_SHIFT)
                    || CGEventSourceKeyState(STATE_COMBINED_SESSION, KEYCODE_RIGHT_SHIFT)
            };
            if is_down && !was_down {
                let fn_enabled = app_handle
                    .state::<AppState>()
                    .fn_key_enabled
                    .lock()
                    .map(|v| *v)
                    .unwrap_or(false);
                if fn_enabled {
                    let action = if shift_down {
                        "toggle-translation"
                    } else {
                        "toggle-dictation"
                    };
                    active_action = Some(action);
                    let shortcut = if shift_down { "Fn+Shift" } else { "Fn" };
                    emit_hotkey_event(&app_handle, action, shortcut, "pressed");
                }
            } else if !is_down && was_down {
                if let Some(action) = active_action.take() {
                    emit_hotkey_event(&app_handle, action, "Fn", "released");
                }
            }
            was_down = is_down;
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });
    Ok(())
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
    runtime_hint: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GlobalShortcutPayload {
    action: String,
    shortcut: String,
    state: String,
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
    fn_key_enabled: Mutex<bool>,
    trigger_mode: Mutex<HotkeyTriggerMode>,
    overlay_position: Mutex<OverlayPosition>,
    output_mode: Mutex<OutputMode>,
    hotkey_runtime: Mutex<HotkeyRuntimeState>,
}

#[derive(Debug, Clone)]
struct HotkeyConfig {
    dictation: String,
    dictation_id: u32,
    translation: String,
    translation_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum HotkeyTriggerMode {
    Tap,
    LongPress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum OverlayPosition {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum OutputMode {
    AutoPaste,
    PasteAndKeep,
    CopyOnly,
}

#[derive(Debug, Clone)]
struct HotkeyRuntimeState {
    suppress_dictation_until: Option<Instant>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HotkeySettings {
    dictation: String,
    translation: String,
    fn_enabled: bool,
    trigger_mode: HotkeyTriggerMode,
    overlay_position: OverlayPosition,
    output_mode: OutputMode,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedHotkeySettings {
    #[serde(default = "default_hotkey_dictation", alias = "toggle")]
    dictation: String,
    #[serde(default = "default_hotkey_translation")]
    translation: String,
    #[serde(default = "default_fn_key_enabled")]
    fn_enabled: bool,
    #[serde(default = "default_trigger_mode")]
    trigger_mode: HotkeyTriggerMode,
    #[serde(default = "default_overlay_position")]
    overlay_position: OverlayPosition,
    #[serde(default = "default_output_mode")]
    output_mode: OutputMode,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct OverlayStatePayload {
    phase: String,
    text: Option<String>,
}

fn emit_hotkey_event(app: &AppHandle, action: &str, shortcut: &str, state: &str) {
    if state == "pressed" {
        let now = Instant::now();
        let state_guard = app.state::<AppState>();
        let Ok(mut runtime) = state_guard.hotkey_runtime.lock() else {
            return;
        };
        if action == "toggle-translation" {
            runtime.suppress_dictation_until =
                Some(now + std::time::Duration::from_millis(260));
        } else if action == "toggle-dictation"
            && runtime
                .suppress_dictation_until
                .is_some_and(|until| now < until)
        {
            return;
        }
    }

    let _ = app.emit(
        HOTKEY_EVENT,
        GlobalShortcutPayload {
            action: action.to_string(),
            shortcut: shortcut.to_string(),
            state: state.to_string(),
        },
    );
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            init_status: Mutex::new(ModelInitStatus::default()),
            hotkeys: Mutex::new(
                build_hotkey_config(HOTKEY_TOGGLE_DICTATION, HOTKEY_TOGGLE_TRANSLATION)
                    .expect("invalid default hotkeys"),
            ),
            fn_key_enabled: Mutex::new(default_fn_key_enabled()),
            trigger_mode: Mutex::new(default_trigger_mode()),
            overlay_position: Mutex::new(default_overlay_position()),
            output_mode: Mutex::new(default_output_mode()),
            hotkey_runtime: Mutex::new(HotkeyRuntimeState {
                suppress_dictation_until: None,
            }),
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

const fn default_fn_key_enabled() -> bool {
    true
}

fn default_hotkey_dictation() -> String {
    HOTKEY_TOGGLE_DICTATION.to_string()
}

fn default_hotkey_translation() -> String {
    HOTKEY_TOGGLE_TRANSLATION.to_string()
}

const fn default_trigger_mode() -> HotkeyTriggerMode {
    HotkeyTriggerMode::Tap
}

const fn default_overlay_position() -> OverlayPosition {
    OverlayPosition::Bottom
}

const fn default_output_mode() -> OutputMode {
    OutputMode::AutoPaste
}

fn build_hotkey_config(dictation: &str, translation: &str) -> Result<HotkeyConfig, String> {
    if dictation == translation {
        return Err("dictation and translation hotkeys must be different".into());
    }

    let dictation_shortcut: tauri_plugin_global_shortcut::Shortcut = dictation
        .parse()
        .map_err(|e| format!("invalid dictation shortcut: {e}"))?;
    let translation_shortcut: tauri_plugin_global_shortcut::Shortcut = translation
        .parse()
        .map_err(|e| format!("invalid translation shortcut: {e}"))?;

    Ok(HotkeyConfig {
        dictation: dictation.to_string(),
        dictation_id: dictation_shortcut.id(),
        translation: translation.to_string(),
        translation_id: translation_shortcut.id(),
    })
}

fn apply_hotkey_shortcuts(app: &AppHandle, new_config: HotkeyConfig) -> Result<HotkeyConfig, String> {
    let state = app.state::<AppState>();
    let old_config = {
        let lock = state
            .hotkeys
            .lock()
            .map_err(|_| "failed to read current hotkeys".to_string())?;
        lock.clone()
    };

    let manager = app.global_shortcut();
    if old_config.dictation != new_config.dictation
        && manager.is_registered(old_config.dictation.as_str())
    {
        manager
            .unregister(old_config.dictation.as_str())
            .map_err(|e| format!("failed to unregister old dictation shortcut: {e}"))?;
    }
    if old_config.translation != new_config.translation
        && manager.is_registered(old_config.translation.as_str())
    {
        manager
            .unregister(old_config.translation.as_str())
            .map_err(|e| format!("failed to unregister old translation shortcut: {e}"))?;
    }

    if !manager.is_registered(new_config.dictation.as_str()) {
        manager
            .register(new_config.dictation.as_str())
            .map_err(|e| format!("failed to register dictation shortcut: {e}"))?;
    }
    if !manager.is_registered(new_config.translation.as_str()) {
        manager
            .register(new_config.translation.as_str())
            .map_err(|e| format!("failed to register translation shortcut: {e}"))?;
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

fn collect_hotkey_settings(app: &AppHandle) -> Result<HotkeySettings, String> {
    let state = app.state::<AppState>();
    let (dictation, translation) = {
        let lock = state
            .hotkeys
            .lock()
            .map_err(|_| "failed to read hotkey settings".to_string())?;
        (lock.dictation.clone(), lock.translation.clone())
    };
    let fn_enabled = state
        .fn_key_enabled
        .lock()
        .map_err(|_| "failed to read fn key settings".to_string())
        .map(|v| *v)?;
    let trigger_mode = state
        .trigger_mode
        .lock()
        .map_err(|_| "failed to read trigger mode settings".to_string())
        .map(|v| *v)?;
    let overlay_position = state
        .overlay_position
        .lock()
        .map_err(|_| "failed to read overlay position settings".to_string())
        .map(|v| *v)?;
    let output_mode = state
        .output_mode
        .lock()
        .map_err(|_| "failed to read output mode settings".to_string())
        .map(|v| *v)?;

    Ok(HotkeySettings {
        dictation,
        translation,
        fn_enabled,
        trigger_mode,
        overlay_position,
        output_mode,
    })
}

fn save_current_hotkey_settings(app: &AppHandle) -> Result<(), String> {
    let current = collect_hotkey_settings(app)?;
    save_persisted_hotkeys(
        app,
        &PersistedHotkeySettings {
            dictation: current.dictation,
            translation: current.translation,
            fn_enabled: current.fn_enabled,
            trigger_mode: current.trigger_mode,
            overlay_position: current.overlay_position,
            output_mode: current.output_mode,
        },
    )
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

fn extract_google_translate_text(value: &serde_json::Value) -> Option<String> {
    let segments = value.get(0)?.as_array()?;
    let mut out = String::new();
    for seg in segments {
        if let Some(part) = seg.get(0).and_then(|v| v.as_str()) {
            out.push_str(part);
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[tauri::command]
fn translate_text_best_effort(text: String, target_lang: Option<String>) -> Result<String, String> {
    let input = text.trim();
    if input.is_empty() {
        return Ok(String::new());
    }
    let target = target_lang
        .unwrap_or_else(|| "en".to_string())
        .trim()
        .to_string();
    if target.is_empty() {
        return Err("target language cannot be empty".into());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("failed to build translate client: {e}"))?;
    let response = client
        .get("https://translate.googleapis.com/translate_a/single")
        .query(&[
            ("client", "gtx"),
            ("sl", "auto"),
            ("tl", target.as_str()),
            ("dt", "t"),
            ("q", input),
        ])
        .send()
        .map_err(|e| format!("translation request failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "translation request failed with status {}",
            response.status()
        ));
    }
    let raw = response
        .text()
        .map_err(|e| format!("failed to read translation response: {e}"))?;
    let json: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("failed to parse translation response: {e}"))?;
    extract_google_translate_text(&json).ok_or_else(|| "translation response was empty".into())
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

#[tauri::command]
fn get_accessibility_status() -> AccessibilityStatus {
    let ax_trusted = macos_is_accessibility_trusted();
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
    let trusted = if cfg!(target_os = "macos") { ax_trusted } else { false };
    AccessibilityStatus {
        supported: cfg!(target_os = "macos"),
        trusted,
        ax_trusted,
        runtime_hint,
    }
}

#[tauri::command]
fn request_accessibility_permission() -> AccessibilityStatus {
    if cfg!(target_os = "macos") {
        let _ = macos_request_accessibility_permission();
    }
    let ax_trusted = macos_is_accessibility_trusted();
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
    let trusted = if cfg!(target_os = "macos") { ax_trusted } else { false };
    AccessibilityStatus {
        supported: cfg!(target_os = "macos"),
        trusted,
        ax_trusted,
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

fn run_osascript(script: &str) -> Result<(), String> {
    let status = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status()
        .map_err(|e| format!("failed to run osascript: {e}"))?;
    if !status.success() {
        return Err("failed to run osascript command".into());
    }
    Ok(())
}

fn pbcopy_text(text: &str) -> Result<(), String> {
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start pbcopy: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("failed to write clipboard text: {e}"))?;
    } else {
        return Err("failed to access pbcopy stdin".into());
    }
    let status = child
        .wait()
        .map_err(|e| format!("failed waiting pbcopy: {e}"))?;
    if !status.success() {
        return Err("pbcopy exited with non-zero status".into());
    }
    Ok(())
}

fn type_text_via_paste(text: &str, keep_result_in_clipboard: bool, do_paste: bool) -> Result<(), String> {
    let previous_clipboard = Command::new("pbpaste")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).to_string());

    pbcopy_text(text)?;
    if do_paste {
        run_osascript("tell application \"System Events\" to keystroke \"v\" using command down")?;
    }

    if !keep_result_in_clipboard {
        if let Some(prev) = previous_clipboard {
            let _ = pbcopy_text(&prev);
        } else {
            let _ = pbcopy_text("");
        }
    }
    Ok(())
}

#[tauri::command]
fn type_text_to_focused_app(app: AppHandle, text: String) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("type_text_to_focused_app is currently only supported on macOS".into());
    }
    let output_mode = app
        .state::<AppState>()
        .output_mode
        .lock()
        .map_err(|_| "failed to read output mode settings".to_string())
        .map(|v| *v)?;

    let requires_accessibility = output_mode != OutputMode::CopyOnly;
    if requires_accessibility && !macos_is_accessibility_trusted() {
        return Err("accessibility permission not granted".into());
    }

    match output_mode {
        OutputMode::AutoPaste => type_text_via_paste(&text, false, true),
        OutputMode::PasteAndKeep => type_text_via_paste(&text, true, true),
        OutputMode::CopyOnly => type_text_via_paste(&text, true, false),
    }
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
    let overlay_position = app
        .state::<AppState>()
        .overlay_position
        .lock()
        .map_err(|_| "failed to read overlay position settings".to_string())
        .map(|v| *v)?;

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
        let y = match overlay_position {
            OverlayPosition::Bottom => {
                monitor_pos.y + (monitor_size.height as i32 - height - OVERLAY_BOTTOM_MARGIN).max(0)
            }
            OverlayPosition::Top => monitor_pos.y + OVERLAY_TOP_MARGIN.max(0),
        };
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

        #[cfg(target_os = "macos")]
        #[allow(unexpected_cfgs)]
        {
            use objc::{msg_send, sel, sel_impl};
            if let Ok(ns_window_ptr) = overlay.ns_window() {
                let ns_window = ns_window_ptr as *mut objc::runtime::Object;
                unsafe {
                    // Keep overlay visible even when the app is not frontmost.
                    let _: () = msg_send![ns_window, orderFrontRegardless];
                }
            }
        }
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
    collect_hotkey_settings(&app)
}

#[tauri::command]
fn set_global_shortcuts(
    app: AppHandle,
    dictation: String,
    translation: String,
    trigger_mode: HotkeyTriggerMode,
    overlay_position: OverlayPosition,
    output_mode: OutputMode,
) -> Result<HotkeySettings, String> {
    let new_config = build_hotkey_config(&dictation, &translation)?;
    let _applied = apply_hotkey_shortcuts(&app, new_config)?;
    {
        let state = app.state::<AppState>();
        let mut trigger_lock = state
            .trigger_mode
            .lock()
            .map_err(|_| "failed to update trigger mode settings".to_string())?;
        *trigger_lock = trigger_mode;
        drop(trigger_lock);
        let mut overlay_lock = state
            .overlay_position
            .lock()
            .map_err(|_| "failed to update overlay position settings".to_string())?;
        *overlay_lock = overlay_position;
        drop(overlay_lock);
        let mut output_lock = state
            .output_mode
            .lock()
            .map_err(|_| "failed to update output mode settings".to_string())?;
        *output_lock = output_mode;
    }

    save_current_hotkey_settings(&app)?;
    collect_hotkey_settings(&app)
}

#[tauri::command]
fn set_fn_key_enabled(app: AppHandle, enabled: bool) -> Result<HotkeySettings, String> {
    let state = app.state::<AppState>();
    {
        let mut lock = state
            .fn_key_enabled
            .lock()
            .map_err(|_| "failed to update fn key settings".to_string())?;
        *lock = enabled;
    }

    save_current_hotkey_settings(&app)?;
    collect_hotkey_settings(&app)
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
                    let state = match event.state {
                        ShortcutState::Pressed => "pressed",
                        ShortcutState::Released => "released",
                    };
                    let state_ref = app.state::<AppState>();
                    let Ok(lock) = state_ref.hotkeys.lock() else {
                        return;
                    };
                    let action = if shortcut.id() == lock.dictation_id {
                        "toggle-dictation"
                    } else if shortcut.id() == lock.translation_id {
                        "toggle-translation"
                    } else {
                        return;
                    };
                    emit_hotkey_event(app, action, &shortcut.to_string(), state);
                })
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let persisted = load_persisted_hotkeys(app.handle())?;
            let fn_enabled = persisted
                .as_ref()
                .map(|saved| saved.fn_enabled)
                .unwrap_or_else(default_fn_key_enabled);
            let trigger_mode = persisted
                .as_ref()
                .map(|saved| saved.trigger_mode)
                .unwrap_or_else(default_trigger_mode);
            let overlay_position = persisted
                .as_ref()
                .map(|saved| saved.overlay_position)
                .unwrap_or_else(default_overlay_position);
            let output_mode = persisted
                .as_ref()
                .map(|saved| saved.output_mode)
                .unwrap_or_else(default_output_mode);

            let desired_cfg = match persisted {
                Some(saved) => match build_hotkey_config(&saved.dictation, &saved.translation) {
                    Ok(cfg) => cfg,
                    Err(err) => {
                        eprintln!(
                            "[typemore] invalid persisted hotkeys ('{}', '{}'), fallback to default: {}",
                            saved.dictation, saved.translation, err
                        );
                        build_hotkey_config(HOTKEY_TOGGLE_DICTATION, HOTKEY_TOGGLE_TRANSLATION)?
                    }
                },
                None => build_hotkey_config(HOTKEY_TOGGLE_DICTATION, HOTKEY_TOGGLE_TRANSLATION)?,
            };

            if let Err(err) = apply_hotkey_shortcuts(app.handle(), desired_cfg.clone()) {
                eprintln!(
                    "[typemore] failed to register hotkeys ('{}', '{}'): {}. fallback to default",
                    desired_cfg.dictation, desired_cfg.translation, err
                );
                let fallback =
                    build_hotkey_config(HOTKEY_TOGGLE_DICTATION, HOTKEY_TOGGLE_TRANSLATION)?;
                apply_hotkey_shortcuts(app.handle(), fallback)?;
            }

            if let Ok(lock) = app.state::<AppState>().hotkeys.lock() {
                eprintln!(
                    "[typemore] active hotkeys: dictation='{}', translation='{}'",
                    lock.dictation, lock.translation
                );
            }
            if let Ok(mut lock) = app.state::<AppState>().fn_key_enabled.lock() {
                *lock = fn_enabled;
            }
            if let Ok(mut lock) = app.state::<AppState>().trigger_mode.lock() {
                *lock = trigger_mode;
            }
            if let Ok(mut lock) = app.state::<AppState>().overlay_position.lock() {
                *lock = overlay_position;
            }
            if let Ok(mut lock) = app.state::<AppState>().output_mode.lock() {
                *lock = output_mode;
            }
            let _ = save_current_hotkey_settings(app.handle());
            eprintln!("[typemore] fn key enabled: {}", fn_enabled);

            #[cfg(target_os = "macos")]
            if let Err(err) = start_macos_fn_key_monitor(app.handle()) {
                eprintln!("[typemore] failed to start fn monitor: {}", err);
            } else {
                eprintln!("[typemore] fn key monitor active");
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
            translate_text_best_effort,
            open_temp_dir,
            get_accessibility_status,
            request_accessibility_permission,
            open_accessibility_settings,
            type_text_to_focused_app,
            get_global_shortcuts,
            set_global_shortcuts,
            set_fn_key_enabled,
            set_overlay_state,
            hide_overlay
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
