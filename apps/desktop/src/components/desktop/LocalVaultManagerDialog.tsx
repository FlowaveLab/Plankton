import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState, type JSX } from "react";

import type { Locale } from "../../i18n";
import { Dialog } from "./PagePrimitives";

type LocalVault = {
  id: string;
  file_name: string;
  unlock_file_name: string;
  label: string;
  subtitle: string;
  exists: boolean;
  unlock_file_exists: boolean;
};

type DeletionPreview = {
  vault_id: string;
  item_count: number;
  field_count: number;
};

type Props = {
  locale: Locale;
  onChanged: () => void;
  onClose: () => void;
};

function copy(locale: Locale, en: string, zh: string): string {
  return locale === "zh-CN" ? zh : en;
}

export function LocalVaultManagerDialog(props: Props): JSX.Element {
  const [vaults, setVaults] = useState<LocalVault[]>([]);
  const [newName, setNewName] = useState("");
  const [deletePreview, setDeletePreview] = useState<DeletionPreview | null>(
    null,
  );
  const [confirmation, setConfirmation] = useState("");
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (): Promise<void> => {
    try {
      setVaults(await invoke<LocalVault[]>("list_local_vaults"));
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function createVault(): Promise<void> {
    setWorking(true);
    setError(null);
    try {
      await invoke("create_local_vault", { vaultId: newName.trim() });
      setNewName("");
      await load();
      props.onChanged();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWorking(false);
    }
  }

  async function inspectDeletion(vaultId: string): Promise<void> {
    setWorking(true);
    setError(null);
    try {
      setDeletePreview(
        await invoke<DeletionPreview>("preview_local_vault_deletion", {
          vaultId,
        }),
      );
      setConfirmation("");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWorking(false);
    }
  }

  async function chooseUnlockFile(vaultId: string): Promise<void> {
    setWorking(true);
    setError(null);
    try {
      const selected = await invoke<LocalVault | null>(
        "pick_local_vault_unlock_file",
        { vaultId },
      );
      if (selected) {
        await load();
        props.onChanged();
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWorking(false);
    }
  }

  async function revealUnlockFile(vaultId: string): Promise<void> {
    setWorking(true);
    setError(null);
    try {
      await invoke("reveal_local_vault_unlock_file", { vaultId });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWorking(false);
    }
  }

  async function deleteVault(): Promise<void> {
    if (!deletePreview) return;
    setWorking(true);
    setError(null);
    try {
      await invoke("delete_local_vault", {
        vaultId: deletePreview.vault_id,
        confirmation,
      });
      setDeletePreview(null);
      setConfirmation("");
      await load();
      props.onChanged();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWorking(false);
    }
  }

  return (
    <Dialog
      closeDisabled={working}
      description={copy(
        props.locale,
        "Create encrypted local vaults or remove an existing vault after typed confirmation.",
        "创建本地加密保险库，或在输入名称确认后移除已有保险库。",
      )}
      footer={
        <button disabled={working} onClick={props.onClose} type="button">
          {copy(props.locale, "Done", "完成")}
        </button>
      }
      onClose={() => {
        if (!working) props.onClose();
      }}
      open
      title={copy(props.locale, "Manage local vaults", "管理本地保险库")}
    >
      <div className="local-vault-manager">
        {error ? (
          <p className="workspace-alert dialog-error" role="alert">
            {error}
          </p>
        ) : null}
        <section className="local-vault-create">
          <label>
            <span>{copy(props.locale, "New vault name", "新保险库名称")}</span>
            <input
              autoComplete="off"
              data-dialog-initial-focus="true"
              disabled={working}
              onChange={(event) => setNewName(event.currentTarget.value)}
              placeholder="work"
              value={newName}
            />
          </label>
          <button
            className="primary"
            disabled={working || !newName.trim()}
            onClick={() => void createVault()}
            type="button"
          >
            {copy(props.locale, "Create vault", "创建保险库")}
          </button>
          <small>
            {copy(
              props.locale,
              "Use letters, numbers, dots, underscores, or hyphens.",
              "可使用字母、数字、点、下划线或连字符。",
            )}
          </small>
        </section>
        <section>
          <h3>{copy(props.locale, "Existing vaults", "已有保险库")}</h3>
          <div className="local-vault-transfer-note" role="note">
            <strong>
              {copy(
                props.locale,
                "The unlock file is never synchronized",
                "unlock 文件永远不会参与同步",
              )}
            </strong>
            <p>
              {copy(
                props.locale,
                "To use a vault on another computer, transfer its unlock file separately through a secure channel. Never commit it to Git or send it through chat or ordinary email.",
                "如需在另一台电脑使用保险库，请通过安全渠道单独传输 unlock 文件。不要将它提交到 Git，也不要通过聊天或普通邮件发送。",
              )}
            </p>
          </div>
          {vaults.length === 0 ? (
            <p className="empty">
              {copy(props.locale, "No local vaults yet.", "尚无本地保险库。")}
            </p>
          ) : (
            <ul className="local-vault-list">
              {vaults.map((vault) => (
                <li key={vault.id}>
                  <span>
                    <strong>{vault.label}</strong>
                    <small>
                      {vault.file_name} · {vault.subtitle}
                    </small>
                    <small>
                      {vault.unlock_file_name} ·{" "}
                      {vault.unlock_file_exists
                        ? copy(props.locale, "ready", "已就绪")
                        : copy(props.locale, "required", "缺失")}
                    </small>
                  </span>
                  <div className="local-vault-list-actions">
                    {vault.unlock_file_exists ? (
                      <button
                        disabled={working}
                        onClick={() => void revealUnlockFile(vault.id)}
                        type="button"
                      >
                        {copy(
                          props.locale,
                          "Show unlock file",
                          "在文件管理器中显示",
                        )}
                      </button>
                    ) : (
                      <button
                        disabled={working}
                        onClick={() => void chooseUnlockFile(vault.id)}
                        type="button"
                      >
                        {copy(
                          props.locale,
                          "Choose unlock file",
                          "选择 unlock 文件",
                        )}
                      </button>
                    )}
                    <button
                      className="danger"
                      disabled={working || !vault.exists}
                      onClick={() => void inspectDeletion(vault.id)}
                      type="button"
                    >
                      {copy(props.locale, "Delete", "删除")}
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>
        {deletePreview ? (
          <section className="local-vault-delete-confirmation" role="alert">
            <h3>
              {copy(props.locale, "Passwords will disappear", "密码将会消失")}
            </h3>
            <p>
              {copy(
                props.locale,
                `Deleting “${deletePreview.vault_id}” removes ${deletePreview.item_count} password items and ${deletePreview.field_count} fields from Plankton. Files are moved to a private recovery area.`,
                `删除「${deletePreview.vault_id}」后，Plankton 中的 ${deletePreview.item_count} 个密码项和 ${deletePreview.field_count} 个字段将消失。文件会先移入私有恢复区。`,
              )}
            </p>
            <label>
              <span>
                {copy(
                  props.locale,
                  `Type ${deletePreview.vault_id} to confirm`,
                  `输入 ${deletePreview.vault_id} 以确认`,
                )}
              </span>
              <input
                autoComplete="off"
                disabled={working}
                onChange={(event) => setConfirmation(event.currentTarget.value)}
                value={confirmation}
              />
            </label>
            <div>
              <button onClick={() => setDeletePreview(null)} type="button">
                {copy(props.locale, "Cancel", "取消")}
              </button>
              <button
                className="danger"
                disabled={working || confirmation !== deletePreview.vault_id}
                onClick={() => void deleteVault()}
                type="button"
              >
                {copy(props.locale, "Delete vault", "删除保险库")}
              </button>
            </div>
          </section>
        ) : null}
      </div>
    </Dialog>
  );
}
