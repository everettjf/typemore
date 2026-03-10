#![allow(unexpected_cfgs)]

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rig::{client::CompletionClient, completion::Prompt};
use serde::{Deserialize, Serialize};
use sherpa_rs::{paraformer::ParaformerConfig, paraformer::ParaformerRecognizer};
use std::{
    collections::HashMap,
    ffi::c_void,
    fs,
    io::{BufWriter, Cursor, Read, Seek, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Position, Size, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutEvent, ShortcutState};
use walkdir::WalkDir;

const MODEL_ARCHIVE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-paraformer-trilingual-zh-cantonese-en.tar.bz2";
const MODEL_DIR_NAME: &str = "sherpa-model";
const EXTRACTED_DIR_NAME: &str = "extracted";
const RECORDINGS_DIR_NAME: &str = "recordings";
const TEMP_DIR_NAME: &str = "tmp";
const TRANSCRIPT_CACHE_FILE: &str = "transcript_cache.json";
const DICTIONARY_WORDS_FILE: &str = "dictionary_words.json";
const INIT_EVENT: &str = "model-init-progress";
const HOTKEY_EVENT: &str = "global-shortcut-triggered";
const OVERLAY_EVENT: &str = "overlay-state";
const RECORDING_SAVED_EVENT: &str = "recording-saved";
const HOTKEY_TOGGLE_DICTATION: &str = "";
const HOTKEY_TOGGLE_TRANSLATION: &str = "";
const LEGACY_HOTKEY_DICTATION_V1: &str = "CommandOrControl+Alt+Space";
const LEGACY_HOTKEY_TRANSLATION_V1: &str = "CommandOrControl+Alt+Enter";
const LEGACY_HOTKEY_DICTATION_V2: &str = "CommandOrControl+Shift+S";
const LEGACY_HOTKEY_TRANSLATION_V2: &str = "CommandOrControl+Alt+Enter";
const OVERLAY_WINDOW_LABEL: &str = "overlay";
const OVERLAY_WIDTH: f64 = 140.0;
const OVERLAY_HEIGHT: f64 = 25.0;
const OVERLAY_BOTTOM_MARGIN: i32 = 150;
const OVERLAY_TOP_MARGIN: i32 = 90;
const MAX_RECORDING_SECS: u64 = 90;
const MAX_NON_IDLE_STUCK_SECS: u64 = 45;

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
        const DECIDE_WINDOW_MS: u128 = 220;

        let mut was_down = false;
        let mut active_action: Option<&'static str> = None;
        let mut press_started_at: Option<Instant> = None;
        let mut shift_seen = false;
        let mut prev_shift_down = false;
        let mut pressed_emitted = false;

        let choose_action = |shift_intent: bool,
                             fn_dictation_enabled: bool,
                             fn_translation_enabled: bool|
         -> Option<&'static str> {
            if shift_intent {
                if fn_translation_enabled {
                    Some("toggle-translation")
                } else {
                    None
                }
            } else if fn_dictation_enabled {
                Some("toggle-dictation")
            } else {
                None
            }
        };

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
            let fn_dictation_enabled = app_handle
                .state::<AppState>()
                .fn_dictation_enabled
                .lock()
                .map(|v| *v)
                .unwrap_or(false);
            let fn_translation_enabled = app_handle
                .state::<AppState>()
                .fn_translation_enabled
                .lock()
                .map(|v| *v)
                .unwrap_or(false);

            if is_down && !was_down {
                press_started_at = Some(Instant::now());
                shift_seen = shift_down;
                pressed_emitted = false;
                active_action = None;
                if fn_dictation_enabled || fn_translation_enabled {
                    let _ = emit_overlay_state(
                        &app_handle,
                        "listening",
                        Some(localize_text(&app_handle, "Listening", "Listening")),
                        Some(0.0),
                    );
                }
            } else if is_down && was_down {
                let shift_rising = !prev_shift_down && shift_down;
                shift_seen |= shift_down;
                if shift_rising && !pressed_emitted && fn_translation_enabled {
                    let action = "toggle-translation";
                    eprintln!("[typemore][fn] pressed action={} shift=true", action);
                    active_action = Some(action);
                    emit_hotkey_event(&app_handle, action, "Fn+Shift", "pressed");
                    handle_native_hotkey_event(&app_handle, action, "pressed");
                    pressed_emitted = true;
                }
                if !pressed_emitted
                    && press_started_at
                        .map(|t| t.elapsed().as_millis() >= DECIDE_WINDOW_MS)
                        .unwrap_or(false)
                {
                    let action =
                        choose_action(shift_seen, fn_dictation_enabled, fn_translation_enabled);
                    if let Some(action) = action {
                        eprintln!(
                            "[typemore][fn] pressed action={} shift={}",
                            action, shift_seen
                        );
                        active_action = Some(action);
                        let shortcut = if shift_seen { "Fn+Shift" } else { "Fn" };
                        emit_hotkey_event(&app_handle, action, shortcut, "pressed");
                        handle_native_hotkey_event(&app_handle, action, "pressed");
                    }
                    pressed_emitted = true;
                }
            } else if !is_down && was_down {
                // For very quick taps, emit press on release so tap mode still works.
                if !pressed_emitted {
                    let action =
                        choose_action(shift_seen || shift_down, fn_dictation_enabled, fn_translation_enabled);
                    if let Some(action) = action {
                        eprintln!(
                            "[typemore][fn] pressed action={} shift={}",
                            action,
                            shift_seen || shift_down
                        );
                        active_action = Some(action);
                        let shortcut = if shift_seen || shift_down { "Fn+Shift" } else { "Fn" };
                        emit_hotkey_event(&app_handle, action, shortcut, "pressed");
                        handle_native_hotkey_event(&app_handle, action, "pressed");
                    }
                }

                if let Some(action) = active_action.take() {
                    eprintln!("[typemore][fn] released action={}", action);
                    emit_hotkey_event(&app_handle, action, "Fn", "released");
                    handle_native_hotkey_event(&app_handle, action, "released");
                }
                press_started_at = None;
                shift_seen = false;
                pressed_emitted = false;
            }
            prev_shift_down = shift_down;
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordingCharStat {
    created_at_ms: u128,
    chars: usize,
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
            message: "Model not initialized".into(),
            ready: false,
            error: None,
        }
    }
}

struct AppState {
    init_status: Mutex<ModelInitStatus>,
    hotkeys: Mutex<HotkeyConfig>,
    fn_dictation_enabled: Mutex<bool>,
    fn_translation_enabled: Mutex<bool>,
    trigger_mode: Mutex<HotkeyTriggerMode>,
    overlay_position: Mutex<OverlayPosition>,
    output_mode: Mutex<OutputMode>,
    translation_target: Mutex<TranslationTargetLang>,
    ui_language: Mutex<UiLanguage>,
    cloud_settings: Mutex<CloudSettings>,
    dictionary_words: Mutex<Vec<String>>,
    hotkey_runtime: Mutex<HotkeyRuntimeState>,
    native_hotkey_session: Mutex<NativeHotkeySession>,
    native_recorder_tx: Mutex<Option<mpsc::Sender<NativeRecorderCommand>>>,
}

#[derive(Debug, Clone)]
struct HotkeyConfig {
    dictation: String,
    dictation_id: Option<u32>,
    translation: String,
    translation_id: Option<u32>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum TranslationTargetLang {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "en")]
    En,
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "ja")]
    Ja,
    #[serde(rename = "ko")]
    Ko,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum CloudVendor {
    Openai,
    Openrouter,
    Anthropic,
    Gemini,
    Groq,
    Deepseek,
    Mistral,
    Xai,
    Perplexity,
    Together,
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudProviderConfig {
    id: String,
    name: String,
    vendor: CloudVendor,
    model: String,
    api_key: String,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default = "default_provider_enabled")]
    enabled: bool,
    #[serde(default)]
    priority: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudPipelineConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    optimize_provider_id: String,
    #[serde(default)]
    translate_provider_id: String,
    #[serde(default = "default_cloud_target_language")]
    target_language: String,
    #[serde(default = "default_optimize_prompt")]
    optimize_prompt: String,
    #[serde(default = "default_translate_prompt")]
    translate_prompt: String,
    #[serde(default = "default_cloud_timeout_ms")]
    timeout_ms: u64,
    #[serde(default = "default_cloud_retries")]
    max_retries: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudSettings {
    #[serde(default)]
    providers: Vec<CloudProviderConfig>,
    #[serde(default)]
    pipeline: CloudPipelineConfig,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudProcessResult {
    final_text: String,
    stage: String,
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestCloudProviderInput {
    provider_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TestCloudProviderResult {
    ok: bool,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum UiLanguage {
    Zh,
    En,
}

#[derive(Debug, Clone)]
struct HotkeyRuntimeState {
    suppress_dictation_until: Option<Instant>,
}

struct NativeRecorder {
    stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeRecorderState {
    Idle,
    Starting,
    Recording,
    Stopping,
    Processing,
}

impl NativeRecorderState {
    const fn as_str(self) -> &'static str {
        match self {
            NativeRecorderState::Idle => "idle",
            NativeRecorderState::Starting => "starting",
            NativeRecorderState::Recording => "recording",
            NativeRecorderState::Stopping => "stopping",
            NativeRecorderState::Processing => "processing",
        }
    }
}

struct NativeHotkeySession {
    active_action: Option<String>,
    state: NativeRecorderState,
    state_since: Instant,
    recording_started_at: Option<Instant>,
}

impl Default for NativeHotkeySession {
    fn default() -> Self {
        Self {
            active_action: None,
            state: NativeRecorderState::Idle,
            state_since: Instant::now(),
            recording_started_at: None,
        }
    }
}

enum NativeRecorderCommand {
    Start { action: String },
    Stop { action: String },
    Reset { reason: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HotkeySettings {
    dictation: String,
    translation: String,
    fn_dictation_enabled: bool,
    fn_translation_enabled: bool,
    trigger_mode: HotkeyTriggerMode,
    overlay_position: OverlayPosition,
    output_mode: OutputMode,
    translation_target: TranslationTargetLang,
    ui_language: UiLanguage,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedHotkeySettings {
    #[serde(default = "default_hotkey_dictation", alias = "toggle")]
    dictation: String,
    #[serde(default = "default_hotkey_translation")]
    translation: String,
    #[serde(default = "default_fn_dictation_enabled")]
    fn_dictation_enabled: bool,
    #[serde(default = "default_fn_translation_enabled")]
    fn_translation_enabled: bool,
    #[serde(default = "default_trigger_mode")]
    trigger_mode: HotkeyTriggerMode,
    #[serde(default = "default_overlay_position")]
    overlay_position: OverlayPosition,
    #[serde(default = "default_output_mode")]
    output_mode: OutputMode,
    #[serde(default = "default_translation_target")]
    translation_target: TranslationTargetLang,
    #[serde(default = "default_ui_language")]
    ui_language: UiLanguage,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct OverlayStatePayload {
    phase: String,
    text: Option<String>,
    level: Option<f32>,
}

fn emit_hotkey_event(app: &AppHandle, action: &str, shortcut: &str, state: &str) {
    if state == "pressed" {
        let now = Instant::now();
        let state_guard = app.state::<AppState>();
        let Ok(mut runtime) = state_guard.hotkey_runtime.lock() else {
            return;
        };
        if action == "toggle-translation" {
            runtime.suppress_dictation_until = Some(now + std::time::Duration::from_millis(260));
        } else if action == "toggle-dictation"
            && runtime
                .suppress_dictation_until
                .is_some_and(|until| now < until)
        {
            return;
        }
    }

    eprintln!(
        "[typemore][hotkey] action={} shortcut={} state={}",
        action, shortcut, state
    );
    let _ = app.emit(
        HOTKEY_EVENT,
        GlobalShortcutPayload {
            action: action.to_string(),
            shortcut: shortcut.to_string(),
            state: state.to_string(),
        },
    );
}

fn emit_recording_saved_event(app: &AppHandle, recording: &RecordingItem) {
    let payload = serde_json::json!({
        "recording": recording,
    });
    let _ = app.emit(RECORDING_SAVED_EVENT, payload);
}

fn current_ui_language(app: &AppHandle) -> UiLanguage {
    app.state::<AppState>()
        .ui_language
        .lock()
        .map(|v| *v)
        .unwrap_or(default_ui_language())
}

fn localize_text(app: &AppHandle, zh: &str, en: &str) -> String {
    if current_ui_language(app) == UiLanguage::En {
        en.to_string()
    } else {
        zh.to_string()
    }
}

fn set_native_recorder_state(
    app: &AppHandle,
    state: NativeRecorderState,
    active_action: Option<String>,
    recording_started_at: Option<Instant>,
) {
    if let Ok(mut session) = app.state::<AppState>().native_hotkey_session.lock() {
        let from = session.state.as_str();
        let to = state.as_str();
        let action_before = session.active_action.clone().unwrap_or_else(|| "-".into());
        session.state = state;
        session.state_since = Instant::now();
        session.active_action = active_action;
        session.recording_started_at = recording_started_at;
        let action_after = session.active_action.clone().unwrap_or_else(|| "-".into());
        eprintln!(
            "[typemore][recorder] state {} -> {} action {} -> {}",
            from, to, action_before, action_after
        );
    }
}

fn reset_native_session_to_idle(app: &AppHandle) {
    set_native_recorder_state(app, NativeRecorderState::Idle, None, None);
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            init_status: Mutex::new(ModelInitStatus::default()),
            hotkeys: Mutex::new(
                build_hotkey_config(HOTKEY_TOGGLE_DICTATION, HOTKEY_TOGGLE_TRANSLATION)
                    .expect("invalid default hotkeys"),
            ),
            fn_dictation_enabled: Mutex::new(default_fn_dictation_enabled()),
            fn_translation_enabled: Mutex::new(default_fn_translation_enabled()),
            trigger_mode: Mutex::new(default_trigger_mode()),
            overlay_position: Mutex::new(default_overlay_position()),
            output_mode: Mutex::new(default_output_mode()),
            translation_target: Mutex::new(default_translation_target()),
            ui_language: Mutex::new(default_ui_language()),
            cloud_settings: Mutex::new(CloudSettings::default()),
            dictionary_words: Mutex::new(Vec::new()),
            hotkey_runtime: Mutex::new(HotkeyRuntimeState {
                suppress_dictation_until: None,
            }),
            native_hotkey_session: Mutex::new(NativeHotkeySession::default()),
            native_recorder_tx: Mutex::new(None),
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

const fn default_fn_dictation_enabled() -> bool {
    true
}

const fn default_fn_translation_enabled() -> bool {
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

const fn default_translation_target() -> TranslationTargetLang {
    TranslationTargetLang::Auto
}

const fn default_ui_language() -> UiLanguage {
    UiLanguage::Zh
}

const fn default_provider_enabled() -> bool {
    true
}

fn default_cloud_target_language() -> String {
    "en".to_string()
}

fn default_optimize_prompt() -> String {
    "You are an expert post-processor for mixed Chinese-English speech transcription.\nCorrect ASR mistakes, punctuation, spacing, and casing while preserving the original language of each segment.\nDo not translate, do not add facts, and do not remove meaningful words.\nKeep code snippets, proper nouns, and technical terms unchanged when possible.\nReturn only the corrected text.".to_string()
}

fn default_translate_prompt() -> String {
    "Translate the input text into {target_language}.\nPreserve meaning and tone. Return only translated text.".to_string()
}

const fn default_cloud_timeout_ms() -> u64 {
    10_000
}

const fn default_cloud_retries() -> u8 {
    1
}

impl Default for CloudPipelineConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            optimize_provider_id: String::new(),
            translate_provider_id: String::new(),
            target_language: default_cloud_target_language(),
            optimize_prompt: default_optimize_prompt(),
            translate_prompt: default_translate_prompt(),
            timeout_ms: default_cloud_timeout_ms(),
            max_retries: default_cloud_retries(),
        }
    }
}

impl Default for CloudSettings {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            pipeline: CloudPipelineConfig::default(),
        }
    }
}

fn build_hotkey_config(dictation: &str, translation: &str) -> Result<HotkeyConfig, String> {
    let dictation = dictation.trim();
    let translation = translation.trim();

    let dictation_set = !dictation.is_empty();
    let translation_set = !translation.is_empty();
    if dictation_set && translation_set && dictation == translation {
        return Err("dictation and translation hotkeys must be different".into());
    }

    let dictation_id = if dictation_set {
        let dictation_shortcut: tauri_plugin_global_shortcut::Shortcut = dictation
            .parse()
            .map_err(|e| format!("invalid dictation shortcut: {e}"))?;
        Some(dictation_shortcut.id())
    } else {
        None
    };
    let translation_id = if translation_set {
        let translation_shortcut: tauri_plugin_global_shortcut::Shortcut = translation
            .parse()
            .map_err(|e| format!("invalid translation shortcut: {e}"))?;
        Some(translation_shortcut.id())
    } else {
        None
    };

    Ok(HotkeyConfig {
        dictation: dictation.to_string(),
        dictation_id,
        translation: translation.to_string(),
        translation_id,
    })
}

fn is_legacy_default_hotkeys(dictation: &str, translation: &str) -> bool {
    (dictation == LEGACY_HOTKEY_DICTATION_V1 && translation == LEGACY_HOTKEY_TRANSLATION_V1)
        || (dictation == LEGACY_HOTKEY_DICTATION_V2 && translation == LEGACY_HOTKEY_TRANSLATION_V2)
}

fn apply_hotkey_shortcuts(
    app: &AppHandle,
    new_config: HotkeyConfig,
) -> Result<HotkeyConfig, String> {
    let state = app.state::<AppState>();
    let old_config = {
        let lock = state
            .hotkeys
            .lock()
            .map_err(|_| "failed to read current hotkeys".to_string())?;
        lock.clone()
    };

    let manager = app.global_shortcut();
    if !old_config.dictation.is_empty()
        && old_config.dictation != new_config.dictation
        && manager.is_registered(old_config.dictation.as_str())
    {
        manager
            .unregister(old_config.dictation.as_str())
            .map_err(|e| format!("failed to unregister old dictation shortcut: {e}"))?;
    }
    if !old_config.translation.is_empty()
        && old_config.translation != new_config.translation
        && manager.is_registered(old_config.translation.as_str())
    {
        manager
            .unregister(old_config.translation.as_str())
            .map_err(|e| format!("failed to unregister old translation shortcut: {e}"))?;
    }

    if !new_config.dictation.is_empty() && !manager.is_registered(new_config.dictation.as_str()) {
        manager
            .register(new_config.dictation.as_str())
            .map_err(|e| format!("failed to register dictation shortcut: {e}"))?;
    }
    if !new_config.translation.is_empty() && !manager.is_registered(new_config.translation.as_str())
    {
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
    let fn_dictation_enabled = state
        .fn_dictation_enabled
        .lock()
        .map_err(|_| "failed to read fn dictation settings".to_string())
        .map(|v| *v)?;
    let fn_translation_enabled = state
        .fn_translation_enabled
        .lock()
        .map_err(|_| "failed to read fn translation settings".to_string())
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
    let translation_target = state
        .translation_target
        .lock()
        .map_err(|_| "failed to read translation target settings".to_string())
        .map(|v| *v)?;
    let ui_language = state
        .ui_language
        .lock()
        .map_err(|_| "failed to read ui language settings".to_string())
        .map(|v| *v)?;

    Ok(HotkeySettings {
        dictation,
        translation,
        fn_dictation_enabled,
        fn_translation_enabled,
        trigger_mode,
        overlay_position,
        output_mode,
        translation_target,
        ui_language,
    })
}

fn save_current_hotkey_settings(app: &AppHandle) -> Result<(), String> {
    let current = collect_hotkey_settings(app)?;
    save_persisted_hotkeys(
        app,
        &PersistedHotkeySettings {
            dictation: current.dictation,
            translation: current.translation,
            fn_dictation_enabled: current.fn_dictation_enabled,
            fn_translation_enabled: current.fn_translation_enabled,
            trigger_mode: current.trigger_mode,
            overlay_position: current.overlay_position,
            output_mode: current.output_mode,
            translation_target: current.translation_target,
            ui_language: current.ui_language,
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
    let raw =
        fs::read_to_string(&path).map_err(|e| format!("failed to read hotkey settings: {e}"))?;
    let parsed = serde_json::from_str::<PersistedHotkeySettings>(&raw)
        .map_err(|e| format!("failed to parse hotkey settings: {e}"))?;
    Ok(Some(parsed))
}

fn save_persisted_hotkeys(
    app: &AppHandle,
    settings: &PersistedHotkeySettings,
) -> Result<(), String> {
    let path = hotkey_settings_file(app)?;
    let raw = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("failed to serialize hotkey settings: {e}"))?;
    fs::write(path, raw).map_err(|e| format!("failed to write hotkey settings: {e}"))
}

fn cloud_settings_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("cloud_settings.json"))
}

fn dictionary_words_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join(DICTIONARY_WORDS_FILE))
}

fn normalize_dictionary_words(words: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::<String>::new();
    let mut seen = std::collections::HashSet::<String>::new();
    for raw in words {
        let word = raw.trim();
        if word.is_empty() {
            continue;
        }
        let key = word.to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        normalized.push(word.to_string());
        if normalized.len() >= 200 {
            break;
        }
    }
    normalized
}

fn load_persisted_dictionary_words(app: &AppHandle) -> Result<Option<Vec<String>>, String> {
    let path = dictionary_words_file(app)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("failed to read dictionary words settings: {e}"))?;
    let parsed = serde_json::from_str::<Vec<String>>(&raw)
        .map_err(|e| format!("failed to parse dictionary words settings: {e}"))?;
    Ok(Some(normalize_dictionary_words(parsed)))
}

fn save_persisted_dictionary_words(app: &AppHandle, words: &[String]) -> Result<(), String> {
    let path = dictionary_words_file(app)?;
    let raw = serde_json::to_string_pretty(words)
        .map_err(|e| format!("failed to serialize dictionary words settings: {e}"))?;
    fs::write(path, raw).map_err(|e| format!("failed to write dictionary words settings: {e}"))
}

fn validate_cloud_settings(settings: &CloudSettings) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    for provider in &settings.providers {
        let id = provider.id.trim();
        if id.is_empty() {
            return Err("provider id cannot be empty".into());
        }
        if !ids.insert(id.to_string()) {
            return Err(format!("duplicate provider id: {id}"));
        }
        if provider.enabled && provider.model.trim().is_empty() {
            return Err(format!("provider {id} model cannot be empty"));
        }
        if provider.enabled
            && provider.vendor != CloudVendor::Ollama
            && provider.api_key.trim().is_empty()
        {
            return Err(format!("provider {id} api key cannot be empty"));
        }
    }

    let optimize_id = settings.pipeline.optimize_provider_id.trim();
    if !optimize_id.is_empty() && !ids.contains(optimize_id) {
        return Err(format!("optimize provider not found: {optimize_id}"));
    }

    let translate_id = settings.pipeline.translate_provider_id.trim();
    if !translate_id.is_empty() && !ids.contains(translate_id) {
        return Err(format!("translate provider not found: {translate_id}"));
    }

    if settings.pipeline.target_language.trim().is_empty() {
        return Err("target language cannot be empty".into());
    }

    Ok(())
}

fn load_persisted_cloud_settings(app: &AppHandle) -> Result<Option<CloudSettings>, String> {
    let path = cloud_settings_file(app)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(&path).map_err(|e| format!("failed to read cloud settings: {e}"))?;
    let parsed = serde_json::from_str::<CloudSettings>(&raw)
        .map_err(|e| format!("failed to parse cloud settings: {e}"))?;
    validate_cloud_settings(&parsed)?;
    Ok(Some(parsed))
}

fn save_persisted_cloud_settings(app: &AppHandle, settings: &CloudSettings) -> Result<(), String> {
    validate_cloud_settings(settings)?;
    let path = cloud_settings_file(app)?;
    let raw = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("failed to serialize cloud settings: {e}"))?;
    fs::write(path, raw).map_err(|e| format!("failed to write cloud settings: {e}"))
}

fn render_prompt_template(template: &str, text: &str, target_language: Option<&str>) -> String {
    let mut rendered = template.replace("{text}", text);
    if let Some(lang) = target_language {
        rendered = rendered.replace("{target_language}", lang);
    }
    if !rendered.contains(text) && !template.contains("{text}") {
        format!("{rendered}\n\nInput:\n{text}")
    } else {
        rendered
    }
}

fn append_dictionary_glossary(prompt: String, dictionary_words: &[String]) -> String {
    if dictionary_words.is_empty() {
        return prompt;
    }
    let mut rendered = prompt;
    rendered.push_str(
        "\n\nPreferred glossary (apply only when relevant, do not force unrelated terms):",
    );
    for word in dictionary_words.iter().take(120) {
        rendered.push_str("\n- ");
        rendered.push_str(word);
    }
    rendered
}

fn replace_case_insensitive(input: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return input.to_string();
    }
    if !input.is_ascii() || !from.is_ascii() || !to.is_ascii() {
        return input.replace(from, to);
    }
    let lower_input = input.to_ascii_lowercase();
    let lower_from = from.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut search_start = 0usize;
    let mut copied = 0usize;
    while let Some(found) = lower_input[search_start..].find(&lower_from) {
        let byte_idx = search_start + found;
        out.push_str(&input[copied..byte_idx]);
        out.push_str(to);
        let next = byte_idx + lower_from.len();
        search_start = next;
        copied = next;
    }
    out.push_str(&input[copied..]);
    out
}

fn split_camel_or_alnum_chunks(word: &str) -> Vec<String> {
    if word.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::<String>::new();
    let mut current = String::new();
    let chars = word.chars().collect::<Vec<_>>();
    for (idx, ch) in chars.iter().enumerate() {
        let prev = if idx > 0 { Some(chars[idx - 1]) } else { None };
        let next = chars.get(idx + 1).copied();
        let should_split = if let Some(prev_ch) = prev {
            (prev_ch.is_ascii_lowercase() && ch.is_ascii_uppercase())
                || (prev_ch.is_ascii_alphabetic()
                    && ch.is_ascii_digit()
                    && !current.is_empty())
                || (prev_ch.is_ascii_digit()
                    && ch.is_ascii_alphabetic()
                    && !current.is_empty())
                || (prev_ch.is_ascii_uppercase()
                    && ch.is_ascii_uppercase()
                    && next.is_some_and(|n| n.is_ascii_lowercase())
                    && !current.is_empty())
        } else {
            false
        };
        if should_split && !current.is_empty() {
            chunks.push(current.clone());
            current.clear();
        }
        current.push(*ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn apply_local_dictionary_terms(text: &str, dictionary_words: &[String]) -> String {
    let mut output = text.to_string();
    for word in dictionary_words {
        let normalized = word.trim();
        if normalized.is_empty() {
            continue;
        }
        output = replace_case_insensitive(&output, normalized, normalized);
        let chunks = split_camel_or_alnum_chunks(normalized);
        if chunks.len() > 1 {
            let lower_chunks = chunks
                .iter()
                .map(|s| s.to_lowercase())
                .collect::<Vec<_>>();
            for sep in [" ", "-", "_"] {
                let variant = lower_chunks.join(sep);
                output = replace_case_insensitive(&output, &variant, normalized);
            }
        }
    }
    output
}

async fn prompt_with_provider(
    provider: &CloudProviderConfig,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    match provider.vendor {
        CloudVendor::Openai => {
            let mut builder: rig::providers::openai::ClientBuilder =
                rig::providers::openai::Client::builder().api_key(provider.api_key.clone());
            if let Some(base_url) = provider.base_url.as_ref().filter(|v| !v.trim().is_empty()) {
                builder = builder.base_url(base_url);
            }
            let client: rig::providers::openai::Client = builder
                .build()
                .map_err(|e| format!("openai client init failed: {e}"))?;
            let agent = client
                .agent(provider.model.clone())
                .preamble(system_prompt)
                .build();
            agent
                .prompt(user_prompt)
                .await
                .map_err(|e| format!("openai prompt failed: {e}"))
        }
        CloudVendor::Openrouter => {
            let mut builder: rig::providers::openrouter::ClientBuilder =
                rig::providers::openrouter::Client::builder().api_key(provider.api_key.clone());
            if let Some(base_url) = provider.base_url.as_ref().filter(|v| !v.trim().is_empty()) {
                builder = builder.base_url(base_url);
            }
            let client: rig::providers::openrouter::Client = builder
                .build()
                .map_err(|e| format!("openrouter client init failed: {e}"))?;
            let agent = client
                .agent(provider.model.clone())
                .preamble(system_prompt)
                .build();
            agent
                .prompt(user_prompt)
                .await
                .map_err(|e| format!("openrouter prompt failed: {e}"))
        }
        CloudVendor::Anthropic => {
            let mut builder: rig::providers::anthropic::ClientBuilder =
                rig::providers::anthropic::Client::builder().api_key(provider.api_key.clone());
            if let Some(base_url) = provider.base_url.as_ref().filter(|v| !v.trim().is_empty()) {
                builder = builder.base_url(base_url);
            }
            let client: rig::providers::anthropic::Client = builder
                .build()
                .map_err(|e| format!("anthropic client init failed: {e}"))?;
            let agent = client
                .agent(provider.model.clone())
                .preamble(system_prompt)
                .build();
            agent
                .prompt(user_prompt)
                .await
                .map_err(|e| format!("anthropic prompt failed: {e}"))
        }
        CloudVendor::Gemini => {
            let mut builder: rig::providers::gemini::client::ClientBuilder =
                rig::providers::gemini::Client::builder().api_key(provider.api_key.clone());
            if let Some(base_url) = provider.base_url.as_ref().filter(|v| !v.trim().is_empty()) {
                builder = builder.base_url(base_url);
            }
            let client: rig::providers::gemini::Client = builder
                .build()
                .map_err(|e| format!("gemini client init failed: {e}"))?;
            let agent = client
                .agent(provider.model.clone())
                .preamble(system_prompt)
                .build();
            agent
                .prompt(user_prompt)
                .await
                .map_err(|e| format!("gemini prompt failed: {e}"))
        }
        CloudVendor::Groq => {
            let mut builder =
                rig::providers::groq::Client::builder().api_key(provider.api_key.as_str());
            if let Some(base_url) = provider.base_url.as_ref().filter(|v| !v.trim().is_empty()) {
                builder = builder.base_url(base_url);
            }
            let client: rig::providers::groq::Client = builder
                .build()
                .map_err(|e| format!("groq client init failed: {e}"))?;
            let agent = client
                .agent(provider.model.clone())
                .preamble(system_prompt)
                .build();
            agent
                .prompt(user_prompt)
                .await
                .map_err(|e| format!("groq prompt failed: {e}"))
        }
        CloudVendor::Deepseek => {
            let mut builder =
                rig::providers::deepseek::Client::builder().api_key(provider.api_key.as_str());
            if let Some(base_url) = provider.base_url.as_ref().filter(|v| !v.trim().is_empty()) {
                builder = builder.base_url(base_url);
            }
            let client: rig::providers::deepseek::Client = builder
                .build()
                .map_err(|e| format!("deepseek client init failed: {e}"))?;
            let agent = client
                .agent(provider.model.clone())
                .preamble(system_prompt)
                .build();
            agent
                .prompt(user_prompt)
                .await
                .map_err(|e| format!("deepseek prompt failed: {e}"))
        }
        CloudVendor::Mistral => {
            let mut builder =
                rig::providers::mistral::Client::builder().api_key(provider.api_key.as_str());
            if let Some(base_url) = provider.base_url.as_ref().filter(|v| !v.trim().is_empty()) {
                builder = builder.base_url(base_url);
            }
            let client: rig::providers::mistral::Client = builder
                .build()
                .map_err(|e| format!("mistral client init failed: {e}"))?;
            let agent = client
                .agent(provider.model.clone())
                .preamble(system_prompt)
                .build();
            agent
                .prompt(user_prompt)
                .await
                .map_err(|e| format!("mistral prompt failed: {e}"))
        }
        CloudVendor::Xai => {
            let mut builder: rig::providers::xai::client::ClientBuilder =
                rig::providers::xai::Client::builder().api_key(provider.api_key.clone());
            if let Some(base_url) = provider.base_url.as_ref().filter(|v| !v.trim().is_empty()) {
                builder = builder.base_url(base_url);
            }
            let client: rig::providers::xai::Client = builder
                .build()
                .map_err(|e| format!("xai client init failed: {e}"))?;
            let agent = client
                .agent(provider.model.clone())
                .preamble(system_prompt)
                .build();
            agent
                .prompt(user_prompt)
                .await
                .map_err(|e| format!("xai prompt failed: {e}"))
        }
        CloudVendor::Perplexity => {
            let mut builder: rig::providers::perplexity::ClientBuilder =
                rig::providers::perplexity::Client::builder().api_key(provider.api_key.clone());
            if let Some(base_url) = provider.base_url.as_ref().filter(|v| !v.trim().is_empty()) {
                builder = builder.base_url(base_url);
            }
            let client: rig::providers::perplexity::Client = builder
                .build()
                .map_err(|e| format!("perplexity client init failed: {e}"))?;
            let agent = client
                .agent(provider.model.clone())
                .preamble(system_prompt)
                .build();
            agent
                .prompt(user_prompt)
                .await
                .map_err(|e| format!("perplexity prompt failed: {e}"))
        }
        CloudVendor::Together => {
            let mut builder: rig::providers::together::client::ClientBuilder =
                rig::providers::together::Client::builder().api_key(provider.api_key.clone());
            if let Some(base_url) = provider.base_url.as_ref().filter(|v| !v.trim().is_empty()) {
                builder = builder.base_url(base_url);
            }
            let client: rig::providers::together::Client = builder
                .build()
                .map_err(|e| format!("together client init failed: {e}"))?;
            let agent = client
                .agent(provider.model.clone())
                .preamble(system_prompt)
                .build();
            agent
                .prompt(user_prompt)
                .await
                .map_err(|e| format!("together prompt failed: {e}"))
        }
        CloudVendor::Ollama => {
            let mut builder: rig::providers::ollama::ClientBuilder =
                rig::providers::ollama::Client::builder().api_key(rig::client::Nothing);
            if let Some(base_url) = provider.base_url.as_ref().filter(|v| !v.trim().is_empty()) {
                builder = builder.base_url(base_url);
            }
            let client: rig::providers::ollama::Client = builder
                .build()
                .map_err(|e| format!("ollama client init failed: {e}"))?;
            let agent = client
                .agent(provider.model.clone())
                .preamble(system_prompt)
                .build();
            agent
                .prompt(user_prompt)
                .await
                .map_err(|e| format!("ollama prompt failed: {e}"))
        }
    }
}

fn call_provider_with_retry(
    provider: &CloudProviderConfig,
    system_prompt: &str,
    user_prompt: &str,
    max_retries: u8,
) -> Result<String, String> {
    let mut last_err = String::new();
    for attempt in 0..=max_retries {
        match tauri::async_runtime::block_on(prompt_with_provider(
            provider,
            system_prompt,
            user_prompt,
        )) {
            Ok(v) => return Ok(v.trim().to_string()),
            Err(err) => {
                last_err = err;
                eprintln!(
                    "[typemore][cloud] provider={} attempt={} failed: {}",
                    provider.id,
                    attempt + 1,
                    last_err
                );
            }
        }
    }
    Err(last_err)
}

fn resolve_provider<'a>(
    settings: &'a CloudSettings,
    provider_id: &str,
) -> Option<&'a CloudProviderConfig> {
    if provider_id.is_empty() {
        return None;
    }
    settings
        .providers
        .iter()
        .find(|p| p.enabled && p.id == provider_id)
}

fn run_cloud_pipeline(
    app: &AppHandle,
    source_text: &str,
    translate: bool,
    target_lang_override: Option<String>,
    mut on_stage: Option<&mut dyn FnMut(&str)>,
) -> CloudProcessResult {
    let source = source_text.trim();
    if source.is_empty() {
        return CloudProcessResult {
            final_text: String::new(),
            stage: "local".into(),
            warnings: Vec::new(),
        };
    }

    let settings = app
        .state::<AppState>()
        .cloud_settings
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default();

    if !settings.pipeline.enabled {
        return CloudProcessResult {
            final_text: source.to_string(),
            stage: "local".into(),
            warnings: Vec::new(),
        };
    }

    let optimize_provider =
        match resolve_provider(&settings, settings.pipeline.optimize_provider_id.trim()) {
            Some(v) => v,
            None => {
                return CloudProcessResult {
                    final_text: source.to_string(),
                    stage: "local".into(),
                    warnings: vec!["optimize provider not configured or disabled".into()],
                }
            }
        };

    let optimize_user = render_prompt_template(&settings.pipeline.optimize_prompt, source, None);
    let dictionary_words = app
        .state::<AppState>()
        .dictionary_words
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default();
    let optimize_user = append_dictionary_glossary(optimize_user, &dictionary_words);
    if let Some(cb) = on_stage.as_mut() {
        cb("optimizing");
    }
    let optimized = match call_provider_with_retry(
        optimize_provider,
        "You improve speech transcription text and return plain text only.",
        &optimize_user,
        settings.pipeline.max_retries,
    ) {
        Ok(text) if !text.trim().is_empty() => text,
        Ok(_) => source.to_string(),
        Err(err) => {
            return CloudProcessResult {
                final_text: source.to_string(),
                stage: "local".into(),
                warnings: vec![format!("optimize failed: {err}")],
            }
        }
    };

    if !translate {
        return CloudProcessResult {
            final_text: optimized,
            stage: "optimized".into(),
            warnings: Vec::new(),
        };
    }

    let target = target_lang_override
        .unwrap_or_else(|| settings.pipeline.target_language.clone())
        .trim()
        .to_string();
    if target.is_empty() {
        return CloudProcessResult {
            final_text: optimized,
            stage: "optimized".into(),
            warnings: vec!["target language is empty".into()],
        };
    }

    let translate_provider = if settings.pipeline.translate_provider_id.trim().is_empty() {
        optimize_provider
    } else {
        match resolve_provider(&settings, settings.pipeline.translate_provider_id.trim()) {
            Some(v) => v,
            None => {
                return CloudProcessResult {
                    final_text: optimized,
                    stage: "optimized".into(),
                    warnings: vec!["translate provider not configured or disabled".into()],
                };
            }
        }
    };

    let translate_user = render_prompt_template(
        &settings.pipeline.translate_prompt,
        &optimized,
        Some(target.as_str()),
    );
    if let Some(cb) = on_stage.as_mut() {
        cb("translating");
    }
    match call_provider_with_retry(
        translate_provider,
        "You are a professional translator. Return translated plain text only.",
        &translate_user,
        settings.pipeline.max_retries,
    ) {
        Ok(translated) if !translated.trim().is_empty() => CloudProcessResult {
            final_text: translated,
            stage: "translated".into(),
            warnings: Vec::new(),
        },
        Ok(_) => CloudProcessResult {
            final_text: optimized,
            stage: "optimized".into(),
            warnings: vec!["translation returned empty response".into()],
        },
        Err(err) => CloudProcessResult {
            final_text: optimized,
            stage: "optimized".into(),
            warnings: vec![format!("translate failed: {err}")],
        },
    }
}

fn recordings_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join(RECORDINGS_DIR_NAME);
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create recordings dir: {e}"))?;
    Ok(dir)
}

fn dictionary_words_snapshot(app: &AppHandle) -> Vec<String> {
    app.state::<AppState>()
        .dictionary_words
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default()
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
    app: &AppHandle,
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
            let should_emit =
                p >= 1.0 || (p - last_ratio) >= 0.003 || last_emit.elapsed().as_millis() >= 120;
            if should_emit {
                on_progress(
                    80.0 * p,
                    if current_ui_language(app) == UiLanguage::En {
                        format!(
                            "Downloading model... {:.1}% ({:.1} MB / {:.1} MB)",
                            p * 100.0,
                            downloaded as f32 / 1_048_576.0,
                            all as f32 / 1_048_576.0
                        )
                    } else {
                        format!(
                            "下载模型中... {:.1}% ({:.1} MB / {:.1} MB)",
                            p * 100.0,
                            downloaded as f32 / 1_048_576.0,
                            all as f32 / 1_048_576.0
                        )
                    },
                );
                last_emit = Instant::now();
                last_ratio = p;
            }
            continue;
        } else {
            if current_ui_language(app) == UiLanguage::En {
                format!(
                    "Downloading model... {:.1} MB",
                    downloaded as f32 / 1_048_576.0
                )
            } else {
                format!("下载模型中... {:.1} MB", downloaded as f32 / 1_048_576.0)
            }
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
                message: localize_text(app, "模型已就绪", "Model is ready"),
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
                message: localize_text(app, "开始下载模型", "Starting model download"),
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
        download_file_with_progress(
            app,
            MODEL_ARCHIVE_URL,
            &archive_path,
            &mut progress_callback,
        )?;
    } else {
        set_init_status(
            app,
            ModelInitStatus {
                running: true,
                phase: "download".into(),
                progress: 80.0,
                message: localize_text(
                    app,
                    "检测到已下载模型包，跳过下载",
                    "Detected existing model archive, skipping download",
                ),
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
            message: localize_text(app, "正在解压模型", "Extracting model files"),
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
            message: localize_text(app, "正在校验模型文件", "Validating model files"),
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
            message: localize_text(app, "模型初始化完成", "Model initialization complete"),
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

fn emit_native_listening_level(
    app: &AppHandle,
    last_emit: &Arc<Mutex<Instant>>,
    smooth_level: &Arc<Mutex<f32>>,
    rms: f32,
) {
    let scaled = (rms * 5.0).clamp(0.0, 1.0);
    let smoothed = if let Ok(mut smooth) = smooth_level.lock() {
        *smooth = (*smooth * 0.72) + (scaled * 0.28);
        *smooth
    } else {
        scaled
    };
    let gated = if smoothed < 0.035 { 0.0 } else { smoothed };
    let should_emit = if let Ok(mut last) = last_emit.lock() {
        if last.elapsed() >= Duration::from_millis(95) {
            *last = Instant::now();
            true
        } else {
            false
        }
    } else {
        false
    };
    if should_emit {
        let _ = emit_overlay_state(app, "listening", None, Some(gated));
    }
}

fn start_native_recording_internal(
    recorder: &mut Option<NativeRecorder>,
    app_handle: &AppHandle,
) -> Result<(), String> {
    if recorder.is_some() {
        return Ok(());
    }

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "no default input device".to_string())?;
    let config = device
        .default_input_config()
        .map_err(|e| format!("failed to get default input config: {e}"))?;

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
    let samples_ref = Arc::clone(&samples);
    let level_last_emit = Arc::new(Mutex::new(Instant::now() - Duration::from_millis(200)));
    let level_smooth = Arc::new(Mutex::new(0.0f32));
    let err_fn = |err| eprintln!("[typemore] native recording stream error: {err}");

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let level_app = app_handle.clone();
            let level_last_emit = Arc::clone(&level_last_emit);
            let level_smooth = Arc::clone(&level_smooth);
            device
                .build_input_stream(
                    &config.config(),
                    move |data: &[f32], _| {
                        if data.is_empty() {
                            return;
                        }
                        let mut sum = 0.0f32;
                        for sample in data {
                            sum += *sample * *sample;
                        }
                        let rms = (sum / data.len() as f32).sqrt();
                        emit_native_listening_level(
                            &level_app,
                            &level_last_emit,
                            &level_smooth,
                            rms,
                        );
                        if let Ok(mut buf) = samples_ref.lock() {
                            buf.extend_from_slice(data);
                        }
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("failed to build f32 input stream: {e}"))?
        }
        cpal::SampleFormat::I16 => {
            let samples_ref = Arc::clone(&samples);
            let level_app = app_handle.clone();
            let level_last_emit = Arc::clone(&level_last_emit);
            let level_smooth = Arc::clone(&level_smooth);
            device
                .build_input_stream(
                    &config.config(),
                    move |data: &[i16], _| {
                        if data.is_empty() {
                            return;
                        }
                        let mut sum = 0.0f32;
                        if let Ok(mut buf) = samples_ref.lock() {
                            for value in data {
                                let sample = *value as f32 / i16::MAX as f32;
                                sum += sample * sample;
                                buf.push(sample);
                            }
                        } else {
                            for value in data {
                                let sample = *value as f32 / i16::MAX as f32;
                                sum += sample * sample;
                            }
                        }
                        let rms = (sum / data.len() as f32).sqrt();
                        emit_native_listening_level(
                            &level_app,
                            &level_last_emit,
                            &level_smooth,
                            rms,
                        );
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("failed to build i16 input stream: {e}"))?
        }
        cpal::SampleFormat::U16 => {
            let samples_ref = Arc::clone(&samples);
            let level_app = app_handle.clone();
            let level_last_emit = Arc::clone(&level_last_emit);
            let level_smooth = Arc::clone(&level_smooth);
            device
                .build_input_stream(
                    &config.config(),
                    move |data: &[u16], _| {
                        if data.is_empty() {
                            return;
                        }
                        let mut sum = 0.0f32;
                        if let Ok(mut buf) = samples_ref.lock() {
                            for value in data {
                                let sample = (*value as f32 / u16::MAX as f32) * 2.0 - 1.0;
                                sum += sample * sample;
                                buf.push(sample);
                            }
                        } else {
                            for value in data {
                                let sample = (*value as f32 / u16::MAX as f32) * 2.0 - 1.0;
                                sum += sample * sample;
                            }
                        }
                        let rms = (sum / data.len() as f32).sqrt();
                        emit_native_listening_level(
                            &level_app,
                            &level_last_emit,
                            &level_smooth,
                            rms,
                        );
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("failed to build u16 input stream: {e}"))?
        }
        format => return Err(format!("unsupported sample format: {format:?}")),
    };

    stream
        .play()
        .map_err(|e| format!("failed to start input stream: {e}"))?;
    *recorder = Some(NativeRecorder {
        stream,
        samples,
        sample_rate,
        channels,
    });
    Ok(())
}

fn stop_native_recording_internal(
    recorder: &mut Option<NativeRecorder>,
) -> Result<(Vec<f32>, u32, u16), String> {
    let recorder = recorder
        .take()
        .ok_or_else(|| "native recorder not active".to_string())?;
    drop(recorder.stream);
    let samples = recorder
        .samples
        .lock()
        .map_err(|_| "failed to read native recorder samples".to_string())?
        .clone();
    Ok((samples, recorder.sample_rate, recorder.channels))
}

fn mix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let channels_usize = channels as usize;
    samples
        .chunks(channels_usize)
        .map(|chunk| chunk.iter().copied().sum::<f32>() / chunk.len() as f32)
        .collect()
}

fn encode_wav_i16_mono(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, String> {
    let mut cursor = Cursor::new(Vec::<u8>::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| format!("wav init failed: {e}"))?;
        for sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            let v = (clamped * i16::MAX as f32).round() as i16;
            writer
                .write_sample(v)
                .map_err(|e| format!("wav sample write failed: {e}"))?;
        }
        writer
            .finalize()
            .map_err(|e| format!("wav finalize failed: {e}"))?;
    }
    cursor
        .rewind()
        .map_err(|e| format!("wav rewind failed: {e}"))?;
    Ok(cursor.into_inner())
}

fn persist_recording_with_text(
    app: &AppHandle,
    wav_data: &[u8],
    text: &str,
    suggested_prefix: &str,
) -> Result<(), String> {
    let dir = recordings_dir(app)?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let base_name = sanitize_filename(suggested_prefix);
    let file_name = format!("{now_ms}_{base_name}.wav");
    let path = dir.join(file_name);
    fs::write(&path, wav_data).map_err(|e| format!("failed to save wav file: {e}"))?;
    let recording =
        to_recording_item(&path).ok_or_else(|| "failed to build recording metadata".to_string())?;
    put_cached_transcript(app, &recording.id, text)?;
    emit_recording_saved_event(app, &recording);
    Ok(())
}

fn infer_translation_target(text: &str) -> &'static str {
    let has_cjk = text.chars().any(|ch| {
        ('\u{3040}'..='\u{30ff}').contains(&ch)
            || ('\u{3400}'..='\u{9fff}').contains(&ch)
            || ('\u{f900}'..='\u{faff}').contains(&ch)
    });
    if has_cjk {
        "en"
    } else {
        "zh-CN"
    }
}

fn resolve_translation_target(app: &AppHandle, text: &str) -> &'static str {
    let mode = app
        .state::<AppState>()
        .translation_target
        .lock()
        .map(|v| *v)
        .unwrap_or(default_translation_target());
    match mode {
        TranslationTargetLang::Auto => infer_translation_target(text),
        TranslationTargetLang::En => "en",
        TranslationTargetLang::ZhCn => "zh-CN",
        TranslationTargetLang::Ja => "ja",
        TranslationTargetLang::Ko => "ko",
    }
}

fn type_text_to_focused_app_impl(app: &AppHandle, text: &str) -> Result<(), String> {
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
        OutputMode::AutoPaste => type_text_via_paste(text, false, true),
        OutputMode::PasteAndKeep => type_text_via_paste(text, true, true),
        OutputMode::CopyOnly => type_text_via_paste(text, true, false),
    }
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
            status.message = localize_text(&app, "模型已就绪", "Model is ready");
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
            message: localize_text(&app, "模型已就绪", "Model is ready"),
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
        message: localize_text(&app, "初始化任务已启动", "Initialization task started"),
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
                    message: localize_text(
                        &app_handle,
                        "模型初始化失败",
                        "Model initialization failed",
                    ),
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
fn list_recording_char_stats(app: AppHandle) -> Result<Vec<RecordingCharStat>, String> {
    let dir = recordings_dir(&app)?;
    let items = collect_recordings(&dir)?;
    let cache = load_transcript_cache(&app)?;
    let stats = items
        .into_iter()
        .map(|item| RecordingCharStat {
            created_at_ms: item.created_at_ms,
            chars: cache
                .get(&item.id)
                .map(|text| text.text.chars().count())
                .unwrap_or(0),
        })
        .collect::<Vec<_>>();
    Ok(stats)
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
    let text = apply_local_dictionary_terms(&text, &dictionary_words_snapshot(&app));
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
    let text = apply_local_dictionary_terms(&text, &dictionary_words_snapshot(&app));

    let recording =
        to_recording_item(&path).ok_or_else(|| "failed to build recording metadata".to_string())?;
    put_cached_transcript(&app, &recording.id, &text)?;
    emit_recording_saved_event(&app, &recording);

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
    use core_foundation::{
        base::TCFType, boolean::CFBoolean, dictionary::CFDictionary, string::CFString,
    };

    let prompt_key = CFString::new("AXTrustedCheckOptionPrompt");
    let prompt_true = CFBoolean::true_value();
    let options =
        CFDictionary::from_CFType_pairs(&[(prompt_key.as_CFType(), prompt_true.as_CFType())]);

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
    let trusted = if cfg!(target_os = "macos") {
        ax_trusted
    } else {
        false
    };
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
    let trusted = if cfg!(target_os = "macos") {
        ax_trusted
    } else {
        false
    };
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

fn type_text_via_paste(
    text: &str,
    keep_result_in_clipboard: bool,
    do_paste: bool,
) -> Result<(), String> {
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
        if do_paste {
            std::thread::sleep(Duration::from_millis(180));
        }
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
    type_text_to_focused_app_impl(&app, &text)
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
    let _ = overlay.set_size(Size::Logical(LogicalSize::new(
        OVERLAY_WIDTH,
        OVERLAY_HEIGHT,
    )));
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

fn emit_overlay_state(
    app: &AppHandle,
    phase: &str,
    text: Option<String>,
    level: Option<f32>,
) -> Result<(), String> {
    let payload = OverlayStatePayload {
        phase: phase.to_string(),
        text,
        level,
    };
    let app_handle = app.clone();
    app.run_on_main_thread(move || {
        if let Err(err) = emit_overlay_state_on_main_thread(&app_handle, payload) {
            eprintln!("[typemore] overlay update failed: {}", err);
        }
    })
    .map_err(|e| format!("failed to schedule overlay update on main thread: {e}"))
}

fn emit_overlay_state_on_main_thread(
    app: &AppHandle,
    payload: OverlayStatePayload,
) -> Result<(), String> {
    let overlay = ensure_overlay_window(app)?;

    if payload.phase == "hidden" {
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
    translation_target: TranslationTargetLang,
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
        drop(output_lock);
        let mut translation_target_lock = state
            .translation_target
            .lock()
            .map_err(|_| "failed to update translation target settings".to_string())?;
        *translation_target_lock = translation_target;
    }

    save_current_hotkey_settings(&app)?;
    collect_hotkey_settings(&app)
}

#[tauri::command]
fn set_fn_key_modes(
    app: AppHandle,
    dictation_enabled: bool,
    translation_enabled: bool,
) -> Result<HotkeySettings, String> {
    let state = app.state::<AppState>();
    {
        let mut dictation_lock = state
            .fn_dictation_enabled
            .lock()
            .map_err(|_| "failed to update fn dictation settings".to_string())?;
        *dictation_lock = dictation_enabled;
        drop(dictation_lock);
        let mut translation_lock = state
            .fn_translation_enabled
            .lock()
            .map_err(|_| "failed to update fn translation settings".to_string())?;
        *translation_lock = translation_enabled;
    }

    save_current_hotkey_settings(&app)?;
    collect_hotkey_settings(&app)
}

#[tauri::command]
fn set_ui_language(app: AppHandle, language: String) -> Result<HotkeySettings, String> {
    let parsed = match language.as_str() {
        "zh" | "zh-CN" | "zh-cn" => UiLanguage::Zh,
        "en" | "en-US" | "en-us" => UiLanguage::En,
        "auto" => UiLanguage::Zh,
        _ => {
            return Err(format!("unsupported ui language: {language}"));
        }
    };
    {
        let state = app.state::<AppState>();
        let mut lock = state
            .ui_language
            .lock()
            .map_err(|_| "failed to update ui language settings".to_string())?;
        *lock = parsed;
    }
    save_current_hotkey_settings(&app)?;
    collect_hotkey_settings(&app)
}

#[tauri::command]
fn get_cloud_settings(app: AppHandle) -> Result<CloudSettings, String> {
    app.state::<AppState>()
        .cloud_settings
        .lock()
        .map(|v| v.clone())
        .map_err(|_| "failed to read cloud settings".to_string())
}

#[tauri::command]
fn get_dictionary_words(app: AppHandle) -> Result<Vec<String>, String> {
    app.state::<AppState>()
        .dictionary_words
        .lock()
        .map(|v| v.clone())
        .map_err(|_| "failed to read dictionary words".to_string())
}

#[tauri::command]
fn set_dictionary_words(app: AppHandle, words: Vec<String>) -> Result<Vec<String>, String> {
    let normalized = normalize_dictionary_words(words);
    {
        let state = app.state::<AppState>();
        let mut lock = state
            .dictionary_words
            .lock()
            .map_err(|_| "failed to update dictionary words".to_string())?;
        *lock = normalized.clone();
    }
    save_persisted_dictionary_words(&app, &normalized)?;
    Ok(normalized)
}

#[tauri::command]
fn set_cloud_settings(app: AppHandle, settings: CloudSettings) -> Result<CloudSettings, String> {
    validate_cloud_settings(&settings)?;
    {
        let state = app.state::<AppState>();
        let mut lock = state
            .cloud_settings
            .lock()
            .map_err(|_| "failed to update cloud settings".to_string())?;
        *lock = settings.clone();
    }
    save_persisted_cloud_settings(&app, &settings)?;
    Ok(settings)
}

#[tauri::command]
fn test_cloud_provider(
    app: AppHandle,
    input: TestCloudProviderInput,
) -> Result<TestCloudProviderResult, String> {
    let settings = app
        .state::<AppState>()
        .cloud_settings
        .lock()
        .map(|v| v.clone())
        .map_err(|_| "failed to read cloud settings".to_string())?;
    let provider = settings
        .providers
        .iter()
        .find(|p| p.id == input.provider_id)
        .ok_or_else(|| "provider not found".to_string())?;
    if !provider.enabled {
        return Ok(TestCloudProviderResult {
            ok: false,
            message: "provider is disabled".into(),
        });
    }
    let response = call_provider_with_retry(
        provider,
        "Reply with exactly: OK",
        "Health check ping",
        settings.pipeline.max_retries,
    );
    match response {
        Ok(_) => Ok(TestCloudProviderResult {
            ok: true,
            message: "connection ok".into(),
        }),
        Err(err) => Ok(TestCloudProviderResult {
            ok: false,
            message: err,
        }),
    }
}

#[tauri::command]
fn process_text_with_cloud(
    app: AppHandle,
    text: String,
    translate: bool,
    target_lang: Option<String>,
) -> Result<CloudProcessResult, String> {
    Ok(run_cloud_pipeline(&app, &text, translate, target_lang, None))
}

#[tauri::command]
fn set_overlay_state(app: AppHandle, phase: String, text: Option<String>) -> Result<(), String> {
    emit_overlay_state(&app, &phase, text, None)
}

#[tauri::command]
fn set_overlay_level(
    app: AppHandle,
    phase: String,
    text: Option<String>,
    level: Option<f32>,
) -> Result<(), String> {
    emit_overlay_state(&app, &phase, text, level)
}

#[tauri::command]
fn hide_overlay(app: AppHandle) -> Result<(), String> {
    emit_overlay_state(&app, "hidden", None, None)
}

fn schedule_hide_overlay(app: &AppHandle, delay_ms: u64) {
    let app_clone = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(delay_ms));
        let _ = emit_overlay_state(&app_clone, "hidden", None, None);
    });
}

#[tauri::command]
fn native_hotkey_confirm(app: AppHandle) -> Result<(), String> {
    let active_action = app
        .state::<AppState>()
        .native_hotkey_session
        .lock()
        .ok()
        .and_then(|s| s.active_action.clone());
    if let Some(action) = active_action {
        handle_native_hotkey_stop(&app, &action);
    }
    Ok(())
}

#[tauri::command]
fn native_hotkey_cancel(app: AppHandle) -> Result<(), String> {
    let tx = app
        .state::<AppState>()
        .native_recorder_tx
        .lock()
        .map_err(|_| "failed to access native recorder tx".to_string())?
        .clone();
    if let Some(tx) = tx {
        tx.send(NativeRecorderCommand::Reset {
            reason: "user-cancel".to_string(),
        })
        .map_err(|e| format!("failed to cancel recording: {e}"))?;
    }
    let _ = emit_overlay_state(&app, "hidden", None, None);
    Ok(())
}

fn spawn_native_recorder_worker(app: &AppHandle) -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<NativeRecorderCommand>();
    {
        let state = app.state::<AppState>();
        let mut lock = state
            .native_recorder_tx
            .lock()
            .map_err(|_| "failed to init native recorder channel".to_string())?;
        *lock = Some(tx);
    }
    let app_handle = app.clone();
    std::thread::spawn(move || {
        let mut recorder: Option<NativeRecorder> = None;
        while let Ok(cmd) = rx.recv() {
            match cmd {
                NativeRecorderCommand::Start { action } => {
                    eprintln!("[typemore][recorder] cmd=start action={}", action);
                    set_native_recorder_state(
                        &app_handle,
                        NativeRecorderState::Starting,
                        Some(action.clone()),
                        None,
                    );
                    // Show listening immediately on hotkey press, independent of voice activity.
                    let _ = emit_overlay_state(&app_handle, "listening", Some(action.clone()), Some(0.0));
                    if let Err(err) = start_native_recording_internal(&mut recorder, &app_handle) {
                        eprintln!(
                            "[typemore][recorder] start failed action={} error={}",
                            action, err
                        );
                        let _ = emit_overlay_state(
                            &app_handle,
                            "ready",
                            Some(if current_ui_language(&app_handle) == UiLanguage::En {
                                format!("Failed to start recording: {err}")
                            } else {
                                format!("录音启动失败: {err}")
                            }),
                            None,
                        );
                        schedule_hide_overlay(&app_handle, 1400);
                        reset_native_session_to_idle(&app_handle);
                        continue;
                    }
                    eprintln!("[typemore][recorder] start ok action={}", action);
                    set_native_recorder_state(
                        &app_handle,
                        NativeRecorderState::Recording,
                        Some(action.clone()),
                        Some(Instant::now()),
                    );
                }
                NativeRecorderCommand::Stop { action } => {
                    eprintln!("[typemore][recorder] cmd=stop action={}", action);
                    set_native_recorder_state(
                        &app_handle,
                        NativeRecorderState::Stopping,
                        Some(action.clone()),
                        None,
                    );
                    let _ = emit_overlay_state(
                        &app_handle,
                        "thinking",
                        Some(localize_text(&app_handle, "Processing", "Processing")),
                        None,
                    );
                    let (samples_raw, sample_rate, channels) =
                        match stop_native_recording_internal(&mut recorder) {
                            Ok(v) => v,
                            Err(err) => {
                                eprintln!(
                                    "[typemore][recorder] stop failed action={} error={}",
                                    action, err
                                );
                                let _ = emit_overlay_state(
                                    &app_handle,
                                    "ready",
                                    Some(if current_ui_language(&app_handle) == UiLanguage::En {
                                        format!("Failed to stop recording: {err}")
                                    } else {
                                        format!("录音停止失败: {err}")
                                    }),
                                    None,
                                );
                                schedule_hide_overlay(&app_handle, 1400);
                                reset_native_session_to_idle(&app_handle);
                                continue;
                            }
                        };
                    set_native_recorder_state(
                        &app_handle,
                        NativeRecorderState::Processing,
                        Some(action.clone()),
                        None,
                    );

                    if samples_raw.is_empty() {
                        eprintln!("[typemore][recorder] empty-audio action={}", action);
                        let _ = emit_overlay_state(
                            &app_handle,
                            "ready",
                            Some(localize_text(
                                &app_handle,
                                "未检测到语音",
                                "No speech detected",
                            )),
                            None,
                        );
                        schedule_hide_overlay(&app_handle, 1200);
                        reset_native_session_to_idle(&app_handle);
                        continue;
                    }

                    let mono = mix_to_mono(&samples_raw, channels);
                    let (model, tokens) = match model_files_if_ready(&app_handle) {
                        Ok(v) => v,
                        Err(err) => {
                            let _ = emit_overlay_state(
                                &app_handle,
                                "ready",
                                Some(if current_ui_language(&app_handle) == UiLanguage::En {
                                    format!("Model not ready: {err}")
                                } else {
                                    format!("模型未就绪: {err}")
                                }),
                                None,
                            );
                            schedule_hide_overlay(&app_handle, 1500);
                            reset_native_session_to_idle(&app_handle);
                            continue;
                        }
                    };

                    let transcribe_started = Instant::now();
                    let mut text = match transcribe_samples(&model, &tokens, sample_rate, &mono) {
                        Ok(v) => v,
                        Err(err) => {
                            eprintln!(
                                "[typemore][recorder] transcribe failed action={} error={}",
                                action, err
                            );
                            let _ = emit_overlay_state(
                                &app_handle,
                                "ready",
                                Some(if current_ui_language(&app_handle) == UiLanguage::En {
                                    format!("Transcription failed: {err}")
                                } else {
                                    format!("识别失败: {err}")
                                }),
                                None,
                            );
                            schedule_hide_overlay(&app_handle, 1500);
                            reset_native_session_to_idle(&app_handle);
                            continue;
                        }
                    };
                    text = apply_local_dictionary_terms(
                        &text,
                        &dictionary_words_snapshot(&app_handle),
                    );
                    eprintln!(
                        "[typemore][recorder] transcribe done action={} samples={} rate={} channels={} elapsed_ms={}",
                        action,
                        samples_raw.len(),
                        sample_rate,
                        channels,
                        transcribe_started.elapsed().as_millis()
                    );
                    let cloud_started = Instant::now();
                    let mut on_stage = |stage: &str| {
                        let text = match stage {
                            "optimizing" => localize_text(&app_handle, "Optimizing", "Optimizing"),
                            "translating" => {
                                localize_text(&app_handle, "Translating", "Translating")
                            }
                            _ => localize_text(&app_handle, "Processing", "Processing"),
                        };
                        let _ = emit_overlay_state(&app_handle, "thinking", Some(text), None);
                    };
                    let cloud_result = run_cloud_pipeline(
                        &app_handle,
                        &text,
                        action == "toggle-translation",
                        if action == "toggle-translation" {
                            Some(resolve_translation_target(&app_handle, &text).to_string())
                        } else {
                            None
                        },
                        Some(&mut on_stage),
                    );
                    if !cloud_result.warnings.is_empty() {
                        for warning in &cloud_result.warnings {
                            eprintln!(
                                "[typemore][cloud] warning action={} msg={}",
                                action, warning
                            );
                        }
                    }
                    text = cloud_result.final_text;
                    eprintln!(
                        "[typemore][cloud] done action={} stage={} elapsed_ms={}",
                        action,
                        cloud_result.stage,
                        cloud_started.elapsed().as_millis()
                    );

                    if let Ok(wav_data) = encode_wav_i16_mono(&mono, sample_rate) {
                        let prefix = if action == "toggle-translation" {
                            "translation"
                        } else {
                            "recording"
                        };
                        let _ = persist_recording_with_text(&app_handle, &wav_data, &text, prefix);
                    }

                    let output = text.trim().to_string();
                    if output.is_empty() {
                        let _ = emit_overlay_state(
                            &app_handle,
                            "ready",
                            Some(localize_text(
                                &app_handle,
                                "未识别到有效文本",
                                "No valid text recognized",
                            )),
                            None,
                        );
                        schedule_hide_overlay(&app_handle, 1200);
                    } else if let Err(err) = type_text_to_focused_app_impl(&app_handle, &output) {
                        let _ = emit_overlay_state(
                            &app_handle,
                            "ready",
                            Some(if current_ui_language(&app_handle) == UiLanguage::En {
                                format!("Failed to type text (text kept): {err}")
                            } else {
                                format!("发送失败（已保留文本）: {err}")
                            }),
                            None,
                        );
                        schedule_hide_overlay(&app_handle, 1700);
                    } else {
                        let _ = emit_overlay_state(&app_handle, "ready", Some(output), None);
                        schedule_hide_overlay(&app_handle, 1200);
                    }
                    reset_native_session_to_idle(&app_handle);
                }
                NativeRecorderCommand::Reset { reason } => {
                    eprintln!("[typemore][recorder] cmd=reset reason={}", reason);
                    if recorder.is_some() {
                        let _ = stop_native_recording_internal(&mut recorder);
                    }
                    reset_native_session_to_idle(&app_handle);
                    if reason == "user-cancel" {
                        let _ = emit_overlay_state(&app_handle, "hidden", None, None);
                        continue;
                    }
                    let _ = emit_overlay_state(
                        &app_handle,
                        "ready",
                        Some(localize_text(
                            &app_handle,
                            "录音已自动重置，请重试",
                            "Recording was auto-reset. Please try again.",
                        )),
                        None,
                    );
                    schedule_hide_overlay(&app_handle, 1200);
                }
            }
        }
    });
    Ok(())
}

fn spawn_native_recorder_watchdog(app: &AppHandle) {
    let app_handle = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        let snapshot = app_handle
            .state::<AppState>()
            .native_hotkey_session
            .lock()
            .ok()
            .map(|s| {
                (
                    s.state,
                    s.state_since,
                    s.recording_started_at,
                    s.active_action.clone(),
                )
            });
        let Some((state, state_since, recording_started_at, action)) = snapshot else {
            continue;
        };
        let now = Instant::now();
        let should_reset = match state {
            NativeRecorderState::Recording => recording_started_at
                .is_some_and(|started| now.duration_since(started).as_secs() > MAX_RECORDING_SECS),
            NativeRecorderState::Starting
            | NativeRecorderState::Stopping
            | NativeRecorderState::Processing => {
                now.duration_since(state_since).as_secs() > MAX_NON_IDLE_STUCK_SECS
            }
            NativeRecorderState::Idle => false,
        };
        if !should_reset {
            continue;
        }
        eprintln!(
            "[typemore][watchdog] timeout state={} action={} -> reset",
            state.as_str(),
            action.unwrap_or_else(|| "-".into())
        );
        if let Ok(mut session) = app_handle.state::<AppState>().native_hotkey_session.lock() {
            if session.state == state {
                session.state = NativeRecorderState::Idle;
                session.state_since = Instant::now();
                session.recording_started_at = None;
                session.active_action = None;
            } else {
                continue;
            }
        } else {
            continue;
        }
        let tx = app_handle
            .state::<AppState>()
            .native_recorder_tx
            .lock()
            .ok()
            .and_then(|v| v.clone());
        if let Some(tx) = tx {
            let _ = tx.send(NativeRecorderCommand::Reset {
                reason: format!("watchdog-timeout-{}", state.as_str()),
            });
        }
    });
}

fn handle_native_hotkey_start(app: &AppHandle, action: &str) {
    let mut should_start = false;
    {
        let state = app.state::<AppState>();
        let maybe_session = state.native_hotkey_session.lock();
        if let Ok(mut session) = maybe_session {
            if session.state != NativeRecorderState::Idle {
                eprintln!(
                    "[typemore][recorder] ignore start action={} state={} active_action={}",
                    action,
                    session.state.as_str(),
                    session.active_action.clone().unwrap_or_else(|| "-".into())
                );
                return;
            }
            session.state = NativeRecorderState::Starting;
            session.state_since = Instant::now();
            session.recording_started_at = None;
            session.active_action = Some(action.to_string());
            should_start = true;
        }
    }
    if !should_start {
        return;
    }
    let tx = app
        .state::<AppState>()
        .native_recorder_tx
        .lock()
        .ok()
        .and_then(|v| v.clone());
    if let Some(tx) = tx {
        eprintln!("[typemore][recorder] queue start action={}", action);
        let _ = tx.send(NativeRecorderCommand::Start {
            action: action.to_string(),
        });
    } else {
        eprintln!("[typemore][recorder] start dropped, worker not ready");
        reset_native_session_to_idle(app);
    }
}

fn handle_native_hotkey_stop(app: &AppHandle, action: &str) {
    let mut should_stop = false;
    let mut active_action: Option<String> = None;
    if let Ok(mut session) = app.state::<AppState>().native_hotkey_session.lock() {
        active_action = session.active_action.clone();
        if active_action.as_deref() == Some(action)
            && matches!(
                session.state,
                NativeRecorderState::Starting | NativeRecorderState::Recording
            )
        {
            session.state = NativeRecorderState::Stopping;
            session.state_since = Instant::now();
            session.recording_started_at = None;
            should_stop = true;
        }
    }
    if !should_stop {
        eprintln!(
            "[typemore][recorder] ignore stop action={} active_action={}",
            action,
            active_action.unwrap_or_else(|| "-".into())
        );
        return;
    }
    let tx = app
        .state::<AppState>()
        .native_recorder_tx
        .lock()
        .ok()
        .and_then(|v| v.clone());
    if let Some(tx) = tx {
        eprintln!("[typemore][recorder] queue stop action={}", action);
        let _ = tx.send(NativeRecorderCommand::Stop {
            action: action.to_string(),
        });
    } else {
        eprintln!("[typemore][recorder] stop dropped, worker not ready");
        reset_native_session_to_idle(app);
    }
}

fn handle_native_hotkey_event(app: &AppHandle, action: &str, state: &str) {
    let trigger_mode = app
        .state::<AppState>()
        .trigger_mode
        .lock()
        .map(|v| *v)
        .unwrap_or(HotkeyTriggerMode::Tap);
    eprintln!(
        "[typemore][hotkey] dispatch action={} state={} mode={:?}",
        action, state, trigger_mode
    );

    match trigger_mode {
        HotkeyTriggerMode::Tap => {
            if state != "pressed" {
                return;
            }
            let is_active = app
                .state::<AppState>()
                .native_hotkey_session
                .lock()
                .ok()
                .and_then(|s| s.active_action.clone())
                .is_some();
            if is_active {
                let active = app
                    .state::<AppState>()
                    .native_hotkey_session
                    .lock()
                    .ok()
                    .and_then(|s| s.active_action.clone())
                    .unwrap_or_else(|| action.to_string());
                handle_native_hotkey_stop(app, &active);
            } else {
                handle_native_hotkey_start(app, action);
            }
        }
        HotkeyTriggerMode::LongPress => {
            if state == "pressed" {
                handle_native_hotkey_start(app, action);
            } else if state == "released" {
                handle_native_hotkey_stop(app, action);
            }
        }
    }
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
                    let action = if lock.dictation_id.is_some_and(|id| shortcut.id() == id) {
                        "toggle-dictation"
                    } else if lock.translation_id.is_some_and(|id| shortcut.id() == id) {
                        "toggle-translation"
                    } else {
                        return;
                    };
                    eprintln!(
                        "[typemore][hotkey] global action={} shortcut={} state={}",
                        action,
                        shortcut,
                        state
                    );
                    emit_hotkey_event(app, action, &shortcut.to_string(), state);
                    handle_native_hotkey_event(app, action, state);
                })
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let mut persisted = load_persisted_hotkeys(app.handle())?;
            if let Some(saved) = persisted.as_mut() {
                if is_legacy_default_hotkeys(saved.dictation.trim(), saved.translation.trim()) {
                    eprintln!(
                        "[typemore] migrate legacy built-in hotkeys to empty defaults (Fn-only)"
                    );
                    saved.dictation.clear();
                    saved.translation.clear();
                }
            }
            let persisted_cloud = load_persisted_cloud_settings(app.handle()).unwrap_or_else(|err| {
                eprintln!("[typemore] failed to load cloud settings: {}", err);
                None
            });
            let persisted_dictionary =
                load_persisted_dictionary_words(app.handle()).unwrap_or_else(|err| {
                    eprintln!("[typemore] failed to load dictionary words settings: {}", err);
                    None
                });
            let fn_dictation_enabled = persisted
                .as_ref()
                .map(|saved| saved.fn_dictation_enabled)
                .unwrap_or_else(default_fn_dictation_enabled);
            let fn_translation_enabled = persisted
                .as_ref()
                .map(|saved| saved.fn_translation_enabled)
                .unwrap_or_else(default_fn_translation_enabled);
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
            let translation_target = persisted
                .as_ref()
                .map(|saved| saved.translation_target)
                .unwrap_or_else(default_translation_target);
            let ui_language = persisted
                .as_ref()
                .map(|saved| saved.ui_language)
                .unwrap_or_else(default_ui_language);

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
            if let Ok(mut lock) = app.state::<AppState>().fn_dictation_enabled.lock() {
                *lock = fn_dictation_enabled;
            }
            if let Ok(mut lock) = app.state::<AppState>().fn_translation_enabled.lock() {
                *lock = fn_translation_enabled;
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
            if let Ok(mut lock) = app.state::<AppState>().translation_target.lock() {
                *lock = translation_target;
            }
            if let Ok(mut lock) = app.state::<AppState>().ui_language.lock() {
                *lock = ui_language;
            }
            if let Ok(mut lock) = app.state::<AppState>().cloud_settings.lock() {
                *lock = persisted_cloud.unwrap_or_default();
            }
            if let Ok(mut lock) = app.state::<AppState>().dictionary_words.lock() {
                *lock = persisted_dictionary.unwrap_or_default();
            }
            let _ = save_current_hotkey_settings(app.handle());
            if let Ok(current_cloud) = app.state::<AppState>().cloud_settings.lock() {
                let _ = save_persisted_cloud_settings(app.handle(), &current_cloud.clone());
            }
            if let Ok(current_dictionary) = app.state::<AppState>().dictionary_words.lock() {
                let _ =
                    save_persisted_dictionary_words(app.handle(), &current_dictionary.clone());
            }
            eprintln!(
                "[typemore] fn keys: dictation={} translation={}",
                fn_dictation_enabled, fn_translation_enabled
            );
            eprintln!("[typemore] ui language: {:?}", ui_language);

            #[cfg(target_os = "macos")]
            if let Err(err) = start_macos_fn_key_monitor(app.handle()) {
                eprintln!("[typemore] failed to start fn monitor: {}", err);
            } else {
                eprintln!("[typemore] fn key monitor active");
            }

            if let Err(err) = spawn_native_recorder_worker(app.handle()) {
                eprintln!("[typemore] failed to start native recorder worker: {}", err);
            } else {
                eprintln!("[typemore] native recorder worker active");
                spawn_native_recorder_watchdog(app.handle());
                eprintln!("[typemore] native recorder watchdog active");
            }

            if let Some(main) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                let _ = main.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        app_handle.exit(0);
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_model_status,
            get_model_init_status,
            init_model,
            list_recordings,
            list_recording_char_stats,
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
            set_fn_key_modes,
            set_ui_language,
            get_cloud_settings,
            set_cloud_settings,
            get_dictionary_words,
            set_dictionary_words,
            test_cloud_provider,
            process_text_with_cloud,
            set_overlay_state,
            set_overlay_level,
            hide_overlay,
            native_hotkey_confirm,
            native_hotkey_cancel
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
