//! 工具状态机骨架(纯数据 + 纯函数,零 UI 依赖)。
//!
//! **vb_app 全量迁移是后续批次,本批只立骨架**(22 篇 §4 R0 交付 4):
//! 画布手势 → 状态机 → 命令的完整接线、scrubby/光标映射等仍按轮次迁入;
//! 本模块先固化「工具枚举镜像 + 活跃/上一个工具」的最小形状,供
//! `vb_kit`/`vb_shell` 投影消费(工具箱高亮、上下文条切换等)。
//!
//! [`ToolId`] **镜像** `vb_app::Tool` 现有 22 个工具(镜像而非引用:
//! 本 crate 禁依赖旧宿主)。两边的一致性由新宿主接线批次(命令单源
//! `commands.yaml`)收口,届时 `tool.*` 命令 id ↔ `ToolId` 的映射表进门禁。

use serde::{Deserialize, Serialize};

/// 工具标识(镜像 `vb_app::Tool`;注释保留各工具的行为要点出处)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolId {
    Select,
    DirectSelect,
    Rect,
    Ellipse,
    /// 直线段:创建细长 Box(HTML 中即一个 2px 高的色条)。
    Line,
    /// 钢笔:逐点落锚点,直线段连接;点击起点或 Enter 闭合/结束。
    Pen,
    Hand,
    /// 缩放工具:单击放大 / Alt+单击缩小 / 拖框缩放到区域。
    Zoom,
    /// 文字工具:单击点文本 / 拖框区域文本。
    Text,
    /// 吸管:点击取色应用到选区;Alt 取全部样式。
    Eyedropper,
    /// 画板工具:拖框新建画板。
    Artboard,
    /// 渐变工具:拖动设定线性渐变方向。
    Gradient,
    /// 剪刀:在矢量锚点处剪开(闭路开口/开路分段)。
    Scissors,
    /// 编组选择:单击选中命中对象所在的整个编组。
    GroupSelect,
    /// 旋转 R:单击设中心,拖动旋转;Shift 约束 15°。
    Rotate,
    /// 镜像 O:单击设中心,拖动决定镜像轴。
    Mirror,
    /// 缩放 S:单击设中心,拖动缩放;Shift 等比;Alt 从对象中心。
    Scale,
    /// 自由变换 E:拖选区四角之一,对角锚定缩放(透视变形不做,网页无对应)。
    FreeTransform,
    /// 铅笔 N:自由绘制 → 按保真度容差抽稀为矢量路径。
    Pencil,
    /// 曲率:点击矢量路径段自动拟合平滑控制点。
    Curvature,
    /// 切片 Shift+K:拖框建立 data-vb-slice 切片。
    Slice,
    /// 度量:拖动量两点击点间距离 / 单击标注对象尺寸。
    Measure,
}

impl ToolId {
    /// 全部工具(工具箱遍历/门禁用;顺序即镜像声明序)。
    pub const ALL: &[ToolId] = &[
        ToolId::Select,
        ToolId::DirectSelect,
        ToolId::Rect,
        ToolId::Ellipse,
        ToolId::Line,
        ToolId::Pen,
        ToolId::Hand,
        ToolId::Zoom,
        ToolId::Text,
        ToolId::Eyedropper,
        ToolId::Artboard,
        ToolId::Gradient,
        ToolId::Scissors,
        ToolId::GroupSelect,
        ToolId::Rotate,
        ToolId::Mirror,
        ToolId::Scale,
        ToolId::FreeTransform,
        ToolId::Pencil,
        ToolId::Curvature,
        ToolId::Slice,
        ToolId::Measure,
    ];

    /// X-4 变换工具族判定(镜像 `vb_app::Tool::is_transform_family` 语义:
    /// 单击设中心 → 拖拽变换的共享入口)。
    pub fn is_transform_family(self) -> bool {
        matches!(
            self,
            ToolId::Rotate | ToolId::Mirror | ToolId::Scale | ToolId::FreeTransform
        )
    }
}

/// 工具状态(纯数据):活跃工具 + 上一个工具。
///
/// 「上一个工具」服务于临时工具语义(如按住空格临时切 Hand,松手回原工具):
/// `set_active` 记录前任,`end_transient` 回退。是否属于临时工具由调用方
/// 裁决(不同手势路径不同),本结构只保证回退链一致。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolState {
    active: Option<ToolId>,
    previous: Option<ToolId>,
}

impl ToolState {
    /// 以默认工具构造(旧宿主默认 = Select)。
    pub fn new(default: ToolId) -> Self {
        Self {
            active: Some(default),
            previous: None,
        }
    }

    /// 活跃工具(未初始化为 None;新宿主开窗即 [`ToolState::new`]。
    /// 保留 Option 以便反序列化出坏值时不 panic,由调用方回退默认)。
    pub fn active(&self) -> Option<ToolId> {
        self.active
    }

    /// 切换工具(记录前任;同工具重复激活不覆盖前任 —— 连点工具图标
    /// 不应丢失回退链)。
    pub fn set_active(&mut self, tool: ToolId) {
        if self.active != Some(tool) {
            self.previous = self.active;
            self.active = Some(tool);
        }
    }

    /// 上一个工具(回退目标)。
    pub fn previous(&self) -> Option<ToolId> {
        self.previous
    }

    /// 结束临时工具:回退到上一个工具。无可回退(或本就无活跃工具)时
    /// 为 no-op 并返回 false。
    pub fn end_transient(&mut self) -> bool {
        match (self.active, self.previous.take()) {
            (Some(_), Some(prev)) => {
                self.active = Some(prev);
                true
            }
            // 无处可退:清掉 previous 残留由 take 完成,状态不动
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_are_distinct_and_complete() {
        // 镜像完整性:ALL 逐一遍历不重复,且与枚举匹配数一致(新加变体
        // 忘挂 ALL 即红,防工具箱漏图标)。
        let all = ToolId::ALL;
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b, "ToolId::ALL 有重复项:{a:?}");
            }
        }
        assert_eq!(all.len(), 22, "镜像 vb_app::Tool 的 22 个工具");
    }

    #[test]
    fn transform_family_mirror() {
        for t in ToolId::ALL {
            assert_eq!(
                t.is_transform_family(),
                matches!(
                    t,
                    ToolId::Rotate | ToolId::Mirror | ToolId::Scale | ToolId::FreeTransform
                )
            );
        }
    }

    #[test]
    fn set_active_records_previous() {
        let mut s = ToolState::new(ToolId::Select);
        assert_eq!(s.active(), Some(ToolId::Select));
        s.set_active(ToolId::Rect);
        assert_eq!(s.active(), Some(ToolId::Rect));
        assert_eq!(s.previous(), Some(ToolId::Select));
    }

    #[test]
    fn same_tool_reactivation_keeps_previous() {
        let mut s = ToolState::new(ToolId::Select);
        s.set_active(ToolId::Pen);
        s.set_active(ToolId::Pen);
        assert_eq!(s.previous(), Some(ToolId::Select), "连点同工具不丢回退链");
    }

    #[test]
    fn end_transient_falls_back_or_noops() {
        let mut s = ToolState::new(ToolId::Select);
        s.set_active(ToolId::Hand);
        assert!(s.end_transient());
        assert_eq!(s.active(), Some(ToolId::Select));
        assert_eq!(s.previous(), None);
        // 已在初态:无处可退
        assert!(!s.end_transient());
    }

    #[test]
    fn default_is_uninitialized_not_panicking() {
        let mut s = ToolState::default();
        assert_eq!(s.active(), None);
        assert!(!s.end_transient());
    }

    #[test]
    fn serde_roundtrip() {
        let mut s = ToolState::new(ToolId::Pencil);
        s.set_active(ToolId::Hand);
        let json = serde_json::to_string(&s).expect("serialize");
        let back: ToolState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, s);
    }
}
