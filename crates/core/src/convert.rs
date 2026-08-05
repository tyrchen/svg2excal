//! Conversion orchestration and native/fallback lowering.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

use base64::Engine as _;
use usvg::{
    filter::{Input as FilterInput, Kind as FilterKind},
    tiny_skia_path::{PathSegment, Point as SkiaPoint, Transform},
};

use crate::{
    ConversionOptions, ConversionProfile, ProvenanceMode,
    error::{ConversionError, LimitResource},
    identity::{element_identity, file_id, group_id},
    ingest::{NormalizedInput, normalize},
    report::{
        ConversionDiagnostic, ConversionReport, ConversionResult, DiagnosticCode,
        DiagnosticSeverity, sort_diagnostics,
    },
    resource::ResourceContext,
    source::{SourceMetadata, SourceNodeMetadata},
    target::{
        BinaryFile, ElementBase, ElementStyle, ExcalidrawColor, ExcalidrawDocument,
        ExcalidrawElement, FileId, Finite, GroupId, LocalPoint, StrokeStyle, TextAlign,
    },
};

const ANTIALIAS_MARGIN_PIXELS: f64 = 2.0;

/// Converts SVG/SVGZ bytes without external I/O.
///
/// # Errors
///
/// Returns a typed [`ConversionError`] for invalid input, denied resources,
/// exhausted limits, strict fidelity violations, or invalid target output.
pub fn convert(
    svg: &[u8],
    options: &ConversionOptions,
) -> Result<ConversionResult, ConversionError> {
    convert_internal(svg, options, None)
}

/// Converts SVG/SVGZ bytes using an explicit bounded resource provider.
///
/// # Errors
///
/// Returns a typed [`ConversionError`] under the same contract as [`convert`],
/// plus provider/resource validation failures.
pub fn convert_with_resources(
    svg: &[u8],
    options: &ConversionOptions,
    resources: &ResourceContext<'_>,
) -> Result<ConversionResult, ConversionError> {
    convert_internal(svg, options, Some(resources))
}

fn convert_internal(
    svg: &[u8],
    options: &ConversionOptions,
    resources: Option<&ResourceContext<'_>>,
) -> Result<ConversionResult, ConversionError> {
    options.check_cancelled()?;
    let normalized = normalize(svg, options, resources)?;
    options.check_cancelled()?;
    let scene_scale = scene_scale(&normalized, options)?;
    let mut context = LoweringContext::new(options, &normalized, scene_scale)?;
    context.lower_group(normalized.tree.root(), 1.0)?;
    options.check_cancelled()?;
    context.finalize_groups()?;
    if context.arrow_count > 0 {
        context.diagnostics.push(ConversionDiagnostic::new(
            DiagnosticCode::BindingNotInferred,
            DiagnosticSeverity::Info,
            0,
            "SVG connectors retain explicit null bindings because proximity is not topology",
        ));
    }
    sort_diagnostics(&mut context.diagnostics);

    if options.profile == ConversionProfile::Strict
        && context
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity() != DiagnosticSeverity::Info)
    {
        return Err(ConversionError::StrictFidelityViolation {
            diagnostics: context.diagnostics,
        });
    }

    let document = ExcalidrawDocument::new(context.elements, context.files);
    document.validate(&options.limits)?;
    let _serialized = document.to_pretty_json_with_limits(&options.limits)?;
    let report = ConversionReport {
        profile: options.profile.into(),
        input_bytes: normalized.input_bytes,
        source_elements: normalized.census.elements,
        source_references: normalized.census.references,
        paint_nodes: normalized.paint_nodes,
        target_elements: document.elements().len(),
        target_points: context.target_points,
        embedded_bytes: context.embedded_bytes,
        fallback_pixels: context.fallback_pixels,
        diagnostics: context.diagnostics,
    };
    Ok(ConversionResult { document, report })
}

fn scene_scale(
    normalized: &NormalizedInput,
    options: &ConversionOptions,
) -> Result<f64, ConversionError> {
    let width = f64::from(normalized.tree.size().width());
    let height = f64::from(normalized.tree.size().height());
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(ConversionError::GeometryOverflow);
    }
    let largest = width.max(height);
    if largest <= options.limits.max_element_extent() {
        return Ok(1.0);
    }
    Ok(options.limits.max_element_extent() / largest)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn transformed(point: SkiaPoint, transform: Transform, scene_scale: f64) -> Self {
        let x = f64::from(transform.sx) * f64::from(point.x)
            + f64::from(transform.kx) * f64::from(point.y)
            + f64::from(transform.tx);
        let y = f64::from(transform.ky) * f64::from(point.x)
            + f64::from(transform.sy) * f64::from(point.y)
            + f64::from(transform.ty);
        Self {
            x: x * scene_scale,
            y: y * scene_scale,
        }
    }

    fn midpoint(self, other: Self) -> Self {
        Self {
            x: (self.x + other.x) * 0.5,
            y: (self.y + other.y) * 0.5,
        }
    }
}

#[derive(Debug)]
struct Subpath {
    points: Vec<Point>,
    closed: bool,
    curved: bool,
}

#[derive(Debug, Clone, Copy)]
enum PaintRole {
    Combined,
    Fill,
    Stroke,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct GroupKey {
    source_order: u32,
    instance: u32,
}

struct LoweringContext<'a> {
    options: &'a ConversionOptions,
    digest: &'a blake3::Hash,
    source: &'a SourceMetadata,
    elements: Vec<ExcalidrawElement>,
    element_ids: BTreeSet<String>,
    element_groups: Vec<Vec<GroupKey>>,
    active_groups: Vec<GroupKey>,
    active_instances: Vec<u32>,
    next_instance: u32,
    reported_marker_ids: BTreeSet<String>,
    files: BTreeMap<FileId, BinaryFile>,
    diagnostics: Vec<ConversionDiagnostic>,
    source_order: u32,
    paint_order: u32,
    current_source_tag: String,
    arrow_count: usize,
    total_path_segments: usize,
    target_points: usize,
    fallback_pixels: u64,
    embedded_bytes: usize,
    scene_scale: f64,
    source_viewport: usvg::NonZeroRect,
}

impl<'a> LoweringContext<'a> {
    fn new(
        options: &'a ConversionOptions,
        normalized: &'a NormalizedInput,
        scene_scale: f64,
    ) -> Result<Self, ConversionError> {
        let mut diagnostics = normalized.diagnostics.clone();
        if scene_scale < 1.0 {
            diagnostics.push(ConversionDiagnostic::new(
                DiagnosticCode::SceneScaledToTargetRange,
                DiagnosticSeverity::Approximation,
                0,
                "scene was uniformly scaled to fit Excalidraw restoration bounds",
            ));
        }
        Ok(Self {
            options,
            digest: &normalized.digest,
            source: &normalized.source,
            elements: Vec::with_capacity(normalized.paint_nodes.min(4096)),
            element_ids: BTreeSet::new(),
            element_groups: Vec::with_capacity(normalized.paint_nodes.min(4096)),
            active_groups: Vec::new(),
            active_instances: Vec::new(),
            next_instance: 0,
            reported_marker_ids: BTreeSet::new(),
            files: BTreeMap::new(),
            diagnostics,
            source_order: 0,
            paint_order: 0,
            current_source_tag: "svg".to_owned(),
            arrow_count: 0,
            total_path_segments: 0,
            target_points: 0,
            fallback_pixels: 0,
            embedded_bytes: 0,
            scene_scale,
            source_viewport: usvg::NonZeroRect::from_xywh(
                0.0,
                0.0,
                normalized.tree.size().width(),
                normalized.tree.size().height(),
            )
            .ok_or(ConversionError::GeometryOverflow)?,
        })
    }

    fn lower_group(
        &mut self,
        group: &usvg::Group,
        inherited_opacity: f64,
    ) -> Result<(), ConversionError> {
        let group_metadata = self
            .source
            .node(group.id())
            .filter(|metadata| metadata.explicit_group)
            .cloned();
        let entered_instance = group_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.tag == "use");
        if entered_instance {
            self.next_instance =
                self.next_instance
                    .checked_add(1)
                    .ok_or(ConversionError::LimitExceeded {
                        resource: LimitResource::WorkUnits,
                        limit: u64::from(u32::MAX),
                    })?;
            self.active_instances.push(self.next_instance);
        }
        let group_anchor = group_metadata.map(|metadata| GroupKey {
            source_order: metadata.order,
            instance: self.active_instances.last().copied().unwrap_or(0),
        });
        if let Some(anchor) = group_anchor {
            self.active_groups.push(anchor);
        }
        let opacity = inherited_opacity * f64::from(group.opacity().get());
        let mut previous_marker_promoted = false;
        for (index, node) in group.children().iter().enumerate() {
            self.options.check_cancelled()?;
            if previous_marker_promoted && self.is_generated_marker_artwork(group.children(), index)
            {
                previous_marker_promoted = false;
                continue;
            }
            previous_marker_promoted = false;
            self.select_source(node)?;
            match node {
                usvg::Node::Group(child) if child.should_isolate() => {
                    if self.can_omit_drop_shadow(child) {
                        self.diagnostics.push(ConversionDiagnostic::new(
                            DiagnosticCode::FilterOmitted,
                            DiagnosticSeverity::Omission,
                            self.source_order,
                            "a bounded cosmetic drop shadow was omitted while preserving source \
                             graphics",
                        ));
                        self.lower_group(child, opacity)?;
                    } else {
                        let code = if child.mask().is_some() {
                            Some(DiagnosticCode::MaskRasterized)
                        } else if child.clip_path().is_some() {
                            Some(DiagnosticCode::ClipRasterized)
                        } else {
                            None
                        };
                        self.fallback_or_violation(node, "isolated SVG compositing group", code)?;
                    }
                }
                usvg::Node::Group(child) => self.lower_group(child, opacity)?,
                usvg::Node::Path(path) if path.is_visible() => {
                    previous_marker_promoted = self.lower_path(
                        node,
                        path,
                        opacity,
                        self.marker_promotion_allowed(group.children(), index, path),
                    )?;
                }
                usvg::Node::Path(_) => {}
                usvg::Node::Text(text) => self.lower_text(node, text, opacity)?,
                usvg::Node::Image(image) => {
                    if image_is_animated(image.kind()) {
                        self.diagnostics.push(ConversionDiagnostic::new(
                            DiagnosticCode::AnimatedImageSnapshot,
                            DiagnosticSeverity::Fallback,
                            self.source_order,
                            "an animated source image was deterministically frozen to its first \
                             frame",
                        ));
                        self.fallback_or_violation(node, "animated source raster image", None)?;
                    } else {
                        self.rasterize_source_image(node)?;
                    }
                }
            }
        }
        if group_anchor.is_some() {
            self.active_groups.pop();
        }
        if entered_instance {
            self.active_instances.pop();
        }
        Ok(())
    }

    fn select_source(&mut self, node: &usvg::Node) -> Result<(), ConversionError> {
        self.paint_order =
            self.paint_order
                .checked_add(1)
                .ok_or(ConversionError::LimitExceeded {
                    resource: LimitResource::WorkUnits,
                    limit: u64::from(u32::MAX),
                })?;
        if let Some(metadata) = self.source.node(node.id()) {
            self.source_order = metadata.order;
            self.current_source_tag.clone_from(&metadata.tag);
        } else {
            self.source_order = self.paint_order;
            match node {
                usvg::Node::Group(_) => "g",
                usvg::Node::Path(_) => "path",
                usvg::Node::Image(_) => "image",
                usvg::Node::Text(_) => "text",
            }
            .clone_into(&mut self.current_source_tag);
        }
        Ok(())
    }

    fn is_generated_marker_artwork(&self, children: &[usvg::Node], index: usize) -> bool {
        let Some(usvg::Node::Group(group)) = children.get(index) else {
            return false;
        };
        if !group.id().is_empty() {
            return false;
        }
        index
            .checked_sub(1)
            .and_then(|previous| children.get(previous))
            .is_some_and(|node| self.source.has_marker(node.id()))
    }

    fn marker_promotion_allowed(
        &self,
        children: &[usvg::Node],
        index: usize,
        path: &usvg::Path,
    ) -> bool {
        let Some(metadata) = children
            .get(index)
            .and_then(|node| self.source.node(node.id()))
            .filter(|metadata| metadata.marker_declared && metadata.marker_fully_recognized)
        else {
            return false;
        };
        let Some(usvg::Node::Group(artwork)) = children.get(index.saturating_add(1)) else {
            return false;
        };
        artwork.id().is_empty() && marker_artwork_matches(artwork, path, metadata)
    }

    fn can_omit_drop_shadow(&self, group: &usvg::Group) -> bool {
        if (group.opacity().get() - 1.0).abs() > f32::EPSILON
            || group.blend_mode() != usvg::BlendMode::Normal
            || group.isolate()
            || group.clip_path().is_some()
            || group.mask().is_some()
            || group.filters().len() != 1
        {
            return false;
        }
        let Some(shadow) = self
            .source
            .node(group.id())
            .and_then(|metadata| metadata.drop_shadow)
        else {
            return false;
        };
        let Some(filter) = group.filters().first() else {
            return false;
        };
        let Some(primitive) = filter.primitives().first() else {
            return false;
        };
        if filter.primitives().len() != 1 {
            return false;
        }
        let FilterKind::DropShadow(resolved) = primitive.kind() else {
            return false;
        };
        if resolved.input() != &FilterInput::SourceGraphic {
            return false;
        }
        let resolved_balanced = resolved.opacity().get() <= 0.15
            && resolved.std_dev_x().get() <= 4.0
            && resolved.std_dev_y().get() <= 4.0
            && resolved.dx().abs() <= 4.0
            && resolved.dy().abs() <= 4.0;
        let resolved_editable = resolved.opacity().get() <= 0.25
            && resolved.std_dev_x().get() <= 8.0
            && resolved.std_dev_y().get() <= 8.0
            && resolved.dx().abs() <= 8.0
            && resolved.dy().abs() <= 8.0;
        match self.options.profile {
            ConversionProfile::Balanced => shadow.balanced_omittable && resolved_balanced,
            ConversionProfile::Editable => shadow.editable_omittable && resolved_editable,
            ConversionProfile::Fidelity | ConversionProfile::Strict => false,
        }
    }

    fn lower_path(
        &mut self,
        node: &usvg::Node,
        path: &usvg::Path,
        opacity: f64,
        marker_promotion_allowed: bool,
    ) -> Result<bool, ConversionError> {
        let source_metadata = self.source.node(node.id()).cloned();
        if self.path_preflight(node, path, source_metadata.as_ref())? {
            return Ok(false);
        }

        if let Some(metadata) = source_metadata.as_ref()
            && metadata.tag == "rect"
            && metadata.rect_radius.is_some()
            && self.lower_correlated_rectangle(node, path, opacity, metadata)?
        {
            return Ok(false);
        }

        if looks_like_axis_aligned_ellipse(path) {
            self.lower_ellipse(path, opacity)?;
            return Ok(false);
        }
        let has_curves = path
            .data()
            .segments()
            .any(|segment| matches!(segment, PathSegment::QuadTo(..) | PathSegment::CubicTo(..)));
        if has_curves
            && matches!(
                self.options.profile,
                ConversionProfile::Fidelity | ConversionProfile::Strict
            )
        {
            self.fallback_or_violation(node, "curved path", None)?;
            return Ok(false);
        }

        let subpaths = flatten_path(
            path,
            self.options.geometry.curve_tolerance_px(),
            self.scene_scale,
            self.options.limits.max_target_points(),
        )?;
        if subpaths.is_empty() {
            return Ok(false);
        }
        if subpaths.len() > 1 && path.fill().is_some() {
            if self.options.profile == ConversionProfile::Editable
                && self.lower_editable_compound_fill(path, &subpaths, opacity)?
            {
                return Ok(false);
            }
            self.fallback_or_violation(node, "compound filled path", None)?;
            return Ok(false);
        }
        if let Some(metadata) = source_metadata.as_ref()
            && metadata.marker_declared
            && metadata.marker_fully_recognized
            && marker_promotion_allowed
            && self.lower_marker_path(path, &subpaths, opacity, metadata)?
        {
            return Ok(true);
        }
        if source_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.marker_declared)
            && self.reported_marker_ids.insert(node.id().to_owned())
        {
            self.diagnostics.push(ConversionDiagnostic::new(
                DiagnosticCode::MarkerPreservedAsGeometry,
                DiagnosticSeverity::Fallback,
                self.source_order,
                "an SVG marker without a proven native equivalent was preserved as explicit \
                 generated geometry",
            ));
        }
        if path.fill().is_some()
            && subpaths
                .iter()
                .any(|subpath| !is_simple_fill_boundary(subpath, self.options))
        {
            self.fallback_or_violation(node, "non-simple filled path", None)?;
            return Ok(false);
        }
        let flattened = subpaths.iter().any(|subpath| subpath.curved);
        for subpath in subpaths {
            self.lower_subpath(path, &subpath, opacity)?;
        }
        if flattened {
            self.diagnostics.push(ConversionDiagnostic::new(
                DiagnosticCode::PathFlattened,
                DiagnosticSeverity::Approximation,
                self.source_order,
                "curved path was flattened within the configured error bound",
            ));
        }
        Ok(false)
    }

    fn path_preflight(
        &mut self,
        node: &usvg::Node,
        path: &usvg::Path,
        source_metadata: Option<&SourceNodeMetadata>,
    ) -> Result<bool, ConversionError> {
        if source_metadata
            .is_some_and(|metadata| metadata.marker_declared && !metadata.marker_fully_recognized)
            && self.reported_marker_ids.insert(node.id().to_owned())
        {
            self.diagnostics.push(ConversionDiagnostic::new(
                DiagnosticCode::MarkerPreservedAsGeometry,
                DiagnosticSeverity::Fallback,
                self.source_order,
                "an unrecognized SVG marker was preserved as explicit generated geometry",
            ));
        }
        if path.fill().is_none() && path.stroke().is_none() {
            return Ok(true);
        }
        if !path_has_only_solid_paint(path) || !stroke_transform_is_scalar(path) {
            let code =
                (!path_has_only_solid_paint(path)).then_some(DiagnosticCode::GradientRasterized);
            self.fallback_or_violation(node, "non-native path paint or stroke transform", code)?;
            return Ok(true);
        }
        if path.stroke().is_some() && !stroke_is_exact_native(path) {
            if matches!(
                self.options.profile,
                ConversionProfile::Fidelity | ConversionProfile::Strict
            ) {
                self.fallback_or_violation(node, "non-native stroke semantics", None)?;
                return Ok(true);
            }
            self.diagnostics.push(ConversionDiagnostic::new(
                DiagnosticCode::StrokeStyleApproximated,
                DiagnosticSeverity::Approximation,
                self.source_order,
                "SVG stroke dash, cap, join, or miter semantics were approximated",
            ));
        }
        self.reserve_path_segments(path.data().segments().count())?;
        Ok(false)
    }

    fn reserve_path_segments(&mut self, segment_count: usize) -> Result<(), ConversionError> {
        if segment_count > self.options.limits.max_path_segments_per_path() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::PathSegments,
                limit: usize_to_u64(self.options.limits.max_path_segments_per_path()),
            });
        }
        self.total_path_segments = self.total_path_segments.checked_add(segment_count).ok_or(
            ConversionError::LimitExceeded {
                resource: LimitResource::PathSegments,
                limit: usize_to_u64(self.options.limits.max_path_segments()),
            },
        )?;
        if self.total_path_segments > self.options.limits.max_path_segments() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::PathSegments,
                limit: usize_to_u64(self.options.limits.max_path_segments()),
            });
        }
        Ok(())
    }

    fn lower_correlated_rectangle(
        &mut self,
        node: &usvg::Node,
        path: &usvg::Path,
        opacity: f64,
        metadata: &SourceNodeMetadata,
    ) -> Result<bool, ConversionError> {
        let transform = path.abs_transform();
        if transform.kx.abs() > 1.0e-6 || transform.ky.abs() > 1.0e-6 {
            return Ok(false);
        }
        let Some((rx, ry)) = metadata.rect_radius else {
            return Ok(false);
        };
        let horizontal_radius = rx * f64::from(transform.sx).abs() * self.scene_scale;
        let vertical_radius = ry * f64::from(transform.sy).abs() * self.scene_scale;
        let bbox = path.abs_bounding_box();
        let x = f64::from(bbox.x()) * self.scene_scale;
        let y = f64::from(bbox.y()) * self.scene_scale;
        let width = f64::from(bbox.width()) * self.scene_scale;
        let height = f64::from(bbox.height()) * self.scene_scale;
        let source_horizontal_radius = horizontal_radius.min(width * 0.5);
        let source_vertical_radius = vertical_radius.min(height * 0.5);
        let radius = source_horizontal_radius.min(source_vertical_radius);
        let target_radius = adaptive_target_radius(width.min(height), radius);
        let radius_error = (source_horizontal_radius - target_radius)
            .abs()
            .max((source_vertical_radius - target_radius).abs());
        if radius_error > self.options.geometry.max_native_radius_error_px() {
            self.fallback_or_violation(node, "non-native rectangle corner radius", None)?;
            return Ok(true);
        }
        if radius_error > 1.0e-6 {
            if matches!(
                self.options.profile,
                ConversionProfile::Fidelity | ConversionProfile::Strict
            ) {
                self.fallback_or_violation(node, "approximated rectangle corner radius", None)?;
                return Ok(true);
            }
            self.diagnostics.push(ConversionDiagnostic::new(
                DiagnosticCode::CornerRadiusApproximated,
                DiagnosticSeverity::Approximation,
                self.source_order,
                "SVG rectangle corners were mapped within the configured target radius error",
            ));
        }
        for role in paint_roles(path, true) {
            let mut style = path_style(path, role, opacity, self.scene_scale);
            style.roundness = (radius > 0.0).then_some(radius);
            let base = self.make_base("rectangle", x, y, width, height, style)?;
            self.push_element(ExcalidrawElement::rectangle(base))?;
        }
        Ok(true)
    }

    fn lower_marker_path(
        &mut self,
        path: &usvg::Path,
        subpaths: &[Subpath],
        opacity: f64,
        metadata: &SourceNodeMetadata,
    ) -> Result<bool, ConversionError> {
        let Some(subpath) = subpaths.first() else {
            return Ok(false);
        };
        let has_painted_fill = path.fill().is_some()
            && subpath.points.len() >= 3
            && !points_are_collinear(&subpath.points);
        if subpaths.len() != 1
            || subpath.closed
            || has_painted_fill
            || path.stroke().is_none()
            || !marker_paint_matches(path, metadata)
        {
            return Ok(false);
        }
        let first = subpath
            .points
            .first()
            .ok_or(ConversionError::GeometryOverflow)?;
        let (_, _, width, height) = bounds(&subpath.points)?;
        let mut local = Vec::with_capacity(subpath.points.len());
        for point in &subpath.points {
            local.push(LocalPoint::new(
                point.x - first.x,
                point.y - first.y,
                &self.options.limits,
            )?);
        }
        self.reserve_points(local.len())?;
        let style = path_style(path, PaintRole::Stroke, opacity, self.scene_scale);
        let base = self.make_base("arrow", first.x, first.y, width, height, style)?;
        self.push_element(ExcalidrawElement::arrow(
            base,
            local,
            metadata.marker_start,
            metadata.marker_end,
        ))?;
        self.arrow_count = self.arrow_count.saturating_add(1);
        Ok(true)
    }

    fn lower_editable_compound_fill(
        &mut self,
        path: &usvg::Path,
        subpaths: &[Subpath],
        opacity: f64,
    ) -> Result<bool, ConversionError> {
        if path.stroke().is_some()
            || subpaths.len() > self.options.limits.max_decomposition_elements()
            || !compound_subpaths_are_disjoint(subpaths, self.options)
        {
            return Ok(false);
        }
        let added_points = subpaths.iter().try_fold(0_usize, |total, subpath| {
            total.checked_add(subpath.points.len().saturating_add(1))
        });
        let Some(added_points) = added_points else {
            return Ok(false);
        };
        if self
            .elements
            .len()
            .checked_add(subpaths.len())
            .is_none_or(|total| total > self.options.limits.max_target_elements())
            || self
                .target_points
                .checked_add(added_points)
                .is_none_or(|total| total > self.options.limits.max_target_points())
        {
            return Ok(false);
        }
        let group = GroupKey {
            source_order: self.source_order,
            instance: self.active_instances.last().copied().unwrap_or(0),
        };
        self.active_groups.push(group);
        for subpath in subpaths {
            self.lower_subpath(path, subpath, opacity)?;
        }
        self.active_groups.pop();
        self.diagnostics.push(ConversionDiagnostic::new(
            DiagnosticCode::CompoundPathDecomposed,
            DiagnosticSeverity::Approximation,
            self.source_order,
            "a disjoint compound fill was decomposed into grouped editable polygons",
        ));
        Ok(true)
    }

    fn lower_ellipse(&mut self, path: &usvg::Path, opacity: f64) -> Result<(), ConversionError> {
        let bbox = path.abs_bounding_box();
        let x = f64::from(bbox.x()) * self.scene_scale;
        let y = f64::from(bbox.y()) * self.scene_scale;
        let width = f64::from(bbox.width()) * self.scene_scale;
        let height = f64::from(bbox.height()) * self.scene_scale;
        for role in paint_roles(path, true) {
            let style = path_style(path, role, opacity, self.scene_scale);
            let base = self.make_base("ellipse", x, y, width, height, style)?;
            self.push_element(ExcalidrawElement::ellipse(base))?;
        }
        Ok(())
    }

    fn lower_subpath(
        &mut self,
        path: &usvg::Path,
        subpath: &Subpath,
        opacity: f64,
    ) -> Result<(), ConversionError> {
        if subpath.points.len() < 2 {
            return Ok(());
        }
        let has_painted_fill = path.fill().is_some()
            && subpath.points.len() >= 3
            && !points_are_collinear(&subpath.points);
        let open_fill_and_stroke = !subpath.closed && has_painted_fill && path.stroke().is_some();
        let roles = paint_roles(
            path,
            subpath.closed && has_painted_fill && !open_fill_and_stroke,
        );
        for role in roles {
            if matches!(role, PaintRole::Stroke) && path.stroke().is_none() {
                continue;
            }
            if matches!(role, PaintRole::Fill) && !has_painted_fill {
                continue;
            }
            let polygon = !matches!(role, PaintRole::Stroke) && path.fill().is_some();
            if polygon && subpath.points.len() < 3 {
                continue;
            }
            let style = path_style(path, role, opacity, self.scene_scale);
            if subpath.closed && is_axis_aligned_rectangle(&subpath.points) {
                let (x, y, width, height) = bounds(&subpath.points)?;
                let base = self.make_base("rectangle", x, y, width, height, style)?;
                self.push_element(ExcalidrawElement::rectangle(base))?;
            } else if subpath.closed && is_target_diamond(&subpath.points) {
                let (x, y, width, height) = bounds(&subpath.points)?;
                let base = self.make_base("diamond", x, y, width, height, style)?;
                self.push_element(ExcalidrawElement::diamond(base))?;
            } else {
                let close_stroke = subpath.closed && matches!(role, PaintRole::Stroke);
                self.emit_linear(&subpath.points, polygon, close_stroke, style)?;
            }
        }
        Ok(())
    }

    fn emit_linear(
        &mut self,
        points: &[Point],
        polygon: bool,
        close_stroke: bool,
        style: ElementStyle,
    ) -> Result<(), ConversionError> {
        let first = points.first().ok_or(ConversionError::GeometryOverflow)?;
        let (x, y, width, height) = bounds(points)?;
        let closes = polygon || close_stroke;
        let mut local = Vec::with_capacity(points.len().saturating_add(usize::from(closes)));
        for point in points {
            local.push(LocalPoint::new(
                point.x - first.x,
                point.y - first.y,
                &self.options.limits,
            )?);
        }
        if closes {
            local.push(LocalPoint::new(0.0, 0.0, &self.options.limits)?);
        }
        let added = local.len();
        self.reserve_points(added)?;
        let base = self.make_base("line", first.x, first.y, width, height, style)?;
        self.push_element(ExcalidrawElement::line(base, local, polygon))?;
        let _ = (x, y);
        Ok(())
    }

    fn lower_text(
        &mut self,
        node: &usvg::Node,
        text: &usvg::Text,
        opacity: f64,
    ) -> Result<(), ConversionError> {
        if text.writing_mode() != usvg::WritingMode::LeftToRight
            || text.dx().iter().any(|value| value.abs() > f32::EPSILON)
            || text.dy().iter().any(|value| value.abs() > f32::EPSILON)
            || text.rotate().iter().any(|value| value.abs() > f32::EPSILON)
        {
            return self.fallback_or_violation(node, "complex SVG text layout", None);
        }
        if text.chunks().len() != 1 {
            return self.fallback_or_violation(node, "multi-chunk SVG text", None);
        }
        let mut content = String::new();
        let mut first_span = None;
        let mut align = TextAlign::Left;
        for chunk in text.chunks() {
            content.push_str(chunk.text());
            align = match chunk.anchor() {
                usvg::TextAnchor::Start => TextAlign::Left,
                usvg::TextAnchor::Middle => TextAlign::Center,
                usvg::TextAnchor::End => TextAlign::Right,
            };
            if first_span.is_none() {
                first_span = chunk.spans().first();
            }
            if chunk.spans().len() != 1 {
                return self.fallback_or_violation(node, "multi-style SVG text", None);
            }
        }
        if content.is_empty() {
            return Ok(());
        }
        let span = first_span.ok_or(ConversionError::NormalizationFailed {
            category: "text has no style span",
        })?;
        let transform = text.abs_transform();
        if !target_font_supports(&content)
            || transform.kx.abs() > 1.0e-6
            || transform.ky.abs() > 1.0e-6
            || transform.sx <= 0.0
            || transform.sy <= 0.0
        {
            return self.fallback_or_violation(
                node,
                "unsupported glyph coverage or text transform",
                None,
            );
        }
        if !self.accept_text_font(node, span)? {
            return Ok(());
        }
        let color = solid_fill_color(span.fill()).ok_or(ConversionError::NormalizationFailed {
            category: "text paint is not a solid color",
        })?;
        let bbox = text.abs_bounding_box();
        let x_scale = f64::from(transform.sx).hypot(f64::from(transform.ky));
        let font_size = f64::from(span.font_size().get()) * x_scale * self.scene_scale;
        let fill_opacity = span
            .fill()
            .map_or(1.0, |fill| f64::from(fill.opacity().get()));
        let style = ElementStyle {
            stroke_color: color,
            background_color: ExcalidrawColor::transparent(),
            stroke_width: 1.0,
            stroke_style: StrokeStyle::Solid,
            opacity: opacity_percent(opacity * fill_opacity),
            roundness: None,
        };
        let base = self.make_base(
            "text",
            f64::from(bbox.x()) * self.scene_scale,
            f64::from(bbox.y()) * self.scene_scale,
            f64::from(bbox.width()) * self.scene_scale,
            f64::from(bbox.height()) * self.scene_scale,
            style,
        )?;
        let font_size = Finite::length(font_size, self.options.limits.max_element_extent())?;
        let line_height = Finite::length(
            self.options.fonts.target_line_height(),
            self.options.limits.max_element_extent(),
        )?;
        self.push_element(ExcalidrawElement::text(
            base,
            font_size,
            content,
            align,
            line_height,
        ))
    }

    fn accept_text_font(
        &mut self,
        node: &usvg::Node,
        span: &usvg::TextSpan,
    ) -> Result<bool, ConversionError> {
        let exact_font = is_exact_target_font(span);
        let exact_style = is_exact_target_font_style(span);
        if (!exact_font || !exact_style)
            && matches!(
                self.options.profile,
                ConversionProfile::Fidelity | ConversionProfile::Strict
            )
        {
            self.fallback_or_violation(node, "non-native target font metrics", None)?;
            return Ok(false);
        }
        if !exact_font {
            self.diagnostics.push(ConversionDiagnostic::new(
                DiagnosticCode::FontSubstituted,
                DiagnosticSeverity::Approximation,
                self.source_order,
                "source text style was mapped to target-compatible Liberation Sans",
            ));
        }
        if !exact_style {
            self.diagnostics.push(ConversionDiagnostic::new(
                DiagnosticCode::FontStyleApproximated,
                DiagnosticSeverity::Approximation,
                self.source_order,
                "source font weight, style, stretch, or spacing was approximated",
            ));
        }
        Ok(true)
    }

    fn fallback_or_violation(
        &mut self,
        node: &usvg::Node,
        _reason: &'static str,
        strict_code: Option<DiagnosticCode>,
    ) -> Result<(), ConversionError> {
        if self.options.profile == ConversionProfile::Strict {
            self.diagnostics.push(ConversionDiagnostic::new(
                strict_code.unwrap_or(DiagnosticCode::PaintIslandRasterized),
                DiagnosticSeverity::Fallback,
                self.source_order,
                "painted SVG content is not exactly representable by the strict target profile",
            ));
            return Ok(());
        }
        if self.clipped_fallback_bounds(node).is_none() {
            return Ok(());
        }
        self.rasterize_node(
            node,
            strict_code.unwrap_or(DiagnosticCode::PaintIslandRasterized),
        )
    }

    fn rasterize_node(
        &mut self,
        node: &usvg::Node,
        diagnostic_code: DiagnosticCode,
    ) -> Result<(), ConversionError> {
        let scale = self.options.raster.fallback_scale() * self.scene_scale;
        let bbox = self
            .raster_bounds(node, scale)
            .ok_or(ConversionError::RasterizationFailed {
                category: "empty fallback bounds",
            })?;
        let width = f64::from(bbox.width()) * scale;
        let height = f64::from(bbox.height()) * scale;
        let pixel_width = checked_pixel_extent(width)?;
        let pixel_height = checked_pixel_extent(height)?;
        let pixels = u64::from(pixel_width)
            .checked_mul(u64::from(pixel_height))
            .ok_or(ConversionError::LimitExceeded {
                resource: LimitResource::RasterPixels,
                limit: self.options.limits.max_raster_pixels_per_island(),
            })?;
        let aggregate = self.reserve_fallback_pixels(pixels)?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(pixel_width, pixel_height).ok_or(
            ConversionError::RasterizationFailed {
                category: "invalid fallback pixmap dimensions",
            },
        )?;
        let render_scale = raster_scale_to_f32(scale)?;
        render_fallback_node(node, bbox, render_scale, &mut pixmap)?;
        let png = pixmap
            .encode_png()
            .map_err(|_| ConversionError::RasterizationFailed {
                category: "PNG encoding",
            })?;
        self.emit_png(
            bbox,
            &png,
            aggregate,
            Some((diagnostic_code, raster_diagnostic_message(diagnostic_code))),
        )
    }

    fn rasterize_source_image(&mut self, node: &usvg::Node) -> Result<(), ConversionError> {
        let scale = self.options.raster.fallback_scale() * self.scene_scale;
        let bbox = self
            .raster_bounds(node, scale)
            .ok_or(ConversionError::RasterizationFailed {
                category: "empty source image bounds",
            })?;
        let pixel_width = checked_pixel_extent(f64::from(bbox.width()) * scale)?;
        let pixel_height = checked_pixel_extent(f64::from(bbox.height()) * scale)?;
        let pixels = u64::from(pixel_width)
            .checked_mul(u64::from(pixel_height))
            .ok_or(ConversionError::LimitExceeded {
                resource: LimitResource::RasterPixels,
                limit: self.options.limits.max_raster_pixels_per_island(),
            })?;
        let aggregate = self.reserve_fallback_pixels(pixels)?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(pixel_width, pixel_height).ok_or(
            ConversionError::RasterizationFailed {
                category: "invalid source image pixmap dimensions",
            },
        )?;
        render_fallback_node(node, bbox, raster_scale_to_f32(scale)?, &mut pixmap)?;
        let png = pixmap
            .encode_png()
            .map_err(|_| ConversionError::RasterizationFailed {
                category: "PNG encoding",
            })?;
        self.emit_png(bbox, &png, aggregate, None)
    }

    fn clipped_fallback_bounds(&self, node: &usvg::Node) -> Option<usvg::NonZeroRect> {
        fallback_bounds(node)?
            .to_rect()
            .intersect(&self.source_viewport.to_rect())?
            .to_non_zero_rect()
    }

    fn raster_bounds(&self, node: &usvg::Node, scale: f64) -> Option<usvg::NonZeroRect> {
        let clipped = self.clipped_fallback_bounds(node)?;
        let viewport = self.source_viewport;
        let left = ((f64::from(clipped.left()) * scale).floor() - ANTIALIAS_MARGIN_PIXELS) / scale;
        let top = ((f64::from(clipped.top()) * scale).floor() - ANTIALIAS_MARGIN_PIXELS) / scale;
        let right = ((f64::from(clipped.right()) * scale).ceil() + ANTIALIAS_MARGIN_PIXELS) / scale;
        let bottom =
            ((f64::from(clipped.bottom()) * scale).ceil() + ANTIALIAS_MARGIN_PIXELS) / scale;
        let x = left.max(f64::from(viewport.left()));
        let y = top.max(f64::from(viewport.top()));
        let bounded_right = right.min(f64::from(viewport.right()));
        let bounded_bottom = bottom.min(f64::from(viewport.bottom()));
        let width = finite_f32(bounded_right - x)?;
        let height = finite_f32(bounded_bottom - y)?;
        usvg::NonZeroRect::from_xywh(finite_f32(x)?, finite_f32(y)?, width, height)
    }

    fn emit_png(
        &mut self,
        bbox: usvg::NonZeroRect,
        png: &[u8],
        aggregate_pixels: u64,
        diagnostic: Option<(DiagnosticCode, &'static str)>,
    ) -> Result<(), ConversionError> {
        let file_id = FileId::new(file_id(png)?);
        let data_url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        );
        let additional_bytes = if self.files.contains_key(&file_id) {
            0
        } else {
            data_url.len()
        };
        let new_embedded = self.reserve_embedded_bytes(additional_bytes)?;
        let style = ElementStyle {
            stroke_color: ExcalidrawColor::transparent(),
            background_color: ExcalidrawColor::transparent(),
            stroke_width: 0.0,
            stroke_style: StrokeStyle::Solid,
            opacity: 100,
            roundness: None,
        };
        let base = self.make_base(
            "image",
            f64::from(bbox.x()) * self.scene_scale,
            f64::from(bbox.y()) * self.scene_scale,
            f64::from(bbox.width()) * self.scene_scale,
            f64::from(bbox.height()) * self.scene_scale,
            style,
        )?;
        let element_id = base.id().as_str().to_owned();
        self.files
            .entry(file_id.clone())
            .or_insert_with(|| BinaryFile::png(file_id.clone(), data_url));
        self.push_element(ExcalidrawElement::image(base, file_id))?;
        self.fallback_pixels = aggregate_pixels;
        self.embedded_bytes = new_embedded;
        if let Some((diagnostic_code, message)) = diagnostic {
            self.diagnostics.push(
                ConversionDiagnostic::new(
                    diagnostic_code,
                    DiagnosticSeverity::Fallback,
                    self.source_order,
                    message,
                )
                .with_target(&element_id),
            );
        }
        Ok(())
    }

    fn reserve_fallback_pixels(&self, pixels: u64) -> Result<u64, ConversionError> {
        if pixels > self.options.limits.max_raster_pixels_per_island() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::RasterPixels,
                limit: self.options.limits.max_raster_pixels_per_island(),
            });
        }
        let aggregate =
            self.fallback_pixels
                .checked_add(pixels)
                .ok_or(ConversionError::LimitExceeded {
                    resource: LimitResource::RasterPixels,
                    limit: self.options.limits.max_raster_pixels(),
                })?;
        if aggregate > self.options.limits.max_raster_pixels() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::RasterPixels,
                limit: self.options.limits.max_raster_pixels(),
            });
        }
        Ok(aggregate)
    }

    fn reserve_embedded_bytes(&self, bytes: usize) -> Result<usize, ConversionError> {
        let limit_value = self.options.limits.max_embedded_output_bytes();
        let aggregate =
            self.embedded_bytes
                .checked_add(bytes)
                .ok_or(ConversionError::LimitExceeded {
                    resource: LimitResource::EmbeddedBytes,
                    limit: usize_to_u64(limit_value),
                })?;
        if aggregate > limit_value {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::EmbeddedBytes,
                limit: usize_to_u64(limit_value),
            });
        }
        Ok(aggregate)
    }

    fn make_base(
        &self,
        role: &str,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        style: ElementStyle,
    ) -> Result<ElementBase, ConversionError> {
        let occurrence =
            u32::try_from(self.elements.len()).map_err(|_| ConversionError::LimitExceeded {
                resource: LimitResource::TargetElements,
                limit: usize_to_u64(self.options.limits.max_target_elements()),
            })?;
        let (id, identity) = element_identity(self.digest, self.source_order, occurrence, role)?;
        let mut base = ElementBase::new(
            id,
            identity,
            x,
            y,
            width,
            height,
            0.0,
            style,
            &self.options.limits,
        )?;
        if self.options.provenance == ProvenanceMode::Compact {
            let (mapping, diagnostic_codes) = if role == "image" {
                ("fallback", vec!["paint-island-rasterized".to_owned()])
            } else {
                ("native", Vec::new())
            };
            base.set_provenance(
                self.source_order,
                &self.current_source_tag,
                mapping,
                diagnostic_codes,
            );
        }
        Ok(base)
    }

    fn push_element(&mut self, element: ExcalidrawElement) -> Result<(), ConversionError> {
        if self.elements.len() >= self.options.limits.max_target_elements() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::TargetElements,
                limit: usize_to_u64(self.options.limits.max_target_elements()),
            });
        }
        if !self.element_ids.insert(element.id().to_owned()) {
            return Err(ConversionError::InvalidGeneratedDocument {
                category: "deterministic element ID collision",
            });
        }
        self.element_groups.push(self.active_groups.clone());
        self.elements.push(element);
        Ok(())
    }

    fn finalize_groups(&mut self) -> Result<(), ConversionError> {
        let mut counts = BTreeMap::<GroupKey, usize>::new();
        for groups in &self.element_groups {
            for key in groups {
                *counts.entry(*key).or_default() += 1;
            }
        }
        let mut target_ids = BTreeMap::<GroupKey, GroupId>::new();
        for (key, count) in &counts {
            if *count >= 2 {
                target_ids.insert(
                    *key,
                    GroupId::new(group_id(self.digest, key.source_order, key.instance)?),
                );
            }
        }
        for (element, groups) in self.elements.iter_mut().zip(&self.element_groups) {
            let group_ids = groups
                .iter()
                .rev()
                .filter_map(|key| target_ids.get(key).cloned())
                .collect();
            element.set_group_ids(group_ids);
        }
        Ok(())
    }

    fn reserve_points(&mut self, count: usize) -> Result<(), ConversionError> {
        self.target_points =
            self.target_points
                .checked_add(count)
                .ok_or(ConversionError::LimitExceeded {
                    resource: LimitResource::TargetPoints,
                    limit: usize_to_u64(self.options.limits.max_target_points()),
                })?;
        if self.target_points > self.options.limits.max_target_points() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::TargetPoints,
                limit: usize_to_u64(self.options.limits.max_target_points()),
            });
        }
        Ok(())
    }
}

fn path_has_only_solid_paint(path: &usvg::Path) -> bool {
    path.fill()
        .is_none_or(|fill| matches!(fill.paint(), usvg::Paint::Color(_)))
        && path
            .stroke()
            .is_none_or(|stroke| matches!(stroke.paint(), usvg::Paint::Color(_)))
}

fn marker_paint_matches(path: &usvg::Path, metadata: &SourceNodeMetadata) -> bool {
    let Some(usvg::Paint::Color(color)) = path.stroke().map(usvg::Stroke::paint) else {
        return false;
    };
    let actual = [color.red, color.green, color.blue];
    metadata
        .marker_start_color
        .is_none_or(|expected| expected == actual)
        && metadata
            .marker_end_color
            .is_none_or(|expected| expected == actual)
}

fn marker_artwork_matches(
    artwork: &usvg::Group,
    connector: &usvg::Path,
    metadata: &SourceNodeMetadata,
) -> bool {
    let expected_paths = usize::from(metadata.marker_start.is_some())
        .saturating_add(usize::from(metadata.marker_end.is_some()));
    if expected_paths == 0 || !marker_paint_matches(connector, metadata) {
        return false;
    }
    let Some(usvg::Paint::Color(connector_color)) = connector.stroke().map(usvg::Stroke::paint)
    else {
        return false;
    };
    let mut path_count = 0_usize;
    marker_group_has_native_paint(artwork, *connector_color, &mut path_count)
        && path_count == expected_paths
}

fn marker_group_has_native_paint(
    group: &usvg::Group,
    expected_color: usvg::Color,
    path_count: &mut usize,
) -> bool {
    if (group.opacity().get() - 1.0).abs() > f32::EPSILON
        || group.blend_mode() != usvg::BlendMode::Normal
        || group.isolate()
        || group.mask().is_some()
        || !group.filters().is_empty()
    {
        return false;
    }
    for child in group.children() {
        match child {
            usvg::Node::Group(nested) => {
                if !marker_group_has_native_paint(nested, expected_color, path_count) {
                    return false;
                }
            }
            usvg::Node::Path(path) => {
                let Some(fill) = path.fill() else {
                    return false;
                };
                if path.stroke().is_some()
                    || (fill.opacity().get() - 1.0).abs() > f32::EPSILON
                    || !matches!(fill.paint(), usvg::Paint::Color(color) if *color == expected_color)
                {
                    return false;
                }
                *path_count = path_count.saturating_add(1);
            }
            usvg::Node::Image(_) | usvg::Node::Text(_) => return false,
        }
    }
    true
}

fn adaptive_target_radius(minimum_dimension: f64, configured_radius: f64) -> f64 {
    const PROPORTIONAL_RADIUS: f64 = 0.25;

    if minimum_dimension <= configured_radius / PROPORTIONAL_RADIUS {
        minimum_dimension * PROPORTIONAL_RADIUS
    } else {
        configured_radius
    }
}

fn compound_subpaths_are_disjoint(subpaths: &[Subpath], options: &ConversionOptions) -> bool {
    if subpaths.iter().any(|subpath| {
        subpath.points.len() < 3
            || points_are_collinear(&subpath.points)
            || !is_simple_fill_boundary(subpath, options)
    }) {
        return false;
    }
    let mut work = 0_usize;
    for (left_index, left) in subpaths.iter().enumerate() {
        for right in subpaths.iter().skip(left_index.saturating_add(1)) {
            let Some(pair_work) = left.points.len().checked_mul(right.points.len()) else {
                return false;
            };
            let Some(next_work) = work.checked_add(pair_work) else {
                return false;
            };
            if next_work > options.limits.max_path_segments() {
                return false;
            }
            work = next_work;
        }
    }
    for (left_index, left) in subpaths.iter().enumerate() {
        for right in subpaths.iter().skip(left_index.saturating_add(1)) {
            if polygons_intersect_or_contain(&left.points, &right.points) {
                return false;
            }
        }
    }
    true
}

fn polygons_intersect_or_contain(left: &[Point], right: &[Point]) -> bool {
    let Some(left_first) = left.first().copied() else {
        return true;
    };
    let Some(right_first) = right.first().copied() else {
        return true;
    };
    let left_edges = polygon_edges(left);
    let right_edges = polygon_edges(right);
    left_edges.iter().any(|(left_start, left_end)| {
        right_edges.iter().any(|(right_start, right_end)| {
            segments_intersect(*left_start, *left_end, *right_start, *right_end)
        })
    }) || point_in_polygon(left_first, right)
        || point_in_polygon(right_first, left)
}

fn polygon_edges(points: &[Point]) -> Vec<(Point, Point)> {
    let Some(last) = points.last().copied() else {
        return Vec::new();
    };
    points
        .windows(2)
        .filter_map(|pair| Some((*pair.first()?, *pair.get(1)?)))
        .chain(points.first().copied().map(|first| (last, first)))
        .collect()
}

fn point_in_polygon(point: Point, polygon: &[Point]) -> bool {
    polygon_edges(polygon)
        .into_iter()
        .fold(false, |inside, (start, end)| {
            let crosses = (start.y > point.y) != (end.y > point.y)
                && point.x < (end.x - start.x) * (point.y - start.y) / (end.y - start.y) + start.x;
            inside != crosses
        })
}

fn raster_diagnostic_message(code: DiagnosticCode) -> &'static str {
    match code {
        DiagnosticCode::GradientRasterized => "a gradient or pattern paint island was rasterized",
        DiagnosticCode::MaskRasterized => "a complete masked paint island was rasterized",
        DiagnosticCode::ClipRasterized => "a complete clipped paint island was rasterized",
        _ => "smallest complete unsupported paint island was rasterized",
    }
}

fn fallback_bounds(node: &usvg::Node) -> Option<usvg::NonZeroRect> {
    node.abs_layer_bounding_box().or_else(|| match node {
        usvg::Node::Path(path) if path.stroke().is_some() => {
            path.abs_stroke_bounding_box().to_non_zero_rect()
        }
        usvg::Node::Text(text) => text.abs_stroke_bounding_box().to_non_zero_rect(),
        usvg::Node::Group(_) | usvg::Node::Path(_) | usvg::Node::Image(_) => None,
    })
}

fn render_fallback_node(
    node: &usvg::Node,
    bbox: usvg::NonZeroRect,
    scale: f32,
    pixmap: &mut resvg::tiny_skia::Pixmap,
) -> Result<(), ConversionError> {
    let render_transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    let is_zero_area_path = matches!(
        node,
        usvg::Node::Path(path)
            if path.data().bounds().width() == 0.0 || path.data().bounds().height() == 0.0
    );
    if let Some(original_bbox) = node.abs_layer_bounding_box()
        && !is_zero_area_path
    {
        let adjusted_transform = render_transform
            .pre_translate(original_bbox.x() - bbox.x(), original_bbox.y() - bbox.y());
        return resvg::render_node(node, adjusted_transform, &mut pixmap.as_mut()).ok_or(
            ConversionError::RasterizationFailed {
                category: "fallback node is not renderable",
            },
        );
    }
    let usvg::Node::Path(path) = node else {
        return Err(ConversionError::RasterizationFailed {
            category: "zero-area fallback node is not a stroked path",
        });
    };
    let Some(stroke) = path.stroke() else {
        return Err(ConversionError::RasterizationFailed {
            category: "zero-area fallback path has no stroke",
        });
    };
    let usvg::Paint::Color(color) = stroke.paint() else {
        return Err(ConversionError::RasterizationFailed {
            category: "zero-area fallback path has non-solid stroke paint",
        });
    };
    let mut paint = resvg::tiny_skia::Paint::default();
    paint.set_color_rgba8(color.red, color.green, color.blue, stroke.opacity().to_u8());
    paint.anti_alias = path.rendering_mode().use_shape_antialiasing();
    let absolute = path.abs_transform();
    let transform = Transform::from_row(
        scale * absolute.sx,
        scale * absolute.ky,
        scale * absolute.kx,
        scale * absolute.sy,
        scale * (absolute.tx - bbox.x()),
        scale * (absolute.ty - bbox.y()),
    );
    pixmap
        .as_mut()
        .stroke_path(path.data(), &paint, &stroke.to_tiny_skia(), transform, None);
    Ok(())
}

fn image_is_animated(kind: &usvg::ImageKind) -> bool {
    match kind {
        usvg::ImageKind::GIF(bytes) => gif_frame_count(bytes) > 1,
        usvg::ImageKind::PNG(bytes) => png_has_animation_control(bytes),
        usvg::ImageKind::WEBP(bytes) => webp_has_animation(bytes),
        usvg::ImageKind::JPEG(_) | usvg::ImageKind::SVG(_) => false,
    }
}

fn gif_frame_count(bytes: &[u8]) -> usize {
    if !bytes.starts_with(b"GIF87a") && !bytes.starts_with(b"GIF89a") {
        return 0;
    }
    let Some(packed) = bytes.get(10).copied() else {
        return 0;
    };
    let global_table_bytes = if packed & 0x80 == 0 {
        0
    } else {
        3_usize.saturating_mul(1_usize << (usize::from(packed & 0x07) + 1))
    };
    let Some(mut position) = 13_usize.checked_add(global_table_bytes) else {
        return 0;
    };
    let mut frames = 0_usize;
    while let Some(introducer) = bytes.get(position).copied() {
        match introducer {
            0x2c => {
                frames = frames.saturating_add(1);
                if frames > 1 {
                    return frames;
                }
                let Some(local_packed_position) = position.checked_add(9) else {
                    return 0;
                };
                let Some(local_packed) = bytes.get(local_packed_position).copied() else {
                    return 0;
                };
                let local_table_bytes = if local_packed & 0x80 == 0 {
                    0
                } else {
                    3_usize.saturating_mul(1_usize << (usize::from(local_packed & 0x07) + 1))
                };
                let Some(data_start) = position
                    .checked_add(11)
                    .and_then(|value| value.checked_add(local_table_bytes))
                else {
                    return 0;
                };
                position = data_start;
                if !skip_image_sub_blocks(bytes, &mut position) {
                    return 0;
                }
            }
            0x21 => {
                let Some(block_start) = position.checked_add(2) else {
                    return 0;
                };
                position = block_start;
                if !skip_image_sub_blocks(bytes, &mut position) {
                    return 0;
                }
            }
            0x3b => return frames,
            _ => return 0,
        }
    }
    0
}

fn png_has_animation_control(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return false;
    }
    let mut position = 8_usize;
    while let Some(header_end) = position.checked_add(8) {
        let Some(header) = bytes.get(position..header_end) else {
            return false;
        };
        let Some(length_bytes) = header.get(..4) else {
            return false;
        };
        let Ok(length_bytes) = <[u8; 4]>::try_from(length_bytes) else {
            return false;
        };
        let Ok(length) = usize::try_from(u32::from_be_bytes(length_bytes)) else {
            return false;
        };
        let Some(kind) = header.get(4..8) else {
            return false;
        };
        if kind == b"acTL" {
            return true;
        }
        let Some(next) = header_end
            .checked_add(length)
            .and_then(|value| value.checked_add(4))
        else {
            return false;
        };
        if next > bytes.len() || kind == b"IEND" {
            return false;
        }
        position = next;
    }
    false
}

fn webp_has_animation(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"RIFF") || bytes.get(8..12) != Some(b"WEBP".as_slice()) {
        return false;
    }
    let mut position = 12_usize;
    while let Some(header_end) = position.checked_add(8) {
        let Some(header) = bytes.get(position..header_end) else {
            return false;
        };
        let Some(kind) = header.get(..4) else {
            return false;
        };
        if kind == b"ANIM" || kind == b"ANMF" {
            return true;
        }
        let Some(length_bytes) = header.get(4..8) else {
            return false;
        };
        let Ok(length_bytes) = <[u8; 4]>::try_from(length_bytes) else {
            return false;
        };
        let Ok(length) = usize::try_from(u32::from_le_bytes(length_bytes)) else {
            return false;
        };
        let padded_length = length.saturating_add(length & 1);
        let Some(next) = header_end.checked_add(padded_length) else {
            return false;
        };
        if next > bytes.len() {
            return false;
        }
        position = next;
    }
    false
}

fn skip_image_sub_blocks(bytes: &[u8], position: &mut usize) -> bool {
    loop {
        let Some(length) = bytes.get(*position).copied().map(usize::from) else {
            return false;
        };
        let Some(data_start) = (*position).checked_add(1) else {
            return false;
        };
        let Some(next) = data_start.checked_add(length) else {
            return false;
        };
        if next > bytes.len() {
            return false;
        }
        *position = next;
        if length == 0 {
            return true;
        }
    }
}

fn stroke_transform_is_scalar(path: &usvg::Path) -> bool {
    if path.stroke().is_none() {
        return true;
    }
    let transform = path.abs_transform();
    let x_scale = f64::from(transform.sx).hypot(f64::from(transform.ky));
    let y_scale = f64::from(transform.kx).hypot(f64::from(transform.sy));
    (x_scale - y_scale).abs() <= 1.0e-6 && x_scale.is_finite() && x_scale > 0.0
}

fn stroke_is_exact_native(path: &usvg::Path) -> bool {
    path.stroke().is_none_or(|stroke| {
        stroke.dasharray().is_none()
            && stroke.dashoffset() == 0.0
            && stroke.linecap() == usvg::LineCap::Round
            && stroke.linejoin() == usvg::LineJoin::Round
    })
}

fn is_exact_target_font(span: &usvg::TextSpan) -> bool {
    matches!(
        span.font().families().first(),
        Some(usvg::FontFamily::Named(name)) if name.eq_ignore_ascii_case("Liberation Sans")
    ) && is_exact_target_font_style(span)
}

fn is_exact_target_font_style(span: &usvg::TextSpan) -> bool {
    span.font().style() == usvg::FontStyle::Normal
        && span.font().stretch() == usvg::FontStretch::Normal
        && span.font().weight() == 400
        && !span.small_caps()
        && span.letter_spacing() == 0.0
        && span.word_spacing() == 0.0
}

fn target_font_supports(text: &str) -> bool {
    use skrifa::MetadataProvider as _;

    static TARGET_FACE: OnceLock<Option<skrifa::FontRef<'static>>> = OnceLock::new();

    TARGET_FACE
        .get_or_init(|| skrifa::FontRef::new(crate::ingest::bundled_target_font()).ok())
        .as_ref()
        .is_some_and(|face| {
            text.chars().all(|character| {
                character.is_whitespace() || face.charmap().map(character).is_some()
            })
        })
}

fn paint_roles(path: &usvg::Path, can_combine: bool) -> Vec<PaintRole> {
    match (path.fill(), path.stroke()) {
        (Some(fill), Some(stroke))
            if can_combine
                && path.paint_order() == usvg::PaintOrder::FillAndStroke
                && (f64::from(fill.opacity().get()) - f64::from(stroke.opacity().get())).abs()
                    <= 0.005 =>
        {
            vec![PaintRole::Combined]
        }
        (Some(_), Some(_)) => match path.paint_order() {
            usvg::PaintOrder::FillAndStroke => vec![PaintRole::Fill, PaintRole::Stroke],
            usvg::PaintOrder::StrokeAndFill => vec![PaintRole::Stroke, PaintRole::Fill],
        },
        (Some(_), None) => vec![PaintRole::Fill],
        (None, Some(_)) => vec![PaintRole::Stroke],
        (None, None) => Vec::new(),
    }
}

fn is_simple_fill_boundary(subpath: &Subpath, options: &ConversionOptions) -> bool {
    const INTERSECTION_WORK_CAP: usize = 4_000_000;

    let points = &subpath.points;
    if points.len() < 3 || points_are_collinear(points) {
        return true;
    }
    let segment_count = points.len();
    if segment_count
        .checked_mul(segment_count)
        .is_none_or(|work| work > INTERSECTION_WORK_CAP)
        || segment_count > options.limits.max_decomposition_elements()
    {
        return false;
    }
    let Some(first) = points.first().copied() else {
        return false;
    };
    let Some(last) = points.last().copied() else {
        return false;
    };
    let edges = points
        .windows(2)
        .filter_map(|pair| Some((*pair.first()?, *pair.get(1)?)))
        .chain(std::iter::once((last, first)))
        .collect::<Vec<_>>();
    for (left, (left_start, left_end)) in edges.iter().copied().enumerate() {
        for right in (left + 1)..edges.len() {
            if right == left + 1 || (left == 0 && right + 1 == segment_count) {
                continue;
            }
            let Some((right_start, right_end)) = edges.get(right).copied() else {
                return false;
            };
            if segments_intersect(left_start, left_end, right_start, right_end) {
                return false;
            }
        }
    }
    true
}

fn points_are_collinear(points: &[Point]) -> bool {
    let Some(first) = points.first().copied() else {
        return true;
    };
    let Some(second) = points
        .iter()
        .copied()
        .find(|point| !approximately_same(first, *point))
    else {
        return true;
    };
    points
        .iter()
        .copied()
        .all(|point| orientation(first, second, point).abs() <= 1.0e-9)
}

fn segments_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    let ab_c = orientation(a, b, c);
    let ab_d = orientation(a, b, d);
    let cd_a = orientation(c, d, a);
    let cd_b = orientation(c, d, b);
    let epsilon = 1.0e-9;

    if ab_c.abs() <= epsilon && point_on_segment(c, a, b, epsilon)
        || ab_d.abs() <= epsilon && point_on_segment(d, a, b, epsilon)
        || cd_a.abs() <= epsilon && point_on_segment(a, c, d, epsilon)
        || cd_b.abs() <= epsilon && point_on_segment(b, c, d, epsilon)
    {
        return true;
    }
    (ab_c > epsilon) != (ab_d > epsilon) && (cd_a > epsilon) != (cd_b > epsilon)
}

fn orientation(a: Point, b: Point, c: Point) -> f64 {
    (b.x - a.x).mul_add(c.y - a.y, -((b.y - a.y) * (c.x - a.x)))
}

fn point_on_segment(point: Point, start: Point, end: Point, epsilon: f64) -> bool {
    point.x >= start.x.min(end.x) - epsilon
        && point.x <= start.x.max(end.x) + epsilon
        && point.y >= start.y.min(end.y) - epsilon
        && point.y <= start.y.max(end.y) + epsilon
}

fn path_style(
    path: &usvg::Path,
    role: PaintRole,
    inherited_opacity: f64,
    scene_scale: f64,
) -> ElementStyle {
    let fill = if matches!(role, PaintRole::Combined | PaintRole::Fill) {
        path.fill()
    } else {
        None
    };
    let stroke = if matches!(role, PaintRole::Combined | PaintRole::Stroke) {
        path.stroke()
    } else {
        None
    };
    let background_color = fill
        .and_then(|paint| solid_paint_color(paint.paint()))
        .unwrap_or_else(ExcalidrawColor::transparent);
    let stroke_color = stroke
        .and_then(|paint| solid_paint_color(paint.paint()))
        .unwrap_or_else(ExcalidrawColor::transparent);
    let transform = path.abs_transform();
    let transform_scale = f64::from(transform.sx).hypot(f64::from(transform.ky));
    let stroke_width = stroke.map_or(0.0, |paint| {
        f64::from(paint.width().get()) * transform_scale * scene_scale
    });
    let alpha = fill
        .map(|paint| f64::from(paint.opacity().get()))
        .or_else(|| stroke.map(|paint| f64::from(paint.opacity().get())))
        .unwrap_or(1.0);
    ElementStyle {
        stroke_color,
        background_color,
        stroke_width,
        stroke_style: classify_dash(stroke),
        opacity: opacity_percent(inherited_opacity * alpha),
        roundness: None,
    }
}

fn classify_dash(stroke: Option<&usvg::Stroke>) -> StrokeStyle {
    let Some(stroke) = stroke else {
        return StrokeStyle::Solid;
    };
    let Some(dashes) = stroke.dasharray() else {
        return StrokeStyle::Solid;
    };
    if stroke.linecap() == usvg::LineCap::Round
        && dashes
            .iter()
            .step_by(2)
            .all(|dash| *dash <= stroke.width().get())
    {
        StrokeStyle::Dotted
    } else {
        StrokeStyle::Dashed
    }
}

fn solid_paint_color(paint: &usvg::Paint) -> Option<ExcalidrawColor> {
    match paint {
        usvg::Paint::Color(color) => Some(ExcalidrawColor::rgb(color.red, color.green, color.blue)),
        _ => None,
    }
}

fn solid_fill_color(fill: Option<&usvg::Fill>) -> Option<ExcalidrawColor> {
    fill.and_then(|paint| solid_paint_color(paint.paint()))
}

fn opacity_percent(opacity: f64) -> u8 {
    let percent = (opacity.clamp(0.0, 1.0) * 100.0).round_ties_even();
    if percent <= 0.0 {
        0
    } else if percent >= 100.0 {
        100
    } else {
        bounded_percent_to_u8(percent)
    }
}

fn looks_like_axis_aligned_ellipse(path: &usvg::Path) -> bool {
    let mut moves = 0_usize;
    let mut cubics = 0_usize;
    let mut closes = 0_usize;
    let mut others = 0_usize;
    for segment in path.data().segments() {
        match segment {
            PathSegment::MoveTo(_) => moves = moves.saturating_add(1),
            PathSegment::CubicTo(..) => cubics = cubics.saturating_add(1),
            PathSegment::Close => closes = closes.saturating_add(1),
            PathSegment::LineTo(_) | PathSegment::QuadTo(..) => {
                others = others.saturating_add(1);
            }
        }
    }
    moves == 1 && cubics == 4 && closes == 1 && others == 0 && !path.abs_transform().has_skew()
}

fn flatten_path(
    path: &usvg::Path,
    tolerance: f64,
    scene_scale: f64,
    max_points: usize,
) -> Result<Vec<Subpath>, ConversionError> {
    let transform = path.abs_transform();
    let mut output = Vec::new();
    let mut current: Option<Subpath> = None;
    for segment in path.data().segments() {
        match segment {
            PathSegment::MoveTo(point) => {
                finish_subpath(&mut current, &mut output);
                current = Some(Subpath {
                    points: vec![Point::transformed(point, transform, scene_scale)],
                    closed: false,
                    curved: false,
                });
            }
            PathSegment::LineTo(point) => {
                let point = Point::transformed(point, transform, scene_scale);
                push_distinct(current.as_mut(), point, max_points)?;
            }
            PathSegment::QuadTo(control, end) => {
                let subpath = current.as_mut().ok_or(ConversionError::GeometryOverflow)?;
                let start = *subpath
                    .points
                    .last()
                    .ok_or(ConversionError::GeometryOverflow)?;
                let control = Point::transformed(control, transform, scene_scale);
                let end = Point::transformed(end, transform, scene_scale);
                flatten_quad(
                    start,
                    control,
                    end,
                    tolerance,
                    0,
                    &mut subpath.points,
                    max_points,
                )?;
                subpath.curved = true;
            }
            PathSegment::CubicTo(control1, control2, end) => {
                let subpath = current.as_mut().ok_or(ConversionError::GeometryOverflow)?;
                let start = *subpath
                    .points
                    .last()
                    .ok_or(ConversionError::GeometryOverflow)?;
                let control1 = Point::transformed(control1, transform, scene_scale);
                let control2 = Point::transformed(control2, transform, scene_scale);
                let end = Point::transformed(end, transform, scene_scale);
                flatten_cubic(
                    start,
                    control1,
                    control2,
                    end,
                    tolerance,
                    0,
                    &mut subpath.points,
                    max_points,
                )?;
                subpath.curved = true;
            }
            PathSegment::Close => {
                if let Some(subpath) = current.as_mut() {
                    subpath.closed = true;
                }
            }
        }
    }
    finish_subpath(&mut current, &mut output);
    Ok(output)
}

fn finish_subpath(current: &mut Option<Subpath>, output: &mut Vec<Subpath>) {
    if let Some(mut subpath) = current.take() {
        deduplicate_points(&mut subpath.points);
        if subpath.closed
            && let (Some(first), Some(last)) = (subpath.points.first(), subpath.points.last())
            && approximately_same(*first, *last)
        {
            subpath.points.pop();
        }
        if !subpath.points.is_empty() {
            output.push(subpath);
        }
    }
}

fn push_distinct(
    subpath: Option<&mut Subpath>,
    point: Point,
    max_points: usize,
) -> Result<(), ConversionError> {
    let subpath = subpath.ok_or(ConversionError::GeometryOverflow)?;
    if subpath
        .points
        .last()
        .is_none_or(|last| !approximately_same(*last, point))
    {
        push_bounded(&mut subpath.points, point, max_points)?;
    }
    Ok(())
}

fn flatten_quad(
    start: Point,
    control: Point,
    end: Point,
    tolerance: f64,
    depth: u8,
    output: &mut Vec<Point>,
    max_points: usize,
) -> Result<(), ConversionError> {
    if distance_to_line(control, start, end) <= tolerance {
        return push_bounded(output, end, max_points);
    }
    if depth >= 20 {
        return Err(ConversionError::LimitExceeded {
            resource: LimitResource::WorkUnits,
            limit: 20,
        });
    }
    let start_control = start.midpoint(control);
    let control_end = control.midpoint(end);
    let middle = start_control.midpoint(control_end);
    flatten_quad(
        start,
        start_control,
        middle,
        tolerance,
        depth + 1,
        output,
        max_points,
    )?;
    flatten_quad(
        middle,
        control_end,
        end,
        tolerance,
        depth + 1,
        output,
        max_points,
    )
}

#[allow(clippy::too_many_arguments)]
fn flatten_cubic(
    start: Point,
    control1: Point,
    control2: Point,
    end: Point,
    tolerance: f64,
    depth: u8,
    output: &mut Vec<Point>,
    max_points: usize,
) -> Result<(), ConversionError> {
    let flatness =
        distance_to_line(control1, start, end).max(distance_to_line(control2, start, end));
    if flatness <= tolerance {
        return push_bounded(output, end, max_points);
    }
    if depth >= 20 {
        return Err(ConversionError::LimitExceeded {
            resource: LimitResource::WorkUnits,
            limit: 20,
        });
    }
    let left_edge = start.midpoint(control1);
    let center_edge = control1.midpoint(control2);
    let right_edge = control2.midpoint(end);
    let left_half = left_edge.midpoint(center_edge);
    let right_half = center_edge.midpoint(right_edge);
    let middle = left_half.midpoint(right_half);
    flatten_cubic(
        start,
        left_edge,
        left_half,
        middle,
        tolerance,
        depth + 1,
        output,
        max_points,
    )?;
    flatten_cubic(
        middle,
        right_half,
        right_edge,
        end,
        tolerance,
        depth + 1,
        output,
        max_points,
    )
}

fn push_bounded(
    output: &mut Vec<Point>,
    point: Point,
    max_points: usize,
) -> Result<(), ConversionError> {
    if !point.x.is_finite() || !point.y.is_finite() {
        return Err(ConversionError::GeometryOverflow);
    }
    if output.len() >= max_points {
        return Err(ConversionError::LimitExceeded {
            resource: LimitResource::TargetPoints,
            limit: usize_to_u64(max_points),
        });
    }
    output.push(point);
    Ok(())
}

fn distance_to_line(point: Point, start: Point, end: Point) -> f64 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let denominator = dx.hypot(dy);
    if denominator <= f64::EPSILON {
        return (point.x - start.x).hypot(point.y - start.y);
    }
    ((dy * point.x - dx * point.y + end.x * start.y - end.y * start.x).abs()) / denominator
}

fn deduplicate_points(points: &mut Vec<Point>) {
    points.dedup_by(|left, right| approximately_same(*left, *right));
}

fn approximately_same(left: Point, right: Point) -> bool {
    (left.x - right.x).abs() <= 1.0e-9 && (left.y - right.y).abs() <= 1.0e-9
}

fn bounds(points: &[Point]) -> Result<(f64, f64, f64, f64), ConversionError> {
    let first = points.first().ok_or(ConversionError::GeometryOverflow)?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x;
    let mut max_y = first.y;
    for point in points.iter().skip(1) {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    if [min_x, min_y, max_x, max_y]
        .iter()
        .any(|value| !value.is_finite())
    {
        return Err(ConversionError::GeometryOverflow);
    }
    Ok((min_x, min_y, max_x - min_x, max_y - min_y))
}

fn is_axis_aligned_rectangle(points: &[Point]) -> bool {
    if points.len() != 4 {
        return false;
    }
    let Ok((min_x, min_y, width, height)) = bounds(points) else {
        return false;
    };
    if width <= 0.0 || height <= 0.0 {
        return false;
    }
    let corners = [
        (min_x.to_bits(), min_y.to_bits()),
        ((min_x + width).to_bits(), min_y.to_bits()),
        ((min_x + width).to_bits(), (min_y + height).to_bits()),
        (min_x.to_bits(), (min_y + height).to_bits()),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    points
        .iter()
        .map(|point| (point.x.to_bits(), point.y.to_bits()))
        .collect::<BTreeSet<_>>()
        == corners
}

fn is_target_diamond(points: &[Point]) -> bool {
    if points.len() != 4 {
        return false;
    }
    let Ok((min_x, min_y, width, height)) = bounds(points) else {
        return false;
    };
    let expected = [
        ((min_x + width * 0.5).to_bits(), min_y.to_bits()),
        ((min_x + width).to_bits(), (min_y + height * 0.5).to_bits()),
        ((min_x + width * 0.5).to_bits(), (min_y + height).to_bits()),
        (min_x.to_bits(), (min_y + height * 0.5).to_bits()),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    points
        .iter()
        .map(|point| (point.x.to_bits(), point.y.to_bits()))
        .collect::<BTreeSet<_>>()
        == expected
}

fn checked_pixel_extent(value: f64) -> Result<u32, ConversionError> {
    let value = value.ceil().max(1.0);
    if !value.is_finite() || value > f64::from(u32::MAX) {
        return Err(ConversionError::GeometryOverflow);
    }
    Ok(bounded_pixel_extent_to_u32(value))
}

#[allow(clippy::cast_possible_truncation)]
fn raster_scale_to_f32(value: f64) -> Result<f32, ConversionError> {
    if !value.is_finite() || !(0.0..=4.0).contains(&value) {
        return Err(ConversionError::GeometryOverflow);
    }
    // resvg's transform is intentionally f32; policy has already bounded this value to 0..=4.
    Ok(value as f32)
}

#[allow(clippy::cast_possible_truncation)]
fn finite_f32(value: f64) -> Option<f32> {
    (value.is_finite() && value.abs() <= f64::from(f32::MAX)).then_some(value as f32)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn bounded_percent_to_u8(value: f64) -> u8 {
    debug_assert!((0.0..=100.0).contains(&value));
    value as u8
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn bounded_pixel_extent_to_u32(value: f64) -> u32 {
    debug_assert!((1.0..=f64::from(u32::MAX)).contains(&value));
    value as u32
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{
        Point, flatten_cubic, gif_frame_count, png_has_animation_control, webp_has_animation,
    };
    use crate::{ConversionOptions, convert};

    #[test]
    fn test_should_convert_basic_shapes_to_stable_json() -> Result<(), Box<dyn std::error::Error>> {
        let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">
          <rect x="1" y="2" width="20" height="10" fill="#ff0000"/>
          <circle cx="50" cy="20" r="10" fill="#00ff00"/>
          <line x1="70" y1="5" x2="90" y2="25" stroke="#0000ff"/>
        </svg>"##;
        let first = convert(input, &ConversionOptions::default())?;
        let second = convert(input, &ConversionOptions::default())?;
        assert_eq!(
            first.document.to_pretty_json()?,
            second.document.to_pretty_json()?
        );
        assert_eq!(first.document.elements().len(), 3);
        Ok(())
    }

    #[test]
    fn test_should_detect_animation_only_from_valid_container_chunks() {
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[1, 0, 1, 0, 0, 0, 0]);
        for _ in 0..2 {
            gif.push(0x2c);
            gif.extend_from_slice(&[0; 9]);
            gif.extend_from_slice(&[2, 1, 0, 0]);
        }
        gif.push(0x3b);
        assert_eq!(gif_frame_count(&gif), 2);
        assert_eq!(gif_frame_count(b"GIF89a,not-a-valid-image"), 0);

        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(b"acTL");
        png.extend_from_slice(&[0, 0, 0, 0]);
        assert!(png_has_animation_control(&png));
        assert!(!png_has_animation_control(
            b"\x89PNG\r\n\x1a\ncompressed-acTL"
        ));

        let mut webp = b"RIFF\0\0\0\0WEBP".to_vec();
        webp.extend_from_slice(b"ANIM");
        webp.extend_from_slice(&[0, 0, 0, 0]);
        assert!(webp_has_animation(&webp));
        assert!(!webp_has_animation(b"RIFF\0\0\0\0WEBPcompressed-ANIM"));
    }

    proptest! {
        #[test]
        fn test_should_not_increase_cubic_points_when_tolerance_increases(
            x1 in -100.0_f64..100.0,
            y1 in -100.0_f64..100.0,
            x2 in -100.0_f64..100.0,
            y2 in -100.0_f64..100.0,
            tolerance in 0.05_f64..4.0,
        ) {
            let start = Point { x: 0.0, y: 0.0 };
            let control1 = Point { x: x1, y: y1 };
            let control2 = Point { x: x2, y: y2 };
            let end = Point { x: 100.0, y: 0.0 };
            let mut finer = vec![start];
            let mut coarser = vec![start];
            let fine_result = flatten_cubic(
                start,
                control1,
                control2,
                end,
                tolerance,
                0,
                &mut finer,
                100_000,
            );
            let coarse_result = flatten_cubic(
                start,
                control1,
                control2,
                end,
                tolerance * 2.0,
                0,
                &mut coarser,
                100_000,
            );
            prop_assert!(fine_result.is_ok());
            prop_assert!(coarse_result.is_ok());
            prop_assert!(coarser.len() <= finer.len());
        }
    }
}
