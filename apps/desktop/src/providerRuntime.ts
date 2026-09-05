import type { ProviderTrace } from "./types";

export type ProviderRuntimeState =
  | "active"
  | "configured_not_called"
  | "configured_pending"
  | "configured_overridden"
  | "not_called"
  | "unavailable";

export type ProviderRuntimeSummary = {
  configuredProviderKind: string | null;
  actualProviderKind: string | null;
  traceProviderKind: string | null;
  traceAvailable: boolean;
  state: ProviderRuntimeState;
};

type ProviderRuntimeInput = {
  configuredProviderKind: string | null;
  actualProviderKind: string | null;
  providerCalled: boolean | null;
  providerTrace: ProviderTrace | null;
};

export function deriveTraceProviderKind(
  providerKind: string | null,
  providerTrace: ProviderTrace | null,
): string | null {
  if (providerKind) {
    return providerKind === "acp_codex" ? "acp" : providerKind;
  }

  if (!providerTrace) {
    return null;
  }

  if (
    providerTrace.package_name === "@zed-industries/codex-acp" ||
    providerTrace.transport === "stdio"
  ) {
    return "acp";
  }

  if (providerTrace.protocol === "anthropic_messages") {
    return "claude";
  }

  return null;
}

export function buildProviderRuntimeSummary(
  input: ProviderRuntimeInput,
): ProviderRuntimeSummary {
  const traceProviderKind = deriveTraceProviderKind(
    input.actualProviderKind,
    input.providerTrace,
  );
  const actualProviderKind = input.actualProviderKind ?? traceProviderKind;
  const configuredProviderKind = input.configuredProviderKind;

  if (
    configuredProviderKind &&
    actualProviderKind &&
    configuredProviderKind !== actualProviderKind
  ) {
    return {
      configuredProviderKind,
      actualProviderKind,
      traceProviderKind,
      traceAvailable: input.providerTrace !== null,
      state: "configured_overridden",
    };
  }

  if (configuredProviderKind && input.providerCalled === false) {
    return {
      configuredProviderKind,
      actualProviderKind,
      traceProviderKind,
      traceAvailable: input.providerTrace !== null,
      state: "configured_not_called",
    };
  }

  if (configuredProviderKind && !actualProviderKind) {
    return {
      configuredProviderKind,
      actualProviderKind,
      traceProviderKind,
      traceAvailable: input.providerTrace !== null,
      state: "configured_pending",
    };
  }

  if (actualProviderKind) {
    return {
      configuredProviderKind,
      actualProviderKind,
      traceProviderKind,
      traceAvailable: input.providerTrace !== null,
      state: "active",
    };
  }

  if (input.providerCalled === false) {
    return {
      configuredProviderKind,
      actualProviderKind,
      traceProviderKind,
      traceAvailable: input.providerTrace !== null,
      state: "not_called",
    };
  }

  return {
    configuredProviderKind,
    actualProviderKind,
    traceProviderKind,
    traceAvailable: input.providerTrace !== null,
    state: "unavailable",
  };
}
