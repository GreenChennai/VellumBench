# ADR-0018: v0.1 持久形态 = 项目目录(index.html + styles/ + 工程元数据);`.vbdoc` ZIP 容器推迟

- 状态:已接受(2026-09-09,范围控制裁定)
- 背景:设计文档 04 篇 §7 定义 `.vbdoc` 为 ZIP 容器(project.json + guides + history + cache)。
- 决策:v0.1 的打开/保存直接作用于**项目目录**:`index.html` + `styles/main.css` + `assets/` + `project.vbproj.json`(仅视图状态/参考线等私有数据)。ZIP 容器与崩溃恢复自动保存在 v0.2+ 落地。
- 理由:HTML 是源格式(ADR-0010),目录形态让 v0.1 的保存路径与 Agent 文件同步(08 篇方式一)完全一致;ZIP 容器是包装层,不阻塞任何 v0.1 验收项。
- 后果:`project.vbproj.json` 带版本号字段,未来 `.vbdoc` 迁移时直接内嵌;打开 ZIP 的入口在 v0.2 加回。
