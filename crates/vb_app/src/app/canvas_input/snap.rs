//! 智能参考线吸附:引擎已下沉 [`vb_session::snap`](R0 会话态批次,
//! 22 篇 §3.3「吸附引擎在 vb_session」——新宿主与旧宿主共用同一份
//! 纯函数,不得手写第二份)。
//!
//! 本文件只剩 `VellumApp` 方法的**薄转发**(`self.doc` → 引擎入参),
//! 调用点(`canvas_input/select.rs`)与签名不动,行为逐字节一致。

use vb_doc::model::Geom;

use crate::app::VellumApp;

impl VellumApp {
    /// 智能参考线:移动中的对象边/中心 对齐 兄弟边/中心 或 画板边/中心。
    /// 返回 (吸附后 x, 吸附后 y, 参考线段[世界坐标])。
    ///
    /// 帧纪律与候选/参考线规则见 [`vb_session::snap::smart_snap`]
    /// (画板偏移换算 `parent_offset_in_artboard` 亦在引擎内,旧宿主
    /// 无独立调用点,不再保留转发壳)。
    pub(super) fn smart_snap(
        &self,
        moving: vb_doc::model::NodeId,
        parent: Option<vb_doc::model::NodeId>,
        artboard: Option<vb_doc::model::NodeId>,
        g: &Geom,
        tol: f64,
    ) -> (f64, f64, Vec<[f64; 4]>) {
        vb_session::snap::smart_snap(&self.doc, moving, parent, artboard, g, tol)
    }
}
