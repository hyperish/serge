//! Cross-platform design tokens, lengths, layout directions, and theme support.

/// CSS/Logical pixel length unit shared across Web DOM and Native GPU/CPU layout.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
    /// Fixed logical pixels (e.g. `16px`).
    Px(f32),
    /// Percentage of parent dimension (e.g. `50%`).
    Percent(f32),
    /// Automatic intrinsic sizing.
    #[default]
    Auto,
}

impl Length {
    /// Resolves this length to pixels, falling back to `default_px` for `Auto` or `Percent`.
    #[must_use]
    pub fn to_px(self, default_px: f32) -> f32 {
        match self {
            Self::Px(v) => v,
            Self::Percent(p) => default_px * (p / 100.0),
            Self::Auto => default_px,
        }
    }
}

impl From<f32> for Length {
    fn from(value: f32) -> Self {
        Self::Px(value)
    }
}

impl From<i32> for Length {
    fn from(value: i32) -> Self {
        Self::Px(value as f32)
    }
}

/// Primary child stacking axis for containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutDirection {
    /// Vertical top-to-bottom stack.
    #[default]
    VStack,
    /// Horizontal left-to-right stack.
    HStack,
    /// Overlapping depth stack.
    ZStack,
}

/// Vertical stack layout token (`layout: v_stack`).
#[allow(non_upper_case_globals)]
pub const v_stack: LayoutDirection = LayoutDirection::VStack;

/// Horizontal stack layout token (`layout: h_stack`).
#[allow(non_upper_case_globals)]
pub const h_stack: LayoutDirection = LayoutDirection::HStack;

/// Depth-overlapping stack layout token (`layout: z_stack`).
#[allow(non_upper_case_globals)]
pub const z_stack: LayoutDirection = LayoutDirection::ZStack;

/// 8-bit sRGBA color value shared across Web CSS tokens and Native GPU/CPU rasterizers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    /// Red channel (`0..=255`).
    pub r: u8,
    /// Green channel (`0..=255`).
    pub g: u8,
    /// Blue channel (`0..=255`).
    pub b: u8,
    /// Alpha channel (`0..=255`).
    pub a: u8,
}

impl Color {
    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    /// Opaque white.
    pub const WHITE: Self = Self::rgba(255, 255, 255, 255);
    /// Opaque black.
    pub const BLACK: Self = Self::rgba(0, 0, 0, 255);
    /// Dark canvas background (`#0F172A`).
    pub const SLATE_900: Self = Self::rgba(15, 23, 42, 255);
    /// Dark card surface (`#1E293B`).
    pub const SLATE_800: Self = Self::rgba(30, 41, 59, 255);
    /// Light card background (`#F8FAFC`).
    pub const SLATE_50: Self = Self::rgba(248, 250, 252, 255);
    /// Crisp light border (`#E2E8F0`).
    pub const SLATE_200: Self = Self::rgba(226, 232, 240, 255);
    /// Indigo accent (`#6366F1`).
    pub const INDIGO_500: Self = Self::rgba(99, 102, 241, 255);
    /// Emerald telemetry trace (`#10B981`).
    pub const EMERALD_500: Self = Self::rgba(16, 185, 129, 255);

    /// Constructs an opaque RGB color.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Constructs an sRGBA color.
    #[must_use]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Returns `[r, g, b, a]` bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Formats this color as a CSS `rgba(...)` string.
    #[must_use]
    pub fn to_css(self) -> String {
        format!(
            "rgba({}, {}, {}, {:.3})",
            self.r,
            self.g,
            self.b,
            f32::from(self.a) / 255.0
        )
    }
}

/// Active color scheme mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    /// Light color palette.
    Light,
    /// Dark color palette.
    #[default]
    Dark,
}

/// Semantic color tokens mapped identically to CSS custom properties and Native GPU/CPU shaders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorTokens {
    /// Viewport background color.
    pub background: Color,
    /// Card and container surface fill.
    pub surface: Color,
    /// Primary foreground text color.
    pub text: Color,
    /// Primary interactive accent color.
    pub accent: Color,
    /// Container outline border color.
    pub border: Color,
}

impl ColorTokens {
    /// Built-in dark mode palette.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            background: Color::SLATE_900,
            surface: Color::SLATE_800,
            text: Color::WHITE,
            accent: Color::INDIGO_500,
            border: Color::rgba(71, 85, 105, 255),
        }
    }

    /// Built-in light mode palette.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            background: Color::SLATE_50,
            surface: Color::WHITE,
            text: Color::SLATE_900,
            accent: Color::INDIGO_500,
            border: Color::SLATE_200,
        }
    }

    /// Returns the palette for `mode`.
    #[must_use]
    pub const fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
        }
    }
}

impl Default for ColorTokens {
    fn default() -> Self {
        Self::dark()
    }
}
