use vb_doc::import::import_project;
use vb_render::encode::encode_artboard_opts;
fn main() {
    let dir = std::path::Path::new(
        r"C:\Users\Velon\.openclaw-autoclaw\workspace\.cluster\730d5867\bench\dot-test",
    );
    let r = import_project(dir).expect("import fail");
    println!("warnings: {:?}", r.warnings);
    let ab = r.doc.artboards[0];
    let n = r.doc.node(ab).unwrap();
    println!("children: {}", n.children.len());
    for cid in &n.children {
        let c = r.doc.node(*cid).unwrap();
        println!(
            "child kind={:?} geom=({},{},{},{}) style={:?}",
            c.kind,
            c.geom.x,
            c.geom.y,
            c.geom.w,
            c.geom.h,
            c.style
                .iter()
                .map(|d| format!("{}={}", d.prop, d.value))
                .collect::<Vec<_>>()
        );
    }
    let list = encode_artboard_opts(&r.doc, ab, false).unwrap();
    println!("drawlist items: {}", list.items.len());
    for (i, it) in list.items.iter().enumerate() {
        println!(
            "#{} fill-is-radial={} rect={:?}",
            i,
            matches!(&it.fill, Some(vb_render::FillDef::RadialGradient { .. })),
            it.rect
        );
    }
}
