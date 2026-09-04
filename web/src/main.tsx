import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { completePassportCallback } from "./passport";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("Missing root element");

if (completePassportCallback()) {
  createRoot(root).render(
    <main className="callback">
      <p>Returning to Pubky Watcher Canvas…</p>
      <a href={import.meta.env.BASE_URL}>Return manually</a>
    </main>,
  );
} else {
  createRoot(root).render(<StrictMode><App /></StrictMode>);
}
