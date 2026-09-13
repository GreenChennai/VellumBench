# -*- coding: utf-8 -*-
p = 'crates/vb_app/src/app.rs'
s = open(p, encoding='utf-8').read()

# ── 1) 顶点枚举统一助手:锚点元素含曲线终点(平滑钢笔产物) ──
old = '''    /// 直接选择:命中检测 — 找光标附近矢量节点的顶点。返回 (sid, 顶点序号)。
    fn find_vector_vertex(&self, wx: f64, wy: f64, tol: f64) -> Option<(String, usize)> {'''
new = '''    /// 直接选择/剪刀共用的顶点枚举:返回 (元素序号, 世界坐标)。
    /// 锚点元素 = MoveTo/LineTo/QuadTo/CurveTo 的终点(平滑钢笔会产出曲线)。
    fn path_anchor_points(
        &self,
        nid: vb_doc::model::NodeId,
    ) -> Vec<(usize, vb_common::geom::Point)> {
        use vb_common::geom::PathEl;
        let Some(n) = self.doc.nodes.get(nid) else {
            return vec![];
        };
        let NodeKind::Vector { path } = &n.kind else {
            return vec![];
        };
        let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) else {
            return vec![];
        };
        path.elements()
            .iter()
            .enumerate()
            .filter_map(|(i, el)| {
                let p = match el {
                    PathEl::MoveTo(p) | PathEl::LineTo(p) => *p,
                    PathEl::QuadTo(_, p) => *p,
                    PathEl::CurveTo(_, _, p) => *p,
                    _ => return None,
                };
                Some((i, vb_common::geom::Point::new(bb.x0 + p.x, bb.y0 + p.y)))
            })
            .collect()
    }

    /// 直接选择:命中检测 — 找光标附近矢量节点的顶点。返回 (sid, 顶点序号)。
    fn find_vector_vertex(&self, wx: f64, wy: f64, tol: f64) -> Option<(String, usize)> {'''
assert old in s, "anchor helper"
s = s.replace(old, new)

# find_vector_vertex 改用助手
old = '''                if let NodeKind::Vector { path } = &n.kind {
                    for (i, el) in path.elements().iter().enumerate() {
                        use vb_common::geom::PathEl;
                        let p = match el {
                            PathEl::MoveTo(p) | PathEl::LineTo(p) => *p,
                            _ => continue,
                        };
                        // 路径以节点原点存储 → 世界 = 世界 bbox 原点 + 点;
                        // 单个节点取不到 bbox 只跳过该节点(此前 `?` 会放弃整棵树)
                        let Some(bb) = vb_tools::abs_bbox_world(&self.doc, id) else {
                            continue;
                        };
                        let ax = bb.x0 + p.x;
                        let ay = bb.y0 + p.y;
                        if (ax - wx).hypot(ay - wy) <= tol {
                            return Some((n.sid.as_str().to_string(), i));
                        }
                    }
                }'''
new = '''                if matches!(n.kind, NodeKind::Vector { .. }) {
                    // 单个节点取不到 bbox 只跳过该节点(此前 `?` 会放弃整棵树)
                    for (i, p) in self.path_anchor_points(id) {
                        if (p.x - wx).hypot(p.y - wy) <= tol {
                            return Some((n.sid.as_str().to_string(), i));
                        }
                    }
                }'''
assert old in s, "find_vector_vertex body"
s = s.replace(old, new)

# vector_vertices 同步改(渲染锚点含曲线终点)
old = '''        let bb = vb_tools::abs_bbox_world(&self.doc, nid).unwrap_or(vb_common::geom::Rect::ZERO);
        path.elements()
            .iter()
            .enumerate()
            .filter_map(|(i, el)| {
                use vb_common::geom::PathEl;
                match el {
                    PathEl::MoveTo(p) | PathEl::LineTo(p) => Some((i, bb.x0 + p.x, bb.y0 + p.y)),
                    _ => None,
                }
            })
            .collect()
    }'''
new = '''        path.elements().iter().enumerate().filter_map(move |(i, el)| {
            use vb_common::geom::PathEl;
            let p = match el {
                PathEl::MoveTo(p) | PathEl::LineTo(p) => *p,
                PathEl::QuadTo(_, p) => *p,
                PathEl::CurveTo(_, _, p) => *p,
                _ => return None,
            };
            Some((i, bb.x0 + p.x, bb.y0 + p.y))
        })
    }'''
# vector_vertices 内的 let NodeKind 解构保留在前;上面 old 需要上下文唯一。
# 先探测实际文本:
if old not in s:
    # 回退:直接找原始函数体片段
    m = re.search(r"        let bb = vb_tools::abs_bbox_world\(&self\.doc, nid\)\.unwrap_or\(vb_common::geom::Rect::ZERO\);\n(.*?)\n    \}", s, re.S)
    raise SystemExit("vector_vertices anchor not found; context:\n" + (m.group(0)[:400] if m else "n/a"))
s = s.replace(old, new)

# DS 拖拽替换也要支持曲线终点
old = '''                                els[vi] = match els[vi] {
                                    vb_common::geom::PathEl::MoveTo(_) => {
                                        vb_common::geom::PathEl::MoveTo(new_pt)
                                    }
                                    vb_common::geom::PathEl::LineTo(_) => {
                                        vb_common::geom::PathEl::LineTo(new_pt)
                                    }
                                    other => other,
                                };'''
new = '''                                els[vi] = match els[vi] {
                                    vb_common::geom::PathEl::MoveTo(_) => {
                                        vb_common::geom::PathEl::MoveTo(new_pt)
                                    }
                                    vb_common::geom::PathEl::LineTo(_) => {
                                        vb_common::geom::PathEl::LineTo(new_pt)
                                    }
                                    vb_common::geom::PathEl::QuadTo(c, _) => {
                                        vb_common::geom::PathEl::QuadTo(c, new_pt)
                                    }
                                    vb_common::geom::PathEl::CurveTo(c1, c2, _) => {
                                        vb_common::geom::PathEl::CurveTo(c1, c2, new_pt)
                                    }
                                    other => other,
                                };'''
assert old in s, "ds drag curve"
s = s.replace(old, new)

open(p, 'w', encoding='utf-8', newline='\n').write(s)
print("vertex helpers ok")
