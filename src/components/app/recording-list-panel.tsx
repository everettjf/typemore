import { useEffect, useRef, useState } from "react";
import * as ContextMenu from "@radix-ui/react-context-menu";
import { Brain, CircleDot, Loader2, Mic, Sparkles } from "lucide-react";
import type { RecordingItem, ModelInitStatus } from "./types";
import { badgeClass, formatListTime, formatTime } from "./utils";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { Progress } from "../ui/progress";
import { ScrollArea } from "../ui/scroll-area";
import { Separator } from "../ui/separator";
import { cn } from "../../lib/utils";

type RecordingListPanelProps = {
  recordings: RecordingItem[];
  selectedId: string | null;
  modelReady: boolean;
  initStatus: ModelInitStatus;
  isRecording: boolean;
  isBusy: boolean;
  onRecordClick: () => void;
  onInitModel: () => void;
  onOpenTempDir: () => void;
  onSelect: (id: string) => void;
  onRename: (recording: RecordingItem) => void;
  onDelete: (recording: RecordingItem) => void;
};

export function RecordingListPanel({
  recordings,
  selectedId,
  modelReady,
  initStatus,
  isRecording,
  isBusy,
  onRecordClick,
  onInitModel,
  onOpenTempDir,
  onSelect,
  onRename,
  onDelete,
}: RecordingListPanelProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!menuOpen) {
      return;
    }
    const onDocClick = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) {
        setMenuOpen(false);
      }
    };
    const onEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setMenuOpen(false);
      }
    };
    document.addEventListener("mousedown", onDocClick);
    document.addEventListener("keydown", onEscape);
    return () => {
      document.removeEventListener("mousedown", onDocClick);
      document.removeEventListener("keydown", onEscape);
    };
  }, [menuOpen]);

  return (
    <Card className="flex min-h-0 flex-col bg-white/90">
      <div className="flex flex-wrap gap-2 border-b border-slate-200 p-3">
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
          {initStatus.running ? <Loader2 className="animate-spin" size={15} /> : <Brain size={15} />}
          {initStatus.running ? "初始化中..." : initStatus.ready ? "模型已就绪" : "初始化模型"}
        </Button>

        <div className="relative" ref={menuRef}>
          <Button
            variant="outline"
            className="inline-flex items-center gap-1"
            onClick={() => setMenuOpen((v) => !v)}
          >
            菜单
          </Button>
          {menuOpen && (
            <div className="absolute right-0 top-[calc(100%+6px)] z-20 min-w-40 rounded-md border border-slate-200 bg-white p-1 shadow-xl">
              <button
                type="button"
                className="block w-full rounded px-2 py-1.5 text-left text-sm text-slate-700 hover:bg-slate-100"
                onClick={() => {
                  setMenuOpen(false);
                  onOpenTempDir();
                }}
              >
                打开临时目录
              </button>
            </div>
          )}
        </div>
      </div>

      <div className="space-y-3 border-b border-slate-200 p-3">
        <div className="flex items-center justify-between text-xs text-slate-500">
          <span className="inline-flex items-center gap-1"><Sparkles size={14} /> 模型状态</span>
          <Badge className={cn(badgeClass(modelReady))}>
            {modelReady ? "Ready" : initStatus.running ? "Running" : "Not Ready"}
          </Badge>
        </div>
        <div className="text-sm text-slate-700 tabular-nums whitespace-nowrap overflow-hidden text-ellipsis">
          {initStatus.message}
        </div>
        <Progress value={Math.min(100, Math.max(0, initStatus.progress))} />
        {initStatus.error && <div className="text-xs text-red-600">{initStatus.error}</div>}
      </div>

      <div className="flex items-center justify-between px-3 py-2 text-sm font-semibold text-slate-700">
        <span>录音列表</span>
        <span className="text-xs font-normal text-slate-500">{recordings.length} 条</span>
      </div>

      <Separator />

      <ScrollArea className="min-h-0 flex-1 overflow-hidden" viewportClassName="h-full w-full p-2">
        <ul className="space-y-1">
          {recordings.map((item) => (
            <ContextMenu.Root key={item.id}>
              <ContextMenu.Trigger asChild>
                <li>
                  <Button
                    variant="outline"
                    className={cn(
                      "w-full rounded-lg border px-3 py-2 text-left transition whitespace-normal",
                      selectedId === item.id
                        ? "border-sky-300 bg-sky-50"
                        : "border-transparent hover:border-slate-200 hover:bg-slate-50"
                    )}
                    onClick={() => onSelect(item.id)}
                    title={item.filePath}
                  >
                    <div className="text-xs font-semibold text-slate-800 leading-5">{formatListTime(item.createdAtMs)}</div>
                    <div className="mt-1 inline-flex items-center gap-1 text-[11px] text-slate-500">
                      <CircleDot size={12} />
                      {formatTime(item.createdAtMs)}
                    </div>
                  </Button>
                </li>
              </ContextMenu.Trigger>
              <ContextMenu.Portal>
                <ContextMenu.Content className="z-50 min-w-36 rounded-md border border-slate-200 bg-white p-1 shadow-xl">
                  <ContextMenu.Item
                    className="cursor-pointer rounded px-2 py-1.5 text-sm text-slate-700 outline-none hover:bg-slate-100"
                    onSelect={() => onRename(item)}
                  >
                    重命名
                  </ContextMenu.Item>
                  <ContextMenu.Item
                    className="cursor-pointer rounded px-2 py-1.5 text-sm text-red-600 outline-none hover:bg-red-50"
                    onSelect={() => onDelete(item)}
                  >
                    删除
                  </ContextMenu.Item>
                </ContextMenu.Content>
              </ContextMenu.Portal>
            </ContextMenu.Root>
          ))}
        </ul>
      </ScrollArea>
    </Card>
  );
}
