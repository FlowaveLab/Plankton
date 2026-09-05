import { getCurrentWindow } from "@tauri-apps/api/window";
import ReactDOM from "react-dom/client";
import "streamdown/styles.css";

import { surfaceForWindowLabel } from "./handoff";
import "./components/desktop/workspace.css";
import "./components/approval-chat.css";
import "./index.css";
import "./styles.css";

const appRoot = document.querySelector<HTMLDivElement>("#app");

if (!appRoot) {
  throw new Error("Missing #app root");
}

const windowLabel = (window as Window & { __TAURI_INTERNALS__?: object })
  .__TAURI_INTERNALS__
  ? getCurrentWindow().label
  : "main";
const surface = surfaceForWindowLabel(windowLabel);
const surfaceModule =
  surface === "compact"
    ? import("./components/CompactApproval")
    : surface === "password-change"
      ? import("./components/PasswordChangeConfirmation")
      : import("./App");

void surfaceModule
  .then((module) => {
    const Surface =
      surface === "compact"
        ? (module as typeof import("./components/CompactApproval"))
            .CompactApproval
        : surface === "password-change"
          ? (module as typeof import("./components/PasswordChangeConfirmation"))
              .PasswordChangeConfirmation
          : (module as typeof import("./App")).default;
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
