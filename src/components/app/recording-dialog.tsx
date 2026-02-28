import * as Dialog from "@radix-ui/react-dialog";
import { AudioLines, Mic } from "lucide-react";
import { Line } from "react-chartjs-2";
import { Button } from "../ui/button";

type RecordingDialogProps = {
  open: boolean;
  recordingSeconds: number;
  inputLevel: number;
  levelChartData: {
    labels: number[];
    datasets: Array<{
      data: number[];
      borderColor: string;
      backgroundColor: string;
      borderWidth: number;
      pointRadius: number;
      tension: number;
      fill: boolean;
    }>;
  };
  onStop: () => void;
};

export function RecordingDialog({
  open,
  recordingSeconds,
  inputLevel,
  levelChartData,
  onStop,
}: RecordingDialogProps) {
  return (
    <Dialog.Root open={open}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-slate-900/45 backdrop-blur-[3px]" />
        <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-[92vw] max-w-xl -translate-x-1/2 -translate-y-1/2 rounded-2xl border border-white/25 bg-slate-900 p-6 text-slate-100 shadow-2xl outline-none">
          <Dialog.Title className="inline-flex items-center gap-2 text-lg font-semibold">
            <AudioLines size={20} />
            正在录音
          </Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-slate-300">
            已录制 {recordingSeconds}s，持续说话以观察输入电平变化。
          </Dialog.Description>

          <div className="mt-5 grid gap-5">
            <div
              className="mx-auto h-28 w-28 rounded-full bg-gradient-to-tr from-emerald-400 to-cyan-400 shadow-[0_0_80px_8px_rgba(16,185,129,0.35)] transition-all duration-100"
              style={{ transform: `scale(${1 + inputLevel * 0.5})` }}
            />

            <div className="rounded-xl border border-slate-700 bg-slate-800/70 p-3">
              <Line
                data={levelChartData}
                options={{
                  animation: false,
                  responsive: true,
                  maintainAspectRatio: false,
                  plugins: { legend: { display: false }, tooltip: { enabled: false } },
                  scales: {
                    x: { display: false },
                    y: { display: false, min: 0, max: 1 },
                  },
                }}
                height={90}
              />
            </div>

            <Button variant="destructive" className="mx-auto inline-flex items-center gap-2" onClick={onStop}>
              <Mic size={15} />
              停止并识别
            </Button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
