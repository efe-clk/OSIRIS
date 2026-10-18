import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import App from "./App";
import "./index.css";

async function boot() {
  // Demo modu yalnızca VITE_DEMO=1 ile derlenen paketlerde vardır (üretim derlemesinden ayıklanır).
  if (import.meta.env.VITE_DEMO === "1") {
    const { installDemo } = await import("./demo");
    installDemo();
  }
  // Not: yönlendirici App içinde (HashRouter — /ui/ alt yolunda çalışır).
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

boot();
