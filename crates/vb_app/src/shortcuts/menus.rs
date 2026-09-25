//! 菜单结构声明:九大菜单的条目表与「已登记未落地」计划表。
//!
//! 06-1 自 `shortcuts.rs` 按职责拆出(纯搬移,零行为变化):
//! 菜单里的键位文本经 `key_text_for` 自动查表(单一真相仍在绑定表)。

use super::key_text_for;

// ─────────────────────────── 菜单结构声明 ───────────────────────────

/// 菜单项。`id` 必须在 `IMPLEMENTED_IDS` 中;
/// 键位文本**自动查表**,禁止在 label 里手写。
#[derive(Debug, Clone, Copy)]
pub struct MenuItem {
    pub id: &'static str,
    pub label: &'static str,
}

/// 「已登记但尚未落地」的菜单项:id → 计划说明。
///
/// 阶段 5(副文档 06 §3)裁定:未实现项**保留在菜单里但置灰**,
/// 悬停即见「计划于 vX」——**绝不出现点了没反应的项**(`design/06 §七`)。
/// 这些 id **同样是已注册命令**(进 `IMPLEMENTED_IDS` / `commands.yaml`),
/// 因此 Agent 经 `run` 调用时也会拿到同一句提示,而不是静默失败。
pub const PLANNED: &[(&str, &str)] = &[
    // file.close 已随阶段 2 落地(关闭窗口 + 关闭确认),移出计划表
    // 05-2(09-B):object.clip_mask / object.release_clip_mask 已随剪切蒙版
    // 建模(Ctrl+7 / Ctrl+Alt+7)落地,移出计划表
    (
        "file.import_html",
        "计划于 v2:把外部 HTML 作为新画板导入(当前请用「打开项目」)",
    ),
    (
        "object.outline_stroke",
        "计划于 v2:轮廓化描边(当前请用视图菜单的轮廓模式查看线框)",
    ),
    (
        "text.create_outlines",
        "计划于 v2:文字转轮廓(需字形轮廓导出)",
    ),
    (
        "effect.distort",
        "计划于 v2:扭曲与变换效果(无 CSS 无损对应)",
    ),
    ("view.hide_edges", "计划于 v2:隐藏边缘(选中框细节开关)"),
    (
        "help.shortcuts",
        "计划于 v2:键位速查表(当前请用「命令搜索」)",
    ),
    ("help.check_update", "计划于 v2:检查更新"),
];

/// 该命令是否「已登记但尚未落地」;返回计划说明(菜单据此置灰 + 悬停提示)。
pub fn planned_reason(id: &str) -> Option<&'static str> {
    PLANNED.iter().find(|(i, _)| *i == id).map(|(_, m)| *m)
}

/// 路径查找器各运算的**输出语义**悬停提示(05-3 / X-1 铁律:输出语义
/// 写清楚,不许"看起来有点像")。描述的是**当前实现的真实行为**。
pub const PATHFINDER_TIPS: &[(&str, &str)] = &[
    (
        "path.union",
        "联集:所选对象合并为一件(并集轮廓);结果保留最先选中对象的样式与位置。",
    ),
    (
        "path.subtract",
        "减去顶层:保留下方对象,减去它与最上层对象的重叠部分(结果 = 下方 − 上方,样式保留下方);其余删除。",
    ),
    (
        "path.intersect",
        "交集:仅保留全部所选的重叠区域(不重叠则报错);样式保留最先选中对象。",
    ),
    (
        "path.xor",
        "差集:挖去全部重叠区域,保留各自未重叠部分(可产出多块,合为一个路径)。",
    ),
    (
        "path.merge",
        "合并:与联集同一几何内核 —— AI 在两操作数下的合并即并集;描边/填充保留原对象样式。",
    ),
    (
        "path.subtract_back",
        "减去后方对象:z 序最上的对象减去其下方全部对象,保留最上(与「减去顶层」保留对象相反)。",
    ),
    (
        "path.crop",
        "裁剪:只保留全部所选的重叠区域并把边界裁齐(与交集同一几何内核)。",
    ),
    (
        "path.divide",
        "分割:全部所选互相求交,重组为互不重叠的原子闭合区域(开放路径按闭合参与);每块填充取覆盖它的最上层对象。一次撤销恢复原状。",
    ),
    (
        "path.trim",
        "修边:每件减去其上方的对象,去掉描边,同填充色的碎片合并为一件;填充保留。一次撤销恢复原状。",
    ),
    (
        "path.outline",
        "轮廓:所有边线在与其他对象的交点处切开,输出为无填充的开放描边线(每段一件);描边继承来源对象,无描边时补 1px 黑。同对象自身交点不切。",
    ),
];

/// 路径查找器命令的输出语义提示(菜单悬停展示)。
pub fn pathfinder_tip(id: &str) -> Option<&'static str> {
    PATHFINDER_TIPS
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, m)| *m)
}

/// 菜单栏标题(AI 规范 9 项;V1 已确认 2026-09-20)。与 [`MENUS`] 一一对应。
pub const MENU_TITLES: [&str; 9] = [
    "文件", "编辑", "对象", "文字", "选择", "效果", "视图", "窗口", "帮助",
];

pub const MENU_FILE: &[MenuItem] = &[
    MenuItem {
        id: "file.new",
        label: "新建",
    },
    MenuItem {
        id: "file.open",
        label: "打开项目…",
    },
    MenuItem {
        id: "file.save",
        label: "保存",
    },
    // 阶段 7(07-E):一键体检(缺失资源/失效链接/冻结块/未使用资产/超长文件)
    MenuItem {
        id: "file.health_check",
        label: "项目健康检查…",
    },
    MenuItem {
        id: "file.close",
        label: "关闭窗口",
    },
    MenuItem {
        id: "file.home",
        label: "主页…",
    },
    MenuItem {
        id: "file.import_html",
        label: "导入 HTML…",
    },
    MenuItem {
        id: "file.export_dialog",
        label: "导出…",
    },
    MenuItem {
        id: "file.export_repeat",
        label: "上次导出(当前画板 PNG @2x)",
    },
    // 09-D(05-2):置入图像(assets/ 引用制;选中图像节点 = 替换 src)
    MenuItem {
        id: "file.place_image",
        label: "置入图像…",
    },
    // 09-M(05-4-A2):文档设置(项目名/输出模式/网格与参考线,项目级)
    MenuItem {
        id: "file.doc_settings",
        label: "文档设置…",
    },
    // 09-N(05-4-A2):外部冲突三方对比(07-R 未采用印记的处置入口)
    MenuItem {
        id: "file.resolve_conflict",
        label: "对比并合并…",
    },
    // X-7(05-4-A2):打印 = 当前画板 → Kiln 临时 PDF → 系统默认程序打开
    MenuItem {
        id: "file.print",
        label: "打印…",
    },
    MenuItem {
        id: "app.quit",
        label: "退出",
    },
];

pub const MENU_EDIT: &[MenuItem] = &[
    MenuItem {
        id: "edit.undo",
        label: "撤销",
    },
    MenuItem {
        id: "edit.redo",
        label: "重做",
    },
    MenuItem {
        id: "edit.select_all",
        label: "全选(当前画板)",
    },
    MenuItem {
        id: "edit.preferences",
        label: "首选项…",
    },
    MenuItem {
        id: "edit.keyboard_shortcuts",
        label: "键盘快捷键…",
    },
    // 04-6:design/06 §二「设置 → 工具 → 显示未支持工具」的最小入口
    // (完整首选项九分类属阶段 5;开关状态入 workspace.json)
    MenuItem {
        id: "edit.toggle_unsupported_tools",
        label: "设置 → 显示未支持工具",
    },
    // 阶段 7(07-A):自动保存间隔档位(最小入口;完整首选项九分类属阶段 5)
    MenuItem {
        id: "edit.autosave_interval",
        label: "设置 → 自动保存间隔",
    },
    // 05-2(X-5):铅笔保真度档位(design/06 §3.7「保真度参数 0–20px,设置项」)
    MenuItem {
        id: "edit.pencil_fidelity",
        label: "设置 → 铅笔保真度",
    },
    // 05-10(09-J):插件管理(安装/授权/启停/日志/重启)
    MenuItem {
        id: "edit.plugins",
        label: "插件管理…",
    },
];

pub const MENU_OBJECT: &[MenuItem] = &[
    MenuItem {
        id: "object.group",
        label: "编组",
    },
    MenuItem {
        id: "object.ungroup",
        label: "取消编组",
    },
    MenuItem {
        id: "object.transform_again",
        label: "再次变换",
    },
    MenuItem {
        id: "object.bring_forward",
        label: "前移一层",
    },
    MenuItem {
        id: "object.bring_to_front",
        label: "置于顶层",
    },
    MenuItem {
        id: "object.send_backward",
        label: "后移一层",
    },
    MenuItem {
        id: "object.send_to_back",
        label: "置于底层",
    },
    MenuItem {
        id: "object.delete",
        label: "删除",
    },
    // ── 路径查找器(10 运算;X-1 于 05-3 全通;悬停见各输出语义)──
    MenuItem {
        id: "path.union",
        label: "路径查找器 → 联集",
    },
    MenuItem {
        id: "path.subtract",
        label: "路径查找器 → 减去顶层",
    },
    MenuItem {
        id: "path.intersect",
        label: "路径查找器 → 交集",
    },
    MenuItem {
        id: "path.xor",
        label: "路径查找器 → 差集",
    },
    MenuItem {
        id: "path.merge",
        label: "路径查找器 → 合并",
    },
    MenuItem {
        id: "path.subtract_back",
        label: "路径查找器 → 减去后方对象",
    },
    MenuItem {
        id: "path.crop",
        label: "路径查找器 → 裁剪",
    },
    MenuItem {
        id: "path.divide",
        label: "路径查找器 → 分割",
    },
    MenuItem {
        id: "path.trim",
        label: "路径查找器 → 修边",
    },
    MenuItem {
        id: "path.outline",
        label: "路径查找器 → 轮廓",
    },
    MenuItem {
        id: "object.distribute_h",
        label: "水平等距分布",
    },
    MenuItem {
        id: "object.distribute_v",
        label: "垂直等距分布",
    },
    MenuItem {
        id: "object.lock",
        label: "锁定所选对象",
    },
    MenuItem {
        id: "object.unlock_all",
        label: "解锁全部对象",
    },
    MenuItem {
        id: "object.hide",
        label: "隐藏所选对象",
    },
    MenuItem {
        id: "object.show_all",
        label: "显示全部对象",
    },
    MenuItem {
        id: "object.clip_mask",
        label: "建立剪切蒙版",
    },
    MenuItem {
        id: "object.release_clip_mask",
        label: "释放剪切蒙版",
    },
    // 05-2(09-C):切片 → 从选区建立(design/06 §3.14)
    MenuItem {
        id: "object.slice_from_selection",
        label: "切片 → 从选区建立",
    },
    // 05-2(09-D):替换图像(与资产面板「替换」同一命令路径)
    MenuItem {
        id: "object.replace_image",
        label: "替换图像…",
    },
    MenuItem {
        id: "object.outline_stroke",
        label: "轮廓化描边",
    },
    // ── 05-8 符号 / 组件(09-H;实例 = 真实 DOM 副本 + 主件同步,零 JS)──
    MenuItem {
        id: "object.symbol_create",
        label: "组件 → 创建组件",
    },
    MenuItem {
        id: "object.symbol_detach",
        label: "组件 → 分离实例",
    },
    MenuItem {
        id: "object.symbol_reset_overrides",
        label: "组件 → 重置覆盖",
    },
    MenuItem {
        id: "object.symbol_swap_main",
        label: "组件 → 替换主件定义",
    },
    MenuItem {
        id: "object.symbol_select_instances",
        label: "组件 → 选择所有实例",
    },
    // ── 05-9 动效时间轴(09-I;关键帧 → @keyframes,预览与导出同源)──
    MenuItem {
        id: "anim.play_toggle",
        label: "动画 → 播放 / 暂停",
    },
    MenuItem {
        id: "anim.stop",
        label: "动画 → 停止并回零",
    },
    MenuItem {
        id: "anim.loop_toggle",
        label: "动画 → 循环开 / 关",
    },
    MenuItem {
        id: "anim.keyframe_add",
        label: "动画 → 在播放头处加关键帧",
    },
    MenuItem {
        id: "anim.keyframe_delete",
        label: "动画 → 删除选中的关键帧",
    },
    MenuItem {
        id: "anim.clear",
        label: "动画 → 清除对象动画",
    },
];

// ── 阶段 5 新增四个菜单(文字 / 选择 / 效果 / 窗口)──

/// 文字菜单(`design/03 §二`)。
pub const MENU_TYPE: &[MenuItem] = &[
    MenuItem {
        id: "view.toggle_char_panel",
        label: "字符",
    },
    MenuItem {
        id: "view.toggle_para_panel",
        label: "段落",
    },
    MenuItem {
        id: "tool.text_cycle_mode",
        label: "点文字 / 区域文字",
    },
    MenuItem {
        id: "text.upper_case",
        label: "更改大小写 → 大写",
    },
    MenuItem {
        id: "text.lower_case",
        label: "更改大小写 → 小写",
    },
    MenuItem {
        id: "text.create_outlines",
        label: "创建轮廓",
    },
    MenuItem {
        id: "text.find_font",
        label: "查找字体…",
    },
];

/// 选择菜单(`design/03 §二`)。
pub const MENU_SELECT: &[MenuItem] = &[
    MenuItem {
        id: "edit.select_all",
        label: "全部(当前画板)",
    },
    MenuItem {
        id: "canvas.cancel",
        label: "取消选择",
    },
    MenuItem {
        id: "select.inverse",
        label: "反向",
    },
    MenuItem {
        id: "select.next_object",
        label: "上方的下一个对象",
    },
    MenuItem {
        id: "select.prev_object",
        label: "下方的下一个对象",
    },
    MenuItem {
        id: "select.same_fill",
        label: "相同 → 填充色",
    },
    MenuItem {
        id: "select.same_stroke",
        label: "相同 → 描边色",
    },
    MenuItem {
        id: "select.same_stroke_width",
        label: "相同 → 描边粗细",
    },
    MenuItem {
        id: "select.all_text",
        label: "对象 → 全部文本对象",
    },
    MenuItem {
        id: "select.all_locked",
        label: "对象 → 所有锁定对象",
    },
    MenuItem {
        id: "select.all_hidden",
        label: "对象 → 所有隐藏对象",
    },
];

/// 效果菜单(`design/06 §4.6` 映射表;每条落 CSS 见外观面板)。
pub const MENU_EFFECT: &[MenuItem] = &[
    MenuItem {
        id: "effect.repeat_last",
        label: "应用上一个效果",
    },
    MenuItem {
        id: "effect.drop_shadow",
        label: "风格化 → 投影",
    },
    MenuItem {
        id: "effect.inner_shadow",
        label: "风格化 → 内阴影",
    },
    MenuItem {
        id: "effect.outer_glow",
        label: "风格化 → 外发光",
    },
    MenuItem {
        id: "effect.inner_glow",
        label: "风格化 → 内发光",
    },
    MenuItem {
        id: "effect.round_corners",
        label: "风格化 → 圆角",
    },
    MenuItem {
        id: "effect.gaussian_blur",
        label: "模糊 → 高斯模糊",
    },
    MenuItem {
        id: "effect.feather",
        label: "羽化…",
    },
    MenuItem {
        id: "effect.distort",
        label: "扭曲和变换…",
    },
];

pub const MENU_VIEW: &[MenuItem] = &[
    MenuItem {
        id: "view.zoom_in",
        label: "放大",
    },
    MenuItem {
        id: "view.zoom_out",
        label: "缩小",
    },
    MenuItem {
        id: "view.fit",
        label: "适合窗口",
    },
    MenuItem {
        id: "view.actual_size",
        label: "实际大小",
    },
    MenuItem {
        id: "view.outline",
        label: "轮廓模式(线框)",
    },
    // 05-2(09-E):像素预览(design/06 §六「按 1:1 设备像素光栅显示」)
    MenuItem {
        id: "view.pixel_preview",
        label: "像素预览",
    },
    MenuItem {
        id: "view.toggle_grid",
        label: "显示网格",
    },
    MenuItem {
        id: "view.toggle_smart_guides",
        label: "智能参考线",
    },
    MenuItem {
        id: "view.toggle_theme",
        label: "浅色主题",
    },
    MenuItem {
        id: "view.next_artboard",
        label: "下一画板",
    },
    MenuItem {
        id: "view.prev_artboard",
        label: "上一画板",
    },
    MenuItem {
        id: "view.zoom_to_selection",
        label: "缩放到选区",
    },
    MenuItem {
        id: "view.toggle_rulers",
        label: "显示标尺",
    },
    MenuItem {
        id: "view.toggle_guides",
        label: "显示参考线",
    },
    MenuItem {
        id: "view.lock_guides",
        label: "锁定参考线",
    },
    MenuItem {
        id: "view.guides_from_selection",
        label: "从选区生成参考线",
    },
    MenuItem {
        id: "view.hide_edges",
        label: "隐藏边缘",
    },
    MenuItem {
        id: "view.browser_proof",
        label: "浏览器校对…",
    },
    // 04-4:调试数据默认隐藏(开发者统计),提示条可关
    MenuItem {
        id: "view.developer_stats",
        label: "开发者统计",
    },
    MenuItem {
        id: "view.toggle_hints",
        label: "提示",
    },
    // H-1:动效总开关(面板/对话框淡入、悬停过渡;首选项「常规」同款)
    MenuItem {
        id: "view.toggle_motion",
        label: "界面动效",
    },
    // 04-3:UI 缩放档位(叠加在系统 DPI 之上;完整首选项属阶段 5)
    MenuItem {
        id: "view.ui_scale_up",
        label: "界面缩放 · 增大",
    },
    MenuItem {
        id: "view.ui_scale_down",
        label: "界面缩放 · 减小",
    },
    MenuItem {
        id: "view.ui_scale_reset",
        label: "界面缩放 · 复位 100%",
    },
];

/// 窗口菜单(`design/03 §二`):工作区 + 面板坞 Tab + 各面板显隐。
///
/// 面板项与副文档 02 的 `F 键` **同源**(同一批 `view.toggle_*` 命令),
/// 因此键位文本自动出现在右侧(`menu_label` 查注册表,禁止手写)。
pub const MENU_WINDOW: &[MenuItem] = &[
    MenuItem {
        id: "window.workspace_basic",
        label: "工作区 → 基本功能",
    },
    MenuItem {
        id: "window.workspace_type",
        label: "工作区 → 排版",
    },
    MenuItem {
        id: "window.workspace_export",
        label: "工作区 → 导出",
    },
    MenuItem {
        id: "window.new_workspace",
        label: "新建工作区…",
    },
    MenuItem {
        id: "view.dock_toolbar_top",
        label: "工具箱 → 停靠到顶部",
    },
    MenuItem {
        id: "view.dock_toolbar_left",
        label: "工具箱 → 停靠到左侧",
    },
    MenuItem {
        id: "view.dock_toolbar_right",
        label: "工具箱 → 停靠到右侧",
    },
    MenuItem {
        id: "view.dock_toolbar_bottom",
        label: "工具箱 → 停靠到底部",
    },
    MenuItem {
        id: "view.toolbar_columns_1",
        label: "工具箱 → 单列",
    },
    MenuItem {
        id: "view.toolbar_columns_2",
        label: "工具箱 → 双列",
    },
    MenuItem {
        id: "view.toggle_transform_panel",
        label: "变换面板",
    },
    MenuItem {
        id: "view.toggle_align_panel",
        label: "对齐面板",
    },
    MenuItem {
        id: "align.to_selection",
        label: "对齐到 → 选区",
    },
    MenuItem {
        id: "align.to_key_object",
        label: "对齐到 → 关键对象",
    },
    MenuItem {
        id: "align.to_artboard",
        label: "对齐到 → 画板",
    },
    MenuItem {
        id: "window.tab_properties",
        label: "属性",
    },
    MenuItem {
        id: "window.tab_layers",
        label: "图层",
    },
    MenuItem {
        id: "window.tab_artboards",
        label: "画板",
    },
    MenuItem {
        id: "window.tab_tokens",
        label: "令牌",
    },
    // 阶段 7(07-D):撤销历史面板(次级坞「变换」组)
    MenuItem {
        id: "view.toggle_history_panel",
        label: "历史面板",
    },
    // 阶段 7b(07-K):资产面板(次级坞「资产」组)
    MenuItem {
        id: "view.toggle_assets_panel",
        label: "资产面板",
    },
    // 05-9(09-I):时间轴面板(次级坞「时间轴」组)
    MenuItem {
        id: "view.toggle_timeline_panel",
        label: "时间轴面板",
    },
    MenuItem {
        id: "view.toggle_char_panel",
        label: "字符面板",
    },
    MenuItem {
        id: "view.toggle_para_panel",
        label: "段落面板",
    },
    MenuItem {
        id: "view.toggle_appearance_panel",
        label: "外观面板",
    },
    MenuItem {
        id: "view.toggle_stroke_panel",
        label: "描边面板",
    },
    MenuItem {
        id: "view.toggle_gradient_panel",
        label: "渐变面板",
    },
    MenuItem {
        id: "view.toggle_opacity_panel",
        label: "透明度面板",
    },
    MenuItem {
        id: "view.toggle_color_panel",
        label: "颜色面板",
    },
    MenuItem {
        id: "view.toggle_layers_panel",
        label: "图层面板显隐",
    },
    MenuItem {
        id: "view.toggle_all_panels",
        label: "隐藏所有面板",
    },
    // 05-10(09-J):插件面板(次级坞「插件」组)
    MenuItem {
        id: "view.toggle_plugins_panel",
        label: "插件面板",
    },
];

pub const MENU_HELP: &[MenuItem] = &[
    MenuItem {
        id: "app.command_palette",
        label: "命令搜索…",
    },
    MenuItem {
        id: "help.shortcuts",
        label: "键位速查表…",
    },
    MenuItem {
        id: "help.check_update",
        label: "检查更新…",
    },
    MenuItem {
        id: "help.capabilities",
        label: "能力台账…",
    },
    MenuItem {
        id: "app.about",
        label: "关于",
    },
];

/// 全部菜单声明(门禁自检用)。**顺序与 [`MENU_TITLES`] 一一对应**。
pub const MENUS: &[&[MenuItem]] = &[
    MENU_FILE,
    MENU_EDIT,
    MENU_OBJECT,
    MENU_TYPE,
    MENU_SELECT,
    MENU_EFFECT,
    MENU_VIEW,
    MENU_WINDOW,
    MENU_HELP,
];

/// 菜单项的完整显示文本:`标签` + 制表符 + `键位`(无绑定时只有标签)。
pub fn menu_label(item: &MenuItem) -> String {
    match key_text_for(item.id) {
        Some(k) => format!("{}\t{k}", item.label),
        None => item.label.to_string(),
    }
}
