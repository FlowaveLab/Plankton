import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState, type JSX } from "react";
import type { AcpConfigOption, AcpProbeResult, AcpProfile } from "../types";

export function AcpSessionOptions({
  profile,
  disabled,
  zh,
  onChange,
  context = "approval",
}: {
  context?: "approval" | "chat";
  profile: AcpProfile;
  disabled: boolean;
  zh: boolean;
  onChange: (profile: AcpProfile) => void;
}): JSX.Element {
  const [options, setOptions] = useState<AcpConfigOption[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rejected, setRejected] = useState<string[]>([]);
  const [revision, setRevision] = useState(0);
  const profileKey = JSON.stringify(profile);
  useEffect(() => {
    let active = true;
    setOptions([]);
    setError(null);
    setRejected([]);
    setLoading(false);
    if (
      (profile.version_mode === "custom" && !profile.program?.trim()) ||
      (profile.version_mode === "pinned" && !profile.version?.trim())
    )
      return;
    setLoading(true);
    // Debounce draft edits; stale agent responses must never overwrite another agent's catalog.
    const timer = setTimeout(() => {
      void invoke<AcpProbeResult>("discover_acp_options", { profile })
        .then((result) => {
          if (!active) return;
          const failure = result.basic.error ?? result.readiness.error;
          if (failure) throw new Error(failure.message);
          setOptions(result.config_options ?? []);
          setRejected(result.rejected_options ?? []);
        })
        .catch((reason) => {
          if (active)
            setError(reason instanceof Error ? reason.message : String(reason));
        })
        .finally(() => {
          if (active) setLoading(false);
        });
    }, 250);
    return () => {
      active = false;
      clearTimeout(timer);
    };
    // Serialized draft is the request identity; the callback is deliberately not a dependency.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [profileKey, revision]);

  function select(option: AcpConfigOption, value: string): void {
    const selections = { ...profile.session_options };
    if (value) selections[option.id] = value;
    else delete selections[option.id];
    onChange({ ...profile, session_options: selections });
  }

  return (
    <section
      className="acp-session-options"
      aria-label={zh ? "Agent 会话选项" : "Agent session options"}
    >
      <div className="acp-session-options__heading">
        <div>
          <h3>{zh ? "模型与运行选项" : "Model and runtime options"}</h3>
          <p className="settings-help">
            {zh
              ? context === "chat"
                ? "仅用于 Chat，自动记住；从下一轮消息生效，不影响自动审批。同一 Agent 的新对话沿用最近的 Chat 选择。"
                : "从所选 Agent 动态读取。未指定的选项使用 Agent 默认值；保存后用于审批。Chat 的选择独立保存。"
              : context === "chat"
                ? "Remembered for Chat only, starting next turn. Reviews are unchanged. New chats with this agent reuse your latest Chat choices."
                : "Read from the selected agent. Unspecified options follow agent defaults; saved choices apply to reviews. Chat preferences are separate."}
          </p>
        </div>
        <button
          type="button"
          disabled={disabled || loading}
          onClick={() => setRevision((v) => v + 1)}
        >
          {zh ? "刷新选项" : "Refresh options"}
        </button>
      </div>
      {loading ? (
        <p role="status">
          {zh ? "正在读取 Agent 配置…" : "Reading agent configuration…"}
        </p>
      ) : null}
      {error ? (
        <p role="alert" className="workspace-alert">
          {error}
        </p>
      ) : null}
      {rejected.length ? (
        <div className="workspace-alert" role="alert">
          {zh
            ? "Agent 已不支持这些已保存的选项："
            : "The agent no longer supports these saved options: "}
          {rejected.join(", ")}
          <button
            type="button"
            disabled={disabled}
            onClick={() => {
              const selections = { ...profile.session_options };
              rejected.forEach((id) => delete selections[id]);
              onChange({ ...profile, session_options: selections });
            }}
          >
            {zh ? "使用当前默认值" : "Use current defaults"}
          </button>
        </div>
      ) : null}
      {!loading && !error && !options.length ? (
        <p className="settings-help">
          {zh
            ? "该 Agent 未返回可配置选项。"
            : "This agent did not advertise configurable options."}
        </p>
      ) : null}
      <div className="settings-form-grid">
        {options.map((option) => {
          const selection = profile.session_options?.[option.id];
          const current = option.options.find(
            (value) => value.value === option.current_value,
          );
          const groups = [
            ...new Set(option.options.map((value) => value.group)),
          ];
          const renderOption = (value: AcpConfigOption["options"][number]) => (
            <option key={value.value} value={value.value}>
              {value.name}
            </option>
          );
          return (
            <label className="settings-field" key={option.id}>
              <span>{option.name}</span>
              <select
                disabled={disabled || loading}
                value={selection ?? ""}
                onChange={(event) => select(option, event.currentTarget.value)}
              >
                <option value="">
                  {zh ? "跟随 Agent" : "Follow agent"}
                  {selection
                    ? ""
                    : ` · ${current?.name ?? option.current_value}`}
                </option>
                {selection &&
                !option.options.some((value) => value.value === selection) ? (
                  <option value={selection} disabled>
                    {selection} ({zh ? "不可用" : "unavailable"})
                  </option>
                ) : null}
                {groups.map((group) =>
                  group ? (
                    <optgroup key={group} label={group}>
                      {option.options
                        .filter((value) => value.group === group)
                        .map(renderOption)}
                    </optgroup>
                  ) : (
                    option.options
                      .filter((value) => !value.group)
                      .map(renderOption)
                  ),
                )}
              </select>
              {option.description ? (
                <small className="settings-help">{option.description}</small>
              ) : null}
            </label>
          );
        })}
      </div>
    </section>
  );
}
