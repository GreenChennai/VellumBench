//! Thin host-side export bridge from the session's immutable DrawList projection.
//!
//! The project Document remains private to `ProjectSession`; this module renders
//! only snapshots and uses the existing SVG/CPU exporters plus Kiln's atomic write.

use std::path::Path;

use sable::gpui::{App, Entity, Window};
use vb_common::atomic::write_atomic;
use vb_common::{MAX_CANVAS_EDGE, MAX_CANVAS_PIXELS};
use vb_platform::gpui_backend::RfdDialog;
use vb_platform::{FileDialog, FileFilter};
use vb_render::encode::{DrawKind, DrawList};
use vb_session::i18n::{t, t_args, FluentValue};
use vb_session::project::ProjectSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Svg,
    Png,
}

impl ExportFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "svg" => Some(Self::Svg),
            "png" => Some(Self::Png),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Png => "png",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportArtifact {
    pub bytes: Vec<u8>,
    pub warning_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    InvalidDimensions,
    PixelBudgetExceeded,
    Render(String),
}

pub fn default_filename(title: &str, format: ExportFormat) -> String {
    let stem: String = title
        .chars()
        .take(96)
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
            {
                '-'
            } else {
                character
            }
        })
        .collect();
    let stem = stem.trim_matches(|character| matches!(character, ' ' | '.' | '-'));
    if stem.is_empty() {
        format!("Untitled.{}", format.extension())
    } else {
        format!("{stem}.{}", format.extension())
    }
}

pub fn render_draw_list(
    list: &DrawList,
    format: ExportFormat,
) -> Result<ExportArtifact, ExportError> {
    validate_dimensions(list)?;
    match format {
        ExportFormat::Svg => {
            let svg = vb_export::svg::render_svg(list, 1, false);
            Ok(ExportArtifact {
                bytes: svg.into_bytes(),
                warning_count: unsupported_svg_count(list),
            })
        }
        ExportFormat::Png => {
            let rendered =
                vb_render::cpu::render_png(list, 1.0, false, None).map_err(ExportError::Render)?;
            Ok(ExportArtifact {
                bytes: rendered.png,
                warning_count: rendered.warnings.len(),
            })
        }
    }
}

pub fn export_project(
    session: Option<&Entity<ProjectSession>>,
    window: &mut Window,
    app: &mut App,
) -> Result<Option<String>, String> {
    let Some(session) = session else {
        return Err(t("ui-canvas-no-project-session"));
    };
    let snapshot = session
        .read(app)
        .active_canvas_snapshot()
        .map_err(|error| export_error_message(ExportError::Render(error.to_string())))?;

    let filters = [
        FileFilter::new("SVG", &["svg"]),
        FileFilter::new("PNG", &["png"]),
    ];
    let default_name = default_filename(&snapshot.title, ExportFormat::Svg);
    let mut dialog = RfdDialog::new();
    let Some(mut path) = dialog.save_file(&t("ui-canvas-export-title"), &default_name, &filters)
    else {
        return Ok(None);
    };
    if path.extension().is_none() {
        path.set_extension("svg");
    }
    let Some(format) = ExportFormat::from_path(&path) else {
        return Err(t("ui-canvas-export-unsupported-format"));
    };
    let artifact = render_draw_list(&snapshot.draw_list, format).map_err(export_error_message)?;
    write_atomic(&path, &artifact.bytes).map_err(|error| {
        t_args(
            "ui-canvas-export-write-failed",
            &[("error", FluentValue::from(error.to_string()))],
        )
    })?;

    let display_path = path.display().to_string();
    let warning_count = artifact.warning_count + snapshot.warnings.len();
    let message = if warning_count == 0 {
        t_args(
            "ui-canvas-exported",
            &[("path", FluentValue::from(display_path))],
        )
    } else {
        t_args(
            "ui-canvas-exported-with-warnings",
            &[
                ("path", FluentValue::from(display_path)),
                ("count", FluentValue::from(warning_count as i64)),
            ],
        )
    };
    let _ = window;
    Ok(Some(message))
}

fn export_error_message(error: ExportError) -> String {
    match error {
        ExportError::InvalidDimensions => t("ui-canvas-export-invalid-dimensions"),
        ExportError::PixelBudgetExceeded => t("ui-canvas-export-too-large"),
        ExportError::Render(error) => t_args(
            "ui-canvas-export-render-failed",
            &[("error", FluentValue::from(error))],
        ),
    }
}

fn validate_dimensions(list: &DrawList) -> Result<(), ExportError> {
    let max_edge = f64::from(MAX_CANVAS_EDGE);
    let width = list.w.ceil();
    let height = list.h.ceil();
    if !width.is_finite()
        || !height.is_finite()
        || width < 1.0
        || height < 1.0
        || width > max_edge
        || height > max_edge
    {
        return Err(ExportError::InvalidDimensions);
    }
    let pixels = (width as u64).saturating_mul(height as u64);
    if pixels > MAX_CANVAS_PIXELS {
        return Err(ExportError::PixelBudgetExceeded);
    }
    Ok(())
}

fn unsupported_svg_count(list: &DrawList) -> usize {
    list.items
        .iter()
        .filter(|item| {
            (item.kind == DrawKind::Image && item.image.is_none())
                || item.kind == DrawKind::FrozenPlaceholder
                || item.clip.is_some()
                || item.overflow_clip.is_some()
                || item
                    .filter
                    .as_ref()
                    .is_some_and(|filter| !filter.is_identity())
                || (item.kind == DrawKind::Text
                    && item
                        .label
                        .as_ref()
                        .is_some_and(|hint| !hint.segments.is_empty()))
        })
        .count()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use vb_render::encode::DrawList;

    use super::{default_filename, render_draw_list, ExportError, ExportFormat};

    fn tiny_list() -> DrawList {
        DrawList {
            w: 2.0,
            h: 2.0,
            background: [1.0, 1.0, 1.0, 1.0],
            items: Vec::new(),
        }
    }

    #[test]
    fn export_format_uses_case_insensitive_safe_extensions() {
        assert_eq!(
            ExportFormat::from_path(Path::new("poster.SVG")),
            Some(ExportFormat::Svg)
        );
        assert_eq!(
            ExportFormat::from_path(Path::new("poster.png")),
            Some(ExportFormat::Png)
        );
        assert_eq!(ExportFormat::from_path(Path::new("poster.pdf")), None);
        assert_eq!(ExportFormat::from_path(Path::new("poster")), None);
    }

    #[test]
    fn default_name_removes_path_and_control_characters() {
        assert_eq!(
            default_filename("../A\\B:poster\n", ExportFormat::Svg),
            "A-B-poster.svg"
        );
        assert_eq!(default_filename("..///", ExportFormat::Png), "Untitled.png");
    }

    #[test]
    fn renders_svg_and_png_from_drawlist_without_document_access() {
        let list = tiny_list();
        let svg = render_draw_list(&list, ExportFormat::Svg).expect("render SVG");
        assert!(String::from_utf8(svg.bytes)
            .expect("SVG UTF-8")
            .contains("<svg "));
        let png = render_draw_list(&list, ExportFormat::Png).expect("render PNG");
        assert!(png.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn rejects_invalid_or_over_budget_artboard_dimensions() {
        let mut list = tiny_list();
        list.w = f64::NAN;
        assert_eq!(
            render_draw_list(&list, ExportFormat::Png).unwrap_err(),
            ExportError::InvalidDimensions
        );
        list.w = 20_000.0;
        list.h = 20_000.0;
        assert_eq!(
            render_draw_list(&list, ExportFormat::Png).unwrap_err(),
            ExportError::PixelBudgetExceeded
        );
    }
}
