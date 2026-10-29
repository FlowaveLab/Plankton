import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState, type JSX } from "react";

import type { AccessRequest, RememberExposureOptions } from "../types";
import {
  defaultExposurePolicy,
  ExposurePolicyEditor,
  exposurePolicyNeedsNetworkAllowlist,
  parseExposurePolicy,
  type CredentialExposurePolicy,
} from "./ExposurePolicy";
import "./remember-exposure.css";

export function RememberExposure(props: {
  request: Pick<AccessRequest, "id" | "policy_mode"> & {
    context: Pick<AccessRequest["context"], "resource_metadata">;
    llm_suggestion?: Pick<
      NonNullable<AccessRequest["llm_suggestion"]>,
      "exposure_report"
    > | null;
  };
  zh: boolean;
  busy: boolean;
  onApprove: (options: RememberExposureOptions) => Promise<void>;
}): JSX.Element {
  const { request, zh, busy, onApprove } = props;
  const snapshot =
    parseExposurePolicy(
      request.context.resource_metadata?.credential_exposure_policy_v1,
    ) ?? defaultExposurePolicy();
  const [open, setOpen] = useState(false);
  const [original, setOriginal] = useState(snapshot);
  const [policy, setPolicy] = useState(snapshot);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const loaded = useRef(false);
  const generation = useRef(0);
  const reloadPolicy = useCallback(async () => {
    if (
      !(window as Window & { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__
    )
      return;
    const version = ++generation.current;
    setLoading(true);
    setLoadError(null);
    try {
      const current = await invoke<CredentialExposurePolicy>(
        "request_current_exposure_policy",
        { requestId: request.id },
      );
      if (version !== generation.current) return;
      setOriginal(current);
      setPolicy(current);
      loaded.current = true;
    } catch (reason) {
      if (version === generation.current)
        setLoadError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (version === generation.current) setLoading(false);
    }
  }, [request.id]);
  useEffect(() => {
    if (open && !loaded.current) void reloadPolicy();
  }, [open, reloadPolicy]);
  useEffect(
    () => () => {
      generation.current += 1;
    },
    [],
  );
  const missingAllowlist = exposurePolicyNeedsNetworkAllowlist(policy);
  const changed = JSON.stringify(policy) !== JSON.stringify(original);
  const unknownEvidence =
    request.llm_suggestion?.exposure_report?.surfaces.some(
      (surface) => surface.evidence_state === "unknown",
    );

  return (
    <section className="remember-exposure">
      <button
        aria-expanded={open}
        aria-controls={`remember-exposure-${request.id}`}
        className="remember-exposure-toggle"
        disabled={busy}
        onClick={() => setOpen(!open)}
        type="button"
      >
        <span>{zh ? "调整并记住暴露域" : "Adjust and remember exposure"}</span>
        <span aria-hidden="true">{open ? "−" : "+"}</span>
      </button>
      {open ? (
        <div
          id={`remember-exposure-${request.id}`}
          className="remember-exposure-body"
        >
          <p>
            {zh
              ? "为此凭据保存允许范围。自动模式下，后续证据完整且范围内的请求将自动处理。"
              : "Save the allowed scope for this credential. Automatic mode handles later requests with complete evidence inside this scope."}
          </p>
          {request.policy_mode !== "llm_automatic" ? (
            <p className="remember-exposure-notice" role="status">
              {zh
                ? "当前使用人工或辅助审批模式，保存范围后仍需按该模式审批。"
                : "Human or assisted review is active. Saving a scope keeps that review mode in place."}
            </p>
          ) : null}
          {unknownEvidence ? (
            <p className="remember-exposure-notice" role="status">
              {zh
                ? "本次仍有未确认的暴露证据。记住范围不会补齐缺失证据，后续同样缺失时仍会请求审批。"
                : "Some exposure evidence is unknown. Remembering a scope does not fill evidence gaps; later incomplete requests still need review."}
            </p>
          ) : null}
          {loading ? (
            <p role="status">
              {zh ? "正在读取当前暴露域…" : "Loading current exposure policy…"}
            </p>
          ) : null}
          {loadError ? <p role="alert">{loadError}</p> : null}
          <button
            className="remember-exposure-reload"
            type="button"
            disabled={busy || loading}
            onClick={() => void reloadPolicy()}
          >
            {zh ? "重新加载当前范围" : "Reload current scope"}
          </button>
          <fieldset disabled={busy || loading || loadError !== null}>
            <ExposurePolicyEditor
              compact
              protectedOnly
              locale={zh ? "zh-CN" : "en"}
              onChange={setPolicy}
              value={policy}
            />
          </fieldset>
          {missingAllowlist ? (
            <p className="remember-exposure-notice" role="alert">
              {zh
                ? "受控网络使用需要填写允许的域名。"
                : "Controlled network use needs an allowed domain."}
            </p>
          ) : null}
          <button
            className="primary"
            disabled={
              busy ||
              loading ||
              loadError !== null ||
              !changed ||
              missingAllowlist ||
              policy.access_mode !== "protected"
            }
            onClick={() =>
              void onApprove({
                exposurePolicy: policy,
                expectedExposurePolicy: original,
              })
            }
            type="button"
          >
            {busy
              ? zh
                ? "保存并批准中…"
                : "Saving and approving…"
              : zh
                ? "通过并记住暴露域"
                : "Approve and remember exposure"}
          </button>
        </div>
      ) : null}
    </section>
  );
}
