//! Primitivos de cor sRGB (`HexColor`) e RGBA (`RgbaColor`) com parser multi-formato.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use specta::Type;

use crate::error::ThemeError;
use crate::oklch::{Oklch, hsl_to_srgb};

/// Cor sólida canônica em sRGB de 24 bits serializada como `#rrggbb` minúsculo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HexColor {
    /// Canal vermelho (`0..=255`).
    pub r: u8,
    /// Canal verde (`0..=255`).
    pub g: u8,
    /// Canal azul (`0..=255`).
    pub b: u8,
}

impl Type for HexColor {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        String::definition(types)
    }
}

impl HexColor {
    /// Constrói um `HexColor` a partir das componentes `r`, `g`, `b`.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Faz o parsing de uma cor opaca nos formatos `#RGB`, `#RRGGBB`, `rgb(...)`, `hsl(...)` ou nome CSS básico.
    ///
    /// Caso um canal alfa seja fornecido (`#RGBA`, `#RRGGBBAA`, `rgba(...)`, `hsla(...)`), ele deve ser `1.0` (`255`);
    /// para cores semitransparentes com `alpha < 1.0`, utilize `parse_color_with_bg` ou `RgbaColor::parse`.
    pub fn parse_field(field: &str, raw: &str) -> Result<Self, ThemeError> {
        let rgba = RgbaColor::parse_field(field, raw)?;
        if rgba.a < 255 {
            return Err(ThemeError::InvalidColor {
                field: field.to_string(),
                value: raw.to_string(),
                reason:
                    "Cor com transparência exige composição sobre o fundo (use parse_color_with_bg)"
                        .to_string(),
            });
        }
        Ok(Self::new(rgba.r, rgba.g, rgba.b))
    }

    /// Faz o parsing de uma string de cor sem contexto de campo específico.
    pub fn parse(raw: &str) -> Result<Self, ThemeError> {
        Self::parse_field("color", raw)
    }

    /// Converte para string canônica `#rrggbb` minúscula.
    #[must_use]
    pub fn to_hex_string(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Converte esta cor sRGB para o espaço perceptual OKLCH.
    #[must_use]
    pub fn to_oklch(self) -> Oklch {
        Oklch::from_srgb(self)
    }

    /// Compõe uma cor `RgbaColor` sobre este `HexColor` de fundo (`alpha * fg + (1 - alpha) * bg`).
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::suboptimal_flops
    )]
    pub fn composite_over(self, fg: RgbaColor) -> Self {
        if fg.a == 255 {
            return Self::new(fg.r, fg.g, fg.b);
        }
        let alpha = f32::from(fg.a) / 255.0;
        let inv = 1.0 - alpha;
        let r = (f32::from(fg.r) * alpha + f32::from(self.r) * inv)
            .round()
            .clamp(0.0, 255.0) as u8;
        let g = (f32::from(fg.g) * alpha + f32::from(self.g) * inv)
            .round()
            .clamp(0.0, 255.0) as u8;
        let b = (f32::from(fg.b) * alpha + f32::from(self.b) * inv)
            .round()
            .clamp(0.0, 255.0) as u8;
        Self::new(r, g, b)
    }

    /// Interpola linearmente no espaço OKLCH entre `self` e `other` com fator `t` (`0.0..=1.0`).
    #[must_use]
    pub fn mix_oklch(self, other: Self, t: f32) -> Self {
        let a = self.to_oklch();
        let b = other.to_oklch();
        a.lerp(b, t.clamp(0.0, 1.0)).to_srgb_gamut_mapped()
    }
}

impl fmt::Display for HexColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl FromStr for HexColor {
    type Err = ThemeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for HexColor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex_string())
    }
}

impl<'de> Deserialize<'de> for HexColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Cor sRGB com canal alfa explícito (`a: 0..=255`), usada para `selection_background`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RgbaColor {
    /// Canal vermelho (`0..=255`).
    pub r: u8,
    /// Canal verde (`0..=255`).
    pub g: u8,
    /// Canal azul (`0..=255`).
    pub b: u8,
    /// Canal alfa (`0..=255`, onde `255 == 1.0`).
    pub a: u8,
}

impl Type for RgbaColor {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        String::definition(types)
    }
}

impl RgbaColor {
    /// Constrói um `RgbaColor` a partir de `r`, `g`, `b`, `a`.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Constrói um `RgbaColor` a partir de um `HexColor` e alfa flutuante (`0.0..=1.0`).
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn from_hex_with_alpha(hex: HexColor, alpha: f32) -> Self {
        let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        Self::new(hex.r, hex.g, hex.b, a)
    }

    /// Retorna a cor sólida base ignorando o canal alfa.
    #[must_use]
    pub const fn without_alpha(self) -> HexColor {
        HexColor::new(self.r, self.g, self.b)
    }

    /// Retorna o canal alfa normalizado em `[0.0, 1.0]`.
    #[must_use]
    pub fn alpha_f32(self) -> f32 {
        f32::from(self.a) / 255.0
    }

    /// Formata no padrão canônico `#rrggbbaa` minúsculo.
    #[must_use]
    pub fn to_hex8_string(self) -> String {
        format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
    }

    /// Formata como `rgba(r, g, b, a)` compatível com CSS/xterm.js.
    #[must_use]
    pub fn to_css_rgba(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            let alpha = (f32::from(self.a) / 255.0 * 1000.0).round() / 1000.0;
            format!("rgba({}, {}, {}, {alpha})", self.r, self.g, self.b)
        }
    }

    /// Faz o parsing de qualquer formato suportado (`#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`, `rgb()`, `rgba()`, `hsl()`, `hsla()` ou nomes CSS básicos).
    pub fn parse(raw: &str) -> Result<Self, ThemeError> {
        Self::parse_field("color", raw)
    }

    /// Faz o parsing de qualquer formato suportado associando erros ao campo informado.
    pub fn parse_field(field: &str, raw: &str) -> Result<Self, ThemeError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ThemeError::InvalidColor {
                field: field.to_string(),
                value: raw.to_string(),
                reason: "String de cor vazia".to_string(),
            });
        }

        if let Some(hex) = trimmed.strip_prefix('#') {
            return Self::parse_hex_digits(field, raw, hex);
        }

        let lower = trimmed.to_ascii_lowercase();
        if let Some(named) = parse_basic_named_color(&lower) {
            return Ok(Self::new(named.r, named.g, named.b, 255));
        }

        if let Some(inner) = lower
            .strip_prefix("rgba(")
            .and_then(|s| s.strip_suffix(')'))
        {
            return Self::parse_rgb_fn(field, raw, inner, true);
        }
        if let Some(inner) = lower.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
            return Self::parse_rgb_fn(field, raw, inner, false);
        }
        if let Some(inner) = lower
            .strip_prefix("hsla(")
            .and_then(|s| s.strip_suffix(')'))
        {
            return Self::parse_hsl_fn(field, raw, inner, true);
        }
        if let Some(inner) = lower.strip_prefix("hsl(").and_then(|s| s.strip_suffix(')')) {
            return Self::parse_hsl_fn(field, raw, inner, false);
        }

        Err(ThemeError::InvalidColor {
            field: field.to_string(),
            value: raw.to_string(),
            reason: "Formato de cor não reconhecido (esperado #RRGGBB, rgb() ou hsl())".to_string(),
        })
    }

    fn parse_hex_digits(field: &str, raw: &str, hex: &str) -> Result<Self, ThemeError> {
        let err = |reason: &str| ThemeError::InvalidColor {
            field: field.to_string(),
            value: raw.to_string(),
            reason: reason.to_string(),
        };

        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(err("Caracteres hexadecimais inválidos"));
        }

        match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).map_err(|_| err("Canal R inválido"))?;
                let g = u8::from_str_radix(&hex[1..2], 16).map_err(|_| err("Canal G inválido"))?;
                let b = u8::from_str_radix(&hex[2..3], 16).map_err(|_| err("Canal B inválido"))?;
                Ok(Self::new(r * 17, g * 17, b * 17, 255))
            }
            4 => {
                let r = u8::from_str_radix(&hex[0..1], 16).map_err(|_| err("Canal R inválido"))?;
                let g = u8::from_str_radix(&hex[1..2], 16).map_err(|_| err("Canal G inválido"))?;
                let b = u8::from_str_radix(&hex[2..3], 16).map_err(|_| err("Canal B inválido"))?;
                let a = u8::from_str_radix(&hex[3..4], 16).map_err(|_| err("Canal A inválido"))?;
                Ok(Self::new(r * 17, g * 17, b * 17, a * 17))
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| err("Canal R inválido"))?;
                let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| err("Canal G inválido"))?;
                let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| err("Canal B inválido"))?;
                Ok(Self::new(r, g, b, 255))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| err("Canal R inválido"))?;
                let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| err("Canal G inválido"))?;
                let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| err("Canal B inválido"))?;
                let a = u8::from_str_radix(&hex[6..8], 16).map_err(|_| err("Canal A inválido"))?;
                Ok(Self::new(r, g, b, a))
            }
            _ => Err(err(
                "Comprimento hexadecimal inválido (esperado 3, 4, 6 ou 8 dígitos)",
            )),
        }
    }

    fn parse_rgb_fn(
        field: &str,
        raw: &str,
        inner: &str,
        allow_alpha: bool,
    ) -> Result<Self, ThemeError> {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        let err = |reason: &str| ThemeError::InvalidColor {
            field: field.to_string(),
            value: raw.to_string(),
            reason: reason.to_string(),
        };

        if (allow_alpha && parts.len() != 4 && parts.len() != 3)
            || (!allow_alpha && parts.len() != 3)
        {
            return Err(err("Quantidade inválida de argumentos em rgb()/rgba()"));
        }

        let r = parse_u8_channel(parts[0]).ok_or_else(|| err("Canal R fora de 0..=255"))?;
        let g = parse_u8_channel(parts[1]).ok_or_else(|| err("Canal G fora de 0..=255"))?;
        let b = parse_u8_channel(parts[2]).ok_or_else(|| err("Canal B fora de 0..=255"))?;
        let a = if parts.len() == 4 {
            parse_alpha_channel(parts[3]).ok_or_else(|| err("Canal alfa fora de 0.0..=1.0"))?
        } else {
            255
        };

        Ok(Self::new(r, g, b, a))
    }

    fn parse_hsl_fn(
        field: &str,
        raw: &str,
        inner: &str,
        allow_alpha: bool,
    ) -> Result<Self, ThemeError> {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        let err = |reason: &str| ThemeError::InvalidColor {
            field: field.to_string(),
            value: raw.to_string(),
            reason: reason.to_string(),
        };

        if (allow_alpha && parts.len() != 4 && parts.len() != 3)
            || (!allow_alpha && parts.len() != 3)
        {
            return Err(err("Quantidade inválida de argumentos em hsl()/hsla()"));
        }

        let h_str = parts[0].strip_suffix("deg").unwrap_or(parts[0]).trim();
        let h: f32 = h_str.parse().map_err(|_| err("Matiz H inválido"))?;
        if !h.is_finite() {
            return Err(err("Matiz H não finito"));
        }

        let s_pct = parts[1]
            .strip_suffix('%')
            .ok_or_else(|| err("Saturação S deve terminar com %"))?
            .trim()
            .parse::<f32>()
            .map_err(|_| err("Saturação S inválida"))?;
        let l_pct = parts[2]
            .strip_suffix('%')
            .ok_or_else(|| err("Luminosidade L deve terminar com %"))?
            .trim()
            .parse::<f32>()
            .map_err(|_| err("Luminosidade L inválida"))?;

        if !(0.0..=100.0).contains(&s_pct) || !(0.0..=100.0).contains(&l_pct) {
            return Err(err("S e L em hsl() devem estar entre 0% e 100%"));
        }

        let a = if parts.len() == 4 {
            parse_alpha_channel(parts[3]).ok_or_else(|| err("Canal alfa fora de 0.0..=1.0"))?
        } else {
            255
        };

        let hex = hsl_to_srgb(h, s_pct / 100.0, l_pct / 100.0);
        Ok(Self::new(hex.r, hex.g, hex.b, a))
    }
}

impl fmt::Display for RgbaColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex8_string())
    }
}

impl FromStr for RgbaColor {
    type Err = ThemeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for RgbaColor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex8_string())
    }
}

impl<'de> Deserialize<'de> for RgbaColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Faz o parsing de uma cor e, caso ela tenha transparência (`alpha < 1.0`), compõe sobre `bg` (se fornecido).
pub fn parse_color_with_bg(
    field: &str,
    raw: &str,
    bg: Option<HexColor>,
) -> Result<HexColor, ThemeError> {
    let rgba = RgbaColor::parse_field(field, raw)?;
    if rgba.a == 255 {
        return Ok(rgba.without_alpha());
    }
    let Some(bg_color) = bg else {
        return Err(ThemeError::InvalidColor {
            field: field.to_string(),
            value: raw.to_string(),
            reason: "Fundo não pode possuir transparência sem cor base para composição".to_string(),
        });
    };
    Ok(bg_color.composite_over(rgba))
}

fn parse_u8_channel(s: &str) -> Option<u8> {
    if let Some(pct_str) = s.strip_suffix('%') {
        let pct: f32 = pct_str.trim().parse().ok()?;
        if !(0.0..=100.0).contains(&pct) {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some((pct / 100.0 * 255.0).round() as u8)
    } else {
        s.parse::<u8>().ok()
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn parse_alpha_channel(s: &str) -> Option<u8> {
    if let Some(pct_str) = s.strip_suffix('%') {
        let pct: f32 = pct_str.trim().parse().ok()?;
        if !(0.0..=100.0).contains(&pct) {
            return None;
        }
        Some((pct / 100.0 * 255.0).round() as u8)
    } else {
        let alpha: f32 = s.parse().ok()?;
        if !(0.0..=1.0).contains(&alpha) {
            return None;
        }
        Some((alpha * 255.0).round() as u8)
    }
}

/// Nomes CSS básicos suportados em esquemas legados.
fn parse_basic_named_color(lower: &str) -> Option<HexColor> {
    match lower {
        "black" => Some(HexColor::new(0x00, 0x00, 0x00)),
        "white" => Some(HexColor::new(0xff, 0xff, 0xff)),
        "red" => Some(HexColor::new(0xff, 0x00, 0x00)),
        "green" => Some(HexColor::new(0x00, 0x80, 0x00)),
        "lime" => Some(HexColor::new(0x00, 0xff, 0x00)),
        "blue" => Some(HexColor::new(0x00, 0x00, 0xff)),
        "yellow" => Some(HexColor::new(0xff, 0xff, 0x00)),
        "cyan" | "aqua" => Some(HexColor::new(0x00, 0xff, 0xff)),
        "magenta" | "fuchsia" => Some(HexColor::new(0xff, 0x00, 0xff)),
        "gray" | "grey" => Some(HexColor::new(0x80, 0x80, 0x80)),
        "silver" => Some(HexColor::new(0xc0, 0xc0, 0xc0)),
        "maroon" => Some(HexColor::new(0x80, 0x00, 0x00)),
        "olive" => Some(HexColor::new(0x80, 0x80, 0x00)),
        "purple" => Some(HexColor::new(0x80, 0x00, 0x80)),
        "teal" => Some(HexColor::new(0x00, 0x80, 0x80)),
        "navy" => Some(HexColor::new(0x00, 0x00, 0x80)),
        "orange" => Some(HexColor::new(0xff, 0xa5, 0x00)),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_supported_color_formats() {
        assert_eq!(
            HexColor::parse("#1a2b3c").unwrap(),
            HexColor::new(0x1a, 0x2b, 0x3c)
        );
        assert_eq!(
            HexColor::parse("#1A2B3C").unwrap().to_hex_string(),
            "#1a2b3c"
        );
        assert_eq!(
            HexColor::parse("#abc").unwrap(),
            HexColor::new(0xaa, 0xbb, 0xcc)
        );
        assert_eq!(
            HexColor::parse("rgb(26, 43, 60)").unwrap(),
            HexColor::new(26, 43, 60)
        );
        assert_eq!(
            HexColor::parse("hsl(0, 100%, 50%)").unwrap(),
            HexColor::new(255, 0, 0)
        );
        assert_eq!(
            HexColor::parse("white").unwrap(),
            HexColor::new(255, 255, 255)
        );
    }

    #[test]
    fn composites_rgba_over_background() {
        let bg = HexColor::new(0, 0, 0);
        let c = parse_color_with_bg("fg", "rgba(255, 255, 255, 0.5)", Some(bg)).unwrap();
        assert_eq!(c, HexColor::new(128, 128, 128));
    }

    #[test]
    fn rejects_invalid_colors() {
        assert!(HexColor::parse("").is_err());
        assert!(HexColor::parse("#12").is_err());
        assert!(HexColor::parse("#zzzzzz").is_err());
        assert!(HexColor::parse("rgb(300, 0, 0)").is_err());
        assert!(HexColor::parse("rgba(255, 0, 0, 1.5)").is_err());
        assert!(HexColor::parse("hsl(120, 150%, 50%)").is_err());
    }
}
