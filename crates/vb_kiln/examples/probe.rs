use vb_doc::import::import_project;
fn main() {
    let dir = std::path::Path::new(r"C:\Users\Velon\.openclaw-autoclaw\workspace\.cluster\730d5867\bench\sample");
    let r = import_project(dir).expect("import fail");
    println!("artboards: {}", r.doc.artboards.len());
    for w in &r.warnings { println!("warn: {w}"); }
    let ab = r.doc.artboards[0];
    let n = r.doc.node(ab).unwrap();
    println!("artboard geom: {}x{}", n.geom.w, n.geom.h);
    println!("style: {:?}", n.style.iter().map(|d| format!("{}={}", d.prop, d.value)).collect::<Vec<_>>());
    println!("children: {}", n.children.len());
}
