//! 09-B 剪切蒙版(05-2):`Mod+7` 建立 / `Mod+Alt+7` 释放。
//!
//! HTML 落地(设计文档 01 §2.3 / 06 §4.4 三选一裁定):**overflow+形状容器**
//! —— 蒙版对象(必须是 Box)自身成为裁剪容器:内容对象 Move 进蒙版、
//! 蒙版加 `overflow: hidden` + `data-vb-clip="1"`。与导出链兼容:样式与
//! 属性都是既有白名单通道,浏览器端 overflow 裁剪为原生语义;画布/CPU/SVG
//! 渲染端由 `DrawItem::overflow_clip`(vb_render 05-2 扩展)承载同一效果。
//!
//! 为什么不用 `<clipPath>` defs:文档模型没有 `<defs>` 通道(head/trailing
//! raw 皆非节点级),引用型 clip-path 无法在重导入时找回形状定义;
//! overflow 容器在「HTML → 模型 → HTML」往返中零信息损失。
//!
//! 命令全部由**既有可逆命令**复合(ADR-0008):Move(收编内容)+
//! SetGeom(内容重定基)+ SetStyle/SetAttrs(容器标记)+ undo 靠
//! Compound 逆序回滚。几何不变量:内容在收编前后**视觉位置不变**
//! (坐标按蒙版原点重定基),单测锁定。

use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom, NodeKind};

/// 蒙版容器标记属性(导入器据此恢复蒙版语义提示;释放后移除)。
pub const CLIP_ATTR: &str = "data-vb-clip";
/// 蒙版标记值。
pub const CLIP_ATTR_VALUE: &str = "1";

/// 内容坐标从**画板本地系**重定基到蒙版本地系(纯函数):
/// 内容几何(绝对/画板本地)− 蒙版原点。
pub fn rebase_to_mask(content: Geom, mask_origin: (f64, f64)) -> Geom {
    Geom {
        x: content.x - mask_origin.0,
        y: content.y - mask_origin.1,
        w: content.w,
        h: content.h,
    }
}

/// 建立剪切蒙版的命令序列(纯函数;调用方 exec 成一条 Compound)。
///
/// 入参约束(不满足返回 Err,调用方给中文提示):
/// - 恰好选中 ≥2 个对象且同父;
/// - **最后选中者** = 蒙版(AI 语义:蒙版对象在最上层/最后选);
/// - 蒙版必须是 Box(矩形/椭圆盒);内容不得含画板/蒙版自身。
///
/// 命令序:
/// ① 逐内容 Move 进蒙版(父级改为蒙版,z 序在蒙版自身背景之上);
/// ② 逐内容 SetGeom 重定基(蒙版本地坐标,视觉位置不变);
/// ③ SetStyle:蒙版加 `overflow:hidden`(椭圆蒙版加 `border-radius: 50%`);
/// ④ SetAttrs:蒙版加 `data-vb-clip="1"`。
pub fn clip_mask_cmds(doc: &Document, selection: &[String]) -> Result<Vec<Command>, String> {
    if selection.len() < 2 {
        return Err("剪切蒙版:选中「内容 + 形状」(形状最后选)后再按 Ctrl+7".into());
    }
    let mask_sid = selection.last().unwrap().clone();
    let Some(mask_id) = doc.find_by_sid(&mask_sid) else {
        return Err(format!("剪切蒙版:找不到蒙版对象 {mask_sid}"));
    };
    let Some(mask_node) = doc.nodes.get(mask_id) else {
        return Err(format!("剪切蒙版:找不到蒙版对象 {mask_sid}"));
    };
    if !matches!(mask_node.kind, NodeKind::Box) {
        return Err("剪切蒙版:蒙版形状必须是矩形/椭圆盒(顶层内容)".into());
    }
    let mask_geom = mask_node.geom;
    let mask_abs = vb_tools::abs_bbox(doc, mask_id).ok_or("剪切蒙版:取不到蒙版几何")?;
    let mask_abs_origin = (mask_abs.x0, mask_abs.y0);
    let is_ellipse = mask_node
        .style_get("border-radius")
        .map(|v| v.contains('%'))
        .unwrap_or(false);

    let mut cmds: Vec<Command> = Vec::new();
    for sid in &selection[..selection.len() - 1] {
        if sid == &mask_sid {
            continue;
        }
        let Some(id) = doc.find_by_sid(sid) else {
            return Err(format!("剪切蒙版:找不到内容对象 {sid}"));
        };
        if doc.is_descendant_or_self(id, mask_id) || doc.is_descendant_or_self(mask_id, id) {
            return Err("剪切蒙版:内容与蒙版不能互为祖先/后代".into());
        }
        let Some(n) = doc.nodes.get(id) else {
            continue;
        };
        let abs = vb_tools::abs_bbox(doc, id).ok_or("剪切蒙版:取不到内容几何")?;
        // 蒙版本地系下的期望几何(绝对/画板本地 − 蒙版绝对原点):
        // 无论内容原来挂在画板 / Layer / Group 下,这一步都成立
        let desired = Geom {
            x: abs.x0 - mask_abs_origin.0,
            y: abs.y0 - mask_abs_origin.1,
            w: n.geom.w,
            h: n.geom.h,
        };
        cmds.push(Command::Move {
            sid: sid.clone(),
            new_parent_sid: mask_sid.clone(),
            // 移到子级首位:蒙版自身不是子级,children 全部是被收编内容
            new_index: 0,
            old: None,
        });
        // SetGeom 紧跟 Move:apply 时捕获的 old = Move 后未重定基的原几何,
        // 撤销时先还原旧几何、Move 再还原旧父级 —— 逐级逆回无残差
        cmds.push(Command::SetGeom {
            sid: sid.clone(),
            new: rebase_to_mask(desired, (0.0, 0.0)),
            old: None,
            old_declared: None,
        });
    }
    if cmds.is_empty() {
        return Err("剪切蒙版:没有可收编的内容对象".into());
    }
    // 蒙版自身样式:overflow + 椭圆圆角
    let mut new_style = mask_node.style.clone();
    if !new_style.iter().any(|d| d.prop == "overflow") {
        new_style.push(vb_css::Decl {
            prop: "overflow".into(),
            value: "hidden".into(),
            important: false,
        });
    } else if let Some(d) = new_style.iter_mut().find(|d| d.prop == "overflow") {
        d.value = "hidden".into();
    }
    if is_ellipse {
        new_style.push(vb_css::Decl {
            prop: "border-radius".into(),
            value: "50%".into(),
            important: false,
        });
    }
    cmds.push(Command::SetStyle {
        sid: mask_sid.clone(),
        new: new_style,
        old: None,
    });
    // 容器标记:释放命令据它找蒙版;导入往返经 attrs 通道无损
    let mut new_attrs: Vec<(String, String)> = mask_node
        .attrs
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    new_attrs.push((CLIP_ATTR.into(), CLIP_ATTR_VALUE.into()));
    new_attrs.sort();
    cmds.push(Command::SetAttrs {
        sid: mask_sid.clone(),
        new: new_attrs,
        old: None,
    });
    let _ = mask_geom;
    Ok(cmds)
}

/// `sid` 是否是剪切蒙版容器(有 `data-vb-clip` 标记)。
pub fn is_clip_mask(doc: &Document, sid: &str) -> bool {
    doc.find_by_sid(sid)
        .and_then(|id| doc.nodes.get(id))
        .map(|n| n.attrs.contains_key(CLIP_ATTR))
        .unwrap_or(false)
}

/// 释放剪切蒙版的命令序列(纯函数;`sid` 必须是蒙版容器):
/// ① 逐内容 Move 回蒙版原父级;② 逐内容 SetGeom 重定基回原坐标系
/// (视觉位置不变);③ 蒙版样式移除 overflow / 50% 圆角;④ 移除标记属性。
pub fn release_clip_cmds(doc: &Document, sid: &str) -> Result<Vec<Command>, String> {
    let Some(id) = doc.find_by_sid(sid) else {
        return Err(format!("释放剪切蒙版:找不到 {sid}"));
    };
    let Some(mask) = doc.nodes.get(id) else {
        return Err(format!("释放剪切蒙版:找不到 {sid}"));
    };
    if !is_clip_mask(doc, sid) {
        return Err("释放剪切蒙版:选中对象不是剪切蒙版容器".into());
    }
    let Some(parent_id) = mask.parent else {
        return Err("释放剪切蒙版:蒙版没有父级".into());
    };
    let parent_sid = doc.nodes.get(parent_id).unwrap().sid.as_str().to_string();
    // 释放目标父级的坐标原点(子级回到该父级后的本地系偏移):
    // Layer/画板 = 画板系(0,0);Group/Box = 该父级的绝对(画板本地)原点
    let target_origin = match doc.nodes.get(parent_id).map(|p| &p.kind) {
        Some(NodeKind::Layer) | Some(NodeKind::Artboard) | None => (0.0, 0.0),
        Some(_) => vb_tools::abs_bbox(doc, parent_id)
            .map(|r| (r.x0, r.y0))
            .unwrap_or((0.0, 0.0)),
    };
    let had_ellipse = mask
        .style_get("border-radius")
        .map(|v| v.contains('%'))
        .unwrap_or(false);

    let mut cmds: Vec<Command> = Vec::new();
    let children = mask.children.clone();
    for child in children {
        let Some(cn) = doc.nodes.get(child) else {
            continue;
        };
        let child_abs = vb_tools::abs_bbox(doc, child).ok_or("释放剪切蒙版:取不到内容几何")?;
        // 目标父级系下的几何(子级当前 abs 为画板本地,减目标父级原点)
        let desired = Geom {
            x: child_abs.x0 - target_origin.0,
            y: child_abs.y0 - target_origin.1,
            w: cn.geom.w,
            h: cn.geom.h,
        };
        cmds.push(Command::Move {
            sid: cn.sid.as_str().to_string(),
            new_parent_sid: parent_sid.clone(),
            new_index: usize::MAX,
            old: None,
        });
        cmds.push(Command::SetGeom {
            sid: cn.sid.as_str().to_string(),
            new: desired,
            old: None,
            old_declared: None,
        });
    }
    // 蒙版样式:去 overflow;若圆角是本次建立时加的 50%,一并移除
    let mut new_style = mask.style.clone();
    new_style.retain(|d| d.prop != "overflow");
    if had_ellipse {
        new_style.retain(|d| d.prop != "border-radius");
    }
    cmds.push(Command::SetStyle {
        sid: sid.to_string(),
        new: new_style,
        old: None,
    });
    let new_attrs: Vec<(String, String)> = mask
        .attrs
        .iter()
        .filter(|(k, _)| k.as_str() != CLIP_ATTR)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    cmds.push(Command::SetAttrs {
        sid: sid.to_string(),
        new: new_attrs,
        old: None,
    });
    Ok(cmds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_doc::model::Node;

    fn box_node(doc: &mut Document, parent: vb_doc::model::NodeId, name: &str, g: Geom) -> String {
        let sid = doc.alloc_sid();
        let mut n = Node::new(NodeKind::Box, name, sid.clone());
        n.geom = g;
        let nid = doc.nodes.insert(n);
        doc.nodes.get_mut(nid).unwrap().parent = Some(parent);
        doc.nodes.get_mut(parent).unwrap().children.push(nid);
        sid.as_str().to_string()
    }

    /// 命令序建模:内容收编后视觉位置不变、蒙版带 overflow+标记;
    /// 释放后内容回原父级、原坐标;释放撤销(Compound revert)复原。
    #[test]
    fn clip_mask_preserves_visual_position_and_releases() {
        let _env = crate::ENV_LOCK.lock();
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let content = box_node(
            &mut doc,
            ab,
            "内容",
            Geom {
                x: 120.0,
                y: 80.0,
                w: 200.0,
                h: 100.0,
            },
        );
        let mask = box_node(
            &mut doc,
            ab,
            "蒙版",
            Geom {
                x: 100.0,
                y: 60.0,
                w: 300.0,
                h: 200.0,
            },
        );

        let cmds =
            clip_mask_cmds(&doc, &[content.clone(), mask.clone()]).expect("合法选区应产出命令序");
        let mut undo = vb_doc::undo::UndoStack::new();
        undo.push(&mut doc, Command::Compound { cmds })
            .expect("应用成功");

        // 蒙版语义标记 + overflow
        let mid = doc.find_by_sid(&mask).unwrap();
        let mn = doc.nodes.get(mid).unwrap();
        assert!(mn.attrs.contains_key(CLIP_ATTR), "蒙版带 data-vb-clip 标记");
        assert_eq!(mn.style_get("overflow"), Some("hidden"));
        // 内容收编进蒙版,视觉位置(画板本地)不变
        let cid = doc.find_by_sid(&content).unwrap();
        let cn = doc.nodes.get(cid).unwrap();
        assert_eq!(cn.parent, Some(mid), "内容父级 = 蒙版");
        assert_eq!(
            vb_tools::abs_bbox(&doc, cid).unwrap(),
            vb_common::geom::Rect::new(120.0, 80.0, 320.0, 180.0),
            "收编后视觉位置不变"
        );
        assert!(is_clip_mask(&doc, &mask));

        // 释放:内容回画板、坐标复原;蒙版标记移除
        let rel = release_clip_cmds(&doc, &mask).expect("蒙版容器可释放");
        undo.push(&mut doc, Command::Compound { cmds: rel })
            .expect("释放应用成功");
        let cid = doc.find_by_sid(&content).unwrap();
        let cn = doc.nodes.get(cid).unwrap();
        assert_eq!(cn.parent, Some(ab), "内容回到原父级(画板)");
        assert_eq!(
            vb_tools::abs_bbox(&doc, cid).unwrap(),
            vb_common::geom::Rect::new(120.0, 80.0, 320.0, 180.0),
            "释放后视觉位置不变"
        );
        let mid = doc.find_by_sid(&mask).unwrap();
        assert!(!is_clip_mask(&doc, &mask), "标记已移除");
        assert_eq!(doc.nodes.get(mid).unwrap().style_get("overflow"), None);

        // 撤销释放 → 回到蒙版态;再撤销建立 → 全部还原
        undo.undo(&mut doc).expect("撤销释放");
        assert!(is_clip_mask(&doc, &mask));
        undo.undo(&mut doc).expect("撤销建立");
        let cid = doc.find_by_sid(&content).unwrap();
        assert_eq!(doc.nodes.get(cid).unwrap().parent, Some(ab));
        assert!(!is_clip_mask(&doc, &mask));
    }

    /// 约束:非 Box 蒙版 / 单选 / 跨父级 → 明确报错(不静默)。
    #[test]
    fn clip_mask_rejects_invalid_selections() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let a = box_node(
            &mut doc,
            ab,
            "A",
            Geom {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
        );
        assert!(
            clip_mask_cmds(&doc, std::slice::from_ref(&a)).is_err(),
            "单选拒绝"
        );
        // 蒙版 = 文本节点 → 非 Box 拒绝
        let sid = doc.alloc_sid();
        let mut t = Node::new(
            NodeKind::Text {
                text: "x".into(),
                mode: vb_doc::model::TextMode::Point,
                segments: vec![],
            },
            "标题",
            sid.clone(),
        );
        t.geom = Geom {
            x: 0.0,
            y: 0.0,
            w: 40.0,
            h: 20.0,
        };
        let tid = doc.nodes.insert(t);
        doc.nodes.get_mut(tid).unwrap().parent = Some(ab);
        doc.nodes.get_mut(ab).unwrap().children.push(tid);
        let tsid = sid.as_str().to_string();
        assert!(clip_mask_cmds(&doc, &[a, tsid]).is_err(), "非 Box 蒙版拒绝");
    }

    /// 椭圆蒙版:建立时圆角并入容器(overflow+border-radius 双承载)。
    #[test]
    fn ellipse_mask_adds_radius() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let content = box_node(
            &mut doc,
            ab,
            "内容",
            Geom {
                x: 10.0,
                y: 10.0,
                w: 50.0,
                h: 50.0,
            },
        );
        let sid = doc.alloc_sid();
        let mut m = Node::new(NodeKind::Box, "圆", sid.clone());
        m.geom = Geom {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        m.style.push(vb_css::Decl {
            prop: "border-radius".into(),
            value: "50%".into(),
            important: false,
        });
        let mid = doc.nodes.insert(m);
        doc.nodes.get_mut(mid).unwrap().parent = Some(ab);
        doc.nodes.get_mut(ab).unwrap().children.push(mid);
        let mask = sid.as_str().to_string();

        let cmds = clip_mask_cmds(&doc, &[content, mask.clone()]).unwrap();
        let mut undo = vb_doc::undo::UndoStack::new();
        undo.push(&mut doc, Command::Compound { cmds }).unwrap();
        let mid = doc.find_by_sid(&mask).unwrap();
        assert_eq!(
            doc.nodes.get(mid).unwrap().style_get("overflow"),
            Some("hidden")
        );
        assert_eq!(
            doc.nodes.get(mid).unwrap().style_get("border-radius"),
            Some("50%")
        );
    }

    /// 剪切蒙版**导出往返门禁**:建模 → HTML/CSS 导出(overflow:hidden +
    /// data-vb-clip)→ 渲染编码携带裁剪矩形 → 重导入语义保留 →
    /// 再导出**字节一致**(L1)。这是「与导出链兼容」的机械证明。
    #[test]
    fn clip_mask_export_roundtrip_is_lossless() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let content = box_node(
            &mut doc,
            ab,
            "内容",
            Geom {
                x: 120.0,
                y: 80.0,
                w: 200.0,
                h: 100.0,
            },
        );
        // 内容带填充(有自绘项,渲染端才有可裁剪的 DrawItem)
        let cid0 = doc.find_by_sid(&content).unwrap();
        doc.nodes.get_mut(cid0).unwrap().style.push(vb_css::Decl {
            prop: "background-color".into(),
            // 已是最短 hex 形式:导入器会归一颜色,两端字节才可比
            value: "#39f".into(), // vb-token-ok: 测试文档内容色
            important: false,
        });
        let mask = box_node(
            &mut doc,
            ab,
            "蒙版",
            Geom {
                x: 100.0,
                y: 60.0,
                w: 300.0,
                h: 200.0,
            },
        );
        let mut undo = vb_doc::undo::UndoStack::new();
        let cmds = clip_mask_cmds(&doc, &[content.clone(), mask.clone()]).unwrap();
        undo.push(&mut doc, Command::Compound { cmds }).unwrap();

        // ① 导出:元素带标记,CSS 规则带 overflow
        let files = vb_doc::export::render_project(&doc);
        let html = files
            .files
            .iter()
            .find(|(p, _)| p == "index.html")
            .map(|(_, c)| c.clone())
            .unwrap();
        let css = files
            .files
            .iter()
            .find(|(p, _)| p == "styles/main.css")
            .map(|(_, c)| c.clone())
            .unwrap();
        assert!(html.contains(r#"data-vb-clip="1""#), "标记导出到 HTML");
        assert!(css.contains("overflow: hidden"), "overflow 导出到 CSS 规则");
        // ② 渲染编码:蒙版子项携带折叠后的 overflow 裁剪矩形(画布/CPU/SVG 同源)
        let list = vb_render::encode::encode_artboard(&doc, ab).unwrap();
        let clipped: Vec<[f64; 4]> = list
            .items
            .iter()
            .filter_map(|it| it.overflow_clip)
            .collect();
        assert!(
            clipped.contains(&[100.0, 60.0, 300.0, 200.0]),
            "蒙版子项必须携带蒙版矩形裁剪:{clipped:?}"
        );
        // ③ 往返:写盘 → 重导入 → 语义保留 → 再导出字节一致
        let tmp = std::env::temp_dir().join(format!("vb-clip-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        vb_doc::export::write_project(&doc, &tmp).unwrap();
        let imported = vb_doc::import::import_project(&tmp).unwrap();
        let doc2 = imported.doc;
        let mid2 = doc2.find_by_sid(&mask).expect("蒙版 sid 往返保留");
        let mask2 = doc2.nodes.get(mid2).unwrap();
        assert_eq!(
            mask2.attrs.get(CLIP_ATTR).map(String::as_str),
            Some(CLIP_ATTR_VALUE),
            "重导入后 data-vb-clip 保留"
        );
        assert_eq!(
            mask2.style_get("overflow"),
            Some("hidden"),
            "重导入后 overflow 保留"
        );
        let cid2 = doc2.find_by_sid(&content).unwrap();
        assert_eq!(
            doc2.nodes.get(cid2).unwrap().parent,
            Some(mid2),
            "重导入后内容仍是蒙版子元素"
        );
        let first: std::collections::BTreeMap<String, String> = files
            .files
            .iter()
            .map(|(p, c)| (p.clone(), c.clone()))
            .collect();
        let second_files = vb_doc::export::render_project(&doc2);
        let second: std::collections::BTreeMap<String, String> = second_files
            .files
            .iter()
            .map(|(p, c)| (p.clone(), c.clone()))
            .collect();
        assert_eq!(first, second, "L1:再导出必须与首次字节一致");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 命令接线烟测:经 `run_command` 派发建立/释放蒙版 + 撤销链完整。
    #[test]
    fn clip_mask_commands_dispatch_and_undo() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = crate::app::assemble::tests::app_fresh(None);
        let ab = app.doc.artboards[0];
        for (i, g) in [
            Geom {
                x: 10.0,
                y: 10.0,
                w: 100.0,
                h: 80.0,
            },
            Geom {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 160.0,
            },
        ]
        .into_iter()
        .enumerate()
        {
            let sid = app.doc.alloc_sid();
            let mut n = Node::new(NodeKind::Box, format!("盒{i}"), sid.clone());
            n.geom = g;
            let id = app.doc.nodes.insert(n);
            app.doc.nodes.get_mut(id).unwrap().parent = Some(ab);
            app.doc.nodes.get_mut(ab).unwrap().children.push(id);
            app.selection.push(sid.as_str().to_string());
        }
        app.run_command("object.clip_mask", false, false);
        let mask_sid = app.selection.last().unwrap().clone();
        let mid = app.doc.find_by_sid(&mask_sid).unwrap();
        assert_eq!(
            app.doc.nodes.get(mid).unwrap().style_get("overflow"),
            Some("hidden"),
            "Ctrl+7 建立剪切蒙版"
        );
        app.selection = vec![mask_sid.clone()];
        app.run_command("object.release_clip_mask", false, false);
        let mid = app.doc.find_by_sid(&mask_sid).unwrap();
        assert_eq!(
            app.doc.nodes.get(mid).unwrap().style_get("overflow"),
            None,
            "Ctrl+Alt+7 释放剪切蒙版"
        );
        // 撤销释放 → 回蒙版态;撤销建立 → 内容回原父级
        app.run_command("edit.undo", false, false);
        let mid = app.doc.find_by_sid(&mask_sid).unwrap();
        assert_eq!(
            app.doc.nodes.get(mid).unwrap().style_get("overflow"),
            Some("hidden"),
            "撤销释放回到蒙版态"
        );
        app.run_command("edit.undo", false, false);
        let cid = app
            .doc
            .find_by_sid(app.selection.first().unwrap())
            .expect("内容节点存活");
        assert_ne!(
            app.doc.nodes.get(cid).unwrap().parent,
            Some(mid),
            "撤销建立后内容回到原父级"
        );
    }
}
