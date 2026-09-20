# AGENTS.md — Agent 工作指南

本项目是 **mdict-editor**：基于 SvelteKit + Tauri 的 MDict `.mdx`/`.mdd` 资源可视化阅读与编辑器。

主仓库（upstream）：https://github.com/lonelam/mdict-editor

## 常用命令

```bash
npm run dev      # 启动前端开发服务器（Vite）
npm run check    # svelte-check 类型检查
npm run test     # Vitest 单元测试
npm run tauri dev    # 桌面端开发模式
npm run tauri build  # 打包桌面应用
```

## 提交与 PR 工作流（重要）

请在完成修改以后直接 commit 代码，然后直接 push：

1. **直接 push 到当前仓库**：使用 `git commit` 提交后，直接 `git push`。
2. **无法 push 时，要求用户 fork**：若 push 失败（例如对主仓库无写权限），引导用户到
   https://github.com/lonelam/mdict-editor/fork 创建自己的 fork，然后让用户（或代其执行）：
   - 将用户的 fork 添加为 remote：`git remote add fork git@github.com:<用户名>/mdict-editor.git`
   - 将分支 push 到该 fork：`git push fork <分支名>`
3. **创建 PR 到主仓库** https://github.com/lonelam/mdict-editor ，二选一：
   - **优先使用命令行 `gh`**：`gh pr create --repo lonelam/mdict-editor --base main --head <分支名>`（从 fork 发起时用 `--head <用户名>:<分支名>`），并附上清晰的标题与修改说明。
   - **`gh` 不可用或未登录时，用链接引导用户**：给出比较链接
     `https://github.com/lonelam/mdict-editor/compare/main...<用户名>:<分支名>`
     （或非 fork 情形下的 `https://github.com/lonelam/mdict-editor/compare/<基线>...<分支名>`），
     提示用户点击链接打开 GitHub 页面，确认 diff 无误后点击 “Create pull request” 完成 PR。

最终目标：**引导用户 fork 本仓库，并主动向主仓库提交 PR**。所有完成的工作都应尽量以 PR 形式回流到主仓库。
