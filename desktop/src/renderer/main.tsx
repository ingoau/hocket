import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./styles/global.css";
import "./styles/m3-shell.css";
import "./styles/m3-library.css";
import "./styles/m3-pages.css";
import "./styles/controls.css";
import "./styles/now-playing.css";
import "@applemusic-like-lyrics/core/style.css";
import { App } from "./App";
import { MiniApp } from "./views/MiniApp";
import { connectStore } from "./store/app";

const params = new URLSearchParams(window.location.search);
const windowKind = params.get("window") === "mini" ? "mini" : "main";

connectStore();

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>{windowKind === "mini" ? <MiniApp /> : <App />}</StrictMode>,
);
