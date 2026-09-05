import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState, type JSX } from "react";

import type { Locale } from "../../i18n";
import { Dialog } from "./PagePrimitives";

type VaultOption = { id: string; label: string; subtitle?: string | null };
type BackendConnection = {
  id: string;
  backend_kind: "local" | "one_password" | "bitwarden" | "custom";
  display_name: string;
  enabled: boolean;
  capabilities: string[];
  config?: Record<string, unknown>;
};

export type MigratablePasswordItem = {
  record_id: string;
  item_id: string;
  title: string;
  fields: Array<{
    resource_id: string;
    provider_kind: string;
    vault?: string | null;
  }>;
};

type MigrationReceipt = {
  migration_id: string;
  mode: "copy" | "move";
  destination: string;
  resource_ids: string[];
  source_deleted: boolean;
};

type Props = {
  catalogRevision: string;
  item: MigratablePasswordItem;
  locale: Locale;
  initialBackend?: string;
  initialMode?: "copy" | "move";
  initialVault?: string;
  onClose: () => void;
  onCompleted: (receipt: MigrationReceipt) => void;
  onManageVaults?: () => void;
  vaultRevision?: number;
};

function message(locale: Locale, en: string, zh: string): string {
  return locale === "zh-CN" ? zh : en;
}

export function PasswordMigrationDialog(props: Props): JSX.Element {
  const [connections, setConnections] = useState<BackendConnection[]>([]);
  const [destinationId, setDestinationId] = useState(
    props.initialBackend || "plankton",
  );
  const [vaultId, setVaultId] = useState(props.initialVault || "");
  const [vaults, setVaults] = useState<VaultOption[]>([]);
  const [mode, setMode] = useState<"copy" | "move">(
    props.initialMode === "move" &&
      props.item.fields.every(
        (field) =>
          field.provider_kind !== "dotenv_file" &&
          field.provider_kind !== "literal",
      )
      ? "move"
      : "copy",
  );
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const moveSupported = props.item.fields.every(
    (field) =>
      field.provider_kind !== "dotenv_file" &&
      field.provider_kind !== "literal",
  );
  const destination = useMemo(
    () => connections.find((entry) => entry.id === destinationId),
    [connections, destinationId],
  );

  useEffect(() => {
    let active = true;
    void invoke<BackendConnection[]>("list_backend_connections")
      .then((available) => {
        if (!active) return;
        setConnections(
          available.filter(
            (entry) =>
              entry.enabled &&
              entry.id !== "plankton" &&
              entry.capabilities.includes("create") &&
              (entry.backend_kind === "one_password" ||
                entry.backend_kind === "bitwarden"),
          ),
        );
      })
      .catch((reason: unknown) => {
        if (active)
          setError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError(null);
    const request =
      destinationId === "plankton"
        ? invoke<VaultOption[]>("list_local_vaults")
        : destination?.backend_kind === "one_password"
          ? invoke<VaultOption[]>("list_onepassword_vaults_command", {
              accountId: String(destination.config?.account ?? ""),
            })
          : destination?.backend_kind === "bitwarden"
            ? invoke<VaultOption[]>("list_bitwarden_containers_command")
            : Promise.resolve([]);
    void request
      .then((options) => {
        if (!active) return;
        setVaults(options);
        setVaultId(
          options.some((option) => option.id === props.initialVault)
            ? (props.initialVault ?? "")
            : (options[0]?.id ?? ""),
        );
      })
      .catch((reason: unknown) => {
        if (!active) return;
        setVaults([]);
        setVaultId("");
        setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [destination, destinationId, props.initialVault, props.vaultRevision]);

  async function migrate(): Promise<void> {
    setSaving(true);
    setError(null);
    try {
      const receipt = await invoke<MigrationReceipt>("migrate_password_item", {
        request: {
          source_record_id: props.item.record_id,
          expected_revision: props.catalogRevision,
          destination:
            destinationId === "plankton"
              ? { kind: "plankton", vault_id: vaultId }
              : {
                  kind: "external",
                  binding_id: destinationId,
                  vault_id: vaultId,
                },
          mode,
        },
      });
      props.onCompleted(receipt);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setSaving(false);
    }
  }

  return (
    <Dialog
      closeDisabled={saving}
      description={message(
        props.locale,
        "The target is written and verified before the source can be removed.",
        "先写入并校验目标；只有校验通过后才允许移除源项。",
      )}
      footer={
        <div className="password-migration-actions">
          <button disabled={saving} onClick={props.onClose} type="button">
            {message(props.locale, "Cancel", "取消")}
          </button>
          <button
            className="primary"
            disabled={saving || loading || !vaultId}
            onClick={() => void migrate()}
            type="button"
          >
            {saving
              ? message(props.locale, "Verifying…", "正在写入并校验…")
              : mode === "move"
                ? message(props.locale, "Verify and move", "校验并迁移")
                : message(props.locale, "Verify and copy", "校验并复制")}
          </button>
        </div>
      }
      onClose={() => {
        if (!saving) props.onClose();
      }}
      open
      title={message(
        props.locale,
        `Move “${props.item.title}”`,
        `迁移「${props.item.title}」`,
      )}
    >
      <div className="password-migration-dialog">
        {error ? (
          <p className="workspace-alert dialog-error" role="alert">
            {error}
          </p>
        ) : null}
        <section
          className="password-migration-route"
          aria-label={message(props.locale, "Migration route", "迁移路径")}
        >
          <div>
            <span>{message(props.locale, "Source", "源保险库")}</span>
            <strong>
              {props.item.fields[0]?.vault ||
                message(props.locale, "Catalog", "目录")}
            </strong>
          </div>
          <span aria-hidden="true" className="password-migration-arrow">
            →
          </span>
          <div>
            <span>{message(props.locale, "Destination", "目标保险库")}</span>
            <strong>
              {vaults.find((vault) => vault.id === vaultId)?.label || "—"}
            </strong>
          </div>
        </section>
        <label>
          {message(props.locale, "Backend", "目标后端")}
          <select
            data-dialog-initial-focus="true"
            disabled={saving}
            onChange={(event) => setDestinationId(event.currentTarget.value)}
            value={destinationId}
          >
            <option value="plankton">Plankton · KDBX4</option>
            {connections.map((connection) => (
              <option key={connection.id} value={connection.id}>
                {connection.display_name}
              </option>
            ))}
          </select>
        </label>
        {destinationId === "plankton" && props.onManageVaults ? (
          <button
            className="ghost"
            onClick={props.onManageVaults}
            type="button"
          >
            {message(
              props.locale,
              "Create or delete local vaults",
              "新建或删除本地保险库",
            )}
          </button>
        ) : null}
        <label>
          {message(props.locale, "Vault", "目标保险库")}
          <select
            disabled={saving || loading}
            onChange={(event) => setVaultId(event.currentTarget.value)}
            value={vaultId}
          >
            {vaults.map((vault) => (
              <option key={vault.id} value={vault.id}>
                {vault.label}
                {vault.subtitle ? ` · ${vault.subtitle}` : ""}
              </option>
            ))}
          </select>
        </label>
        <fieldset className="password-migration-mode">
          <legend>
            {message(props.locale, "After verification", "校验通过后")}
          </legend>
          <label>
            <input
              checked={mode === "copy"}
              disabled={saving}
              name="migration-mode"
              onChange={() => setMode("copy")}
              type="radio"
            />
            <span>
              <strong>
                {message(props.locale, "Keep source", "保留源项")}
              </strong>
              <small>
                {message(
                  props.locale,
                  "Create a verified copy.",
                  "创建一份经回读校验的副本。",
                )}
              </small>
            </span>
          </label>
          <label>
            <input
              checked={mode === "move"}
              disabled={saving || !moveSupported}
              name="migration-mode"
              onChange={() => setMode("move")}
              type="radio"
            />
            <span>
              <strong>
                {message(props.locale, "Remove source", "移除源项")}
              </strong>
              <small>
                {moveSupported
                  ? message(
                      props.locale,
                      "Provider items go to recovery trash.",
                      "外部后端源项进入可恢复回收站。",
                    )
                  : message(
                      props.locale,
                      "Unavailable for file or literal sources.",
                      "文件或字面量来源不可自动移除。",
                    )}
              </small>
            </span>
          </label>
        </fieldset>
      </div>
    </Dialog>
  );
}
