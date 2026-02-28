import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  BookText,
  Check,
  Copy,
  FolderOpen,
  History,
  Home,
  Loader2,
  Mic,
  Plus,
  RefreshCcw,
  Settings,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import {
  CategoryScale,
  Chart as ChartJS,
  Filler,
  LineElement,
  LinearScale,
  PointElement,
  Tooltip,
} from "chart.js";
import { blobToMono16kWav } from "./components/app/audio";
import { RecordingDialog } from "./components/app/recording-dialog";
import type { ModelInitStatus, RecordingItem, SaveAndTranscribeResult } from "./components/app/types";
import { badgeClass, defaultInitStatus, formatCurrentRecordingTime, formatListTime } from "./components/app/utils";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Card } from "./components/ui/card";
import { Progress } from "./components/ui/progress";
import { ScrollArea } from "./components/ui/scroll-area";
import { Separator } from "./components/ui/separator";
import { Textarea } from "./components/ui/textarea";
import { cn } from "./lib/utils";

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Filler, Tooltip);

type Page = "home" | "history" | "dictionary";
type LangMode = "auto" | "zh-CN" | "en-US";
type UiLang = "zh" | "en";

const DICTIONARY_STORAGE_KEY = "typemore.dictionary.words";
const LANG_MODE_STORAGE_KEY = "typemore.lang.mode";

const I18N = {
  zh: {
    appTagline: "离线语音转文字",
    navHome: "首页",
    navHistory: "历史",
    navDictionary: "词库",
    navSettings: "设置",
    titleHome: "自然说话，清晰落字。",
    subHome: "聚焦核心流程：初始化模型、录音、转写。",
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
    settingsTempDirTitle: "临时目录",
    settingsTempDirDesc: "打开应用的临时目录，用于查看当前运行过程中的临时文件。",
    settingsOpenTempDir: "打开临时目录",
    settingsLanguageTitle: "语言",
    settingsLanguageDesc: "支持自动跟随系统语言，也可以手动切换。",
    languageModeLabel: "界面语言",
    langAuto: "自动（跟随系统）",
    langZh: "中文",
    langEn: "English",
    recordingPrefix: "录音",
    transcriptInitFailed: "初始化失败: {error}",
    transcriptListenFailed: "监听模型进度失败: {error}",
    transcriptModelReady: "模型已就绪，可以开始录音识别。",
    transcriptModelInitFailed: "模型初始化失败: {error}",
    transcriptRecordingFailed: "录音识别失败: {error}",
    transcriptNeedInit: "请先初始化模型，再开始录音。",
    transcriptNeedSelect: "请先在左侧选择一个录音。",
    transcriptRetryFailed: "识别失败: {error}",
    transcriptCopyFailed: "[复制失败] {error}",
    transcriptRenameFailed: "重命名失败: {error}",
    transcriptDeleteFailed: "删除失败: {error}",
    transcriptOpenTempDirOk: "已打开临时目录: {dir}",
    transcriptOpenTempDirFailed: "打开临时目录失败: {error}",
  },
  en: {
    appTagline: "Offline speech to text",
    navHome: "Home",
    navHistory: "History",
    navDictionary: "Dictionary",
    navSettings: "Settings",
    titleHome: "Speak naturally, write clearly.",
    subHome: "Focus on core flow: model init, recording, and transcription.",
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
    settingsTempDirTitle: "Temporary Directory",
    settingsTempDirDesc: "Open app temporary directory to inspect runtime temp files.",
    settingsOpenTempDir: "Open Temporary Directory",
    settingsLanguageTitle: "Language",
    settingsLanguageDesc: "Auto follow system language, or switch manually.",
    languageModeLabel: "Interface language",
    langAuto: "Auto (System)",
    langZh: "Chinese",
    langEn: "English",
    recordingPrefix: "recording",
    transcriptInitFailed: "Initialization failed: {error}",
    transcriptListenFailed: "Failed to listen model progress: {error}",
    transcriptModelReady: "Model is ready. You can start recording.",
    transcriptModelInitFailed: "Model initialization failed: {error}",
    transcriptRecordingFailed: "Recording transcription failed: {error}",
    transcriptNeedInit: "Please initialize the model before recording.",
    transcriptNeedSelect: "Please select a recording from the left list.",
    transcriptRetryFailed: "Transcription failed: {error}",
    transcriptCopyFailed: "[Copy failed] {error}",
    transcriptRenameFailed: "Rename failed: {error}",
    transcriptDeleteFailed: "Delete failed: {error}",
    transcriptOpenTempDirOk: "Opened temporary directory: {dir}",
    transcriptOpenTempDirFailed: "Failed to open temporary directory: {error}",
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

function App() {
  const [page, setPage] = useState<Page>("home");
  const [settingsOpen, setSettingsOpen] = useState(false);

  const [recordings, setRecordings] = useState<RecordingItem[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [transcript, setTranscript] = useState("");
  const [initStatus, setInitStatus] = useState<ModelInitStatus>(defaultInitStatus());
  const [isRecording, setIsRecording] = useState(false);
  const [isBusy, setIsBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [inputLevel, setInputLevel] = useState(0);
  const [levelHistory, setLevelHistory] = useState<number[]>(Array(36).fill(0));
  const [recordingSeconds, setRecordingSeconds] = useState(0);

  const [dictionaryWords, setDictionaryWords] = useState<string[]>([]);
  const [newWord, setNewWord] = useState("");
  const [langMode, setLangMode] = useState<LangMode>(() => {
    const raw = window.localStorage.getItem(LANG_MODE_STORAGE_KEY);
    return raw === "zh-CN" || raw === "en-US" || raw === "auto" ? raw : "auto";
  });

  const recorderRef = useRef<MediaRecorder | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const chunksRef = useRef<BlobPart[]>([]);

  const levelAudioCtxRef = useRef<AudioContext | null>(null);
  const levelAnalyserRef = useRef<AnalyserNode | null>(null);
  const levelRafRef = useRef<number | null>(null);

  const selected = useMemo(
    () => recordings.find((item) => item.id === selectedId) ?? null,
    [recordings, selectedId]
  );

  const modelReady = initStatus.ready;

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

  const levelChartData = useMemo(
    () => ({
      labels: levelHistory.map((_, idx) => idx),
      datasets: [
        {
          data: levelHistory,
          borderColor: "rgb(34, 197, 94)",
          backgroundColor: "rgba(34, 197, 94, 0.2)",
          borderWidth: 2,
          pointRadius: 0,
          tension: 0.35,
          fill: true,
        },
      ],
    }),
    [levelHistory]
  );

  async function loadRecordings() {
    const items = await invoke<RecordingItem[]>("list_recordings");
    setRecordings(items);
    setSelectedId((prev) => prev ?? (items.length > 0 ? items[0].id : null));
  }

  async function loadInitStatus() {
    const status = await invoke<ModelInitStatus>("get_model_init_status");
    setInitStatus(status);
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

  useEffect(() => {
    Promise.all([loadRecordings(), loadInitStatus()]).catch((err) => {
      setTranscript(t("transcriptInitFailed", { error: String(err) }));
    });

    let unlisten: (() => void) | undefined;
    listen<ModelInitStatus>("model-init-progress", (event) => {
      setInitStatus(event.payload);
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((err) => {
        setTranscript(t("transcriptListenFailed", { error: String(err) }));
      });

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [t]);

  useEffect(() => {
    if (!isRecording) {
      setRecordingSeconds(0);
      return;
    }
    const timer = window.setInterval(() => {
      setRecordingSeconds((prev) => prev + 1);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [isRecording]);

  useEffect(() => {
    if (!selectedId) {
      return;
    }
    invoke<string | null>("get_recording_cached_transcript", { id: selectedId })
      .then((cachedText) => {
        if (cachedText && cachedText.length > 0) {
          setTranscript(cachedText);
        }
      })
      .catch(() => {});
  }, [selectedId]);

  function stopLevelMonitor() {
    if (levelRafRef.current != null) {
      cancelAnimationFrame(levelRafRef.current);
      levelRafRef.current = null;
    }
    if (levelAudioCtxRef.current) {
      void levelAudioCtxRef.current.close();
      levelAudioCtxRef.current = null;
    }
    levelAnalyserRef.current = null;
    setInputLevel(0);
    setLevelHistory(Array(36).fill(0));
  }

  function startLevelMonitor(stream: MediaStream) {
    const audioCtx = new AudioContext();
    const source = audioCtx.createMediaStreamSource(stream);
    const analyser = audioCtx.createAnalyser();
    analyser.fftSize = 1024;
    analyser.smoothingTimeConstant = 0.85;
    source.connect(analyser);

    levelAudioCtxRef.current = audioCtx;
    levelAnalyserRef.current = analyser;

    const data = new Uint8Array(analyser.fftSize);
    let frame = 0;

    const tick = () => {
      const current = levelAnalyserRef.current;
      if (!current) {
        return;
      }
      current.getByteTimeDomainData(data);

      let sum = 0;
      for (let i = 0; i < data.length; i += 1) {
        const centered = (data[i] - 128) / 128;
        sum += centered * centered;
      }
      const rms = Math.sqrt(sum / data.length);
      const normalized = Math.min(1, rms * 5.5);
      setInputLevel(normalized);

      frame += 1;
      if (frame % 2 === 0) {
        setLevelHistory((prev) => [...prev.slice(1), normalized]);
      }

      levelRafRef.current = requestAnimationFrame(tick);
    };

    levelRafRef.current = requestAnimationFrame(tick);
  }

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

  async function startRecording() {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    streamRef.current = stream;
    chunksRef.current = [];

    startLevelMonitor(stream);

    const recorder = new MediaRecorder(stream);
    recorderRef.current = recorder;

    recorder.ondataavailable = (event) => {
      if (event.data.size > 0) {
        chunksRef.current.push(event.data);
      }
    };

    recorder.onstop = async () => {
      stopLevelMonitor();
      try {
        setIsBusy(true);
        const blob = new Blob(chunksRef.current, { type: "audio/webm" });
        const wav = await blobToMono16kWav(blob);
        const wavData = Array.from(new Uint8Array(wav));
        const payload = {
          suggestedName: `${t("recordingPrefix")}_${new Date().toISOString().replace(/[:.]/g, "-")}`,
          wavData,
        };

        const result = await invoke<SaveAndTranscribeResult>("save_recording_and_transcribe", {
          payload,
        });

        setRecordings((prev) => [result.recording, ...prev]);
        setSelectedId(result.recording.id);
        setTranscript(result.text || "(empty)");
        setPage("history");
      } catch (err) {
        setTranscript(t("transcriptRecordingFailed", { error: String(err) }));
      } finally {
        setIsBusy(false);
      }
    };

    recorder.start();
    setIsRecording(true);
  }

  function stopRecording() {
    if (recorderRef.current && recorderRef.current.state !== "inactive") {
      recorderRef.current.stop();
    }
    streamRef.current?.getTracks().forEach((track) => track.stop());
    streamRef.current = null;
    recorderRef.current = null;
    setIsRecording(false);
  }

  async function onRecordClick() {
    if (!modelReady) {
      setTranscript(t("transcriptNeedInit"));
      return;
    }
    if (isRecording) {
      stopRecording();
      return;
    }
    try {
      await startRecording();
    } catch (err) {
      setTranscript(`Mic access failed: ${String(err)}`);
      stopLevelMonitor();
    }
  }

  async function onRetranscribeSelected() {
    if (!selected) {
      setTranscript(t("transcriptNeedSelect"));
      return;
    }
    setIsBusy(true);
    try {
      const text = await invoke<string>("transcribe_recording", {
        id: selected.id,
        force: true,
      });
      setTranscript(text || "(empty)");
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
      const renamed = await invoke<RecordingItem>("rename_recording", {
        id: recording.id,
        newName: name,
      });
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

  async function onOpenTempDir() {
    try {
      const dir = await invoke<string>("open_temp_dir");
      setTranscript(t("transcriptOpenTempDirOk", { dir }));
    } catch (err) {
      setTranscript(t("transcriptOpenTempDirFailed", { error: String(err) }));
    }
  }

  function addDictionaryWord() {
    const word = newWord.trim();
    if (!word) {
      return;
    }
    const exists = dictionaryWords.some((item) => item.toLowerCase() === word.toLowerCase());
    if (exists) {
      setNewWord("");
      return;
    }
    setDictionaryWords((prev) => [word, ...prev]);
    setNewWord("");
  }

  function removeDictionaryWord(word: string) {
    setDictionaryWords((prev) => prev.filter((item) => item !== word));
  }

  const navItems: Array<{ key: Page; label: string; icon: typeof Home }> = [
    { key: "home", label: t("navHome"), icon: Home },
    { key: "history", label: t("navHistory"), icon: History },
    { key: "dictionary", label: t("navDictionary"), icon: BookText },
  ];

  return (
    <main className="min-h-screen bg-[radial-gradient(circle_at_top_left,_#edf4ff,_#f8fafc_50%,_#eef7ff)] p-3 text-slate-900 md:p-5">
      <div className="mx-auto grid h-[calc(100vh-1.5rem)] max-w-[1540px] grid-cols-1 gap-3 rounded-3xl border border-white/70 bg-white/60 p-3 shadow-2xl shadow-slate-200/70 backdrop-blur md:h-[calc(100vh-2.5rem)] md:grid-cols-[230px_1fr] md:p-4">
        <aside className="flex min-h-0 flex-col rounded-2xl border border-slate-200/80 bg-white/95 p-3">
          <div className="px-2 pb-3 pt-1">
            <div className="text-2xl font-bold tracking-tight">TypeMore</div>
            <div className="text-xs text-slate-500">{t("appTagline")}</div>
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
              onClick={() => setSettingsOpen(true)}
            >
              <Settings size={16} />
              {t("navSettings")}
            </button>
          </div>
        </aside>

        <section className="min-h-0 overflow-hidden rounded-2xl border border-slate-200/80 bg-white/95 p-4 md:p-5">
          {page === "home" && (
            <div className="grid h-full min-h-0 gap-4 md:grid-rows-[auto_auto_1fr]">
              <header className="flex flex-wrap items-center justify-between gap-3">
                <div>
                  <h1 className="text-3xl font-semibold tracking-tight">{t("titleHome")}</h1>
                  <p className="mt-1 text-sm text-slate-500">{t("subHome")}</p>
                </div>
                <Badge className={cn(badgeClass(modelReady))}>
                  {modelReady ? t("modelReady") : initStatus.running ? t("modelInitializing") : t("modelNotReady")}
                </Badge>
              </header>

              <Card className="p-4">
                <div className="mb-4 flex flex-wrap items-center gap-2">
                  <Button
                    className={cn(
                      "h-11 rounded-xl px-4 inline-flex items-center gap-2 shadow-sm",
                      isRecording
                        ? "bg-red-600 hover:bg-red-700 border-red-600 hover:border-red-700"
                        : "bg-slate-900 hover:bg-slate-800 border-slate-900 hover:border-slate-800"
                    )}
                    onClick={onRecordClick}
                    disabled={isBusy || initStatus.running}
                  >
                    <Mic size={16} />
                    <span
                      className={cn(
                        "inline-block h-2 w-2 rounded-full bg-white/90",
                        isRecording ? "animate-pulse" : "opacity-70"
                      )}
                    />
                    {isRecording ? t("stopRecording") : t("startRecording")}
                  </Button>
                  <Button
                    variant="outline"
                    className="inline-flex items-center gap-2"
                    onClick={onInitModel}
                    disabled={isBusy || initStatus.running}
                  >
                    {initStatus.running ? <Loader2 className="animate-spin" size={15} /> : <Sparkles size={15} />}
                    {initStatus.running ? t("initModelRunning") : modelReady ? t("initModelReady") : t("initModelStart")}
                  </Button>
                  <Button variant="outline" onClick={() => setPage("history")}>
                    {t("viewHistory")}
                  </Button>
                </div>

                <div className="space-y-2">
                  <div className="text-sm text-slate-700 tabular-nums whitespace-nowrap overflow-hidden text-ellipsis">
                    {initStatus.message}
                  </div>
                  <Progress value={Math.min(100, Math.max(0, initStatus.progress))} />
                  {initStatus.error && <div className="text-xs text-red-600">{initStatus.error}</div>}
                </div>
              </Card>

              <Card className="min-h-0 overflow-hidden">
                <div className="flex items-center justify-between border-b border-slate-200 px-4 py-3">
                  <div className="text-sm font-semibold text-slate-700">{t("recentRecordings")}</div>
                  <div className="text-xs text-slate-500">{t("countItems", { count: recordings.length })}</div>
                </div>
                <ScrollArea className="h-full" viewportClassName="h-full p-3">
                  <div className="space-y-2">
                    {recordings.slice(0, 8).map((item) => (
                      <button
                        key={item.id}
                        type="button"
                        className="block w-full rounded-xl border border-slate-200 bg-white px-3 py-2 text-left hover:bg-slate-50"
                        onClick={() => {
                          setSelectedId(item.id);
                          setPage("history");
                        }}
                      >
                        <div className="text-sm font-medium text-slate-800">{item.name}</div>
                        <div className="mt-1 text-xs text-slate-500">{formatListTime(item.createdAtMs)}</div>
                      </button>
                    ))}
                    {recordings.length === 0 && (
                      <div className="rounded-xl border border-dashed border-slate-300 px-3 py-6 text-center text-sm text-slate-500">
                        {t("noRecordings")}
                      </div>
                    )}
                  </div>
                </ScrollArea>
              </Card>
            </div>
          )}

          {page === "history" && (
            <div className="grid h-full min-h-0 gap-4 md:grid-cols-[340px_1fr]">
              <Card className="flex min-h-0 flex-col overflow-hidden">
                <div className="flex items-center justify-between border-b border-slate-200 px-4 py-3">
                  <div className="text-lg font-semibold">{t("historyTitle")}</div>
                  <div className="text-xs text-slate-500">{t("countItems", { count: recordings.length })}</div>
                </div>

                <ScrollArea className="min-h-0 flex-1" viewportClassName="h-full w-full p-3">
                  <ul className="space-y-2">
                    {recordings.map((item) => (
                      <li key={item.id}>
                        <div
                          className={cn(
                            "rounded-xl border px-3 py-2",
                            selectedId === item.id
                              ? "border-sky-300 bg-sky-50"
                              : "border-slate-200 bg-white hover:bg-slate-50"
                          )}
                        >
                          <button
                            type="button"
                            className="w-full text-left"
                            onClick={() => setSelectedId(item.id)}
                            title={item.filePath}
                          >
                            <div className="text-sm font-medium text-slate-800">{item.name}</div>
                            <div className="mt-1 text-xs text-slate-500">{formatListTime(item.createdAtMs)}</div>
                          </button>
                          <div className="mt-2 flex items-center gap-2">
                            <Button variant="outline" className="h-7 px-2 text-xs" onClick={() => onRename(item)}>
                              {t("rename")}
                            </Button>
                            <Button variant="outline" className="h-7 px-2 text-xs" onClick={() => onDelete(item)}>
                              <Trash2 size={13} />
                              {t("delete")}
                            </Button>
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
                    {selected
                      ? t("currentRecording", { time: formatCurrentRecordingTime(selected.createdAtMs) })
                      : t("noSelectedRecording")}
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

                <Textarea
                  value={transcript}
                  onChange={(event) => setTranscript(event.target.value)}
                  placeholder={t("transcriptPlaceholder")}
                />
              </Card>
            </div>
          )}

          {page === "dictionary" && (
            <div className="grid h-full min-h-0 gap-4 md:grid-rows-[auto_auto_1fr]">
              <header className="flex items-center justify-between">
                <h2 className="text-3xl font-semibold tracking-tight">{t("dictionaryTitle")}</h2>
                <Badge className="bg-slate-100 text-slate-700 border-slate-200">
                  {t("dictionaryWords", { count: dictionaryWords.length })}
                </Badge>
              </header>

              <Card className="p-3">
                <div className="flex flex-wrap gap-2">
                  <input
                    value={newWord}
                    onChange={(event) => setNewWord(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        addDictionaryWord();
                      }
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
                      <div
                        key={word}
                        className="inline-flex items-center gap-2 rounded-full border border-slate-300 bg-white px-3 py-1.5 text-sm"
                      >
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
                      <div className="w-full rounded-xl border border-dashed border-slate-300 px-3 py-10 text-center text-sm text-slate-500">
                        {t("dictionaryEmpty")}
                      </div>
                    )}
                  </div>
                </ScrollArea>
              </Card>
            </div>
          )}
        </section>
      </div>

      {settingsOpen && (
        <div className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/35 p-4 backdrop-blur-[2px]">
          <div className="grid h-[min(680px,90vh)] w-[min(980px,95vw)] grid-cols-[220px_1fr] overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-2xl">
            <aside className="border-r border-slate-200 bg-slate-50/80 p-3">
              <div className="mb-3 px-2 text-xs font-semibold uppercase tracking-wider text-slate-500">{t("settingsTitle")}</div>
              <div className="rounded-lg bg-white px-3 py-2 text-sm font-medium text-slate-900 shadow-sm">
                {t("settingsSectionGeneral")}
              </div>
            </aside>

            <section className="flex min-h-0 flex-col">
              <div className="flex items-center justify-between border-b border-slate-200 px-6 py-4">
                <h2 className="text-3xl font-semibold tracking-tight">{t("settingsTitle")}</h2>
                <Button variant="outline" className="h-9 w-9 justify-center p-0" onClick={() => setSettingsOpen(false)}>
                  <X size={16} />
                </Button>
              </div>

              <div className="space-y-4 p-6">
                <Card className="p-4">
                  <div className="text-lg font-semibold text-slate-900">{t("settingsLanguageTitle")}</div>
                  <p className="mt-1 text-sm text-slate-600">{t("settingsLanguageDesc")}</p>
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

                <Card className="p-4">
                  <div className="text-lg font-semibold text-slate-900">{t("settingsTempDirTitle")}</div>
                  <p className="mt-1 text-sm text-slate-600">{t("settingsTempDirDesc")}</p>
                  <div className="mt-4">
                    <Button variant="outline" onClick={onOpenTempDir}>
                      <FolderOpen size={16} />
                      {t("settingsOpenTempDir")}
                    </Button>
                  </div>
                </Card>
              </div>
            </section>
          </div>
        </div>
      )}

      <RecordingDialog
        open={isRecording}
        recordingSeconds={recordingSeconds}
        inputLevel={inputLevel}
        levelChartData={levelChartData}
        onStop={onRecordClick}
      />
    </main>
  );
}

export default App;
