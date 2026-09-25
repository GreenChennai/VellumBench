//! 视图导航与拾取:缩放/适合窗口、活动画板、命中拾取、插入目标定位。
//!
//! 06-1 自 `app.rs` 按「生命周期 / 派发 / 外部监听 / 导航拾取」拆出
//! (纯搬移,零行为变化);生命周期主体仍在 `app.rs`。

use vb_doc::model::NodeKind;

use super::VellumApp;

impl VellumApp {
    /// 缩放到选区(C3):选区联合 bbox 充满视口;无选区回退 fit_view。
    pub(super) fn zoom_to_selection(&mut self) {
        let Some(rect) = self.canvas_rect else {
            return;
        };
        let mut x0 = f64::INFINITY;
        let mut y0 = f64::INFINITY;
        let mut x1 = f64::NEG_INFINITY;
        let mut y1 = f64::NEG_INFINITY;
        for sid in &self.selection {
            if let Some(nid) = self.doc.find_by_sid(sid) {
                if let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) {
                    x0 = x0.min(bb.x0);
                    y0 = y0.min(bb.y0);
                    x1 = x1.max(bb.x1);
                    y1 = y1.max(bb.y1);
                }
            }
        }
        if !x0.is_finite() {
            self.fit_view();
            return;
        }
        let margin = 60.0f64;
        let w = (x1 - x0).max(20.0);
        let h = (y1 - y0).max(20.0);
        let zoom = ((rect.width() as f64 - margin * 2.0) / w)
            .min((rect.height() as f64 - margin * 2.0) / h)
            .clamp(0.01, 64.0);
        self.camera.zoom = zoom;
        self.camera.pan_x = rect.width() as f64 / 2.0 - (x0 + w / 2.0) * zoom;
        self.camera.pan_y = rect.height() as f64 / 2.0 - (y0 + h / 2.0) * zoom;
    }

    pub(super) fn fit_view(&mut self) {
        let Some(rect) = self.canvas_rect else { return };
        let Some(&ab) = self.doc.artboards.first() else {
            return;
        };
        let Some(_n) = self.doc.nodes.get(ab) else {
            return;
        };
        // 全部画板的联合 bbox(纵向排布)
        let mut min_y = f64::INFINITY;
        let mut max_r = f64::NEG_INFINITY;
        let mut max_b = f64::NEG_INFINITY;
        let mut min_x = f64::INFINITY;
        for &a in &self.doc.artboards {
            if let Some(an) = self.doc.nodes.get(a) {
                min_x = min_x.min(an.geom.x);
                min_y = min_y.min(an.geom.y);
                max_r = max_r.max(an.geom.x + an.geom.w);
                max_b = max_b.max(an.geom.y + an.geom.h);
            }
        }
        let w = (max_r - min_x).max(1.0);
        let h = (max_b - min_y).max(1.0);
        let margin = 60.0f64;
        // clamp 下限:画布极窄时分子为负会产生负缩放(视图翻转)
        let zoom = (((rect.width() as f64) - margin * 2.0) / w)
            .min(((rect.height() as f64) - margin * 2.0) / h)
            .clamp(0.01, 4.0);
        self.camera.zoom = zoom;
        self.camera.pan_x = rect.left() as f64 + margin - min_x * zoom;
        self.camera.pan_y = rect.top() as f64 + margin - min_y * zoom;
    }

    /// 当前活动画板(含选区的画板,否则第一个)。
    pub(super) fn active_artboard(&self) -> Option<vb_doc::model::NodeId> {
        self.selection
            .first()
            .and_then(|sid| self.doc.find_by_sid(sid))
            .and_then(|nid| {
                let mut p = Some(nid);
                loop {
                    match p {
                        Some(id) => {
                            let n = self.doc.nodes.get(id).unwrap();
                            if matches!(n.kind, NodeKind::Artboard) {
                                break Some(id);
                            }
                            p = n.parent;
                        }
                        None => break None,
                    }
                }
            })
            .or(self.doc.artboards.first().copied())
    }

    pub(super) fn active_artboard_name(&self) -> String {
        self.active_artboard()
            .and_then(|a| self.doc.nodes.get(a).map(|n| n.name.clone()))
            .unwrap_or_else(|| "无".into())
    }

    pub(super) fn artboard_at_world(&self, wx: f64, wy: f64) -> Option<vb_doc::model::NodeId> {
        self.doc.artboards.iter().copied().find(|&a| {
            self.doc
                .nodes
                .get(a)
                .map(|n| {
                    wx >= n.geom.x
                        && wx <= n.geom.x + n.geom.w
                        && wy >= n.geom.y
                        && wy <= n.geom.y + n.geom.h
                })
                .unwrap_or(false)
        })
    }

    /// 隔离栈顶(当前隔离的编组;空 = 未隔离)。
    pub(super) fn isolate_top(&self) -> Option<vb_doc::model::NodeId> {
        self.isolate_stack.last().copied()
    }

    /// 选中/命中统一入口:隔离模式下只在隔离子树内拾取。
    pub(super) fn pick_at_world(&self, wx: f64, wy: f64) -> Option<vb_doc::model::NodeId> {
        if let Some(iso) = self.isolate_top() {
            let ab = vb_tools::artboard_of(&self.doc, iso)?;
            let (ox, oy) = self.doc.artboard_origin(ab);
            return vb_tools::hit_test_root(&self.doc, iso, wx - ox, wy - oy);
        }
        let ab = self.artboard_at_world(wx, wy)?;
        vb_tools::hit_test(&self.doc, ab, wx, wy)
    }

    /// 新对象的插入目标:隔离模式下落进隔离组(06 篇 §4.3),否则所属画板。
    pub(super) fn insert_target(&mut self, wx: f64, wy: f64) -> vb_doc::model::NodeId {
        if let Some(iso) = self.isolate_top() {
            return iso;
        }
        match self
            .artboard_at_world(wx, wy)
            .or(self.doc.artboards.first().copied())
        {
            Some(ab) => ab,
            // 兜底:命令层「至少一块画板」守卫之外的第二道保险(导入 0 画板
            // 文档后直接开画等)。恢复路径直接落一块默认画板,不走 undo。
            None => {
                let id = self.doc.new_artboard("画板 1", 1440.0, 900.0);
                self.status = "画布为空,已重建默认画板".into();
                id
            }
        }
    }

    /// 世界坐标 → 指定父级的本地坐标(累计父级 geom 偏移,止于画板)。
    /// 直接用世界坐标建对象会在第 2+ 画板/编组内产生双倍偏移。
    pub(super) fn world_to_parent_local(
        &self,
        parent: vb_doc::model::NodeId,
        mut x: f64,
        mut y: f64,
    ) -> (f64, f64) {
        let mut p = self.doc.nodes.get(parent).and_then(|n| n.parent);
        while let Some(pid) = p {
            let pn = self.doc.nodes.get(pid).expect("parent 存活");
            if matches!(pn.kind, NodeKind::Artboard) {
                break;
            }
            x -= pn.geom.x;
            y -= pn.geom.y;
            p = pn.parent;
        }
        (x, y)
    }
}
