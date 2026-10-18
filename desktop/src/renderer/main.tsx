import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./styles/global.css";
import "./styles/m3-shell.css";
import "./styles/m3-library.css";
import "./styles/m3-pages.css";
import "./styles/controls.css";
import "./styles/now-playing.css";
import "./styles/menus.css";
import { connectStore } from "./store/app";

const params = new URLSearchParams(window.location.search);
const windowKind = params.get("window") === "mini" ? "mini" : "main";

connectStore();

const root = createRoot(document.getElementById("root") as HTMLElement);

if (windowKind === "mini") {
  // The mini player is a second renderer: keep Kawarp, AMLL and the main shell out of it.
  void import("./views/MiniApp").then(({ MiniApp }) => {
    root.render(<StrictMode><MiniApp /></StrictMode>);
  });
} else {
  void import("./App").then(({ App }) => {
    root.render(<StrictMode><App /></StrictMode>);
  });
}
