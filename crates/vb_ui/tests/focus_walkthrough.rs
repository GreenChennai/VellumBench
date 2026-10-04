//! G-UI-D:焦点走查 · 静态断言(审查 2026-10-04 §8.12)。
//!
//! **egui 0.35 的事实面**(本门禁的依据,`egui::Sense`/`Memory` 源码口径):
//! 1. `Sense::click()` / `drag()` / `click_and_drag()` 都含 `FOCUSABLE`
//!    位 —— **一切可交互控件(egui 原生 + 自绘)自动进 Tab 序**,
//!    Tab/Shift+Tab 导航由 `Memory::FocusDirection` 原生承担;
//! 2. 有焦点的控件上,`Response::clicked()` 对 Space/Enter 同样为真
//!    —— 自绘控件的键盘激活免费;
//! 3. **焦点环不自绘就没有**:egui 只给原生控件描样式,自绘控件必须
//!    在 `resp.has_focus()` 时自己画 accent 环。
//!
//! 因此门禁断言:① 自绘可交互控件的焦点环调用点齐备;
//! ② 读屏语义(`WidgetInfo`)在自定义三态控件上齐备。
//! 逐面板的可达清单见 `docs/design/ui-focus-a11y.md`(人工走查表)。

const COMPONENTS: &str = include_str!("../src/components.rs");

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// 自绘焦点环:环调用点 ≥ 自绘可交互控件数
/// (ToolButton / NumField 标签 / icon_button / checkbox / radio / switch /
/// TextField / PanelTabs Tab 页 = 8 处)。
#[test]
fn custom_controls_paint_focus_rings() {
    let rings = count(COMPONENTS, "paint_focus_ring(") - 1; // 减定义本身
    assert!(
        rings >= 8,
        "G-UI-D:自绘控件的键盘焦点环调用点不足(找到 {rings},需 ≥8)—— \
         新增自绘控件必须在 resp.has_focus() 时画焦点环"
    );
    assert!(
        COMPONENTS.contains("fn paint_focus_ring"),
        "焦点环助手必须存在于 components.rs(单一实现)"
    );
}

/// S5(§8.10 诚实清单 ①):PanelTabs 的 Tab 页是 allocate 自绘控件,
/// 逐控件断言其环真接线(`resp.has_focus()` → `paint_focus_ring`),
/// 不许只靠总数蒙混。vb_app 侧的图层面板行/状态栏文本项/启动器卡片
/// 由 `docs/design/ui-focus-a11y.md` 走查表人工钉住(跨 crate 静态扫描
/// 归属 CI 分档项)。
#[test]
fn panel_tabs_tabs_paint_focus_ring() {
    let body = COMPONENTS
        .split("pub fn ui_ex")
        .nth(1)
        .and_then(|rest| rest.split("pub fn ui(").next())
        .unwrap_or("");
    assert!(
        body.contains("resp.has_focus()") && body.contains("paint_focus_ring("),
        "G-UI-D:PanelTabs Tab 页缺键盘焦点环(S5 清单 ① 回归)"
    );
}

/// 读屏语义:自定义三态控件(checkbox/radio/switch)必须登记 `WidgetInfo`
/// (UI-03 的起步项;egui 原生控件由 egui 自行登记)。
#[test]
fn custom_controls_register_widget_info() {
    let infos = count(COMPONENTS, "WidgetInfo::selected(");
    assert!(
        infos >= 3,
        "G-UI-D:checkbox/radio/switch 的 WidgetInfo 登记缺失(找到 {infos})"
    );
}

/// Tab 序的机制面:自绘控件一律经 `Sense::click` / `Sense::drag` /
/// `Sense::click_and_drag` 分配 —— egui 0.35 三者都含 FOCUSABLE 位,
/// 因此"自定义控件不在 Tab 序"这类回归只可能来自改用 `Sense::hover()`。
/// 这里钉住:可交互组件函数体内不得出现纯 hover 感知。
#[test]
fn interactive_controls_never_use_hover_only_sense() {
    // 逐函数扫描:含 "clicked()" 判定的函数不得allocate Sense::hover()
    for func in COMPONENTS
        .split("pub fn ")
        .chain(COMPONENTS.split("    fn "))
    {
        let body = func.split('{').take(2).last().unwrap_or("");
        if body.contains("resp.clicked()") && body.contains("Sense::hover()") {
            panic!(
                "G-UI-D:可点击控件使用了纯 hover 感知(不可聚焦):{}",
                func.lines().next().unwrap_or("?")
            );
        }
    }
}
