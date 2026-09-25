//! 阶段 6「工具箱分组」验收门(副文档 07-1-7 / 07-3-1)。
//!
//! 两条判据:
//! 1. **选择(`V`)与直接选择(`A`)是两个独立工具** —— 各自在工具箱里各占一格、
//!    图标不同、键位不同,且同属"选择族"(便于同族展开切换);
//! 2. **同族展开的数据源自洽** —— 每个工具只属于一个族,族内至少含它自己,
//!    单成员族不出菜单(渲染侧据此判断)。

use vb_app::app::toolbar::{family_of, TOOLBOX};
use vb_app::app::Tool;

#[test]
fn select_and_direct_select_are_distinct_tools() {
    let sel = TOOLBOX
        .iter()
        .find(|(t, ..)| *t == Tool::Select)
        .expect("工具箱必须有「选择」");
    let ds = TOOLBOX
        .iter()
        .find(|(t, ..)| *t == Tool::DirectSelect)
        .expect("工具箱必须有「直接选择」");

    assert_ne!(sel.1, ds.1, "两者必须是不同图标(实心 / 空心指针)");
    assert_ne!(sel.3, ds.3, "两者必须是不同键位(V / A)");
    assert_eq!(sel.3, "V");
    assert_eq!(ds.3, "A");

    // 相邻(design/03 §一:同族相邻)
    let i = TOOLBOX
        .iter()
        .position(|(t, ..)| *t == Tool::Select)
        .unwrap();
    let j = TOOLBOX
        .iter()
        .position(|(t, ..)| *t == Tool::DirectSelect)
        .unwrap();
    assert_eq!(j, i + 1, "选择与直接选择应紧邻在同一族里");
}

#[test]
fn every_tool_appears_exactly_once_and_has_key_and_icon() {
    // 05-2:度量工具无标准键位(design/02/06 均未定义;工具箱/菜单触发),
    // 是**唯一**的显式免键位白名单 —— 新工具要么绑键,要么进这份名单并说明。
    const KEYLESS: &[Tool] = &[Tool::Measure];
    let mut seen: Vec<String> = Vec::new();
    for (tool, _icon, label, key) in TOOLBOX {
        let name = format!("{tool:?}");
        assert!(!seen.contains(&name), "工具箱重复出现:{name}");
        seen.push(name);
        assert!(!label.is_empty(), "{tool:?} 无名称");
        if !KEYLESS.contains(tool) {
            assert!(!key.is_empty(), "{tool:?} 无键位(工具箱必须能提示快捷键)");
        }
    }
    assert_eq!(seen.len(), TOOLBOX.len());
    // 白名单里的工具必须真实存在于工具箱(防名单腐烂)
    for k in KEYLESS {
        assert!(
            TOOLBOX.iter().any(|(t, ..)| t == k),
            "免键位白名单含工具箱外的工具:{k:?}"
        );
    }
}

#[test]
fn families_contain_their_own_member_and_partition_the_toolbox() {
    for (tool, ..) in TOOLBOX {
        let fam = family_of(*tool);
        assert!(
            fam.iter().any(|(t, ..)| t == tool),
            "{tool:?} 的同族里没有它自己"
        );
        assert!(!fam.is_empty(), "{tool:?} 的族为空");
    }

    // 族内成员必须两两不同,且不含箱外工具
    for (tool, ..) in TOOLBOX {
        let fam = family_of(*tool);
        let mut set: Vec<String> = Vec::new();
        for (t, ..) in fam {
            let name = format!("{t:?}");
            assert!(!set.contains(&name), "{tool:?} 的族里有重复成员");
            set.push(name);
            assert!(
                TOOLBOX.iter().any(|(x, ..)| x == t),
                "{tool:?} 的族含工具箱外的工具"
            );
        }
    }
}

#[test]
fn singleton_families_have_no_flyout() {
    // 文字 / 画板:各自独立(族长 1 → 不出同族菜单)
    assert_eq!(family_of(Tool::Text).len(), 1);
    assert_eq!(family_of(Tool::Artboard).len(), 1);

    // 有实际同族关系的族:长度 > 1
    assert!(family_of(Tool::Select).len() >= 2);
    assert!(family_of(Tool::Rect).len() >= 2);
    assert!(family_of(Tool::Zoom).len() >= 2);
    assert!(family_of(Tool::Pen).len() >= 2);
}
