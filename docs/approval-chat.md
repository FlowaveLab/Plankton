# 审批对话：功能与验收

适用入口：审批详情与审计记录中的“与审批 Agent 对话”。每条审批有自己的会话历史，原审批对话继续使用原 Agent session；“新建对话”建立独立上下文，并带入该审批的脱敏证据。

## 已实现

- 新建、切换、搜索、重命名会话，自动按首条问题生成标题。
- 每个会话保留独立草稿。生成时可以切到其他会话，后台事件不会写入当前会话。
- 消息、标题和 Agent session 在本机持久化；重新打开应用后恢复历史。被中断的生成标记为已停止，可以继续。
- 输入区固定在消息区下方；生成默认跟随到底部，手动上翻暂停跟随，“滚动到最新消息”恢复跟随。
- 长代码与表格在消息内滚动；思考和工具调用保留结构化展示。支持复制回复，代码和表格控件提供中文文案。
- Enter 发送，Shift + Enter 换行，中文输入法确认候选词不发送。发送等待期间防止重复提交；失败保留错误与草稿。
- AI Elements 使用 Plankton 的纸色、墨色、朱红及直角边框；窄窗口改用会话选择器。

## 验收步骤

1. 在一条审批详情中展开聊天，发送问题 A，确认逐步输出与底部跟随；上翻阅读后输出应继续，但视口不应被拉回。点击向下按钮恢复跟随。
2. 生成中“新建对话”，发送问题 B；来回切换，确认两份消息和草稿各自独立，历史列表显示后台生成状态。
3. 新会话开始输出后停止，再继续发送，确认 Agent 可以延续该会话。
4. 重命名对话，退出并重新打开应用，再次打开同一条审批，确认标题、消息和 session 恢复。
5. 缩小窗口检查会话选择器、长代码/表格的滚动，以及输入区是否始终可用。
6. 用中文输入法输入并确认候选词，再按 Enter 发送；Shift + Enter 应只换行。模拟连接失败，确认错误不消失、草稿可重新发送。

## 实现位置

- `apps/desktop/src/approvalChatApi.ts`：Tauri 会话接口及类型。
- `apps/desktop/src/hooks/useApprovalChat.ts`：会话、草稿、异步响应与事件隔离。
- `apps/desktop/src/components/ApprovalChat.tsx`：历史列表、消息区和输入区。
- `apps/desktop/src/components/ApprovalChatMessageBody.tsx`：文本、思考及工具调用。
- `apps/desktop/src/components/approval-chat.css`：主题及响应布局；由 `src/main.tsx` 提前加载。
- `apps/desktop/src-tauri/src/main.rs`：会话所有权、持久化、流式和停止生命周期。
- `crates/plankton-core/src/acp.rs`：在输出前发出 `SessionStarted`，用于停止后的续接。

历史文件与 daemon state 位于同一目录，命名为 `approval-chat-<database namespace>.json`。采用私有临时文件加原子替换，Unix 权限为 0600；流式过程中最多每 500ms 保存一次，结束、停止和重命名立即保存。异常退出可能丢失最后一个保存间隔内的片段。损坏的历史不会被静默覆盖，错误仅影响聊天，不阻止主应用启动。会话续接依赖所配置 ACP Agent 支持并保留该 session。

AI Elements 沿用仓库已有组件，并参照 [Conversation](https://elements.ai-sdk.dev/components/conversation) 与 [Message](https://elements.ai-sdk.dev/components/message) 的滚动及 Streamdown 样式要求进行适配。

## 验证记录（2026-09-05）

- 前端全部 29 个测试文件、314 项测试通过；随后小幅边界修正再次通过聊天相关 16 项测试。
- 桌面 Rust 单元及集成测试共 83 项通过；ACP 聊天相关 4 项测试通过。
- TypeScript、Vite 生产构建、入口 CSS 检查、Rust 格式检查与 Clippy 通过。
- 浏览器使用实际 React 组件和受控模拟 API：确认流式跟随、上翻暂停、恢复跟随、输入区位置固定、会话新建/重命名、中文输入法不误发；1440px 和 480px 宽度无页面横向溢出。
- Rust 测试/Clippy 使用 `TAURI_CONFIG='{"bundle":{"resources":[]}}'`，跳过与聊天无关的 KeePassXC 资源复制。默认资源复制曾遇到本机 `Operation not permitted`，因此这些检查不代表安装包构建通过。
- 未发起真实审批或模型请求，未替换已安装应用。真实 Agent 续接和最终产品体验由用户验收。
