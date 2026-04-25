import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { cn } from "../../lib/utils";
import { resolveUiLangFromLocalSetting, type UiLang } from "../../lib/lang";

type OverlayPhase = "hidden" | "listening" | "thinking" | "ready";

type OverlayStatePayload = {
  phase: OverlayPhase;
  text?: string | null;
  level?: number | null;
};

const OVERLAY_BAR_COUNT = 28;

export function OverlayWindowApp() {
  const [phase, setPhase] = useState<OverlayPhase>("hidden");
  const [text, setText] = useState("");
  const [uiLang, setUiLang] = useState<UiLang>(() => resolveUiLangFromLocalSetting());
  const [level, setLevel] = useState(0);
  const [levelHistory, setLevelHistory] = useState<number[]>(() =>
    Array(OVERLAY_BAR_COUNT).fill(0)
  );

  useEffect(() => {
    document.documentElement.classList.add("overlay-mode");
    document.body.classList.add("overlay-mode");
    document.getElementById("root")?.classList.add("overlay-mode");
    let unlisten: (() => void) | undefined;
    listen<OverlayStatePayload>("overlay-state", (event) => {
      const nextUiLang = resolveUiLangFromLocalSetting();
      setUiLang(nextUiLang);
      const nextPhase = event.payload.phase;
      setPhase(nextPhase);
      setText((prev) => {
        if (typeof event.payload.text === "string") {
          return event.payload.text;
        }
        if (nextPhase === "hidden") {
          return "";
        }
        return prev;
      });
      if (typeof event.payload.level === "number") {
        const lv = Math.max(0, Math.min(1, event.payload.level));
        setLevel(lv);
        setLevelHistory((prev) => [...prev.slice(1), lv]);
      } else if (nextPhase === "hidden" || nextPhase === "ready") {
        setLevel(0);
        if (nextPhase === "hidden") {
          setLevelHistory(Array(OVERLAY_BAR_COUNT).fill(0));
        }
      }
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

  const isListening = phase === "listening";
  const isReady = phase === "ready";
  const title = isListening
    ? uiLang === "zh"
      ? "听写中"
      : "Listening"
    : phase === "thinking"
      ? text?.trim() || (uiLang === "zh" ? "处理中" : "Processing")
      : uiLang === "zh"
        ? "完成"
        : "Ready";

  const ringBaseColor = isListening
    ? "bg-emerald-400"
    : isReady
      ? "bg-emerald-300"
      : "bg-sky-400";
  const ringHaloColor = isListening
    ? "bg-emerald-400/40"
    : isReady
      ? "bg-emerald-300/30"
      : "bg-sky-400/30";
  const ringScale = 1 + (isListening ? level * 1.1 : 0);
  const ringOpacity = isListening ? 0.25 + level * 0.75 : 0.35;
  const titleColor = isListening
    ? "text-emerald-200"
    : isReady
      ? "text-emerald-200"
      : "text-sky-200";

  return (
    <main className="h-screen w-screen bg-transparent p-0">
      <div
        className={cn(
          "flex h-full w-full items-center gap-3 overflow-hidden rounded-[18px] border border-white/15 bg-black/85 px-3 text-white shadow-[0_12px_32px_rgba(0,0,0,0.5)] transition-opacity duration-300",
          isReady ? "opacity-95" : "opacity-100"
        )}
      >
        <div className="relative flex h-8 w-8 flex-none items-center justify-center">
          <div
            className={cn("absolute inset-0 rounded-full", ringHaloColor)}
            style={{
              transform: `scale(${ringScale})`,
              opacity: ringOpacity,
              transition: "transform 80ms linear, opacity 80ms linear",
            }}
          />
          <div
            className={cn(
              "relative h-3 w-3 rounded-full shadow-[0_0_8px_currentColor]",
              ringBaseColor
            )}
          />
        </div>

        <div className="flex h-full min-w-0 flex-1 flex-col justify-center gap-1 py-1">
          <div
            className={cn(
              "truncate text-xs font-semibold leading-tight tracking-tight",
              titleColor
            )}
          >
            {title}
          </div>
          {isListening ? (
            <div className="flex h-4 items-center gap-[2px]">
              {levelHistory.map((lv, i) => {
                const pct = Math.max(6, Math.min(100, lv * 130));
                return (
                  <div
                    key={i}
                    className="w-[3px] flex-none rounded-[1px] bg-emerald-400"
                    style={{
                      height: `${pct}%`,
                      opacity: 0.35 + lv * 0.65,
                      transition: "height 70ms ease-out, opacity 70ms ease-out",
                    }}
                  />
                );
              })}
            </div>
          ) : (
            <div className="truncate text-[10px] leading-tight text-white/60">
              {text?.trim() ?? ""}
            </div>
          )}
        </div>
      </div>
    </main>
  );
}
