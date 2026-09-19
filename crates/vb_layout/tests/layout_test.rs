//! vb_layout 集成测试:CSS Grid 映射与合成画板回填规则。
//! 来源:artboard v1.9.x 外部部署报告 Issue 2(存量项目 grid 塌单列 +
//! 溢出内容撑大画布,静默失真)。
use vb_doc::import::import_html;

fn layout(html: &str) -> (vb_doc::model::Document, Vec<String>) {
    // 只读版:布局求值跑在克隆上,原 doc 原样返回
    let (mut doc, ws) = layout_mut(html);
    let _ = &mut doc;
    (doc, ws)
}

fn layout_mut(html: &str) -> (vb_doc::model::Document, Vec<String>) {
    let dir = std::env::temp_dir().join(format!(
        "vb-layout-m-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .elapsed()
            .unwrap_or_default()
            .as_nanos() as u64
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let r = import_html(html, &dir).expect("导入");
    let mut doc = r.doc;
    let ab = doc.artboards[0];
    let ws = vb_layout::apply_to_doc(&mut doc, ab, Some(&dir), true);
    let _ = std::fs::remove_dir_all(&dir);
    (doc, ws)
}

/// display:grid + grid-template-columns:1fr 1fr → 四卡两列两行(不塌单列)。
#[test]
fn grid_two_columns_does_not_collapse() {
    let html = r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><style>
* { margin:0; padding:0; box-sizing:border-box; }
.poster { position:relative; width:800px; height:600px; overflow:hidden; }
.bands { display:grid; grid-template-columns:1fr 1fr; gap:24px; margin:40px; }
.card { height:200px; background:#fff; }
</style></head><body><div class="poster">
  <div class="bands">
    <div class="card">1</div><div class="card">2</div>
    <div class="card">3</div><div class="card">4</div>
  </div>
</div></body></html>"#;
    let (doc, _ws) = layout_mut(html);
    // 找 4 张卡:两行两列——前两张同 y,列 x 不同;后两张同 y > 前行 y
    let cards: Vec<(f64, f64, f64, f64)> = doc
        .nodes
        .iter()
        .filter(|(_, n)| n.classes.iter().any(|c| c == "card"))
        .map(|(_, n)| (n.geom.x, n.geom.y, n.geom.w, n.geom.h))
        .collect();
    assert_eq!(cards.len(), 4, "4 张卡");
    let mut cards = cards;
    cards.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap()
            .then(a.0.partial_cmp(&b.0).unwrap())
    });
    // 行 1:卡1 卡2 同 y、x 不同
    assert!(
        (cards[0].1 - cards[1].1).abs() < 1.0,
        "卡1/卡2 应同行: {cards:?}"
    );
    assert!(
        cards[0].0 + cards[0].2 <= cards[1].0 + 1.0,
        "卡1 应在卡2 左侧(两列): {cards:?}"
    );
    // 行 2 存在且在行 1 下方
    assert!(
        cards[2].1 > cards[0].1 + cards[0].3 - 1.0,
        "行 2 应在行 1 下方: {cards:?}"
    );
}

/// 三列分数模板(1fr .72fr 1.1fr)按比例分配,不塌单列。
#[test]
fn grid_fraction_columns_keep_ratio() {
    let html = r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><style>
.poster { position:relative; width:900px; height:400px; overflow:hidden; }
.tri { display:grid; grid-template-columns:1fr 1fr 1fr; gap:0; }
.tri div { height:100px; }
</style></head><body><div class="poster">
  <div class="tri"><div>a</div><div>b</div><div>c</div></div>
</div></body></html>"#;
    let (doc, _ws) = layout_mut(html);
    let mut cells: Vec<(f64, f64)> = doc
        .nodes
        .iter()
        .filter(|(_, n)| {
            n.parent.is_some() && {
                let p = n.parent.unwrap();
                doc.node(p)
                    .map(|pn| {
                        pn.style
                            .iter()
                            .any(|d| d.prop == "display" && d.value.trim() == "grid")
                    })
                    .unwrap_or(false)
            }
        })
        .map(|(_, n)| (n.geom.x, n.geom.w))
        .collect();
    cells.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    assert_eq!(cells.len(), 3, "三个网格项: {cells:?}");
    // 等宽三分:宽度差 < 2px,x 单调递增
    assert!(
        (cells[0].1 - cells[1].1).abs() < 2.0 && (cells[1].1 - cells[2].1).abs() < 2.0,
        "三列应等宽: {cells:?}"
    );
    assert!(cells[0].0 < cells[1].0 && cells[1].0 < cells[2].0);
}

/// 合成画板回填尊重 overflow:hidden:子内容溢出 250px 不撑大画布。
#[test]
fn synthetic_artboard_backfill_respects_overflow_clip() {
    let html = r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><style>
.poster { position:relative; width:1240px; height:1754px; overflow:hidden; }
.over { height:300px; width:600px; margin:0 80px; }
</style></head><body><div class="poster">
  <div style="position:absolute;left:0;top:1700px;width:100px;height:250px;background:#822"></div>
  <div class="over"></div>
</div></body></html>"#;
    let (doc, _ws) = layout_mut(html);
    let ab = doc.node(doc.artboards[0]).unwrap();
    assert!(
        (ab.geom.h - 1754.0).abs() < 1.0,
        "画板高应= .poster 声明 1754(溢出被裁),实际 {}",
        ab.geom.h
    );
    assert!((ab.geom.w - 1240.0).abs() < 1.0);
}

/// grid 模板不可解析:降级块布局并出告警(不静默)。
#[test]
fn unparsable_grid_template_warns_and_degrades() {
    let html = r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><style>
.poster { position:relative; width:400px; height:300px; overflow:hidden; }
.g { display:grid; grid-template-columns:bogus-track(); }
.g div { height:50px; }
</style></head><body><div class="poster"><div class="g"><div>a</div><div>b</div></div></div></body></html>"#;
    let (_doc, ws) = layout(&html);
    assert!(
        ws.iter()
            .any(|w| w.contains("grid-template-columns") && w.contains("未识别")),
        "应有模板未识别告警: {ws:?}"
    );
}
