//! **能力台账**(副文档 09-3):能力 / 状态 / 命令 ID / 是否 Agent 可复现。
//!
//! 这是「还有哪些没做」的**单一真相**:菜单、面板、README 与
//! 「帮助 → 能力台账」窗口都读它,不再各自维护一份口头清单。
//!
//! 三条纪律:
//! 1. **不允许"点了没反应"**(`design/06 §七`):凡 [`CapStatus::Planned`]
//!    必须带「计划于 vX」的中文说明,并出现在台账窗口里;
//! 2. `commands` 里列出的每个 id 都必须是**已注册命令**(门禁测试逐条校验),
//!    否则台账就是在说谎;
//! 3. 状态是**代码实测**的结论,不是愿景 —— 改动状态必须与代码同 commit。
//!
//! 阶段 5(05-1,副文档 05 §三)新增**三态收敛**规则:
//! `Done` / `Partial` / `Dropped` 取代悬空的"计划"。
//! [`CapStatus::Dropped`] 是**明确不做**:理由必填、必须"UI 无入口"、
//! 必须 README 写替代方案(门禁测试逐条校验);**仅限**副文档 05 §2.4
//! 的"与 HTML/产品定位根本冲突"项,不许当成甩锅出口。

/// 能力状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapStatus {
    /// 已落地(有命令 ID、可被 Agent 复现)。
    Done,
    /// 部分落地:`note` 说明**已做哪一半、缺哪一半**。
    Partial(&'static str),
    /// 未落地:`note` 必须是「计划于 vX:…」(门禁校验前缀)。
    Planned(&'static str),
    /// **明确不做**(05-1 三态收敛):`note` 必填理由;同时必须
    /// ①UI 无入口;②README 写明替代方案。理由**不得**再含"计划于"
    /// 字样(防悬空承诺,门禁校验)。
    Dropped(&'static str),
}

impl CapStatus {
    pub fn badge(self) -> &'static str {
        match self {
            CapStatus::Done => "已落地",
            CapStatus::Partial(_) => "部分",
            CapStatus::Planned(_) => "计划",
            CapStatus::Dropped(_) => "不做",
        }
    }

    pub fn note(self) -> &'static str {
        match self {
            CapStatus::Done => "",
            CapStatus::Partial(n) | CapStatus::Planned(n) | CapStatus::Dropped(n) => n,
        }
    }
}

/// 一条能力。
#[derive(Debug, Clone, Copy)]
pub struct Capability {
    /// 台账编号(与副文档 09 §2 / 主文档 §12 对齐)。
    pub id: &'static str,
    /// 中文名(窗口与文档共用)。
    pub name: &'static str,
    pub status: CapStatus,
    /// 相关命令 ID(可为空 = 无命令入口,纯手工交互)。
    pub commands: &'static [&'static str],
}

/// 能力台账(**单一真相**)。
pub const CAPABILITIES: &[Capability] = &[
    // ── 副文档 09 §2:已规划但未落地的用户可见能力 ──
    Capability {
        id: "09-A",
        name: "隔离模式(双击进编组 / 面包屑 / 其余淡化)",
        status: CapStatus::Done,
        commands: &["canvas.cancel"],
    },
    Capability {
        id: "09-B",
        name: "蒙版",
        status: CapStatus::Done,
        commands: &[
            "view.toggle_opacity_panel",
            "object.clip_mask",
            "object.release_clip_mask",
        ],
    },
    Capability {
        id: "09-C",
        name: "切片工具(Shift+K → data-vb-slice)",
        status: CapStatus::Done,
        commands: &["tool.slice", "object.slice_from_selection", "file.export_dialog"],
    },
    Capability {
        id: "09-D",
        name: "图像能力(置入 / 替换 / 缺失资源告警)",
        status: CapStatus::Done,
        commands: &["file.place_image", "object.replace_image"],
    },
    Capability {
        id: "09-E",
        name: "编辑态辅助(标尺 / 网格 / 参考线 / 轮廓 / 像素预览 / 度量)",
        status: CapStatus::Done,
        commands: &[
            "view.toggle_rulers",
            "view.toggle_guides",
            "view.lock_guides",
            "view.guides_from_selection",
            "view.outline",
            "view.toggle_grid",
            "view.hide_edges",
            "view.pixel_preview",
            "tool.measure",
        ],
    },
    Capability {
        id: "09-F",
        name: "响应式断点 / 伪类编辑",
        status: CapStatus::Done,
        // 05-5(阶段 5-C):断点清单存 index.html 的 vb-breakpoints meta,
        // 覆盖样式写 @media (max-width: Npx) canonical 块(SetMediaStyle,
        // 可撤销);状态栏切换器 + 文档设置断点行 + view.breakpoint_cycle。
        // 伪类最小闭环::hover 经属性面板「状态」下拉落到 selector:hover
        // 规则(SetPseudoStyle)。诚实边界:画布预览不套用覆盖样式(以
        // 「浏览器校对」为准);其余伪类与媒体查询组合保持冻结块原文。
        commands: &[
            "view.breakpoint_cycle",
            "style.state_toggle",
            "file.doc_settings",
        ],
    },
    Capability {
        id: "09-G",
        name: "设计令牌面板(改一处全站生效)",
        status: CapStatus::Done,
        commands: &["window.tab_tokens", "color.toggle_target"],
    },
    Capability {
        id: "09-H",
        name: "符号 / 组件",
        status: CapStatus::Partial(
            "已落地:主件原型存文档内 vb-symbol-defs(hidden,零 JS)、实例为真实 DOM 副本、             覆盖登记与主件同步(复用多结果事务,可撤销)、创建/分离/重置覆盖/替换主件定义/选择所有实例;             跨文档复用(05-8-5 可选项,symbols/*.html 引用导入)留后续复议",
        ),
        commands: &[
            "object.symbol_create",
            "object.symbol_detach",
            "object.symbol_reset_overrides",
            "object.symbol_swap_main",
            "object.symbol_select_instances",
        ],
    },
    Capability {
        id: "09-I",
        name: "动效时间轴面板",
        status: CapStatus::Done,
        commands: &[
            "view.toggle_timeline_panel",
            "anim.play_toggle",
            "anim.stop",
            "anim.loop_toggle",
            "anim.keyframe_add",
            "anim.keyframe_delete",
            "anim.clear",
        ],
    },
    Capability {
        id: "09-J",
        name: "插件系统",
        status: CapStatus::Done,
        // 05-10(阶段 5-F):插件 = 外部子进程 + stdio JSON-RPC 2.0 +
        // manifest 权限(ADR-VB-L12,与 vellum-mcp 同构零新依赖)。
        // 已落地:plugin.json 严格 schema(未知字段拒绝)、默认零权限 +
        // 首次启用授权弹窗 + plugins.json 持久化、越权拒绝并记录、
        // runCommand(白名单内走宿主命令路径)/ 文档只读投影 / 受控面板
        // UI(有限元件,按钮=回发通知)/ 导出器(只落用户选的目录)、
        // 状态机 Stopped/Starting/Running/Crashed/Unauthorized + 超时杀 +
        // 崩溃隔离(独立进程,DROP 兜底)+ 日志环、仓库自带示例插件
        // plugins/example-stats(统计元素)。诚实边界:面板为只读投影 +
        // 受控元件(无画布直接交互);插件命令菜单聚合在插件面板与命令
        // 路径,不新增第十个顶菜单(菜单栏保持 AI 规范 9 项)。
        commands: &["edit.plugins", "view.toggle_plugins_panel"],
    },
    Capability {
        id: "09-K",
        name: "CRDT / 协同 / Web 版 / 多平台",
        // 阶段 5-G(05-11,ADR-VB-L13 / ADR-0031 / ADR-0032):协同 =
        // 会话合并层,文档真相仍是 canonical HTML。逐项状态:
        // - K1 多平台 = Partial:linux/macos 交叉编译级通过(vb_render 的
        //   fontconfig 走 dlopen 特性)+ CI 加 linux/macos 双 runner
        //   (check+test 级);字体回退/窗口 DPI/GPU 画布差异无法本机验证,
        //   待实机清单 docs/k1-nonwindows-runtime-checklist.md。
        // - K2 Web = Done(按分册裁剪壳路线):crates/vb_web —— canonical
        //   HTML 预览(预览即真相)+ 画板切换 + 文字轻编辑(Command::SetText,
        //   rev 推进)+ 导出与桌面 Ctrl+S 字节同源;无头 Edge 自动验收 5 项
        //   机械断言全过(?autotest=1)。完整 vb_app 的 wasm 化受原生依赖
        //   阻碍(rfd/notify/eframe run_native/pdfium),评估与承诺边界见
        //   docs/k2-web-capability.md;pdfium 已 cfg 门控(wasm 不编译)。
        // - K3 多写者 = Done:故障注入 e2e 三场景全绿
        //   (vb_app/tests/multi_writer_fault_injection.rs)—— 双进程交替
        //   编辑零丢改动 / 同节点同属性冲突检出 + 09-N 双向裁决 /
        //   半截写盘崩溃重放 + .vb-autosave 快照恢复;GUI 触发链(印记 →
        //   对话框 → 裁决)由 conflict_dialog 单测覆盖。
        // - K4 协同会话 = Partial:会话合并层全绿 —— vb_doc::collab 的
        //   LWW-Element-State CRDT(元素 = sid+属性,全序 = lamport+writer;
        //   合并三律有断言)+ 共享目录传输(.vb-collab ops JSON,坏文件
        //   容忍)+ 收敛断言(两端文件表字节一致、并发编辑零丢失、往返
        //   L0/L1 不破);结构性变更不走 CRDT(命令 + 三向对比 + rev 乐观锁)。
        //   缺的一半:GUI 会话入口(「启动协同会话」菜单)未接 —— 会话层
        //   当前可经库 API/测试驱动,无菜单命令。
        status: CapStatus::Partial(
            "逐项:K1=Partial(编译级通过+CI 双 runner;字体/DPI/GPU 运行时留后续实机回填,清单 docs/k1-nonwindows-runtime-checklist.md)|K2=Done(裁剪 web 壳 vb_web:HTML 预览+切画板+文字轻编辑+字节同源导出,浏览器 5 断言全过;完整 wasm 化受阻,评估 docs/k2-web-capability.md)|K3=Done(故障注入 e2e 三场景全绿:交替零丢/冲突双向裁决/崩溃重放+快照恢复)|K4=Partial(LWW 会话合并层+共享目录传输+收敛/L0/L1 断言全绿,ADR-0031;GUI 会话入口留后续接线)",
        ),
        commands: &[],
    },
    Capability {
        id: "09-L",
        name: "首选项九分类 / 键位方案编辑器 / 工作区保存",
        status: CapStatus::Done,
        commands: &[
            "edit.preferences",
            "edit.keyboard_shortcuts",
            "window.workspace_basic",
            "view.dock_toolbar_left",
            "view.developer_stats",
            "view.toggle_hints",
            "view.ui_scale_up",
            "view.ui_scale_down",
            "view.ui_scale_reset",
        ],
    },
    Capability {
        id: "09-M",
        name: "文档设置对话框(命名 / 画板预设 / 输出选项 / 网格与参考线)",
        status: CapStatus::Done,
        commands: &["file.doc_settings"],
    },
    Capability {
        id: "09-N",
        name: "外部修改冲突对话框(三方对比)",
        status: CapStatus::Done,
        commands: &["file.resolve_conflict"],
    },
    Capability {
        id: "09-O",
        name: "字体缺失 / 冻结块警告对话框",
        status: CapStatus::Done,
        commands: &["text.find_font"],
    },
    Capability {
        id: "09-P",
        name: "门禁 6 术语扫描 / i18n",
        status: CapStatus::Done,
        // 05-7(阶段 5-C):i18n 骨架 —— 仓库根 i18n/zh.ftl + en.ftl
        // (编译期内嵌),t(key) 回退链(当前语言 → 中文 → key 本身,
        // 缺词记一次调试日志);首选项「常规」页界面语言切换,持久化
        // workspace.json ui_lang。术语门禁 tools/check_terminology.py
        // 扫描中英两套资源(中文禁用词子串 + 英文对应词整词)。
        // 诚实边界:骨架覆盖 = 本批新增对话框/菜单文案;既有界面文案
        // 仍为中文硬编码,全量抽取留后续批次(键集一致性有门禁测试)。
        commands: &[],
    },
    // ── 阶段 2(副文档 02:启动主页与多窗口)──
    Capability {
        id: "02-A",
        name: "启动流程(无参数开主页 / --project 直达 / --help / 拖拽打开)",
        status: CapStatus::Done,
        commands: &["home.open_project"],
    },
    Capability {
        id: "02-B",
        name: "启动主页(最近项目列表 / 新建 / 打开 / 模板 / 搜索 / 键盘导航 / 空态)",
        status: CapStatus::Done,
        commands: &[
            "home.new_project",
            "home.open_project",
            "home.new_from_template",
            "home.open_selected",
            "home.search",
            "home.select_next",
            "home.select_prev",
            "home.remove_selected",
            "home.pin_selected",
            "home.capabilities",
        ],
    },
    Capability {
        id: "02-C",
        name: "最近项目与最近会话(recent.json:LRU/固定/失效项置灰/原子写+损坏回退)",
        status: CapStatus::Done,
        commands: &["home.restore_session"],
    },
    Capability {
        id: "02-D",
        name: "新建项目对话框(画板预设/取向/画板数/输出模式,合法序列化,新窗口打开)",
        status: CapStatus::Done,
        commands: &["home.new_project", "home.new_from_template", "file.new"],
    },
    Capability {
        id: "02-E",
        name: "多窗口(一项目一窗口 / 状态隔离 / 同项目聚焦 / 关闭确认 / 标题未保存*)",
        status: CapStatus::Done,
        commands: &["file.close", "file.home", "file.open"],
    },
    Capability {
        id: "02-F",
        name: "项目缩略图(.vb-cache/thumb.png,首次打开后台生成,失败占位)",
        status: CapStatus::Done,
        commands: &[],
    },
    // ── 阶段 7(副文档 07-1:数据安全批次)──
    Capability {
        id: "07-A",
        name: "自动保存(.vb-autosave/ 滚动快照 ×3,间隔可配置,不覆盖 index.html)",
        status: CapStatus::Done,
        commands: &["edit.autosave_interval", "file.save"],
    },
    Capability {
        id: "07-B",
        name: "崩溃恢复(启动检出快照 → 恢复/丢弃/查看差异三方对比;强杀进程可复活)",
        status: CapStatus::Done,
        commands: &[],
    },
    Capability {
        id: "07-C",
        name: "未保存标记与关闭确认(标题 <项目名>* / 窗口与进程两级 保存·丢弃·取消)",
        status: CapStatus::Done,
        commands: &["file.close", "app.quit"],
    },
    Capability {
        id: "07-D",
        name: "撤销历史面板(最近 50 步中文命令名,点击跳转;回退遇重做尾需确认)",
        status: CapStatus::Done,
        commands: &["view.toggle_history_panel", "edit.undo", "edit.redo"],
    },
    Capability {
        id: "07-E",
        name: "项目健康检查(缺失资源/失效链接/冻结块/未使用资产/超长文件,可点击定位)",
        status: CapStatus::Done,
        commands: &["file.health_check"],
    },
    // ── 阶段 7b(编辑体验 / 外部协同批次:07-K / 07-L / 07-N / 07-Q / 07-R)──
    Capability {
        id: "07-K",
        name: "资产面板(assets/ 清单·引用关系·定位到引用·替换引用·未使用标记)",
        status: CapStatus::Done,
        commands: &["view.toggle_assets_panel"],
    },
    Capability {
        id: "07-L",
        name: "图层面板富交互(拖拽重排含跨编组/Alt+拖复制/颜色标记/右键菜单)",
        status: CapStatus::Partial(
            "拖拽重排·Alt+拖复制·颜色标记(data-vb-mark)·右键菜单(改名/复制/删除/编组/显隐/锁定/前后移/隔离/锁定其他/隐藏其他/选择同类/转换为编组)已落地;单节点导出与剪切蒙版菜单项因无命令支撑未入菜单,留后续",
        ),
        commands: &["view.toggle_layers_panel"],
    },
    Capability {
        id: "07-N",
        name: "无障碍检查(缺 alt 图片/交互元素可访问名/对比度 WCAG AA 粗判,只提示不强制,可定位)",
        status: CapStatus::Done,
        commands: &["file.health_check"],
    },
    Capability {
        id: "07-Q",
        name: "artboard 项目识别(主页来源徽标 vb-/vs-/vsm-artboard;--project 直开/编辑/存回兼容)",
        status: CapStatus::Done,
        commands: &["home.open_project", "home.open_selected"],
    },
    Capability {
        id: "07-R",
        name: "Agent 改动可见性(外部改动印记:状态栏已重载/未采用 + 点击看触发时间与文件)",
        status: CapStatus::Done,
        commands: &[],
    },
    // ── 承接各报告遗留(主文档 §12.1 的"还差什么") ──
    Capability {
        id: "X-1",
        name: "路径查找器 10 运算",
        status: CapStatus::Done,
        // 05-3:形状模式 4 项 + 路径查找器 6 项全部有真实几何实现与
        // 对象菜单「路径查找器」子菜单入口;分割/修边/轮廓走
        // Command::MultiResult 多结果事务(一条撤销),几何与输出语义
        // 由 vb_tools::pathfinder 单测与菜单 tooltip 锁定。
        commands: &[
            "path.union",
            "path.subtract",
            "path.intersect",
            "path.xor",
            "path.merge",
            "path.subtract_back",
            "path.crop",
            "path.divide",
            "path.trim",
            "path.outline",
        ],
    },
    Capability {
        id: "X-2",
        name: "画布真文本(ADR-0017)",
        status: CapStatus::Partial(
            "CPU 导出已是真字形;画布仍为 egui 近似 —— 画布角落常驻「预览为近似渲染」提示、状态栏常驻「近似渲染」标注(03-5,与 UI 口径一致),可经「视图 → 浏览器校对」对拍真实浏览器;Parley 多行/双向留后续(复议中)",
        ),
        commands: &["view.browser_proof"],
    },
    Capability {
        id: "X-3",
        name: "SVG / PDF 导入边界",
        status: CapStatus::Partial(
            "05-6:SVG 自由曲线已真实矢量化(C/Q 贝塞尔保真,A 圆弧为 usvg 三次逼近并标注);filter/mask/clipPath/渐变/pattern 逐项跳过或近似且导入时给用户可见清单(kiln-cli 输出 warnings 字段),不静默;文本/图像/图元折算保持既有边界。PDF 导入的同类逐项清单与取色来源标注留后续",
        ),
        commands: &[],
    },
    Capability {
        id: "X-4",
        name: "变换工具族(旋转 R / 镜像 O / 缩放 S / 自由变换 E)",
        status: CapStatus::Done,
        commands: &[
            "view.toggle_transform_panel",
            "tool.rotate",
            "tool.mirror",
            "tool.scale",
            "tool.free_transform",
        ],
    },
    Capability {
        id: "X-5",
        name: "曲线工具(曲率) / 铅笔 / 形状生成器",
        status: CapStatus::Partial(
            "曲率(点击路径段自动拟合平滑控制点)与铅笔(自由绘制 → 保真度容差抽稀为路径)已落地;形状生成器依赖「一次操作 → 多节点」多结果底座(05-3 批次),留后续",
        ),
        commands: &["tool.curvature", "tool.pencil", "edit.pencil_fidelity"],
    },
    Capability {
        id: "X-6",
        name: "浏览器校对(画布 vs 系统浏览器,原生 CDP;并排/叠加/滑块 + 分数/热力图/导出;浏览器缺失显式降级。原计划接 WPI,因属独立项目改用同仓库 vb_browser)",
        status: CapStatus::Done,
        commands: &["view.browser_proof"],
    },
    Capability {
        id: "X-7",
        name: "「关于」之外的应用级对话框(打印 / 新建工作区)",
        status: CapStatus::Done,
        commands: &["file.print", "window.new_workspace"],
    },
    // ── 阶段 5(05-1 三态收敛):Dropped 仅限副文档 05 §2.4 的两条 ──
    // 「明确不做」也要显式、可检查:理由必填、UI 无入口、README 写替代。
    Capability {
        id: "DROP-1",
        name: "实时上色 / 网格工具 / 图像描摹 / 3D / 透视网格",
        status: CapStatus::Dropped(
            "无 HTML/CSS 对应,与『用 AI 心智编辑标准 HTML』定位冲突。替代:多对象配色用路径查找器(对象菜单),渐变过渡用多层径向渐变叠加,位图素材直接置入后用蒙版裁剪",
        ),
        commands: &[],
    },
    Capability {
        id: "DROP-2",
        name: "云端工程 / 账号体系 / 资源市场",
        status: CapStatus::Dropped(
            "与本地优先 + HTML 源格式定位冲突(云端会引入账号与素材许可链)。替代:项目即本地目录(index.html 可 diff),素材走 assets/ 目录与资产面板,跨机同步交给文件盘/网盘",
        ),
        commands: &[],
    },
    // ── 第四轮 B1:UI 界面全量优化 + 操作手感优化 ──
    Capability {
        id: "B1",
        name: "界面动效总开关(对话框/面板淡入、悬停过渡、toast 消退,可关且持久化)",
        status: CapStatus::Done,
        commands: &["view.toggle_motion"],
    },
];

/// 台账窗口显隐(会话态;不持久化 —— 它是"查一下",不是工作区布局的一部分)。
#[derive(Debug, Default)]
pub struct CapabilityUi {
    pub open: bool,
}

impl CapabilityUi {
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::is_implemented;
    use std::collections::HashSet;

    /// 台账里列出的每个命令都必须是**已注册命令**(否则台账在说谎)。
    #[test]
    fn every_listed_command_is_implemented() {
        for c in CAPABILITIES {
            for id in c.commands {
                assert!(
                    is_implemented(id),
                    "能力台账 {} 引用了未注册命令 {id}",
                    c.id
                );
            }
        }
    }

    /// 编号唯一(方便文档/窗口互相引用)。
    #[test]
    fn capability_ids_are_unique() {
        let mut seen = HashSet::new();
        for c in CAPABILITIES {
            assert!(seen.insert(c.id), "台账编号重复:{}", c.id);
        }
        assert_eq!(seen.len(), CAPABILITIES.len());
    }

    /// 未落地项必须写明「计划于 vX」(09 §2 原则:不许点了没反应、不许含糊)。
    #[test]
    fn planned_items_declare_a_version() {
        for c in CAPABILITIES {
            if let CapStatus::Planned(note) = c.status {
                assert!(
                    note.contains("计划于 v"),
                    "{} ({}) 的计划说明必须写明版本:{note}",
                    c.id,
                    c.name
                );
                assert!(!note.trim().is_empty(), "{} 计划说明为空", c.id);
            }
        }
    }

    /// 06-4 收口门禁(05-1 遗留 / 主文档 §5.1 阶段 5 验收门):台账不允许
    /// 任何悬空 [`CapStatus::Planned`] —— 「计划于 vX」是没有交付日期的
    /// 承诺,本身就是技术债。合法出路只有两条:
    /// ①本轮能做的 → `Done`;缺半的 → `Partial`(写明缺哪一半与去向,这是
    ///   合法态,见 [`partial_items_explain_the_gap`]);
    /// ②与 HTML/产品定位根本冲突的 → `Dropped`(门槛极高,另有两条门禁
    ///   锁死理由与 UI 无入口)。
    #[test]
    fn no_dangling_planned() {
        let planned: Vec<&Capability> = CAPABILITIES
            .iter()
            .filter(|c| matches!(c.status, CapStatus::Planned(_)))
            .collect();
        assert!(
            planned.is_empty(),
            "台账仍有 {} 条悬空 Planned,必须收口(Done/Partial/Dropped):{}",
            planned.len(),
            planned.iter().map(|c| c.id).collect::<Vec<_>>().join(", ")
        );
    }

    /// 部分落地项必须说清"缺哪一半"(只写"部分"等于没说)。
    #[test]
    fn partial_items_explain_the_gap() {
        for c in CAPABILITIES {
            if let CapStatus::Partial(note) = c.status {
                assert!(
                    note.contains("计划于 v") || note.contains("留后续") || note.contains("复议"),
                    "{} ({}) 的部分说明必须指向去向:{note}",
                    c.id,
                    c.name
                );
            }
        }
    }

    /// 副文档 09 §2 的 16 条(09-A…09-P)必须**一条不漏**地在台账里。
    #[test]
    fn all_doc_items_are_covered() {
        for suffix in [
            "09-A", "09-B", "09-C", "09-D", "09-E", "09-F", "09-G", "09-H", "09-I", "09-J", "09-K",
            "09-L", "09-M", "09-N", "09-O", "09-P",
        ] {
            assert!(
                CAPABILITIES.iter().any(|c| c.id == suffix),
                "能力台账缺副文档 09 的 {suffix}"
            );
        }
    }

    /// 每条能力都得有人话名字(窗口直接显示,不能空)。
    #[test]
    fn names_are_non_empty() {
        for c in CAPABILITIES {
            assert!(!c.name.trim().is_empty(), "{} 无名称", c.id);
            assert!(!c.id.trim().is_empty(), "能力台账条目编号为空");
        }
    }

    /// 05-1 三态收敛门禁①:`Dropped` 计数 ≤2(仅限副文档 05 §2.4 的
    /// "根本冲突"两条),理由非空且**不得含"计划于"字样** —— 防悬空承诺
    /// 的机械约束:「不做」就是不做,不许再挂版本号。
    #[test]
    fn dropped_items_are_limited_and_reasoned() {
        let dropped: Vec<&Capability> = CAPABILITIES
            .iter()
            .filter(|c| matches!(c.status, CapStatus::Dropped(_)))
            .collect();
        assert!(
            dropped.len() <= 2,
            "Dropped 仅限副文档 05 §2.4 两条,当前 {} 条",
            dropped.len()
        );
        for c in &dropped {
            let note = c.status.note();
            assert!(!note.trim().is_empty(), "{} 的 Dropped 理由为空", c.id);
            assert!(
                !note.contains("计划于"),
                "{} 的 Dropped 理由不得再含「计划于」(防悬空):{note}",
                c.id
            );
        }
    }

    /// 05-1 三态收敛门禁②:所有 `Dropped` 项必须**UI 无入口可复现** ——
    /// 不列任何命令、命令注册表(`IMPLEMENTED_IDS`)里没有它的 id、
    /// 快捷键表里也没有。出现即视为"灰按钮/死入口"回归。
    #[test]
    fn dropped_items_have_no_command_entry() {
        for c in CAPABILITIES {
            if !matches!(c.status, CapStatus::Dropped(_)) {
                continue;
            }
            assert!(
                c.commands.is_empty(),
                "{} 是 Dropped 项,不得声明命令入口",
                c.id
            );
            for id in c.commands {
                assert!(
                    !is_implemented(id),
                    "{} 的 Dropped 能力出现在命令注册表:{id}",
                    c.id
                );
            }
        }
    }
}
