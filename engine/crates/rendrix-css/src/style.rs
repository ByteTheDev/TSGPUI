//! The computed style struct: the single contract between the CSS engine, layout and paint.

use crate::color::Color;
use crate::values::Dim;
use std::collections::BTreeMap;
use std::sync::Arc;

macro_rules! keyword_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $kw:literal),+ $(,)? } default $def:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }
        impl Default for $name { fn default() -> Self { $name::$def } }
        impl $name {
            pub fn parse(s: &str) -> Option<Self> {
                match s.trim().to_ascii_lowercase().as_str() { $($kw => Some($name::$variant),)+ _ => None }
            }
        }
    };
}

keyword_enum!(Display {
    None => "none", Block => "block", Inline => "inline", InlineBlock => "inline-block",
    Flex => "flex", InlineFlex => "inline-flex", Grid => "grid", InlineGrid => "inline-grid", Contents => "contents",
} default Inline);
keyword_enum!(Position {
    Static => "static", Relative => "relative", Absolute => "absolute", Fixed => "fixed", Sticky => "sticky",
} default Static);
keyword_enum!(FlexDirection {
    Row => "row", RowReverse => "row-reverse", Column => "column", ColumnReverse => "column-reverse",
} default Row);
keyword_enum!(FlexWrap { NoWrap => "nowrap", Wrap => "wrap", WrapReverse => "wrap-reverse" } default NoWrap);
keyword_enum!(Align {
    Auto => "auto", Normal => "normal", Start => "start", End => "end", FlexStart => "flex-start", FlexEnd => "flex-end",
    Center => "center", Stretch => "stretch", Baseline => "baseline", SpaceBetween => "space-between",
    SpaceAround => "space-around", SpaceEvenly => "space-evenly",
} default Auto);
keyword_enum!(Overflow {
    Visible => "visible", Hidden => "hidden", Scroll => "scroll", Auto => "auto", Clip => "clip",
} default Visible);
keyword_enum!(TextAlign {
    Left => "left", Right => "right", Center => "center", Justify => "justify", Start => "start", End => "end",
} default Left);
keyword_enum!(WhiteSpace {
    Normal => "normal", NoWrap => "nowrap", Pre => "pre", PreWrap => "pre-wrap", PreLine => "pre-line",
} default Normal);
keyword_enum!(FontStyle { Normal => "normal", Italic => "italic", Oblique => "oblique" } default Normal);
keyword_enum!(BoxSizing { ContentBox => "content-box", BorderBox => "border-box" } default ContentBox);
keyword_enum!(Visibility { Visible => "visible", Hidden => "hidden", Collapse => "collapse" } default Visible);
keyword_enum!(PointerEvents { Auto => "auto", None => "none" } default Auto);
keyword_enum!(TextTransform {
    None => "none", Uppercase => "uppercase", Lowercase => "lowercase", Capitalize => "capitalize",
} default None);
keyword_enum!(BorderStyle {
    None => "none", Hidden => "hidden", Solid => "solid", Dashed => "dashed", Dotted => "dotted", Double => "double",
} default None);
keyword_enum!(ObjectFit {
    Fill => "fill", Contain => "contain", Cover => "cover", None => "none", ScaleDown => "scale-down",
} default Fill);
keyword_enum!(Cursor {
    Auto => "auto", Default => "default", Pointer => "pointer", Text => "text", Move => "move",
    NotAllowed => "not-allowed", Grab => "grab", Grabbing => "grabbing", Crosshair => "crosshair",
    Wait => "wait", Help => "help", ColResize => "col-resize", RowResize => "row-resize",
} default Auto);
keyword_enum!(GridAutoFlow {
    Row => "row", Column => "column", RowDense => "row dense", ColumnDense => "column dense",
} default Row);
keyword_enum!(BackgroundRepeat {
    Repeat => "repeat", NoRepeat => "no-repeat", RepeatX => "repeat-x", RepeatY => "repeat-y",
} default Repeat);
keyword_enum!(UserSelect { Auto => "auto", None => "none", Text => "text", All => "all" } default Auto);

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TextDecoration {
    pub underline: bool,
    pub line_through: bool,
    pub overline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    Normal,
    Number(f32),
    Px(f32),
}

impl LineHeight {
    pub fn resolve(self, font_size: f32) -> f32 {
        match self {
            LineHeight::Normal => font_size * 1.2,
            LineHeight::Number(n) => font_size * n,
            LineHeight::Px(p) => p,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrackSize {
    Auto,
    Px(f32),
    Percent(f32),
    Fr(f32),
    MinContent,
    MaxContent,
    MinMax(Box<TrackSize>, Box<TrackSize>),
    FitContent(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RepeatCount {
    Count(u16),
    AutoFill,
    AutoFit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrackItem {
    Single(TrackSize),
    Repeat(RepeatCount, Vec<TrackSize>),
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum GridLine {
    #[default]
    Auto,
    Line(i16),
    Span(u16),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
    pub inset: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GradientStop {
    pub color: Color,
    /// Position along the gradient line as a fraction 0..=1 (None → distributed automatically).
    pub pos: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackgroundImage {
    /// Angle in radians, CSS convention (0 = to top, clockwise).
    LinearGradient { angle: f32, stops: Vec<GradientStop> },
    RadialGradient { stops: Vec<GradientStop> },
    Url(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackgroundSize {
    Auto,
    Cover,
    Contain,
    Explicit(Dim, Dim),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransformOp {
    Translate(Dim, Dim),
    Scale(f32, f32),
    /// Radians.
    Rotate(f32),
    Skew(f32, f32),
    Matrix([f32; 6]),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimingFunction {
    pub kind: TimingKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TimingKind {
    CubicBezier(f32, f32, f32, f32),
    Steps(u32, bool),
    Linear,
}

impl TimingFunction {
    pub fn ease() -> Self {
        TimingFunction { kind: TimingKind::CubicBezier(0.25, 0.1, 0.25, 1.0) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    /// Property name or `all`.
    pub property: String,
    pub duration: f32,
    pub delay: f32,
    pub timing: TimingFunction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationDirection {
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillMode {
    None,
    Forwards,
    Backwards,
    Both,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnimationSpec {
    pub name: String,
    pub duration: f32,
    pub delay: f32,
    pub timing: TimingFunction,
    /// `f32::INFINITY` for `infinite`.
    pub iterations: f32,
    pub direction: AnimationDirection,
    pub fill: FillMode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    // box tree
    pub display: Display,
    pub position: Position,
    pub inset: [Dim; 4], // top right bottom left
    pub z_index: Option<i32>,
    pub float_none: bool,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    pub box_sizing: BoxSizing,
    pub visibility: Visibility,
    pub opacity: f32,
    pub pointer_events: PointerEvents,
    pub cursor: Cursor,
    pub user_select: UserSelect,

    // sizing
    pub width: Dim,
    pub height: Dim,
    pub min_width: Dim,
    pub min_height: Dim,
    pub max_width: Dim,
    pub max_height: Dim,
    pub aspect_ratio: Option<f32>,
    pub margin: [Dim; 4],
    pub padding: [Dim; 4],
    pub border_width: [f32; 4],
    pub border_style: [BorderStyle; 4],
    pub border_color: [Color; 4],
    /// Corner radii (top-left, top-right, bottom-right, bottom-left).
    pub border_radius: [Dim; 4],
    pub outline_width: f32,
    pub outline_color: Color,
    pub outline_offset: f32,

    // flex
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Dim,
    pub order: i32,
    pub align_items: Align,
    pub align_self: Align,
    pub align_content: Align,
    pub justify_content: Align,
    pub justify_items: Align,
    pub justify_self: Align,
    pub row_gap: Dim,
    pub column_gap: Dim,

    // grid
    pub grid_template_columns: Vec<TrackItem>,
    pub grid_template_rows: Vec<TrackItem>,
    pub grid_auto_columns: Vec<TrackSize>,
    pub grid_auto_rows: Vec<TrackSize>,
    pub grid_auto_flow: GridAutoFlow,
    pub grid_column: (GridLine, GridLine),
    pub grid_row: (GridLine, GridLine),

    // paint
    pub color: Color,
    pub background_color: Color,
    pub background_images: Vec<BackgroundImage>,
    pub background_size: BackgroundSize,
    pub background_repeat: BackgroundRepeat,
    pub box_shadow: Vec<Shadow>,
    pub transform: Vec<TransformOp>,
    pub transform_origin: (Dim, Dim),
    pub object_fit: ObjectFit,
    /// `content` for generated boxes (::before/::after).
    pub content: Option<String>,

    // text (inherited)
    pub font_family: Arc<[String]>,
    pub font_size: f32,
    pub font_weight: u16,
    pub font_style: FontStyle,
    pub line_height: LineHeight,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub text_align: TextAlign,
    pub text_decoration: TextDecoration,
    pub text_transform: TextTransform,
    pub white_space: WhiteSpace,
    pub text_overflow_ellipsis: bool,
    pub caret_color: Option<Color>,
    pub list_style_none: bool,

    // motion
    pub transitions: Vec<Transition>,
    pub animations: Vec<AnimationSpec>,

    /// Custom properties (inherited). Values are the raw token text.
    pub custom: Arc<BTreeMap<String, String>>,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        ComputedStyle {
            display: Display::Inline,
            position: Position::Static,
            inset: [Dim::Auto; 4],
            z_index: None,
            float_none: true,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            box_sizing: BoxSizing::ContentBox,
            visibility: Visibility::Visible,
            opacity: 1.0,
            pointer_events: PointerEvents::Auto,
            cursor: Cursor::Auto,
            user_select: UserSelect::Auto,
            width: Dim::Auto,
            height: Dim::Auto,
            min_width: Dim::Auto,
            min_height: Dim::Auto,
            max_width: Dim::Auto,
            max_height: Dim::Auto,
            aspect_ratio: None,
            margin: [Dim::Px(0.0); 4],
            padding: [Dim::Px(0.0); 4],
            border_width: [0.0; 4],
            border_style: [BorderStyle::None; 4],
            border_color: [Color::BLACK; 4],
            border_radius: [Dim::Px(0.0); 4],
            outline_width: 0.0,
            outline_color: Color::BLACK,
            outline_offset: 0.0,
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::NoWrap,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Dim::Auto,
            order: 0,
            align_items: Align::Normal,
            align_self: Align::Auto,
            align_content: Align::Normal,
            justify_content: Align::Normal,
            justify_items: Align::Normal,
            justify_self: Align::Auto,
            row_gap: Dim::Px(0.0),
            column_gap: Dim::Px(0.0),
            grid_template_columns: Vec::new(),
            grid_template_rows: Vec::new(),
            grid_auto_columns: Vec::new(),
            grid_auto_rows: Vec::new(),
            grid_auto_flow: GridAutoFlow::Row,
            grid_column: (GridLine::Auto, GridLine::Auto),
            grid_row: (GridLine::Auto, GridLine::Auto),
            color: Color::BLACK,
            background_color: Color::TRANSPARENT,
            background_images: Vec::new(),
            background_size: BackgroundSize::Auto,
            background_repeat: BackgroundRepeat::Repeat,
            box_shadow: Vec::new(),
            transform: Vec::new(),
            transform_origin: (Dim::Percent(50.0), Dim::Percent(50.0)),
            object_fit: ObjectFit::Fill,
            content: None,
            font_family: Arc::from(vec!["sans-serif".to_string()]),
            font_size: 16.0,
            font_weight: 400,
            font_style: FontStyle::Normal,
            line_height: LineHeight::Normal,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_align: TextAlign::Start,
            text_decoration: TextDecoration::default(),
            text_transform: TextTransform::None,
            white_space: WhiteSpace::Normal,
            text_overflow_ellipsis: false,
            caret_color: None,
            list_style_none: false,
            transitions: Vec::new(),
            animations: Vec::new(),
            custom: Arc::new(BTreeMap::new()),
        }
    }
}

impl ComputedStyle {
    /// Start a child's style: inherited properties carry over, everything else is initial.
    pub fn inherit_from(parent: &ComputedStyle) -> ComputedStyle {
        ComputedStyle {
            color: parent.color,
            font_family: parent.font_family.clone(),
            font_size: parent.font_size,
            font_weight: parent.font_weight,
            font_style: parent.font_style,
            line_height: parent.line_height,
            letter_spacing: parent.letter_spacing,
            word_spacing: parent.word_spacing,
            text_align: parent.text_align,
            text_transform: parent.text_transform,
            white_space: parent.white_space,
            visibility: parent.visibility,
            cursor: parent.cursor,
            caret_color: parent.caret_color,
            list_style_none: parent.list_style_none,
            pointer_events: parent.pointer_events,
            user_select: parent.user_select,
            custom: parent.custom.clone(),
            ..ComputedStyle::default()
        }
    }

    pub fn is_inline_level(&self) -> bool {
        matches!(self.display, Display::Inline)
    }

    pub fn is_out_of_flow(&self) -> bool {
        matches!(self.position, Position::Absolute | Position::Fixed)
    }

    pub fn has_transform(&self) -> bool {
        !self.transform.is_empty()
    }

    pub fn is_scroll_container(&self) -> bool {
        matches!(self.overflow_x, Overflow::Scroll | Overflow::Auto | Overflow::Hidden | Overflow::Clip)
            || matches!(self.overflow_y, Overflow::Scroll | Overflow::Auto | Overflow::Hidden | Overflow::Clip)
    }

    pub fn clips_overflow(&self) -> bool {
        !matches!(self.overflow_x, Overflow::Visible) || !matches!(self.overflow_y, Overflow::Visible)
    }

    /// True when this box establishes a stacking context.
    pub fn creates_stacking_context(&self) -> bool {
        self.opacity < 1.0
            || self.has_transform()
            || (self.position != Position::Static && self.z_index.is_some())
            || matches!(self.position, Position::Fixed | Position::Sticky)
    }
}
