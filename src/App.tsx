import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import {
  CategoryScale,
  Chart as ChartJS,
  Filler,
  Legend,
  LineElement,
  LinearScale,
  PointElement,
  Tooltip,
} from "chart.js";
import { Line } from "react-chartjs-2";
import {
  BookText,
  Check,
  Copy,
  FolderOpen,
  History,
  Home,
  Loader2,
  Pencil,
  Plus,
  RefreshCcw,
  Settings,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { blobToMono16kWav } from "./components/app/audio";
import type { ModelInitStatus, RecordingItem, SaveAndTranscribeResult } from "./components/app/types";
import { badgeClass, defaultInitStatus, formatCurrentRecordingTime, formatListTime } from "./components/app/utils";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Card } from "./components/ui/card";
import { ScrollArea } from "./components/ui/scroll-area";
import { Separator } from "./components/ui/separator";
import { Textarea } from "./components/ui/textarea";
import { cn } from "./lib/utils";

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Tooltip, Legend, Filler);

type Page = "home" | "history" | "dictionary" | "cloud";
type LangMode = "auto" | "zh-CN" | "en-US";
type UiLang = "zh" | "en";
type CaptureTarget = "dictation" | "translation" | null;
type HotkeyAction = "toggle-dictation" | "toggle-translation";
type HotkeyEventState = "pressed" | "released";
type HotkeyTriggerMode = "tap" | "long-press";
type OverlayPosition = "top" | "bottom";
type OutputMode = "auto-paste" | "paste-and-keep" | "copy-only";
type TranslationTargetLang = "auto" | "en" | "zh-CN" | "ja" | "ko";
type SettingsSection = "language" | "hotkey" | "processing" | "providers" | "temp" | "about";
type CloudVendor =
  | "openai"
  | "openrouter"
  | "anthropic"
  | "gemini"
  | "groq"
  | "deepseek"
  | "mistral"
  | "xai"
  | "perplexity"
  | "together"
  | "ollama";

type AccessibilityStatus = {
  supported: boolean;
  trusted: boolean;
  axTrusted?: boolean;
  tccAllowed?: boolean | null;
  runtimeHint?: string | null;
};

type GlobalShortcutPayload = {
  action: HotkeyAction;
  shortcut: string;
  state: HotkeyEventState;
};

type HotkeySettings = {
  dictation: string;
  translation: string;
  fnEnabled: boolean;
  triggerMode: HotkeyTriggerMode;
  overlayPosition: OverlayPosition;
  outputMode: OutputMode;
  translationTarget: TranslationTargetLang;
  uiLanguage?: "zh" | "en";
};

type OverlayStatePayload = {
  phase: "hidden" | "listening" | "thinking" | "ready";
  text?: string | null;
  level?: number | null;
};

type RecordingSavedPayload = {
  recording: RecordingItem;
};

type CloudProviderConfig = {
  id: string;
  name: string;
  vendor: CloudVendor;
  model: string;
  apiKey: string;
  baseUrl?: string | null;
  enabled: boolean;
  priority: number;
};

type CloudPipelineConfig = {
  enabled: boolean;
  optimizeProviderId: string;
  translateProviderId: string;
  targetLanguage: string;
  optimizePrompt: string;
  translatePrompt: string;
  timeoutMs: number;
  maxRetries: number;
};

type CloudSettings = {
  providers: CloudProviderConfig[];
  pipeline: CloudPipelineConfig;
};

type CloudProcessResult = {
  finalText: string;
  stage: "local" | "optimized" | "translated";
  warnings: string[];
};

type TestCloudProviderResult = {
  ok: boolean;
  message: string;
};

const DICTIONARY_STORAGE_KEY = "typemore.dictionary.words";
const LANG_MODE_STORAGE_KEY = "typemore.lang.mode";
const DEFAULT_HOTKEY_DICTATION = "";
const DEFAULT_HOTKEY_TRANSLATION = "";
const DEFAULT_TRIGGER_MODE: HotkeyTriggerMode = "tap";
const DEFAULT_OVERLAY_POSITION: OverlayPosition = "bottom";
const DEFAULT_OUTPUT_MODE: OutputMode = "auto-paste";
const BACKEND_NATIVE_HOTKEY_PIPELINE = true;
const CLOUD_VENDOR_OPTIONS: Array<{ value: CloudVendor; label: string }> = [
  { value: "openai", label: "OpenAI" },
  { value: "openrouter", label: "OpenRouter" },
  { value: "anthropic", label: "Anthropic" },
  { value: "gemini", label: "Gemini" },
  { value: "groq", label: "Groq" },
  { value: "deepseek", label: "DeepSeek" },
  { value: "mistral", label: "Mistral" },
  { value: "xai", label: "xAI" },
  { value: "perplexity", label: "Perplexity" },
  { value: "together", label: "Together AI" },
  { value: "ollama", label: "Ollama" },
];

const TARGET_LANGUAGE_OPTIONS: Array<{ value: string; labelZh: string; labelEn: string }> = [
  { value: "en", labelZh: "English (英文)", labelEn: "English" },
  { value: "zh-CN", labelZh: "中文（简体）", labelEn: "Chinese (Simplified)" },
  { value: "zh-TW", labelZh: "中文（繁体）", labelEn: "Chinese (Traditional)" },
  { value: "ja", labelZh: "日语", labelEn: "Japanese" },
  { value: "ko", labelZh: "韩语", labelEn: "Korean" },
  { value: "es", labelZh: "西班牙语", labelEn: "Spanish" },
  { value: "fr", labelZh: "法语", labelEn: "French" },
  { value: "de", labelZh: "德语", labelEn: "German" },
  { value: "pt", labelZh: "葡萄牙语", labelEn: "Portuguese" },
  { value: "it", labelZh: "意大利语", labelEn: "Italian" },
  { value: "ru", labelZh: "俄语", labelEn: "Russian" },
  { value: "ar", labelZh: "阿拉伯语", labelEn: "Arabic" },
  { value: "hi", labelZh: "印地语", labelEn: "Hindi" },
  { value: "tr", labelZh: "土耳其语", labelEn: "Turkish" },
  { value: "vi", labelZh: "越南语", labelEn: "Vietnamese" },
  { value: "th", labelZh: "泰语", labelEn: "Thai" },
  { value: "id", labelZh: "印尼语", labelEn: "Indonesian" },
  { value: "pl", labelZh: "波兰语", labelEn: "Polish" },
  { value: "nl", labelZh: "荷兰语", labelEn: "Dutch" },
  { value: "sv", labelZh: "瑞典语", labelEn: "Swedish" },
];

const I18N = {
  zh: {
    navHome: "首页",
    navHistory: "历史",
    navDictionary: "词库",
    navSettings: "设置",
    titleHome: "用你的声音，打出更多文字。",
    subHome: "开源、离线优先，支持 BYOD（Bring Your Own Key）的语音转写工作流。",
    featureOpenSource: "Open Source",
    featureOfflineFirst: "Offline First",
    featureByod: "BYOD",
    statsDailyInputTitle: "输入趋势",
    statsDailyInputDesc: "基于历史录音与缓存文本自动统计最近 30 天数据。",
    statsToday: "今日",
    statsDailyAvg: "日均",
    statsMonthTotal: "本月输入",
    statsYearTotal: "本年输入",
    statsTotal: "累计输入",
    statsUnitChars: "字",
    statsVersion: "版本",
    modelReady: "模型已就绪",
    modelInitializing: "初始化中",
    modelNotReady: "模型未就绪",
    stopRecording: "停止录音",
    startRecording: "开始录音",
    initModelRunning: "初始化中...",
    initModelReady: "模型已就绪",
    initModelStart: "初始化模型",
    viewHistory: "查看历史",
    recentRecordings: "最近录音",
    noRecordings: "还没有录音记录，先初始化模型并开始录音。",
    countItems: "{count} 条",
    historyTitle: "历史",
    historyRefresh: "刷新",
    historySelectAll: "全选",
    historyClearSelection: "取消全选",
    historyDeleteSelected: "删除所选",
    historySelectedCount: "已选 {count} 条",
    historyDeleteSelectedConfirm: "确认删除已选中的 {count} 条录音吗？此操作不可恢复。",
    rename: "重命名",
    delete: "删除",
    retranscribe: "重新识别",
    copyResult: "复制结果",
    currentRecording: "当前录音: {time}",
    noSelectedRecording: "当前未选中录音",
    transcriptPlaceholder: "识别结果会显示在这里...",
    dictionaryTitle: "词库",
    dictionaryWords: "{count} 个词",
    dictionaryPlaceholder: "添加新词，比如 TestFlight",
    dictionaryAdd: "添加词条",
    dictionaryEmpty: "还没有词条，先添加几个常用专有名词。",
    dictionaryDelete: "删除词条",
    settingsTitle: "设置",
    settingsSectionGeneral: "通用",
    settingsSectionLanguage: "语言",
    settingsSectionHotkey: "快捷键",
    settingsSectionCloud: "云端处理",
    settingsSectionProviders: "模型厂商",
    settingsSectionTemp: "临时目录",
    settingsSectionAbout: "关于",
    settingsTempDirTitle: "临时目录",
    settingsTempDirDesc: "打开应用的临时目录，用于查看当前运行过程中的临时文件。",
    settingsOpenTempDir: "打开临时目录",
    settingsProjectInfo: "项目信息",
    settingsProjectWebsite: "官网",
    settingsProjectGithub: "开源仓库",
    settingsProjectMoreApps: "更多 App",
    settingsProjectLicense: "协议",
    settingsLanguageTitle: "语言",
    settingsLanguageDesc: "支持自动跟随系统语言，也可以手动切换。",
    settingsHotkeyTitle: "全局快捷键",
    settingsHotkeyDesc: "支持点按切换（按一次开始/停止）和长按模式（按下开始、松开停止）。",
    settingsHotkeyBuiltInHint: "内置快捷键可留空（仅使用 Fn / Fn+Shift）。也可按需单独设置听写或翻译快捷键。",
    settingsHotkeyDictation: "听写快捷键",
    settingsHotkeyTranslation: "翻译快捷键",
    settingsFnKeyToggle: "启用 Fn 单键切换录音（macOS）",
    settingsHotkeyTogglePlaceholder: "例如: CommandOrControl+Alt+Space",
    settingsHotkeySave: "保存快捷键",
    settingsHotkeyReset: "恢复默认",
    settingsHotkeyRecord: "录制",
    settingsHotkeyRecording: "录制中...",
    settingsHotkeyPressHint: "点击“录制”后直接按组合键；按 Esc 取消。",
    settingsTriggerMode: "触发模式",
    settingsTriggerModeTap: "点按切换",
    settingsTriggerModeLongPress: "长按松开结束",
    settingsOverlayPosition: "悬浮窗位置",
    settingsOverlayPositionTop: "顶部",
    settingsOverlayPositionBottom: "底部",
    settingsOutputMode: "发送方式",
    settingsOutputModeAutoPaste: "自动粘贴并恢复剪贴板",
    settingsOutputModePasteAndKeep: "自动粘贴并保留结果到剪贴板",
    settingsOutputModeCopyOnly: "仅复制到剪贴板（不自动粘贴）",
    settingsHotkeyConflictTitle: "快捷键冲突",
    settingsHotkeyConflictSame: "听写快捷键与翻译快捷键不能相同。",
    settingsHotkeyConflictWithSystem: "与系统常用快捷键冲突：{value}",
    settingsHotkeyWarningSaveBlocked: "请先修复冲突再保存。",
    settingsTranslationTarget: "翻译目标语言",
    settingsTranslationTargetAuto: "自动（中英互转）",
    settingsTranslationTargetEn: "英文",
    settingsTranslationTargetZh: "中文",
    settingsTranslationTargetJa: "日文",
    settingsTranslationTargetKo: "韩文",
    settingsCloudTitle: "云端处理",
    settingsCloudDesc: "将本地识别结果交给云端做二次优化与翻译。建议先在“模型厂商”里配置可用模型。",
    settingsCloudGuide: "处理顺序：本地识别 -> 云端优化 -> (翻译快捷键时) 云端翻译。任一步失败会自动回退到上一步结果。",
    settingsCloudEnabled: "启用云端后处理",
    settingsCloudOptimizeProvider: "优化模型",
    settingsCloudTranslateProvider: "翻译模型",
    settingsCloudTargetLanguage: "云端目标语言",
    settingsCloudOptimizePrompt: "优化 Prompt",
    settingsCloudTranslatePrompt: "翻译 Prompt",
    settingsCloudOptimizeSection: "优化阶段",
    settingsCloudTranslateSection: "翻译阶段",
    settingsCloudOptimizeDesc: "用于修复 ASR 文本中的标点、分词和识别错误。",
    settingsCloudTranslateDesc: "仅在翻译快捷键触发时执行，输出为目标语言结果。",
    settingsCloudTimeoutMs: "请求超时(ms)",
    settingsCloudRetries: "失败重试次数",
    settingsCloudProviders: "厂商列表",
    settingsCloudProviderVendor: "厂商",
    settingsCloudProviderModel: "模型",
    settingsCloudProviderApiKey: "API Key",
    settingsCloudProviderBaseUrl: "Base URL(可选)",
    settingsCloudProviderEnabled: "启用",
    settingsCloudAddProvider: "新增厂商",
    settingsCloudRemoveProvider: "删除",
    settingsCloudSave: "保存云端设置",
    settingsCloudTest: "测试连接",
    settingsCloudNoProviderHint: "暂无可用模型。请先到“模型厂商”新增并保存，再返回这里选择。",
    settingsProvidersTitle: "模型厂商",
    settingsProvidersDesc: "每条配置是一个可调用模型（厂商 + 模型）。优化/翻译会从这里选择。",
    settingsProvidersGuide: "建议先添加 1 个优化模型，再按需添加 1 个翻译模型。",
    settingsProvidersSave: "保存厂商配置",
    transcriptCloudProcessFailed: "云端处理失败，已回退本地结果: {error}",
    languageModeLabel: "界面语言",
    langAuto: "自动（跟随系统）",
    langZh: "中文",
    langEn: "English",
    accessTitle: "辅助功能权限",
    accessNeed: "要在任意应用中输入文字，请先授予 macOS 辅助功能权限。",
    accessGranted: "权限已开启，可向其他应用输入文字。",
    accessRequest: "请求权限",
    accessOpenSettings: "打开系统设置",
    accessRefresh: "刷新状态",
    recordingPrefix: "录音",
    transcriptInitFailed: "初始化失败: {error}",
    transcriptListenFailed: "监听模型进度失败: {error}",
    transcriptModelReady: "模型已就绪，可以开始录音识别。",
    transcriptModelInitFailed: "模型初始化失败: {error}",
    transcriptInitInProgress: "模型初始化中，请稍后再按一次快捷键。",
    transcriptRecordingFailed: "录音识别失败: {error}",
    transcriptNeedInit: "请先初始化模型，再开始录音。",
    transcriptNeedSelect: "请先在左侧选择一个录音。",
    transcriptRetryFailed: "识别失败: {error}",
    transcriptCopyFailed: "[复制失败] {error}",
    transcriptRenameFailed: "重命名失败: {error}",
    transcriptDeleteFailed: "删除失败: {error}",
    transcriptOpenTempDirOk: "已打开临时目录: {dir}",
    transcriptOpenTempDirFailed: "打开临时目录失败: {error}",
    transcriptTypeFailed: "输入到当前应用失败: {error}",
    transcriptTranslating: "翻译中...",
    transcriptTranslationFailed: "翻译失败，已回退原文: {error}",
    fallbackTitle: "未能自动输入",
    fallbackDesc: "请点击复制后手动粘贴到目标输入框。",
    fallbackCopy: "复制识别结果",
    fallbackClose: "关闭",
  },
  en: {
    navHome: "Home",
    navHistory: "History",
    navDictionary: "Dictionary",
    navSettings: "Settings",
    titleHome: "Type More with your voice.",
    subHome: "Open-source and offline-first voice transcription workflow with BYOD (Bring Your Own Key).",
    featureOpenSource: "Open Source",
    featureOfflineFirst: "Offline First",
    featureByod: "BYOD",
    statsDailyInputTitle: "Input Trend",
    statsDailyInputDesc: "Auto-calculated from recording history and cached transcripts for the last 30 days.",
    statsToday: "Today",
    statsDailyAvg: "Daily Avg",
    statsMonthTotal: "This Month",
    statsYearTotal: "This Year",
    statsTotal: "Total",
    statsUnitChars: "chars",
    statsVersion: "Version",
    modelReady: "Model Ready",
    modelInitializing: "Initializing",
    modelNotReady: "Not Ready",
    stopRecording: "Stop Recording",
    startRecording: "Start Recording",
    initModelRunning: "Initializing...",
    initModelReady: "Model Ready",
    initModelStart: "Initialize Model",
    viewHistory: "View History",
    recentRecordings: "Recent Recordings",
    noRecordings: "No recordings yet. Initialize the model and start recording.",
    countItems: "{count} items",
    historyTitle: "History",
    historyRefresh: "Refresh",
    historySelectAll: "Select all",
    historyClearSelection: "Clear selection",
    historyDeleteSelected: "Delete selected",
    historySelectedCount: "{count} selected",
    historyDeleteSelectedConfirm: "Delete {count} selected recordings? This cannot be undone.",
    rename: "Rename",
    delete: "Delete",
    retranscribe: "Retranscribe",
    copyResult: "Copy",
    currentRecording: "Current recording: {time}",
    noSelectedRecording: "No recording selected",
    transcriptPlaceholder: "Transcription result will appear here...",
    dictionaryTitle: "Dictionary",
    dictionaryWords: "{count} words",
    dictionaryPlaceholder: "Add a word, e.g. TestFlight",
    dictionaryAdd: "Add word",
    dictionaryEmpty: "No dictionary words yet. Add a few proper nouns first.",
    dictionaryDelete: "Delete word",
    settingsTitle: "Settings",
    settingsSectionGeneral: "General",
    settingsSectionLanguage: "Language",
    settingsSectionHotkey: "Hotkeys",
    settingsSectionCloud: "Cloud Processing",
    settingsSectionProviders: "Model Providers",
    settingsSectionTemp: "Temporary Directory",
    settingsSectionAbout: "About",
    settingsTempDirTitle: "Temporary Directory",
    settingsTempDirDesc: "Open app temporary directory to inspect runtime temp files.",
    settingsOpenTempDir: "Open Temporary Directory",
    settingsProjectInfo: "Project Info",
    settingsProjectWebsite: "Website",
    settingsProjectGithub: "Open-source repo",
    settingsProjectMoreApps: "More apps",
    settingsProjectLicense: "License",
    settingsLanguageTitle: "Language",
    settingsLanguageDesc: "Auto follow system language, or switch manually.",
    settingsHotkeyTitle: "Global Hotkey",
    settingsHotkeyDesc: "Supports tap-toggle and long-press mode (press to start, release to stop).",
    settingsHotkeyBuiltInHint: "Built-in shortcuts can stay empty (Fn / Fn+Shift only), or you can set dictation/translation shortcuts individually.",
    settingsHotkeyDictation: "Dictation hotkey",
    settingsHotkeyTranslation: "Translation hotkey",
    settingsFnKeyToggle: "Enable Fn one-key dictation toggle (macOS)",
    settingsHotkeyTogglePlaceholder: "e.g. CommandOrControl+Alt+Space",
    settingsHotkeySave: "Save hotkey",
    settingsHotkeyReset: "Reset default",
    settingsHotkeyRecord: "Record",
    settingsHotkeyRecording: "Recording...",
    settingsHotkeyPressHint: "Click Record then press combo. Press Esc to cancel.",
    settingsTriggerMode: "Trigger mode",
    settingsTriggerModeTap: "Tap to toggle",
    settingsTriggerModeLongPress: "Long press, release to stop",
    settingsOverlayPosition: "Overlay position",
    settingsOverlayPositionTop: "Top",
    settingsOverlayPositionBottom: "Bottom",
    settingsOutputMode: "Output mode",
    settingsOutputModeAutoPaste: "Auto-paste and restore previous clipboard",
    settingsOutputModePasteAndKeep: "Auto-paste and keep result in clipboard",
    settingsOutputModeCopyOnly: "Copy-only (no auto-paste)",
    settingsHotkeyConflictTitle: "Hotkey conflicts",
    settingsHotkeyConflictSame: "Dictation and translation hotkeys must be different.",
    settingsHotkeyConflictWithSystem: "Conflicts with common system shortcut: {value}",
    settingsHotkeyWarningSaveBlocked: "Resolve conflicts before saving.",
    settingsTranslationTarget: "Translation target",
    settingsTranslationTargetAuto: "Auto (ZH <-> EN)",
    settingsTranslationTargetEn: "English",
    settingsTranslationTargetZh: "Chinese",
    settingsTranslationTargetJa: "Japanese",
    settingsTranslationTargetKo: "Korean",
    settingsCloudTitle: "Cloud Processing",
    settingsCloudDesc: "Send local ASR output to cloud for optimization and translation. Configure models in “Model Providers” first.",
    settingsCloudGuide: "Pipeline: Local ASR -> Cloud optimize -> (translation hotkey) Cloud translate. It falls back automatically on failures.",
    settingsCloudEnabled: "Enable cloud post-processing",
    settingsCloudOptimizeProvider: "Optimize model",
    settingsCloudTranslateProvider: "Translate model",
    settingsCloudTargetLanguage: "Cloud target language",
    settingsCloudOptimizePrompt: "Optimize prompt",
    settingsCloudTranslatePrompt: "Translate prompt",
    settingsCloudOptimizeSection: "Optimize Stage",
    settingsCloudTranslateSection: "Translate Stage",
    settingsCloudOptimizeDesc: "Fix ASR punctuation, segmentation, and recognition errors.",
    settingsCloudTranslateDesc: "Runs only for translation hotkey and outputs target-language text.",
    settingsCloudTimeoutMs: "Timeout (ms)",
    settingsCloudRetries: "Retry count",
    settingsCloudProviders: "Providers",
    settingsCloudProviderVendor: "Vendor",
    settingsCloudProviderModel: "Model",
    settingsCloudProviderApiKey: "API Key",
    settingsCloudProviderBaseUrl: "Base URL (optional)",
    settingsCloudProviderEnabled: "Enabled",
    settingsCloudAddProvider: "Add provider",
    settingsCloudRemoveProvider: "Remove",
    settingsCloudSave: "Save cloud settings",
    settingsCloudTest: "Test connection",
    settingsCloudNoProviderHint: "No models configured yet. Go to “Model Providers” and add one first.",
    settingsProvidersTitle: "Model Providers",
    settingsProvidersDesc: "Each entry is one callable model (vendor + model). Optimize/Translate selects from this list.",
    settingsProvidersGuide: "Start with one optimize model, then optionally add a dedicated translate model.",
    settingsProvidersSave: "Save provider configs",
    transcriptCloudProcessFailed: "Cloud processing failed, fallback to local text: {error}",
    languageModeLabel: "Interface language",
    langAuto: "Auto (System)",
    langZh: "Chinese",
    langEn: "English",
    accessTitle: "Accessibility Permission",
    accessNeed: "To type into any app, please grant macOS Accessibility permission.",
    accessGranted: "Permission granted. You can type into other apps.",
    accessRequest: "Request Permission",
    accessOpenSettings: "Open System Settings",
    accessRefresh: "Refresh",
    recordingPrefix: "recording",
    transcriptInitFailed: "Initialization failed: {error}",
    transcriptListenFailed: "Failed to listen model progress: {error}",
    transcriptModelReady: "Model is ready. You can start recording.",
    transcriptModelInitFailed: "Model initialization failed: {error}",
    transcriptInitInProgress: "Model is initializing. Please press the hotkey again in a moment.",
    transcriptRecordingFailed: "Recording transcription failed: {error}",
    transcriptNeedInit: "Please initialize the model before recording.",
    transcriptNeedSelect: "Please select a recording from the left list.",
    transcriptRetryFailed: "Transcription failed: {error}",
    transcriptCopyFailed: "[Copy failed] {error}",
    transcriptRenameFailed: "Rename failed: {error}",
    transcriptDeleteFailed: "Delete failed: {error}",
    transcriptOpenTempDirOk: "Opened temporary directory: {dir}",
    transcriptOpenTempDirFailed: "Failed to open temporary directory: {error}",
    transcriptTypeFailed: "Failed to type into focused app: {error}",
    transcriptTranslating: "Translating...",
    transcriptTranslationFailed: "Translation failed, fallback to original text: {error}",
    fallbackTitle: "Auto typing failed",
    fallbackDesc: "Copy the result and paste it to your target input manually.",
    fallbackCopy: "Copy transcription",
    fallbackClose: "Close",
  },
} as const;

function formatI18n(template: string, vars?: Record<string, string | number>) {
  if (!vars) {
    return template;
  }
  return template.replace(/\{(\w+)\}/g, (_, key: string) => String(vars[key] ?? ""));
}

function detectSystemLang(): UiLang {
  const lang = (navigator.language || "en-US").toLowerCase();
  return lang.startsWith("zh") ? "zh" : "en";
}

function resolveUiLangFromLocalSetting(): UiLang {
  const raw = window.localStorage.getItem(LANG_MODE_STORAGE_KEY);
  if (raw === "zh-CN") {
    return "zh";
  }
  if (raw === "en-US") {
    return "en";
  }
  return detectSystemLang();
}

function defaultCloudSettings(): CloudSettings {
  return {
    providers: [],
    pipeline: {
      enabled: false,
      optimizeProviderId: "",
      translateProviderId: "",
      targetLanguage: "en",
      optimizePrompt:
        "You are an expert text post-processor for speech transcription.\nFix recognition errors, punctuation, casing, and spacing.\nDo not add new facts. Return only the corrected text.",
      translatePrompt:
        "Translate the input text into {target_language}.\nPreserve meaning and tone. Return only translated text.",
      timeoutMs: 10000,
      maxRetries: 1,
    },
  };
}

function cloudVendorLabel(vendor: CloudVendor) {
  return CLOUD_VENDOR_OPTIONS.find((item) => item.value === vendor)?.label ?? vendor;
}

function buildProviderLabel(provider: Pick<CloudProviderConfig, "vendor" | "model">) {
  const model = provider.model.trim() || "model";
  return `${cloudVendorLabel(provider.vendor)} / ${model}`;
}

function makeProviderId(vendor: CloudVendor, model: string) {
  const normalized = `${vendor}-${model || "model"}`
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 36);
  return `${normalized || "provider"}-${Date.now().toString().slice(-6)}`;
}

type DailyInputStat = {
  dateLabel: string;
  chars: number;
};

type UsageStats = {
  todayChars: number;
  dailyAvgChars: number;
  monthChars: number;
  yearChars: number;
  totalChars: number;
  dailyPoints: DailyInputStat[];
};

const HOME_STATS_DAYS = 30;

function emptyUsageStats(): UsageStats {
  const today = new Date();
  const dailyPoints: DailyInputStat[] = [];
  for (let offset = HOME_STATS_DAYS - 1; offset >= 0; offset -= 1) {
    const d = new Date(today);
    d.setDate(today.getDate() - offset);
    const mm = String(d.getMonth() + 1).padStart(2, "0");
    const dd = String(d.getDate()).padStart(2, "0");
    dailyPoints.push({ dateLabel: `${mm}/${dd}`, chars: 0 });
  }
  return {
    todayChars: 0,
    dailyAvgChars: 0,
    monthChars: 0,
    yearChars: 0,
    totalChars: 0,
    dailyPoints,
  };
}

function OverlayWindowApp() {
  const [phase, setPhase] = useState<OverlayStatePayload["phase"]>("hidden");
  const [text, setText] = useState("");
  const [level, setLevel] = useState(0);
  const [uiLang, setUiLang] = useState<UiLang>(() => resolveUiLangFromLocalSetting());

  useEffect(() => {
    document.documentElement.classList.add("overlay-mode");
    document.body.classList.add("overlay-mode");
    document.getElementById("root")?.classList.add("overlay-mode");
    let unlisten: (() => void) | undefined;
    listen<OverlayStatePayload>("overlay-state", (event) => {
      const nextUiLang = resolveUiLangFromLocalSetting();
      setUiLang(nextUiLang);
      setPhase(event.payload.phase);
      setText(event.payload.text ?? "");
      setLevel(Math.max(0, Math.min(1, Number(event.payload.level ?? 0))));
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) {
        unlisten();
      }
      document.documentElement.classList.remove("overlay-mode");
      document.body.classList.remove("overlay-mode");
      document.getElementById("root")?.classList.remove("overlay-mode");
    };
  }, []);

  if (phase === "hidden") {
    return <div className="h-screen w-screen bg-transparent" />;
  }

  const title =
    phase === "listening"
      ? uiLang === "zh"
        ? "正在听..."
        : "Listening..."
      : phase === "thinking"
        ? uiLang === "zh"
          ? "识别中..."
          : "Processing..."
        : uiLang === "zh"
          ? "就绪"
          : "Ready";
  const speakingActive = level > 0.1;
  return (
    <main className="h-screen w-screen bg-transparent p-0">
      <div
        className={cn(
          "h-full w-full overflow-hidden rounded-lg border border-white/20 bg-black/90 px-2 text-white shadow-2xl transition-opacity duration-300",
          phase === "ready" ? "opacity-95" : "opacity-100"
        )}
      >
        <div className="flex h-full items-center justify-between gap-3">
          <div
            className={cn(
              "text-xs font-semibold tracking-tight leading-none transition-colors duration-100",
              phase === "listening" && speakingActive ? "text-emerald-200" : "text-white"
            )}
          >
            {title}
          </div>
          {phase === "listening" ? (
            <div className="flex h-4 items-end gap-1">
              {[0.55, 0.8, 1, 0.82, 0.58].map((factor, index) => {
                const active = Math.max(0.1, Math.min(1, level * 1.45 * factor));
                const heightPx = 4 + Math.round(active * 10);
                const speakingBar = active > 0.22;
                return (
                  <span
                    key={index}
                    className={cn(
                      "w-1 rounded-full transition-all duration-75",
                      speakingBar ? "bg-emerald-300" : "bg-white"
                    )}
                    style={{
                      height: `${heightPx}px`,
                      opacity: 0.35 + active * 0.65,
                      boxShadow: speakingBar
                        ? "0 0 8px rgba(110, 231, 183, 0.7)"
                        : "0 0 0 rgba(255,255,255,0)",
                    }}
                  />
                );
              })}
            </div>
          ) : (
            text && <div className="truncate text-[10px] text-white/80">{text}</div>
          )}
        </div>
      </div>
    </main>
  );
}

function MainApp() {
  const [page, setPage] = useState<Page>("home");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("language");
  const [recordings, setRecordings] = useState<RecordingItem[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [transcript, setTranscript] = useState("");
  const [initStatus, setInitStatus] = useState<ModelInitStatus>(defaultInitStatus());
  const [isRecording, setIsRecording] = useState(false);
  const [isBusy, setIsBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [dictionaryWords, setDictionaryWords] = useState<string[]>([]);
  const [newWord, setNewWord] = useState("");
  const [langMode, setLangMode] = useState<LangMode>(() => {
    const raw = window.localStorage.getItem(LANG_MODE_STORAGE_KEY);
    return raw === "zh-CN" || raw === "en-US" || raw === "auto" ? raw : "auto";
  });
  const [accessibility, setAccessibility] = useState<AccessibilityStatus>({ supported: false, trusted: false });
  const [hotkeyDictation, setHotkeyDictation] = useState(DEFAULT_HOTKEY_DICTATION);
  const [hotkeyTranslation, setHotkeyTranslation] = useState(DEFAULT_HOTKEY_TRANSLATION);
  const [fnKeyEnabled, setFnKeyEnabled] = useState(true);
  const [triggerMode, setTriggerMode] = useState<HotkeyTriggerMode>(DEFAULT_TRIGGER_MODE);
  const [overlayPosition, setOverlayPosition] = useState<OverlayPosition>(DEFAULT_OVERLAY_POSITION);
  const [outputMode, setOutputMode] = useState<OutputMode>(DEFAULT_OUTPUT_MODE);
  const [translationTargetLang, setTranslationTargetLang] = useState<TranslationTargetLang>("auto");
  const [cloudSettings, setCloudSettings] = useState<CloudSettings>(defaultCloudSettings);
  const [savingCloudSettings, setSavingCloudSettings] = useState(false);
  const [savingHotkeys, setSavingHotkeys] = useState(false);
  const [captureTarget, setCaptureTarget] = useState<CaptureTarget>(null);
  const [fallbackText, setFallbackText] = useState<string | null>(null);
  const [historyMenuId, setHistoryMenuId] = useState<string | null>(null);
  const [historySelectedIds, setHistorySelectedIds] = useState<string[]>([]);
  const [usageStats, setUsageStats] = useState<UsageStats>(() => emptyUsageStats());
  const [appVersion, setAppVersion] = useState("-");

  const recorderRef = useRef<MediaRecorder | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const chunksRef = useRef<BlobPart[]>([]);
  const isRecordingRef = useRef(false);
  const modelReadyRef = useRef(false);
  const recordingByHotkeyRef = useRef(false);
  const triggerModeRef = useRef<HotkeyTriggerMode>(DEFAULT_TRIGGER_MODE);
  const activeHoldActionRef = useRef<HotkeyAction | null>(null);
  const activeHotkeyActionRef = useRef<HotkeyAction | null>(null);
  const pendingStartActionRef = useRef<HotkeyAction | null>(null);
  const cancelAfterStartActionRef = useRef<HotkeyAction | null>(null);
  const lastPressedAtRef = useRef<Record<HotkeyAction, number>>({
    "toggle-dictation": 0,
    "toggle-translation": 0,
  });
  const suppressDictationUntilRef = useRef(0);
  const suppressTranslationUntilRef = useRef(0);
  const audioContextRef = useRef<AudioContext | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const levelBufferRef = useRef<Uint8Array | null>(null);
  const overlayLevelTimerRef = useRef<number | null>(null);
  const lastOverlayLevelSentAtRef = useRef(0);
  const lastOverlayLevelRef = useRef(0);

  const selected = useMemo(
    () => recordings.find((item) => item.id === selectedId) ?? null,
    [recordings, selectedId]
  );
  const allHistorySelected = recordings.length > 0 && historySelectedIds.length === recordings.length;

  const modelReady = initStatus.ready;

  useEffect(() => {
    isRecordingRef.current = isRecording;
  }, [isRecording]);

  useEffect(() => {
    modelReadyRef.current = modelReady;
  }, [modelReady]);

  useEffect(() => {
    triggerModeRef.current = triggerMode;
  }, [triggerMode]);

  const uiLang: UiLang = useMemo(() => {
    if (langMode === "zh-CN") {
      return "zh";
    }
    if (langMode === "en-US") {
      return "en";
    }
    return detectSystemLang();
  }, [langMode]);

  const t = useMemo(() => {
    const dict = I18N[uiLang];
    return (key: keyof typeof dict, vars?: Record<string, string | number>) =>
      formatI18n(dict[key], vars);
  }, [uiLang]);

  useEffect(() => {
    const backendLang = uiLang === "zh" ? "zh-CN" : "en-US";
    invoke<HotkeySettings>("set_ui_language", { language: backendLang }).catch(() => {});
  }, [uiLang]);

  function normalizeHotkeyLabel(value: string) {
    return value.replace(/CommandOrControl/g, "Cmd/Ctrl").replace(/Alt/g, "Option/Alt");
  }

  function canonicalHotkey(value: string) {
    return value
      .split("+")
      .map((v) => v.trim())
      .filter(Boolean)
      .join("+")
      .toLowerCase();
  }

  function findSystemHotkeyConflict(value: string): string | null {
    if (!value.trim()) {
      return null;
    }
    const knownConflicts = [
      "CommandOrControl+Space",
      "CommandOrControl+Tab",
      "CommandOrControl+Q",
      "CommandOrControl+W",
      "CommandOrControl+M",
      "CommandOrControl+H",
      "CommandOrControl+`",
    ];
    const current = canonicalHotkey(value);
    const hit = knownConflicts.find((item) => canonicalHotkey(item) === current);
    return hit ?? null;
  }

  function buildShortcutFromKeyboardEvent(event: KeyboardEvent): string | null {
    const isModifierOnly = ["Shift", "Control", "Alt", "Meta"].includes(event.key);
    if (isModifierOnly) {
      return null;
    }
    let key = "";
    if (event.key === " ") {
      key = "Space";
    } else if (event.key === "Enter") {
      key = "Enter";
    } else if (event.key.length === 1 && /^[a-zA-Z0-9]$/.test(event.key)) {
      key = event.key.toUpperCase();
    } else if (/^F[0-9]{1,2}$/.test(event.key)) {
      key = event.key.toUpperCase();
    } else {
      return null;
    }
    const parts: string[] = [];
    if (event.metaKey || event.ctrlKey) {
      parts.push("CommandOrControl");
    }
    if (event.altKey) {
      parts.push("Alt");
    }
    if (event.shiftKey) {
      parts.push("Shift");
    }
    parts.push(key);
    return parts.join("+");
  }

  const dictationSystemConflict = useMemo(
    () => findSystemHotkeyConflict(hotkeyDictation),
    [hotkeyDictation]
  );
  const translationSystemConflict = useMemo(
    () => findSystemHotkeyConflict(hotkeyTranslation),
    [hotkeyTranslation]
  );
  const hasDuplicateHotkeys = useMemo(
    () =>
      Boolean(hotkeyDictation.trim()) &&
      Boolean(hotkeyTranslation.trim()) &&
      canonicalHotkey(hotkeyDictation) === canonicalHotkey(hotkeyTranslation),
    [hotkeyDictation, hotkeyTranslation]
  );
  const hasHotkeyConflicts = Boolean(dictationSystemConflict || translationSystemConflict || hasDuplicateHotkeys);

  async function setOverlayState(
    phase: "listening" | "thinking" | "ready",
    text?: string,
    level?: number,
    silent?: boolean
  ) {
    try {
      if (typeof level === "number") {
        await invoke("set_overlay_level", { phase, text: text ?? null, level });
      } else {
        await invoke("set_overlay_state", { phase, text: text ?? null });
      }
    } catch (err) {
      if (!silent) {
        setTranscript(`[overlay] ${String(err)}`);
      }
    }
  }

  async function hideOverlay() {
    try {
      await invoke("hide_overlay");
    } catch {
      // ignore
    }
  }

  function stopOverlayLevelMeter() {
    if (overlayLevelTimerRef.current !== null) {
      window.clearInterval(overlayLevelTimerRef.current);
      overlayLevelTimerRef.current = null;
    }
    if (audioContextRef.current) {
      void audioContextRef.current.close();
      audioContextRef.current = null;
    }
    analyserRef.current = null;
    levelBufferRef.current = null;
    lastOverlayLevelRef.current = 0;
    lastOverlayLevelSentAtRef.current = 0;
  }

  function startOverlayLevelMeter(stream: MediaStream) {
    stopOverlayLevelMeter();
    const AudioContextCtor = window.AudioContext || (window as Window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AudioContextCtor) {
      return;
    }
    const ctx = new AudioContextCtor();
    const src = ctx.createMediaStreamSource(stream);
    const analyser = ctx.createAnalyser();
    analyser.fftSize = 1024;
    analyser.smoothingTimeConstant = 0.65;
    src.connect(analyser);
    const buffer = new Uint8Array(analyser.fftSize);
    audioContextRef.current = ctx;
    analyserRef.current = analyser;
    levelBufferRef.current = buffer;

    overlayLevelTimerRef.current = window.setInterval(() => {
      if (!analyserRef.current || !levelBufferRef.current || !recordingByHotkeyRef.current || !isRecordingRef.current) {
        return;
      }
      analyserRef.current.getByteTimeDomainData(levelBufferRef.current);
      let sum = 0;
      for (let i = 0; i < levelBufferRef.current.length; i += 1) {
        const normalized = (levelBufferRef.current[i] - 128) / 128;
        sum += normalized * normalized;
      }
      const rms = Math.sqrt(sum / levelBufferRef.current.length);
      const amplified = Math.min(1, rms * 6.5);
      const prev = lastOverlayLevelRef.current;
      const smoothed = lastOverlayLevelRef.current * 0.7 + amplified * 0.3;
      lastOverlayLevelRef.current = smoothed;

      const now = Date.now();
      const shouldSend = Math.abs(smoothed - prev) > 0.025 || now - lastOverlayLevelSentAtRef.current > 160;
      if (!shouldSend) {
        return;
      }
      lastOverlayLevelSentAtRef.current = now;
      void setOverlayState("listening", undefined, smoothed, true);
    }, 80);
  }

  async function loadRecordings() {
    const items = await invoke<RecordingItem[]>("list_recordings");
    setRecordings(items);
    setSelectedId((prev) => {
      if (!prev) {
        return items.length > 0 ? items[0].id : null;
      }
      return items.some((item) => item.id === prev) ? prev : (items.length > 0 ? items[0].id : null);
    });
    setHistorySelectedIds((prev) => prev.filter((id) => items.some((item) => item.id === id)));
  }

  async function refreshUsageStats(items: RecordingItem[]) {
    const now = new Date();
    const todayKey = `${now.getFullYear()}-${now.getMonth()}-${now.getDate()}`;
    const month = now.getMonth();
    const year = now.getFullYear();

    const recentKeys: string[] = [];
    const recentLabels = new Map<string, string>();
    for (let offset = HOME_STATS_DAYS - 1; offset >= 0; offset -= 1) {
      const d = new Date(now);
      d.setDate(now.getDate() - offset);
      const key = `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
      recentKeys.push(key);
      const mm = String(d.getMonth() + 1).padStart(2, "0");
      const dd = String(d.getDate()).padStart(2, "0");
      recentLabels.set(key, `${mm}/${dd}`);
    }

    const charByDay = new Map<string, number>();
    for (const key of recentKeys) {
      charByDay.set(key, 0);
    }

    let todayChars = 0;
    let monthChars = 0;
    let yearChars = 0;
    let totalChars = 0;

    const texts = await Promise.all(
      items.map((item) =>
        invoke<string | null>("get_recording_cached_transcript", { id: item.id }).catch(() => null)
      )
    );

    items.forEach((item, idx) => {
      const text = texts[idx] ?? "";
      const chars = Array.from(text).length;
      if (!chars) {
        return;
      }
      totalChars += chars;
      const d = new Date(item.createdAtMs);
      if (d.getFullYear() === year) {
        yearChars += chars;
        if (d.getMonth() === month) {
          monthChars += chars;
        }
      }
      const dayKey = `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
      if (dayKey === todayKey) {
        todayChars += chars;
      }
      if (charByDay.has(dayKey)) {
        charByDay.set(dayKey, (charByDay.get(dayKey) ?? 0) + chars);
      }
    });

    const dailyPoints = recentKeys.map((key) => ({
      dateLabel: recentLabels.get(key) ?? key,
      chars: charByDay.get(key) ?? 0,
    }));
    const dailyAvgChars = Math.round(dailyPoints.reduce((sum, p) => sum + p.chars, 0) / Math.max(1, dailyPoints.length));
    setUsageStats({
      todayChars,
      dailyAvgChars,
      monthChars,
      yearChars,
      totalChars,
      dailyPoints,
    });
  }

  async function loadInitStatus() {
    const status = await invoke<ModelInitStatus>("get_model_init_status");
    setInitStatus(status);
  }

  async function loadGlobalShortcuts() {
    const settings = await invoke<HotkeySettings>("get_global_shortcuts");
    setHotkeyDictation(settings.dictation);
    setHotkeyTranslation(settings.translation);
    setFnKeyEnabled(settings.fnEnabled);
    setTriggerMode(settings.triggerMode);
    setOverlayPosition(settings.overlayPosition);
    setOutputMode(settings.outputMode);
    setTranslationTargetLang(settings.translationTarget);
  }

  async function loadCloudSettings() {
    try {
      const settings = await invoke<CloudSettings>("get_cloud_settings");
      setCloudSettings(settings);
    } catch {
      setCloudSettings(defaultCloudSettings());
    }
  }

  async function refreshAccessibilityStatus() {
    try {
      const status = await invoke<AccessibilityStatus>("get_accessibility_status");
      setAccessibility(status);
    } catch {
      setAccessibility({ supported: false, trusted: false });
    }
  }

  useEffect(() => {
    const raw = window.localStorage.getItem(DICTIONARY_STORAGE_KEY);
    if (!raw) {
      return;
    }
    try {
      const words = JSON.parse(raw) as string[];
      if (Array.isArray(words)) {
        setDictionaryWords(words.filter((word) => typeof word === "string"));
      }
    } catch {
      window.localStorage.removeItem(DICTIONARY_STORAGE_KEY);
    }
  }, []);

  useEffect(() => {
    window.localStorage.setItem(DICTIONARY_STORAGE_KEY, JSON.stringify(dictionaryWords));
  }, [dictionaryWords]);

  useEffect(() => {
    window.localStorage.setItem(LANG_MODE_STORAGE_KEY, langMode);
  }, [langMode]);

  useEffect(() => () => {
    stopOverlayLevelMeter();
  }, []);

  useEffect(() => {
    if (!historyMenuId) {
      return;
    }
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Element | null;
      if (target?.closest("[data-history-menu]")) {
        return;
      }
      setHistoryMenuId(null);
    };
    window.addEventListener("mousedown", onPointerDown, true);
    return () => window.removeEventListener("mousedown", onPointerDown, true);
  }, [historyMenuId]);

  useEffect(() => {
    Promise.all([loadRecordings(), loadInitStatus(), refreshAccessibilityStatus(), loadGlobalShortcuts(), loadCloudSettings()]).catch((err) => {
      setTranscript(t("transcriptInitFailed", { error: String(err) }));
    });

    let unlistenInit: (() => void) | undefined;
    let unlistenHotkey: (() => void) | undefined;
    let unlistenRecordingSaved: (() => void) | undefined;

    listen<ModelInitStatus>("model-init-progress", (event) => {
      setInitStatus(event.payload);
    })
      .then((fn) => {
        unlistenInit = fn;
      })
      .catch((err) => {
        setTranscript(t("transcriptListenFailed", { error: String(err) }));
      });

    listen<GlobalShortcutPayload>("global-shortcut-triggered", (event) => {
      if (BACKEND_NATIVE_HOTKEY_PIPELINE) {
        return;
      }
      void onHotkeyEvent(event.payload);
    })
      .then((fn) => {
        unlistenHotkey = fn;
      })
      .catch((err) => {
        setTranscript(t("transcriptListenFailed", { error: String(err) }));
      });

    listen<RecordingSavedPayload>("recording-saved", () => {
      void loadRecordings();
    })
      .then((fn) => {
        unlistenRecordingSaved = fn;
      })
      .catch(() => {});

    return () => {
      if (unlistenInit) {
        unlistenInit();
      }
      if (unlistenHotkey) {
        unlistenHotkey();
      }
      if (unlistenRecordingSaved) {
        unlistenRecordingSaved();
      }
    };
  }, [t]);

  useEffect(() => {
    getVersion()
      .then((version) => setAppVersion(version))
      .catch(() => setAppVersion("-"));
  }, []);

  useEffect(() => {
    void refreshUsageStats(recordings);
  }, [recordings]);

  useEffect(() => {
    if (!selectedId) {
      setTranscript("");
      return;
    }
    invoke<string | null>("get_recording_cached_transcript", { id: selectedId })
      .then((cachedText) => {
        setTranscript(cachedText ?? "");
      })
      .catch(() => {});
  }, [selectedId]);

  useEffect(() => {
    if (!captureTarget) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape" && !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey) {
        setCaptureTarget(null);
        return;
      }
      const shortcut = buildShortcutFromKeyboardEvent(event);
      if (!shortcut) {
        return;
      }
      if (captureTarget === "dictation") {
        setHotkeyDictation(shortcut);
      } else if (captureTarget === "translation") {
        setHotkeyTranslation(shortcut);
      }
      setCaptureTarget(null);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [captureTarget]);

  async function onInitModel() {
    try {
      const status = await invoke<ModelInitStatus>("init_model");
      setInitStatus(status);
      if (status.ready) {
        setTranscript(t("transcriptModelReady"));
      }
    } catch (err) {
      setTranscript(t("transcriptModelInitFailed", { error: String(err) }));
    }
  }

  async function tryTypeToFocusedApp(text: string) {
    try {
      await invoke("type_text_to_focused_app", { text });
      return true;
    } catch (err) {
      setTranscript(t("transcriptTypeFailed", { error: String(err) }));
      return false;
    }
  }

  function inferTranslationTarget(text: string): Exclude<TranslationTargetLang, "auto"> {
    const hasCjk = /[\u3040-\u30ff\u3400-\u9fff\uf900-\ufaff]/.test(text);
    return hasCjk ? "en" : "zh-CN";
  }

  async function runHotkeyPostProcess(text: string, action: HotkeyAction | null) {
    const source = text.trim();
    if (!source) {
      return source;
    }
    const isTranslate = action === "toggle-translation";
    const targetLang = isTranslate
      ? translationTargetLang === "auto"
        ? inferTranslationTarget(source)
        : translationTargetLang
      : undefined;
    try {
      if (isTranslate) {
        setTranscript(t("transcriptTranslating"));
        await setOverlayState("thinking", t("transcriptTranslating"));
      }
      const result = await invoke<CloudProcessResult>("process_text_with_cloud", {
        text: source,
        translate: isTranslate,
        targetLang,
      });
      return (result.finalText || source).trim();
    } catch (err) {
      setTranscript(t("transcriptCloudProcessFailed", { error: String(err) }));
      return source;
    }
  }

  async function startRecording() {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    streamRef.current = stream;
    chunksRef.current = [];

    const recorder = new MediaRecorder(stream);
    recorderRef.current = recorder;

    recorder.ondataavailable = (event) => {
      if (event.data.size > 0) {
        chunksRef.current.push(event.data);
      }
    };

    recorder.onstop = async () => {
      try {
        setIsBusy(true);
        const blob = new Blob(chunksRef.current, { type: "audio/webm" });
        const wav = await blobToMono16kWav(blob);
        const wavData = Array.from(new Uint8Array(wav));
        const payload = {
          suggestedName: `${t("recordingPrefix")}_${new Date().toISOString().replace(/[:.]/g, "-")}`,
          wavData,
        };

        const result = await invoke<SaveAndTranscribeResult>("save_recording_and_transcribe", { payload });
        const postProcessed = await runHotkeyPostProcess(result.text || "", activeHotkeyActionRef.current);
        setRecordings((prev) => [result.recording, ...prev]);
        setSelectedId(result.recording.id);
        setTranscript(postProcessed || "");

        if (recordingByHotkeyRef.current) {
          const ok = await tryTypeToFocusedApp(postProcessed || "");
          if (!ok && postProcessed) {
            setFallbackText(postProcessed);
          }
          await setOverlayState("ready", ok ? "" : postProcessed);
          window.setTimeout(() => {
            void hideOverlay();
          }, 1800);
        } else {
          setPage("history");
        }
      } catch (err) {
        setTranscript(t("transcriptRecordingFailed", { error: String(err) }));
        if (recordingByHotkeyRef.current) {
          await setOverlayState("ready", t("transcriptRecordingFailed", { error: String(err) }));
        }
      } finally {
        recordingByHotkeyRef.current = false;
        activeHoldActionRef.current = null;
        activeHotkeyActionRef.current = null;
        pendingStartActionRef.current = null;
        cancelAfterStartActionRef.current = null;
        setIsBusy(false);
      }
    };

    recorder.start();
    if (recordingByHotkeyRef.current) {
      startOverlayLevelMeter(stream);
    }
    isRecordingRef.current = true;
    setIsRecording(true);
  }

  function stopRecording() {
    if (recorderRef.current && recorderRef.current.state !== "inactive") {
      recorderRef.current.stop();
    }
    streamRef.current?.getTracks().forEach((track) => track.stop());
    stopOverlayLevelMeter();
    streamRef.current = null;
    recorderRef.current = null;
    isRecordingRef.current = false;
    setIsRecording(false);
  }

  async function ensureModelReadyForHotkey() {
    if (!modelReadyRef.current) {
      try {
        const status = await invoke<ModelInitStatus>("init_model");
        setInitStatus(status);
        if (!status.ready) {
          const msg = t("transcriptInitInProgress");
          setTranscript(msg);
          await setOverlayState("ready", msg);
          window.setTimeout(() => {
            void hideOverlay();
          }, 1600);
          return;
        }
      } catch (err) {
        const msg = t("transcriptModelInitFailed", { error: String(err) });
        setTranscript(msg);
        await setOverlayState("ready", msg);
        window.setTimeout(() => {
          void hideOverlay();
        }, 1800);
        return;
      }
    }
    return true;
  }

  async function startRecordingFromHotkey(action: HotkeyAction) {
    if (isBusy || isRecordingRef.current) {
      return;
    }
    pendingStartActionRef.current = action;
    const ready = await ensureModelReadyForHotkey();
    if (!ready) {
      pendingStartActionRef.current = null;
      return;
    }
    try {
      recordingByHotkeyRef.current = true;
      activeHoldActionRef.current = action;
      activeHotkeyActionRef.current = action;
      await startRecording();
      await setOverlayState("listening");
      if (
        triggerModeRef.current === "long-press" &&
        cancelAfterStartActionRef.current === action
      ) {
        cancelAfterStartActionRef.current = null;
        await stopRecordingFromHotkey();
        activeHoldActionRef.current = null;
      }
    } catch (err) {
      setTranscript(t("transcriptRecordingFailed", { error: String(err) }));
      await setOverlayState("ready", t("transcriptRecordingFailed", { error: String(err) }));
    } finally {
      if (pendingStartActionRef.current === action) {
        pendingStartActionRef.current = null;
      }
    }
  }

  async function stopRecordingFromHotkey() {
    if (!isRecordingRef.current) {
      return;
    }
    await setOverlayState("thinking");
    stopRecording();
  }

  async function onHotkeyEvent(payload: GlobalShortcutPayload) {
    const now = Date.now();
    if (payload.action === "toggle-translation") {
      suppressDictationUntilRef.current = now + 260;
    }
    if (payload.action === "toggle-dictation" && now < suppressDictationUntilRef.current) {
      return;
    }
    if (payload.action === "toggle-dictation") {
      suppressTranslationUntilRef.current = now + 130;
    } else if (now < suppressTranslationUntilRef.current) {
      return;
    }

    if (triggerModeRef.current === "tap") {
      if (payload.state !== "pressed") {
        return;
      }
      const lastPressed = lastPressedAtRef.current[payload.action];
      if (now - lastPressed < 260) {
        return;
      }
      lastPressedAtRef.current[payload.action] = now;
      if (isRecordingRef.current) {
        await stopRecordingFromHotkey();
      } else {
        await startRecordingFromHotkey(payload.action);
      }
      return;
    }

    if (payload.state === "pressed") {
      if (isRecordingRef.current) {
        return;
      }
      await startRecordingFromHotkey(payload.action);
      return;
    }

    if (
      payload.state === "released" &&
      recordingByHotkeyRef.current &&
      activeHoldActionRef.current === payload.action
    ) {
      await stopRecordingFromHotkey();
      activeHoldActionRef.current = null;
      return;
    }

    if (
      payload.state === "released" &&
      triggerModeRef.current === "long-press" &&
      pendingStartActionRef.current === payload.action
    ) {
      cancelAfterStartActionRef.current = payload.action;
    }
  }

  async function onRetranscribeSelected() {
    if (!selected) {
      setTranscript(t("transcriptNeedSelect"));
      return;
    }
    setIsBusy(true);
    try {
      const text = await invoke<string>("transcribe_recording", { id: selected.id, force: true });
      setTranscript(text || "");
    } catch (err) {
      setTranscript(t("transcriptRetryFailed", { error: String(err) }));
    } finally {
      setIsBusy(false);
    }
  }

  async function onCopyTranscript() {
    if (!transcript.trim()) {
      return;
    }
    try {
      await navigator.clipboard.writeText(transcript);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch (err) {
      setTranscript((prev) => `${prev}\n\n${t("transcriptCopyFailed", { error: String(err) })}`);
    }
  }

  async function onRename(recording: RecordingItem) {
    const title = uiLang === "zh" ? "输入新名称" : "Enter new name";
    const name = window.prompt(title, recording.name);
    if (!name) {
      return;
    }
    try {
      const renamed = await invoke<RecordingItem>("rename_recording", { id: recording.id, newName: name });
      setRecordings((prev) => prev.map((item) => (item.id === recording.id ? renamed : item)));
      if (selectedId === recording.id) {
        setSelectedId(renamed.id);
      }
    } catch (err) {
      setTranscript(t("transcriptRenameFailed", { error: String(err) }));
    }
  }

  async function onDelete(recording: RecordingItem) {
    try {
      await invoke("delete_recording", { id: recording.id });
      setHistorySelectedIds((prev) => prev.filter((id) => id !== recording.id));
      setRecordings((prev) => {
        const next = prev.filter((item) => item.id !== recording.id);
        if (selectedId === recording.id) {
          setSelectedId(next.length > 0 ? next[0].id : null);
          setTranscript("");
        }
        return next;
      });
    } catch (err) {
      setTranscript(t("transcriptDeleteFailed", { error: String(err) }));
    }
  }

  function onToggleHistoryItem(id: string, checked: boolean) {
    setHistorySelectedIds((prev) => {
      if (checked) {
        if (prev.includes(id)) {
          return prev;
        }
        return [...prev, id];
      }
      return prev.filter((item) => item !== id);
    });
  }

  async function onDeleteSelectedHistory() {
    if (historySelectedIds.length === 0) {
      return;
    }
    const ok = window.confirm(t("historyDeleteSelectedConfirm", { count: historySelectedIds.length }));
    if (!ok) {
      return;
    }
    try {
      await Promise.all(historySelectedIds.map((id) => invoke("delete_recording", { id })));
      const selectedSet = new Set(historySelectedIds);
      setRecordings((prev) => {
        const next = prev.filter((item) => !selectedSet.has(item.id));
        if (selectedId && selectedSet.has(selectedId)) {
          setSelectedId(next.length > 0 ? next[0].id : null);
          setTranscript("");
        }
        return next;
      });
      setHistorySelectedIds([]);
      setHistoryMenuId(null);
    } catch (err) {
      setTranscript(t("transcriptDeleteFailed", { error: String(err) }));
    }
  }

  async function onOpenTempDir() {
    try {
      await invoke<string>("open_temp_dir");
    } catch (err) {
      setTranscript(t("transcriptOpenTempDirFailed", { error: String(err) }));
    }
  }

  async function onRequestAccessibilityPermission() {
    try {
      const status = await invoke<AccessibilityStatus>("request_accessibility_permission");
      setAccessibility(status);
    } catch {
      await refreshAccessibilityStatus();
    }
  }

  async function onOpenAccessibilitySettings() {
    try {
      await invoke("open_accessibility_settings");
    } catch (err) {
      setTranscript(String(err));
    }
  }

  async function onSaveHotkeys() {
    const dictation = hotkeyDictation.trim();
    const translation = hotkeyTranslation.trim();
    if (hasHotkeyConflicts) {
      if (hasHotkeyConflicts) {
        setTranscript(t("settingsHotkeyWarningSaveBlocked"));
      }
      return;
    }
    setSavingHotkeys(true);
    try {
      const next = await invoke<HotkeySettings>("set_global_shortcuts", {
        dictation,
        translation,
        triggerMode,
        overlayPosition,
        outputMode,
        translationTarget: translationTargetLang,
      });
      setHotkeyDictation(next.dictation);
      setHotkeyTranslation(next.translation);
      setFnKeyEnabled(next.fnEnabled);
      setTriggerMode(next.triggerMode);
      setOverlayPosition(next.overlayPosition);
      setOutputMode(next.outputMode);
      setTranslationTargetLang(next.translationTarget);
    } catch (err) {
      setTranscript(String(err));
    } finally {
      setSavingHotkeys(false);
    }
  }

  async function onResetHotkeys() {
    setHotkeyDictation(DEFAULT_HOTKEY_DICTATION);
    setHotkeyTranslation(DEFAULT_HOTKEY_TRANSLATION);
    setTriggerMode(DEFAULT_TRIGGER_MODE);
    setOverlayPosition(DEFAULT_OVERLAY_POSITION);
    setOutputMode(DEFAULT_OUTPUT_MODE);
    setSavingHotkeys(true);
    try {
      const next = await invoke<HotkeySettings>("set_global_shortcuts", {
        dictation: DEFAULT_HOTKEY_DICTATION,
        translation: DEFAULT_HOTKEY_TRANSLATION,
        triggerMode: DEFAULT_TRIGGER_MODE,
        overlayPosition: DEFAULT_OVERLAY_POSITION,
        outputMode: DEFAULT_OUTPUT_MODE,
        translationTarget: "auto",
      });
      setHotkeyDictation(next.dictation);
      setHotkeyTranslation(next.translation);
      setFnKeyEnabled(next.fnEnabled);
      setTriggerMode(next.triggerMode);
      setOverlayPosition(next.overlayPosition);
      setOutputMode(next.outputMode);
      setTranslationTargetLang(next.translationTarget);
    } catch (err) {
      setTranscript(String(err));
    } finally {
      setSavingHotkeys(false);
    }
  }

  async function onToggleFnKeyEnabled(nextEnabled: boolean) {
    setSavingHotkeys(true);
    try {
      const next = await invoke<HotkeySettings>("set_fn_key_enabled", { enabled: nextEnabled });
      setHotkeyDictation(next.dictation);
      setHotkeyTranslation(next.translation);
      setFnKeyEnabled(next.fnEnabled);
      setTriggerMode(next.triggerMode);
      setOverlayPosition(next.overlayPosition);
      setOutputMode(next.outputMode);
      setTranslationTargetLang(next.translationTarget);
    } catch (err) {
      setTranscript(String(err));
    } finally {
      setSavingHotkeys(false);
    }
  }

  function updateCloudPipeline<K extends keyof CloudPipelineConfig>(key: K, value: CloudPipelineConfig[K]) {
    setCloudSettings((prev) => ({
      ...prev,
      pipeline: {
        ...prev.pipeline,
        [key]: value,
      },
    }));
  }

  function updateCloudProvider(index: number, patch: Partial<CloudProviderConfig>) {
    setCloudSettings((prev) => {
      const providers = prev.providers.map((provider, idx) => {
        if (idx !== index) {
          return provider;
        }
        const next = { ...provider, ...patch };
        return {
          ...next,
          name: buildProviderLabel(next),
        };
      });
      return { ...prev, providers };
    });
  }

  function addCloudProvider() {
    const vendor: CloudVendor = "openai";
    const model = "gpt-4o-mini";
    setCloudSettings((prev) => ({
      ...prev,
      providers: [
        ...prev.providers,
        {
          id: makeProviderId(vendor, model),
          name: buildProviderLabel({ vendor, model }),
          vendor,
          model,
          apiKey: "",
          baseUrl: "",
          enabled: true,
          priority: prev.providers.length,
        },
      ],
    }));
  }

  function removeCloudProvider(index: number) {
    setCloudSettings((prev) => {
      const removed = prev.providers[index];
      const providers = prev.providers.filter((_, idx) => idx !== index);
      const optimizeProviderId =
        prev.pipeline.optimizeProviderId === removed?.id ? "" : prev.pipeline.optimizeProviderId;
      const translateProviderId =
        prev.pipeline.translateProviderId === removed?.id ? "" : prev.pipeline.translateProviderId;
      return {
        providers,
        pipeline: {
          ...prev.pipeline,
          optimizeProviderId,
          translateProviderId,
        },
      };
    });
  }

  async function onSaveCloudSettings() {
    setSavingCloudSettings(true);
    try {
      const providers = cloudSettings.providers.map((provider, index) => ({
        ...provider,
        id: provider.id.trim() || makeProviderId(provider.vendor, provider.model),
        name: buildProviderLabel(provider),
        model: provider.model.trim(),
        apiKey: provider.apiKey.trim(),
        baseUrl: provider.baseUrl?.trim() || null,
        priority: Number.isFinite(provider.priority) ? provider.priority : index,
      }));
      const payload: CloudSettings = {
        providers,
        pipeline: {
          ...cloudSettings.pipeline,
          targetLanguage: cloudSettings.pipeline.targetLanguage.trim() || "en",
          optimizePrompt: cloudSettings.pipeline.optimizePrompt.trim(),
          translatePrompt: cloudSettings.pipeline.translatePrompt.trim(),
          timeoutMs: Math.max(1000, Math.min(120000, Number(cloudSettings.pipeline.timeoutMs) || 10000)),
          maxRetries: Math.max(0, Math.min(4, Number(cloudSettings.pipeline.maxRetries) || 0)),
        },
      };
      const saved = await invoke<CloudSettings>("set_cloud_settings", { settings: payload });
      setCloudSettings(saved);
    } catch (err) {
      setTranscript(String(err));
    } finally {
      setSavingCloudSettings(false);
    }
  }

  async function onSaveProvidersOnly() {
    await onSaveCloudSettings();
  }

  async function onTestCloudProvider(providerId: string) {
    try {
      const result = await invoke<TestCloudProviderResult>("test_cloud_provider", {
        input: { providerId },
      });
      const message = result.ok
        ? `${providerId}: ${result.message}`
        : `${providerId}: ${result.message}`;
      setTranscript(message);
    } catch (err) {
      setTranscript(String(err));
    }
  }

  function addDictionaryWord() {
    const word = newWord.trim();
    if (!word) {
      return;
    }
    const exists = dictionaryWords.some((item) => item.toLowerCase() === word.toLowerCase());
    if (!exists) {
      setDictionaryWords((prev) => [word, ...prev]);
    }
    setNewWord("");
  }

  function removeDictionaryWord(word: string) {
    setDictionaryWords((prev) => prev.filter((item) => item !== word));
  }

  const navItems: Array<{ key: Page; label: string; icon: typeof Home }> = [
    { key: "home", label: t("navHome"), icon: Home },
    { key: "history", label: t("navHistory"), icon: History },
    { key: "dictionary", label: t("navDictionary"), icon: BookText },
    { key: "cloud", label: t("settingsSectionCloud"), icon: Sparkles },
  ];
  const dailyInputChartData = useMemo(
    () => ({
      labels: usageStats.dailyPoints.map((item) => item.dateLabel),
      datasets: [
        {
          label: t("statsDailyInputTitle"),
          data: usageStats.dailyPoints.map((item) => item.chars),
          borderColor: "rgb(37, 99, 235)",
          backgroundColor: (context: any) => {
            const chart = context.chart;
            const { ctx, chartArea } = chart;
            if (!chartArea) {
              return "rgba(37, 99, 235, 0.24)";
            }
            const gradient = ctx.createLinearGradient(0, chartArea.top, 0, chartArea.bottom);
            gradient.addColorStop(0, "rgba(37, 99, 235, 0.30)");
            gradient.addColorStop(1, "rgba(37, 99, 235, 0.04)");
            return gradient;
          },
          tension: 0.35,
          fill: true,
          pointRadius: 2,
          pointHoverRadius: 4,
          borderWidth: 2.5,
        },
      ],
    }),
    [usageStats.dailyPoints, t]
  );
  const dailyInputChartOptions = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: {
          display: false,
        },
        tooltip: {
          callbacks: {
            label: (ctx: any) => `${ctx.parsed?.y ?? 0} ${t("statsUnitChars")}`,
          },
        },
      },
      scales: {
        x: {
          grid: {
            color: "rgba(148, 163, 184, 0.18)",
          },
          ticks: {
            maxRotation: 0,
            color: "#64748b",
            font: { size: 10 },
          },
        },
        y: {
          beginAtZero: true,
          grid: {
            color: "rgba(148, 163, 184, 0.18)",
          },
          ticks: {
            color: "#64748b",
            font: { size: 10 },
          },
        },
      },
    }),
    [t]
  );
  const settingsSections: Array<{ key: SettingsSection; label: string }> = [
    { key: "language", label: t("settingsSectionLanguage") },
    { key: "hotkey", label: t("settingsSectionHotkey") },
    { key: "providers", label: t("settingsSectionProviders") },
    { key: "temp", label: t("settingsSectionTemp") },
    { key: "about", label: t("settingsSectionAbout") },
  ];
  const currentSettingsLabel =
    settingsSections.find((item) => item.key === settingsSection)?.label ?? t("settingsTitle");

  return (
    <main className="typemore-app h-screen p-0 text-slate-900">
      <div className="tm-shell mx-auto grid h-full max-w-[1540px] grid-cols-1 gap-3 rounded-3xl p-3 backdrop-blur md:grid-cols-[230px_1fr] md:p-4">
        <aside className="tm-side flex min-h-0 flex-col rounded-2xl bg-white/95 p-3">
          <div className="px-2 pb-3 pt-1">
            <div className="flex items-center gap-2">
              <img src="/favicon.png" alt="Type More" className="h-7 w-7 rounded-md" />
              <div className="text-2xl font-bold tracking-tight">Type More</div>
            </div>
          </div>

          <nav className="space-y-1">
            {navItems.map((item) => {
              const Icon = item.icon;
              const active = page === item.key;
              return (
                <button
                  key={item.key}
                  type="button"
                  onClick={() => setPage(item.key)}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left text-sm font-medium transition",
                    active ? "bg-slate-900 text-white" : "text-slate-700 hover:bg-slate-100"
                  )}
                >
                  <Icon size={16} />
                  {item.label}
                </button>
              );
            })}
          </nav>

          <div className="mt-auto space-y-3">
            <Separator />
            <button
              type="button"
              className="flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left text-sm font-medium text-slate-700 transition hover:bg-slate-100"
              onClick={() => {
                setSettingsSection("language");
                setSettingsOpen(true);
              }}
            >
              <Settings size={16} />
              {t("navSettings")}
            </button>
          </div>
        </aside>

        <section className="tm-main min-h-0 overflow-y-auto rounded-2xl bg-white/95 p-4 md:p-5">
          {page === "home" && (
            <div className="grid min-h-full gap-4 md:grid-rows-[auto_auto_1fr]">
              <header className="flex flex-wrap items-start justify-between gap-3">
                <div>
                  <h1 className="text-3xl font-semibold tracking-tight">{t("titleHome")}</h1>
                  <p className="mt-1 text-sm text-slate-500">{t("subHome")}</p>
                </div>
                <div className="flex items-center gap-2">
                  <Badge className={cn(badgeClass(modelReady))}>
                    {modelReady ? t("modelReady") : initStatus.running ? t("modelInitializing") : t("modelNotReady")}
                  </Badge>
                  <Badge className="border-slate-200 bg-slate-100 text-slate-700">
                    {t("statsVersion")}: v{appVersion}
                  </Badge>
                </div>
              </header>

              <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-5">
                <Card className="p-3">
                  <div className="text-xs text-slate-500">{t("statsToday")}</div>
                  <div className="mt-1 text-2xl font-semibold text-slate-900">{usageStats.todayChars.toLocaleString()}</div>
                  <div className="text-xs text-slate-500">{t("statsUnitChars")}</div>
                </Card>
                <Card className="p-3">
                  <div className="text-xs text-slate-500">{t("statsDailyAvg")}</div>
                  <div className="mt-1 text-2xl font-semibold text-slate-900">{usageStats.dailyAvgChars.toLocaleString()}</div>
                  <div className="text-xs text-slate-500">{t("statsUnitChars")}</div>
                </Card>
                <Card className="p-3">
                  <div className="text-xs text-slate-500">{t("statsMonthTotal")}</div>
                  <div className="mt-1 text-2xl font-semibold text-slate-900">{usageStats.monthChars.toLocaleString()}</div>
                  <div className="text-xs text-slate-500">{t("statsUnitChars")}</div>
                </Card>
                <Card className="p-3">
                  <div className="text-xs text-slate-500">{t("statsYearTotal")}</div>
                  <div className="mt-1 text-2xl font-semibold text-slate-900">{usageStats.yearChars.toLocaleString()}</div>
                  <div className="text-xs text-slate-500">{t("statsUnitChars")}</div>
                </Card>
                <Card className="p-3">
                  <div className="text-xs text-slate-500">{t("statsTotal")}</div>
                  <div className="mt-1 text-2xl font-semibold text-slate-900">{usageStats.totalChars.toLocaleString()}</div>
                  <div className="text-xs text-slate-500">{t("statsUnitChars")}</div>
                </Card>
              </div>

              <Card className="p-4">
                <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
                  <div>
                    <div className="text-lg font-semibold text-slate-900">{t("statsDailyInputTitle")}</div>
                    <p className="mt-1 text-sm text-slate-600">{t("statsDailyInputDesc")}</p>
                  </div>
                  <div className="flex gap-2">
                    <Button variant="outline" className="h-10" onClick={onInitModel} disabled={isBusy || initStatus.running}>
                      {initStatus.running ? <Loader2 className="animate-spin" size={14} /> : <Sparkles size={14} />}
                      {initStatus.running ? t("initModelRunning") : modelReady ? t("initModelReady") : t("initModelStart")}
                    </Button>
                  </div>
                </div>
                <div className="h-[340px] w-full">
                  <Line data={dailyInputChartData} options={dailyInputChartOptions} />
                </div>
              </Card>

              {accessibility.supported && !accessibility.trusted && (
                <Card className="p-4">
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <div>
                      <div className="inline-flex items-center gap-2 text-base font-semibold text-slate-800">
                        {accessibility.trusted ? <ShieldCheck size={18} className="text-emerald-600" /> : <ShieldAlert size={18} className="text-amber-600" />}
                        {t("accessTitle")}
                      </div>
                      <p className="mt-1 text-sm text-slate-600">{accessibility.trusted ? t("accessGranted") : t("accessNeed")}</p>
                      <p className="mt-1 text-xs text-slate-500">
                        AX: {String(accessibility.axTrusted ?? false)} | TCC: {accessibility.tccAllowed == null ? "unknown" : String(accessibility.tccAllowed)}
                      </p>
                      {accessibility.runtimeHint && <p className="mt-1 text-xs text-amber-600">{accessibility.runtimeHint}</p>}
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <Button variant="outline" onClick={onRequestAccessibilityPermission}>{t("accessRequest")}</Button>
                      <Button variant="outline" onClick={onOpenAccessibilitySettings}>{t("accessOpenSettings")}</Button>
                      <Button variant="outline" onClick={refreshAccessibilityStatus}>{t("accessRefresh")}</Button>
                    </div>
                  </div>
                </Card>
              )}
            </div>
          )}

          {page === "history" && (
            <div className="grid h-full min-h-0 gap-4 md:grid-cols-[340px_1fr]">
              <Card className="flex min-h-0 flex-col overflow-hidden">
                <div className="border-b border-slate-200 px-4 py-3">
                  <div className="flex items-center justify-between gap-2">
                    <div className="inline-flex items-center gap-2">
                      <div className="whitespace-nowrap text-lg font-semibold">{t("historyTitle")}</div>
                      <span className="rounded-full border border-slate-200 bg-slate-50 px-2 py-0.5 text-xs text-slate-500">
                        {t("countItems", { count: recordings.length })}
                      </span>
                    </div>
                    <Button
                      variant="outline"
                      className="h-8 w-8 justify-center p-0 text-[15px] leading-none"
                      onClick={() => {
                        void loadRecordings();
                      }}
                      title={t("historyRefresh")}
                    >
                      ↻
                    </Button>
                  </div>
                  <div className="mt-3 flex items-center justify-between gap-2 rounded-lg border border-slate-200 bg-slate-50/70 px-2 py-2">
                    <Button
                      variant="outline"
                      className="h-8 px-3 py-1 text-xs whitespace-nowrap"
                      onClick={() => {
                        setHistorySelectedIds(allHistorySelected ? [] : recordings.map((item) => item.id));
                      }}
                      disabled={recordings.length === 0}
                    >
                      {allHistorySelected ? t("historyClearSelection") : t("historySelectAll")}
                    </Button>
                    <div className="inline-flex items-center gap-2">
                      <span className="whitespace-nowrap rounded-md bg-white px-2 py-1 text-xs text-slate-500">
                        {t("historySelectedCount", { count: historySelectedIds.length })}
                      </span>
                    <Button
                      variant="outline"
                      className={cn(
                        "h-8 px-3 py-1 text-xs whitespace-nowrap",
                        historySelectedIds.length > 0
                          ? "border-red-300 bg-red-50 text-red-700 hover:border-red-400 hover:bg-red-100"
                          : "text-slate-400"
                      )}
                      onClick={() => {
                        void onDeleteSelectedHistory();
                      }}
                      disabled={historySelectedIds.length === 0}
                    >
                      {t("historyDeleteSelected")}
                    </Button>
                    </div>
                  </div>
                </div>

                <ScrollArea className="min-h-0 flex-1" viewportClassName="h-full w-full p-3">
                  <ul className="space-y-2">
                    {recordings.map((item) => (
                      <li key={item.id} className="group relative">
                        <div
                          role="button"
                          tabIndex={0}
                          className={cn(
                            "rounded-xl border px-3 py-1.5 transition",
                            selectedId === item.id ? "border-sky-300 bg-sky-50" : "border-slate-200 bg-white hover:bg-slate-50"
                          )}
                          onClick={() => {
                            setSelectedId(item.id);
                            setHistoryMenuId(null);
                          }}
                          onContextMenu={(event) => {
                            event.preventDefault();
                            setSelectedId(item.id);
                            setHistoryMenuId(item.id);
                          }}
                          onKeyDown={(event) => {
                            if (event.key === "Enter" || event.key === " ") {
                              event.preventDefault();
                              setSelectedId(item.id);
                              setHistoryMenuId(null);
                            }
                          }}
                        >
                          <div className="flex items-start justify-between gap-2">
                            <div className="flex min-w-0 items-start gap-2">
                              <input
                                type="checkbox"
                                className="mt-1 h-4 w-4 rounded border-slate-300 accent-sky-600"
                                checked={historySelectedIds.includes(item.id)}
                                onClick={(event) => {
                                  event.stopPropagation();
                                }}
                                onChange={(event) => {
                                  onToggleHistoryItem(item.id, event.target.checked);
                                }}
                              />
                              <div className="min-w-0">
                                <div className="line-clamp-2 text-sm font-medium leading-5 text-slate-800">{item.name}</div>
                                <div className="mt-1 text-xs text-slate-500">{formatListTime(item.createdAtMs)}</div>
                              </div>
                            </div>
                            <div className="relative self-center" data-history-menu>
                              <Button
                                variant="outline"
                                className="h-6 w-6 justify-center p-0 text-[15px] leading-none text-slate-500 opacity-0 transition-opacity group-hover:opacity-100"
                                onClick={(event) => {
                                  event.stopPropagation();
                                  setHistoryMenuId((prev) => (prev === item.id ? null : item.id));
                                }}
                                title={uiLang === "zh" ? "更多操作" : "More actions"}
                              >
                                ⋯
                              </Button>
                              {historyMenuId === item.id && (
                                <div className="absolute right-0 top-7 z-20 w-32 rounded-md border border-slate-200 bg-white p-1 shadow-lg">
                                  <button
                                    type="button"
                                    className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm text-slate-700 hover:bg-slate-100"
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      setHistoryMenuId(null);
                                      void onRename(item);
                                    }}
                                  >
                                    <Pencil size={13} />
                                    {t("rename")}
                                  </button>
                                  <button
                                    type="button"
                                    className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm text-slate-700 hover:bg-slate-100"
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      setHistoryMenuId(null);
                                      void onDelete(item);
                                    }}
                                  >
                                    <Trash2 size={13} />
                                    {t("delete")}
                                  </button>
                                </div>
                              )}
                            </div>
                          </div>
                        </div>
                      </li>
                    ))}
                  </ul>
                </ScrollArea>
              </Card>

              <Card className="flex min-h-0 flex-col p-3">
                <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
                  <div className="text-sm text-slate-500">
                    {selected ? t("currentRecording", { time: formatCurrentRecordingTime(selected.createdAtMs) }) : t("noSelectedRecording")}
                  </div>
                  <div className="inline-flex items-center gap-2">
                    <Button
                      variant="outline"
                      className="h-9 w-9 justify-center p-0"
                      onClick={onRetranscribeSelected}
                      disabled={!selected || isBusy || initStatus.running || !modelReady}
                      title={t("retranscribe")}
                    >
                      {isBusy ? <Loader2 className="animate-spin" size={15} /> : <RefreshCcw size={15} />}
                    </Button>
                    <Button
                      variant="outline"
                      className="h-9 w-9 justify-center p-0"
                      onClick={onCopyTranscript}
                      disabled={!transcript.trim()}
                      title={t("copyResult")}
                    >
                      {copied ? <Check size={15} /> : <Copy size={15} />}
                    </Button>
                  </div>
                </div>

                <Textarea value={transcript} onChange={(event) => setTranscript(event.target.value)} placeholder={t("transcriptPlaceholder")} />
              </Card>
            </div>
          )}

          {page === "cloud" && (
            <div className="grid h-full min-h-0 gap-4 md:grid-rows-[auto_1fr]">
              <header className="flex items-center justify-between">
                <h2 className="text-3xl font-semibold tracking-tight">{t("settingsSectionCloud")}</h2>
              </header>

              <Card className="p-4">
                <p className="text-sm text-slate-600">{t("settingsCloudDesc")}</p>
                <p className="mt-2 text-xs text-slate-500">{t("settingsCloudGuide")}</p>
                <div className="mt-3 space-y-3">
                  <label className="flex items-center gap-2 text-sm text-slate-700">
                    <input
                      type="checkbox"
                      checked={cloudSettings.pipeline.enabled}
                      onChange={(event) => updateCloudPipeline("enabled", event.target.checked)}
                      className="h-4 w-4 rounded border-slate-300 text-sky-600 focus:ring-sky-400"
                    />
                    <span>{t("settingsCloudEnabled")}</span>
                  </label>

                  {cloudSettings.providers.length === 0 && (
                    <div className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-900">
                      <div>{t("settingsCloudNoProviderHint")}</div>
                      <button
                        type="button"
                        className="mt-2 rounded border border-amber-300 bg-white px-2 py-1 font-medium text-amber-900"
                        onClick={() => {
                          setSettingsSection("providers");
                          setSettingsOpen(true);
                        }}
                      >
                        {t("settingsSectionProviders")}
                      </button>
                    </div>
                  )}

                  <div className="rounded-xl border border-slate-200 bg-white/70 p-3">
                    <div className="text-sm font-semibold text-slate-900">{t("settingsCloudOptimizeSection")}</div>
                    <p className="mt-1 text-xs text-slate-500">{t("settingsCloudOptimizeDesc")}</p>
                    <div className="mt-3">
                      <label className="mb-1 block text-sm text-slate-700">{t("settingsCloudOptimizeProvider")}</label>
                      <select
                        value={cloudSettings.pipeline.optimizeProviderId}
                        onChange={(event) => updateCloudPipeline("optimizeProviderId", event.target.value)}
                        className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                      >
                        <option value="">-</option>
                        {cloudSettings.providers.filter((provider) => provider.enabled).map((provider) => (
                          <option key={provider.id} value={provider.id}>
                            {buildProviderLabel(provider)}
                          </option>
                        ))}
                      </select>
                    </div>
                    <div className="mt-3">
                      <label className="mb-1 block text-sm text-slate-700">{t("settingsCloudOptimizePrompt")}</label>
                      <textarea
                        value={cloudSettings.pipeline.optimizePrompt}
                        onChange={(event) => updateCloudPipeline("optimizePrompt", event.target.value)}
                        className="min-h-[96px] w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-sm outline-none ring-sky-300 focus:ring"
                      />
                    </div>
                  </div>

                  <div className="rounded-xl border border-slate-200 bg-white/70 p-3">
                    <div className="text-sm font-semibold text-slate-900">{t("settingsCloudTranslateSection")}</div>
                    <p className="mt-1 text-xs text-slate-500">{t("settingsCloudTranslateDesc")}</p>
                    <div>
                      <label className="mb-1 mt-3 block text-sm text-slate-700">{t("settingsCloudTranslateProvider")}</label>
                      <select
                        value={cloudSettings.pipeline.translateProviderId}
                        onChange={(event) => updateCloudPipeline("translateProviderId", event.target.value)}
                        className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                      >
                        <option value="">-</option>
                        {cloudSettings.providers.filter((provider) => provider.enabled).map((provider) => (
                          <option key={provider.id} value={provider.id}>
                            {buildProviderLabel(provider)}
                          </option>
                        ))}
                      </select>
                    </div>
                    <div className="mt-3 grid gap-3 md:grid-cols-3">
                      <div>
                        <label className="mb-1 block text-sm text-slate-700">{t("settingsCloudTargetLanguage")}</label>
                        <select
                          value={cloudSettings.pipeline.targetLanguage || "en"}
                          onChange={(event) => updateCloudPipeline("targetLanguage", event.target.value)}
                          className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                        >
                          {TARGET_LANGUAGE_OPTIONS.map((item) => (
                            <option key={item.value} value={item.value}>
                              {uiLang === "zh" ? item.labelZh : item.labelEn}
                            </option>
                          ))}
                        </select>
                      </div>
                      <div>
                        <label className="mb-1 block text-sm text-slate-700">{t("settingsCloudTimeoutMs")}</label>
                        <input
                          type="number"
                          value={cloudSettings.pipeline.timeoutMs}
                          onChange={(event) => updateCloudPipeline("timeoutMs", Number(event.target.value) || 10000)}
                          className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                        />
                      </div>
                      <div>
                        <label className="mb-1 block text-sm text-slate-700">{t("settingsCloudRetries")}</label>
                        <input
                          type="number"
                          value={cloudSettings.pipeline.maxRetries}
                          onChange={(event) => updateCloudPipeline("maxRetries", Number(event.target.value) || 0)}
                          className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                        />
                      </div>
                    </div>
                    <div>
                      <label className="mb-1 mt-3 block text-sm text-slate-700">{t("settingsCloudTranslatePrompt")}</label>
                      <textarea
                        value={cloudSettings.pipeline.translatePrompt}
                        onChange={(event) => updateCloudPipeline("translatePrompt", event.target.value)}
                        className="min-h-[96px] w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-sm outline-none ring-sky-300 focus:ring"
                      />
                    </div>
                  </div>

                  <div className="flex gap-2">
                    <Button variant="outline" onClick={onSaveCloudSettings} disabled={savingCloudSettings}>
                      {savingCloudSettings ? <Loader2 size={14} className="animate-spin" /> : null}
                      {t("settingsCloudSave")}
                    </Button>
                  </div>
                </div>
              </Card>
            </div>
          )}

          {page === "dictionary" && (
            <div className="grid h-full min-h-0 gap-4 md:grid-rows-[auto_auto_1fr]">
              <header className="flex items-center justify-between">
                <h2 className="text-3xl font-semibold tracking-tight">{t("dictionaryTitle")}</h2>
                <Badge className="bg-slate-100 text-slate-700 border-slate-200">{t("dictionaryWords", { count: dictionaryWords.length })}</Badge>
              </header>

              <Card className="p-3">
                <div className="flex flex-wrap gap-2">
                  <input
                    value={newWord}
                    onChange={(event) => setNewWord(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") addDictionaryWord();
                    }}
                    placeholder={t("dictionaryPlaceholder")}
                    className="h-10 min-w-[220px] flex-1 rounded-md border border-slate-300 px-3 text-sm outline-none ring-sky-300 transition focus:ring"
                  />
                  <Button onClick={addDictionaryWord}>
                    <Plus size={15} />
                    {t("dictionaryAdd")}
                  </Button>
                </div>
              </Card>

              <Card className="min-h-0 overflow-hidden">
                <ScrollArea className="h-full" viewportClassName="h-full p-3">
                  <div className="flex flex-wrap gap-2">
                    {dictionaryWords.map((word) => (
                      <div key={word} className="inline-flex items-center gap-2 rounded-full border border-slate-300 bg-white px-3 py-1.5 text-sm">
                        <span>{word}</span>
                        <button
                          type="button"
                          className="rounded p-0.5 text-slate-500 hover:bg-slate-100 hover:text-slate-700"
                          onClick={() => removeDictionaryWord(word)}
                          title={t("dictionaryDelete")}
                        >
                          <X size={13} />
                        </button>
                      </div>
                    ))}
                    {dictionaryWords.length === 0 && (
                      <div className="w-full rounded-xl border border-dashed border-slate-300 px-3 py-10 text-center text-sm text-slate-500">{t("dictionaryEmpty")}</div>
                    )}
                  </div>
                </ScrollArea>
              </Card>
            </div>
          )}
        </section>
      </div>

      {settingsOpen && (
        <div className="tm-settings-mask fixed inset-0 z-40 flex items-center justify-center p-4">
          <div className="tm-settings-panel grid h-[min(680px,90vh)] w-[min(980px,95vw)] grid-cols-[220px_1fr] overflow-hidden rounded-2xl bg-white">
            <aside className="tm-settings-sidebar border-r border-slate-200 p-3">
              <div className="mb-3 px-2 text-xs font-semibold uppercase tracking-wider text-slate-500">{t("settingsTitle")}</div>
              <div className="space-y-1">
                {settingsSections.map((item) => (
                  <button
                    key={item.key}
                    type="button"
                    onClick={() => setSettingsSection(item.key)}
                    className={cn(
                      "w-full rounded-lg px-3 py-2 text-left text-sm font-medium transition",
                      settingsSection === item.key
                        ? "bg-white text-slate-900 shadow-sm"
                        : "text-slate-600 hover:bg-white/80 hover:text-slate-900"
                    )}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            </aside>

            <section className="tm-settings-content flex min-h-0 flex-col overflow-hidden">
              <div className="flex items-center justify-between border-b border-slate-200 px-6 py-4">
                <h2 className="text-lg font-semibold tracking-tight text-slate-800">{currentSettingsLabel}</h2>
                <Button variant="outline" className="h-9 w-9 justify-center p-0" onClick={() => setSettingsOpen(false)}>
                  <X size={16} />
                </Button>
              </div>

              <div className="min-h-0 flex-1 overflow-y-auto p-6">
                <div className="space-y-4 pb-2">
                  {settingsSection === "language" && (
                  <Card className="tm-settings-card p-4">
                    <p className="text-sm text-slate-600">{t("settingsLanguageDesc")}</p>
                    <p className="mt-2 text-xs text-slate-500">UI language updates immediately and also syncs to backend prompt/status messages.</p>
                    <div className="mt-4 max-w-xs">
                      <label className="mb-1 block text-sm text-slate-700">{t("languageModeLabel")}</label>
                      <select
                        value={langMode}
                        onChange={(event) => setLangMode(event.target.value as LangMode)}
                        className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                      >
                        <option value="auto">{t("langAuto")}</option>
                        <option value="zh-CN">{t("langZh")}</option>
                        <option value="en-US">{t("langEn")}</option>
                      </select>
                    </div>
                  </Card>
                  )}

                  {settingsSection === "hotkey" && (
                  <Card className="tm-settings-card p-4">
                    <p className="text-sm text-slate-600">{t("settingsHotkeyDesc")}</p>
                    <p className="mt-2 text-xs text-slate-500">{t("settingsHotkeyBuiltInHint")}</p>
                    <p className="mt-2 text-xs text-slate-500">Recommended: keep dictation and translation shortcuts distinct to avoid accidental mode switching.</p>
                    <div className="mt-3 space-y-3">
                      <div>
                        <label className="mb-1 block text-sm text-slate-700">{t("settingsHotkeyDictation")}</label>
                        <div className="flex gap-2">
                          <input
                            value={normalizeHotkeyLabel(hotkeyDictation)}
                            readOnly
                            placeholder={t("settingsHotkeyTogglePlaceholder")}
                            className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none"
                          />
                          <Button
                            variant="outline"
                            type="button"
                            className="h-10 min-w-[96px] justify-center whitespace-nowrap"
                            onClick={() => setCaptureTarget("dictation")}
                            disabled={savingHotkeys}
                          >
                            {captureTarget === "dictation" ? t("settingsHotkeyRecording") : t("settingsHotkeyRecord")}
                          </Button>
                        </div>
                      </div>
                      <div>
                        <label className="mb-1 block text-sm text-slate-700">{t("settingsHotkeyTranslation")}</label>
                        <div className="flex gap-2">
                          <input
                            value={normalizeHotkeyLabel(hotkeyTranslation)}
                            readOnly
                            placeholder={t("settingsHotkeyTogglePlaceholder")}
                            className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none"
                          />
                          <Button
                            variant="outline"
                            type="button"
                            className="h-10 min-w-[96px] justify-center whitespace-nowrap"
                            onClick={() => setCaptureTarget("translation")}
                            disabled={savingHotkeys}
                          >
                            {captureTarget === "translation" ? t("settingsHotkeyRecording") : t("settingsHotkeyRecord")}
                          </Button>
                        </div>
                      </div>
                      <div>
                        <label className="mb-1 block text-sm text-slate-700">{t("settingsTriggerMode")}</label>
                        <select
                          value={triggerMode}
                          onChange={(event) => setTriggerMode(event.target.value as HotkeyTriggerMode)}
                          className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                        >
                          <option value="tap">{t("settingsTriggerModeTap")}</option>
                          <option value="long-press">{t("settingsTriggerModeLongPress")}</option>
                        </select>
                      </div>
                      <div>
                        <label className="mb-1 block text-sm text-slate-700">{t("settingsOverlayPosition")}</label>
                        <select
                          value={overlayPosition}
                          onChange={(event) => setOverlayPosition(event.target.value as OverlayPosition)}
                          className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                        >
                          <option value="bottom">{t("settingsOverlayPositionBottom")}</option>
                          <option value="top">{t("settingsOverlayPositionTop")}</option>
                        </select>
                      </div>
                      <div>
                        <label className="mb-1 block text-sm text-slate-700">{t("settingsOutputMode")}</label>
                        <select
                          value={outputMode}
                          onChange={(event) => setOutputMode(event.target.value as OutputMode)}
                          className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                        >
                          <option value="auto-paste">{t("settingsOutputModeAutoPaste")}</option>
                          <option value="paste-and-keep">{t("settingsOutputModePasteAndKeep")}</option>
                          <option value="copy-only">{t("settingsOutputModeCopyOnly")}</option>
                        </select>
                      </div>
                      <div>
                        <label className="mb-1 block text-sm text-slate-700">{t("settingsTranslationTarget")}</label>
                        <select
                          value={translationTargetLang}
                          onChange={(event) => setTranslationTargetLang(event.target.value as TranslationTargetLang)}
                          className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                        >
                          <option value="auto">{t("settingsTranslationTargetAuto")}</option>
                          <option value="en">{t("settingsTranslationTargetEn")}</option>
                          <option value="zh-CN">{t("settingsTranslationTargetZh")}</option>
                          <option value="ja">{t("settingsTranslationTargetJa")}</option>
                          <option value="ko">{t("settingsTranslationTargetKo")}</option>
                        </select>
                      </div>
                      {hasHotkeyConflicts && (
                        <div className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-900">
                          <div className="font-semibold">{t("settingsHotkeyConflictTitle")}</div>
                          {hasDuplicateHotkeys && <div>{t("settingsHotkeyConflictSame")}</div>}
                          {dictationSystemConflict && (
                            <div>{t("settingsHotkeyConflictWithSystem", { value: normalizeHotkeyLabel(dictationSystemConflict) })}</div>
                          )}
                          {translationSystemConflict && (
                            <div>{t("settingsHotkeyConflictWithSystem", { value: normalizeHotkeyLabel(translationSystemConflict) })}</div>
                          )}
                        </div>
                      )}
                      <label className="flex items-center gap-2 text-sm text-slate-700">
                        <input
                          type="checkbox"
                          checked={fnKeyEnabled}
                          disabled={savingHotkeys}
                          onChange={(event) => void onToggleFnKeyEnabled(event.target.checked)}
                          className="h-4 w-4 rounded border-slate-300 text-sky-600 focus:ring-sky-400"
                        />
                        <span>{t("settingsFnKeyToggle")}</span>
                      </label>
                      <p className="text-xs text-slate-500">{t("settingsHotkeyPressHint")}</p>
                      <div className="flex gap-2">
                        <Button variant="outline" onClick={onSaveHotkeys} disabled={savingHotkeys || hasHotkeyConflicts}>
                          {savingHotkeys ? <Loader2 size={14} className="animate-spin" /> : null}
                          {t("settingsHotkeySave")}
                        </Button>
                        <Button variant="outline" onClick={onResetHotkeys} disabled={savingHotkeys}>{t("settingsHotkeyReset")}</Button>
                      </div>
                    </div>
                  </Card>
                  )}

                  {settingsSection === "processing" && (
                  <Card className="tm-settings-card p-4">
                    <p className="text-sm text-slate-600">{t("settingsCloudDesc")}</p>
                    <p className="mt-2 text-xs text-slate-500">{t("settingsCloudGuide")}</p>
                    <div className="mt-3 space-y-3">
                      <label className="flex items-center gap-2 text-sm text-slate-700">
                        <input
                          type="checkbox"
                          checked={cloudSettings.pipeline.enabled}
                          onChange={(event) => updateCloudPipeline("enabled", event.target.checked)}
                          className="h-4 w-4 rounded border-slate-300 text-sky-600 focus:ring-sky-400"
                        />
                        <span>{t("settingsCloudEnabled")}</span>
                      </label>

                      {cloudSettings.providers.length === 0 && (
                        <div className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-900">
                          <div>{t("settingsCloudNoProviderHint")}</div>
                          <button
                            type="button"
                            className="mt-2 rounded border border-amber-300 bg-white px-2 py-1 font-medium text-amber-900"
                            onClick={() => setSettingsSection("providers")}
                          >
                            {t("settingsSectionProviders")}
                          </button>
                        </div>
                      )}

                      <div className="rounded-xl border border-slate-200 bg-white/70 p-3">
                        <div className="text-sm font-semibold text-slate-900">{t("settingsCloudOptimizeSection")}</div>
                        <p className="mt-1 text-xs text-slate-500">{t("settingsCloudOptimizeDesc")}</p>
                        <div className="mt-3">
                          <label className="mb-1 block text-sm text-slate-700">{t("settingsCloudOptimizeProvider")}</label>
                          <select
                            value={cloudSettings.pipeline.optimizeProviderId}
                            onChange={(event) => updateCloudPipeline("optimizeProviderId", event.target.value)}
                            className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                          >
                            <option value="">-</option>
                            {cloudSettings.providers.filter((provider) => provider.enabled).map((provider) => (
                              <option key={provider.id} value={provider.id}>
                                {buildProviderLabel(provider)}
                              </option>
                            ))}
                          </select>
                        </div>
                        <div className="mt-3">
                          <label className="mb-1 block text-sm text-slate-700">{t("settingsCloudOptimizePrompt")}</label>
                          <textarea
                            value={cloudSettings.pipeline.optimizePrompt}
                            onChange={(event) => updateCloudPipeline("optimizePrompt", event.target.value)}
                            className="min-h-[96px] w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-sm outline-none ring-sky-300 focus:ring"
                          />
                        </div>
                      </div>

                      <div className="rounded-xl border border-slate-200 bg-white/70 p-3">
                        <div className="text-sm font-semibold text-slate-900">{t("settingsCloudTranslateSection")}</div>
                        <p className="mt-1 text-xs text-slate-500">{t("settingsCloudTranslateDesc")}</p>
                        <div>
                          <label className="mb-1 mt-3 block text-sm text-slate-700">{t("settingsCloudTranslateProvider")}</label>
                          <select
                            value={cloudSettings.pipeline.translateProviderId}
                            onChange={(event) => updateCloudPipeline("translateProviderId", event.target.value)}
                            className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                          >
                            <option value="">-</option>
                            {cloudSettings.providers.filter((provider) => provider.enabled).map((provider) => (
                              <option key={provider.id} value={provider.id}>
                                {buildProviderLabel(provider)}
                              </option>
                            ))}
                          </select>
                        </div>
                        <div className="mt-3 grid gap-3 md:grid-cols-3">
                          <div>
                            <label className="mb-1 block text-sm text-slate-700">{t("settingsCloudTargetLanguage")}</label>
                            <select
                              value={cloudSettings.pipeline.targetLanguage || "en"}
                              onChange={(event) => updateCloudPipeline("targetLanguage", event.target.value)}
                              className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                            >
                              {TARGET_LANGUAGE_OPTIONS.map((item) => (
                                <option key={item.value} value={item.value}>
                                  {uiLang === "zh" ? item.labelZh : item.labelEn}
                                </option>
                              ))}
                            </select>
                          </div>
                          <div>
                            <label className="mb-1 block text-sm text-slate-700">{t("settingsCloudTimeoutMs")}</label>
                            <input
                              type="number"
                              value={cloudSettings.pipeline.timeoutMs}
                              onChange={(event) => updateCloudPipeline("timeoutMs", Number(event.target.value) || 10000)}
                              className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                            />
                          </div>
                          <div>
                            <label className="mb-1 block text-sm text-slate-700">{t("settingsCloudRetries")}</label>
                            <input
                              type="number"
                              value={cloudSettings.pipeline.maxRetries}
                              onChange={(event) => updateCloudPipeline("maxRetries", Number(event.target.value) || 0)}
                              className="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                            />
                          </div>
                        </div>
                        <div>
                          <label className="mb-1 mt-3 block text-sm text-slate-700">{t("settingsCloudTranslatePrompt")}</label>
                          <textarea
                            value={cloudSettings.pipeline.translatePrompt}
                            onChange={(event) => updateCloudPipeline("translatePrompt", event.target.value)}
                            className="min-h-[96px] w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-sm outline-none ring-sky-300 focus:ring"
                          />
                        </div>
                      </div>

                      <div className="flex gap-2">
                        <Button variant="outline" onClick={onSaveCloudSettings} disabled={savingCloudSettings}>
                          {savingCloudSettings ? <Loader2 size={14} className="animate-spin" /> : null}
                          {t("settingsCloudSave")}
                        </Button>
                      </div>
                    </div>
                  </Card>
                  )}

                  {settingsSection === "providers" && (
                  <Card className="tm-settings-card p-4">
                    <p className="text-sm text-slate-600">{t("settingsProvidersDesc")}</p>
                    <p className="mt-2 text-xs text-slate-500">{t("settingsProvidersGuide")}</p>
                    <div className="mt-3 flex items-center justify-between">
                      <div className="text-sm font-medium text-slate-700">{t("settingsCloudProviders")}</div>
                      <Button variant="outline" onClick={addCloudProvider}>
                        <Plus size={14} />
                        {t("settingsCloudAddProvider")}
                      </Button>
                    </div>
                    <div className="mt-3 space-y-3">
                      {cloudSettings.providers.map((provider, index) => (
                        <div key={`${provider.id}-${index}`} className="rounded-lg border border-slate-200 p-3">
                          <div className="grid gap-3 md:grid-cols-2">
                            <div>
                              <label className="mb-1 block text-xs text-slate-600">{t("settingsCloudProviderVendor")}</label>
                              <select
                                value={provider.vendor}
                                onChange={(event) => updateCloudProvider(index, { vendor: event.target.value as CloudVendor })}
                                className="h-9 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                              >
                                {CLOUD_VENDOR_OPTIONS.map((item) => (
                                  <option key={item.value} value={item.value}>
                                    {item.label}
                                  </option>
                                ))}
                              </select>
                            </div>
                            <div>
                              <label className="mb-1 block text-xs text-slate-600">{t("settingsCloudProviderModel")}</label>
                              <input
                                value={provider.model}
                                onChange={(event) => updateCloudProvider(index, { model: event.target.value })}
                                className="h-9 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                              />
                            </div>
                            <div>
                              <label className="mb-1 block text-xs text-slate-600">{t("settingsCloudProviderApiKey")}</label>
                              <input
                                type="password"
                                value={provider.apiKey}
                                onChange={(event) => updateCloudProvider(index, { apiKey: event.target.value })}
                                className="h-9 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                              />
                            </div>
                            <div>
                              <label className="mb-1 block text-xs text-slate-600">{t("settingsCloudProviderBaseUrl")}</label>
                              <input
                                value={provider.baseUrl ?? ""}
                                onChange={(event) => updateCloudProvider(index, { baseUrl: event.target.value })}
                                className="h-9 w-full rounded-md border border-slate-300 bg-white px-3 text-sm outline-none ring-sky-300 focus:ring"
                              />
                            </div>
                          </div>
                          <div className="mt-3 flex flex-wrap items-center gap-2">
                            <label className="inline-flex items-center gap-2 text-sm text-slate-700">
                              <input
                                type="checkbox"
                                checked={provider.enabled}
                                onChange={(event) => updateCloudProvider(index, { enabled: event.target.checked })}
                                className="h-4 w-4 rounded border-slate-300 text-sky-600 focus:ring-sky-400"
                              />
                              {t("settingsCloudProviderEnabled")}
                            </label>
                            <Button variant="outline" onClick={() => void onTestCloudProvider(provider.id)}>
                              {t("settingsCloudTest")}
                            </Button>
                            <Button variant="outline" onClick={() => removeCloudProvider(index)}>
                              {t("settingsCloudRemoveProvider")}
                            </Button>
                          </div>
                        </div>
                      ))}
                      <div className="flex gap-2">
                        <Button variant="outline" onClick={onSaveProvidersOnly} disabled={savingCloudSettings}>
                          {savingCloudSettings ? <Loader2 size={14} className="animate-spin" /> : null}
                          {t("settingsProvidersSave")}
                        </Button>
                      </div>
                    </div>
                  </Card>
                  )}

                  {settingsSection === "temp" && (
                  <Card className="tm-settings-card p-4">
                    <p className="text-sm text-slate-600">{t("settingsTempDirDesc")}</p>
                    <p className="mt-2 text-xs text-slate-500">Useful for diagnosing recordings, temporary conversion files, and release artifacts.</p>
                    <div className="mt-4">
                      <Button variant="outline" onClick={onOpenTempDir}>
                        <FolderOpen size={16} />
                        {t("settingsOpenTempDir")}
                      </Button>
                    </div>
                  </Card>
                  )}

                  {settingsSection === "about" && (
                  <Card className="tm-settings-card p-4">
                    <p className="text-sm text-slate-600">Project website, open-source repository, and license information.</p>
                    <div className="mt-4 space-y-2 text-sm text-slate-700">
                      <div className="font-medium text-slate-900">{t("settingsProjectInfo")}</div>
                      <div className="flex items-center justify-between gap-3">
                        <span className="text-slate-500">{t("settingsProjectWebsite")}</span>
                        <a
                          href="https://typemore.app"
                          target="_blank"
                          rel="noreferrer"
                          className="text-sky-700 hover:text-sky-800 hover:underline"
                        >
                          typemore.app
                        </a>
                      </div>
                      <div className="flex items-center justify-between gap-3">
                        <span className="text-slate-500">{t("settingsProjectGithub")}</span>
                        <a
                          href="https://github.com/everettjf/typemore"
                          target="_blank"
                          rel="noreferrer"
                          className="text-sky-700 hover:text-sky-800 hover:underline"
                        >
                          github.com/everettjf/typemore
                        </a>
                      </div>
                      <div className="flex items-center justify-between gap-3">
                        <span className="text-slate-500">{t("settingsProjectMoreApps")}</span>
                        <a
                          href="https://xnu.app"
                          target="_blank"
                          rel="noreferrer"
                          className="text-sky-700 hover:text-sky-800 hover:underline"
                        >
                          xnu.app
                        </a>
                      </div>
                      <div className="flex items-center justify-between gap-3">
                        <span className="text-slate-500">{t("settingsProjectLicense")}</span>
                        <span>Apache-2.0</span>
                      </div>
                    </div>
                  </Card>
                  )}
                </div>
              </div>
            </section>
          </div>
        </div>
      )}

      {fallbackText && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/35 p-4">
          <Card className="w-[min(560px,92vw)] p-4">
            <div className="text-lg font-semibold">{t("fallbackTitle")}</div>
            <p className="mt-1 text-sm text-slate-600">{t("fallbackDesc")}</p>
            <Textarea value={fallbackText} readOnly className="mt-3 min-h-[140px]" />
            <div className="mt-3 flex gap-2">
              <Button
                variant="outline"
                onClick={async () => {
                  await navigator.clipboard.writeText(fallbackText);
                }}
              >
                {t("fallbackCopy")}
              </Button>
              <Button variant="outline" onClick={() => setFallbackText(null)}>
                {t("fallbackClose")}
              </Button>
            </div>
          </Card>
        </div>
      )}

    </main>
  );
}

function App() {
  if (window.location.hash.includes("/overlay")) {
    return <OverlayWindowApp />;
  }
  return <MainApp />;
}

export default App;
