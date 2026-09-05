import { SecretInput } from "../SecretInput";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState, type JSX } from "react";

import type { Locale } from "../../i18n";
import {
  CollectionExposurePolicyEditor,
  FieldExposurePolicyEditor,
  ExposurePolicySummary,
  defaultExposurePolicy,
  exposurePolicyNeedsNetworkAllowlist,
  normalizeExposurePolicy,
  type CredentialExposurePolicy,
} from "../ExposurePolicy";
import { Dialog } from "./PagePrimitives";
import "./password-vault.css";

type PasswordEntry = {
  key: string;
  value: string;
};

type PasswordDraftPreview = {
  descriptor: {
    kind: string;
    path?: string;
    names?: string[];
    keys?: string[];
    account?: string | null;
    fields?: { key: string; reference: string }[];
  };
  entries: PasswordEntry[];
  suggested_item_title?: string | null;
  suggested_destination?:
    | { kind: "plankton"; vault_id: string }
    | { kind: "external"; binding_id: string; vault_id: string }
    | null;
  suggested_layout?: {
    description?: string | null;
    tags?: string[];
    field_labels?: Record<string, string>;
    field_resources?: Record<string, string>;
    default_exposure_policy?: CredentialExposurePolicy | null;
    field_exposure_policies?: Record<string, CredentialExposurePolicy>;
  } | null;
};

type PasswordDraftCommitReceipt = {
  draft_id: string;
  destination: string;
  resource_ids: string[];
};

type BackendConnection = {
  id: string;
  backend_kind: "local" | "one_password" | "bitwarden" | "custom";
  display_name: string;
  enabled: boolean;
  capabilities: string[];
  config?: Record<string, unknown>;
};
type VaultOption = { id: string; label: string; subtitle?: string | null };

type PasswordAddDialogProps = {
  draftId: string;
  locale: Locale;
  onClose: () => void;
  onCommitted: (draftId: string, receipt: PasswordDraftCommitReceipt) => void;
  onManageVaults?: () => void;
  vaultRevision?: number;
};

function defaultItemTitle(draft: PasswordDraftPreview, locale: Locale): string {
  const suggestedTitle = draft.suggested_item_title?.trim();
  if (suggestedTitle) {
    return suggestedTitle;
  }
  if (draft.descriptor.kind === "one_password")
    return locale === "zh-CN" ? "1Password 导入" : "1Password import";
  if (draft.descriptor.kind === "file") {
    const segments =
      draft.descriptor.path?.split(/[\\/]/).filter(Boolean) ?? [];
    const fileName = segments.at(-1);
    if (fileName === ".env") {
      const parentName = segments.at(-2);
      return parentName
        ? locale === "zh-CN"
          ? `${parentName} 环境变量`
          : `${parentName} environment`
        : locale === "zh-CN"
          ? "环境变量"
          : "Environment variables";
    }
    return fileName ?? (locale === "zh-CN" ? "导入文件" : "Imported file");
  }
  if (draft.descriptor.names?.length === 1) {
    return draft.descriptor.names[0];
  }
  return `Environment secrets (${draft.entries.length} fields)`;
}

function draftSourceSummary(
  draft: PasswordDraftPreview,
  locale: Locale,
): string {
  if (draft.descriptor.kind === "one_password") {
    return [
      draft.descriptor.account,
      ...(draft.descriptor.fields ?? []).map(
        (field) => `${field.key}: ${field.reference}`,
      ),
    ]
      .filter(Boolean)
      .join(" · ");
  }
  if (draft.descriptor.path) return draft.descriptor.path;
  const names = draft.descriptor.names ?? draft.descriptor.keys ?? [];
  if (names.length > 0) return names.join(", ");
  return locale === "zh-CN" ? "由 CLI 发起" : "Created from the CLI";
}

export function PasswordAddDialog(props: PasswordAddDialogProps): JSX.Element {
  const [preview, setPreview] = useState<PasswordDraftPreview | null>(null);
  const [connections, setConnections] = useState<BackendConnection[]>([]);
  const [destinationId, setDestinationId] = useState("plankton");
  const [vaultId, setVaultId] = useState("default");
  const [itemTitle, setItemTitle] = useState("");
  const [section, setSection] = useState("Credentials");
  const [tags, setTags] = useState("");
  const [description, setDescription] = useState("");
  const [fieldLabels, setFieldLabels] = useState<Record<string, string>>({});
  const [fieldResources, setFieldResources] = useState<Record<string, string>>(
    {},
  );
  const [fieldValues, setFieldValues] = useState<Record<string, string>>({});
  const [collectionExposurePolicy, setCollectionExposurePolicy] = useState(
    defaultExposurePolicy,
  );
  const [fieldExposurePolicies, setFieldExposurePolicies] = useState<
    Record<string, CredentialExposurePolicy>
  >({});
  const [vaultOptions, setVaultOptions] = useState<VaultOption[]>([]);
  const [preferredVaultId, setPreferredVaultId] = useState("");
  const [vaultsLoading, setVaultsLoading] = useState(false);
  const [phase, setPhase] = useState<"review" | "confirm">("review");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const confirmHeadingRef = useRef<HTMLHeadingElement | null>(null);
  const initialFieldRef = useRef<HTMLInputElement | null>(null);
  const currentDraftIdRef = useRef(props.draftId);
  const mountedRef = useRef(true);
  const zh = props.locale === "zh-CN";
  currentDraftIdRef.current = props.draftId;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    setPreview(null);
    setConnections([]);
    setDestinationId("plankton");
    setVaultId("default");
    setItemTitle("");
    setSection("Credentials");
    setTags("");
    setDescription("");
    setFieldLabels({});
    setFieldResources({});
    setFieldValues({});
    setFieldExposurePolicies({});
    setCollectionExposurePolicy(defaultExposurePolicy());
    setVaultOptions([]);
    setPreferredVaultId("");
    setVaultsLoading(false);
    setPhase("review");
    setSaving(false);
    setError(null);
    void Promise.all([
      invoke<PasswordDraftPreview>("preview_password_draft", {
        draftId: props.draftId,
      }),
      invoke<BackendConnection[]>("list_backend_connections"),
    ])
      .then(([draft, availableConnections]) => {
        if (!active) return;
        setPreview(draft);
        if (draft.suggested_destination) {
          setDestinationId(
            draft.suggested_destination.kind === "plankton"
              ? "plankton"
              : draft.suggested_destination.binding_id,
          );
          setPreferredVaultId(draft.suggested_destination.vault_id);
          setVaultId(draft.suggested_destination.vault_id);
        }
        setItemTitle(defaultItemTitle(draft, props.locale));
        setDescription(draft.suggested_layout?.description ?? "");
        setTags((draft.suggested_layout?.tags ?? []).join(", "));
        setFieldLabels(
          Object.fromEntries(
            draft.entries.map((entry) => [
              entry.key,
              draft.suggested_layout?.field_labels?.[entry.key] ?? entry.key,
            ]),
          ),
        );
        setFieldResources(draft.suggested_layout?.field_resources ?? {});
        setCollectionExposurePolicy(
          normalizeExposurePolicy(
            draft.suggested_layout?.default_exposure_policy,
          ),
        );
        setFieldExposurePolicies(
          Object.fromEntries(
            Object.entries(
              draft.suggested_layout?.field_exposure_policies ?? {},
            ).map(([key, policy]) => [key, normalizeExposurePolicy(policy)]),
          ),
        );
        setFieldValues(
          Object.fromEntries(
            draft.entries.map((entry) => [
              entry.key,
              draft.descriptor.kind === "manual" ? "" : entry.value,
            ]),
          ),
        );
        setConnections(
          availableConnections.filter(
            (connection) =>
              connection.enabled &&
              connection.id !== "plankton" &&
              connection.capabilities.includes("create"),
          ),
        );
      })
      .catch(() => {
        if (active) {
          setError(
            zh
              ? "无法加载密码草稿。请查看诊断信息后重试。"
              : "Password draft could not be loaded. Check Diagnostics and try again.",
          );
        }
      });
    return () => {
      active = false;
    };
  }, [props.draftId, props.locale]);

  useEffect(() => {
    const connection = connections.find((entry) => entry.id === destinationId);
    if (!connection && destinationId !== "plankton") {
      setVaultOptions([]);
      return;
    }
    let active = true;
    setVaultsLoading(true);
    setError(null);
    const request =
      destinationId === "plankton"
        ? invoke<VaultOption[]>("list_local_vaults")
        : connection?.backend_kind === "one_password"
          ? invoke<VaultOption[]>("list_onepassword_vaults_command", {
              accountId: String(connection.config?.account ?? ""),
            })
          : connection?.backend_kind === "bitwarden"
            ? invoke<Array<VaultOption & { kind?: string }>>(
                "list_bitwarden_containers_command",
              )
            : Promise.resolve([]);
    void request
      .then((options) => {
        if (!active) return;
        setVaultOptions(options);
        if (options.length > 0) {
          setVaultId(
            options.some((option) => option.id === preferredVaultId)
              ? preferredVaultId
              : options[0].id,
          );
        }
      })
      .catch(() => {
        if (active) {
          setVaultOptions([]);
          setError(
            zh
              ? "无法加载目标保险库。请查看诊断信息后重试。"
              : "Destination vaults could not be loaded. Check Diagnostics and try again.",
          );
        }
      })
      .finally(() => {
        if (active) setVaultsLoading(false);
      });
    return () => {
      active = false;
    };
  }, [connections, destinationId, preferredVaultId, props.vaultRevision]);

  useEffect(() => {
    if (preview && phase === "review") {
      initialFieldRef.current?.focus();
    } else if (preview && phase === "confirm") {
      confirmHeadingRef.current?.focus();
    }
  }, [phase, preview]);

  async function confirm(): Promise<void> {
    const committingDraftId = props.draftId;
    setSaving(true);
    setError(null);
    let receipt: PasswordDraftCommitReceipt;
    try {
      receipt = await invoke<PasswordDraftCommitReceipt>(
        "confirm_password_draft",
        {
          draftId: props.draftId,
          destination:
            destinationId === "plankton"
              ? {
                  kind: "plankton",
                  vault_id: vaultId.trim() || "default",
                }
              : {
                  kind: "external",
                  binding_id: destinationId,
                  vault_id: vaultId.trim() || "default",
                },
          layout: {
            item_title: itemTitle.trim(),
            section: section.trim(),
            tags: tags
              .split(",")
              .map((tag) => tag.trim())
              .filter(Boolean),
            description: description.trim() || null,
            field_labels: fieldLabels,
            field_resources: fieldResources,
            default_exposure_policy: collectionExposurePolicy,
            field_exposure_policies: fieldExposurePolicies,
          },
          values: fieldValues,
        },
      );
    } catch {
      if (
        mountedRef.current &&
        currentDraftIdRef.current === committingDraftId
      ) {
        setError(
          zh
            ? "无法保存密码草稿。请查看诊断信息后重试。"
            : "Password draft could not be saved. Check Diagnostics and try again.",
        );
        setSaving(false);
      }
      return;
    }
    if (mountedRef.current && currentDraftIdRef.current === committingDraftId) {
      setSaving(false);
    }
    props.onCommitted(committingDraftId, receipt);
  }

  const footer = (
    <div className="password-add-dialog-footer-actions">
      <button
        className="ghost"
        disabled={saving}
        onClick={props.onClose}
        type="button"
      >
        {zh ? "取消" : "Cancel"}
      </button>
      {phase === "review" ? (
        <button
          className="primary"
          disabled={
            !preview ||
            preview.entries.length === 0 ||
            !itemTitle.trim() ||
            !section.trim() ||
            preview.entries.some(
              (entry) => (fieldValues[entry.key] ?? "").length === 0,
            ) ||
            exposurePolicyNeedsNetworkAllowlist(collectionExposurePolicy) ||
            Object.values(fieldExposurePolicies).some(
              exposurePolicyNeedsNetworkAllowlist,
            )
          }
          onClick={() => setPhase("confirm")}
          type="button"
        >
          {zh ? "下一步：确认保存" : "Next: review and save"}
        </button>
      ) : (
        <>
          <button
            className="ghost"
            disabled={saving}
            onClick={() => setPhase("review")}
            type="button"
          >
            {zh ? "返回" : "Back"}
          </button>
          <button
            className="primary"
            disabled={saving}
            onClick={() => void confirm()}
            type="button"
          >
            {saving
              ? zh
                ? "正在写入…"
                : "Saving…"
              : zh
                ? "确认并保存"
                : "Confirm and save"}
          </button>
        </>
      )}
    </div>
  );

  return (
    <Dialog
      closeDisabled={saving}
      closeLabel={zh ? "关闭保存密码草稿对话框" : "Close password draft dialog"}
      description={
        phase === "review" && preview?.descriptor.kind === "one_password"
          ? zh
            ? "从 1Password 读取的字段可在此编辑，确认后保存到所选保险库。"
            : "Review and edit the fields read from 1Password, then confirm to save them to your chosen vault."
          : phase === "review"
            ? zh
              ? "检查名称、密码和保存位置，下一步确认后保存。"
              : "Check the name, passwords, and destination before saving."
            : zh
              ? "确认以下内容后，密码才会保存到保险库。"
              : "Your passwords will be saved only after you confirm below."
      }
      footer={footer}
      onClose={() => {
        if (!saving) props.onClose();
      }}
      open
      title={
        phase === "review"
          ? preview?.descriptor.kind === "one_password"
            ? zh
              ? "从 1Password 导入"
              : "Import from 1Password"
            : zh
              ? "添加密码"
              : "Add passwords"
          : zh
            ? "确认保存"
            : "Review and save"
      }
    >
      <div className="password-add-dialog-content">
        {error ? (
          <p className="workspace-alert dialog-error" role="alert">
            {error}
          </p>
        ) : null}

        {!preview ? (
          <section className="confirm-pane">
            <p>{zh ? "正在读取草稿…" : "Loading draft…"}</p>
          </section>
        ) : phase === "review" ? (
          <div className="dialog-grid password-add-review-grid">
            <section className="password-add-item-structure">
              <h3>{zh ? "名称与密码" : "Name and passwords"}</h3>
              <label>
                {zh ? "条目标题" : "Item title"}
                <input
                  data-dialog-initial-focus="true"
                  onChange={(event) => setItemTitle(event.currentTarget.value)}
                  ref={initialFieldRef}
                  value={itemTitle}
                />
              </label>
              <CollectionExposurePolicyEditor
                locale={props.locale}
                value={collectionExposurePolicy}
                onChange={setCollectionExposurePolicy}
              />
              <div className="password-add-fields-heading">
                <span>
                  {zh
                    ? `${preview.entries.length} 个密码字段`
                    : `${preview.entries.length} password fields`}
                </span>
              </div>
              <dl className="password-add-field-list">
                {preview.entries.map((entry) => (
                  <div className="password-add-field-row" key={entry.key}>
                    <dt className="password-add-field-identity">
                      <label htmlFor={`field-label-${entry.key}`}>
                        {zh ? "字段名称" : "Field name"}
                      </label>
                      <input
                        aria-label={`${entry.key} field label`}
                        id={`field-label-${entry.key}`}
                        onChange={(event) =>
                          setFieldLabels((current) => ({
                            ...current,
                            [entry.key]: event.currentTarget.value,
                          }))
                        }
                        value={fieldLabels[entry.key] ?? entry.key}
                      />
                    </dt>
                    <dd className="password-add-field-value">
                      <SecretInput
                        key={`${props.draftId}:${entry.key}`}
                        aria-label={`${entry.key} password`}
                        fieldName={fieldLabels[entry.key] ?? entry.key}
                        locale={props.locale}
                        autoComplete="new-password"
                        autoReveal={
                          (
                            fieldExposurePolicies[entry.key] ??
                            collectionExposurePolicy
                          ).access_mode === "direct"
                        }
                        onChange={(event) => {
                          const value = event.currentTarget.value;
                          setFieldValues((current) => ({
                            ...current,
                            [entry.key]: value,
                          }));
                        }}
                        placeholder={
                          zh
                            ? "由你填写，不会回传 CLI"
                            : "Enter locally; never returned to CLI"
                        }
                        value={fieldValues[entry.key] ?? ""}
                      />
                    </dd>
                    <dd className="password-add-field-exposure">
                      <details>
                        <summary>
                          {zh
                            ? "使用权限（高级）"
                            : "Usage permissions (advanced)"}{" "}
                          ·{" "}
                          {fieldExposurePolicies[entry.key]
                            ? zh
                              ? "自定义"
                              : "Custom"
                            : zh
                              ? "继承默认"
                              : "Inherited"}{" "}
                          ·{" "}
                          {((
                            fieldExposurePolicies[entry.key] ??
                            collectionExposurePolicy
                          ).access_mode ?? "protected") === "direct"
                            ? zh
                              ? "直接可见 · 无需审批"
                              : "Direct · no approval"
                            : zh
                              ? "受保护"
                              : "Protected"}
                        </summary>
                        <FieldExposurePolicyEditor
                          defaultPolicy={collectionExposurePolicy}
                          locale={props.locale}
                          onChange={(policy) =>
                            setFieldExposurePolicies((current) => {
                              const next = { ...current };
                              if (policy) next[entry.key] = policy;
                              else delete next[entry.key];
                              return next;
                            })
                          }
                          customPolicy={fieldExposurePolicies[entry.key]}
                        />
                      </details>
                    </dd>
                  </div>
                ))}
              </dl>
            </section>
            <section className="password-add-destination">
              <h3>{zh ? "保存位置" : "Destination"}</h3>
              <label>
                {zh ? "保存到" : "Save to"}
                <select
                  onChange={(event) =>
                    setDestinationId(event.currentTarget.value)
                  }
                  value={destinationId}
                >
                  <option value="plankton">
                    {zh
                      ? "Plankton（本机加密保存）"
                      : "Plankton (encrypted on this device)"}
                  </option>
                  {connections.map((connection) => (
                    <option key={connection.id} value={connection.id}>
                      {connection.display_name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                {zh ? "保险库" : "Vault"}
                {vaultOptions.length > 0 ? (
                  <select
                    disabled={vaultsLoading}
                    onChange={(event) => setVaultId(event.currentTarget.value)}
                    value={vaultId}
                  >
                    {vaultOptions.map((option) => (
                      <option key={option.id} value={option.id}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    disabled={vaultsLoading}
                    onChange={(event) => setVaultId(event.currentTarget.value)}
                    value={vaultId}
                  />
                )}
              </label>
              {destinationId === "plankton" && props.onManageVaults ? (
                <button
                  className="ghost"
                  onClick={props.onManageVaults}
                  type="button"
                >
                  {zh
                    ? "新建或删除本地保险库"
                    : "Create or delete local vaults"}
                </button>
              ) : null}
            </section>
            <details className="password-add-optional">
              <summary>
                {zh ? "标签与备注（可选）" : "Tags and notes (optional)"}
              </summary>
              <div className="password-add-optional-fields">
                <label>
                  {zh ? "分区" : "Section"}
                  <input
                    onChange={(event) => setSection(event.currentTarget.value)}
                    value={section}
                  />
                </label>
                <label>
                  {zh ? "标签（逗号分隔）" : "Tags (comma-separated)"}
                  <input
                    onChange={(event) => setTags(event.currentTarget.value)}
                    value={tags}
                  />
                </label>
                <label>
                  {zh ? "备注" : "Description"}
                  <textarea
                    onChange={(event) =>
                      setDescription(event.currentTarget.value)
                    }
                    rows={2}
                    value={description}
                  />
                </label>
              </div>
            </details>
            <details className="password-add-source-context">
              <summary>{zh ? "查看来源信息" : "View source details"}</summary>
              <div>
                <strong>
                  CLI ·{" "}
                  {preview.descriptor.kind === "one_password"
                    ? "1Password"
                    : preview.descriptor.kind}
                </strong>
                <span>{draftSourceSummary(preview, props.locale)}</span>
              </div>
              <details>
                <summary>{zh ? "草稿标识" : "Draft ID"}</summary>
                <code>{props.draftId}</code>
              </details>
            </details>
          </div>
        ) : (
          <section className="confirm-pane">
            <p className="eyebrow">{zh ? "最终确认" : "FINAL CONFIRMATION"}</p>
            <h3
              aria-live="polite"
              data-testid="password-final-confirmation-heading"
              ref={confirmHeadingRef}
              tabIndex={-1}
            >
              {preview.entries.length} {zh ? "个密码字段" : "password fields"}
            </h3>
            <p>
              {zh
                ? `条目「${itemTitle}」/ 分区「${section}」${tags.trim() ? ` / 标签 ${tags}` : ""}`
                : `Item “${itemTitle}” / section “${section}”${tags.trim() ? ` / tags ${tags}` : ""}`}
            </p>
            <p>
              {zh ? "保存到：" : "Save to: "}
              <strong>
                {destinationId === "plankton"
                  ? "Plankton"
                  : connections.find(
                      (connection) => connection.id === destinationId,
                    )?.display_name || destinationId}
              </strong>
              {" / "}
              {vaultOptions.find((option) => option.id === vaultId)?.label ||
                vaultId ||
                "default"}
            </p>
            <ExposurePolicySummary
              compact
              locale={props.locale}
              value={collectionExposurePolicy}
            />
            <div className="password-add-policy-review">
              {preview.entries.map((entry) => (
                <section key={entry.key}>
                  <h4>{fieldLabels[entry.key] ?? entry.key}</h4>
                  <small>
                    {fieldExposurePolicies[entry.key]
                      ? zh
                        ? "自定义"
                        : "Custom"
                      : zh
                        ? "继承默认"
                        : "Inherit defaults"}
                  </small>
                  <p>
                    {(
                      fieldExposurePolicies[entry.key] ??
                      collectionExposurePolicy
                    ).access_mode === "direct"
                      ? zh
                        ? "直接可见 · 无需审批"
                        : "Direct access · no approval"
                      : zh
                        ? "受保护 · 按使用权限审批"
                        : "Protected · usage reviewed"}
                  </p>
                  <details>
                    <summary>
                      {zh ? "查看使用权限" : "View usage permissions"}
                    </summary>
                    <ExposurePolicySummary
                      compact
                      locale={props.locale}
                      value={
                        fieldExposurePolicies[entry.key] ??
                        collectionExposurePolicy
                      }
                    />
                  </details>
                </section>
              ))}
            </div>
          </section>
        )}
      </div>
    </Dialog>
  );
}
