//! 控制面板(S1-c 02-2,AI 招牌)+ 面板元数据。
//!
//! 职责分三层(ADR-VB-U04:**工具只声明,不各画各的**):
//!
//! 1. **Spec(纯数据)**:[`ControlPanelSpec`] 描述"当前工具态应有哪些
//!    字段"(标签 / 控件种类 / 写回命令);由纯函数 [`spec_for`] 按
//!    `design/03 §三` 的八工具态字段表生成 —— 无 egui、无文档依赖,
//!    门 1 单测直接断言字段集合;
//! 2. **共享写回构建器(纯函数)**:[`geom_field_cmds`] /
//!    [`style_prop_cmds`] / [`attr_cmds`] / [`rotation_cmds`] /
//!    [`rename_cmds`] / 渐变系列 —— 面板控件提交时调它构造文档命令,
//!    门 2 文档状态级测试走同一条路径(`UndoStack::push` + 断言
//!    Document/CSS),保证"面板上每个控件真的写文档";
//! 3. **渲染器**:[`VellumApp::control_bar`] 菜单栏下 40px 通栏,统一
//!    消费 spec(按字段 id 取值 / 提交),右侧固定区:文档标题(改名,
//!    经 `SetMetaTitle`)、画板切换下拉、缩放下拉。
//!
//! **不做"点了没反应"**:design/03 字段表里暂无命令支撑的项(倾斜、
//! 参考点九宫格、色标编辑、浏览器校对按钮等)**不进 spec**,登记为
//! 阶段 2/4/5/8 遗留项(见 02c 报告);提示性条目用 `Hint`(非交互,
//! 不属于"点了没反应")。

use egui::Color32;
use vb_css::Decl;
use vb_doc::commands::Command;
use vb_ui::components::{ColorField, NumField};
use vb_ui::theme;

use super::{fmt_deg, parse_rotate_deg, set_style_prop, Tool, VellumApp};

// ═══════════════════════════ 1. Spec(纯数据) ═══════════════════════════

/// 控件种类(渲染器按此选择对应控件;数值框一律是升级版
/// `NumField`:scrubby + 表达式,design/03 §四 字段规则)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtlKind {
    /// 数值框(scrubby / 步进 / 表达式)
    Num,
    /// 颜色框(取色器浮窗 + var(--x))
    Color,
    /// 下拉(选项由渲染器按字段 id 提供)
    Combo,
    /// 单行文本输入(失焦提交)
    Text,
    /// 按钮(零参动作;写回走 app 命令 ID 或文档命令)
    Button,
    /// 提示文案(非交互;design/03 明示的"提示"类条目)
    Hint,
}

/// 字段写回命令。二者都是"命令路径":Agent 可复现 ——
/// `Doc` 走文档命令(patch 等价 op / Kiln 命令流),`App` 走
/// `run_command`(ID 注册于 commands.yaml 三处同步)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtlWrite {
    /// 文档命令(变体名 + 参数提示,如 "SetStyle(background-color)")
    Doc(&'static str),
    /// app 命令(commands.yaml 注册 ID,如 "view.fit")
    App(&'static str),
    /// 无(Hint 提示,非交互)
    None,
}

/// 一个字段:机器标识 + 标签 + 控件种类 + 写回命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CtlField {
    /// 机器标识(门 1 断言 / 渲染器取值-提交分发键;稳定不改)。
    pub id: &'static str,
    pub label: &'static str,
    pub kind: CtlKind,
    pub write: CtlWrite,
}

const fn f(id: &'static str, label: &'static str, kind: CtlKind, write: CtlWrite) -> CtlField {
    CtlField {
        id,
        label,
        kind,
        write,
    }
}

/// 控制面板 spec:一个工具态 + 字段列表。
#[derive(Debug, Clone, PartialEq)]
pub struct ControlPanelSpec {
    /// 工具态名(如 "select.transform";门 1 映射表的键)。
    pub state: &'static str,
    pub fields: Vec<CtlField>,
}

impl ControlPanelSpec {
    /// 字段 id 列表(测试与报告用)。
    pub fn ids(&self) -> Vec<&'static str> {
        self.fields.iter().map(|x| x.id).collect()
    }
}

/// 工具态判定的纯输入(从 `VellumApp` 每帧投影;测试可手工构造)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CtlCtx {
    pub tool: Tool,
    pub has_selection: bool,
}

/// 生成当前工具态的 spec(design/03 §三 八工具态字段表的代码化)。
///
/// 未列入 design/03 的工具(抓手/缩放/吸管/剪刀)给纯提示态,不放假控件;
/// 直线/编组选择复用最接近的已定义态(直线=椭圆态样式字段,编组选择=变换)。
pub fn spec_for(c: &CtlCtx) -> ControlPanelSpec {
    match c.tool {
        Tool::Select | Tool::GroupSelect => {
            if c.has_selection {
                transform_spec()
            } else {
                artboard_opts_spec()
            }
        }
        Tool::DirectSelect => ControlPanelSpec {
            state: "direct.anchor",
            fields: vec![
                f("ax", "锚点X", CtlKind::Num, CtlWrite::Doc("SetVector")),
                f("ay", "锚点Y", CtlKind::Num, CtlWrite::Doc("SetVector")),
                f(
                    "anchor.hint",
                    "拖画布锚点改位;手柄/转换点 → 阶段 2",
                    CtlKind::Hint,
                    CtlWrite::None,
                ),
            ],
        },
        Tool::Pen => ControlPanelSpec {
            state: "pen",
            fields: vec![
                f(
                    "pen.fill",
                    "填充",
                    CtlKind::Color,
                    CtlWrite::Doc("SetStyle(fill)"),
                ),
                f(
                    "pen.stroke",
                    "描边",
                    CtlKind::Color,
                    CtlWrite::Doc("SetStyle(stroke)"),
                ),
                f(
                    "pen.sw",
                    "粗细",
                    CtlKind::Num,
                    CtlWrite::Doc("SetStyle(stroke-width)"),
                ),
                f(
                    "pen.hint",
                    "点击落锚点 · 点击起点或 Enter 自动闭合",
                    CtlKind::Hint,
                    CtlWrite::None,
                ),
            ],
        },
        Tool::Rect => shape_spec(true),
        // 椭圆/直线:圆角是矩形专属(椭圆 50% 由创建层写;直线 2px 条)
        Tool::Ellipse | Tool::Line => shape_spec(false),
        Tool::Text => ControlPanelSpec {
            state: "text",
            fields: vec![
                f(
                    "t.size",
                    "字号",
                    CtlKind::Num,
                    CtlWrite::Doc("SetStyle(font-size)"),
                ),
                f(
                    "t.align",
                    "对齐",
                    CtlKind::Combo,
                    CtlWrite::Doc("SetStyle(text-align)"),
                ),
                f(
                    "t.color",
                    "颜色",
                    CtlKind::Color,
                    CtlWrite::Doc("SetStyle(color)"),
                ),
                f(
                    "text.hint",
                    "字符/段落面板:Ctrl+T / Ctrl+Alt+T",
                    CtlKind::Hint,
                    CtlWrite::None,
                ),
            ],
        },
        Tool::Gradient => ControlPanelSpec {
            state: "gradient",
            fields: vec![
                f(
                    "g.kind",
                    "类型",
                    CtlKind::Combo,
                    CtlWrite::Doc("SetStyle(background-image)"),
                ),
                f(
                    "g.angle",
                    "角度",
                    CtlKind::Num,
                    CtlWrite::Doc("SetStyle(background-image)"),
                ),
                f(
                    "g.reverse",
                    "反向",
                    CtlKind::Button,
                    CtlWrite::Doc("SetStyle(background-image)"),
                ),
                f(
                    "g.hint",
                    "色标编辑 → 阶段 4(05 外观/渐变)",
                    CtlKind::Hint,
                    CtlWrite::None,
                ),
            ],
        },
        Tool::Artboard => ControlPanelSpec {
            state: "artboard",
            fields: vec![
                f(
                    "ab.preset",
                    "预设",
                    CtlKind::Combo,
                    CtlWrite::Doc("SetGeom"),
                ),
                f("ab.name", "名称", CtlKind::Text, CtlWrite::Doc("Rename")),
                f("ab.x", "X", CtlKind::Num, CtlWrite::Doc("SetGeom")),
                f("ab.y", "Y", CtlKind::Num, CtlWrite::Doc("SetGeom")),
                f("ab.w", "W", CtlKind::Num, CtlWrite::Doc("SetGeom")),
                f("ab.h", "H", CtlKind::Num, CtlWrite::Doc("SetGeom")),
                f(
                    "ab.hint",
                    "适配内容 → 阶段 2(画板面板增强)",
                    CtlKind::Hint,
                    CtlWrite::None,
                ),
            ],
        },
        // design/03 §三 未定义的工具态:只提示,不放假控件
        Tool::Hand | Tool::Zoom => ControlPanelSpec {
            state: "view.hint",
            fields: vec![f(
                "view.hint",
                "抓手:拖动平移(Space 同) · 缩放:单击放大 / Alt+单击缩小 / 拖框 · Ctrl+0 适合窗口",
                CtlKind::Hint,
                CtlWrite::None,
            )],
        },
        Tool::Eyedropper => ControlPanelSpec {
            state: "eyedropper.hint",
            fields: vec![f(
                "eyedropper.hint",
                "吸管:单击对象取色应用到选区 · Alt+单击吸取全部样式",
                CtlKind::Hint,
                CtlWrite::None,
            )],
        },
        Tool::Scissors => ControlPanelSpec {
            state: "scissors.hint",
            fields: vec![f(
                "scissors.hint",
                "剪刀:在矢量路径的锚点上单击剪开(闭路开口 / 开路分段)",
                CtlKind::Hint,
                CtlWrite::None,
            )],
        },
    }
}

/// 选择·有选区 → 变换(design/03:X/Y/W/H/旋转 + 填充/不透明度 +
/// 对齐画板 + 编组/排列;倾斜 / 参考点九宫格 → 阶段 2,描边 → 05)。
fn transform_spec() -> ControlPanelSpec {
    ControlPanelSpec {
        state: "select.transform",
        fields: vec![
            f("x", "X", CtlKind::Num, CtlWrite::Doc("SetGeom")),
            f("y", "Y", CtlKind::Num, CtlWrite::Doc("SetGeom")),
            f("w", "W", CtlKind::Num, CtlWrite::Doc("SetGeom")),
            f("h", "H", CtlKind::Num, CtlWrite::Doc("SetGeom")),
            f(
                "rot",
                "∠",
                CtlKind::Num,
                CtlWrite::Doc("SetStyle(transform)"),
            ),
            f(
                "fill",
                "填充",
                CtlKind::Color,
                CtlWrite::Doc("SetStyle(background-color)"),
            ),
            f(
                "opacity",
                "不透明",
                CtlKind::Num,
                CtlWrite::Doc("SetStyle(opacity)"),
            ),
            f(
                "align.h",
                "⬌画板",
                CtlKind::Button,
                CtlWrite::App("align.hcenter"),
            ),
            f(
                "align.v",
                "⬍画板",
                CtlKind::Button,
                CtlWrite::App("align.vcenter"),
            ),
            f(
                "group",
                "编组",
                CtlKind::Button,
                CtlWrite::App("object.group"),
            ),
            f(
                "fwd",
                "前移",
                CtlKind::Button,
                CtlWrite::App("object.bring_forward"),
            ),
            f(
                "bwd",
                "后移",
                CtlKind::Button,
                CtlWrite::App("object.send_backward"),
            ),
        ],
    }
}

/// 选择·无选区 → 画板选项(预设 / 取向 / 尺寸 / 背景 / 画板数)。
fn artboard_opts_spec() -> ControlPanelSpec {
    ControlPanelSpec {
        state: "select.artboard",
        fields: vec![
            f(
                "ab.preset",
                "预设",
                CtlKind::Combo,
                CtlWrite::Doc("SetGeom"),
            ),
            f(
                "ab.orient",
                "取向",
                CtlKind::Combo,
                CtlWrite::Doc("SetGeom"),
            ),
            f("ab.w", "W", CtlKind::Num, CtlWrite::Doc("SetGeom")),
            f("ab.h", "H", CtlKind::Num, CtlWrite::Doc("SetGeom")),
            f(
                "ab.bg",
                "画板底",
                CtlKind::Color,
                CtlWrite::Doc("SetStyle(background-color)"),
            ),
            f("ab.add", "+画板", CtlKind::Button, CtlWrite::Doc("Insert")),
        ],
    }
}

/// 矩形 / 椭圆 / 直线 → 填充 / 描边 / 粗细 / 圆角(矩形;起始角 → 遗留项)。
fn shape_spec(radius: bool) -> ControlPanelSpec {
    let mut fields = vec![
        f(
            "sh.fill",
            "填充",
            CtlKind::Color,
            CtlWrite::Doc("SetStyle(background-color)"),
        ),
        f(
            "sh.stroke",
            "描边",
            CtlKind::Color,
            CtlWrite::Doc("SetStyle(border)"),
        ),
        f(
            "sh.sw",
            "粗细",
            CtlKind::Num,
            CtlWrite::Doc("SetStyle(border)"),
        ),
    ];
    if radius {
        fields.push(f(
            "sh.radius",
            "圆角",
            CtlKind::Num,
            CtlWrite::Doc("SetStyle(border-radius)"),
        ));
    }
    ControlPanelSpec {
        state: if radius { "rect" } else { "ellipse" },
        fields,
    }
}

/// 固定右侧区字段(任何工具态都在;不属于八态映射表,单列断言)。
pub const FIXED_FIELDS: [CtlField; 3] = [
    f(
        "doc.title",
        "标题",
        CtlKind::Text,
        CtlWrite::Doc("SetMetaTitle"),
    ),
    f(
        "ab.switch",
        "画板",
        CtlKind::Combo,
        CtlWrite::App("view.zoom_to_selection"),
    ),
    f("zoom", "缩放", CtlKind::Combo, CtlWrite::App("view.fit")),
];

// ═══════════════════ 2. 共享写回构建器(纯函数,门 2 测试走这里) ═══════════════════

/// 几何分量(geom_field_cmds 的替换目标)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeomAxis {
    X,
    Y,
    W,
    H,
}

/// 多选写回收敛:单条 → 原样;多条 → Compound(一次 undo);空 → None。
pub fn combine(cmds: Vec<Command>) -> Option<Command> {
    match cmds.len() {
        0 => None,
        1 => cmds.into_iter().next(),
        _ => Some(Command::Compound { cmds }),
    }
}

/// SetGeom:把选中对象的某一几何分量替换为 `v`(其余分量保持各自原值;
/// 多选 → Compound)。属性面板变换组与控制面板共用。
pub fn geom_field_cmds(
    doc: &vb_doc::model::Document,
    sids: &[String],
    axis: GeomAxis,
    v: f64,
) -> Vec<Command> {
    sids.iter()
        .filter_map(|sid| {
            let nid = doc.find_by_sid(sid)?;
            let mut g = doc.nodes.get(nid)?.geom;
            match axis {
                GeomAxis::X => g.x = v,
                GeomAxis::Y => g.y = v,
                GeomAxis::W => g.w = v.max(1.0),
                GeomAxis::H => g.h = v.max(1.0),
            }
            Some(Command::SetGeom {
                sid: sid.clone(),
                new: g,
                old: None,
                old_declared: None,
            })
        })
        .collect()
}

/// SetGeom:同时替换**同一对象**的多个几何分量(画板预设/取向等
/// 双分量编辑)。逐分量各建一条 SetGeom 会以同一基准互相覆盖 ——
/// 必须在一条命令里合并。
pub fn geom_axes_cmd(
    doc: &vb_doc::model::Document,
    sid: &str,
    sets: &[(GeomAxis, f64)],
) -> Option<Command> {
    let nid = doc.find_by_sid(sid)?;
    let mut g = doc.nodes.get(nid)?.geom;
    for (axis, v) in sets {
        match axis {
            GeomAxis::X => g.x = *v,
            GeomAxis::Y => g.y = *v,
            GeomAxis::W => g.w = v.max(1.0),
            GeomAxis::H => g.h = v.max(1.0),
        }
    }
    Some(Command::SetGeom {
        sid: sid.to_string(),
        new: g,
        old: None,
        old_declared: None,
    })
}

/// SetStyle:在每个选中对象**自身样式**上设一个属性(保留其余声明;
/// 多选 → Compound)。属性面板外观/布局/文本组与控制面板共用。
pub fn style_prop_cmds(
    doc: &vb_doc::model::Document,
    sids: &[String],
    prop: &str,
    value: &str,
) -> Vec<Command> {
    sids.iter()
        .filter_map(|sid| {
            let nid = doc.find_by_sid(sid)?;
            let style = doc.nodes.get(nid)?.style.clone();
            Some(Command::SetStyle {
                sid: sid.clone(),
                new: set_style_prop(style, prop, value),
                old: None,
            })
        })
        .collect()
}

/// SetStyle 属性删除(颜色框「清除」语义:声明整条移除;本无此声明
/// 的对象不产生命令)。
pub fn style_prop_remove_cmds(
    doc: &vb_doc::model::Document,
    sids: &[String],
    prop: &str,
) -> Vec<Command> {
    sids.iter()
        .filter_map(|sid| {
            let nid = doc.find_by_sid(sid)?;
            let style = doc.nodes.get(nid)?.style.clone();
            let mut next = style.clone();
            next.retain(|d: &Decl| d.prop != prop);
            if next.len() == style.len() {
                return None;
            }
            Some(Command::SetStyle {
                sid: sid.clone(),
                new: next,
                old: None,
            })
        })
        .collect()
}

/// SetAttrs:href / alt / aria-label / target;`value` 空 = 删除属性
/// (属性本就不存在时不产生命令)。
pub fn attr_cmds(
    doc: &vb_doc::model::Document,
    sids: &[String],
    key: &str,
    value: &str,
) -> Vec<Command> {
    sids.iter()
        .filter_map(|sid| {
            let nid = doc.find_by_sid(sid)?;
            let n = doc.nodes.get(nid)?;
            let mut merged: Vec<(String, String)> = n
                .attrs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if value.is_empty() {
                let before = merged.len();
                merged.retain(|(k, _)| k != key);
                if merged.len() == before {
                    return None;
                }
            } else {
                match merged.iter_mut().find(|(k, _)| k == key) {
                    Some(slot) => slot.1 = value.to_string(),
                    None => merged.push((key.to_string(), value.to_string())),
                }
            }
            Some(Command::SetAttrs {
                sid: sid.clone(),
                new: merged,
                old: None,
            })
        })
        .collect()
}

/// 旋转数值化:SetStyle `transform: rotate(<deg>deg)`(与画布角点
/// 旋转拖拽同一条写回路径;现模型 transform 只承载 rotate,倾斜 →
/// 阶段 2 时再改为合并写)。
pub fn rotation_cmds(doc: &vb_doc::model::Document, sids: &[String], deg: f64) -> Vec<Command> {
    let v = format!("rotate({}deg)", fmt_deg(deg));
    style_prop_cmds(doc, sids, "transform", &v)
}

/// 节点当前旋转角(style.transform;无声明 = 0)。
pub fn rotation_deg_of(style: &[Decl]) -> f64 {
    style
        .iter()
        .find(|d| d.prop == "transform")
        .and_then(|d| parse_rotate_deg(&d.value))
        .unwrap_or(0.0)
}

/// Rename:图层名(导出 → `data-vb-name`;导出组「名称」字段共用)。
pub fn rename_cmds(doc: &vb_doc::model::Document, sids: &[String], name: &str) -> Vec<Command> {
    sids.iter()
        .filter(|sid| doc.find_by_sid(sid).is_some())
        .map(|sid| Command::Rename {
            sid: sid.clone(),
            new: name.to_string(),
            old: None,
        })
        .collect()
}

// ────────────────── 渐变数值化(02-2:现有拖方向能力 + 角度/类型/反向) ──────────────────
//
// 阶段 4(05-2)把渐变**结构化模型**上收到 `vb_ui::gradient`(唯一真相:色标有
// 位置/显式标记/中点/不透明度)。本节的字符串级接口保留给控制面板的数值化
// 微调(角度/类型/反向),内部改为**委托**结构化解析,保证两处口径不会分叉。

/// 渐变类型(权威定义在 `vb_ui::gradient`;此处再导出以保持既有调用路径)。
pub use vb_ui::gradient::GradKind;

/// 解析 background-image 渐变值 → (类型, 角度, 色标串列表)。
///
/// 线性:`linear-gradient(45deg, #a 0%, #b 100%)`;径向:角度无意义(0),
/// 锚点段(`circle at 50% 50%`)保真留在 `stops[0]`。
pub fn parse_gradient(value: &str) -> Option<(GradKind, f64, Vec<String>)> {
    let g = vb_ui::gradient::parse(value)?;
    let mut stops: Vec<String> = Vec::new();
    if let Some(h) = &g.head {
        stops.push(h.clone());
    }
    // 段序 = 色标序;中点提示以裸百分比段紧跟其所属色标
    for (i, s) in g.stops.iter().enumerate() {
        stops.push(s.to_css());
        if let Some((_, p)) = g.hints.iter().find(|(hi, _)| *hi == i) {
            stops.push(format!("{}%", vb_common::units::fmt_num(p * 100.0)));
        }
    }
    Some((g.kind, g.angle, stops))
}

/// 拼回 background-image 值(径向固定 `circle at 50% 50%` 锚点)。
pub fn build_gradient(kind: GradKind, angle: f64, stops: &[String]) -> String {
    match kind {
        GradKind::Linear => {
            format!(
                "linear-gradient({}deg, {})",
                fmt_deg(angle),
                stops.join(", ")
            )
        }
        // stops[0] 是锚点段(缺省补 50% 50%);其余为色标
        GradKind::Radial => match stops.split_first() {
            Some((head, rest)) if head.trim().starts_with("circle") => {
                format!("radial-gradient({}, {})", head, rest.join(", "))
            }
            _ => format!("radial-gradient(circle at 50% 50%, {})", stops.join(", ")),
        },
    }
}

/// 渐变角度数值化:改写选中对象已有渐变的角度;无渐变时回退 =
/// 按该角度生成「现填充 → 白」双色渐变(与画布拖方向同一落点)。
pub fn gradient_angle_cmds(
    doc: &vb_doc::model::Document,
    sids: &[String],
    angle: f64,
) -> Vec<Command> {
    angle_or_kind_edit(doc, sids, Some(angle), None)
}

/// 渐变类型切换:线性 ↔ 径向(保留色标;径向锚点固定中心,
/// 线性角回退 90° = 自左向右,与画布默认拖向一致)。
pub fn gradient_kind_cmds(
    doc: &vb_doc::model::Document,
    sids: &[String],
    kind: GradKind,
) -> Vec<Command> {
    angle_or_kind_edit(doc, sids, None, Some(kind))
}

/// 渐变反向:线性 = 角度 +180(方向逆转,色标不动);径向 = 色标
/// 顺序逆序(位置跟着色标走,CSS 渲染端会归一化停点顺序)。
/// 无渐变的对象**跳过**(反向不无中生有)。
pub fn gradient_reverse_cmds(doc: &vb_doc::model::Document, sids: &[String]) -> Vec<Command> {
    edit_each_gradient(doc, sids, None, |kind, angle, stops| match kind {
        GradKind::Linear => build_gradient(kind, (angle + 180.0).rem_euclid(360.0), &stops),
        GradKind::Radial => {
            // stops[0] 是锚点段(circle at …):保持在首位,只逆序色标
            let (head, rest) = match stops.split_first() {
                Some((h, r)) if h.trim().starts_with("circle") => (Some(h), r),
                _ => (None, &stops[..]),
            };
            let mut rev = rest.to_vec();
            rev.reverse();
            let mut out = head.map(|h| vec![h.clone()]).unwrap_or_default();
            out.extend(rev);
            build_gradient(kind, angle, &out)
        }
    })
}

fn angle_or_kind_edit(
    doc: &vb_doc::model::Document,
    sids: &[String],
    angle: Option<f64>,
    kind: Option<GradKind>,
) -> Vec<Command> {
    // 回退生成规格(无既有渐变的对象):只改角度 → (线性, 该角度);
    // 切类型 → (目标类型, 线性默认 90°)
    let fallback = match (angle, kind) {
        (Some(a), _) => (GradKind::Linear, a),
        (None, Some(k)) => (k, 90.0),
        (None, None) => unreachable!("角度与类型至少给一个"),
    };
    edit_each_gradient(doc, sids, Some(fallback), |k, a, mut stops| {
        let nk = kind.unwrap_or(k);
        let na = match (angle, kind) {
            (Some(x), None) => x, // 只改角度
            (None, Some(GradKind::Linear)) => 90.0,
            (None, Some(GradKind::Radial)) | (Some(_), Some(_)) | (None, None) => a,
        };
        // 径向 → 线性:剥掉锚点段(stops[0] 的 circle at …)
        if nk == GradKind::Linear
            && k == GradKind::Radial
            && stops.first().map(|x| x.trim().starts_with("circle")) == Some(true)
        {
            stops.remove(0);
        }
        build_gradient(nk, na, &stops)
    })
}

/// 对每个选中对象做一次渐变值改写。
///
/// `fallback` = 对象**没有**渐变时的生成规格((类型, 角度);
/// `None` = 跳过无渐变对象,不无中生有)。回退生成的色标 =
/// 「现填充 → 白」,与画布拖方向写回(`apply_gradient_to_selection`)
/// 完全同构。
fn edit_each_gradient(
    doc: &vb_doc::model::Document,
    sids: &[String],
    fallback: Option<(GradKind, f64)>,
    rewrite: impl Fn(GradKind, f64, Vec<String>) -> String,
) -> Vec<Command> {
    sids.iter()
        .filter_map(|sid| {
            let nid = doc.find_by_sid(sid)?;
            let n = doc.nodes.get(nid)?;
            let cur = n
                .style
                .iter()
                .find(|d| d.prop == "background-image")
                .map(|d| d.value.clone());
            let value = match cur.as_deref().and_then(parse_gradient) {
                Some((k, a, stops)) => rewrite(k, a, stops),
                None => {
                    let (fk, fa) = fallback?;
                    let c1 = n
                        .fill_color()
                        .map(|c| c.to_shortest_hex())
                        .unwrap_or_else(|| "#d4d4d4".into()); // vb-token-ok: 文档内容色
                    let stops: Vec<String> =
                        [format!("{c1} 0%"), "#ffffff 100%".to_string()].to_vec();
                    build_gradient(fk, fa, &stops)
                }
            };
            Some(Command::SetStyle {
                sid: sid.clone(),
                new: set_style_prop(n.style.clone(), "background-image", &value),
                old: None,
            })
        })
        .collect()
}

// ═══════════════════════════ 3. 渲染器 ═══════════════════════════

// 画板尺寸预设(02-5-3):**常量单一来源 = `panels::artboards::AB_PRESETS`**
// (S1-d 起画板面板与本面板共用;七档 Web 1920/1440/1080、移动 750/375、
// A4 横竖 + 自定义)。PRESET_CUSTOM 用于尺寸不匹配任何行时的显示名。
use crate::app::panels::artboards::{AB_PRESETS, PRESET_CUSTOM};

impl VellumApp {
    /// 当前工具态 spec(把 VellumApp 状态投影成纯输入再查表)。
    pub(crate) fn current_spec(&self) -> ControlPanelSpec {
        spec_for(&CtlCtx {
            tool: self.tool,
            has_selection: !self.selection.is_empty(),
        })
    }

    /// 控制面板:菜单栏下 40px 通栏(egui 顶栏按 show 顺序堆叠,
    /// 本面板在 menu 之后 show,自然落在其下方)。
    pub(crate) fn control_bar(&mut self, ui: &mut egui::Ui) {
        let t = theme::tokens(ui.ctx());
        egui::Panel::top("control_bar")
            .exact_size(theme::space::CONTROL_BAR_HEIGHT)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(t.bg_panel)
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ui, |ui| {
                // 固定区预留宽:标题(130)+ 画板切换(170)+ 缩放(90)+ 分隔与间距
                const FIXED_RESERVE: f32 = 430.0;
                let avail = ui.available_width();
                let spec = self.current_spec();
                ui.horizontal(|ui| {
                    // ── 变量区(随工具态;过窄窗口横向滚动,不挤压固定区) ──
                    egui::ScrollArea::horizontal()
                        .auto_shrink([false, true])
                        .max_width((avail - FIXED_RESERVE).max(120.0))
                        .show(ui, |ui| {
                            ui.set_min_height(theme::space::ROW_HEIGHT);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = theme::space::S3;
                                for field in &spec.fields {
                                    self.control_field_ui(ui, field);
                                }
                                // 需要选区的态没有对象时给引导,不画死控件
                                // (不做"点了没反应")
                                if needs_selection(spec.state) && self.selection.is_empty() {
                                    ui.weak("先选中对象");
                                }
                            });
                        });
                    ui.separator();
                    // ── 固定右侧区(02-2-3) ──
                    ui.spacing_mut().item_spacing.x = theme::space::S3;
                    self.fixed_zone_ui(ui);
                });
            });
    }

    /// 单个字段的取值 + 控件 + 提交(按 id 分发;取值/写回都走 §2 构建器)。
    fn control_field_ui(&mut self, ui: &mut egui::Ui, field: &CtlField) {
        match field.kind {
            CtlKind::Hint => {
                ui.weak(field.label);
                return;
            }
            CtlKind::Button => {
                let clicked = ui.button(field.label).on_hover_text(field.id).clicked();
                if clicked {
                    match field.write {
                        CtlWrite::App(id) => self.run_command(id, false, false),
                        CtlWrite::Doc("Insert") => self.add_default_artboard(),
                        CtlWrite::Doc("SetStyle(background-image)") => {
                            // 渐变反向(线性 +180° / 径向色标逆序)
                            let sids = self.selection.clone();
                            let cmds = gradient_reverse_cmds(&self.doc, &sids);
                            if let Some(cmd) = combine(cmds) {
                                self.exec(cmd);
                                self.say("渐变已反向");
                            }
                        }
                        _ => {}
                    }
                }
                return;
            }
            _ => {}
        }
        match field.id {
            // ── 变换(design/03:选择·有选区) ──
            "x" | "y" | "w" | "h" => self.geom_field_ui(ui, field),
            "rot" => {
                let Some(v0) = self.primary_style().map(|s| rotation_deg_of(&s)) else {
                    return;
                };
                let mut v = v0;
                let r = NumField::new(field.label, &mut v)
                    .speed(1.0)
                    .step(15.0)
                    .label_width(20.0)
                    .width(56.0)
                    .unit("°")
                    .ui(ui);
                let cmd = r.changed.then(|| {
                    let sids = self.selection.clone();
                    combine(rotation_cmds(&self.doc, &sids, v))
                });
                self.num_commit(r, cmd.flatten());
            }
            "fill" | "pen.fill" | "sh.fill" | "t.color" => {
                let prop = match field.id {
                    "pen.fill" => "fill",
                    "t.color" => "color",
                    _ => "background-color",
                };
                self.color_field_ui(ui, field.label, prop);
            }
            "opacity" => {
                let v0 = self.primary_style_num("opacity").unwrap_or(1.0);
                let mut v = v0;
                let r = NumField::new(field.label, &mut v)
                    .speed(0.01)
                    .step(0.01)
                    .range(0.0, 1.0)
                    .label_width(44.0)
                    .width(56.0)
                    .ui(ui);
                let cmd = r.changed.then(|| {
                    let sids = self.selection.clone();
                    let cmds = style_prop_cmds(&self.doc, &sids, "opacity", &format!("{v:.2}"));
                    combine(cmds)
                });
                self.num_commit(r, cmd.flatten());
            }
            // ── 直接选择:锚点坐标 ──
            "ax" | "ay" => self.anchor_field_ui(ui, field),
            // ── 钢笔 / 形状 ──
            "pen.stroke" => self.color_field_ui(ui, field.label, "stroke"),
            "pen.sw" => self.style_num_field_ui(ui, field.label, "stroke-width", 0.1, 200.0),
            "sh.stroke" => self.border_color_ui(ui, field.label),
            "sh.sw" => self.border_width_ui(ui, field.label),
            "sh.radius" => self.style_num_field_ui(ui, field.label, "border-radius", 0.0, 4000.0),
            // ── 文字 ──
            "t.size" => self.style_num_field_ui(ui, field.label, "font-size", 1.0, 1000.0),
            "t.align" => self.text_align_ui(ui),
            // ── 渐变 ──
            "g.kind" => self.gradient_kind_ui(ui),
            "g.angle" => self.gradient_angle_ui(ui),
            // ── 画板选项 / 画板工具 ──
            "ab.preset" => self.artboard_preset_ui(ui),
            "ab.orient" => self.artboard_orient_ui(ui),
            "ab.w" | "ab.h" => self.artboard_size_ui(ui, field),
            "ab.x" | "ab.y" => self.artboard_pos_ui(ui, field),
            "ab.name" => self.artboard_name_ui(ui),
            "ab.bg" => self.artboard_bg_ui(ui),
            // Button / Hint 已在上面的分支处理;兜底不给交互控件
            "align.h" | "align.v" | "group" | "fwd" | "bwd" | "g.reverse" | "ab.add"
            | "anchor.hint" | "pen.hint" | "text.hint" | "g.hint" | "ab.hint" | "view.hint"
            | "eyedropper.hint" | "scissors.hint" => {}
            other => {
                ui.weak(other);
            }
        }
    }

    /// 变换 X/Y/W/H(主选中取值;多选逐对象替换该分量)。
    fn geom_field_ui(&mut self, ui: &mut egui::Ui, field: &CtlField) {
        let Some(sid) = self.selection.last().cloned() else {
            return;
        };
        let Some(nid) = self.doc.find_by_sid(&sid) else {
            return;
        };
        let g = self.doc.nodes.get(nid).unwrap().geom;
        let (ab_w, ab_h) = self.active_artboard_size();
        let axis = match field.id {
            "x" => GeomAxis::X,
            "y" => GeomAxis::Y,
            "w" => GeomAxis::W,
            _ => GeomAxis::H,
        };
        let mut v = match axis {
            GeomAxis::X => g.x,
            GeomAxis::Y => g.y,
            GeomAxis::W => g.w,
            GeomAxis::H => g.h,
        };
        let r = num_compact(ui, field.label, &mut v)
            .percent_base(if matches!(axis, GeomAxis::X | GeomAxis::W) {
                ab_w
            } else {
                ab_h
            })
            .range(0.0, 100000.0)
            .ui(ui);
        let cmd = r.changed.then(|| {
            let sids = self.selection.clone();
            combine(geom_field_cmds(&self.doc, &sids, axis, v))
        });
        self.num_commit(r, cmd.flatten());
    }

    /// 通用「样式数值字段」(描边粗细 / 圆角 / 字号)。
    fn style_num_field_ui(&mut self, ui: &mut egui::Ui, label: &str, prop: &str, lo: f64, hi: f64) {
        let Some(v0) = self.primary_style_num(prop) else {
            return;
        };
        let mut v = v0;
        let r = NumField::new(label, &mut v)
            .speed(1.0)
            .step(1.0)
            .range(lo, hi)
            .unit("px")
            .label_width(44.0)
            .width(56.0)
            .ui(ui);
        let cmd = r.changed.then(|| {
            let sids = self.selection.clone();
            let cmds = style_prop_cmds(&self.doc, &sids, prop, &format!("{}px", v as i64));
            combine(cmds)
        });
        self.num_commit(r, cmd.flatten());
    }

    /// 颜色字段(填充 / 描边 / 字色):取色器浮窗 + var(--x) +
    /// 清除 = 整条声明移除(写回一律经 style_prop_cmds)。
    fn color_field_ui(&mut self, ui: &mut egui::Ui, label: &str, prop: &str) {
        let Some(style) = self.primary_style() else {
            return;
        };
        let cur = style
            .iter()
            .find(|d| d.prop == prop)
            .and_then(|d| vb_common::color::parse_color(&d.value));
        let mut col = cur
            .map(|c| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
            .unwrap_or(Color32::WHITE);
        let tokens = self.doc.tokens.clone();
        let r = ColorField::new(label, &mut col).doc_tokens(&tokens).ui(ui);
        if let Some(name) = r.var_picked {
            let sids = self.selection.clone();
            let cmds = style_prop_cmds(&self.doc, &sids, prop, &format!("var(--{name})"));
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
                self.say(format!("{label} → var(--{name})"));
            }
        } else if r.cleared {
            let sids = self.selection.clone();
            let cmds = style_prop_remove_cmds(&self.doc, &sids, prop);
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
                self.say(format!("{label} 已清除"));
            }
        } else if r.changed {
            let [cr, cg, cb, ca] = col.to_srgba_unmultiplied();
            let hex = vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex();
            let sids = self.selection.clone();
            let cmds = style_prop_cmds(&self.doc, &sids, prop, &hex);
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
            }
        }
    }

    /// 描边色(border 简写 `{w}px solid {color}`;无边框以 1px 起步;
    /// 简写不支持 var()/清除 —— 未变即不写)。
    fn border_color_ui(&mut self, ui: &mut egui::Ui, label: &str) {
        let Some((w, col)) = self.primary_border() else {
            return;
        };
        let mut c = col
            .map(|x| Color32::from_rgba_unmultiplied(x.r, x.g, x.b, x.a))
            .unwrap_or(Color32::BLACK);
        let tokens = self.doc.tokens.clone();
        let r = ColorField::new(label, &mut c).doc_tokens(&tokens).ui(ui);
        if r.var_picked.is_some() || r.cleared || !r.changed {
            return;
        }
        let [cr, cg, cb, ca] = c.to_srgba_unmultiplied();
        let hex = vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex();
        let sids = self.selection.clone();
        let cmds = style_prop_cmds(
            &self.doc,
            &sids,
            "border",
            &format!("{}px solid {hex}", w as i64),
        );
        if let Some(cmd) = combine(cmds) {
            self.exec(cmd);
        }
    }

    /// 描边粗细(border 简写的宽度分量;沿用当前描边色)。
    fn border_width_ui(&mut self, ui: &mut egui::Ui, label: &str) {
        let Some((w, col)) = self.primary_border() else {
            return;
        };
        let mut v = w;
        let r = NumField::new(label, &mut v)
            .speed(0.5)
            .step(1.0)
            .range(0.0, 100.0)
            .unit("px")
            .label_width(44.0)
            .width(56.0)
            .ui(ui);
        let cmd = r.changed.then(|| {
            let hex = col
                .map(|c| c.to_shortest_hex())
                .unwrap_or_else(|| "#1a1a1a".into()); // vb-token-ok: 文档内容色
            let sids = self.selection.clone();
            let cmds = style_prop_cmds(
                &self.doc,
                &sids,
                "border",
                &format!("{}px solid {hex}", v as i64),
            );
            combine(cmds)
        });
        self.num_commit(r, cmd.flatten());
    }

    /// 文字对齐(text-align;真实 CSS,浏览器校对可见;画布为近似渲染)。
    fn text_align_ui(&mut self, ui: &mut egui::Ui) {
        let cur = self
            .primary_style()
            .and_then(|s| {
                s.iter()
                    .find(|d| d.prop == "text-align")
                    .map(|d| d.value.clone())
            })
            .unwrap_or_else(|| "left".into());
        let mut sel = cur.clone();
        egui::ComboBox::from_id_salt("ctl_text_align")
            .selected_text(format!("对齐 {sel}"))
            .show_ui(ui, |ui| {
                for v in ["left", "center", "right", "justify"] {
                    ui.selectable_value(&mut sel, v.to_string(), v);
                }
            });
        if sel != cur {
            let sids = self.selection.clone();
            let cmds = style_prop_cmds(&self.doc, &sids, "text-align", &sel);
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
            }
        }
    }

    /// 渐变类型(线性 ↔ 径向,保留色标)。
    fn gradient_kind_ui(&mut self, ui: &mut egui::Ui) {
        let cur_kind = self.primary_gradient().map(|(k, _, _)| k);
        let mut sel = cur_kind.unwrap_or(GradKind::Linear);
        egui::ComboBox::from_id_salt("ctl_g_kind")
            .selected_text(match sel {
                GradKind::Linear => "线性",
                GradKind::Radial => "径向",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut sel, GradKind::Linear, "线性");
                ui.selectable_value(&mut sel, GradKind::Radial, "径向");
            });
        if cur_kind == Some(sel) {
            return;
        }
        let sids = self.selection.clone();
        let cmds = gradient_kind_cmds(&self.doc, &sids, sel);
        if let Some(cmd) = combine(cmds) {
            self.exec(cmd);
        }
    }

    /// 渐变角度数值化(在现有拖方向能力之上;无渐变回退生成双色)。
    fn gradient_angle_ui(&mut self, ui: &mut egui::Ui) {
        let Some((_, a0, _)) = self.primary_gradient() else {
            return;
        };
        let mut v = a0;
        let r = NumField::new("角度", &mut v)
            .speed(1.0)
            .step(15.0)
            .range(0.0, 360.0)
            .unit("°")
            .label_width(44.0)
            .width(56.0)
            .ui(ui);
        let cmd = r.changed.then(|| {
            let sids = self.selection.clone();
            let cmds = gradient_angle_cmds(&self.doc, &sids, v);
            combine(cmds)
        });
        self.num_commit(r, cmd.flatten());
    }

    /// 画板预设下拉(改活动画板 W/H;匹配当前尺寸时高亮显示名称)。
    fn artboard_preset_ui(&mut self, ui: &mut egui::Ui) {
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let (w, h, sid) = {
            let n = self.doc.nodes.get(ab).unwrap();
            (n.geom.w, n.geom.h, n.sid.as_str().to_string())
        };
        let label = AB_PRESETS
            .iter()
            .find(|(_, pw, ph)| (*pw - w).abs() < 0.5 && (*ph - h).abs() < 0.5)
            .map(|(n, _, _)| *n)
            .unwrap_or(PRESET_CUSTOM);
        egui::ComboBox::from_id_salt("ctl_ab_preset")
            .selected_text(format!("预设 {label}"))
            .show_ui(ui, |ui| {
                for (name, pw, ph) in AB_PRESETS {
                    let active = (pw - w).abs() < 0.5 && (ph - h).abs() < 0.5;
                    if ui.selectable_label(active, name).clicked() {
                        if let Some(cmd) =
                            geom_axes_cmd(&self.doc, &sid, &[(GeomAxis::W, pw), (GeomAxis::H, ph)])
                        {
                            self.exec(cmd);
                            self.say(format!("画板预设 → {name}"));
                        }
                    }
                }
            });
    }

    /// 画板取向(横/竖:w、h 互换,经 SetGeom)。
    fn artboard_orient_ui(&mut self, ui: &mut egui::Ui) {
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let (w, h, sid) = {
            let n = self.doc.nodes.get(ab).unwrap();
            (n.geom.w, n.geom.h, n.sid.as_str().to_string())
        };
        let mut sel = if w >= h { "横" } else { "竖" };
        let before = sel;
        egui::ComboBox::from_id_salt("ctl_ab_orient")
            .selected_text(format!("取向 {sel}"))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut sel, "横", "横向");
                ui.selectable_value(&mut sel, "竖", "纵向");
            });
        if sel != before {
            if let Some(cmd) = geom_axes_cmd(
                &self.doc,
                &sid,
                &[(GeomAxis::W, h.max(1.0)), (GeomAxis::H, w.max(1.0))],
            ) {
                self.exec(cmd);
                self.say(format!(
                    "画板取向 → {}",
                    if sel == "横" { "横向" } else { "纵向" }
                ));
            }
        }
    }

    /// 画板尺寸 W/H(活动画板;SetGeom)。
    fn artboard_size_ui(&mut self, ui: &mut egui::Ui, field: &CtlField) {
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let (g, sid) = {
            let n = self.doc.nodes.get(ab).unwrap();
            (n.geom, n.sid.as_str().to_string())
        };
        let is_w = field.id == "ab.w";
        let mut v = if is_w { g.w } else { g.h };
        let r = NumField::new(field.label, &mut v)
            .speed(1.0)
            .step(1.0)
            .range(1.0, 100000.0)
            .label_width(20.0)
            .width(56.0)
            .percent_base(if is_w { g.w.max(1.0) } else { g.h.max(1.0) })
            .ui(ui);
        let cmd = r.changed.then(|| {
            let axis = if is_w { GeomAxis::W } else { GeomAxis::H };
            combine(geom_field_cmds(&self.doc, &[sid], axis, v))
        });
        self.num_commit(r, cmd.flatten());
    }

    /// 画板位置 X/Y(画板 geom 即世界坐标)。
    fn artboard_pos_ui(&mut self, ui: &mut egui::Ui, field: &CtlField) {
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let (g, sid) = {
            let n = self.doc.nodes.get(ab).unwrap();
            (n.geom, n.sid.as_str().to_string())
        };
        let is_x = field.id == "ab.x";
        let mut v = if is_x { g.x } else { g.y };
        let r = num_compact(ui, field.label, &mut v).ui(ui);
        let cmd = r.changed.then(|| {
            let axis = if is_x { GeomAxis::X } else { GeomAxis::Y };
            combine(geom_field_cmds(&self.doc, &[sid], axis, v))
        });
        self.num_commit(r, cmd.flatten());
    }

    /// 画板名称(Rename;画板工具态)。
    fn artboard_name_ui(&mut self, ui: &mut egui::Ui) {
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let (sid, name) = {
            let n = self.doc.nodes.get(ab).unwrap();
            (n.sid.as_str().to_string(), n.name.clone())
        };
        let mut buf = name.clone();
        if ui
            .add_sized([120.0, 18.0], egui::TextEdit::singleline(&mut buf))
            .lost_focus()
            && buf != name
            && !buf.trim().is_empty()
        {
            let cmds = rename_cmds(&self.doc, &[sid], buf.trim());
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
                self.say(format!("画板已改名 → {}", buf.trim()));
            }
        }
    }

    /// 画板背景色(活动画板节点的 background-color;SetStyle)。
    fn artboard_bg_ui(&mut self, ui: &mut egui::Ui) {
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let (style, sid) = {
            let n = self.doc.nodes.get(ab).unwrap();
            (n.style.clone(), n.sid.as_str().to_string())
        };
        let cur = style
            .iter()
            .find(|d| d.prop == "background-color")
            .and_then(|d| vb_common::color::parse_color(&d.value));
        let mut col = cur
            .map(|c| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
            .unwrap_or(Color32::WHITE);
        let tokens = self.doc.tokens.clone();
        let r = ColorField::new("画板底", &mut col)
            .doc_tokens(&tokens)
            .ui(ui);
        if let Some(name) = r.var_picked {
            let cmds = style_prop_cmds(
                &self.doc,
                &[sid],
                "background-color",
                &format!("var(--{name})"),
            );
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
                self.say(format!("画板底 → var(--{name})"));
            }
        } else if r.cleared {
            let cmds = style_prop_remove_cmds(&self.doc, &[sid], "background-color");
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
                self.say("画板底已清除");
            }
        } else if r.changed {
            let [cr, cg, cb, ca] = col.to_srgba_unmultiplied();
            let hex = vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex();
            let cmds = style_prop_cmds(&self.doc, &[sid], "background-color", &hex);
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
            }
        }
    }

    /// 锚点坐标(直接选择:当前锚点 = 拖拽/点选中的,否则选中矢量第 0 个;
    /// 写回 SetVector,与画布锚点拖拽同一路径)。
    fn anchor_field_ui(&mut self, ui: &mut egui::Ui, field: &CtlField) {
        let Some((sid, vi)) = self.anchor_target() else {
            ui.weak("选中矢量路径后可改锚点坐标");
            return;
        };
        let Some(nid) = self.doc.find_by_sid(&sid) else {
            return;
        };
        let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) else {
            return;
        };
        let Some((_, wx, wy)) = self
            .vector_vertices(&sid)
            .into_iter()
            .find(|(i, _, _)| *i == vi)
        else {
            return;
        };
        let is_x = field.id == "ax";
        let mut v = if is_x { wx } else { wy };
        let r = num_compact(ui, field.label, &mut v).speed(1.0).ui(ui);
        let cmd = r.changed.then(|| {
            let local = if is_x {
                (v - bb.x0, wy - bb.y0)
            } else {
                (wx - bb.x0, v - bb.y0)
            };
            self.build_anchor_cmd(&sid, vi, local)
        });
        self.num_commit(r, cmd.flatten());
    }

    /// 固定右侧区:文档标题(改名经 SetMetaTitle,导出写 `<title>`)+
    /// 画板切换下拉(联动画布:选中 + 缩放到选区)+ 缩放下拉
    /// (全部走既有 view.* 命令 ID;浏览器校对按钮 → 阶段 8 遗留)。
    fn fixed_zone_ui(&mut self, ui: &mut egui::Ui) {
        // 文档标题(真实写文档:SetMetaTitle)
        let mut title = self.doc.meta.title.clone();
        ui.label("标题");
        if ui
            .add_sized([110.0, 18.0], egui::TextEdit::singleline(&mut title))
            .lost_focus()
            && !title.trim().is_empty()
            && title.trim() != self.doc.meta.title
        {
            let new = title.trim().to_string();
            self.exec(Command::SetMetaTitle { new, old: None });
            self.say(format!("文档标题 → {}", title.trim()));
        }
        // 画板切换下拉(联动画布)
        let idx = self
            .selection
            .first()
            .and_then(|s| self.doc.find_by_sid(s))
            .and_then(|id| self.doc.artboards.iter().position(|&a| a == id));
        let cur_name = idx
            .map(|i| {
                self.doc
                    .nodes
                    .get(self.doc.artboards[i])
                    .unwrap()
                    .name
                    .clone()
            })
            .unwrap_or_else(|| format!("共 {} 块", self.doc.artboards.len()));
        egui::ComboBox::from_id_salt("ctl_ab_switch")
            .selected_text(format!(
                "{}/{} {}",
                idx.map(|i| i + 1).unwrap_or(0),
                self.doc.artboards.len(),
                short_name(&cur_name, 8)
            ))
            .show_ui(ui, |ui| {
                for (i, &ab) in self.doc.artboards.clone().iter().enumerate() {
                    let n = self.doc.nodes.get(ab).unwrap();
                    let selected = idx == Some(i);
                    if ui
                        .selectable_label(
                            selected,
                            format!("{} {}×{}", n.name, n.geom.w as i64, n.geom.h as i64),
                        )
                        .clicked()
                    {
                        self.selection = vec![n.sid.as_str().to_string()];
                        // 联动画布:选中后视图居中(既有命令路径)
                        self.run_command("view.zoom_to_selection", false, false);
                    }
                }
            });
        // 缩放下拉(全部既有命令 ID)
        let pct = (self.camera.zoom * 100.0).round() as i64;
        egui::ComboBox::from_id_salt("ctl_zoom")
            .selected_text(format!("{pct}%"))
            .show_ui(ui, |ui| {
                if ui.selectable_label(false, "适合窗口").clicked() {
                    self.run_command("view.fit", false, false);
                    ui.close();
                }
                if ui.selectable_label(false, "100%").clicked() {
                    self.run_command("view.actual_size", false, false);
                    ui.close();
                }
                if ui.selectable_label(false, "放大一档").clicked() {
                    self.run_command("view.zoom_in", false, false);
                    ui.close();
                }
                if ui.selectable_label(false, "缩小一档").clicked() {
                    self.run_command("view.zoom_out", false, false);
                    ui.close();
                }
            });
        // 浏览器校对按钮:design/03 §三 称"本产品独有",但现有命令层
        // 无可复用实现(vb_browser 仅 CLI 侧)—— 不放假按钮,登记阶段 8。
    }

    // ── 取值辅助(主选中快照) ──

    fn primary_style(&self) -> Option<Vec<Decl>> {
        let sid = self.selection.last()?;
        let nid = self.doc.find_by_sid(sid)?;
        Some(self.doc.nodes.get(nid)?.style.clone())
    }

    fn primary_style_num(&self, prop: &str) -> Option<f64> {
        let style = self.primary_style()?;
        let d = style.iter().find(|d| d.prop == prop)?;
        let v: f64 = d.value.trim_end_matches("px").trim().parse().ok()?;
        Some(v)
    }

    /// 主选中 border 简写 → (宽, 颜色);无边框 = (1, None)。
    fn primary_border(&self) -> Option<(f64, Option<vb_common::Rgba>)> {
        let style = self.primary_style()?;
        match style.iter().find(|d| d.prop == "border") {
            Some(d) => {
                let mut it = d.value.split_whitespace();
                let w: f64 = it
                    .next()
                    .and_then(|s| s.trim_end_matches("px").parse().ok())
                    .unwrap_or(1.0);
                let col = it.find_map(vb_common::color::parse_color);
                Some((w, col))
            }
            None => Some((1.0, None)),
        }
    }

    /// 主选中渐变 → (类型, 角度, 色标)。
    fn primary_gradient(&self) -> Option<(GradKind, f64, Vec<String>)> {
        let style = self.primary_style()?;
        let d = style.iter().find(|d| d.prop == "background-image")?;
        parse_gradient(&d.value)
    }

    /// 直接选择的锚点目标:(sid, 顶点序号)。
    /// 拖拽/点选中的锚点优先;否则取首个选中矢量的第 0 个锚点。
    fn anchor_target(&self) -> Option<(String, usize)> {
        if let Some((sid, vi)) = &self.ds_vertex {
            return Some((sid.clone(), *vi));
        }
        let sid = self.selection.last()?;
        self.vector_vertices(sid)
            .first()
            .map(|(i, _, _)| (sid.clone(), *i))
    }
}

/// 需要选区才有意义的工具态(无选区时渲染器给引导文案而非死控件)。
fn needs_selection(state: &str) -> bool {
    matches!(
        state,
        "select.transform" | "gradient" | "text" | "pen" | "rect" | "ellipse"
    )
}

/// 控制面板紧凑数值框(单行 40px 条:窄标签 + 56px 输入)。
fn num_compact<'a>(_: &mut egui::Ui, label: &'a str, v: &'a mut f64) -> NumField<'a> {
    let w = if label.chars().count() <= 2 {
        20.0
    } else {
        44.0
    };
    NumField::new(label, v)
        .speed(1.0)
        .label_width(w)
        .width(56.0)
}

/// 名称截断(下拉选中态避免撑爆 40px 条)。
fn short_name(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let t: String = s.chars().take(max_chars).collect();
        format!("{t}…")
    }
}

// ═══════════════════════════ 4. 测试 ═══════════════════════════

#[cfg(test)]
mod tests {
    use vb_doc::model::{Document, Geom, Node, NodeKind, NodeTree};
    use vb_doc::undo::UndoStack;

    use super::*;

    // ───────────── 门 1:八工具态字段映射表(design/03 §三) ─────────────

    /// 工具态 → 字段清单映射表(门 1 断言的真相表,与 02c 报告同源)。
    fn expect_table() -> Vec<(&'static str, Vec<&'static str>)> {
        vec![
            (
                // 选择·无选区 → 画板选项:预设/取向/尺寸/背景/画板数(+画板)
                "select.artboard",
                vec!["ab.preset", "ab.orient", "ab.w", "ab.h", "ab.bg", "ab.add"],
            ),
            (
                // 选择·有选区 → 变换:X/Y/W/H/∠ + 填充/不透明 + 对齐画板 + 编组/排列
                "select.transform",
                vec![
                    "x", "y", "w", "h", "rot", "fill", "opacity", "align.h", "align.v", "group",
                    "fwd", "bwd",
                ],
            ),
            (
                // 直接选择 → 锚点坐标
                "direct.anchor",
                vec!["ax", "ay", "anchor.hint"],
            ),
            (
                // 钢笔 → 填充/描边/粗细 + 自动闭合提示
                "pen",
                vec!["pen.fill", "pen.stroke", "pen.sw", "pen.hint"],
            ),
            (
                // 矩形 → 填充/描边/粗细/圆角(起始角 → 遗留项)
                "rect",
                vec!["sh.fill", "sh.stroke", "sh.sw", "sh.radius"],
            ),
            (
                // 椭圆 → 填充/描边/粗细(圆角是矩形的)
                "ellipse",
                vec!["sh.fill", "sh.stroke", "sh.sw"],
            ),
            (
                // 文字 → 字号/对齐/颜色(字体族 → 阶段 3)
                "text",
                vec!["t.size", "t.align", "t.color", "text.hint"],
            ),
            (
                // 渐变 → 类型/角度/反向(色标编辑 → 阶段 4)
                "gradient",
                vec!["g.kind", "g.angle", "g.reverse", "g.hint"],
            ),
            (
                // 画板工具 → 预设/名称/尺寸/位置(适配内容 → 阶段 2)
                "artboard",
                vec![
                    "ab.preset",
                    "ab.name",
                    "ab.x",
                    "ab.y",
                    "ab.w",
                    "ab.h",
                    "ab.hint",
                ],
            ),
        ]
    }

    fn ctx(tool: Tool, has_selection: bool) -> CtlCtx {
        CtlCtx {
            tool,
            has_selection,
        }
    }

    /// 门 1(逐态):spec_for 的输出与 design/03 §三 字段表一一对应。
    #[test]
    fn eight_tool_states_match_design03_field_table() {
        let cases = vec![
            ctx(Tool::Select, false),
            ctx(Tool::Select, true),
            ctx(Tool::DirectSelect, true),
            ctx(Tool::Pen, false),
            ctx(Tool::Rect, false),
            ctx(Tool::Ellipse, false),
            ctx(Tool::Text, false),
            ctx(Tool::Gradient, true),
            ctx(Tool::Artboard, false),
        ];
        for c in &cases {
            let spec = spec_for(c);
            let expect = expect_table()
                .into_iter()
                .find(|(s, _)| *s == spec.state)
                .unwrap_or_else(|| panic!("态 {} 不在映射表", spec.state));
            assert_eq!(
                spec.ids(),
                expect.1,
                "工具态 {} 的字段集合与 design/03 §三 映射表不一致",
                spec.state
            );
        }
    }

    /// 门 1(无遗漏):映射表里的每个态都由 spec_for 实际产出。
    #[test]
    fn mapping_table_states_are_all_reachable() {
        let produced: Vec<&str> = [
            ctx(Tool::Select, false),
            ctx(Tool::Select, true),
            ctx(Tool::DirectSelect, true),
            ctx(Tool::Pen, false),
            ctx(Tool::Rect, false),
            ctx(Tool::Ellipse, false),
            ctx(Tool::Text, false),
            ctx(Tool::Gradient, true),
            ctx(Tool::Artboard, false),
        ]
        .iter()
        .map(spec_for)
        .map(|s| s.state)
        .collect();
        for (state, _) in expect_table() {
            assert!(
                produced.contains(&state),
                "映射表态 {state} 未被任何工具态产出"
            );
        }
    }

    /// 门 1(回落与提示态):直线 = 椭圆态字段;编组选择 = 变换态;
    /// 抓手/缩放/吸管/剪刀只给提示,无假控件。
    #[test]
    fn fallback_states_reuse_nearest_defined_spec() {
        assert_eq!(spec_for(&ctx(Tool::Line, false)).state, "ellipse");
        assert_eq!(
            spec_for(&ctx(Tool::GroupSelect, true)).state,
            "select.transform"
        );
        for t in [Tool::Hand, Tool::Zoom, Tool::Eyedropper, Tool::Scissors] {
            let spec = spec_for(&ctx(t, false));
            assert!(
                spec.fields.iter().all(|x| x.kind == CtlKind::Hint),
                "{} 态只允许提示字段",
                spec.state
            );
        }
    }

    /// 固定右侧区:文档标题 / 画板切换 / 缩放 三字段恒在(设计 §三 尾注)。
    #[test]
    fn fixed_zone_has_title_artboard_switch_zoom() {
        let ids: Vec<&str> = FIXED_FIELDS.iter().map(|x| x.id).collect();
        assert_eq!(ids, vec!["doc.title", "ab.switch", "zoom"]);
        assert_eq!(FIXED_FIELDS[0].write, CtlWrite::Doc("SetMetaTitle"));
        assert_eq!(
            FIXED_FIELDS[1].write,
            CtlWrite::App("view.zoom_to_selection")
        );
    }

    /// 渐变解析/拼回:线性、径向、rgb() 色标的顶层逗号拆分。
    #[test]
    fn gradient_parse_build_roundtrip() {
        let (k, a, stops) =
            parse_gradient("linear-gradient(45deg, #ff5a1f 0%, #ffffff 100%)").unwrap();
        assert_eq!(k, GradKind::Linear);
        assert!((a - 45.0).abs() < 1e-9);
        assert_eq!(stops, vec!["#ff5a1f 0%", "#ffffff 100%"]);
        assert_eq!(
            build_gradient(k, a, &stops),
            "linear-gradient(45deg, #ff5a1f 0%, #ffffff 100%)"
        );
        let (k, _, stops) =
            parse_gradient("radial-gradient(circle at 50% 50%, rgb(255, 0, 0) 0%, #00ff00 100%)")
                .unwrap();
        assert_eq!(k, GradKind::Radial);
        assert_eq!(stops.len(), 3, "锚点段 + 2 色标");
        assert_eq!(stops[0], "circle at 50% 50%", "径向锚点段保留在 stops[0]");
        assert_eq!(stops[1], "rgb(255, 0, 0) 0%", "rgb() 内逗号不得拆分色标");
        assert!(parse_gradient("solid #fff").is_none());
        // 径向 → 线性:锚点段剥掉,色标保留
        let (_, a, stops) =
            parse_gradient("radial-gradient(circle at 35% 35%, #ff0000 0%, #00ff00 100%)").unwrap();
        let mut from_radial = stops.clone();
        from_radial.remove(0);
        assert_eq!(
            build_gradient(GradKind::Linear, 90.0, &from_radial),
            "linear-gradient(90deg, #ff0000 0%, #00ff00 100%)"
        );
        let _ = a;
    }

    // ───────────── 门 2:文档状态级(经命令路径) ─────────────

    /// 夹具:默认文档 + 一个盒子;返回 (doc, sid)。
    fn box_doc() -> (Document, String) {
        let mut doc = Document::new_default();
        let ab = doc.artboards.first().copied().unwrap();
        let sid = doc.alloc_sid();
        let mut n = Node::new(NodeKind::Box, "盒子", sid.clone());
        n.geom = Geom {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 50.0,
        };
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(ab);
        doc.nodes.get_mut(ab).unwrap().children.push(id);
        (doc, sid.as_str().to_string())
    }

    fn push(stack: &mut UndoStack, doc: &mut Document, cmd: Command) {
        stack.push(doc, cmd).expect("命令应成功应用");
    }

    fn style_of<'a>(doc: &'a Document, sid: &str) -> &'a Vec<Decl> {
        &doc.nodes.get(doc.find_by_sid(sid).unwrap()).unwrap().style
    }

    fn decl<'a>(style: &'a [Decl], prop: &str) -> &'a str {
        &style.iter().find(|d| d.prop == prop).unwrap().value
    }

    /// 门 2(变换):X/Y/W/H 逐分量写回只动该分量;undo 精确逆回。
    #[test]
    fn geom_field_writes_single_axis_and_reverts() {
        let (mut doc, sid) = box_doc();
        let mut stack = UndoStack::new();
        let sids = vec![sid.clone()];
        for (axis, v) in [
            (GeomAxis::X, 42.0),
            (GeomAxis::Y, 43.0),
            (GeomAxis::W, 200.0),
            (GeomAxis::H, 80.0),
        ] {
            for cmd in geom_field_cmds(&doc, &sids, axis, v) {
                push(&mut stack, &mut doc, cmd);
            }
        }
        let g = &doc.nodes.get(doc.find_by_sid(&sid).unwrap()).unwrap().geom;
        assert_eq!((g.x, g.y, g.w, g.h), (42.0, 43.0, 200.0, 80.0));
        while stack.undo(&mut doc).unwrap().is_some() {}
        let g = &doc.nodes.get(doc.find_by_sid(&sid).unwrap()).unwrap().geom;
        assert_eq!(
            (g.x, g.y, g.w, g.h),
            (10.0, 20.0, 100.0, 50.0),
            "undo 应逆回"
        );
    }

    /// 门 2(变换 ∠):旋转数值化写 transform: rotate(Ndeg);undo 还原。
    #[test]
    fn rotation_numeric_write_and_revert() {
        let (mut doc, sid) = box_doc();
        let mut stack = UndoStack::new();
        let sids = vec![sid.clone()];
        for cmd in rotation_cmds(&doc, &sids, 37.5) {
            push(&mut stack, &mut doc, cmd);
        }
        assert_eq!(decl(style_of(&doc, &sid), "transform"), "rotate(37.5deg)");
        assert!((rotation_deg_of(style_of(&doc, &sid)) - 37.5).abs() < 1e-9);
        stack.undo(&mut doc).unwrap();
        assert!(
            style_of(&doc, &sid).iter().all(|d| d.prop != "transform"),
            "undo 后 transform 声明应消失"
        );
    }

    /// 门 2(外观):填充/不透明度写 CSS;清除 = 移除声明;
    /// 多选 Compound 一条 undo 同时逆回两个对象(拉开合并窗口使
    /// 「填充」「不透明度」为独立条目,断言不受合并语义干扰)。
    #[test]
    fn appearance_fill_opacity_and_multi_select_compound() {
        let (mut doc, sid) = box_doc();
        // 第二个盒子(多选)
        let ab = doc.artboards.first().copied().unwrap();
        let sid2 = doc.alloc_sid();
        let n = Node::new(NodeKind::Box, "盒子2", sid2.clone());
        let id2 = doc.nodes.insert(n);
        doc.nodes.get_mut(id2).unwrap().parent = Some(ab);
        doc.nodes.get_mut(ab).unwrap().children.push(id2);
        let sids = vec![sid.clone(), sid2.as_str().to_string()];
        let mut stack = UndoStack::new();

        // vb-token-ok: 文档内容测试数据(非 UI 皮肤色)
        let cmd = combine(style_prop_cmds(&doc, &sids, "background-color", "#ff5a1f")).unwrap();
        push(&mut stack, &mut doc, cmd);
        // 拉开 >500ms 合并窗口:使「不透明度」成为独立 undo 条目
        std::thread::sleep(std::time::Duration::from_millis(600));
        let cmd = combine(style_prop_cmds(&doc, &sids, "opacity", "0.5")).unwrap();
        push(&mut stack, &mut doc, cmd);
        // vb-token-ok: 文档内容测试数据
        assert_eq!(decl(style_of(&doc, &sid), "background-color"), "#ff5a1f");
        assert_eq!(decl(style_of(&doc, &sid), "opacity"), "0.5");
        assert_eq!(decl(style_of(&doc, sid2.as_str()), "opacity"), "0.5");

        // 多选一条 undo:两个对象的 opacity 同步逆回(填充不动)
        stack.undo(&mut doc).unwrap();
        assert!(
            style_of(&doc, &sid).iter().all(|d| d.prop != "opacity"),
            "一条 undo 应逆回主选中"
        );
        assert!(
            style_of(&doc, sid2.as_str())
                .iter()
                .all(|d| d.prop != "opacity"),
            "一条 undo 应同步逆回第二条目标"
        );
        // vb-token-ok: 文档内容测试数据
        assert_eq!(decl(style_of(&doc, &sid), "background-color"), "#ff5a1f");

        // 清除:声明整条移除;重复清除不再产生命令(不刷 undo 栈)
        let cmd = combine(style_prop_cmds(&doc, &sids, "opacity", "0.7")).unwrap();
        push(&mut stack, &mut doc, cmd);
        let cmds = style_prop_remove_cmds(&doc, &sids, "opacity");
        assert_eq!(cmds.len(), 2, "两个目标各一条");
        let cmd = combine(cmds).unwrap();
        push(&mut stack, &mut doc, cmd);
        assert!(style_of(&doc, &sid).iter().all(|d| d.prop != "opacity"));
        assert!(style_prop_remove_cmds(&doc, &sids, "opacity").is_empty());
    }

    /// 门 2(交互/无障碍):href、target、alt、aria-label 写 attrs;
    /// 空值 = 删除;导出 HTML 里可见。
    #[test]
    fn interaction_and_a11y_attrs_written_to_document() {
        let (mut doc, sid) = box_doc();
        let mut stack = UndoStack::new();
        let sids = vec![sid.clone()];
        for (k, v) in [
            ("href", "https://example.com"),
            ("target", "_blank"),
            ("alt", "封面图"),
            ("aria-label", "主标题"),
        ] {
            for cmd in attr_cmds(&doc, &sids, k, v) {
                push(&mut stack, &mut doc, cmd);
            }
        }
        {
            let n = doc.nodes.get(doc.find_by_sid(&sid).unwrap()).unwrap();
            for (k, v) in [
                ("href", "https://example.com"),
                ("target", "_blank"),
                ("alt", "封面图"),
                ("aria-label", "主标题"),
            ] {
                assert_eq!(n.attrs.get(k).map(String::as_str), Some(v), "attr {k}");
            }
        }
        // 删除语义:空值移除且已删后不再产生命令
        let cmds = attr_cmds(&doc, &sids, "target", "");
        assert_eq!(cmds.len(), 1);
        for cmd in cmds {
            push(&mut stack, &mut doc, cmd);
        }
        assert!(attr_cmds(&doc, &sids, "target", "").is_empty());
        let html = vb_doc::export::render_project(&doc).files;
        assert!(
            html.iter()
                .any(|(_, c)| c.contains("aria-label=\"主标题\"")),
            "导出 HTML 应携带无障碍属性"
        );
    }

    /// 门 2(导出组):图层名改名 → data-vb-name;文档标题 → `<title>`;
    /// 两者均可撤销。
    #[test]
    fn export_group_name_and_doc_title_are_real_document_state() {
        let (mut doc, sid) = box_doc();
        let mut stack = UndoStack::new();
        let sids = vec![sid.clone()];
        let cmd = combine(rename_cmds(&doc, &sids, "英雄区")).unwrap();
        push(&mut stack, &mut doc, cmd);
        push(
            &mut stack,
            &mut doc,
            Command::SetMetaTitle {
                new: "产品官网".into(),
                old: None,
            },
        );
        assert_eq!(
            doc.nodes.get(doc.find_by_sid(&sid).unwrap()).unwrap().name,
            "英雄区"
        );
        let html = vb_doc::export::render_project(&doc).files;
        assert!(html
            .iter()
            .any(|(_, c)| c.contains("data-vb-name=\"英雄区\"")));
        assert!(html
            .iter()
            .any(|(_, c)| c.contains("<title>产品官网</title>")));
        // 撤销文档标题
        stack.undo(&mut doc).unwrap();
        assert_eq!(doc.meta.title, "未命名", "SetMetaTitle 应可撤销");
    }

    /// 门 2(渐变):角度数值化改写既有渐变;无渐变回退生成;反向 = +180°;
    /// 类型切换保留色标。
    #[test]
    fn gradient_numeric_angle_and_reverse() {
        let (mut doc, sid) = box_doc();
        let mut stack = UndoStack::new();
        let sids = vec![sid.clone()];
        // 无渐变 → 角度提交回退生成「现填充 → 白」
        for cmd in gradient_angle_cmds(&doc, &sids, 30.0) {
            push(&mut stack, &mut doc, cmd);
        }
        assert_eq!(
            decl(style_of(&doc, &sid), "background-image"),
            "linear-gradient(30deg, #d4d4d4 0%, #ffffff 100%)"
        );
        // 再改角度:只改角度,色标不动
        for cmd in gradient_angle_cmds(&doc, &sids, 120.0) {
            push(&mut stack, &mut doc, cmd);
        }
        assert_eq!(
            decl(style_of(&doc, &sid), "background-image"),
            "linear-gradient(120deg, #d4d4d4 0%, #ffffff 100%)"
        );
        // 反向:+180°
        for cmd in gradient_reverse_cmds(&doc, &sids) {
            push(&mut stack, &mut doc, cmd);
        }
        assert_eq!(
            decl(style_of(&doc, &sid), "background-image"),
            "linear-gradient(300deg, #d4d4d4 0%, #ffffff 100%)"
        );
        // 类型切换:线性 → 径向(保留色标)
        for cmd in gradient_kind_cmds(&doc, &sids, GradKind::Radial) {
            push(&mut stack, &mut doc, cmd);
        }
        assert_eq!(
            decl(style_of(&doc, &sid), "background-image"),
            "radial-gradient(circle at 50% 50%, #d4d4d4 0%, #ffffff 100%)"
        );
    }

    /// 画板工具态字段:预设/取向/尺寸写活动画板 geom;名称走 Rename。
    #[test]
    fn artboard_state_fields_write_active_artboard() {
        let (mut doc, _sid) = box_doc();
        let ab_sid = doc
            .nodes
            .get(doc.artboards[0])
            .unwrap()
            .sid
            .as_str()
            .to_string();
        let mut stack = UndoStack::new();
        let sids = vec![ab_sid.clone()];
        // 预设:W+H 一条 SetGeom(逐分量两条会以同一基准互相覆盖)
        let cmd = geom_axes_cmd(
            &doc,
            &ab_sid,
            &[(GeomAxis::W, 1920.0), (GeomAxis::H, 1080.0)],
        )
        .unwrap();
        assert!(
            matches!(cmd, Command::SetGeom { .. }),
            "W+H 应合并为一条 SetGeom"
        );
        push(&mut stack, &mut doc, cmd);
        let g = &doc.nodes.get(doc.artboards[0]).unwrap().geom;
        assert_eq!((g.w, g.h), (1920.0, 1080.0));
        // 取向互换(横 ↔ 竖)
        let cmd = geom_axes_cmd(
            &doc,
            &ab_sid,
            &[(GeomAxis::W, 1080.0), (GeomAxis::H, 1920.0)],
        )
        .unwrap();
        push(&mut stack, &mut doc, cmd);
        let g = &doc.nodes.get(doc.artboards[0]).unwrap().geom;
        assert_eq!((g.w, g.h), (1080.0, 1920.0));
        // 名称走 Rename
        let cmd = combine(rename_cmds(&doc, &sids, "首页")).unwrap();
        push(&mut stack, &mut doc, cmd);
        assert_eq!(doc.nodes.get(doc.artboards[0]).unwrap().name, "首页");
        // 三次 undo 逐步逆回
        stack.undo(&mut doc).unwrap();
        stack.undo(&mut doc).unwrap();
        stack.undo(&mut doc).unwrap();
        let g = &doc.nodes.get(doc.artboards[0]).unwrap().geom;
        assert_eq!((g.w, g.h), (1440.0, 900.0), "三次 undo 应回到默认画板尺寸");
    }

    /// combine 收敛规则:单条不包 Compound,空集为 None。
    #[test]
    fn combine_of_single_is_not_wrapped() {
        let (doc, sid) = box_doc();
        let cmds = rename_cmds(&doc, &[sid], "只改一个");
        assert_eq!(cmds.len(), 1);
        assert!(matches!(combine(cmds), Some(Command::Rename { .. })));
        assert!(combine(Vec::new()).is_none());
    }

    /// 锚点写回:SetVector 只动目标元素(直接选择数值化共用路径)。
    #[test]
    fn anchor_numeric_edit_moves_only_target_element() {
        use vb_common::geom::{BezPath, PathEl, Point};
        let (mut doc, _sid) = box_doc();
        let ab = doc.artboards.first().copied().unwrap();
        let vsid = doc.alloc_sid();
        let mut path = BezPath::new();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(100.0, 0.0));
        let mut n = Node::new(NodeKind::Vector { path }, "路径", vsid.clone());
        n.parent = Some(ab);
        let vid = doc.nodes.insert(n);
        doc.nodes.get_mut(ab).unwrap().children.push(vid);
        let vsid_s = vsid.as_str().to_string();

        // 与控制面板锚点字段同款写法:重写第 1 个元素的终点
        let nid = doc.find_by_sid(&vsid_s).unwrap();
        let mut els: Vec<PathEl> = match &doc.nodes.get(nid).unwrap().kind {
            NodeKind::Vector { path } => path.elements().to_vec(),
            _ => unreachable!(),
        };
        els[1] = PathEl::LineTo(Point::new(150.0, 25.0));
        let mut np = BezPath::new();
        for el in els {
            np.push(el);
        }
        let mut stack = UndoStack::new();
        push(
            &mut stack,
            &mut doc,
            Command::SetVector {
                sid: vsid_s.clone(),
                new: np,
                old: None,
            },
        );
        let nid = doc.find_by_sid(&vsid_s).unwrap();
        match &doc.nodes.get(nid).unwrap().kind {
            NodeKind::Vector { path } => {
                let els = path.elements().to_vec();
                assert_eq!(els[0], PathEl::MoveTo(Point::new(0.0, 0.0)), "其余元素不动");
                assert_eq!(els[1], PathEl::LineTo(Point::new(150.0, 25.0)));
            }
            _ => unreachable!(),
        }
    }

    /// NodeTree 克隆辅助与命令层同构(导入健康检查)。
    #[test]
    fn node_tree_import_still_valid() {
        let (doc, sid) = box_doc();
        let nid = doc.find_by_sid(&sid).unwrap();
        let tree = NodeTree::from_document(&doc, nid).expect("可克隆子树");
        assert_eq!(tree.node.sid.as_str(), sid);
    }
}
