//! Validated conversion policy and resource-limit options.

use typed_builder::TypedBuilder;

use crate::{ConversionError, InputRejection};

/// Policy used when the source cannot be represented exactly by native target elements.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConversionProfile {
    /// Prefer native elements, bounded visual approximations, then minimal raster fallback.
    #[default]
    Balanced,
    /// Prefer editable vector decomposition and diagnosed approximations.
    Editable,
    /// Prefer raster fallback over visual approximation.
    Fidelity,
    /// Reject every effective painted construct that is not exactly native.
    Strict,
}

/// Controls compact source provenance attached to generated elements.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProvenanceMode {
    /// Do not attach source provenance.
    #[default]
    None,
    /// Attach bounded, non-sensitive mapping provenance.
    Compact,
}

/// Hard security and output limits applied to every conversion stage.
#[non_exhaustive]
#[derive(Debug, Clone)]
#[allow(clippy::struct_field_names)] // `max_*` makes every public security bound unambiguous.
pub struct ConversionLimits {
    max_input_bytes: usize,
    max_decompressed_bytes: usize,
    max_svgz_expansion_ratio: usize,
    max_xml_elements: usize,
    max_xml_depth: usize,
    max_attributes_per_element: usize,
    max_total_attributes: usize,
    max_single_text_bytes: usize,
    max_data_url_bytes: usize,
    max_total_text_bytes: usize,
    max_references: usize,
    max_reference_depth: usize,
    max_paint_nodes: usize,
    max_path_segments_per_path: usize,
    max_path_segments: usize,
    max_target_elements: usize,
    max_target_points: usize,
    max_decomposition_elements: usize,
    max_raster_pixels_per_island: u64,
    max_raster_pixels: u64,
    max_embedded_output_bytes: usize,
    max_serialized_json_bytes: usize,
    max_coordinate: f64,
    max_element_extent: f64,
    max_resource_bytes: usize,
    max_resource_bytes_total: usize,
}

impl Default for ConversionLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 16 * 1024 * 1024,
            max_decompressed_bytes: 64 * 1024 * 1024,
            max_svgz_expansion_ratio: 100,
            max_xml_elements: 100_000,
            max_xml_depth: 128,
            max_attributes_per_element: 256,
            max_total_attributes: 500_000,
            max_single_text_bytes: 1024 * 1024,
            max_data_url_bytes: 12 * 1024 * 1024,
            max_total_text_bytes: 32 * 1024 * 1024,
            max_references: 100_000,
            max_reference_depth: 32,
            max_paint_nodes: 250_000,
            max_path_segments_per_path: 100_000,
            max_path_segments: 1_000_000,
            max_target_elements: 100_000,
            max_target_points: 1_000_000,
            max_decomposition_elements: 10_000,
            max_raster_pixels_per_island: 16_000_000,
            max_raster_pixels: 64_000_000,
            max_embedded_output_bytes: 64 * 1024 * 1024,
            max_serialized_json_bytes: 64 * 1024 * 1024,
            max_coordinate: 1_000_000.0,
            max_element_extent: 70_000.0,
            max_resource_bytes: 8 * 1024 * 1024,
            max_resource_bytes_total: 64 * 1024 * 1024,
        }
    }
}

impl ConversionLimits {
    /// Starts a fallible builder initialized with the secure defaults.
    #[must_use]
    pub fn builder() -> ConversionLimitsBuilder {
        ConversionLimitsBuilder {
            limits: Self::default(),
        }
    }
    /// Maximum accepted compressed or uncompressed input size.
    #[must_use]
    pub const fn max_input_bytes(&self) -> usize {
        self.max_input_bytes
    }

    /// Maximum decompressed SVGZ size.
    #[must_use]
    pub const fn max_decompressed_bytes(&self) -> usize {
        self.max_decompressed_bytes
    }

    /// Maximum permitted SVGZ expansion ratio.
    #[must_use]
    pub const fn max_svgz_expansion_ratio(&self) -> usize {
        self.max_svgz_expansion_ratio
    }

    /// Maximum XML element count.
    #[must_use]
    pub const fn max_xml_elements(&self) -> usize {
        self.max_xml_elements
    }

    /// Maximum XML nesting depth.
    #[must_use]
    pub const fn max_xml_depth(&self) -> usize {
        self.max_xml_depth
    }

    /// Maximum attributes on one element.
    #[must_use]
    pub const fn max_attributes_per_element(&self) -> usize {
        self.max_attributes_per_element
    }

    /// Maximum aggregate attribute count.
    #[must_use]
    pub const fn max_total_attributes(&self) -> usize {
        self.max_total_attributes
    }

    /// Maximum bytes in one ordinary attribute or text node.
    #[must_use]
    pub const fn max_single_text_bytes(&self) -> usize {
        self.max_single_text_bytes
    }

    /// Maximum lexical bytes in one data URL.
    #[must_use]
    pub const fn max_data_url_bytes(&self) -> usize {
        self.max_data_url_bytes
    }

    /// Maximum aggregate XML text and attribute bytes.
    #[must_use]
    pub const fn max_total_text_bytes(&self) -> usize {
        self.max_total_text_bytes
    }

    /// Maximum local or external reference count.
    #[must_use]
    pub const fn max_references(&self) -> usize {
        self.max_references
    }

    /// Maximum reference expansion depth.
    #[must_use]
    pub const fn max_reference_depth(&self) -> usize {
        self.max_reference_depth
    }

    /// Maximum normalized paint-node count.
    #[must_use]
    pub const fn max_paint_nodes(&self) -> usize {
        self.max_paint_nodes
    }

    /// Maximum parsed segments in one path.
    #[must_use]
    pub const fn max_path_segments_per_path(&self) -> usize {
        self.max_path_segments_per_path
    }

    /// Maximum aggregate parsed path segments.
    #[must_use]
    pub const fn max_path_segments(&self) -> usize {
        self.max_path_segments
    }

    /// Maximum target element count.
    #[must_use]
    pub const fn max_target_elements(&self) -> usize {
        self.max_target_elements
    }

    /// Maximum aggregate target local points.
    #[must_use]
    pub const fn max_target_points(&self) -> usize {
        self.max_target_points
    }

    /// Maximum decomposition output for one paint island.
    #[must_use]
    pub const fn max_decomposition_elements(&self) -> usize {
        self.max_decomposition_elements
    }

    /// Maximum pixels for one raster fallback island.
    #[must_use]
    pub const fn max_raster_pixels_per_island(&self) -> u64 {
        self.max_raster_pixels_per_island
    }

    /// Maximum aggregate raster fallback pixels.
    #[must_use]
    pub const fn max_raster_pixels(&self) -> u64 {
        self.max_raster_pixels
    }

    /// Maximum aggregate embedded binary bytes.
    #[must_use]
    pub const fn max_embedded_output_bytes(&self) -> usize {
        self.max_embedded_output_bytes
    }

    /// Maximum serialized JSON bytes.
    #[must_use]
    pub const fn max_serialized_json_bytes(&self) -> usize {
        self.max_serialized_json_bytes
    }

    /// Maximum absolute target coordinate.
    #[must_use]
    pub const fn max_coordinate(&self) -> f64 {
        self.max_coordinate
    }

    /// Maximum target element width or height.
    #[must_use]
    pub const fn max_element_extent(&self) -> f64 {
        self.max_element_extent
    }

    /// Maximum encoded bytes in one provided resource.
    #[must_use]
    pub const fn max_resource_bytes(&self) -> usize {
        self.max_resource_bytes
    }

    /// Maximum aggregate provided-resource bytes.
    #[must_use]
    pub const fn max_resource_bytes_total(&self) -> usize {
        self.max_resource_bytes_total
    }

    pub(crate) fn validate(&self) -> bool {
        self.max_input_bytes > 0
            && self.max_decompressed_bytes >= self.max_input_bytes
            && self.max_svgz_expansion_ratio > 0
            && self.max_xml_elements > 0
            && self.max_xml_depth > 0
            && self.max_attributes_per_element > 0
            && self.max_total_attributes >= self.max_attributes_per_element
            && self.max_single_text_bytes > 0
            && self.max_data_url_bytes > 0
            && self.max_total_text_bytes >= self.max_single_text_bytes
            && self.max_references > 0
            && self.max_reference_depth > 0
            && self.max_paint_nodes > 0
            && self.max_path_segments_per_path > 0
            && self.max_path_segments >= self.max_path_segments_per_path
            && self.max_target_elements > 0
            && self.max_target_points > 0
            && self.max_decomposition_elements > 0
            && self.max_decomposition_elements <= self.max_target_elements
            && self.max_raster_pixels_per_island > 0
            && self.max_raster_pixels >= self.max_raster_pixels_per_island
            && self.max_embedded_output_bytes > 0
            && self.max_serialized_json_bytes > 0
            && self.max_coordinate.is_finite()
            && self.max_coordinate > 0.0
            && self.max_element_extent.is_finite()
            && self.max_element_extent > 0.0
            && self.max_element_extent <= self.max_coordinate
            && self.max_resource_bytes > 0
            && self.max_resource_bytes_total >= self.max_resource_bytes
    }
}

/// Fallible builder for a complete, internally consistent limit set.
#[derive(Debug, Clone)]
pub struct ConversionLimitsBuilder {
    limits: ConversionLimits,
}

macro_rules! limit_setter {
    ($name:ident, $field:ident, $type:ty, $doc:literal) => {
        #[doc = $doc]
        #[must_use]
        pub const fn $name(mut self, value: $type) -> Self {
            self.limits.$field = value;
            self
        }
    };
}

impl ConversionLimitsBuilder {
    limit_setter!(
        max_input_bytes,
        max_input_bytes,
        usize,
        "Sets the input-byte cap."
    );
    limit_setter!(
        max_decompressed_bytes,
        max_decompressed_bytes,
        usize,
        "Sets the decompressed-byte cap."
    );
    limit_setter!(
        max_svgz_expansion_ratio,
        max_svgz_expansion_ratio,
        usize,
        "Sets the SVGZ expansion-ratio cap."
    );
    limit_setter!(
        max_xml_elements,
        max_xml_elements,
        usize,
        "Sets the XML element cap."
    );
    limit_setter!(
        max_xml_depth,
        max_xml_depth,
        usize,
        "Sets the XML depth cap."
    );
    limit_setter!(
        max_attributes_per_element,
        max_attributes_per_element,
        usize,
        "Sets the per-element attribute cap."
    );
    limit_setter!(
        max_total_attributes,
        max_total_attributes,
        usize,
        "Sets the aggregate attribute cap."
    );
    limit_setter!(
        max_single_text_bytes,
        max_single_text_bytes,
        usize,
        "Sets the per-value text cap."
    );
    limit_setter!(
        max_data_url_bytes,
        max_data_url_bytes,
        usize,
        "Sets the data-URL cap."
    );
    limit_setter!(
        max_total_text_bytes,
        max_total_text_bytes,
        usize,
        "Sets the aggregate text cap."
    );
    limit_setter!(
        max_references,
        max_references,
        usize,
        "Sets the reference-count cap."
    );
    limit_setter!(
        max_reference_depth,
        max_reference_depth,
        usize,
        "Sets the reference-depth cap."
    );
    limit_setter!(
        max_paint_nodes,
        max_paint_nodes,
        usize,
        "Sets the normalized paint-node cap."
    );
    limit_setter!(
        max_path_segments_per_path,
        max_path_segments_per_path,
        usize,
        "Sets the per-path segment cap."
    );
    limit_setter!(
        max_path_segments,
        max_path_segments,
        usize,
        "Sets the aggregate path-segment cap."
    );
    limit_setter!(
        max_target_elements,
        max_target_elements,
        usize,
        "Sets the target-element cap."
    );
    limit_setter!(
        max_target_points,
        max_target_points,
        usize,
        "Sets the target-point cap."
    );
    limit_setter!(
        max_decomposition_elements,
        max_decomposition_elements,
        usize,
        "Sets the per-island decomposition cap."
    );
    limit_setter!(
        max_raster_pixels_per_island,
        max_raster_pixels_per_island,
        u64,
        "Sets the per-island raster-pixel cap."
    );
    limit_setter!(
        max_raster_pixels,
        max_raster_pixels,
        u64,
        "Sets the aggregate raster-pixel cap."
    );
    limit_setter!(
        max_embedded_output_bytes,
        max_embedded_output_bytes,
        usize,
        "Sets the embedded-output cap."
    );
    limit_setter!(
        max_serialized_json_bytes,
        max_serialized_json_bytes,
        usize,
        "Sets the serialized-JSON cap."
    );
    limit_setter!(
        max_coordinate,
        max_coordinate,
        f64,
        "Sets the absolute-coordinate cap."
    );
    limit_setter!(
        max_element_extent,
        max_element_extent,
        f64,
        "Sets the element-extent cap."
    );
    limit_setter!(
        max_resource_bytes,
        max_resource_bytes,
        usize,
        "Sets the per-resource cap."
    );
    limit_setter!(
        max_resource_bytes_total,
        max_resource_bytes_total,
        usize,
        "Sets the aggregate resource cap."
    );

    /// Validates relationships among every bound and returns an immutable limit set.
    ///
    /// # Errors
    ///
    /// Returns an invalid-options rejection when any bound is zero, non-finite, or
    /// inconsistent with its corresponding aggregate bound.
    pub fn build(self) -> Result<ConversionLimits, ConversionError> {
        if self.limits.validate() {
            Ok(self.limits)
        } else {
            Err(ConversionError::InputRejected(
                InputRejection::InvalidOptions,
            ))
        }
    }
}

/// Geometry approximation settings.
#[non_exhaustive]
#[derive(Debug, Clone)]
#[allow(clippy::struct_field_names)] // Units belong in option names at the public boundary.
pub struct GeometryOptions {
    curve_tolerance_px: f64,
    max_native_radius_error_px: f64,
    max_text_anchor_error_px: f64,
}

impl Default for GeometryOptions {
    fn default() -> Self {
        Self {
            curve_tolerance_px: 0.5,
            max_native_radius_error_px: 1.0,
            max_text_anchor_error_px: 1.0,
        }
    }
}

impl GeometryOptions {
    /// Creates validated geometry approximation settings.
    ///
    /// # Errors
    ///
    /// Returns an invalid-options rejection when a value is non-finite or out of range.
    pub fn try_new(
        curve_tolerance_px: f64,
        max_native_radius_error_px: f64,
        max_text_anchor_error_px: f64,
    ) -> Result<Self, ConversionError> {
        let options = Self {
            curve_tolerance_px,
            max_native_radius_error_px,
            max_text_anchor_error_px,
        };
        if options.validate() {
            Ok(options)
        } else {
            Err(ConversionError::InputRejected(
                InputRejection::InvalidOptions,
            ))
        }
    }

    /// Curve-to-chord error tolerance in CSS pixels.
    #[must_use]
    pub const fn curve_tolerance_px(&self) -> f64 {
        self.curve_tolerance_px
    }

    /// Maximum corner-radius approximation error in CSS pixels.
    #[must_use]
    pub const fn max_native_radius_error_px(&self) -> f64 {
        self.max_native_radius_error_px
    }

    /// Maximum target-font anchor correction error in CSS pixels.
    #[must_use]
    pub const fn max_text_anchor_error_px(&self) -> f64 {
        self.max_text_anchor_error_px
    }

    pub(crate) fn validate(&self) -> bool {
        (0.01..=16.0).contains(&self.curve_tolerance_px)
            && self.max_native_radius_error_px.is_finite()
            && self.max_native_radius_error_px >= 0.0
            && self.max_text_anchor_error_px.is_finite()
            && self.max_text_anchor_error_px >= 0.0
    }
}

/// Raster fallback settings.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct RasterOptions {
    fallback_scale: f64,
    allow_nested_svg: bool,
}

impl Default for RasterOptions {
    fn default() -> Self {
        Self {
            fallback_scale: 2.0,
            allow_nested_svg: false,
        }
    }
}

impl RasterOptions {
    /// Creates validated raster fallback settings.
    ///
    /// # Errors
    ///
    /// Returns an invalid-options rejection when the scale is non-finite or out of range.
    pub fn try_new(fallback_scale: f64, allow_nested_svg: bool) -> Result<Self, ConversionError> {
        let options = Self {
            fallback_scale,
            allow_nested_svg,
        };
        if options.validate() {
            Ok(options)
        } else {
            Err(ConversionError::InputRejected(
                InputRejection::InvalidOptions,
            ))
        }
    }

    /// Requested raster fallback scale, bounded to 0.25–4.0.
    #[must_use]
    pub const fn fallback_scale(&self) -> f64 {
        self.fallback_scale
    }

    /// Whether nested SVG data images are admitted under shared limits.
    #[must_use]
    pub const fn allow_nested_svg(&self) -> bool {
        self.allow_nested_svg
    }

    pub(crate) fn validate(&self) -> bool {
        (0.25..=4.0).contains(&self.fallback_scale)
    }
}

/// Deterministic font-policy settings.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct FontOptions {
    substitute_with_liberation_sans: bool,
    target_line_height: f64,
}

impl Default for FontOptions {
    fn default() -> Self {
        Self {
            substitute_with_liberation_sans: true,
            target_line_height: 1.2,
        }
    }
}

impl FontOptions {
    /// Creates validated deterministic font settings.
    ///
    /// # Errors
    ///
    /// Returns an invalid-options rejection when the line height is non-finite or out of range.
    pub fn try_new(
        substitute_with_liberation_sans: bool,
        target_line_height: f64,
    ) -> Result<Self, ConversionError> {
        let options = Self {
            substitute_with_liberation_sans,
            target_line_height,
        };
        if options.validate() {
            Ok(options)
        } else {
            Err(ConversionError::InputRejected(
                InputRejection::InvalidOptions,
            ))
        }
    }

    /// Whether unavailable source families use the bundled target-compatible face.
    #[must_use]
    pub const fn substitute_with_liberation_sans(&self) -> bool {
        self.substitute_with_liberation_sans
    }

    /// Unitless Excalidraw line height used for native text.
    #[must_use]
    pub const fn target_line_height(&self) -> f64 {
        self.target_line_height
    }

    pub(crate) fn validate(&self) -> bool {
        self.target_line_height.is_finite() && (0.5..=4.0).contains(&self.target_line_height)
    }
}

/// Complete conversion policy.
#[non_exhaustive]
#[derive(Debug, Clone, TypedBuilder)]
#[builder(field_defaults(default))]
pub struct ConversionOptions {
    /// Fidelity/editability policy.
    pub profile: ConversionProfile,
    /// Hard resource limits.
    pub limits: ConversionLimits,
    /// Geometry approximation settings.
    pub geometry: GeometryOptions,
    /// Raster fallback settings.
    pub raster: RasterOptions,
    /// Deterministic font settings.
    pub fonts: FontOptions,
    /// Optional compact provenance.
    pub provenance: ProvenanceMode,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl ConversionOptions {
    pub(crate) fn is_valid(&self) -> bool {
        self.limits.validate()
            && self.geometry.validate()
            && self.raster.validate()
            && self.fonts.validate()
    }
}
