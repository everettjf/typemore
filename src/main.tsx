import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

const splash = document.getElementById("boot-splash");
if (splash) {
  window.requestAnimationFrame(() => {
    splash.classList.add("tm-hide");
    window.setTimeout(() => splash.remove(), 220);
  });
}
