import { Check, Copy, Loader2, RefreshCcw } from "lucide-react";
import type { RecordingItem, ModelInitStatus } from "./types";
import { formatCurrentRecordingTime } from "./utils";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { Textarea } from "../ui/textarea";

type TranscriptPanelProps = {
  selected: RecordingItem | null;
  isBusy: boolean;
  initStatus: ModelInitStatus;
  modelReady: boolean;
  transcript: string;
  copied: boolean;
  onTranscriptChange: (value: string) => void;
  onRetranscribe: () => void;
  onCopy: () => void;
};

export function TranscriptPanel({
  selected,
  isBusy,
  initStatus,
  modelReady,
  transcript,
  copied,
  onTranscriptChange,
  onRetranscribe,
  onCopy,
}: TranscriptPanelProps) {
  return (
    <Card className="flex min-h-0 flex-col p-3">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
        <div>
          <div className="text-sm text-slate-500">{selected ? `当前录音: ${formatCurrentRecordingTime(selected.createdAtMs)}` : "当前未选中录音"}</div>
        </div>
        <div className="inline-flex items-center gap-2">
          <Button
            variant="outline"
            className="h-9 w-9 justify-center p-0"
            onClick={onRetranscribe}
            disabled={!selected || isBusy || initStatus.running || !modelReady}
            title="重新识别"
          >
            {isBusy ? <Loader2 className="animate-spin" size={15} /> : <RefreshCcw size={15} />}
          </Button>
          <Button
            variant="outline"
            className="h-9 w-9 justify-center p-0"
            onClick={onCopy}
            disabled={!transcript.trim()}
            title="复制结果"
          >
            {copied ? <Check size={15} /> : <Copy size={15} />}
          </Button>
        </div>
      </div>

      <Textarea
        value={transcript}
        onChange={(e) => onTranscriptChange(e.target.value)}
        placeholder="识别结果会显示在这里..."
      />
    </Card>
  );
}
