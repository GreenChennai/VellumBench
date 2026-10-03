# ADR-0051: i18n 策略——Fluent 单源、回退链与门禁解析器约束

- 状态:**已裁定并落地**(R0 第 7 交付 + R0 收口批;实现见 `vb_common::i18n`、`i18n/*.ftl`、`tools/gen_cmd_ftl.py`)
- 背景:22 篇 §4 R0 交付 7 与 G-UI3;全量双语收口在 R7,本 ADR 记录地基批已固化的策略。

## 1. 已裁定事项

1. **技术选型 Fluent**(`fluent` 0.17 + `unic-langid` + `fluent-syntax`,crates.io 锁版):ftl 经 `include_str!` 编译期内嵌,零运行时文件依赖;`OnceLock` 惰性构建双语并发版 Bundle,**`t()` 可先于任何显式 init 调用**(默认 zh,`AtomicU8` 语言态,`set_language` 热切换)。
2. **回退链**:当前语言 → zh → 内置最小表 → key 本身;缺词 `debug_assert!`(debug 红、release 返回 key 不 panic);`set_use_isolating(false)`(隔离符在旧宿主文本栈会变豆腐块)。
3. **目录规范**:命令标签 `cmd-<点分 id 转连字符>`(由 `tools/gen_cmd_ftl.py` 从 `CMD_LABELS` 机械抽取,禁手抄);界面文案 `ui-<面板>-<语义>`;zh 值与既有 UI 逐字一致(术语门禁不许漂),en 遵守 CONTEXT.md 术语表与禁用表。`i18n_catalog_complete` 门禁:CMD_LABELS 每 id 双语齐备 + zh 值逐字一致 + 既有 key 保留 + 恰 204 无孤儿。
4. **点号桥接**:FTL 标识符不允许 `.`,模块边界做 `.`→`-` 规范化(资源字节不动,旧宿主平键解析器零改动)。
5. **手工段纪律**:生成器只整段替换 BEGIN/END 标记块;手工 key 写在标记块外,重跑幂等。
6. **消费端**:vb_session::i18n 条目级 re-export(`t/t_args/try_t/FluentValue`),UI 侧只认识 vb_session,不直依赖 fluent 取词内部(fluent 的直依赖仅为再导出,已入 vb_session purity 白名单申报)。

## 2. R0 收口批追加的裁定(本 ADR 立此存照)

7. **门禁解析器只认单行消息**:`i18n_catalog_complete` 的解析器是单行 `key = value` 正则,FTL 多行续行消息会使门禁误报"缺 '='"。**裁定:目录一律单行消息**(长文案用 `|` 分隔);解析器升级为支持续行属可选改进,未做前以单行为硬约束(zh/en 双侧已按此改写)。
8. **purity 白名单申报**:vb_session 的 `fluent`(再导出)与 `serde_json`(mru recent.json 运行时依赖)经 `dependencies_stay_in_allowlist` 断言申报,申报理由内联在断言消息里。

## 3. 后果

- R7 收口清单:①vb_app 冻结宿主不接 t()(旧宿主停在 zh,可接受);②en.ftl 翻译复核(SC 报告已列 5 处拿不准条目);③双语 key 完整性断言并入机器门禁(现为脚本外核查);④语言设置项接入新宿主设置页(R2)。
