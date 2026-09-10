//! 光标体系（设计文档 14 篇 §3.7；`vb-ui-tokens.json` 的 `cursor` 段）。
//!
//! 光标是**最便宜的可感知反馈**：用户还没点击，就能从光标形状知道
//! "这里能拖动 / 这里能旋转 / 这里不能改"。原先画布只有一个
//! "抓手 / 默认" 两态切换，8 个缩放手柄、旋转圈、锁定对象全都用同一个
//! 光标 —— 用户只能靠试。这个模块把那条 §3.7 表变成可复用的常量与函数。
//!
//! ## 已知限制
//!
//! egui **不支持自定义光标图片**，所以 Illustrator 那个"弯曲双箭头"的旋转
//! 光标做不出来，只能退到最接近的内置图标（[`HANDLE_ROTATE`]）并靠角外圈
//! 的视觉提示兜底。这一条写进 tokens.json 的 `known_egui_constraints`。

use egui::CursorIcon;

/// 画布空白处的默认光标。
pub const CANVAS_DEFAULT: CursorIcon = CursorIcon::Default;
/// 悬停在对象上（可点选 / 可拖动）。
pub const HOVER_OBJECT: CursorIcon = CursorIcon::PointingHand;
/// 角手柄（NW/SE 方向）。
pub const HANDLE_CORNER_NWSE: CursorIcon = CursorIcon::ResizeNwSe;
/// 角手柄（NE/SW 方向）。
pub const HANDLE_CORNER_NESW: CursorIcon = CursorIcon::ResizeNeSw;
/// 左右边手柄。
pub const HANDLE_EDGE_H: CursorIcon = CursorIcon::ResizeHorizontal;
/// 上下边手柄。
pub const HANDLE_EDGE_V: CursorIcon = CursorIcon::ResizeVertical;
/// 旋转。
///
/// ⚠️ Illustrator 用的是自定义弯曲箭头，egui 给不了 → 用对角缩放箭头兜底。
pub const HANDLE_ROTATE: CursorIcon = CursorIcon::ResizeNwSe;
/// 可平移（空格 / 抓手工具）。
pub const PAN: CursorIcon = CursorIcon::Grab;
/// 正在平移。
pub const PANNING: CursorIcon = CursorIcon::Grabbing;
/// 钢笔 / 路径工具（精确落点）。
pub const PEN: CursorIcon = CursorIcon::Crosshair;
/// 文本工具 / 文本编辑。
pub const TEXT: CursorIcon = CursorIcon::Text;
/// 锁定对象 —— "不能动"必须显式表达，否则用户会以为是软件卡了。
pub const LOCKED: CursorIcon = CursorIcon::NotAllowed;
/// 放大 / 缩小（缩放工具或 Alt+滚轮）。
pub const ZOOM_IN: CursorIcon = CursorIcon::ZoomIn;
/// 见 [`ZOOM_IN`]。
pub const ZOOM_OUT: CursorIcon = CursorIcon::ZoomOut;

/// 8 个缩放手柄 → 光标。
///
/// 手柄编号与 `vb_app::app::hit_handle` 一致：
/// `0=NW 1=N 2=NE 3=E 4=SE 5=S 6=SW 7=W`。
///
/// 对角手柄共享同一种光标是**正确的**：NW 与 SE 的拖拽方向在同一条对角线
/// 上，用户看到的是"沿这条线缩放"，不是"朝哪个角"。
pub const fn for_handle(handle: u8) -> CursorIcon {
    match handle {
        // NW / SE：主对角线
        0 | 4 => HANDLE_CORNER_NWSE,
        // NE / SW：副对角线
        2 | 6 => HANDLE_CORNER_NESW,
        // N / S：上下边，纵向缩放
        1 | 5 => HANDLE_EDGE_V,
        // E / W 以及越界值：左右边，横向缩放
        _ => HANDLE_EDGE_H,
    }
}

/// 悬停到某个对象上时该用什么光标。
///
/// `locked` 优先于一切 —— 一个锁定的对象不该给出任何"可拖动"的暗示。
pub const fn for_hover(locked: bool) -> CursorIcon {
    if locked {
        LOCKED
    } else {
        HOVER_OBJECT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 8 个手柄必须全部有光标，且对角两两一致。
    #[test]
    fn every_handle_has_a_cursor() {
        assert_eq!(for_handle(0), for_handle(4), "NW 与 SE 应对角同向");
        assert_eq!(for_handle(2), for_handle(6), "NE 与 SW 应对角同向");
        assert_eq!(for_handle(1), for_handle(5), "N 与 S 应同为纵向");
        assert_eq!(for_handle(3), for_handle(7), "E 与 W 应同为横向");
        // 三条轴必须互不相同，否则用户分不出在缩哪条边
        let axes = [for_handle(0), for_handle(2), for_handle(1), for_handle(3)];
        for (i, a) in axes.iter().enumerate() {
            for b in &axes[i + 1..] {
                assert_ne!(a, b, "两个不同的缩放手柄给出了相同光标");
            }
        }
    }

    /// 越界的手柄编号不能 panic（光标在热路径上，每帧都算）。
    #[test]
    fn out_of_range_handle_is_safe() {
        assert_eq!(for_handle(200), HANDLE_EDGE_H);
    }

    /// 锁定对象必须给"禁止"光标，而不是普通悬停光标。
    #[test]
    fn locked_overrides_hover() {
        assert_eq!(for_hover(true), LOCKED);
        assert_eq!(for_hover(false), HOVER_OBJECT);
        assert_ne!(for_hover(true), for_hover(false));
    }

    /// 旋转光标与对角缩放手柄撞车是**已知妥协**（egui 无自定义光标图片）。
    /// 这条测试把这个事实钉住：将来 egui 支持自定义光标时，
    /// 这里会失败，提醒实现者去把旋转光标换成真正的弯曲箭头。
    #[test]
    fn rotate_cursor_is_a_documented_compromise() {
        assert_eq!(
            HANDLE_ROTATE, HANDLE_CORNER_NWSE,
            "egui 已能自定义光标时，请把旋转光标换成真正的旋转箭头，\
             并更新 tokens.json 的 known_egui_constraints"
        );
    }
}
