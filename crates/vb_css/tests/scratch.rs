#[test]
fn scratch_root_vars() {
    let css = r#":root{
    --bg:#0e1014;      /* 炭黑主底 */
    --panel:#161922;   /* 面板 1 */
    --cyan:#00ffd1;
}"#;
    let sheet = vb_doc::import::parse_stylesheet(css);
    println!("root_vars = {:?}", sheet.root_vars);
    assert!(sheet.root_vars.iter().any(|(k, v)| k == "panel" && v == "#161922"));
}
