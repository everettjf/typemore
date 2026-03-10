import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

const isOverlayRoute = window.location.hash.includes("/overlay");

if (!isOverlayRoute) {
  const splash = document.createElement("div");
  splash.id = "boot-splash";
  splash.setAttribute("aria-hidden", "true");
  splash.style.position = "fixed";
  splash.style.inset = "0";
  splash.style.zIndex = "9999";
  splash.style.display = "flex";
  splash.style.alignItems = "center";
  splash.style.justifyContent = "center";
  splash.style.background =
    "radial-gradient(1200px 460px at -12% -8%, #d9ebff 0%, transparent 60%)," +
    "radial-gradient(860px 420px at 110% 0%, #d2f3ff 0%, transparent 55%)," +
    "linear-gradient(145deg, #f3f8ff 0%, #f8fbff 48%, #ecf6ff 100%)";
  splash.style.transition = "opacity 180ms ease";
  splash.innerHTML =
    '<div style="display:inline-flex;align-items:center;gap:12px;padding:14px 18px;border-radius:16px;border:1px solid rgba(148,163,184,0.26);background:rgba(255,255,255,0.88);box-shadow:0 12px 36px rgba(15,23,42,0.1);color:#0f172a;font-family:Avenir Next,SF Pro Rounded,SF Pro Text,PingFang SC,Segoe UI,sans-serif;">' +
    '<img src="/favicon.png" alt="" width="20" height="20" />' +
    "<strong>Type More</strong>" +
    '<span style="width:10px;height:10px;border-radius:9999px;background:#1d4ed8;display:inline-block;"></span>' +
    "</div>";
  document.body.appendChild(splash);

  window.requestAnimationFrame(() => {
    splash.style.opacity = "0";
    splash.style.pointerEvents = "none";
    window.setTimeout(() => splash.remove(), 220);
  });
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
