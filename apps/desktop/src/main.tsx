import { getCurrentWindow } from "@tauri-apps/api/window";
import ReactDOM from "react-dom/client";
import "streamdown/styles.css";

import { surfaceForWindowLabel } from "./handoff";
import "./components/desktop/workspace.css";
import "./components/approval-chat.css";
import "./index.css";
import "./styles.css";
// Native companion windows must have their layout before a lazy surface mounts.
// Keep these critical styles in index.html's stylesheet, not an async CSS chunk.
import "./components/approval-notification.css";
import "./components/remember-exposure.css";

const appRoot = document.querySelector<HTMLDivElement>("#app");

if (!appRoot) {
  throw new Error("Missing #app root");
}

const windowLabel = (window as Window & { __TAURI_INTERNALS__?: object })
  .__TAURI_INTERNALS__
  ? getCurrentWindow().label
  : "main";
const surface = surfaceForWindowLabel(windowLabel);
async function loadSurface() {
  // Keep imports in separate branches. A ternary import expression can merge
  // Vite's preload dependencies and leave another surface's CSS unloaded.
  if (surface === "compact") {
    return (await import("./components/CompactApproval")).CompactApproval;
  }
  if (surface === "notification") {
    return (await import("./components/ApprovalNotification"))
      .ApprovalNotification;
  }
  if (surface === "password-change") {
    return (await import("./components/PasswordChangeConfirmation"))
      .PasswordChangeConfirmation;
  }
  return (await import("./App")).default;
}

void loadSurface()
  .then((Surface) => {
    ReactDOM.createRoot(appRoot).render(<Surface />);
  })
  .catch((error: unknown) => {
    console.error("Plankton UI failed to start.", error);
    const message = error instanceof Error ? error.message : String(error);
    const alert = document.createElement("main");
    alert.className = "startup-error";
    alert.setAttribute("role", "alert");

    const title = document.createElement("h1");
    title.textContent = "Plankton UI failed to start";
    const detail = document.createElement("p");
    detail.textContent = message;
    alert.append(title, detail);
    appRoot.replaceChildren(alert);
  });
