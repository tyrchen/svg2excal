//! Typed Excalidraw v2 target model, validation, and stable serialization.

use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::{
    ConversionLimits,
    error::{ConversionError, LimitResource},
    identity::{Identity, file_id as deterministic_file_id},
};

const TARGET_SOURCE: &str = "https://github.com/tyrchen/svg2excal";
const MAX_ROUGH_SEED: u32 = i32::MAX as u32;

/// Content-addressed Excalidraw binary-file identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FileId(String);

impl FileId {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    /// Identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Deterministic Excalidraw group identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GroupId(String);

impl GroupId {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    /// Identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct ElementId(String);

impl ElementId {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Finite(f64);

impl Finite {
    pub(crate) fn coordinate(value: f64, limit: f64) -> Result<Self, ConversionError> {
        let value = canonical_coordinate(value);
        if !value.is_finite() || value.abs() > limit {
            return Err(ConversionError::GeometryOverflow);
        }
        Ok(Self(value))
    }

    pub(crate) fn length(value: f64, limit: f64) -> Result<Self, ConversionError> {
        let value = canonical_coordinate(value);
        if !value.is_finite() || value < 0.0 || value > limit {
            return Err(ConversionError::GeometryOverflow);
        }
        Ok(Self(value))
    }

    pub(crate) fn angle(value: f64) -> Result<Self, ConversionError> {
        let value = canonical_angle(value);
        if !value.is_finite() {
            return Err(ConversionError::GeometryOverflow);
        }
        Ok(Self(value))
    }
}

/// Canonical Excalidraw color.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct ExcalidrawColor(String);

impl ExcalidrawColor {
    pub(crate) fn transparent() -> Self {
        Self("transparent".to_owned())
    }

    pub(crate) fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self(format!("#{red:02x}{green:02x}{blue:02x}"))
    }

    fn is_valid(&self) -> bool {
        if self.0 == "transparent" {
            return true;
        }
        self.0.len() == 7
            && self.0.starts_with('#')
            && self
                .0
                .bytes()
                .skip(1)
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FillStyle {
    Solid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum StrokeStyle {
    Solid,
    Dashed,
    Dotted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Roundness {
    #[serde(rename = "type")]
    kind: u8,
    value: Finite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BoundElement {
    id: ElementId,
    #[serde(rename = "type")]
    kind: BoundElementKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum BoundElementKind {
    Arrow,
    Text,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Svg2ExcalCustomDataEnvelope {
    svg2excal: Svg2ExcalCustomData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Svg2ExcalCustomData {
    version: u8,
    source_key: String,
    source_tag: String,
    mapping: String,
    diagnostic_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ElementBase {
    id: ElementId,
    x: Finite,
    y: Finite,
    width: Finite,
    height: Finite,
    angle: Finite,
    stroke_color: ExcalidrawColor,
    background_color: ExcalidrawColor,
    fill_style: FillStyle,
    stroke_width: Finite,
    stroke_style: StrokeStyle,
    roundness: Option<Roundness>,
    roughness: u8,
    opacity: u8,
    group_ids: Vec<GroupId>,
    frame_id: Option<ElementId>,
    index: Option<String>,
    seed: u32,
    version: NonZeroU32,
    version_nonce: u32,
    is_deleted: bool,
    bound_elements: Option<Vec<BoundElement>>,
    updated: u64,
    link: Option<String>,
    locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    custom_data: Option<Svg2ExcalCustomDataEnvelope>,
}

#[derive(Debug, Clone)]
pub(crate) struct ElementStyle {
    pub(crate) stroke_color: ExcalidrawColor,
    pub(crate) background_color: ExcalidrawColor,
    pub(crate) stroke_width: f64,
    pub(crate) stroke_style: StrokeStyle,
    pub(crate) opacity: u8,
    pub(crate) roundness: Option<f64>,
}

impl ElementBase {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        id: String,
        identity: Identity,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        angle: f64,
        style: ElementStyle,
        limits: &ConversionLimits,
    ) -> Result<Self, ConversionError> {
        let roundness = style
            .roundness
            .map(|value| {
                Ok(Roundness {
                    kind: 3,
                    value: Finite::length(value, limits.max_element_extent())?,
                })
            })
            .transpose()?;
        Ok(Self {
            id: ElementId::new(id),
            x: Finite::coordinate(x, limits.max_coordinate())?,
            y: Finite::coordinate(y, limits.max_coordinate())?,
            width: Finite::length(width, limits.max_element_extent())?,
            height: Finite::length(height, limits.max_element_extent())?,
            angle: Finite::angle(angle)?,
            stroke_color: style.stroke_color,
            background_color: style.background_color,
            fill_style: FillStyle::Solid,
            stroke_width: Finite::length(style.stroke_width, limits.max_element_extent())?,
            stroke_style: style.stroke_style,
            roundness,
            roughness: 0,
            opacity: style.opacity,
            group_ids: Vec::new(),
            frame_id: None,
            index: None,
            seed: identity.seed,
            version: NonZeroU32::MIN,
            version_nonce: identity.nonce,
            is_deleted: false,
            bound_elements: None,
            updated: 1,
            link: None,
            locked: false,
            custom_data: None,
        })
    }

    pub(crate) fn id(&self) -> &ElementId {
        &self.id
    }

    pub(crate) fn set_provenance(
        &mut self,
        source_order: u32,
        source_tag: &str,
        mapping: &str,
        diagnostic_codes: Vec<String>,
    ) {
        self.custom_data = Some(Svg2ExcalCustomDataEnvelope {
            svg2excal: Svg2ExcalCustomData {
                version: 1,
                source_key: format!("source-{source_order}"),
                source_tag: source_tag.to_owned(),
                mapping: mapping.to_owned(),
                diagnostic_codes,
            },
        });
    }

    pub(crate) fn set_group_ids(&mut self, group_ids: Vec<GroupId>) {
        self.group_ids = group_ids;
    }

    fn validate(&self, limits: &ConversionLimits) -> bool {
        !self.id.0.is_empty()
            && self.id.0.len() <= 64
            && self.x.0.is_finite()
            && self.x.0.abs() <= limits.max_coordinate()
            && self.y.0.is_finite()
            && self.y.0.abs() <= limits.max_coordinate()
            && (0.0..=limits.max_element_extent()).contains(&self.width.0)
            && (0.0..=limits.max_element_extent()).contains(&self.height.0)
            && self.angle.0.is_finite()
            && self.stroke_color.is_valid()
            && self.background_color.is_valid()
            && self.stroke_width.0.is_finite()
            && self.stroke_width.0 >= 0.0
            && self
                .roundness
                .as_ref()
                .is_none_or(|roundness| roundness.kind == 3 && roundness.value.0 > 0.0)
            && self.opacity <= 100
            && self.roughness == 0
            && (1..=MAX_ROUGH_SEED).contains(&self.seed)
            && self.version == NonZeroU32::MIN
            && self.version_nonce <= MAX_ROUGH_SEED
            && !self.is_deleted
            && self.frame_id.is_none()
            && self.index.is_none()
            && self.bound_elements.is_none()
            && self.updated == 1
            && self.link.is_none()
            && !self.locked
            && self.group_ids.iter().collect::<BTreeSet<_>>().len() == self.group_ids.len()
            && self
                .custom_data
                .as_ref()
                .is_none_or(Svg2ExcalCustomDataEnvelope::is_valid)
            && self.group_ids.len() <= 64
    }
}

impl Svg2ExcalCustomDataEnvelope {
    fn is_valid(&self) -> bool {
        let data = &self.svg2excal;
        data.version == 1
            && !data.source_key.is_empty()
            && data.source_key.len() <= 64
            && data
                .source_key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            && !data.source_tag.is_empty()
            && data.source_tag.len() <= 32
            && data
                .source_tag
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
            && matches!(
                data.mapping.as_str(),
                "native" | "approximate" | "decomposed" | "fallback"
            )
            && data.diagnostic_codes.len() <= 16
            && data.diagnostic_codes.iter().all(|code| {
                !code.is_empty()
                    && code.len() <= 64
                    && code
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
            })
    }
}

/// Shape-only element payload used by rectangles, diamonds, and ellipses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeElement {
    #[serde(flatten)]
    base: ElementBase,
}

/// Local point relative to an element's `x`/`y` origin.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LocalPoint([Finite; 2]);

impl LocalPoint {
    pub(crate) fn new(x: f64, y: f64, limits: &ConversionLimits) -> Result<Self, ConversionError> {
        Ok(Self([
            Finite::coordinate(x, limits.max_coordinate())?,
            Finite::coordinate(y, limits.max_coordinate())?,
        ]))
    }

    fn is_origin(&self) -> bool {
        self.0.iter().all(|value| value.0 == 0.0)
    }

    fn values(&self) -> (f64, f64) {
        let [x, y] = self.0;
        (x.0, y.0)
    }
}

/// Target arrowhead name.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrowhead {
    /// Standard arrow.
    Arrow,
    /// Triangle.
    Triangle,
    /// Bar.
    Bar,
    /// Filled circle/dot.
    Dot,
    /// Diamond.
    Diamond,
    /// Circle outline.
    CircleOutline,
    /// Diamond outline.
    DiamondOutline,
}

/// Excalidraw line payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineElement {
    #[serde(flatten)]
    base: ElementBase,
    points: Vec<LocalPoint>,
    start_binding: Option<FixedPointBinding>,
    end_binding: Option<FixedPointBinding>,
    start_arrowhead: Option<Arrowhead>,
    end_arrowhead: Option<Arrowhead>,
    polygon: bool,
}

/// Excalidraw arrow payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrowElement {
    #[serde(flatten)]
    base: ElementBase,
    points: Vec<LocalPoint>,
    start_binding: Option<FixedPointBinding>,
    end_binding: Option<FixedPointBinding>,
    start_arrowhead: Option<Arrowhead>,
    end_arrowhead: Option<Arrowhead>,
    polygon: bool,
    elbowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixedPointBinding {
    element_id: ElementId,
    focus: Finite,
    gap: Finite,
    fixed_point: Option<[Finite; 2]>,
}

/// Excalidraw text payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextElement {
    #[serde(flatten)]
    base: ElementBase,
    font_size: Finite,
    font_family: u8,
    text: String,
    original_text: String,
    text_align: TextAlign,
    vertical_align: VerticalAlign,
    container_id: Option<ElementId>,
    auto_resize: bool,
    line_height: Finite,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum VerticalAlign {
    Top,
}

/// Excalidraw image payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageElement {
    #[serde(flatten)]
    base: ElementBase,
    file_id: FileId,
    status: ImageStatus,
    scale: [Finite; 2],
    crop: Option<ImageCrop>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ImageStatus {
    Saved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImageCrop {
    x: Finite,
    y: Finite,
    width: Finite,
    height: Finite,
    natural_width: Finite,
    natural_height: Finite,
}

/// Complete supported Excalidraw element union.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ExcalidrawElement {
    /// Native rectangle.
    Rectangle(ShapeElement),
    /// Native target diamond.
    Diamond(ShapeElement),
    /// Native ellipse.
    Ellipse(ShapeElement),
    /// Markerless line or polygon.
    Line(LineElement),
    /// Explicit source connector with recognized endpoint markers.
    Arrow(ArrowElement),
    /// Editable single-style text.
    Text(TextElement),
    /// Existing raster image or minimal raster fallback island.
    Image(ImageElement),
}

impl ExcalidrawElement {
    pub(crate) fn rectangle(base: ElementBase) -> Self {
        Self::Rectangle(ShapeElement { base })
    }

    pub(crate) fn diamond(base: ElementBase) -> Self {
        Self::Diamond(ShapeElement { base })
    }

    pub(crate) fn ellipse(base: ElementBase) -> Self {
        Self::Ellipse(ShapeElement { base })
    }

    pub(crate) fn line(base: ElementBase, points: Vec<LocalPoint>, polygon: bool) -> Self {
        Self::Line(LineElement {
            base,
            points,
            start_binding: None,
            end_binding: None,
            start_arrowhead: None,
            end_arrowhead: None,
            polygon,
        })
    }

    pub(crate) fn arrow(
        base: ElementBase,
        points: Vec<LocalPoint>,
        start_arrowhead: Option<Arrowhead>,
        end_arrowhead: Option<Arrowhead>,
    ) -> Self {
        Self::Arrow(ArrowElement {
            base,
            points,
            start_binding: None,
            end_binding: None,
            start_arrowhead,
            end_arrowhead,
            polygon: false,
            elbowed: false,
        })
    }

    pub(crate) fn set_group_ids(&mut self, group_ids: Vec<GroupId>) {
        match self {
            Self::Rectangle(element) | Self::Diamond(element) | Self::Ellipse(element) => {
                element.base.set_group_ids(group_ids);
            }
            Self::Line(element) => element.base.set_group_ids(group_ids),
            Self::Arrow(element) => element.base.set_group_ids(group_ids),
            Self::Text(element) => element.base.set_group_ids(group_ids),
            Self::Image(element) => element.base.set_group_ids(group_ids),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn text(
        base: ElementBase,
        font_size: Finite,
        text: String,
        align: TextAlign,
        line_height: Finite,
    ) -> Self {
        Self::Text(TextElement {
            base,
            font_size,
            font_family: 2,
            original_text: text.clone(),
            text,
            text_align: align,
            vertical_align: VerticalAlign::Top,
            container_id: None,
            auto_resize: true,
            line_height,
        })
    }

    pub(crate) fn image(base: ElementBase, file_id: FileId) -> Self {
        Self::Image(ImageElement {
            base,
            file_id,
            status: ImageStatus::Saved,
            scale: [Finite(1.0), Finite(1.0)],
            crop: None,
        })
    }

    fn base(&self) -> &ElementBase {
        match self {
            Self::Rectangle(element) | Self::Diamond(element) | Self::Ellipse(element) => {
                &element.base
            }
            Self::Line(element) => &element.base,
            Self::Arrow(element) => &element.base,
            Self::Text(element) => &element.base,
            Self::Image(element) => &element.base,
        }
    }

    fn points(&self) -> &[LocalPoint] {
        match self {
            Self::Line(element) => &element.points,
            Self::Arrow(element) => &element.points,
            _ => &[],
        }
    }

    /// Stable target element ID.
    #[must_use]
    pub fn id(&self) -> &str {
        self.base().id.as_str()
    }

    /// Target element type string.
    #[must_use]
    pub const fn element_type(&self) -> &'static str {
        match self {
            Self::Rectangle(_) => "rectangle",
            Self::Diamond(_) => "diamond",
            Self::Ellipse(_) => "ellipse",
            Self::Line(_) => "line",
            Self::Arrow(_) => "arrow",
            Self::Text(_) => "text",
            Self::Image(_) => "image",
        }
    }
}

/// Supported embedded output MIME type.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageMimeType {
    /// Canonical PNG.
    #[serde(rename = "image/png")]
    Png,
}

/// Embedded Excalidraw binary file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryFile {
    mime_type: ImageMimeType,
    id: FileId,
    data_url: String,
    created: u64,
    version: NonZeroU32,
}

impl BinaryFile {
    pub(crate) fn png(id: FileId, data_url: String) -> Self {
        Self {
            mime_type: ImageMimeType::Png,
            id,
            data_url,
            created: 1,
            version: NonZeroU32::MIN,
        }
    }

    /// Content-addressed file ID.
    #[must_use]
    pub const fn id(&self) -> &FileId {
        &self.id
    }

    /// Canonical PNG data URL.
    #[must_use]
    pub fn data_url(&self) -> &str {
        &self.data_url
    }
}

/// Minimal stable Excalidraw application state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExcalidrawAppState {
    view_background_color: String,
    grid_mode_enabled: bool,
}

impl Default for ExcalidrawAppState {
    fn default() -> Self {
        Self {
            view_background_color: "#ffffff".to_owned(),
            grid_mode_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ExcalidrawDocumentType {
    Excalidraw,
}

/// Typed Excalidraw v2 document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExcalidrawDocument {
    #[serde(rename = "type")]
    kind: ExcalidrawDocumentType,
    version: u32,
    source: String,
    elements: Vec<ExcalidrawElement>,
    app_state: ExcalidrawAppState,
    files: BTreeMap<FileId, BinaryFile>,
}

impl ExcalidrawDocument {
    pub(crate) fn new(
        elements: Vec<ExcalidrawElement>,
        files: BTreeMap<FileId, BinaryFile>,
    ) -> Self {
        Self {
            kind: ExcalidrawDocumentType::Excalidraw,
            version: 2,
            source: TARGET_SOURCE.to_owned(),
            elements,
            app_state: ExcalidrawAppState::default(),
            files,
        }
    }

    /// Elements in authoritative SVG paint order.
    #[must_use]
    pub fn elements(&self) -> &[ExcalidrawElement] {
        &self.elements
    }

    /// Content-addressed embedded files.
    #[must_use]
    pub const fn files(&self) -> &BTreeMap<FileId, BinaryFile> {
        &self.files
    }

    /// Validates every target invariant under the supplied hard limits.
    ///
    /// # Errors
    ///
    /// Returns [`ConversionError::InvalidGeneratedDocument`] or a limit error.
    pub fn validate(&self, limits: &ConversionLimits) -> Result<(), ConversionError> {
        if self.version != 2
            || self.source != TARGET_SOURCE
            || self.app_state.view_background_color != "#ffffff"
            || self.app_state.grid_mode_enabled
        {
            return invalid("document envelope");
        }
        if self.elements.len() > limits.max_target_elements() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::TargetElements,
                limit: usize_to_u64(limits.max_target_elements()),
            });
        }
        let referenced_files = validate_elements(&self.elements, limits)?;
        validate_files(&self.files, &referenced_files, limits)?;
        Ok(())
    }

    /// Serializes deterministic two-space JSON with one terminal newline.
    ///
    /// # Errors
    ///
    /// Returns an error when validation fails, serialization fails, or the JSON cap is exceeded.
    pub fn to_pretty_json(&self) -> Result<String, ConversionError> {
        self.to_pretty_json_with_limits(&ConversionLimits::default())
    }

    /// Serializes using caller-selected hard limits.
    ///
    /// # Errors
    ///
    /// Returns an error when validation fails, serialization fails, or the JSON cap is exceeded.
    pub fn to_pretty_json_with_limits(
        &self,
        limits: &ConversionLimits,
    ) -> Result<String, ConversionError> {
        self.validate(limits)?;
        let mut output = serde_json::to_string_pretty(self).map_err(|_| {
            ConversionError::InvalidGeneratedDocument {
                category: "JSON serialization",
            }
        })?;
        output.push('\n');
        if output.len() > limits.max_serialized_json_bytes() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::SerializedJson,
                limit: usize_to_u64(limits.max_serialized_json_bytes()),
            });
        }
        Ok(output)
    }

    /// Parses and immediately validates typed Excalidraw JSON.
    ///
    /// # Errors
    ///
    /// Returns an error for oversized, malformed, or invariant-violating JSON.
    pub fn from_json(input: &[u8], limits: &ConversionLimits) -> Result<Self, ConversionError> {
        if input.len() > limits.max_serialized_json_bytes() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::SerializedJson,
                limit: usize_to_u64(limits.max_serialized_json_bytes()),
            });
        }
        let document: Self = serde_json::from_slice(input).map_err(|_| {
            ConversionError::InvalidGeneratedDocument {
                category: "typed JSON parse",
            }
        })?;
        document.validate(limits)?;
        Ok(document)
    }
}

fn validate_elements(
    elements: &[ExcalidrawElement],
    limits: &ConversionLimits,
) -> Result<BTreeSet<FileId>, ConversionError> {
    let mut element_ids = BTreeSet::new();
    let mut group_ids = BTreeMap::<String, GroupStats>::new();
    let mut referenced_files = BTreeSet::new();
    let mut total_points = 0_usize;
    for (element_index, element) in elements.iter().enumerate() {
        let base = element.base();
        if !base.validate(limits) || !element_ids.insert(base.id.0.clone()) {
            return invalid("element base or duplicate ID");
        }
        for (group_index, group_id) in base.group_ids.iter().enumerate() {
            if group_id.0.is_empty() || group_id.0.len() > 64 {
                return invalid("group ID");
            }
            let parent = base
                .group_ids
                .get(group_index.saturating_add(1))
                .map(|group| group.0.clone());
            if let Some(stats) = group_ids.get_mut(&group_id.0) {
                if stats.parent != parent {
                    return invalid("inconsistent group nesting");
                }
                stats.last = element_index;
                stats.count = stats.count.saturating_add(1);
            } else {
                group_ids.insert(
                    group_id.0.clone(),
                    GroupStats {
                        first: element_index,
                        last: element_index,
                        count: 1,
                        parent,
                    },
                );
            }
        }
        total_points = total_points.checked_add(element.points().len()).ok_or(
            ConversionError::LimitExceeded {
                resource: LimitResource::TargetPoints,
                limit: usize_to_u64(limits.max_target_points()),
            },
        )?;
        validate_element_payload(element, &mut referenced_files)?;
    }
    if group_ids.values().any(|stats| {
        stats.count < 2 || stats.last.saturating_sub(stats.first).saturating_add(1) != stats.count
    }) {
        return invalid("singleton or non-contiguous group");
    }
    if total_points > limits.max_target_points() {
        return Err(ConversionError::LimitExceeded {
            resource: LimitResource::TargetPoints,
            limit: usize_to_u64(limits.max_target_points()),
        });
    }
    Ok(referenced_files)
}

#[derive(Debug)]
struct GroupStats {
    first: usize,
    last: usize,
    count: usize,
    parent: Option<String>,
}

fn validate_element_payload(
    element: &ExcalidrawElement,
    referenced_files: &mut BTreeSet<FileId>,
) -> Result<(), ConversionError> {
    match element {
        ExcalidrawElement::Line(line) => {
            if line.start_binding.is_some()
                || line.end_binding.is_some()
                || line.start_arrowhead.is_some()
                || line.end_arrowhead.is_some()
                || line.base.roundness.is_some()
            {
                return invalid("line bindings, arrowheads, or roundness");
            }
            validate_linear(&line.base, &line.points, line.polygon)
        }
        ExcalidrawElement::Arrow(arrow) => {
            if arrow.polygon
                || arrow.elbowed
                || arrow.start_binding.is_some()
                || arrow.end_binding.is_some()
                || arrow.base.roundness.is_some()
                || (arrow.start_arrowhead.is_none() && arrow.end_arrowhead.is_none())
            {
                return invalid("arrow flags, bindings, or arrowheads");
            }
            validate_linear(&arrow.base, &arrow.points, false)
        }
        ExcalidrawElement::Text(text) => {
            if text.text.is_empty()
                || text.text.len() > 1024 * 1024
                || text.original_text != text.text
                || text.font_family != 2
                || text.container_id.is_some()
                || !text.auto_resize
                || text.font_size.0 <= 0.0
                || text.line_height.0 <= 0.0
                || text.base.roundness.is_some()
            {
                return invalid("text fields");
            }
            Ok(())
        }
        ExcalidrawElement::Image(image) => {
            if image.crop.is_some()
                || image
                    .scale
                    .iter()
                    .any(|value| value.0.to_bits() != 1.0_f64.to_bits())
                || image.base.roundness.is_some()
            {
                return invalid("image fields");
            }
            referenced_files.insert(image.file_id.clone());
            Ok(())
        }
        ExcalidrawElement::Rectangle(_)
        | ExcalidrawElement::Diamond(_)
        | ExcalidrawElement::Ellipse(_) => Ok(()),
    }
}

fn validate_files(
    files: &BTreeMap<FileId, BinaryFile>,
    referenced_files: &BTreeSet<FileId>,
    limits: &ConversionLimits,
) -> Result<(), ConversionError> {
    if referenced_files.len() != files.len() {
        return invalid("unreferenced file");
    }
    let mut embedded_bytes = 0_usize;
    for (key, file) in files {
        if file.data_url.len() > limits.max_embedded_output_bytes() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::EmbeddedBytes,
                limit: usize_to_u64(limits.max_embedded_output_bytes()),
            });
        }
        validate_file(key, file, referenced_files)?;
        embedded_bytes = embedded_bytes.checked_add(file.data_url.len()).ok_or(
            ConversionError::LimitExceeded {
                resource: LimitResource::EmbeddedBytes,
                limit: usize_to_u64(limits.max_embedded_output_bytes()),
            },
        )?;
    }
    if embedded_bytes > limits.max_embedded_output_bytes() {
        return Err(ConversionError::LimitExceeded {
            resource: LimitResource::EmbeddedBytes,
            limit: usize_to_u64(limits.max_embedded_output_bytes()),
        });
    }
    Ok(())
}

fn validate_file(
    key: &FileId,
    file: &BinaryFile,
    referenced_files: &BTreeSet<FileId>,
) -> Result<(), ConversionError> {
    if key != &file.id
        || !referenced_files.contains(key)
        || file.created != 1
        || file.version != NonZeroU32::MIN
    {
        return invalid("binary file integrity");
    }
    let encoded = file.data_url.strip_prefix("data:image/png;base64,").ok_or(
        ConversionError::InvalidGeneratedDocument {
            category: "binary file data URL",
        },
    )?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| ConversionError::InvalidGeneratedDocument {
            category: "binary file base64",
        })?;
    if !decoded.starts_with(b"\x89PNG\r\n\x1a\n") || deterministic_file_id(&decoded)? != file.id.0 {
        return invalid("binary file content address");
    }
    Ok(())
}

fn validate_linear(
    base: &ElementBase,
    points: &[LocalPoint],
    polygon: bool,
) -> Result<(), ConversionError> {
    let first = points
        .first()
        .ok_or(ConversionError::InvalidGeneratedDocument {
            category: "linear points",
        })?;
    if !first.is_origin() || points.len() < 2 {
        return invalid("linear points");
    }
    let distinct = points
        .iter()
        .map(|point| {
            let (x, y) = point.values();
            (x.to_bits(), y.to_bits())
        })
        .collect::<BTreeSet<_>>();
    if distinct.len() < 2 {
        return invalid("degenerate linear points");
    }
    let (min_x, max_x, min_y, max_y) = points.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(min_x, max_x, min_y, max_y), point| {
            let (x, y) = point.values();
            (min_x.min(x), max_x.max(x), min_y.min(y), max_y.max(y))
        },
    );
    if (canonical_coordinate(max_x - min_x) - base.width.0).abs() > 0.000_002
        || (canonical_coordinate(max_y - min_y) - base.height.0).abs() > 0.000_002
    {
        return invalid("linear bounds");
    }
    if polygon && (points.len() < 4 || !points.last().is_some_and(LocalPoint::is_origin)) {
        return invalid("polygon closure");
    }
    Ok(())
}

fn invalid<T>(category: &'static str) -> Result<T, ConversionError> {
    Err(ConversionError::InvalidGeneratedDocument { category })
}

pub(crate) fn canonical_coordinate(value: f64) -> f64 {
    canonical(value, 1_000_000.0)
}

fn canonical_angle(value: f64) -> f64 {
    canonical(value, 1_000_000_000_000.0)
}

fn canonical(value: f64, scale: f64) -> f64 {
    let rounded = (value * scale).round_ties_even() / scale;
    if rounded == 0.0 { 0.0 } else { rounded }
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{canonical_angle, canonical_coordinate};

    #[test]
    fn test_should_canonicalize_negative_zero() {
        assert_eq!(canonical_coordinate(-0.0).to_bits(), 0.0_f64.to_bits());
        assert_eq!(canonical_angle(-0.0).to_bits(), 0.0_f64.to_bits());
    }

    #[test]
    fn test_should_round_coordinates_to_micropixels() {
        assert!((canonical_coordinate(1.000_000_6) - 1.000_001).abs() < f64::EPSILON);
    }
}
