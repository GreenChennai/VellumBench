//! 渐变面板(`^F9`,副文档 05-2)+ 结构化渐变写回。
//!
//! **单一真相**:渐变的唯一结构化表示是 [`vb_ui::gradient::Gradient`]
//! (色标有位置 / 显式标记 / 中点 / 不透明度)。本模块只做三件事:
//! ① 从文档**投影**出当前渐变;② 把面板/画布批注的编辑**折成命令**;
//! ③ 渲染面板。构建器是纯函数(可文档状态级单测),渲染层只调它们。
//!
//! **写回落点**(按优先级):
//! 1. 节点已由外观模型接管填充(`data-vb-appearance` 里有 Fill 条目)→
//!    改写该条目的 `FillBody::Gradient`,与外观面板同源、不打架;
//! 2. 否则直接改 `background-image` 的对应层(**保留其余背景层**,不吞数据)。

use vb_doc::commands::Command;
use vb_doc::model::{Document, NodeKind};
use vb_ui::components::{caption, ColorField, NumField, NumFieldResponse};
use vb_ui::gradient::{self, GradKind, Gradient, Stop};

use crate::app::appearance::{self, AppearanceItem, AppearanceTarget, FillBody, BLEND_MODES};
use crate::app::VellumApp;

/// 构建器结果:`Err` 是给用户看的中文提示(绝不静默)。
pub type GradResult = Result<Option<Command>, String>;

/// 画布批注的屏幕端点 + 当前渐变(供 `canvas.rs` 绘制与命中)。
pub type AnnotScreen = ((f32, f32), (f32, f32), Gradient);

/// 渐变写回落点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradSink {
    /// 外观模型第 `index` 条填充条目。
    FillItem { index: usize },
    /// `background-image` 的第 `layer` 层。
    Layer { layer: usize },
}

/// 面板/画布共用的渐变投影。
#[derive(Debug, Clone)]
pub struct GradProj {
    pub sid: String,
    pub kind: NodeKind,
    /// 背景层(拆分后的原样串;`Layer` 落点时整组回写)。
    pub layers: Vec<String>,
    /// 当前渐变所在层下标(`None` = 该对象还没有渐变)。
    pub idx: Option<usize>,
    pub sink: GradSink,
}

impl GradProj {
    /// 当前渐变(有则解析)。
    pub fn gradient(&self) -> Option<Gradient> {
        gradient::parse(&self.layers[self.idx?])
    }

    /// 把第 `idx` 层替换为 `g`,返回回写用的层列表。
    pub fn with_layer(&self, g: &Gradient) -> Vec<String> {
        let mut layers = self.layers.clone();
        let i = self.idx.unwrap_or(0);
        if layers.is_empty() {
            layers.push(g.to_css());
        } else if i < layers.len() {
            layers[i] = g.to_css();
        } else {
            layers.push(g.to_css());
        }
        layers
    }

    /// 该对象的回退填充色(生成渐变时的起点色)。
    pub fn fill_hex(&self, doc: &Document) -> String {
        doc.find_by_sid(&self.sid)
            .and_then(|nid| doc.nodes.get(nid))
            .and_then(|n| n.fill_color())
            .map(|c| c.to_shortest_hex())
            .unwrap_or_else(|| {
                // vb-token-ok: 文档内容色(无填充时的渐变起点)
                "#d4d4d4".to_string()
            })
    }
}

/// 投影选中对象的渐变状态(每帧从文档回读,面板不私存)。
pub fn project(doc: &Document, sid: &str) -> Option<GradProj> {
    let nid = doc.find_by_sid(sid)?;
    let n = doc.nodes.get(nid)?;
    let attrs: Vec<(String, String)> = n
        .attrs
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let model = appearance::decode_model(&n.kind, &n.style, &attrs);

    // ① 外观模型的 Fill 条目(模型接管,优先级最高)。
    // 一条填充 = 一个背景层,故优先认领**已经是渐变**的那条;没有渐变时
    // 以首条填充作为「生成渐变」的落点(其余层保持不动,不吞数据)。
    let fills: Vec<(usize, &FillBody)> = model
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| match it {
            AppearanceItem::Fill(f) => Some((i, &f.body)),
            _ => None,
        })
        .collect();
    let picked = fills
        .iter()
        .find(|(_, b)| matches!(b, FillBody::Gradient { .. }))
        .or_else(|| fills.first());
    if let Some(&(index, body)) = picked {
        let is_grad = matches!(body, FillBody::Gradient { .. });
        let value = match body {
            FillBody::Gradient { value } | FillBody::Solid { value } | FillBody::Raw { value } => {
                value.clone()
            }
        };
        return Some(GradProj {
            sid: sid.to_string(),
            kind: n.kind.clone(),
            layers: vec![value],
            idx: is_grad.then_some(0),
            sink: GradSink::FillItem { index },
        });
    }

    // ② 直接读 background-image 的层组
    let raw = n
        .style
        .iter()
        .find(|d| d.prop == "background-image")
        .map(|d| d.value.clone())
        .unwrap_or_default();
    let layers: Vec<String> = if raw.trim().is_empty() {
        Vec::new()
    } else {
        gradient::split_top_commas(&raw)
    };
    let idx = layers.iter().position(|l| gradient::parse(l).is_some());
    Some(GradProj {
        sid: sid.to_string(),
        kind: n.kind.clone(),
        layers,
        idx,
        sink: GradSink::Layer {
            layer: idx.unwrap_or(0),
        },
    })
}

/// 生成写回命令。`layers` 为回写后的完整层组(见 [`GradProj::with_layer`])。
pub fn write_cmd(doc: &Document, p: &GradProj, layers: &[String]) -> GradResult {
    match appearance::target_of(&p.kind) {
        AppearanceTarget::Text => {
            return Err("文字对象的渐变填充计划于 v2(CSS 无文字渐变);可先改纯色".into())
        }
        AppearanceTarget::Vector => {
            return Err("矢量路径的渐变填充计划于 v2(SVG defs 未建模);可先改纯色".into())
        }
        AppearanceTarget::Frozen => return Err("冻结对象(原样片段)不可编辑渐变".into()),
        AppearanceTarget::Box => {}
    }
    let first = layers.first().cloned().unwrap_or_default();
    match p.sink {
        GradSink::FillItem { index } => {
            appearance::set_fill_body_cmd(doc, &p.sid, index, FillBody::Gradient { value: first })
        }
        GradSink::Layer { .. } => {
            let nid = doc
                .find_by_sid(&p.sid)
                .ok_or_else(|| "对象不存在".to_string())?;
            let n = doc.nodes.get(nid).ok_or_else(|| "对象不存在".to_string())?;
            let value = layers.join(", ");
            Ok(Some(Command::SetStyle {
                sid: p.sid.clone(),
                new: super::set_style_prop(n.style.clone(), "background-image", &value),
                old: None,
            }))
        }
    }
}

/// 为「还没有渐变」的对象生成回退渐变(现填充 → 白)。
pub fn fallback_for(p: &GradProj, doc: &Document, angle: f64) -> Gradient {
    Gradient::fallback_linear(angle, &p.fill_hex(doc))
}

/// 角度渐变(conic):`design/06 §3.11` 标 v2 —— 面板只给禁用项 + 提示。
pub const CONIC_TOOLTIP: &str = "角度渐变(conic)计划于 v2 支持;当前提供线性与径向";

impl VellumApp {
    /// 主循环装配:渐变浮窗(^F9)。
    pub(crate) fn show_gradient_panel(&mut self, ui: &mut egui::Ui) {
        if !self.gradient_panel_open {
            return;
        }
        let mut open = true;
        egui::Window::new("渐变")
            .open(&mut open)
            .collapsible(false)
            .default_width(324.0)
            .show(ui.ctx(), |ui| self.gradient_panel_body(ui));
        self.gradient_panel_open = open;
    }

    /// 离散/参数编辑统一收口:成功入 undo,失败 toast(05-6「绝不沉默」)。
    fn grad_apply(&mut self, r: GradResult, discrete: bool) {
        match r {
            Ok(Some(cmd)) => {
                if discrete {
                    self.undo.merging_enabled = false;
                    self.exec(cmd);
                    self.undo.merging_enabled = true;
                } else {
                    self.exec(cmd);
                }
            }
            Ok(None) => {}
            Err(msg) => self.toast_warn(msg),
        }
    }

    /// 拖动/数值输入走提交会话(连续拖动 = 一条 undo)。
    fn grad_session(&mut self, r: NumFieldResponse, res: GradResult) {
        let cmd = match &res {
            Ok(Some(c)) => Some(c.clone()),
            _ => None,
        };
        self.num_commit(r, cmd);
        if let Err(msg) = res {
            self.toast_warn(msg);
        }
    }

    /// 写回一个渐变(供面板与画布批注共用)。
    pub(crate) fn grad_commit(&mut self, g: &Gradient, discrete: bool) {
        let Some(sid) = self.selection.last().cloned() else {
            self.toast_warn("未选中对象");
            return;
        };
        let Some(p) = project(&self.doc, &sid) else {
            self.toast_warn("对象不存在");
            return;
        };
        let layers = p.with_layer(g);
        let res = write_cmd(&self.doc, &p, &layers);
        self.grad_apply(res, discrete);
    }

    /// 画布渐变批注的屏幕端点(`start`, `end`)与当前渐变。
    pub(crate) fn grad_annot_screen(&self) -> Option<AnnotScreen> {
        let (x0, y0, x1, y1) = self.gradient_annot?;
        let sid = self.selection.last()?.clone();
        let g = project(&self.doc, &sid)?.gradient()?;
        let a = self.camera.world_to_screen(x0, y0);
        let b = self.camera.world_to_screen(x1, y1);
        Some(((a.0 as f32, a.1 as f32), (b.0 as f32, b.1 as f32), g))
    }

    /// 双击画布批注上的色标(05-2-3):选中该色标并打开渐变面板。
    /// `screen` 为画布局部坐标;返回是否命中。
    pub(crate) fn grad_annot_double_click(&mut self, screen: (f32, f32)) -> bool {
        let Some((a, b, g)) = self.grad_annot_screen() else {
            return false;
        };
        let Some(i) = vb_ui::gradient::hit_annot_stop(a, b, &g, screen, 11.0) else {
            return false;
        };
        self.gradient_sel = Some(i);
        self.gradient_panel_open = true;
        self.say(format!(
            "已选中渐变色标 {}/{} —— 在渐变面板改颜色/位置",
            i + 1,
            g.stops.len()
        ));
        true
    }

    fn gradient_panel_body(&mut self, ui: &mut egui::Ui) {
        let Some(sid) = self.selection.last().cloned() else {
            ui.label(caption(ui, "未选中对象 —— 选中一个盒对象后可编辑其渐变。"));
            return;
        };
        let Some(p) = project(&self.doc, &sid) else {
            ui.label(caption(ui, "对象已不存在。"));
            return;
        };

        // 目标能力门(诚实置灰 + 说明,不做假控件)
        match appearance::target_of(&p.kind) {
            AppearanceTarget::Text => {
                ui.label(caption(
                    ui,
                    "文字对象:文字渐变计划于 v2(可先用字符面板改字色)。",
                ));
                return;
            }
            AppearanceTarget::Vector => {
                ui.label(caption(ui, "矢量路径:渐变填充计划于 v2(SVG defs 未建模)。"));
                return;
            }
            AppearanceTarget::Frozen => {
                ui.label(caption(ui, "冻结对象(原样片段)不可编辑渐变。"));
                return;
            }
            AppearanceTarget::Box => {}
        }

        let tokens = self.doc.tokens.clone();
        let mut grad = p.gradient();

        // ── 类型 / 角度(控制条)──
        ui.horizontal(|ui| {
            ui.label("类型");
            let mut kind = grad.as_ref().map(|g| g.kind).unwrap_or(GradKind::Linear);
            let before = kind;
            ui.selectable_value(&mut kind, GradKind::Linear, "线性");
            ui.selectable_value(&mut kind, GradKind::Radial, "径向");
            ui.add_enabled(false, egui::Button::new("角度"))
                .on_disabled_hover_text(CONIC_TOOLTIP);
            if kind != before {
                if let Some(g) = grad.as_mut() {
                    g.kind = kind;
                    match kind {
                        GradKind::Radial => {
                            g.head = Some("circle at 50% 50%".to_string());
                            g.angle_explicit = false;
                        }
                        GradKind::Linear => {
                            // 径向 → 线性:锚点段必须剥掉,角度落地为显式值
                            g.head = None;
                            g.angle_explicit = true;
                        }
                    }
                    let layers = p.with_layer(g);
                    self.grad_apply(write_cmd(&self.doc, &p, &layers), true);
                } else {
                    self.toast_warn("该对象还没有渐变 —— 点下方「生成渐变」");
                }
            }
        });

        let Some(mut g) = grad.take() else {
            ui.label(caption(ui, "该对象当前没有渐变。"));
            if ui
                .button("用当前填充生成线性渐变")
                .on_hover_text("色标 = 现填充色 0% → 白色 100%(与画布拖动同一落点)")
                .clicked()
            {
                let f = fallback_for(&p, &self.doc, 90.0);
                self.grad_commit(&f, true);
            }
            return;
        };

        // ── 角度(线性)──
        if g.kind == GradKind::Linear {
            let mut ang = g.angle;
            let r = NumField::new("角度", &mut ang)
                .unit("°")
                .speed(1.0)
                .step(1.0)
                .width(64.0)
                .ui(ui);
            if r.changed {
                g.angle = ang;
                g.angle_explicit = true; // 用户显式给角度 → 必须落地写 `Ndeg`
                let layers = p.with_layer(&g);
                let res = write_cmd(&self.doc, &p, &layers);
                self.grad_session(r, res);
            }
        }

        ui.add_space(2.0);
        // ── 自绘色标条(V4 决策)──
        let bar = vb_ui::gradient::gradient_bar(ui, &mut g, &mut self.gradient_sel, &tokens);
        if bar.changed && !bar.discrete {
            let layers = p.with_layer(&g);
            let res = write_cmd(&self.doc, &p, &layers);
            self.grad_session(
                NumFieldResponse {
                    changed: true,
                    scrub_started: bar.drag_started,
                    scrub_ended: bar.drag_ended,
                    ..Default::default()
                },
                res,
            );
        } else if bar.discrete {
            let layers = p.with_layer(&g);
            self.grad_apply(write_cmd(&self.doc, &p, &layers), true);
        }
        if let Some(i) = bar.double_clicked {
            self.gradient_sel = Some(i);
            self.say("双击色标:在下方改颜色/位置/不透明度");
        }
        ui.label(caption(
            ui,
            "拖动圆点改位置 · 点空白加点 · Alt+点击删点 · 拖菱形改中点 · 双击选中改色",
        ));

        // ── 选中色标编辑 ──
        let sel = self.gradient_sel.unwrap_or(0).min(g.stops.len() - 1);
        self.gradient_sel = Some(sel);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(format!("色标 {}/{}", sel + 1, g.stops.len()));
            if ui
                .button("＋ 加点")
                .on_hover_text("在选中色标与下一个之间插入中点色")
                .clicked()
            {
                if let Some(m) = g.midpoint_default(sel) {
                    let c = g.sample(m, &tokens);
                    g.stops.push(Stop::new(m, gradient::rgba_shortest(c)));
                    g.stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
                    let layers = p.with_layer(&g);
                    self.grad_apply(write_cmd(&self.doc, &p, &layers), true);
                }
            }
            if ui
                .add_enabled(g.stops.len() > 2, egui::Button::new("删除色标"))
                .on_disabled_hover_text("渐变至少保留两个色标")
                .clicked()
            {
                g.stops.remove(sel);
                self.gradient_sel = Some(0);
                let layers = p.with_layer(&g);
                self.grad_apply(write_cmd(&self.doc, &p, &layers), true);
            }
            if ui
                .button("反向")
                .on_hover_text("线性 = 角度 +180°;径向 = 色标镜像")
                .clicked()
            {
                g.reverse();
                let layers = p.with_layer(&g);
                self.grad_apply(write_cmd(&self.doc, &p, &layers), true);
            }
        });

        let mut pos_pct = g.stops[sel].pos * 100.0;
        let r_pos = NumField::new("位置", &mut pos_pct)
            .unit("%")
            .speed(1.0)
            .step(1.0)
            .range(0.0, 100.0)
            .width(64.0)
            .ui(ui);
        if r_pos.changed {
            g.stops[sel].pos = pos_pct / 100.0;
            g.stops[sel].explicit = true;
            let layers = p.with_layer(&g);
            let res = write_cmd(&self.doc, &p, &layers);
            self.grad_session(r_pos, res);
        }

        // 颜色(点击色块 = 紧凑取色器;Alt+点击 = 完整取色器)
        let mut col = gradient::stop_color(&g.stops[sel], &tokens).unwrap_or_default();
        let cf = ColorField::new("颜色", &mut col).doc_tokens(&tokens).ui(ui);
        if cf.changed || cf.var_picked.is_some() {
            g.stops[sel].color = match cf.var_picked {
                Some(name) => format!("var(--{name})"),
                None => gradient::rgba_shortest(col),
            };
            let layers = p.with_layer(&g);
            self.grad_apply(write_cmd(&self.doc, &p, &layers), true);
        }

        // 不透明度标(05-2-1)
        let mut alpha_pct = gradient::stop_alpha(&g.stops[sel], &tokens) * 100.0;
        let r_a = NumField::new("不透明度", &mut alpha_pct)
            .unit("%")
            .speed(1.0)
            .step(1.0)
            .range(0.0, 100.0)
            .width(64.0)
            .ui(ui);
        if r_a.changed {
            let c = gradient::with_alpha(&g.stops[sel], alpha_pct / 100.0, &tokens);
            g.stops[sel].color = c;
            let layers = p.with_layer(&g);
            let res = write_cmd(&self.doc, &p, &layers);
            self.grad_session(r_a, res);
        }

        ui.separator();
        ui.label(vb_ui::mono(&g.to_css()));
        let _ = BLEND_MODES;
    }
}
