import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

const isOverlayRoute = window.location.hash.includes("/overlay");
const splash = document.getElementById("boot-splash");
if (splash && isOverlayRoute) {
  splash.remove();
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

if (splash && !isOverlayRoute) {
  window.requestAnimationFrame(() => {
    splash.classList.add("tm-hide");
    window.setTimeout(() => splash.remove(), 220);
  });
}
