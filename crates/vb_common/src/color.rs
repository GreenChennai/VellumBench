//! 颜色:解析与最短 hex 规范化(设计文档 04 篇 §5.2:小写 hex,`#ffffff` → `#fff`)。

/// sRGB 颜色(8bit 各通道)。sRGB 即网页唯一色彩空间(设计文档 03 篇 §5.4)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    pub const WHITE: Rgba = Rgba {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const BLACK: Rgba = Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// 最短 hex:`#fff` / `#aabbcc` / `#aabbccdd`(alpha < 255 才带)。
    pub fn to_shortest_hex(self) -> String {
        let hex = |v: u8| format!("{v:02x}");
        let (r, g, b, a) = (hex(self.r), hex(self.g), hex(self.b), hex(self.a));
        let compress = |pair: &str| {
            let b = pair.as_bytes();
            if b[0] == b[1] {
                format!("{}", b[0] as char)
            } else {
                pair.to_string()
            }
        };
        if self.a == 255 {
            let (r, g, b) = (compress(&r), compress(&g), compress(&b));
            if r.len() == 1 && g.len() == 1 && b.len() == 1 {
                format!("#{r}{g}{b}")
            } else {
                format!("#{}{}{}", hex(self.r), hex(self.g), hex(self.b))
            }
        } else {
            format!("#{r}{g}{b}{a}")
        }
    }

    pub fn to_rgb_f32(self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }
}

/// 解析颜色:hex(`#rgb #rgba #rrggbb #rrggbbaa`)、`rgb()/rgba()`、常用命名色。
pub fn parse_color(s: &str) -> Option<Rgba> {
    let t = s.trim();
    if let Some(hexpart) = t.strip_prefix('#') {
        return parse_hex(hexpart);
    }
    let lower = t.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("rgba(") {
        let v = parse_func_args(rest)?;
        return Some(Rgba::new(v[0], v[1], v[2], v[3]));
    }
    if let Some(rest) = lower.strip_prefix("rgb(") {
        // CSS Color 4:rgb() 可以带 alpha(`rgb(255 0 0 / 50%)`)
        let v = parse_func_args(rest)?;
        return Some(Rgba::new(v[0], v[1], v[2], v[3]));
    }
    if let Some(rest) = lower.strip_prefix("hsla(") {
        let (h, sl, li, a) = parse_hsl_args(rest)?;
        return Some(hsl_to_rgb(h, sl, li, a));
    }
    if let Some(rest) = lower.strip_prefix("hsl(") {
        let (h, sl, li, a) = parse_hsl_args(rest)?;
        return Some(hsl_to_rgb(h, sl, li, a));
    }
    named(&lower)
}

fn parse_hex(h: &str) -> Option<Rgba> {
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match h.len() {
        3 => {
            let v: Vec<u8> = (0..3)
                .map(|i| u8::from_str_radix(&h[i..i + 1], 16).map(|x| x * 17))
                .collect::<Result<_, _>>()
                .ok()?;
            Some(Rgba::new(v[0], v[1], v[2], 255))
        }
        4 => {
            let v: Vec<u8> = (0..4)
                .map(|i| u8::from_str_radix(&h[i..i + 1], 16).map(|x| x * 17))
                .collect::<Result<_, _>>()
                .ok()?;
            Some(Rgba::new(v[0], v[1], v[2], v[3]))
        }
        6 => Some(Rgba::new(
            u8::from_str_radix(&h[0..2], 16).ok()?,
            u8::from_str_radix(&h[2..4], 16).ok()?,
            u8::from_str_radix(&h[4..6], 16).ok()?,
            255,
        )),
        8 => Some(Rgba::new(
            u8::from_str_radix(&h[0..2], 16).ok()?,
            u8::from_str_radix(&h[2..4], 16).ok()?,
            u8::from_str_radix(&h[4..6], 16).ok()?,
            u8::from_str_radix(&h[6..8], 16).ok()?,
        )),
        _ => None,
    }
}

/// 解析 `r,g b[/ a]` 参数到 `)`;支持 0-255 与百分比,以及
/// CSS Color 4 现代空格语法(`rgb(255 0 0 / 50%)`,此前整条解析失败
/// 导致填充静默丢失)。
fn parse_func_args(rest: &str) -> Option<[u8; 4]> {
    let inner = rest.strip_suffix(')')?;
    // 现代语法:斜杠前 3 通道,后 alpha
    if inner.contains('/') {
        let (rgb_part, a_part) = inner.split_once('/')?;
        let chans: Vec<&str> = rgb_part.split_whitespace().collect();
        if chans.len() != 3 {
            return None;
        }
        let a = parse_alpha(a_part.trim())?;
        return Some([
            chan_u8(chans[0])?,
            chan_u8(chans[1])?,
            chan_u8(chans[2])?,
            a,
        ]);
    }
    // 空格语法无 alpha:`rgb(255 0 0)`(含逗号时不是空格语法)
    let spaced: Vec<&str> = inner.split_whitespace().collect();
    if !inner.contains(',') && spaced.len() == 3 {
        return Some([
            chan_u8(spaced[0])?,
            chan_u8(spaced[1])?,
            chan_u8(spaced[2])?,
            255,
        ]);
    }
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() != 3 && parts.len() != 4 {
        return None;
    }
    let ch = |s: &str| -> Option<u8> {
        if let Some(p) = s.strip_suffix('%') {
            let f: f64 = p.trim().parse().ok()?;
            Some((f * 255.0 / 100.0).round().clamp(0.0, 255.0) as u8)
        } else {
            let f: f64 = s.parse().ok()?;
            Some(f.round().clamp(0.0, 255.0) as u8)
        }
    };
    let a = if parts.len() == 4 {
        parse_alpha(parts[3])?
    } else {
        255
    };
    Some([ch(parts[0])?, ch(parts[1])?, ch(parts[2])?, a])
}

/// alpha:0-1 浮点或百分比。
fn parse_alpha(s: &str) -> Option<u8> {
    if let Some(p) = s.strip_suffix('%') {
        let f: f64 = p.trim().parse().ok()?;
        Some(((f / 100.0) * 255.0).round().clamp(0.0, 255.0) as u8)
    } else {
        let f: f64 = s.parse().ok()?;
        Some((f * 255.0).round().clamp(0.0, 255.0) as u8)
    }
}

/// 通道:0-255 数字或百分比。
fn chan_u8(s: &str) -> Option<u8> {
    if let Some(p) = s.strip_suffix('%') {
        let f: f64 = p.trim().parse().ok()?;
        Some((f * 255.0 / 100.0).round().clamp(0.0, 255.0) as u8)
    } else {
        let f: f64 = s.parse().ok()?;
        Some(f.round().clamp(0.0, 255.0) as u8)
    }
}

/// 解析 hsl/hsla 参数:`120, 50%, 50%[, a]` 或 `120deg 50% 50% / a`。
/// 返回 (hue 度数, s%, l%, alpha u8)。
fn parse_hsl_args(rest: &str) -> Option<(f64, f64, f64, u8)> {
    let inner = rest.strip_suffix(')')?;
    let (body, a_part) = if inner.contains('/') {
        let (b, a) = inner.split_once('/')?;
        (b, Some(a.trim()))
    } else if inner.contains(',') {
        // 逗号语法:h,s%,l%[,a]
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if parts.len() != 3 && parts.len() != 4 {
            return None;
        }
        let a = if parts.len() == 4 {
            parse_alpha(parts[3])?
        } else {
            255
        };
        return Some((
            parse_hue(parts[0])?,
            parse_pct(parts[1])?,
            parse_pct(parts[2])?,
            a,
        ));
    } else {
        (inner, None)
    };
    let comps: Vec<&str> = body.split_whitespace().collect();
    if comps.len() != 3 {
        return None;
    }
    let a = match a_part {
        Some(x) => parse_alpha(x)?,
        None => 255,
    };
    Some((
        parse_hue(comps[0])?,
        parse_pct(comps[1])?,
        parse_pct(comps[2])?,
        a,
    ))
}

/// 色相:`120` / `120deg` / `0.5turn` / `1.2rad` / `200grad`。
fn parse_hue(s: &str) -> Option<f64> {
    let t = s.trim();
    if let Some(v) = t.strip_suffix("deg") {
        return v.trim().parse().ok();
    }
    if let Some(v) = t.strip_suffix("turn") {
        let f: f64 = v.trim().parse().ok()?;
        return Some(f * 360.0);
    }
    if let Some(v) = t.strip_suffix("grad") {
        let f: f64 = v.trim().parse().ok()?;
        return Some(f * 360.0 / 400.0);
    }
    if let Some(v) = t.strip_suffix("rad") {
        let f: f64 = v.trim().parse().ok()?;
        return Some(f * 180.0 / std::f64::consts::PI);
    }
    t.parse().ok()
}

/// s/l:百分比或 0-1 数字。
fn parse_pct(s: &str) -> Option<f64> {
    let t = s.trim();
    if let Some(p) = t.strip_suffix('%') {
        return p.trim().parse().ok();
    }
    let f: f64 = t.parse().ok()?;
    Some(f * 100.0)
}

/// HSL → sRGB(标准算法;sRGB 即网页唯一色彩空间)。
fn hsl_to_rgb(h_deg: f64, s_pct: f64, l_pct: f64, a: u8) -> Rgba {
    let h = h_deg.rem_euclid(360.0) / 360.0;
    let s = (s_pct / 100.0).clamp(0.0, 1.0);
    let l = (l_pct / 100.0).clamp(0.0, 1.0);
    if s == 0.0 {
        let v = (l * 255.0).round() as u8;
        return Rgba::new(v, v, v, a);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue = |mut t: f64| -> f64 {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            return p + (q - p) * 6.0 * t;
        }
        if t < 0.5 {
            return q;
        }
        if t < 2.0 / 3.0 {
            return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
        }
        p
    };
    Rgba::new(
        (hue(h + 1.0 / 3.0) * 255.0).round() as u8,
        (hue(h) * 255.0).round() as u8,
        (hue(h - 1.0 / 3.0) * 255.0).round() as u8,
        a,
    )
}

fn named(lower: &str) -> Option<Rgba> {
    let c = match lower {
        "black" => Rgba::new(0, 0, 0, 255),
        "white" => Rgba::new(255, 255, 255, 255),
        "red" => Rgba::new(255, 0, 0, 255),
        "green" => Rgba::new(0, 128, 0, 255),
        "lime" => Rgba::new(0, 255, 0, 255),
        "blue" => Rgba::new(0, 0, 255, 255),
        "yellow" => Rgba::new(255, 255, 0, 255),
        "orange" => Rgba::new(255, 165, 0, 255),
        "purple" => Rgba::new(128, 0, 128, 255),
        "gray" | "grey" => Rgba::new(128, 128, 128, 255),
        "silver" => Rgba::new(192, 192, 192, 255),
        "maroon" => Rgba::new(128, 0, 0, 255),
        "navy" => Rgba::new(0, 0, 128, 255),
        "teal" => Rgba::new(0, 128, 128, 255),
        "olive" => Rgba::new(128, 128, 0, 255),
        "fuchsia" | "magenta" => Rgba::new(255, 0, 255, 255),
        "aqua" | "cyan" => Rgba::new(0, 255, 255, 255),
        "pink" => Rgba::new(255, 192, 203, 255),
        "brown" => Rgba::new(165, 42, 42, 255),
        "transparent" => Rgba::TRANSPARENT,
        _ => return None,
    };
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortest_hex() {
        assert_eq!(parse_color("#ffffff").unwrap().to_shortest_hex(), "#fff");
        assert_eq!(parse_color("#FFFFFF").unwrap().to_shortest_hex(), "#fff");
        assert_eq!(parse_color("#f00").unwrap().to_shortest_hex(), "#f00");
        assert_eq!(parse_color("#ff5a1f").unwrap().to_shortest_hex(), "#ff5a1f");
        assert_eq!(
            parse_color("rgba(0, 168, 112, 0.5)")
                .unwrap()
                .to_shortest_hex(),
            "#00a87080"
        );
        assert_eq!(
            parse_color("rgb(100%, 0%, 0%)").unwrap().to_shortest_hex(),
            "#f00"
        );
        assert_eq!(parse_color("orange").unwrap().to_shortest_hex(), "#ffa500");
    }

    #[test]
    fn roundtrip_parse() {
        let c = parse_color("#2b1a12").unwrap();
        assert_eq!((c.r, c.g, c.b, c.a), (0x2b, 0x1a, 0x12, 255));
        assert!(parse_color("not-a-color").is_none());
        assert!(parse_color("#12345").is_none());
    }
}

#[cfg(test)]
mod b2_tests {
    use super::*;

    /// B2:CSS Color 4 现代空格/斜杠语法。
    #[test]
    fn modern_space_syntax() {
        assert_eq!(
            parse_color("rgb(255 0 0)").unwrap().to_shortest_hex(),
            "#f00"
        );
        assert_eq!(
            parse_color("rgb(255 0 0 / 50%)").unwrap().to_shortest_hex(),
            "#ff000080"
        );
        assert_eq!(
            parse_color("rgba(0 168 112 / 0.5)")
                .unwrap()
                .to_shortest_hex(),
            "#00a87080"
        );
        // 逗号语法不回退
        assert_eq!(
            parse_color("rgba(0, 168, 112, 0.5)")
                .unwrap()
                .to_shortest_hex(),
            "#00a87080"
        );
    }

    /// B2:hsl()/hsla() 全形式。
    #[test]
    fn hsl_syntax() {
        // hsl(120, 50%, 50%) = rgb(64,191,64)
        assert_eq!(
            parse_color("hsl(120, 50%, 50%)").unwrap().to_shortest_hex(),
            "#40bf40"
        );
        // 0.5turn = 180° = 青色
        assert_eq!(
            parse_color("hsl(0.5turn 100% 50%)")
                .unwrap()
                .to_shortest_hex(),
            "#0ff"
        );
        assert_eq!(
            parse_color("hsla(30, 100%, 50%, 0.5)")
                .unwrap()
                .to_shortest_hex(),
            "#ff800080"
        );
        // 灰色系:s=0
        assert_eq!(
            parse_color("hsl(200 0% 50%)").unwrap().to_shortest_hex(),
            "#808080"
        );
    }
}
