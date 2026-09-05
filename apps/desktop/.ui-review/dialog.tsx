import ReactDOM from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import "../src/components/desktop/workspace.css";
import "../src/index.css";
import "../src/styles.css";
import { PasswordAddDialog } from "../src/components/desktop/PasswordAddDialog";
const locale =
  new URLSearchParams(location.search).get("locale") === "zh-CN"
    ? "zh-CN"
    : "en";
mockIPC((command) => {
  if (command === "preview_password_draft")
    return {
      descriptor: {
        kind: "environment",
        names: [
          "TEST_EMAIL_PASSWORD",
          "TEST_DATABASE_PASSWORD",
          "TEST_API_TOKEN",
        ],
      },
      entries: [
        "TEST_EMAIL_PASSWORD",
        "TEST_DATABASE_PASSWORD",
        "TEST_API_TOKEN",
      ].map((key) => ({ key, value: "DemoOnly-NotARealPassword!" })),
      suggested_item_title: "功能测试凭据（虚构）",
    };
  if (command === "list_backend_connections") return [];
  if (command === "list_local_vaults")
    return [
      {
        id: "default",
        label: locale === "zh-CN" ? "个人保险库" : "Personal vault",
      },
    ];
  throw new Error(`Preview does not support ${command}`);
});
ReactDOM.createRoot(document.getElementById("app")!).render(
  <div className="desktop-workspace">
    <aside className="workspace-navigation">
      <div className="workspace-brand">
        <strong>PLANKTON</strong>
      </div>
    </aside>
    <main className="workspace-content">
      <PasswordAddDialog
        draftId="ui-preview-only"
        locale={locale}
        onClose={() => {}}
        onCommitted={() => {}}
      />
    </main>
  </div>,
);
