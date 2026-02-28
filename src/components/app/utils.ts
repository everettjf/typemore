import type { ModelInitStatus } from "./types";

export function formatTime(ms: number): string {
  return new Date(ms).toLocaleString();
}

export function formatListTime(ms: number): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(ms));
}

export function formatCurrentRecordingTime(ms: number): string {
  return new Intl.DateTimeFormat(undefined, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(ms));
}

export function defaultInitStatus(): ModelInitStatus {
  return {
    running: false,
    phase: "idle",
    progress: 0,
    message: "模型尚未初始化",
    ready: false,
    error: null,
  };
}

export function badgeClass(ok: boolean): string {
  return ok
    ? "bg-emerald-100 text-emerald-700 border-emerald-200"
    : "bg-amber-100 text-amber-700 border-amber-200";
}
