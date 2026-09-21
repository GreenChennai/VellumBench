//! P0-1 无标记稿件保真往返回归(08a 探针 → 08c 修复)。
//!
//! 夹具全部**字节原样**复制自 artboard 真实产出(探针报告 §样稿来源):
//! - `fixtures/p01/s2-kv.html`      ← artboard-studio/s2-kv/src/index.html(1920×1080)
//! - `fixtures/p01/show2-card.html` ← .agents/skills/artboard/assets/cases/show2-card.html(1063×638)
//! - `fixtures/p01/cf-cover-src/`   ← artboard-studio/cf-cover/src(项目目录形态,1080×1920)
//!
//! s2-kv/cf-cover 的字体文件(24MB)未随库分发:文本度量走系统兜底字体,
//! 文字几何断言全部采用**锚定边/相对关系**等字体无关口径。
//!
//! 断言覆盖(08c 任务书):
//! - 打开:无标记 → 启发式识别画板(尺寸=声明值),缺标记告警不静默;
//! - 几何:绝对(px)/百分比/inset/right|bottom 锚/流式(flex)与 CSS 声明语义一致;
//! - L0:不编辑直接保存,原始声明原样写回(无 100×100 烤入、无声明丢失);
//! - L1:编辑(改文字+改填充)保存 → 再开再存字节幂等;
//! - vb- 契约:保存写 vb-artboard/vb-group,无 vs-/vsm- 残留,data-vb-id 编辑不变。

use std::path::{Path, PathBuf};

use vb_doc::commands::Command;
use vb_doc::export::render_project;
use vb_doc::import::import_project;
use vb_doc::model::{Document, Geom, NodeKind};
use vb_doc::undo::UndoStack;

// ---------- 夹具装载(复制到临时目录,避免写回仓库) ----------

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/p01")
}

fn unique_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("vb-p01-{}-{tag}-{n}", std::process::id()))
}

fn load_single_file(name: &str) -> (Document, Vec<String>, PathBuf) {
    let dir = unique_dir(name.trim_end_matches(".html"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(fixture_dir().join(name), dir.join("index.html")).unwrap();
    import_with_layout(&dir)
}

fn load_project(rel: &str) -> (Document, Vec<String>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "vb-p01-{}-{}",
        std::process::id(),
        rel.replace(['/', '\\'], "-")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(rel)).unwrap();
    for f in ["index.html", "person.png"] {
        std::fs::copy(fixture_dir().join(rel).join(f), dir.join(rel).join(f)).unwrap();
    }
    import_with_layout(&dir.join(rel))
}

/// 与 vellum-cli open_doc 同口径:导入 + 内存布局求值。
fn import_with_layout(dir: &Path) -> (Document, Vec<String>, PathBuf) {
    let mut r = import_project(dir).expect("导入失败");
    let warnings = r.warnings.clone();
    let dir_out = r.project_dir.clone();
    let synthetic = r.synthetic_artboard;
    let mut lw = vb_layout::apply_import_layout(&mut r.doc, Some(&dir_out), synthetic);
    let mut all = warnings;
    all.append(&mut lw);
    (r.doc, all, dir_out)
}

/// 按类名找节点(命中首个)。
fn find_by_class(doc: &Document, class: &str) -> vb_doc::model::NodeId {
    doc.nodes
        .iter()
        .find(|(_, n)| n.classes.iter().any(|c| c == class))
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("找不到类 {class} 的节点"))
}

fn node_geom(doc: &Document, id: vb_doc::model::NodeId) -> Geom {
    doc.node(id).unwrap().geom
}

fn approx(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// ---------- 1. 画板启发式识别 ----------

#[test]
fn p01_unmarked_poster_recognized_as_artboard() {
    for (file, (w, h)) in [
        ("s2-kv.html", (1920.0, 1080.0)),
        ("show2-card.html", (1063.0, 638.0)),
    ] {
        let (doc, warnings, _) = load_single_file(file);
        assert_eq!(doc.artboards.len(), 1, "{file}: 画板数应为 1");
        let ab = doc.node(doc.artboards[0]).unwrap();
        assert!(
            matches!(ab.kind, NodeKind::Artboard),
            "{file}: 顶层容器应识别为 Artboard"
        );
        assert!(
            approx(ab.geom.w, w, 0.01) && approx(ab.geom.h, h, 0.01),
            "{file}: 画板尺寸应为声明值 {w}×{h},实际 {}×{}",
            ab.geom.w,
            ab.geom.h
        );
        assert_eq!(
            ab.tag, "div",
            "{file}: 序列化标签必须保真(div 不得改写 section)"
        );
        assert!(
            warnings.iter().any(|w| w.contains("画板标记")),
            "{file}: 缺标记必须告警(不静默):{warnings:?}"
        );
    }
}

#[test]
fn p01_unmarked_project_dir_recognized_as_artboard() {
    let (doc, warnings, _) = load_project("cf-cover-src");
    assert_eq!(doc.artboards.len(), 1);
    let ab = doc.node(doc.artboards[0]).unwrap();
    assert!(matches!(ab.kind, NodeKind::Artboard));
    assert!(
        approx(ab.geom.w, 1080.0, 0.01) && approx(ab.geom.h, 1920.0, 0.01),
        "项目目录形态:画板尺寸应为声明值 1080×1920,实际 {}×{}",
        ab.geom.w,
        ab.geom.h
    );
    assert!(warnings.iter().any(|w| w.contains("画板标记")));
}

// ---------- 2. 几何与 CSS 声明语义一致(绝对/百分比/inset/锚定/流式) ----------

#[test]
fn p01_geometry_matches_css_semantics_s2kv() {
    let (doc, _, _) = load_single_file("s2-kv.html");

    // inset:0 全出血层(.l1)→ 铺满画板,不再是 100×100 小块
    let l1 = node_geom(&doc, find_by_class(&doc, "l1"));
    assert!(
        approx(l1.x, 0.0, 0.01) && approx(l1.y, 0.0, 0.01),
        "inset:0 层塌到了 ({},{})",
        l1.x,
        l1.y
    );
    assert!(approx(l1.w, 1920.0, 0.01) && approx(l1.h, 1080.0, 0.01));

    // 百分比锚(.orbit.o1:left:50%;top:46%;width:560px;height:560px)
    // **阶段 2(03)起 `transform: translate(...)` 参与画布几何折算**:
    // 本例还有 `translate(-50%,-50%)` = (-280,-280),故画布坐标 =
    // (960-280, 496.8-280) —— 这正是浏览器里的视觉位置(此前差半宽/半高,
    // 判据 A「画布与浏览器一致」不成立)。
    let o1 = node_geom(&doc, find_by_class(&doc, "o1"));
    assert!(
        approx(o1.x, 960.0 - 280.0, 0.6) && approx(o1.y, 496.8 - 280.0, 0.6),
        "orbit 百分比锚 + translate 折算错: {o1:?}"
    );
    assert!(approx(o1.w, 560.0, 0.01) && approx(o1.h, 560.0, 0.01));

    // 百分比小元素(.d1:left:22%;top:26%;width:5px;height:5px)
    let d1 = node_geom(&doc, find_by_class(&doc, "d1"));
    assert!(approx(d1.x, 0.22 * 1920.0, 0.6) && approx(d1.y, 0.26 * 1080.0, 0.6));
    assert!(approx(d1.w, 5.0, 0.01) && approx(d1.h, 5.0, 0.01));

    // right/bottom 锚(.wall:left:96px;right:96px;bottom:60px)
    let wall = node_geom(&doc, find_by_class(&doc, "wall"));
    assert!(approx(wall.x, 96.0, 0.01), "wall 左锚错: {wall:?}");
    assert!(
        approx(wall.w, 1920.0 - 192.0, 0.01),
        "wall 左右锚定宽错: {wall:?}"
    );
    assert!(
        approx(wall.y + wall.h, 1080.0 - 60.0, 0.5),
        "wall 底锚(距底 60px)错: {wall:?}"
    );

    // 绝对 px 锚(.logo:top:60px;left:96px)
    let logo = node_geom(&doc, find_by_class(&doc, "logo"));
    assert!(approx(logo.x, 96.0, 0.01) && approx(logo.y, 60.0, 0.01));

    // 流式子元素(.wall > .tier ×2):纵向堆叠,不再全部叠在 (0,0)
    let wall_id = find_by_class(&doc, "wall");
    let tiers: Vec<Geom> = doc
        .node(wall_id)
        .unwrap()
        .children
        .iter()
        .filter(|&&c| {
            doc.node(c)
                .map(|n| n.classes.iter().any(|c| c == "tier"))
                .unwrap_or(false)
        })
        .map(|&c| node_geom(&doc, c))
        .collect();
    assert!(tiers.len() >= 2, "应有两行 tier");
    assert!(
        tiers[1].y > tiers[0].y + 10.0,
        "流式行必须纵向堆叠: {tiers:?}"
    );
    assert!(tiers[0].y > 0.0 || tiers[0].x > 0.0, "流式行塌到画布原点");
}

#[test]
fn p01_geometry_matches_css_semantics_show2card() {
    let (doc, _, _) = load_single_file("show2-card.html");

    // inset:22px 边框层
    let frame = node_geom(&doc, find_by_class(&doc, "frame"));
    assert!(
        approx(frame.x, 22.0, 0.01)
            && approx(frame.y, 22.0, 0.01)
            && approx(frame.w, 1063.0 - 44.0, 0.01)
            && approx(frame.h, 638.0 - 44.0, 0.01),
        "inset:22px 求值错: {frame:?}"
    );

    // right/bottom 四角(绝对 px 类 .c1 已由折叠路径覆盖;此处验证反向锚)
    let c1 = node_geom(&doc, find_by_class(&doc, "c1"));
    assert!(
        approx(c1.x, 22.0, 0.01) && approx(c1.y, 22.0, 0.01),
        "左上角错: {c1:?}"
    );
    let c2 = node_geom(&doc, find_by_class(&doc, "c2"));
    assert!(
        approx(c2.x, 1063.0 - 22.0 - 26.0, 0.01) && approx(c2.y, 22.0, 0.01),
        "右上角(right:22px)错: {c2:?}"
    );
    let c4 = node_geom(&doc, find_by_class(&doc, "c4"));
    assert!(
        approx(c4.x, 1063.0 - 22.0 - 26.0, 0.01) && approx(c4.y + c4.h, 638.0 - 22.0, 0.5),
        "右下角(right:22px;bottom:22px)错: {c4:?}"
    );

    // 底部锚文字(.firm:left:80px;bottom:52px):底边距画板底 52px(字体无关)
    let firm = node_geom(&doc, find_by_class(&doc, "firm"));
    assert!(approx(firm.x, 80.0, 0.01));
    assert!(
        approx(firm.y + firm.h, 638.0 - 52.0, 0.6),
        "firm 底锚错: {firm:?}"
    );

    // 绝对定位容器内的流式子元素(.contact > div ×3):纵向堆叠、左对齐容器
    let contact_id = find_by_class(&doc, "contact");
    let rows: Vec<Geom> = doc
        .node(contact_id)
        .unwrap()
        .children
        .iter()
        .map(|&c| node_geom(&doc, c))
        .collect();
    assert!(rows.len() >= 3, "联系行应有 3 行");
    for w in &rows {
        assert!(w.x.abs() < 0.6, "流式行 x 应对齐容器左缘(父相对): {w:?}");
    }
    assert!(
        rows[1].y > rows[0].y + 30.0 && rows[2].y > rows[1].y + 30.0,
        "流式联系行必须纵向堆叠: {rows:?}"
    );
}

// ---------- 3. L0:不编辑直接保存,原始声明原样写回 ----------

#[test]
fn p01_l0_save_preserves_original_declarations() {
    let (doc, _, _) = load_single_file("s2-kv.html");
    let out = render_project(&doc);
    let html = out
        .files
        .iter()
        .find(|(p, _)| p == "index.html")
        .unwrap()
        .1
        .clone();
    let css = out
        .files
        .iter()
        .find(|(p, _)| p == "styles/main.css")
        .unwrap()
        .1
        .clone();

    // 画板尺寸 = 声明值(kiln/browser 同口径)
    assert!(css.contains("width: 1920px"), "画板声明宽度丢失");
    assert!(css.contains("height: 1080px"), "画板声明高度丢失");

    // 原始声明保真:百分比锚 / inset / 反向锚 / 变换装饰
    for needle in [
        "left: 50%",
        "top: 46%",                         // orbit / planet 百分比锚
        "inset: 0",                         // l1/l2 全出血层
        "transform: translate(-50%, -50%)", // 未知属性原样保留
        "right: 96px",
        "bottom: 60px", // wall 反向锚
    ] {
        assert!(css.contains(needle), "L0 丢失原始声明:{needle}");
    }

    // 反向证据:塌缩烤入签名(默认 100×100 补写)不得出现
    assert!(!css.contains("width: 100px"), "出现 100×100 烤入(width)");
    assert!(!css.contains("height: 100px"), "出现 100×100 烤入(height)");
    assert!(
        !css.contains("left: 0px;\n  top: 0px"),
        "百分比锚被烤写为 (0,0)"
    );

    // 文本保真
    for t in [
        "万物有梧",
        "智联可栖",
        "青梧智联 GREENPARASOL",
        "战略合作伙伴",
    ] {
        assert!(html.contains(t), "L0 丢失文本:{t}");
    }
    // data-vb-id 已由保存织入(Agent 寻址契约)
    assert!(html.contains("data-vb-id="));
}

#[test]
fn p01_l0_declaration_sets_source_subset_of_output() {
    // 机械化口径:源 CSS 声明集(类规则 ∪ 链规则 ∪ 行内)⊆ 导出声明集
    // (∆ 仅允许导出基类 .vb-artboard 的 position:relative/overflow:hidden)
    for file in ["s2-kv.html", "show2-card.html"] {
        let src = std::fs::read_to_string(fixture_dir().join(file)).unwrap();
        let (doc, _, _) = load_single_file(file);
        let out = render_project(&doc);
        let css_out = out
            .files
            .iter()
            .find(|(p, _)| p == "styles/main.css")
            .map(|(_, c)| c.clone())
            .unwrap_or_default();

        let canon = |s: &str| {
            s.split(';')
                .map(|chunk| {
                    // 去 selector 前缀与收尾 '}'(:root/.l1 { position: absolute …)
                    let d = match chunk.rsplit_once('{') {
                        Some((_, d)) => d,
                        None => chunk,
                    };
                    match d.rsplit_once('}') {
                        Some((d, _)) => d,
                        None => d,
                    }
                })
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .filter_map(vb_css::Decl::parse)
                .map(|d| d.to_css())
                .collect::<std::collections::BTreeSet<_>>()
        };
        let out_decls = canon(&css_out);
        // ① 无丢失(节点级口径):每个可建模节点(非 #text;行内文本段样式
        //    经 SegStyle 走 HTML span 内联,属既有机制)的声明都必须在导出 CSS 中
        for (_, n) in doc.nodes.iter() {
            if n.tag == "#text" || matches!(n.kind, NodeKind::Frozen { .. }) {
                continue;
            }
            for d in &n.style {
                assert!(
                    out_decls.contains(&d.to_css()),
                    "{file}: 导出丢失节点「{}」的声明 {}",
                    n.name,
                    d.to_css()
                );
            }
        }
        // ② 无烤入(全文件口径):导出出现而源没有的声明,只允许画板基类两条
        //    (源声明集含类规则/链规则/at-rule/raw 全部,排除 body/html 规则)
        let sheet = vb_doc::import::parse_stylesheet(&src);
        let mut src_decls = std::collections::BTreeSet::new();
        let mut has_font_shorthand = false;
        for (_, _, decls) in &sheet.class_rules {
            for d in decls {
                if d.prop == "font" {
                    has_font_shorthand = true;
                }
                src_decls.insert(d.to_css());
            }
        }
        for r in &sheet.rules {
            if r.chain.iter().all(|c| {
                c.classes.is_empty()
                    && c.tag
                        .as_deref()
                        .map(|t| t == "body" || t == "html")
                        .unwrap_or(false)
            }) {
                continue;
            }
            for d in &r.decls {
                src_decls.insert(d.to_css());
            }
        }
        for b in &sheet.raw_blocks {
            for d in canon(b) {
                src_decls.insert(d);
            }
        }
        // :root 设计令牌(导出侧按令牌表重写)
        for (k, v) in &sheet.root_vars {
            if let Some(d) = vb_css::Decl::parse(&format!("--{k}: {v}")) {
                src_decls.insert(d.to_css());
            }
        }
        let allowed: std::collections::BTreeSet<String> =
            ["position: relative", "overflow: hidden"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        // `font` 简写在导入期展开为单项声明(canonical 正规化,见
        // import::expand_font_shorthand),展开产物不视为烤入
        let font_expanded: std::collections::BTreeSet<String> = [
            "font-size",
            "line-height",
            "font-weight",
            "font-style",
            "font-family",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        for d in &out_decls {
            let font_ok = has_font_shorthand
                && d.split(':')
                    .next()
                    .map(|p| font_expanded.contains(p.trim()))
                    .unwrap_or(false);
            assert!(
                src_decls.contains(d) || allowed.contains(d) || font_ok,
                "{file}: 导出凭空新增声明(计算几何烤入?):{d}"
            );
        }
    }
}

// ---------- 4. L1:编辑(文字+填充)保存 → 再开再存字节幂等 ----------

#[test]
fn p01_l1_edit_then_resave_byte_idempotent() {
    let (mut doc, _, dir) = load_single_file("s2-kv.html");

    // 编辑一:改文字(.kick 的文本段)
    let text_id = doc
        .nodes
        .iter()
        .find(|(_, n)| n.text() == Some("青梧智联 · IoT 峰会 2026"))
        .map(|(id, _)| id)
        .expect("找不到 kick 文本节点");
    let mut undo = UndoStack::new();
    let text_sid = doc.node(text_id).unwrap().sid.as_str().to_string();
    undo.push(
        &mut doc,
        Command::SetText {
            sid: text_sid,
            new: "青梧智联 · IoT 峰会 2027".into(),
            old: None,
        },
    )
    .expect("set_text 失败");

    // 编辑二:改填充(.l1 的 background-color)
    let l1_id = find_by_class(&doc, "l1");
    let l1 = doc.node(l1_id).unwrap();
    let mut style = l1.style.clone();
    let prop = "background-color";
    if let Some(d) = style.iter_mut().find(|d| d.prop == prop) {
        d.value = "#123456".into();
    } else {
        style.push(vb_css::Decl {
            prop: prop.into(),
            value: "#123456".into(),
            important: false,
        });
    }
    let sid = l1.sid.as_str().to_string();
    undo.push(
        &mut doc,
        Command::SetStyle {
            sid,
            new: style,
            old: None,
        },
    )
    .expect("set_style 失败");

    // 第一次导出(保存)
    let first = render_project(&doc);
    // 落盘 → 再打开(同 vellum-cli 口径:导入 + 布局)→ 再保存
    for (rel, content) in &first.files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }
    let (doc2, _, _) = import_with_layout(&dir);
    let second = render_project(&doc2);

    assert_eq!(first.files.len(), second.files.len(), "文件数不一致");
    for (a, b) in first.files.iter().zip(second.files.iter()) {
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, b.1, "L1 幂等失败:{} 两轮保存字节不同", a.0);
    }
    // 编辑确实落了(防「假幂等」:编辑没生效导致两轮相同)
    let css = &first
        .files
        .iter()
        .find(|(p, _)| p == "styles/main.css")
        .unwrap()
        .1;
    assert!(css.contains("#123456"), "填充编辑未落盘");
    let html = &first
        .files
        .iter()
        .find(|(p, _)| p == "index.html")
        .unwrap()
        .1;
    assert!(html.contains("IoT 峰会 2027"), "文字编辑未落盘");
    // 原始几何声明仍保真(编辑的是文字/填充,不是几何)
    assert!(css.contains("left: 50%") && css.contains("inset: 0"));
}

// ---------- 5. vb- 契约(08-5) ----------

#[test]
fn p01_vb_contract_markers_and_stable_ids() {
    let (mut doc, _, _) = load_single_file("show2-card.html");

    // 未编辑保存:画板带 vb-artboard;全文无 vs-/vsm- 残留
    let sids_before: Vec<String> = doc
        .nodes
        .values()
        .map(|n| n.sid.as_str().to_string())
        .collect();
    let out1 = render_project(&doc);
    let html1 = out1
        .files
        .iter()
        .find(|(p, _)| p == "index.html")
        .unwrap()
        .1
        .clone();
    assert!(html1.contains("vb-artboard"), "保存必须写 vb-artboard 标记");
    for residue in [
        "vs-artboard",
        "vsm-artboard",
        "vs-layer",
        "vsm-layer",
        "vs-group",
        "vsm-group",
    ] {
        assert!(!html1.contains(residue), "保存残留旧前缀:{residue}");
    }

    // 编辑:改名 + 移动 + 重排 + 编组(角标两枚)
    let mut undo = UndoStack::new();
    let name_id = find_by_class(&doc, "name");
    let c3 = find_by_class(&doc, "c3");
    let c4 = find_by_class(&doc, "c4");
    let name_sid = doc.node(name_id).unwrap().sid.as_str().to_string();
    let c3_sid = doc.node(c3).unwrap().sid.as_str().to_string();
    let c4_sid = doc.node(c4).unwrap().sid.as_str().to_string();
    let group_sid = "zz0group";
    let _parent_sid = doc
        .node(c3)
        .unwrap()
        .parent
        .and_then(|p| doc.node(p))
        .map(|p| p.sid.as_str().to_string())
        .unwrap();
    for c in [
        Command::Rename {
            sid: name_sid.clone(),
            new: "主名".into(),
            old: None,
        },
        Command::SetGeom {
            sid: name_sid.clone(),
            new: Geom {
                x: 90.0,
                y: 240.0,
                w: 104.0,
                h: 62.0,
            },
            old: None,
            old_declared: None,
        },
        Command::Group {
            member_sids: vec![c3_sid.clone(), c4_sid.clone()],
            name: "角标".into(),
            group_sid: group_sid.into(),
            old_slots: None,
        },
    ] {
        undo.push(&mut doc, c).expect("命令失败");
    }

    let out2 = render_project(&doc);
    let html2 = out2
        .files
        .iter()
        .find(|(p, _)| p == "index.html")
        .unwrap()
        .1
        .clone();

    // data-vb-id 全体不变(改名/移动/编组都不换身份)
    let sids_after: Vec<String> = doc
        .nodes
        .values()
        .map(|n| n.sid.as_str().to_string())
        .collect();
    assert_eq!(
        sids_before.len(),
        sids_after.len() - 1,
        "编组应恰好新增 1 个节点 sid"
    );
    for s in &sids_before {
        assert!(sids_after.contains(s), "sid 漂移:{s}");
    }
    assert!(html2.contains(&format!(r#"data-vb-id="{name_sid}""#)));

    // 编组落盘必须写 vb-group(此前写裸 div,重开降级 Box)
    assert!(html2.contains("vb-group"), "编组保存必须写 vb-group 标记");

    // 编辑态 L1:再开再存字节幂等(组语义经往返保持)
    let dir = unique_dir("contract");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (rel, content) in &out2.files {
        std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), content).unwrap();
    }
    let (doc3, _, _) = import_with_layout(&dir);
    // vb-group 重开仍是编组
    let group_node = doc3
        .nodes
        .iter()
        .find(|(_, n)| matches!(n.kind, NodeKind::Group))
        .map(|(_, n)| n.sid.as_str().to_string());
    assert!(group_node.is_some(), "vb-group 重开应识别为编组");
    let out3 = render_project(&doc3);
    let html3 = out3
        .files
        .iter()
        .find(|(p, _)| p == "index.html")
        .unwrap()
        .1
        .clone();
    let css2 = out2
        .files
        .iter()
        .find(|(p, _)| p == "styles/main.css")
        .unwrap()
        .1
        .clone();
    let css3 = out3
        .files
        .iter()
        .find(|(p, _)| p == "styles/main.css")
        .unwrap()
        .1
        .clone();
    assert_eq!(html2, html3, "编辑态 L1:HTML 两轮保存字节不同");
    assert_eq!(css2, css3, "编辑态 L1:CSS 两轮保存字节不同");
}
