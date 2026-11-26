//! `serge-style` — design tokens and the style IR shared by both targets
//! (`docs/02-architecture.md` §1). The web target turns a [`Style`] into CSS
//! ([`Style::to_css`]); native targets hand it to the layout solver and the
//! scene collector, so the web bundle never contains a layout solver.

use std::fmt;

/// An sRGB color with straight alpha.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha (255 is opaque).
    pub a: u8,
}

impl Color {
    /// Fully transparent.
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    /// Black.
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// White.
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    /// An opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    /// A color with alpha.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    /// `0xRRGGBB`, opaque.
    pub const fn hex(rgb: u32) -> Color {
        Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
    }

    /// The same color with alpha `a` (0.0–1.0).
    pub fn with_alpha(self, a: f32) -> Color {
        Color {
            a: (a.clamp(0.0, 1.0) * 255.0).round() as u8,
            ..self
        }
    }

    /// Linear mix toward `other` by `t` (0.0–1.0).
    pub fn mix(self, other: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Color::rgba(
            m(self.r, other.r),
            m(self.g, other.g),
            m(self.b, other.b),
            m(self.a, other.a),
        )
    }

    /// CSS form: `rgba(r, g, b, a)`.
    pub fn css(self) -> String {
        format!(
            "rgba({}, {}, {}, {})",
            self.r,
            self.g,
            self.b,
            f32::from(self.a) / 255.0
        )
    }
}

/// A length.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Length {
    /// Decided by the layout (content or stretch).
    #[default]
    Auto,
    /// Logical pixels (`16px`).
    Px(f32),
    /// Percent of the parent (`50%`).
    Percent(f32),
    /// Multiples of the root font size, 16 px (`1.5rem`).
    Rem(f32),
}

impl Length {
    /// Logical pixels, resolving `rem` (percent and auto are `None`).
    pub fn px(self) -> Option<f32> {
        match self {
            Length::Px(v) => Some(v),
            Length::Rem(v) => Some(v * ROOT_FONT_SIZE),
            Length::Auto | Length::Percent(_) => None,
        }
    }

    /// CSS form.
    pub fn css(self) -> String {
        match self {
            Length::Auto => "auto".into(),
            Length::Px(v) => format!("{v}px"),
            Length::Percent(v) => format!("{v}%"),
            Length::Rem(v) => format!("{v}rem"),
        }
    }
}

impl From<f32> for Length {
    fn from(v: f32) -> Self {
        Length::Px(v)
    }
}

impl From<i32> for Length {
    fn from(v: i32) -> Self {
        Length::Px(v as f32)
    }
}

/// The root font size in logical pixels.
pub const ROOT_FONT_SIZE: f32 = 16.0;

/// `16px` in `ui!` becomes `px(16.0)`.
pub fn px(v: f32) -> Length {
    Length::Px(v)
}

/// `50%` in `ui!` becomes `pct(50.0)`.
pub fn pct(v: f32) -> Length {
    Length::Percent(v)
}

/// `1.5rem` in `ui!` becomes `rem(1.5)`.
pub fn rem(v: f32) -> Length {
    Length::Rem(v)
}

/// How a container places its children.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Layout {
    /// Top to bottom (`v_stack`).
    #[default]
    Column,
    /// Left to right (`h_stack`).
    Row,
}

/// Cross-axis and main-axis alignment.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Align {
    /// Children take the full cross size (cross axis only).
    #[default]
    Stretch,
    /// At the start.
    Start,
    /// Centered.
    Center,
    /// At the end.
    End,
    /// Space between children (main axis only).
    SpaceBetween,
}

impl Align {
    fn css(self, main: bool) -> &'static str {
        match self {
            Align::Stretch if main => "flex-start",
            Align::Stretch => "stretch",
            Align::Start => "flex-start",
            Align::Center => "center",
            Align::End => "flex-end",
            Align::SpaceBetween if main => "space-between",
            Align::SpaceBetween => "stretch",
        }
    }
}

/// One style property and its value.
#[derive(Clone, PartialEq, Debug)]
pub enum StyleProp {
    /// Width.
    Width(Length),
    /// Height.
    Height(Length),
    /// Minimum width.
    MinWidth(Length),
    /// Maximum width.
    MaxWidth(Length),
    /// Padding on all sides.
    Padding(Length),
    /// Horizontal padding.
    PaddingX(Length),
    /// Vertical padding.
    PaddingY(Length),
    /// Space between children.
    Gap(Length),
    /// Child placement.
    Layout(Layout),
    /// Cross-axis alignment of children.
    Align(Align),
    /// Main-axis alignment of children.
    Justify(Align),
    /// Background color.
    Background(Color),
    /// Text color.
    Color(Color),
    /// Corner radius.
    Radius(Length),
    /// Border width.
    BorderWidth(Length),
    /// Border color.
    BorderColor(Color),
    /// Font size.
    FontSize(Length),
    /// Font weight (100–900).
    FontWeight(u16),
    /// Share of free main-axis space.
    Grow(f32),
}

/// A resolved style: every property has a value (defaults from the widget
/// kind and the theme, then the `ui!` properties in order).
#[derive(Clone, PartialEq, Debug)]
pub struct Style {
    /// Width.
    pub width: Length,
    /// Height.
    pub height: Length,
    /// Minimum width.
    pub min_width: Length,
    /// Maximum width.
    pub max_width: Length,
    /// Padding (left/right).
    pub padding_x: Length,
    /// Padding (top/bottom).
    pub padding_y: Length,
    /// Space between children.
    pub gap: Length,
    /// Child placement.
    pub layout: Layout,
    /// Cross-axis alignment.
    pub align: Align,
    /// Main-axis alignment.
    pub justify: Align,
    /// Background.
    pub background: Color,
    /// Text color.
    pub color: Color,
    /// Corner radius.
    pub radius: Length,
    /// Border width.
    pub border_width: Length,
    /// Border color.
    pub border_color: Color,
    /// Font size.
    pub font_size: Length,
    /// Font weight.
    pub font_weight: u16,
    /// Flex grow.
    pub grow: f32,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            width: Length::Auto,
            height: Length::Auto,
            min_width: Length::Auto,
            max_width: Length::Auto,
            padding_x: Length::Px(0.0),
            padding_y: Length::Px(0.0),
            gap: Length::Px(0.0),
            layout: Layout::Column,
            align: Align::Stretch,
            justify: Align::Start,
            background: Color::TRANSPARENT,
            color: Theme::LIGHT.text,
            radius: Length::Px(0.0),
            border_width: Length::Px(0.0),
            border_color: Color::TRANSPARENT,
            font_size: Length::Px(ROOT_FONT_SIZE),
            font_weight: 400,
            grow: 0.0,
        }
    }
}

impl Style {
    /// Applies one property.
    pub fn apply(&mut self, prop: &StyleProp) {
        match *prop {
            StyleProp::Width(v) => self.width = v,
            StyleProp::Height(v) => self.height = v,
            StyleProp::MinWidth(v) => self.min_width = v,
            StyleProp::MaxWidth(v) => self.max_width = v,
            StyleProp::Padding(v) => {
                self.padding_x = v;
                self.padding_y = v;
            }
            StyleProp::PaddingX(v) => self.padding_x = v,
            StyleProp::PaddingY(v) => self.padding_y = v,
            StyleProp::Gap(v) => self.gap = v,
            StyleProp::Layout(v) => self.layout = v,
            StyleProp::Align(v) => self.align = v,
            StyleProp::Justify(v) => self.justify = v,
            StyleProp::Background(v) => self.background = v,
            StyleProp::Color(v) => self.color = v,
            StyleProp::Radius(v) => self.radius = v,
            StyleProp::BorderWidth(v) => self.border_width = v,
            StyleProp::BorderColor(v) => self.border_color = v,
            StyleProp::FontSize(v) => self.font_size = v,
            StyleProp::FontWeight(v) => self.font_weight = v,
            StyleProp::Grow(v) => self.grow = v,
        }
    }

    /// Font size in logical pixels.
    pub fn font_px(&self) -> f32 {
        self.font_size.px().unwrap_or(ROOT_FONT_SIZE)
    }

    /// The CSS declarations of this style (`name: value;`).
    pub fn to_css(&self) -> String {
        let mut out = String::new();
        let mut decl = |name: &str, value: String| {
            out.push_str(name);
            out.push_str(": ");
            out.push_str(&value);
            out.push_str("; ");
        };
        decl("display", "flex".into());
        decl(
            "flex-direction",
            match self.layout {
                Layout::Column => "column",
                Layout::Row => "row",
            }
            .into(),
        );
        decl("align-items", self.align.css(false).into());
        decl("justify-content", self.justify.css(true).into());
        decl("box-sizing", "border-box".into());
        decl("width", self.width.css());
        decl("height", self.height.css());
        if self.min_width != Length::Auto {
            decl("min-width", self.min_width.css());
        }
        if self.max_width != Length::Auto {
            decl("max-width", self.max_width.css());
        }
        decl(
            "padding",
            format!("{} {}", self.padding_y.css(), self.padding_x.css()),
        );
        decl("gap", self.gap.css());
        decl("background", self.background.css());
        decl("color", self.color.css());
        decl("border-radius", self.radius.css());
        decl(
            "border",
            format!(
                "{} solid {}",
                self.border_width.css(),
                self.border_color.css()
            ),
        );
        decl("font-size", self.font_size.css());
        decl("font-weight", self.font_weight.to_string());
        decl("flex-grow", self.grow.to_string());
        out
    }
}

/// Theme tokens. Web pages also get them as CSS custom properties
/// (`--serge-primary`, …); native renderers use the colors directly.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Theme {
    /// Page background.
    pub background: Color,
    /// Card and input background.
    pub surface: Color,
    /// Body text.
    pub text: Color,
    /// Secondary text, placeholders.
    pub muted: Color,
    /// Accent (buttons, slider fill, focus).
    pub primary: Color,
    /// Text on the accent.
    pub on_primary: Color,
    /// Borders and tracks.
    pub border: Color,
}

impl Theme {
    /// The light theme.
    pub const LIGHT: Theme = Theme {
        background: Color::hex(0xf4f5f7),
        surface: Color::hex(0xffffff),
        text: Color::hex(0x1d1f24),
        muted: Color::hex(0x6b7280),
        primary: Color::hex(0x3b5bdb),
        on_primary: Color::hex(0xffffff),
        border: Color::hex(0xd6d9e0),
    };

    /// The dark theme.
    pub const DARK: Theme = Theme {
        background: Color::hex(0x15171c),
        surface: Color::hex(0x1f2229),
        text: Color::hex(0xe8eaef),
        muted: Color::hex(0x9aa1ad),
        primary: Color::hex(0x7b93ff),
        on_primary: Color::hex(0x0d1020),
        border: Color::hex(0x343844),
    };

    /// The tokens as CSS custom properties.
    pub fn css_variables(&self) -> String {
        let vars = [
            ("background", self.background),
            ("surface", self.surface),
            ("text", self.text),
            ("muted", self.muted),
            ("primary", self.primary),
            ("on-primary", self.on_primary),
            ("border", self.border),
        ];
        vars.iter()
            .map(|(n, c)| format!("--serge-{n}: {}; ", c.css()))
            .collect()
    }
}

impl fmt::Display for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.css())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_apply_in_order_and_render_css() {
        let mut s = Style::default();
        s.apply(&StyleProp::Padding(px(16.0)));
        s.apply(&StyleProp::PaddingX(px(4.0)));
        s.apply(&StyleProp::Layout(Layout::Row));
        assert_eq!((s.padding_x, s.padding_y), (px(4.0), px(16.0)));
        let css = s.to_css();
        assert!(css.contains("padding: 16px 4px;"), "{css}");
        assert!(css.contains("flex-direction: row;"), "{css}");
        assert_eq!(rem(2.0).px(), Some(32.0));
        assert_eq!(Color::hex(0x102030), Color::rgb(16, 32, 48));
    }
}
