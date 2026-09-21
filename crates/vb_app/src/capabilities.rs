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

/// 能力状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapStatus {
    /// 已落地(有命令 ID、可被 Agent 复现)。
    Done,
    /// 部分落地:`note` 说明**已做哪一半、缺哪一半**。
    Partial(&'static str),
    /// 未落地:`note` 必须是「计划于 vX:…」(门禁校验前缀)。
    Planned(&'static str),
}

impl CapStatus {
    pub fn badge(self) -> &'static str {
        match self {
            CapStatus::Done => "已落地",
            CapStatus::Partial(_) => "部分",
            CapStatus::Planned(_) => "计划",
        }
    }

    pub fn note(self) -> &'static str {
        match self {
            CapStatus::Done => "",
            CapStatus::Partial(n) | CapStatus::Planned(n) => n,
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
        status: CapStatus::Partial(
            "不透明度蒙版(制作/反转/释放 → mask-image)已落地;剪切蒙版(路径级 Mod+7)计划于 v2",
        ),
        commands: &["view.toggle_opacity_panel", "object.clip_mask"],
    },
    Capability {
        id: "09-C",
        name: "切片工具(Shift+K → data-vb-slice)",
        status: CapStatus::Planned("计划于 v2:切片标记与导出联动(data-vb-slice 尚未建模)"),
        commands: &[],
    },
    Capability {
        id: "09-D",
        name: "图像能力(置入 / 替换 / 缺失资源告警)",
        status: CapStatus::Partial(
            "布局期已探测图像固有尺寸、缺失资源给告警;置入/替换对话框计划于 v2",
        ),
        commands: &[],
    },
    Capability {
        id: "09-E",
        name: "编辑态辅助(标尺 / 网格 / 参考线 / 轮廓 / 像素预览 / 度量)",
        status: CapStatus::Partial(
            "标尺·参考线·网格·智能参考线·轮廓模式已落地;像素预览与度量工具计划于 v2",
        ),
        commands: &[
            "view.toggle_rulers",
            "view.toggle_guides",
            "view.lock_guides",
            "view.guides_from_selection",
            "view.outline",
            "view.toggle_grid",
            "view.hide_edges",
        ],
    },
    Capability {
        id: "09-F",
        name: "响应式断点 / 伪类编辑",
        status: CapStatus::Planned(
            "计划于 v2:伪类规则导入已保真(L0),编辑器内的断点与伪类编辑尚未开始",
        ),
        commands: &[],
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
        status: CapStatus::Planned("计划于 v2:组件实例与主件同步"),
        commands: &[],
    },
    Capability {
        id: "09-I",
        name: "动效时间轴面板",
        status: CapStatus::Planned("计划于 v2:关键帧与动画时间轴"),
        commands: &[],
    },
    Capability {
        id: "09-J",
        name: "插件系统",
        status: CapStatus::Planned("计划于 v2.x:插件宿主与权限模型"),
        commands: &[],
    },
    Capability {
        id: "09-K",
        name: "CRDT / 协同 / Web 版 / 多平台",
        status: CapStatus::Planned("计划于 v3:不承诺档期"),
        commands: &[],
    },
    Capability {
        id: "09-L",
        name: "首选项九分类 / 键位方案编辑器 / 工作区保存",
        status: CapStatus::Partial(
            "工作区保存(workspace.json 停靠位/列数/面板坞态,损坏回退+告警)已落地;首选项九分类与键位编辑器计划于 v2",
        ),
        commands: &[
            "edit.preferences",
            "edit.keyboard_shortcuts",
            "window.workspace_basic",
            "view.dock_toolbar_left",
        ],
    },
    Capability {
        id: "09-M",
        name: "文档设置对话框(网格 / 参考线 / 断点 / 令牌 / 命名 / 输出选项)",
        status: CapStatus::Planned("计划于 v2:文档级设置集中入口"),
        commands: &[],
    },
    Capability {
        id: "09-N",
        name: "外部修改冲突对话框(三方对比)",
        status: CapStatus::Partial(
            "外部修改检测与自动采用(rev 判定 + 文件监听热重载)已落地;三方对比对话框计划于 v2",
        ),
        commands: &[],
    },
    Capability {
        id: "09-O",
        name: "字体缺失 / 冻结块警告对话框",
        status: CapStatus::Partial(
            "冻结块在不支持它的面板里给中文提示;字体缺失专项对话框计划于 v2(字体已随包安装)",
        ),
        commands: &["text.find_font"],
    },
    Capability {
        id: "09-P",
        name: "门禁 6 术语扫描 / i18n",
        status: CapStatus::Partial(
            "ci.ps1 步骤 6「术语扫描」已接;多语言内容(i18n)计划于 v2",
        ),
        commands: &[],
    },
    // ── 承接各报告遗留(主文档 §12.1 的"还差什么") ──
    Capability {
        id: "X-1",
        name: "路径查找器扩展运算",
        status: CapStatus::Partial(
            "合并 / 减去后方对象 / 裁剪 已落地(与基础运算同几何内核);分割·修边·轮廓需「一条命令产出多节点」的多结果模型,计划于 v2",
        ),
        commands: &["path.merge", "path.subtract_back", "path.crop"],
    },
    Capability {
        id: "X-2",
        name: "画布真文本(ADR-0017)",
        status: CapStatus::Partial(
            "CPU 导出已是真字形;画布仍为 egui 近似,Parley 多行/双向留后续(复议中)",
        ),
        commands: &[],
    },
    Capability {
        id: "X-3",
        name: "SVG / PDF 导入边界",
        status: CapStatus::Partial(
            "自由曲线以包围盒近似 + 警告;渐变取中点色;filter/mask/clipPath 跳过 —— 留后续逐版补,不静默",
        ),
        commands: &[],
    },
    Capability {
        id: "X-4",
        name: "变换工具族(旋转 R / 镜像 O / 缩放 S / 自由变换 E)",
        status: CapStatus::Planned(
            "计划于 v2:数值化变换(面板 ⇧F8)与画布手柄已落地;工具化的中心点单击语义待做",
        ),
        commands: &["view.toggle_transform_panel"],
    },
    Capability {
        id: "X-5",
        name: "曲线工具(曲率) / 铅笔 / 形状生成器",
        status: CapStatus::Planned("计划于 v2:P1 优先级,排在路径布尔之后"),
        commands: &[],
    },
    Capability {
        id: "X-6",
        name: "浏览器校对(WPI 截图对拍)",
        status: CapStatus::Planned("计划于 v2:接入 WPI 截图与像素 diff"),
        commands: &["view.browser_proof"],
    },
    Capability {
        id: "X-7",
        name: "「关于」之外的应用级对话框(打印 / 新建工作区)",
        status: CapStatus::Planned("计划于 v2:打印与自定义工作区"),
        commands: &["file.print", "window.new_workspace"],
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
}
