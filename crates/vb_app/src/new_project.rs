//! 新建项目(阶段 2 / 副文档 02-4):对话框规格、画板预设、项目生成与模板复制。
//!
//! 纪律(**02-4-5**):生成走 `Document::new_default()` → `write_project`
//! 的**合法序列化路径**(稳定 id、`vb-artboard` 标记、styles/main.css),
//! 绝不另写一份 HTML 拼装 —— 保证新项目能被 `vellum-cli tree --json`
//! 读取、能 `kiln-cli export` 导出。
//!
//! 「从模板新建」(**02-4-4**)= 整目录复制模板(模板本身就是合法项目)。

use std::path::{Path, PathBuf};

use vb_doc::model::{Document, OutputMode};

/// 画板尺寸预设(02-4-1:Web 1920/1440/1080/750/375、A4、自定义)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtboardPreset {
    Web1920,
    Web1440,
    Web1080,
    Mobile750,
    Mobile375,
    A4,
    /// 自定义:宽高取对话框输入。
    Custom,
}

impl ArtboardPreset {
    /// 下拉框顺序(与 `ALL` 一一对应)。
    pub const ALL: [ArtboardPreset; 7] = [
        ArtboardPreset::Web1920,
        ArtboardPreset::Web1440,
        ArtboardPreset::Web1080,
        ArtboardPreset::Mobile750,
        ArtboardPreset::Mobile375,
        ArtboardPreset::A4,
        ArtboardPreset::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ArtboardPreset::Web1920 => "Web · 1920×1080",
            ArtboardPreset::Web1440 => "Web · 1440×900",
            ArtboardPreset::Web1080 => "Web · 1080×1920",
            ArtboardPreset::Mobile750 => "移动 · 750×1334",
            ArtboardPreset::Mobile375 => "移动 · 375×667",
            ArtboardPreset::A4 => "A4 · 794×1123(96dpi)",
            ArtboardPreset::Custom => "自定义",
        }
    }

    /// 基础尺寸(宽, 高);自定义返回 None(由对话框输入决定)。
    pub fn base_size(self) -> Option<(f64, f64)> {
        match self {
            ArtboardPreset::Web1920 => Some((1920.0, 1080.0)),
            ArtboardPreset::Web1440 => Some((1440.0, 900.0)),
            ArtboardPreset::Web1080 => Some((1080.0, 1920.0)),
            ArtboardPreset::Mobile750 => Some((750.0, 1334.0)),
            ArtboardPreset::Mobile375 => Some((375.0, 667.0)),
            ArtboardPreset::A4 => Some((794.0, 1123.0)),
            ArtboardPreset::Custom => None,
        }
    }

    /// 套用取向(02-4-1):纵向 = 宽高对调(自定义预设原样返回)。
    pub fn sized(self, portrait: bool, custom_w: f64, custom_h: f64) -> (f64, f64) {
        let (w, h) = match self.base_size() {
            Some(wh) => wh,
            None => (custom_w.max(16.0), custom_h.max(16.0)),
        };
        if portrait {
            (h.min(w), w.max(h))
        } else {
            (w.max(h), h.min(w))
        }
    }
}

/// 输出模式(02-4-1:外链 CSS / 单文件)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputChoice {
    ExternalCss,
    SingleFile,
}

impl OutputChoice {
    pub const ALL: [OutputChoice; 2] = [OutputChoice::ExternalCss, OutputChoice::SingleFile];

    pub fn label(self) -> &'static str {
        match self {
            OutputChoice::ExternalCss => "外链 CSS(styles/main.css)",
            OutputChoice::SingleFile => "单文件(CSS 内联)",
        }
    }

    fn to_mode(self) -> OutputMode {
        match self {
            OutputChoice::ExternalCss => OutputMode::ExternalCss,
            OutputChoice::SingleFile => OutputMode::SingleFile,
        }
    }
}

/// 「新建项目」对话框的完整输入(02-4-1)。
#[derive(Debug, Clone, PartialEq)]
pub struct NewProjectSpec {
    /// 项目名(= 目录名 = 文档标题)。
    pub name: String,
    /// 位置(父目录)。
    pub location: PathBuf,
    pub preset: ArtboardPreset,
    /// 取向:true = 纵向。
    pub portrait: bool,
    /// 画板数(1..=12,纵向排布)。
    pub artboards: u32,
    pub output: OutputChoice,
    /// 自定义预设的宽/高(其它预设忽略)。
    pub custom_w: f64,
    pub custom_h: f64,
}

impl Default for NewProjectSpec {
    fn default() -> Self {
        NewProjectSpec {
            name: "未命名项目".into(),
            location: default_location(),
            preset: ArtboardPreset::Web1440,
            portrait: false,
            artboards: 1,
            output: OutputChoice::ExternalCss,
            custom_w: 1080.0,
            custom_h: 1920.0,
        }
    }
}

/// 新建对话框的默认位置:用户目录(不存在则退当前目录)。
fn default_location() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_dir());
    home.unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

impl NewProjectSpec {
    /// 目标目录 = 位置 / 项目名(目录名做非法字符清洗)。
    pub fn target_dir(&self) -> PathBuf {
        self.location.join(sanitize_dir_name(&self.name))
    }

    /// 校验:返回第一条错误(通过则 None)。
    pub fn validate(&self) -> Option<String> {
        if self.name.trim().is_empty() {
            return Some("项目名不能为空".into());
        }
        if !self.location.is_dir() {
            return Some(format!("位置不存在:{}", self.location.display()));
        }
        if self.target_dir().exists() {
            return Some(format!(
                "目录已存在:{}(换个项目名或位置)",
                self.target_dir().display()
            ));
        }
        if matches!(self.preset, ArtboardPreset::Custom)
            && (self.custom_w < 16.0 || self.custom_h < 16.0)
        {
            return Some("自定义画板尺寸至少 16×16".into());
        }
        None
    }
}

/// 目录名清洗:去掉文件系统/HTML 双非法字符与首尾空白。
pub fn sanitize_dir_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c => c,
        })
        .collect();
    cleaned.trim().to_string()
}

/// 生成最小合法项目(**02-4-2**):`index.html` + `styles/main.css` + `assets/`,
/// 稳定 id 与 `vb-artboard` 标记由 `write_project` 的序列化路径给出。
/// 成功返回项目目录。
pub fn create_project(spec: &NewProjectSpec) -> Result<PathBuf, String> {
    if let Some(err) = spec.validate() {
        return Err(err);
    }
    let dir = spec.target_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建项目目录失败:{e}"))?;
    std::fs::create_dir_all(dir.join("assets")).map_err(|e| format!("创建 assets 失败:{e}"))?;

    // 合法序列化路径:new_default(单画板 1440×900)→ 改标题/输出/画板几何 → write_project
    let mut doc = Document::new_default();
    doc.meta.title = spec.name.trim().to_string();
    doc.meta.output = spec.output.to_mode();
    let (w, h) = spec
        .preset
        .sized(spec.portrait, spec.custom_w, spec.custom_h);
    if let Some(&first) = doc.artboards.first() {
        if let Some(n) = doc.nodes.get_mut(first) {
            n.geom.w = w;
            n.geom.h = h;
            n.name = "画板 1".into();
        }
    }
    let gap = 80.0;
    for i in 1..spec.artboards.clamp(1, 12) {
        let y = h * i as f64 + gap * i as f64;
        doc.new_artboard(&format!("画板 {}", i + 1), w, h);
        // new_artboard 落在 (0,0),平移到纵向排布位(直接改几何:尚无 undo 栈)
        if let Some(id) = doc.artboards.last() {
            if let Some(n) = doc.nodes.get_mut(*id) {
                n.geom.y = y;
            }
        }
    }
    vb_doc::export::write_project(&doc, &dir).map_err(|e| format!("写项目文件失败:{e}"))?;
    Ok(dir)
}

// ─────────────────────────── 从模板新建(02-4-4) ───────────────────────────

/// 模板根目录解析(按序尝试,首个存在的胜出):
/// `VB_TEMPLATES` 环境变量 → exe 同级/上级 `examples` → 开发期源码树 `examples`。
pub fn templates_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("VB_TEMPLATES") {
        if !p.trim().is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("examples"));
            candidates.push(dir.parent().map(|p| p.join("examples")).unwrap_or_default());
        }
    }
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        // crates/vb_app → 仓库根/examples
        candidates.push(
            PathBuf::from(&manifest)
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.join("examples"))
                .unwrap_or_default(),
        );
    }
    // 目录必须真的装着模板(至少一个含 index.html 的子目录)才认:
    // 排除 target/debug/examples 这类"存在但为空"的 cargo 约定目录
    candidates
        .into_iter()
        .find(|p| looks_like_templates_root(p))
}

/// 目录里至少有一个含 index.html 的子目录 → 认定是模板根。
fn looks_like_templates_root(p: &Path) -> bool {
    if !p.is_dir() {
        return false;
    }
    std::fs::read_dir(p)
        .map(|rd| {
            rd.flatten()
                .any(|e| e.path().is_dir() && e.path().join("index.html").is_file())
        })
        .unwrap_or(false)
}

/// 列出可用模板(目录名,排序)。
pub fn list_templates() -> Vec<String> {
    let Some(root) = templates_root() else {
        return Vec::new();
    };
    let mut out: Vec<String> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir() && e.path().join("index.html").is_file())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    out.sort();
    out
}

/// 复制模板 → `location/name`,返回新项目目录。
pub fn create_from_template(
    template: &str,
    location: &Path,
    name: &str,
) -> Result<PathBuf, String> {
    if name.trim().is_empty() {
        return Err("项目名不能为空".into());
    }
    if !location.is_dir() {
        return Err(format!("位置不存在:{}", location.display()));
    }
    let Some(root) = templates_root() else {
        return Err("找不到模板目录(examples);可用 VB_TEMPLATES 指定".into());
    };
    let src = root.join(sanitize_dir_name(template));
    if !src.join("index.html").is_file() {
        return Err(format!("模板不存在:{}", src.display()));
    }
    let dst = location.join(sanitize_dir_name(name));
    if dst.exists() {
        return Err(format!("目录已存在:{}(换个项目名或位置)", dst.display()));
    }
    copy_dir_recursive(&src, &dst)?;
    Ok(dst)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("创建目录 {} 失败:{e}", dst.display()))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("读 {} 失败:{e}", src.display()))? {
        let entry = entry.map_err(|e| format!("读目录项失败:{e}"))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| format!("复制 {} 失败:{e}", from.display()))?;
        }
    }
    Ok(())
}

// ─────────────────────────── 新建对话框(02-4-1;主页与项目窗口共用) ───────────────────────────

/// 对话框的结论(渲染函数的返回值;发送方据此发外壳请求)。
pub enum DialogAction {
    Create(NewProjectSpec),
    CreateTemplate {
        template: String,
        location: PathBuf,
        name: String,
    },
    Cancel,
}

/// 「新建项目 / 从模板新建」对话框状态(主页与项目窗口各持一份会话态)。
pub struct NewProjectDialog {
    pub spec: NewProjectSpec,
    /// 实时校验错误(非空时禁用确认按钮,02 篇纪律:不出现点了没反应)。
    pub error: Option<String>,
    /// 可用模板目录名(`list_templates()`)。
    pub templates: Vec<String>,
    pub template_sel: usize,
    /// true = 从模板新建模式。
    pub template_mode: bool,
    /// 模板目录不可得(给可读提示,不静默)。
    pub templates_missing: bool,
}

impl Default for NewProjectDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl NewProjectDialog {
    pub fn new() -> Self {
        NewProjectDialog {
            spec: NewProjectSpec::default(),
            error: None,
            templates: list_templates(),
            template_sel: 0,
            template_mode: false,
            templates_missing: false,
        }
    }

    pub fn new_template() -> Self {
        let mut d = Self::new();
        d.template_mode = true;
        d.templates_missing = d.templates.is_empty();
        d
    }

    fn validate_live(&mut self) {
        self.error = if self.template_mode {
            if self.templates.is_empty() {
                Some("找不到模板目录(examples);可用 VB_TEMPLATES 指定".into())
            } else if self.spec.name.trim().is_empty() {
                Some("项目名不能为空".into())
            } else if !self.spec.location.is_dir() {
                Some(format!("位置不存在:{}", self.spec.location.display()))
            } else if self.spec.target_dir().exists() {
                Some(format!(
                    "目录已存在:{}(换个项目名或位置)",
                    self.spec.target_dir().display()
                ))
            } else {
                None
            }
        } else {
            self.spec.validate()
        };
    }
}

/// 渲染对话框;返回 `Some(DialogAction)` = 对话框结束。
pub fn dialog_ui(ui: &mut egui::Ui, dlg: &mut NewProjectDialog) -> Option<DialogAction> {
    let mut action: Option<DialogAction> = None;
    dlg.validate_live();
    egui::Grid::new("vb-newproj-grid")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            if dlg.template_mode {
                ui.label("模板");
                if dlg.templates.is_empty() {
                    ui.colored_label(
                        // 错误提示走主题 danger 令牌(值即设计令牌 #F24822,两主题可读)
                        vb_ui::theme::tokens(ui.ctx()).danger,
                        "找不到模板目录(examples);可用 VB_TEMPLATES 指定",
                    );
                } else {
                    egui::ComboBox::from_id_salt("vb-newproj-template")
                        .selected_text(
                            dlg.templates
                                .get(dlg.template_sel)
                                .map(String::as_str)
                                .unwrap_or("?"),
                        )
                        .show_ui(ui, |ui| {
                            for (i, tpl) in dlg.templates.iter().enumerate() {
                                ui.selectable_value(&mut dlg.template_sel, i, tpl);
                            }
                        });
                }
                ui.end_row();
            } else {
                ui.label("画板预设");
                egui::ComboBox::from_id_salt("vb-newproj-preset")
                    .selected_text(dlg.spec.preset.label())
                    .show_ui(ui, |ui| {
                        for p in ArtboardPreset::ALL {
                            ui.selectable_value(&mut dlg.spec.preset, p, p.label());
                        }
                    });
                ui.end_row();

                ui.label("取向");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut dlg.spec.portrait, false, "横向");
                    ui.selectable_value(&mut dlg.spec.portrait, true, "纵向");
                    if matches!(dlg.spec.preset, ArtboardPreset::Custom) {
                        ui.separator();
                        ui.add(
                            egui::DragValue::new(&mut dlg.spec.custom_w)
                                .range(16.0..=100000.0)
                                .prefix("宽 "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut dlg.spec.custom_h)
                                .range(16.0..=100000.0)
                                .prefix("高 "),
                        );
                    }
                });
                ui.end_row();

                ui.label("画板数");
                ui.add(egui::DragValue::new(&mut dlg.spec.artboards).range(1..=12));
                ui.end_row();

                ui.label("输出模式");
                egui::ComboBox::from_id_salt("vb-newproj-output")
                    .selected_text(dlg.spec.output.label())
                    .show_ui(ui, |ui| {
                        for o in OutputChoice::ALL {
                            ui.selectable_value(&mut dlg.spec.output, o, o.label());
                        }
                    });
                ui.end_row();
            }

            ui.label("项目名");
            ui.text_edit_singleline(&mut dlg.spec.name);
            ui.end_row();

            ui.label("位置");
            ui.horizontal(|ui| {
                let mut location = dlg.spec.location.to_string_lossy().to_string();
                let resp = ui.add_sized(
                    [340.0, vb_ui::theme::row_height(ui.ctx())],
                    egui::TextEdit::singleline(&mut location),
                );
                if resp.changed() {
                    dlg.spec.location = PathBuf::from(location);
                }
                if ui.button("浏览…").clicked() {
                    if let Some(dir) = rfd::FileDialog::new()
                        .set_title("选择项目位置")
                        .pick_folder()
                    {
                        dlg.spec.location = dir;
                    }
                }
            });
            ui.end_row();
        });

    if let Some(err) = &dlg.error {
        ui.add_space(4.0);
        // 错误提示走主题 danger 令牌(值即设计令牌 #F24822,两主题可读)
        ui.colored_label(vb_ui::theme::tokens(ui.ctx()).danger, err);
    }
    ui.add_space(8.0);
    ui.separator();
    ui.horizontal(|ui| {
        let can_confirm = dlg.error.is_none();
        if ui
            .add_enabled(can_confirm, egui::Button::new("创建(新窗口打开)"))
            .clicked()
        {
            action = Some(if dlg.template_mode {
                DialogAction::CreateTemplate {
                    template: dlg
                        .templates
                        .get(dlg.template_sel)
                        .cloned()
                        .unwrap_or_default(),
                    location: dlg.spec.location.clone(),
                    name: dlg.spec.name.clone(),
                }
            } else {
                DialogAction::Create(dlg.spec.clone())
            });
        }
        if ui.button("取消").clicked() {
            action = Some(DialogAction::Cancel);
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_expected_base_sizes() {
        assert_eq!(ArtboardPreset::Web1920.base_size(), Some((1920.0, 1080.0)));
        assert_eq!(ArtboardPreset::Mobile375.base_size(), Some((375.0, 667.0)));
        assert_eq!(ArtboardPreset::A4.base_size(), Some((794.0, 1123.0)));
        assert_eq!(ArtboardPreset::Custom.base_size(), None);
    }

    #[test]
    fn orientation_swaps_dimensions() {
        // 横向 1440×900:宽 ≥ 高
        let (w, h) = ArtboardPreset::Web1440.sized(false, 0.0, 0.0);
        assert_eq!((w, h), (1440.0, 900.0));
        // 纵向:宽高对调
        let (w, h) = ArtboardPreset::Web1440.sized(true, 0.0, 0.0);
        assert_eq!((w, h), (900.0, 1440.0));
        // 本就竖长的预设选横向 → 对调成横
        let (w, h) = ArtboardPreset::Mobile375.sized(false, 0.0, 0.0);
        assert_eq!((w, h), (667.0, 375.0));
        // 自定义吃输入
        assert_eq!(
            ArtboardPreset::Custom.sized(false, 500.0, 300.0),
            (500.0, 300.0)
        );
    }

    #[test]
    fn sanitize_strips_filesystem_hostile_chars() {
        assert_eq!(sanitize_dir_name(" a/b<c>|:*?\" "), "a-b-c------");
        assert_eq!(sanitize_dir_name("正常名"), "正常名");
    }

    #[test]
    fn spec_validation_rejects_bad_input() {
        let mut spec = NewProjectSpec::default();
        assert!(spec.validate().is_none(), "默认规格应合法");
        spec.name = "  ".into();
        assert!(spec.validate().is_some(), "空名必须被拒");
        spec.name = "x".into();
        spec.location = std::env::temp_dir().join("vb-不存在位置-xyz");
        assert!(spec.validate().is_some(), "位置不存在必须被拒");
    }

    #[test]
    fn create_project_writes_importable_project() {
        // 端到端:生成 → 能被 import_project 读回(合法序列化路径的直接证据)
        let base = std::env::temp_dir().join(format!("vb-newproj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let spec = NewProjectSpec {
            name: "我的项目".into(),
            location: base.clone(),
            preset: ArtboardPreset::Mobile375,
            portrait: false,
            artboards: 3,
            output: OutputChoice::ExternalCss,
            ..NewProjectSpec::default()
        };
        let dir = create_project(&spec).expect("生成项目");
        assert!(dir.join("index.html").is_file());
        assert!(dir.join("styles").join("main.css").is_file());
        assert!(dir.join("assets").is_dir(), "assets/ 必须存在");
        let html = std::fs::read_to_string(dir.join("index.html")).unwrap();
        assert!(html.contains("vb-artboard"), "必须有画板标记");
        assert!(html.contains("data-vb-id"), "必须有稳定 id");
        let r = vb_doc::import::import_project(&dir).expect("生成项目必须可导入");
        assert_eq!(r.doc.artboards.len(), 3, "画板数要生效");
        assert_eq!(r.doc.meta.title, "我的项目");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn create_project_refuses_existing_dir() {
        let base = std::env::temp_dir().join(format!("vb-newproj-ex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let spec = NewProjectSpec {
            name: "已存在".into(),
            location: base.clone(),
            ..NewProjectSpec::default()
        };
        std::fs::create_dir_all(spec.target_dir()).unwrap();
        assert!(create_project(&spec).is_err(), "目录已存在必须被拒");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn templates_root_falls_back_to_source_tree_in_dev() {
        // 开发机(CARGO_MANIFEST_DIR 可用)必须解析到仓库 examples/
        if std::env::var("CARGO_MANIFEST_DIR").is_ok() {
            let root = templates_root();
            assert!(root.is_some(), "开发期应能解析模板根:{root:?}");
            assert!(root.unwrap().join("landing").exists());
        }
        // 无论环境如何,列表函数都不 panic
        let _ = list_templates();
    }
}
