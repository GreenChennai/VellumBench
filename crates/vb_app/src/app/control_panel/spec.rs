//! 控制面板 · 字段规格层(纯数据,无 egui / 文档依赖)。
//!
//! 06-1 自 `control_panel.rs` 按「规格 / 写回 / 渲染」三层拆出
//! (纯搬移,零行为变化);写回构建器在 `writes`,渲染在 `render`/`editors`。

use crate::app::Tool;

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
        // ── 阶段 5(05-2)新工具态:纯提示(design/03 未定义字段表)──
        Tool::Rotate | Tool::Mirror | Tool::Scale | Tool::FreeTransform => ControlPanelSpec {
            state: "xform.hint",
            fields: vec![f(
                "xform.hint",
                "变换工具:单击画布点设定中心 → 拖拽对象按工具语义变换 · Shift 15°/等比 · Alt 从对象中心 · Esc 回选择",
                CtlKind::Hint,
                CtlWrite::None,
            )],
        },
        Tool::Pencil => ControlPanelSpec {
            state: "pencil.hint",
            fields: vec![f(
                "pencil.hint",
                "铅笔:按住拖动自由绘制,松手按保真度容差抽稀为矢量路径(编辑 → 设置 → 铅笔保真度)",
                CtlKind::Hint,
                CtlWrite::None,
            )],
        },
        Tool::Curvature => ControlPanelSpec {
            state: "curvature.hint",
            fields: vec![f(
                "curvature.hint",
                "曲率:在矢量路径锚点附近单击,自动拟合平滑控制点(直接选择 A 可微调)",
                CtlKind::Hint,
                CtlWrite::None,
            )],
        },
        Tool::Slice => ControlPanelSpec {
            state: "slice.hint",
            fields: vec![f(
                "slice.hint",
                "切片:拖框建立 data-vb-slice 切片;选中对象后单击 = 从选区建立;vellum-cli export --slice 按名出图",
                CtlKind::Hint,
                CtlWrite::None,
            )],
        },
        Tool::Measure => ControlPanelSpec {
            state: "measure.hint",
            fields: vec![f(
                "measure.hint",
                "度量:拖动量两点距离;单击对象标注尺寸;Esc 退出",
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
///
/// 04-5-3(P1-⑤)对照 `design/03 §三` 补齐:文档要求
/// 「预设尺寸、取向、背景色、**画板数**」四项 —— 画板数此前只有
/// 「+画板」按钮,现补一个只读计数(`ab.count`),文档口径逐字对齐。
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
            // 只读计数(渲染层显示当前画板数;增删走「+画板」/画板面板)
            f("ab.count", "画板数", CtlKind::Hint, CtlWrite::None),
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
