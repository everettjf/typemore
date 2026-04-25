export type UiLang = "zh" | "en";

export const LANG_MODE_STORAGE_KEY = "typemore.lang.mode";

export function detectSystemLang(): UiLang {
  const lang = (navigator.language || "en-US").toLowerCase();
  return lang.startsWith("zh") ? "zh" : "en";
}

export function resolveUiLangFromLocalSetting(): UiLang {
  const raw = window.localStorage.getItem(LANG_MODE_STORAGE_KEY);
  if (raw === "zh-CN") {
    return "zh";
  }
  if (raw === "en-US") {
    return "en";
  }
  return detectSystemLang();
}
