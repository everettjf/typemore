import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
import { RecordingListPanel } from "./components/app/recording-list-panel";
import { TranscriptPanel } from "./components/app/transcript-panel";
import type { ModelInitStatus, RecordingItem, SaveAndTranscribeResult } from "./components/app/types";
import { defaultInitStatus } from "./components/app/utils";

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Filler, Tooltip);

function App() {
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
    if (!selectedId && items.length > 0) {
      setSelectedId(items[0].id);
    }
  }

  async function loadInitStatus() {
    const status = await invoke<ModelInitStatus>("get_model_init_status");
    setInitStatus(status);
  }

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
        setSelectedId(null);
        setTranscript("");
      }
    } catch (err) {
      setTranscript(`删除失败: ${String(err)}`);
    }
  }

  return (
    <main className="min-h-screen bg-[radial-gradient(circle_at_top_left,_#eff6ff,_#f8fafc_55%,_#ecfeff)] p-4 text-slate-900 md:p-6">
      <div className="mx-auto grid h-[calc(100vh-2rem)] max-w-[1520px] grid-cols-1 gap-4 rounded-2xl border border-white/70 bg-white/60 p-3 shadow-xl shadow-slate-200/60 backdrop-blur md:h-[calc(100vh-3rem)] md:grid-cols-[370px_1fr] md:p-4">
        <RecordingListPanel
          recordings={recordings}
          selectedId={selectedId}
          modelReady={modelReady}
          initStatus={initStatus}
          isRecording={isRecording}
          isBusy={isBusy}
          onRecordClick={onRecordClick}
          onInitModel={onInitModel}
          onSelect={setSelectedId}
          onRename={onRename}
          onDelete={onDelete}
        />

        <TranscriptPanel
          selected={selected}
          isBusy={isBusy}
          initStatus={initStatus}
          modelReady={modelReady}
          transcript={transcript}
          copied={copied}
          onTranscriptChange={setTranscript}
          onRetranscribe={onRetranscribeSelected}
          onCopy={onCopyTranscript}
        />
      </div>

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
