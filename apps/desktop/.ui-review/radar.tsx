import ReactDOM from "react-dom/client";
import {
  ExposureRadar,
  defaultExposurePolicy,
} from "../src/components/ExposurePolicy";
import "../src/styles.css";
import "../src/components/desktop/password-vault.css";
const before = defaultExposurePolicy();
const after = {
  ...before,
  surfaces: before.surfaces.map((field) => ({
    ...field,
    max_level: field.surface === "network" ? 2 : 1,
  })),
};
ReactDOM.createRoot(document.getElementById("app")!).render(
  <main
    style={{
      maxWidth: 560,
      margin: "30px auto",
      padding: 20,
      background: "#fffefb",
    }}
  >
    <ExposureRadar
      locale="zh-CN"
      primary={after}
      secondary={before}
      breachedSurfaces={["network", "llm_context"]}
    />
  </main>,
);
