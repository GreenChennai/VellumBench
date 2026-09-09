//! 几何:重导出 kurbo(ADR-0002,Vello 原生几何库)+ 本项目常用别名。

pub use kurbo::{Affine, Circle, Ellipse, Point, Rect, RoundedRect, RoundedRectRadii, Vec2};

/// 画布世界坐标矩形(画板在无限平面上的位置)。
pub fn rect_xywh(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(x, y, x + w, y + h)
}

/// 世界坐标 → 画板本地坐标(画板左上为原点,Y 向下,ADR-0007)。
pub fn world_to_artboard(artboard_x: f64, artboard_y: f64, p: Point) -> Point {
    Point::new(p.x - artboard_x, p.y - artboard_y)
}
