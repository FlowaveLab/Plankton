export type WorkspaceView =
  | "requests"
  | "passwords"
  | "connections"
  | "agents"
  | "policies"
  | "audit"
  | "diagnostics";

export type PasswordField = {
  key: string;
  label: string;
  value: string;
  resourceId: string;
  secret?: boolean;
};

export type PasswordBackend = "plankton" | "one_password" | "bitwarden";
export type PasswordOrigin =
  | "local"
  | "dotenv"
  | "plankton_vault"
  | "one_password"
  | "bitwarden";

export type PasswordItem = {
  id: string;
  title: string;
  vault: string;
  group: string;
  tags: string[];
  username: string;
  notes: string;
  updatedAt: string;
  fields: PasswordField[];
  backend: PasswordBackend;
  origin: PasswordOrigin;
  archived?: boolean;
};

export const workspaceNav: Array<{
  id: WorkspaceView;
  en: string;
  zh: string;
}> = [
  { id: "requests", en: "Requests", zh: "请求" },
  { id: "passwords", en: "Passwords", zh: "密码库" },
  { id: "connections", en: "Connections", zh: "连接" },
  { id: "agents", en: "Agents & Models", zh: "智能体与模型" },
  { id: "policies", en: "Policies", zh: "策略" },
  { id: "diagnostics", en: "Diagnostics", zh: "诊断" },
];
