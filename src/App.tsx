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

const DICTIONARY_STORAGE_KEY = "typemore.dictionary.words";

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
    Promise.all([loadRecordings(), loadInitStatus()]).catch((err) => {
      setTranscript(`初始化失败: ${String(err)}`);
    });

    let unlisten: (() => void) | undefined;
    listen<ModelInitStatus>("model-init-progress", (event) => {
      setInitStatus(event.payload);
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((err) => {
        setTranscript(`监听模型进度失败: ${String(err)}`);
      });

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

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
        setTranscript("模型已就绪，可以开始录音识别。");
      }
    } catch (err) {
      setTranscript(`模型初始化失败: ${String(err)}`);
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
          suggestedName: `录音_${new Date().toISOString().replace(/[:.]/g, "-")}`,
          wavData,
        };

        const result = await invoke<SaveAndTranscribeResult>("save_recording_and_transcribe", {
          payload,
        });

        setRecordings((prev) => [result.recording, ...prev]);
        setSelectedId(result.recording.id);
        setTranscript(result.text || "(无识别结果)");
        setPage("history");
      } catch (err) {
        setTranscript(`录音识别失败: ${String(err)}`);
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
    streamRef.current?.getTracks().forEach((t) => t.stop());
    streamRef.current = null;
    recorderRef.current = null;
    setIsRecording(false);
  }

  async function onRecordClick() {
    if (!modelReady) {
      setTranscript("请先初始化模型，再开始录音。");
      return;
    }
    if (isRecording) {
      stopRecording();
      return;
    }
    try {
      await startRecording();
    } catch (err) {
      setTranscript(`无法访问麦克风: ${String(err)}`);
      stopLevelMonitor();
    }
  }

  async function onRetranscribeSelected() {
    if (!selected) {
      setTranscript("请先在左侧选择一个录音。");
      return;
    }
    setIsBusy(true);
    try {
      const text = await invoke<string>("transcribe_recording", {
        id: selected.id,
        force: true,
      });
      setTranscript(text || "(无识别结果)");
    } catch (err) {
      setTranscript(`识别失败: ${String(err)}`);
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
      setTranscript((prev) => `${prev}\n\n[复制失败] ${String(err)}`);
    }
  }

  async function onRename(recording: RecordingItem) {
    const name = window.prompt("输入新名称", recording.name);
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
      setTranscript(`重命名失败: ${String(err)}`);
    }
  }

  async function onDelete(recording: RecordingItem) {
    try {
      await invoke("delete_recording", { id: recording.id });
      setRecordings((prev) => prev.filter((item) => item.id !== recording.id));
      if (selectedId === recording.id) {
        const remaining = recordings.filter((item) => item.id !== recording.id);
        setSelectedId(remaining.length > 0 ? remaining[0].id : null);
        setTranscript("");
      }
    } catch (err) {
      setTranscript(`删除失败: ${String(err)}`);
    }
  }

  async function onOpenTempDir() {
    try {
      const dir = await invoke<string>("open_temp_dir");
      setTranscript(`已打开临时目录: ${dir}`);
    } catch (err) {
      setTranscript(`打开临时目录失败: ${String(err)}`);
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
    { key: "home", label: "Home", icon: Home },
    { key: "history", label: "History", icon: History },
    { key: "dictionary", label: "Dictionary", icon: BookText },
  ];

  return (
    <main className="min-h-screen bg-[radial-gradient(circle_at_top_left,_#edf4ff,_#f8fafc_50%,_#eef7ff)] p-3 text-slate-900 md:p-5">
      <div className="mx-auto grid h-[calc(100vh-1.5rem)] max-w-[1540px] grid-cols-1 gap-3 rounded-3xl border border-white/70 bg-white/60 p-3 shadow-2xl shadow-slate-200/70 backdrop-blur md:h-[calc(100vh-2.5rem)] md:grid-cols-[230px_1fr] md:p-4">
        <aside className="flex min-h-0 flex-col rounded-2xl border border-slate-200/80 bg-white/95 p-3">
          <div className="px-2 pb-3 pt-1">
            <div className="text-2xl font-bold tracking-tight">Typemore</div>
            <div className="text-xs text-slate-500">Offline Speech to Text</div>
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
              Settings
            </button>
          </div>
        </aside>

        <section className="min-h-0 overflow-hidden rounded-2xl border border-slate-200/80 bg-white/95 p-4 md:p-5">
          {page === "home" && (
            <div className="grid h-full min-h-0 gap-4 md:grid-rows-[auto_auto_1fr]">
              <header className="flex flex-wrap items-center justify-between gap-3">
                <div>
                  <h1 className="text-3xl font-semibold tracking-tight">Speak naturally, write clearly.</h1>
                  <p className="mt-1 text-sm text-slate-500">
                    Focus on core workflow: model init, recording, and transcript.
                  </p>
                </div>
                <Badge className={cn(badgeClass(modelReady))}>
                  {modelReady ? "Model Ready" : initStatus.running ? "Initializing" : "Not Ready"}
                </Badge>
              </header>

              <Card className="p-4">
                <div className="mb-4 flex flex-wrap items-center gap-2">
                  <Button
                    className={cn(
                      "inline-flex items-center gap-2",
                      isRecording ? "bg-red-600 hover:bg-red-700 border-red-600 hover:border-red-700" : ""
                    )}
                    onClick={onRecordClick}
                    disabled={isBusy || initStatus.running}
                  >
                    <Mic size={16} />
                    {isRecording ? "停止录音" : "开始录音"}
                  </Button>
                  <Button
                    variant="outline"
                    className="inline-flex items-center gap-2"
                    onClick={onInitModel}
                    disabled={isBusy || initStatus.running}
                  >
                    {initStatus.running ? <Loader2 className="animate-spin" size={15} /> : <Sparkles size={15} />}
                    {initStatus.running ? "初始化中..." : modelReady ? "模型已就绪" : "初始化模型"}
                  </Button>
                  <Button variant="outline" onClick={() => setPage("history")}>
                    查看 History
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
                  <div className="text-sm font-semibold text-slate-700">最近录音</div>
                  <div className="text-xs text-slate-500">{recordings.length} 条</div>
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
                        还没有录音记录，先初始化模型并开始录音。
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
                  <div className="text-lg font-semibold">History</div>
                  <div className="text-xs text-slate-500">{recordings.length} 条</div>
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
                              重命名
                            </Button>
                            <Button variant="outline" className="h-7 px-2 text-xs" onClick={() => onDelete(item)}>
                              <Trash2 size={13} />
                              删除
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
                    {selected ? `当前录音: ${formatCurrentRecordingTime(selected.createdAtMs)}` : "当前未选中录音"}
                  </div>
                  <div className="inline-flex items-center gap-2">
                    <Button
                      variant="outline"
                      className="h-9 w-9 justify-center p-0"
                      onClick={onRetranscribeSelected}
                      disabled={!selected || isBusy || initStatus.running || !modelReady}
                      title="重新识别"
                    >
                      {isBusy ? <Loader2 className="animate-spin" size={15} /> : <RefreshCcw size={15} />}
                    </Button>
                    <Button
                      variant="outline"
                      className="h-9 w-9 justify-center p-0"
                      onClick={onCopyTranscript}
                      disabled={!transcript.trim()}
                      title="复制结果"
                    >
                      {copied ? <Check size={15} /> : <Copy size={15} />}
                    </Button>
                  </div>
                </div>

                <Textarea
                  value={transcript}
                  onChange={(e) => setTranscript(e.target.value)}
                  placeholder="识别结果会显示在这里..."
                />
              </Card>
            </div>
          )}

          {page === "dictionary" && (
            <div className="grid h-full min-h-0 gap-4 md:grid-rows-[auto_auto_1fr]">
              <header className="flex items-center justify-between">
                <h2 className="text-3xl font-semibold tracking-tight">Dictionary</h2>
                <Badge className="bg-slate-100 text-slate-700 border-slate-200">{dictionaryWords.length} words</Badge>
              </header>

              <Card className="p-3">
                <div className="flex flex-wrap gap-2">
                  <input
                    value={newWord}
                    onChange={(e) => setNewWord(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        addDictionaryWord();
                      }
                    }}
                    placeholder="添加新词，比如 TestFlight"
                    className="h-10 min-w-[220px] flex-1 rounded-md border border-slate-300 px-3 text-sm outline-none ring-sky-300 transition focus:ring"
                  />
                  <Button onClick={addDictionaryWord}>
                    <Plus size={15} />
                    Add word
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
                          title="删除词条"
                        >
                          <X size={13} />
                        </button>
                      </div>
                    ))}
                    {dictionaryWords.length === 0 && (
                      <div className="w-full rounded-xl border border-dashed border-slate-300 px-3 py-10 text-center text-sm text-slate-500">
                        还没有词条，先添加几个常用专有名词。
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
              <div className="mb-3 px-2 text-xs font-semibold uppercase tracking-wider text-slate-500">Settings</div>
              <div className="rounded-lg bg-white px-3 py-2 text-sm font-medium text-slate-900 shadow-sm">General</div>
            </aside>

            <section className="flex min-h-0 flex-col">
              <div className="flex items-center justify-between border-b border-slate-200 px-6 py-4">
                <h2 className="text-3xl font-semibold tracking-tight">Settings</h2>
                <Button variant="outline" className="h-9 w-9 justify-center p-0" onClick={() => setSettingsOpen(false)}>
                  <X size={16} />
                </Button>
              </div>

              <div className="space-y-4 p-6">
                <Card className="p-4">
                  <div className="text-lg font-semibold text-slate-900">临时目录</div>
                  <p className="mt-1 text-sm text-slate-600">
                    打开应用的临时目录，用于查看当前运行过程中的临时文件。
                  </p>
                  <div className="mt-4">
                    <Button variant="outline" onClick={onOpenTempDir}>
                      <FolderOpen size={16} />
                      打开临时目录
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
