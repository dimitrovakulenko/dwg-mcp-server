use std::collections::{BTreeMap, HashMap, HashSet};
use std::f64::consts::TAU;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

const MAX_PIXELS: u64 = 16_777_216;
const MAX_BLOCK_DEPTH: usize = 32;
const MAX_DISPLAY_ITEMS: usize = 250_000;
const MAX_SVG_BYTES: usize = 32 * 1024 * 1024;
const ROBUST_FIT_MIN_ITEMS: usize = 100;
const ROBUST_FIT_TRIM_DIVISOR: usize = 100;
const ROBUST_FIT_OUTLIER_RATIO: f64 = 20.0;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Point2 {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

impl Bounds {
    fn empty() -> Self {
        Self {
            min: [f64::INFINITY; 2],
            max: [f64::NEG_INFINITY; 2],
        }
    }

    fn include(&mut self, point: Point2) {
        self.min[0] = self.min[0].min(point.x);
        self.min[1] = self.min[1].min(point.y);
        self.max[0] = self.max[0].max(point.x);
        self.max[1] = self.max[1].max(point.y);
    }

    fn include_bounds(&mut self, other: Self) {
        self.include(Point2 {
            x: other.min[0],
            y: other.min[1],
        });
        self.include(Point2 {
            x: other.max[0],
            y: other.max[1],
        });
    }

    fn intersects(self, other: Self) -> bool {
        self.max[0] >= other.min[0]
            && self.min[0] <= other.max[0]
            && self.max[1] >= other.min[1]
            && self.min[1] <= other.max[1]
    }

    fn intersection(self, other: Self) -> Option<Self> {
        let intersection = Self {
            min: [self.min[0].max(other.min[0]), self.min[1].max(other.min[1])],
            max: [self.max[0].min(other.max[0]), self.max[1].min(other.max[1])],
        };
        intersection.valid().then_some(intersection)
    }

    fn valid(self) -> bool {
        self.min
            .iter()
            .chain(self.max.iter())
            .all(|value| value.is_finite())
            && self.max[0] >= self.min[0]
            && self.max[1] >= self.min[1]
            && (self.max[0] > self.min[0] || self.max[1] > self.min[1])
    }

    fn padded(self, fraction: f64) -> Self {
        let dx = (self.max[0] - self.min[0]) * fraction;
        let dy = (self.max[1] - self.min[1]) * fraction;
        Self {
            min: [self.min[0] - dx, self.min[1] - dy],
            max: [self.max[0] + dx, self.max[1] + dy],
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RenderTarget {
    #[default]
    Model,
    Layout {
        #[serde(rename = "layoutHandle")]
        layout_handle: String,
    },
    Viewport {
        #[serde(rename = "viewportHandle")]
        viewport_handle: String,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RenderFormat {
    #[default]
    Png,
    Svg,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RenderBackground {
    Model,
    #[default]
    Paper,
    Transparent,
    White,
    Black,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RenderRequest {
    #[serde(default)]
    pub target: RenderTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Bounds>,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default)]
    pub format: RenderFormat,
    #[serde(default)]
    pub background: RenderBackground,
    #[serde(default = "default_padding")]
    pub padding: f64,
}

fn default_width() -> u32 {
    1600
}
fn default_height() -> u32 {
    1200
}
fn default_padding() -> f64 {
    0.02
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RenderView {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub bounds: Bounds,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewport_handle: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub viewport_handles: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RenderDiagnostics {
    pub rendered_entities: usize,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fallbacks: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unsupported_by_type: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RenderOutput {
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub rendered_region: Bounds,
    pub data: String,
    #[serde(flatten)]
    pub diagnostics: RenderDiagnostics,
}

#[derive(Clone, Debug)]
pub struct SourceEntity {
    pub handle: String,
    pub type_name: String,
    pub kind: String,
    pub properties: BTreeMap<String, Value>,
    pub container_block_handle: Option<String>,
    pub layout_handle: Option<String>,
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("invalid render request: {0}")]
    InvalidRequest(String),
    #[error("render target was not found: {0}")]
    TargetNotFound(String),
    #[error("render target has no drawable bounds")]
    EmptyTarget,
    #[error("render resource limit exceeded: {0}")]
    ResourceLimit(String),
    #[error("PNG rasterization failed: {0}")]
    Rasterization(String),
}

#[derive(Clone, Copy, Debug)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    fn translation(x: f64, y: f64) -> Self {
        Self {
            e: x,
            f: y,
            ..Self::IDENTITY
        }
    }
    fn scale(x: f64, y: f64) -> Self {
        Self {
            a: x,
            d: y,
            ..Self::IDENTITY
        }
    }
    fn rotation(angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            e: 0.0,
            f: 0.0,
        }
    }
    fn then(self, rhs: Self) -> Self {
        Self {
            a: rhs.a * self.a + rhs.c * self.b,
            b: rhs.b * self.a + rhs.d * self.b,
            c: rhs.a * self.c + rhs.c * self.d,
            d: rhs.b * self.c + rhs.d * self.d,
            e: rhs.a * self.e + rhs.c * self.f + rhs.e,
            f: rhs.b * self.e + rhs.d * self.f + rhs.f,
        }
    }
    fn point(self, point: Point2) -> Point2 {
        Point2 {
            x: self.a * point.x + self.c * point.y + self.e,
            y: self.b * point.x + self.d * point.y + self.f,
        }
    }

    fn svg(self) -> String {
        format!(
            "matrix({} {} {} {} {} {})",
            f(self.a),
            f(self.b),
            f(self.c),
            f(self.d),
            f(self.e),
            f(self.f)
        )
    }
}

#[derive(Clone, Debug)]
enum Graphic {
    Path {
        data: String,
        fill: bool,
        transform: Option<Matrix>,
    },
    Text {
        point: Point2,
        text: String,
        height: f64,
        rotation: f64,
        anchor: &'static str,
    },
}

#[derive(Clone, Debug)]
struct DisplayItem {
    handle: String,
    type_name: String,
    color: String,
    graphic: Graphic,
    bounds: Bounds,
    clip: Option<Bounds>,
    method: &'static str,
}

#[derive(Clone, Debug)]
struct Viewport {
    handle: String,
    layout_handle: String,
    paper_bounds: Bounds,
    view_bounds: Bounds,
    view_center: Point2,
    twist: f64,
    enabled: bool,
}

pub struct RenderDocument {
    entities: Vec<SourceEntity>,
    by_block: HashMap<String, Vec<usize>>,
    layers: HashMap<String, String>,
    block_bases: HashMap<String, Point2>,
    layouts: HashMap<String, usize>,
    viewports: HashMap<String, Viewport>,
    model_block: Option<String>,
    model_extents: Option<Bounds>,
}

impl RenderDocument {
    pub fn new(entities: Vec<SourceEntity>) -> Self {
        let mut by_block = HashMap::<String, Vec<usize>>::new();
        let mut layers = HashMap::new();
        let mut block_bases = HashMap::new();
        let mut layouts = HashMap::new();
        let mut paper_viewports = HashMap::<String, String>::new();
        let mut model_block = None;
        let mut model_extents = None;

        for (index, entity) in entities.iter().enumerate() {
            if let Some(block) = &entity.container_block_handle {
                by_block.entry(block.clone()).or_default().push(index);
            }
            match entity.type_name.as_str() {
                "AcDbLayerTableRecord" => {
                    if let Some(color) = number(&entity.properties, "color") {
                        layers.insert(entity.handle.clone(), aci_color(color as i64, false));
                    }
                }
                "AcDbLayout" => {
                    layouts.insert(entity.handle.clone(), index);
                    if let Some(viewport) = string(&entity.properties, "active_viewport") {
                        paper_viewports.insert(entity.handle.clone(), viewport.to_owned());
                    }
                }
                "AcDbBlockTableRecord" => {
                    block_bases.insert(
                        entity.handle.clone(),
                        point(&entity.properties, "base_pt").unwrap_or_default(),
                    );
                }
                "BLOCK_CONTROL" => {
                    model_block = string(&entity.properties, "model_space").map(str::to_owned);
                }
                "HEADER" => {
                    let min = point(&entity.properties, "EXTMIN");
                    let max = point(&entity.properties, "EXTMAX");
                    model_extents = min.zip(max).and_then(|(min, max)| {
                        let bounds = Bounds {
                            min: [min.x, min.y],
                            max: [max.x, max.y],
                        };
                        bounds.valid().then_some(bounds)
                    });
                }
                _ => {}
            }
        }

        let mut viewports = HashMap::new();
        for entity in &entities {
            if entity.type_name != "AcDbViewport" {
                continue;
            }
            let Some(layout_handle) = entity.layout_handle.clone() else {
                continue;
            };
            let Some(center) = point(&entity.properties, "center") else {
                continue;
            };
            let width = number(&entity.properties, "width").unwrap_or(0.0).abs();
            let height = number(&entity.properties, "height").unwrap_or(0.0).abs();
            let view_height = number(&entity.properties, "VIEWSIZE")
                .unwrap_or(height)
                .abs();
            if width == 0.0 || height == 0.0 || view_height == 0.0 {
                continue;
            }
            let view_offset = point(&entity.properties, "VIEWCTR").unwrap_or_default();
            let view_target = point(&entity.properties, "view_target").unwrap_or_default();
            let view_center = Point2 {
                x: view_target.x + view_offset.x,
                y: view_target.y + view_offset.y,
            };
            let view_width = view_height * width / height;
            viewports.insert(
                entity.handle.clone(),
                Viewport {
                    handle: entity.handle.clone(),
                    layout_handle,
                    paper_bounds: Bounds {
                        min: [center.x - width / 2.0, center.y - height / 2.0],
                        max: [center.x + width / 2.0, center.y + height / 2.0],
                    },
                    view_bounds: Bounds {
                        min: [
                            view_center.x - view_width / 2.0,
                            view_center.y - view_height / 2.0,
                        ],
                        max: [
                            view_center.x + view_width / 2.0,
                            view_center.y + view_height / 2.0,
                        ],
                    },
                    view_center,
                    twist: number(&entity.properties, "twist_angle").unwrap_or(0.0),
                    enabled: paper_viewports
                        .get(&entity.layout_handle.clone().unwrap_or_default())
                        .is_none_or(|paper_viewport| paper_viewport != &entity.handle),
                },
            );
        }

        Self {
            entities,
            by_block,
            layers,
            block_bases,
            layouts,
            viewports,
            model_block,
            model_extents,
        }
    }

    pub fn list_views(&self) -> Result<Vec<RenderView>, RenderError> {
        let mut diagnostics = RenderDiagnostics::default();
        let model_bounds = self
            .model_extents
            .or_else(|| {
                let items = self
                    .model_items(Matrix::IDENTITY, None, None, &mut diagnostics)
                    .ok()?;
                items_bounds(&items)
            })
            .unwrap_or(Bounds {
                min: [0.0, 0.0],
                max: [1.0, 1.0],
            });
        let mut views = vec![RenderView {
            id: "model".to_owned(),
            kind: "model".to_owned(),
            name: "Model".to_owned(),
            bounds: model_bounds,
            layout_handle: None,
            viewport_handle: None,
            viewport_handles: Vec::new(),
        }];

        let mut layouts = self.layouts.iter().collect::<Vec<_>>();
        layouts.sort_by_key(|(_, index)| {
            number(&self.entities[**index].properties, "tab_order").unwrap_or(0.0) as i64
        });
        for (handle, index) in layouts {
            let entity = &self.entities[*index];
            if string(&entity.properties, "layout_name") == Some("Model") {
                continue;
            }
            let name = string(&entity.properties, "layout_name")
                .unwrap_or("Layout")
                .to_owned();
            let bounds =
                layout_bounds(entity)
                    .map(|mut bounds| {
                        for viewport in self.viewports.values().filter(|viewport| {
                            viewport.layout_handle == *handle && viewport.enabled
                        }) {
                            bounds.include_bounds(viewport.paper_bounds);
                        }
                        bounds
                    })
                    .unwrap_or(Bounds {
                        min: [0.0, 0.0],
                        max: [1.0, 1.0],
                    });
            let mut viewport_handles = self
                .viewports
                .values()
                .filter(|vp| vp.layout_handle == *handle && vp.enabled)
                .map(|vp| vp.handle.clone())
                .collect::<Vec<_>>();
            viewport_handles.sort();
            views.push(RenderView {
                id: format!("layout:{handle}"),
                kind: "layout".to_owned(),
                name: name.clone(),
                bounds,
                layout_handle: Some(handle.clone()),
                viewport_handle: None,
                viewport_handles: viewport_handles.clone(),
            });
            for viewport_handle in viewport_handles {
                let vp = &self.viewports[&viewport_handle];
                views.push(RenderView {
                    id: format!("viewport:{viewport_handle}"),
                    kind: "viewport".to_owned(),
                    name: format!("{name} / viewport {viewport_handle}"),
                    bounds: vp.view_bounds,
                    layout_handle: Some(handle.clone()),
                    viewport_handle: Some(viewport_handle),
                    viewport_handles: Vec::new(),
                });
            }
        }
        Ok(views)
    }

    pub fn render(&self, request: RenderRequest) -> Result<RenderOutput, RenderError> {
        validate_request(&request)?;
        let mut diagnostics = RenderDiagnostics::default();
        let (items, natural_bounds) = match &request.target {
            RenderTarget::Model => {
                let items =
                    self.model_items(Matrix::IDENTITY, None, request.region, &mut diagnostics)?;
                let bounds =
                    automatic_bounds(&items, &mut diagnostics).ok_or(RenderError::EmptyTarget)?;
                (items, bounds)
            }
            RenderTarget::Layout { layout_handle } => {
                let items = self.layout_items(layout_handle, request.region, &mut diagnostics)?;
                let declared_bounds = self
                    .layouts
                    .get(layout_handle)
                    .and_then(|index| layout_bounds(&self.entities[*index]));
                let item_bounds = automatic_bounds(&items, &mut diagnostics);
                let bounds = match (declared_bounds, item_bounds) {
                    (Some(mut declared), Some(items)) => {
                        declared.include_bounds(items);
                        declared
                    }
                    (Some(bounds), None) | (None, Some(bounds)) => bounds,
                    (None, None) => return Err(RenderError::EmptyTarget),
                };
                (items, bounds)
            }
            RenderTarget::Viewport { viewport_handle } => {
                let viewport = self.viewports.get(viewport_handle).ok_or_else(|| {
                    RenderError::TargetNotFound(format!("viewport {viewport_handle}"))
                })?;
                (
                    self.model_items(Matrix::IDENTITY, None, request.region, &mut diagnostics)?,
                    viewport.view_bounds,
                )
            }
        };
        let mut region = request
            .region
            .unwrap_or_else(|| natural_bounds.padded(request.padding));
        if request.region.is_none() {
            region = fit_output_aspect(region, request.width, request.height);
        }
        if !region.valid() || region.max[0] == region.min[0] || region.max[1] == region.min[1] {
            return Err(RenderError::InvalidRequest(
                "region must have finite min values smaller than max values".to_owned(),
            ));
        }
        if matches!(request.background, RenderBackground::Transparent)
            && items.iter().any(|item| item.type_name == "AcDbWipeout")
        {
            diagnostics.warnings.push(
                "WIPEOUT regions are rendered as opaque white on transparent backgrounds"
                    .to_owned(),
            );
        }
        let svg = svg_document(
            &items,
            region,
            request.width,
            request.height,
            request.background,
        )?;
        let (mime_type, bytes) = match request.format {
            RenderFormat::Svg => ("image/svg+xml", svg.into_bytes()),
            RenderFormat::Png => (
                "image/png",
                rasterize_svg(&svg, request.width, request.height)?,
            ),
        };
        Ok(RenderOutput {
            mime_type: mime_type.to_owned(),
            width: request.width,
            height: request.height,
            rendered_region: region,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
            diagnostics,
        })
    }

    fn layout_items(
        &self,
        layout_handle: &str,
        cull_region: Option<Bounds>,
        diagnostics: &mut RenderDiagnostics,
    ) -> Result<Vec<DisplayItem>, RenderError> {
        let layout_index = *self
            .layouts
            .get(layout_handle)
            .ok_or_else(|| RenderError::TargetNotFound(format!("layout {layout_handle}")))?;
        let block =
            string(&self.entities[layout_index].properties, "block_header").ok_or_else(|| {
                RenderError::TargetNotFound(format!(
                    "layout {layout_handle} has no paper-space block"
                ))
            })?;
        let mut items = Vec::new();
        self.expand_block(
            block,
            Matrix::IDENTITY,
            None,
            cull_region,
            0,
            &mut HashSet::new(),
            &mut items,
            diagnostics,
        )?;
        for viewport in self
            .viewports
            .values()
            .filter(|viewport| viewport.layout_handle == layout_handle && viewport.enabled)
        {
            let sx = (viewport.paper_bounds.max[0] - viewport.paper_bounds.min[0])
                / (viewport.view_bounds.max[0] - viewport.view_bounds.min[0]);
            let sy = (viewport.paper_bounds.max[1] - viewport.paper_bounds.min[1])
                / (viewport.view_bounds.max[1] - viewport.view_bounds.min[1]);
            let paper_center = Point2 {
                x: (viewport.paper_bounds.min[0] + viewport.paper_bounds.max[0]) / 2.0,
                y: (viewport.paper_bounds.min[1] + viewport.paper_bounds.max[1]) / 2.0,
            };
            let transform = Matrix::translation(-viewport.view_center.x, -viewport.view_center.y)
                .then(Matrix::rotation(-viewport.twist))
                .then(Matrix::scale(sx, sy))
                .then(Matrix::translation(paper_center.x, paper_center.y));
            items.extend(self.model_items(
                transform,
                Some(viewport.paper_bounds),
                cull_region,
                diagnostics,
            )?);
        }
        Ok(items)
    }

    fn model_items(
        &self,
        transform: Matrix,
        clip: Option<Bounds>,
        cull_region: Option<Bounds>,
        diagnostics: &mut RenderDiagnostics,
    ) -> Result<Vec<DisplayItem>, RenderError> {
        let mut items = Vec::new();
        if let Some(block) = &self.model_block {
            self.expand_block(
                block,
                transform,
                clip,
                cull_region,
                0,
                &mut HashSet::new(),
                &mut items,
                diagnostics,
            )?;
        } else {
            diagnostics
                .warnings
                .push("model-space block was not found".to_owned());
        }
        Ok(items)
    }

    #[allow(clippy::too_many_arguments)]
    fn expand_block(
        &self,
        block: &str,
        transform: Matrix,
        clip: Option<Bounds>,
        cull_region: Option<Bounds>,
        depth: usize,
        stack: &mut HashSet<String>,
        items: &mut Vec<DisplayItem>,
        diagnostics: &mut RenderDiagnostics,
    ) -> Result<(), RenderError> {
        if depth >= MAX_BLOCK_DEPTH || !stack.insert(block.to_owned()) {
            *diagnostics
                .fallbacks
                .entry("recursionLimit".to_owned())
                .or_default() += 1;
            return Ok(());
        }
        for index in self.by_block.get(block).into_iter().flatten() {
            let entity = &self.entities[*index];
            if entity.type_name == "AcDbViewport" {
                continue;
            }
            if let Some(target) = insert_block(entity) {
                if !self.by_block.contains_key(target) {
                    *diagnostics
                        .unsupported_by_type
                        .entry(entity.type_name.clone())
                        .or_default() += 1;
                    continue;
                }
                let insertion = point(&entity.properties, "ins_pt").unwrap_or_default();
                let base = self.block_bases.get(target).copied().unwrap_or_default();
                let scale =
                    point3_xy(&entity.properties, "scale").unwrap_or(Point2 { x: 1.0, y: 1.0 });
                let rotation = number(&entity.properties, "rotation").unwrap_or(0.0);
                let local = Matrix::translation(-base.x, -base.y)
                    .then(Matrix::scale(scale.x, scale.y))
                    .then(Matrix::rotation(rotation))
                    .then(Matrix::translation(insertion.x, insertion.y))
                    .then(transform);
                self.expand_block(
                    target,
                    local,
                    clip,
                    cull_region,
                    depth + 1,
                    stack,
                    items,
                    diagnostics,
                )?;
                if entity.type_name.contains("Dimension") {
                    *diagnostics
                        .fallbacks
                        .entry("generatedBlock".to_owned())
                        .or_default() += 1;
                }
                continue;
            }
            match display_item(entity, transform, clip, &self.layers) {
                Some(item) => {
                    let Some(effective_bounds) = item_effective_bounds(&item) else {
                        continue;
                    };
                    if cull_region.is_some_and(|region| !effective_bounds.intersects(region)) {
                        continue;
                    }
                    if items.len() >= MAX_DISPLAY_ITEMS {
                        return Err(RenderError::ResourceLimit(format!(
                            "more than {MAX_DISPLAY_ITEMS} drawable entities; request a smaller explicit region"
                        )));
                    }
                    diagnostics.rendered_entities += 1;
                    if item.method != "native" {
                        *diagnostics
                            .fallbacks
                            .entry(item.method.to_owned())
                            .or_default() += 1;
                    }
                    items.push(item);
                }
                None if supported_entity(&entity.type_name) => {
                    *diagnostics
                        .fallbacks
                        .entry("degenerateGeometry".to_owned())
                        .or_default() += 1;
                }
                None if entity.kind == "entity" && !auxiliary_entity(&entity.type_name) => {
                    *diagnostics
                        .unsupported_by_type
                        .entry(entity.type_name.clone())
                        .or_default() += 1;
                }
                None => {}
            }
        }
        stack.remove(block);
        Ok(())
    }
}

fn validate_request(request: &RenderRequest) -> Result<(), RenderError> {
    if request.width == 0
        || request.height == 0
        || u64::from(request.width) * u64::from(request.height) > MAX_PIXELS
    {
        return Err(RenderError::InvalidRequest(format!(
            "output must contain between 1 and {MAX_PIXELS} pixels"
        )));
    }
    if !request.padding.is_finite() || !(0.0..=1.0).contains(&request.padding) {
        return Err(RenderError::InvalidRequest(
            "padding must be between 0 and 1".to_owned(),
        ));
    }
    Ok(())
}

fn insert_block(entity: &SourceEntity) -> Option<&str> {
    if entity.type_name.contains("Dimension") {
        string(&entity.properties, "block")
    } else if matches!(
        entity.type_name.as_str(),
        "AcDbBlockReference" | "AcDbMInsertBlock" | "AcDbTable"
    ) {
        string(&entity.properties, "block_header")
    } else {
        None
    }
}

fn auxiliary_entity(type_name: &str) -> bool {
    matches!(
        type_name,
        "AcDbBlockBegin"
            | "AcDbBlockEnd"
            | "SEQEND"
            | "AcDb2dVertex"
            | "AcDb3dPolylineVertex"
            | "AcDbPolyFaceMeshVertex"
            | "AcDbFaceRecord"
    )
}

fn supported_entity(type_name: &str) -> bool {
    matches!(
        type_name,
        "AcDbLine"
            | "AcDbCircle"
            | "AcDbArc"
            | "AcDbEllipse"
            | "AcDbSpline"
            | "AcDbText"
            | "AcDbMText"
            | "AcDbAttribute"
            | "AcDbAttributeDefinition"
            | "AcDbHatch"
            | "AcDbPoint"
            | "AcDbFace"
            | "AcDbTrace"
            | "AcDbPolyline"
            | "AcDb2dPolyline"
            | "AcDb3dPolyline"
            | "AcDbWipeout"
    )
}

fn display_item(
    entity: &SourceEntity,
    transform: Matrix,
    clip: Option<Bounds>,
    layers: &HashMap<String, String>,
) -> Option<DisplayItem> {
    if number(&entity.properties, "invisible").unwrap_or(0.0) != 0.0 {
        return None;
    }
    let color = entity_color(entity, layers);
    let mut points_for_bounds = Vec::new();
    let mut render_method = "native";
    let graphic = match entity.type_name.as_str() {
        "AcDbLine" => {
            let points = [
                point(&entity.properties, "start")?,
                point(&entity.properties, "end")?,
            ]
            .map(|p| transform.point(p));
            points_for_bounds.extend(points);
            Graphic::Path {
                data: path_from_points(&points, false),
                fill: false,
                transform: None,
            }
        }
        "AcDbCircle" => {
            let center = point(&entity.properties, "center")?;
            let radius = number(&entity.properties, "radius")?.abs();
            points_for_bounds.extend([
                transform.point(Point2 {
                    x: center.x - radius,
                    y: center.y - radius,
                }),
                transform.point(Point2 {
                    x: center.x + radius,
                    y: center.y + radius,
                }),
                transform.point(Point2 {
                    x: center.x - radius,
                    y: center.y + radius,
                }),
                transform.point(Point2 {
                    x: center.x + radius,
                    y: center.y - radius,
                }),
            ]);
            Graphic::Path {
                data: format!(
                    "M {} {} A {} {} 0 1 1 {} {} A {} {} 0 1 1 {} {}",
                    f(center.x - radius),
                    f(center.y),
                    f(radius),
                    f(radius),
                    f(center.x + radius),
                    f(center.y),
                    f(radius),
                    f(radius),
                    f(center.x - radius),
                    f(center.y)
                ),
                fill: false,
                transform: Some(transform),
            }
        }
        "AcDbArc" => arc_graphic(entity, transform, &mut points_for_bounds)?,
        "AcDbEllipse" => {
            let center = point(&entity.properties, "center")?;
            let major = point3_xy(&entity.properties, "sm_axis")?;
            let ratio = number(&entity.properties, "axis_ratio").unwrap_or(1.0);
            let start = number(&entity.properties, "start_angle").unwrap_or(0.0);
            let end = number(&entity.properties, "end_angle").unwrap_or(TAU);
            let angle = major.y.atan2(major.x);
            let major_len = (major.x * major.x + major.y * major.y).sqrt();
            let pts = sampled_curve(start, end, 96, |t| Point2 {
                x: center.x + major_len * t.cos() * angle.cos()
                    - major_len * ratio * t.sin() * angle.sin(),
                y: center.y
                    + major_len * t.cos() * angle.sin()
                    + major_len * ratio * t.sin() * angle.cos(),
            })
            .into_iter()
            .map(|p| transform.point(p))
            .collect::<Vec<_>>();
            points_for_bounds.extend_from_slice(&pts);
            Graphic::Path {
                data: ellipse_path(center, major, ratio, start, end, true),
                fill: false,
                transform: Some(transform),
            }
        }
        "AcDbText" | "AcDbMText" | "AcDbAttribute" | "AcDbAttributeDefinition" => {
            let horizontal = number(&entity.properties, "horiz_alignment").unwrap_or(0.0) as i64;
            let insertion = if horizontal != 0 {
                point(&entity.properties, "alignment_pt")
                    .or_else(|| point(&entity.properties, "ins_pt"))?
            } else {
                point(&entity.properties, "ins_pt")?
            };
            let point = transform.point(insertion);
            let raw_text = string(&entity.properties, "text_value")
                .or_else(|| string(&entity.properties, "text"))
                .unwrap_or("");
            let text = clean_mtext(raw_text);
            let height = number(&entity.properties, "height")
                .or_else(|| number(&entity.properties, "text_height"))
                .unwrap_or(1.0)
                .abs()
                * matrix_scale(transform);
            let rotation =
                number(&entity.properties, "rotation").unwrap_or(0.0) + matrix_rotation(transform);
            let attachment = number(&entity.properties, "attachment").unwrap_or(1.0) as i64;
            let anchor = if entity.type_name == "AcDbMText" {
                match (attachment - 1).rem_euclid(3) {
                    1 => "middle",
                    2 => "end",
                    _ => "start",
                }
            } else {
                match horizontal {
                    1 | 4 => "middle",
                    2 | 5 => "end",
                    _ => "start",
                }
            };
            let max_line_length = text
                .lines()
                .map(str::chars)
                .map(Iterator::count)
                .max()
                .unwrap_or(0);
            points_for_bounds.extend([
                point,
                Point2 {
                    x: point.x + height * max_line_length as f64 * 0.65,
                    y: point.y + height * text.lines().count().max(1) as f64 * 1.2,
                },
            ]);
            Graphic::Text {
                point,
                text,
                height,
                rotation,
                anchor,
            }
        }
        "AcDbHatch" => {
            let (data, pts, approximate) = hatch_geometry(&entity.properties, transform);
            if pts.len() < 2 {
                return None;
            }
            if approximate {
                render_method = "adaptiveNurbs";
            }
            points_for_bounds.extend_from_slice(&pts);
            Graphic::Path {
                data,
                fill: entity
                    .properties
                    .get("is_solid_fill")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                transform: Some(transform),
            }
        }
        "AcDbSpline" => {
            let (graphic, pts, method) = spline_geometry(&entity.properties, transform)?;
            render_method = method;
            points_for_bounds.extend(pts);
            graphic
        }
        "AcDbPolyline" => {
            let (data, pts) = polyline_geometry(&entity.properties, transform)?;
            points_for_bounds.extend(pts);
            Graphic::Path {
                data,
                fill: false,
                transform: Some(transform),
            }
        }
        "AcDbPoint" => {
            let p = transform.point(
                point(&entity.properties, "point")
                    .or_else(|| point(&entity.properties, "location"))
                    .or_else(|| xy_point(&entity.properties))?,
            );
            let r = 0.1 * matrix_scale(transform);
            points_for_bounds.extend([
                Point2 {
                    x: p.x - r,
                    y: p.y - r,
                },
                Point2 {
                    x: p.x + r,
                    y: p.y + r,
                },
            ]);
            Graphic::Path {
                data: format!(
                    "M {} {} L {} {} M {} {} L {} {}",
                    f(p.x - r),
                    f(p.y),
                    f(p.x + r),
                    f(p.y),
                    f(p.x),
                    f(p.y - r),
                    f(p.x),
                    f(p.y + r)
                ),
                fill: false,
                transform: None,
            }
        }
        "AcDbWipeout" => {
            let (points, frame, approximate) = wipeout_points(&entity.properties, transform)?;
            if approximate {
                render_method = "rectangularWipeout";
            }
            let inverted = entity
                .properties
                .get("clip_mode")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            points_for_bounds.extend_from_slice(if inverted { &frame } else { &points });
            Graphic::Path {
                data: if inverted {
                    format!(
                        "{} {}",
                        path_from_points(&frame, true),
                        path_from_points(&points, true)
                    )
                } else {
                    path_from_points(&points, true)
                },
                fill: true,
                transform: None,
            }
        }
        _ if entity.type_name.contains("Dimension") => return None,
        _ => {
            let pts = generic_points(&entity.properties)
                .into_iter()
                .map(|p| transform.point(p))
                .collect::<Vec<_>>();
            if pts.len() < 2 {
                return None;
            }
            points_for_bounds.extend_from_slice(&pts);
            let closed = entity.type_name.contains("Solid")
                || entity.type_name.contains("Face")
                || number(&entity.properties, "flag").unwrap_or(0.0) as i64 & 1 != 0;
            Graphic::Path {
                data: path_from_points(&pts, closed),
                fill: entity.type_name.contains("Solid"),
                transform: None,
            }
        }
    };
    let mut bounds = Bounds::empty();
    for point in points_for_bounds {
        bounds.include(point);
    }
    if !bounds.valid() {
        return None;
    }
    Some(DisplayItem {
        handle: entity.handle.clone(),
        type_name: entity.type_name.clone(),
        color,
        graphic,
        bounds,
        clip,
        method: render_method,
    })
}

fn arc_graphic(
    entity: &SourceEntity,
    transform: Matrix,
    bounds: &mut Vec<Point2>,
) -> Option<Graphic> {
    let center = point(&entity.properties, "center")?;
    let radius = number(&entity.properties, "radius")?.abs();
    let start = number(&entity.properties, "start_angle")?;
    let end = number(&entity.properties, "end_angle")?;
    let delta = (end - start).rem_euclid(TAU);
    let p1 = Point2 {
        x: center.x + radius * start.cos(),
        y: center.y + radius * start.sin(),
    };
    let p2 = Point2 {
        x: center.x + radius * end.cos(),
        y: center.y + radius * end.sin(),
    };
    bounds.extend(
        sampled_curve(start, start + delta, 24, |t| Point2 {
            x: center.x + radius * t.cos(),
            y: center.y + radius * t.sin(),
        })
        .into_iter()
        .map(|p| transform.point(p)),
    );
    Some(Graphic::Path {
        data: format!(
            "M {} {} A {} {} 0 {} 1 {} {}",
            f(p1.x),
            f(p1.y),
            f(radius),
            f(radius),
            i32::from(delta > std::f64::consts::PI),
            f(p2.x),
            f(p2.y)
        ),
        fill: false,
        transform: Some(transform),
    })
}

fn ellipse_path(
    center: Point2,
    major: Point2,
    ratio: f64,
    start: f64,
    end: f64,
    is_ccw: bool,
) -> String {
    let rx = major.x.hypot(major.y);
    let ry = rx * ratio.abs();
    let rotation = major.y.atan2(major.x).to_degrees();
    let point_at = |angle: f64| {
        let local = Point2 {
            x: rx * angle.cos(),
            y: ry * angle.sin(),
        };
        let rotation = Matrix::rotation(major.y.atan2(major.x));
        let rotated = rotation.point(local);
        Point2 {
            x: center.x + rotated.x,
            y: center.y + rotated.y,
        }
    };
    let mut delta = if is_ccw {
        (end - start).rem_euclid(TAU)
    } else {
        (start - end).rem_euclid(TAU)
    };
    if delta < 1e-9 {
        delta = TAU;
    }
    let sweep = i32::from(is_ccw);
    let first = point_at(start);
    if delta >= TAU - 1e-7 {
        let middle = point_at(
            start
                + if is_ccw {
                    std::f64::consts::PI
                } else {
                    -std::f64::consts::PI
                },
        );
        return format!(
            "M {} {} A {} {} {} 0 {} {} {} A {} {} {} 0 {} {} {}",
            f(first.x),
            f(first.y),
            f(rx),
            f(ry),
            f(rotation),
            sweep,
            f(middle.x),
            f(middle.y),
            f(rx),
            f(ry),
            f(rotation),
            sweep,
            f(first.x),
            f(first.y)
        );
    }
    let last = point_at(if is_ccw { start + delta } else { start - delta });
    format!(
        "M {} {} A {} {} {} {} {} {} {}",
        f(first.x),
        f(first.y),
        f(rx),
        f(ry),
        f(rotation),
        i32::from(delta > std::f64::consts::PI),
        sweep,
        f(last.x),
        f(last.y)
    )
}

fn generic_points(properties: &BTreeMap<String, Value>) -> Vec<Point2> {
    for key in [
        "points",
        "vertices",
        "fit_pts",
        "ctrl_pts",
        "control_points",
    ] {
        if let Some(points) = properties.get(key).and_then(Value::as_array) {
            let parsed = points.iter().filter_map(value_point).collect::<Vec<_>>();
            if parsed.len() >= 2 {
                return parsed;
            }
        }
    }
    [
        "corner1",
        "corner2",
        "corner3",
        "corner4",
        "point_0",
        "point_1",
        "point_2",
        "point_3",
        "xline1_pt",
        "xline2_pt",
        "def_pt",
        "text_midpt",
    ]
    .into_iter()
    .filter_map(|key| point(properties, key))
    .collect()
}

fn wipeout_points(
    properties: &BTreeMap<String, Value>,
    transform: Matrix,
) -> Option<(Vec<Point2>, Vec<Point2>, bool)> {
    let origin = point(properties, "pt0")?;
    let u = point(properties, "uvec")?;
    let v = point(properties, "vvec")?;
    let image_size = point(properties, "image_size")?;
    if image_size.x.abs() < f64::EPSILON || image_size.y.abs() < f64::EPSILON {
        return None;
    }
    let to_world = |point: Point2| {
        let x = point.x + 0.5;
        let y = point.y + 0.5;
        transform.point(Point2 {
            x: origin.x + u.x * x + v.x * y,
            y: origin.y + u.y * x + v.y * y,
        })
    };
    let frame = [
        Point2 { x: -0.5, y: -0.5 },
        Point2 {
            x: image_size.x - 0.5,
            y: -0.5,
        },
        Point2 {
            x: image_size.x - 0.5,
            y: image_size.y - 0.5,
        },
        Point2 {
            x: -0.5,
            y: image_size.y - 0.5,
        },
    ]
    .map(to_world)
    .to_vec();
    let mut clip = properties
        .get("clip_verts")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(value_point).collect::<Vec<_>>())
        .unwrap_or_default();
    let approximate = clip.len() < 2;
    if approximate {
        return Some((frame.clone(), frame, true));
    }
    if clip.len() == 2 {
        let [first, last] = [clip[0], clip[1]];
        clip = vec![
            first,
            Point2 {
                x: last.x,
                y: first.y,
            },
            last,
            Point2 {
                x: first.x,
                y: last.y,
            },
        ];
    }
    if clip.len() < 3 {
        return None;
    }
    Some((clip.into_iter().map(to_world).collect(), frame, false))
}

fn polyline_geometry(
    properties: &BTreeMap<String, Value>,
    transform: Matrix,
) -> Option<(String, Vec<Point2>)> {
    let points = properties
        .get("points")?
        .as_array()?
        .iter()
        .filter_map(value_point)
        .collect::<Vec<_>>();
    if points.len() < 2 {
        return None;
    }
    let bulges = properties
        .get("bulges")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(|value| value.as_f64().unwrap_or(0.0))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let flag = number(properties, "flag").unwrap_or(0.0) as i64;
    let closed = flag & 512 != 0 || flag & 1 != 0;
    Some(path_from_bulges(&points, &bulges, closed, transform))
}

fn path_from_bulges(
    points: &[Point2],
    bulges: &[f64],
    closed: bool,
    transform: Matrix,
) -> (String, Vec<Point2>) {
    let mut data = format!("M {} {}", f(points[0].x), f(points[0].y));
    let mut bounds_points = vec![transform.point(points[0])];
    let segment_count = points.len() - 1 + usize::from(closed);
    for index in 0..segment_count {
        let start = points[index];
        let end = points[(index + 1) % points.len()];
        let bulge = bulges.get(index).copied().unwrap_or(0.0);
        if bulge.abs() < 1e-12 {
            data.push_str(&format!(" L {} {}", f(end.x), f(end.y)));
            bounds_points.push(transform.point(end));
            continue;
        }
        let chord = (end.x - start.x).hypot(end.y - start.y);
        if chord < 1e-12 {
            continue;
        }
        let radius = chord * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
        let center_offset = chord * (1.0 - bulge * bulge) / (4.0 * bulge);
        let center = Point2 {
            x: (start.x + end.x) / 2.0 - (end.y - start.y) / chord * center_offset,
            y: (start.y + end.y) / 2.0 + (end.x - start.x) / chord * center_offset,
        };
        let angle = 4.0 * bulge.atan();
        let start_angle = (start.y - center.y).atan2(start.x - center.x);
        bounds_points.extend((0..=32).map(|step| {
            let value = start_angle + angle * step as f64 / 32.0;
            transform.point(Point2 {
                x: center.x + radius * value.cos(),
                y: center.y + radius * value.sin(),
            })
        }));
        data.push_str(&format!(
            " A {} {} 0 {} {} {} {}",
            f(radius),
            f(radius),
            i32::from(angle.abs() > std::f64::consts::PI),
            i32::from(bulge > 0.0),
            f(end.x),
            f(end.y)
        ));
    }
    if closed {
        data.push_str(" Z");
    }
    (data, bounds_points)
}

#[derive(Clone, Copy)]
struct NurbsControl {
    point: Point2,
    weight: f64,
}

fn spline_controls(properties: &BTreeMap<String, Value>) -> Vec<NurbsControl> {
    properties
        .get("controlPoints")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|control| {
            Some(NurbsControl {
                point: value_point(control.get("point")?)?,
                weight: control.get("weight").and_then(Value::as_f64).unwrap_or(1.0),
            })
        })
        .collect()
}

fn spline_geometry(
    properties: &BTreeMap<String, Value>,
    transform: Matrix,
) -> Option<(Graphic, Vec<Point2>, &'static str)> {
    let controls = spline_controls(properties);
    let degree = number(properties, "degree").unwrap_or(3.0) as usize;
    if controls.len() == degree + 1
        && matches!(degree, 2 | 3)
        && controls
            .iter()
            .all(|control| (control.weight - 1.0).abs() < 1e-12)
    {
        let points = controls
            .iter()
            .map(|control| control.point)
            .collect::<Vec<_>>();
        let data = if degree == 2 {
            format!(
                "M {} {} Q {} {} {} {}",
                f(points[0].x),
                f(points[0].y),
                f(points[1].x),
                f(points[1].y),
                f(points[2].x),
                f(points[2].y)
            )
        } else {
            format!(
                "M {} {} C {} {} {} {} {} {}",
                f(points[0].x),
                f(points[0].y),
                f(points[1].x),
                f(points[1].y),
                f(points[2].x),
                f(points[2].y),
                f(points[3].x),
                f(points[3].y)
            )
        };
        return Some((
            Graphic::Path {
                data,
                fill: false,
                transform: Some(transform),
            },
            points
                .into_iter()
                .map(|point| transform.point(point))
                .collect(),
            "native",
        ));
    }

    let knots = properties
        .get("knots")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_f64).collect::<Vec<_>>())
        .unwrap_or_default();
    if !controls.is_empty() && knots.len() >= controls.len() + degree + 1 {
        let points = adaptive_nurbs_points(&controls, &knots, degree, transform)?;
        return Some((
            Graphic::Path {
                data: path_from_points(&points, false),
                fill: false,
                transform: None,
            },
            points,
            "adaptiveNurbs",
        ));
    }

    let fit_points = properties
        .get("fitPoints")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(value_point)
        .collect::<Vec<_>>();
    let (data, bounds) = catmull_rom_path(&fit_points, transform)?;
    Some((
        Graphic::Path {
            data,
            fill: false,
            transform: Some(transform),
        },
        bounds,
        "adaptiveNurbs",
    ))
}

fn adaptive_nurbs_points(
    controls: &[NurbsControl],
    knots: &[f64],
    degree: usize,
    transform: Matrix,
) -> Option<Vec<Point2>> {
    let start = *knots.get(degree)?;
    let end = *knots.get(controls.len())?;
    if !start.is_finite() || !end.is_finite() || end <= start {
        return None;
    }
    let transformed_controls = controls
        .iter()
        .map(|control| transform.point(control.point))
        .collect::<Vec<_>>();
    let mut bounds = Bounds::empty();
    for point in &transformed_controls {
        bounds.include(*point);
    }
    let tolerance =
        ((bounds.max[0] - bounds.min[0]).hypot(bounds.max[1] - bounds.min[1]) * 1e-4).max(1e-8);
    let mut result = Vec::new();
    let mut span_start = start;
    let mut first = transform.point(nurbs_point(controls, knots, degree, start)?);
    result.push(first);
    for span in degree..controls.len() {
        let span_end = knots[span + 1].min(end);
        if span_end <= span_start + f64::EPSILON {
            continue;
        }
        let last = transform.point(nurbs_point(controls, knots, degree, span_end)?);
        adaptive_nurbs_segment(
            controls,
            knots,
            degree,
            transform,
            span_start,
            first,
            span_end,
            last,
            tolerance,
            0,
            &mut result,
        )?;
        span_start = span_end;
        first = last;
    }
    (result.len() >= 2).then_some(result)
}

#[allow(clippy::too_many_arguments)]
fn adaptive_nurbs_segment(
    controls: &[NurbsControl],
    knots: &[f64],
    degree: usize,
    transform: Matrix,
    start: f64,
    first: Point2,
    end: f64,
    last: Point2,
    tolerance: f64,
    depth: usize,
    result: &mut Vec<Point2>,
) -> Option<()> {
    let middle_parameter = (start + end) / 2.0;
    let middle = transform.point(nurbs_point(controls, knots, degree, middle_parameter)?);
    let quarter = transform.point(nurbs_point(
        controls,
        knots,
        degree,
        (start + middle_parameter) / 2.0,
    )?);
    let three_quarters = transform.point(nurbs_point(
        controls,
        knots,
        degree,
        (middle_parameter + end) / 2.0,
    )?);
    let deviation = point_line_distance(middle, first, last)
        .max(point_line_distance(quarter, first, last))
        .max(point_line_distance(three_quarters, first, last));
    if depth < 14 && deviation > tolerance {
        adaptive_nurbs_segment(
            controls,
            knots,
            degree,
            transform,
            start,
            first,
            middle_parameter,
            middle,
            tolerance,
            depth + 1,
            result,
        )?;
        adaptive_nurbs_segment(
            controls,
            knots,
            degree,
            transform,
            middle_parameter,
            middle,
            end,
            last,
            tolerance,
            depth + 1,
            result,
        )?;
    } else {
        result.push(last);
    }
    Some(())
}

fn nurbs_point(
    controls: &[NurbsControl],
    knots: &[f64],
    degree: usize,
    parameter: f64,
) -> Option<Point2> {
    if degree >= controls.len() || knots.len() < controls.len() + degree + 1 {
        return None;
    }
    let last_control = controls.len() - 1;
    let domain_end = knots[controls.len()];
    let span = if parameter >= domain_end {
        last_control
    } else {
        (degree..=last_control)
            .find(|index| parameter >= knots[*index] && parameter < knots[*index + 1])?
    };
    let mut work = (0..=degree)
        .map(|index| {
            let control = controls[span - degree + index];
            [
                control.point.x * control.weight,
                control.point.y * control.weight,
                control.weight,
            ]
        })
        .collect::<Vec<_>>();
    for level in 1..=degree {
        for index in (level..=degree).rev() {
            let knot_index = span - degree + index;
            let denominator = knots[knot_index + degree + 1 - level] - knots[knot_index];
            let alpha = if denominator.abs() < f64::EPSILON {
                0.0
            } else {
                (parameter - knots[knot_index]) / denominator
            };
            for coordinate in 0..3 {
                work[index][coordinate] =
                    (1.0 - alpha) * work[index - 1][coordinate] + alpha * work[index][coordinate];
            }
        }
    }
    let value = work[degree];
    if value[2].abs() < f64::EPSILON {
        return None;
    }
    Some(Point2 {
        x: value[0] / value[2],
        y: value[1] / value[2],
    })
}

fn point_line_distance(point: Point2, start: Point2, end: Point2) -> f64 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length_squared = dx * dx + dy * dy;
    if length_squared < f64::EPSILON {
        return (point.x - start.x).hypot(point.y - start.y);
    }
    ((dy * point.x - dx * point.y + end.x * start.y - end.y * start.x).abs())
        / length_squared.sqrt()
}

fn catmull_rom_path(points: &[Point2], transform: Matrix) -> Option<(String, Vec<Point2>)> {
    if points.len() < 2 {
        return None;
    }
    let mut data = format!("M {} {}", f(points[0].x), f(points[0].y));
    let mut bounds = vec![transform.point(points[0])];
    for index in 0..points.len() - 1 {
        let previous = if index == 0 {
            points[index]
        } else {
            points[index - 1]
        };
        let first = points[index];
        let last = points[index + 1];
        let next = points.get(index + 2).copied().unwrap_or(last);
        let control1 = Point2 {
            x: first.x + (last.x - previous.x) / 6.0,
            y: first.y + (last.y - previous.y) / 6.0,
        };
        let control2 = Point2 {
            x: last.x - (next.x - first.x) / 6.0,
            y: last.y - (next.y - first.y) / 6.0,
        };
        data.push_str(&format!(
            " C {} {} {} {} {} {}",
            f(control1.x),
            f(control1.y),
            f(control2.x),
            f(control2.y),
            f(last.x),
            f(last.y)
        ));
        bounds.extend([control1, control2, last].map(|point| transform.point(point)));
    }
    Some((data, bounds))
}

fn hatch_geometry(
    properties: &BTreeMap<String, Value>,
    transform: Matrix,
) -> (String, Vec<Point2>, bool) {
    let mut data = String::new();
    let mut all_points = Vec::new();
    let mut approximate = false;
    for contour in properties
        .get("contours")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(points) = contour.get("points").and_then(Value::as_array) {
            let points = points.iter().filter_map(value_point).collect::<Vec<_>>();
            let bulges = contour
                .get("bulges")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .map(|value| value.as_f64().unwrap_or(0.0))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if points.len() >= 2 {
                let (path, bounds) = path_from_bulges(
                    &points,
                    &bulges,
                    contour
                        .get("isClosed")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    transform,
                );
                data.push_str(&path);
                data.push(' ');
                all_points.extend(bounds);
            }
            continue;
        }

        let mut contour_data = String::new();
        let mut started = false;
        for segment in contour
            .get("segments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match segment.get("type").and_then(Value::as_str) {
                Some("line") => {
                    let points = segment
                        .get("points")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(value_point)
                        .collect::<Vec<_>>();
                    if points.len() >= 2 {
                        if !started {
                            contour_data.push_str(&format!(
                                "M {} {}",
                                f(points[0].x),
                                f(points[0].y)
                            ));
                            started = true;
                        }
                        contour_data.push_str(&format!(" L {} {}", f(points[1].x), f(points[1].y)));
                        all_points.extend(points.into_iter().map(|point| transform.point(point)));
                    }
                }
                Some("circularArc") => {
                    let center = segment.get("center").and_then(value_point);
                    let radius = segment.get("radius").and_then(Value::as_f64);
                    let start = segment.get("startAngle").and_then(Value::as_f64);
                    let end = segment.get("endAngle").and_then(Value::as_f64);
                    if let (Some(center), Some(radius), Some(start), Some(end)) =
                        (center, radius, start, end)
                    {
                        let is_ccw = segment
                            .get("isCcw")
                            .and_then(Value::as_bool)
                            .unwrap_or(true);
                        let (first, commands, points) = ellipse_arc_geometry(
                            center,
                            Point2 { x: radius, y: 0.0 },
                            1.0,
                            start,
                            end,
                            is_ccw,
                        );
                        if !started {
                            contour_data.push_str(&format!("M {} {}", f(first.x), f(first.y)));
                            started = true;
                        }
                        contour_data.push_str(&commands);
                        all_points.extend(points.into_iter().map(|point| transform.point(point)));
                    }
                }
                Some("ellipticalArc") => {
                    let center = segment.get("center").and_then(value_point);
                    let major = segment.get("majorAxisVector").and_then(value_point);
                    let ratio = segment.get("minorMajorRatio").and_then(Value::as_f64);
                    let start = segment.get("startAngle").and_then(Value::as_f64);
                    let end = segment.get("endAngle").and_then(Value::as_f64);
                    if let (Some(center), Some(major), Some(ratio), Some(start), Some(end)) =
                        (center, major, ratio, start, end)
                    {
                        let is_ccw = segment
                            .get("isCcw")
                            .and_then(Value::as_bool)
                            .unwrap_or(true);
                        let (first, commands, points) =
                            ellipse_arc_geometry(center, major, ratio, start, end, is_ccw);
                        if !started {
                            contour_data.push_str(&format!("M {} {}", f(first.x), f(first.y)));
                            started = true;
                        }
                        contour_data.push_str(&commands);
                        all_points.extend(points.into_iter().map(|point| transform.point(point)));
                    }
                }
                Some("spline") => {
                    if let Some(object) = segment.as_object() {
                        let spline = object
                            .iter()
                            .map(|(key, value)| (key.clone(), value.clone()))
                            .collect::<BTreeMap<_, _>>();
                        if let Some(points) = hatch_spline_points(&spline) {
                            if !started {
                                contour_data.push_str(&format!(
                                    "M {} {}",
                                    f(points[0].x),
                                    f(points[0].y)
                                ));
                                started = true;
                            }
                            for point in &points[1..] {
                                contour_data.push_str(&format!(" L {} {}", f(point.x), f(point.y)));
                            }
                            all_points
                                .extend(points.into_iter().map(|point| transform.point(point)));
                            approximate = true;
                        }
                    }
                }
                _ => {}
            }
        }
        if started {
            if contour
                .get("isClosed")
                .and_then(Value::as_bool)
                .unwrap_or(true)
            {
                contour_data.push_str(" Z");
            }
            data.push_str(&contour_data);
            data.push(' ');
        }
    }
    (data, all_points, approximate)
}

fn ellipse_arc_geometry(
    center: Point2,
    major: Point2,
    ratio: f64,
    start: f64,
    end: f64,
    is_ccw: bool,
) -> (Point2, String, Vec<Point2>) {
    let rx = major.x.hypot(major.y);
    let ry = rx * ratio.abs();
    let rotation_radians = major.y.atan2(major.x);
    let rotation = rotation_radians.to_degrees();
    let point_at = |angle: f64| {
        let (sin_rotation, cos_rotation) = rotation_radians.sin_cos();
        Point2 {
            x: center.x + rx * angle.cos() * cos_rotation - ry * angle.sin() * sin_rotation,
            y: center.y + rx * angle.cos() * sin_rotation + ry * angle.sin() * cos_rotation,
        }
    };
    let mut delta = if is_ccw {
        (end - start).rem_euclid(TAU)
    } else {
        (start - end).rem_euclid(TAU)
    };
    if delta < 1e-9 {
        delta = TAU;
    }
    let first = point_at(start);
    let direction = if is_ccw { 1.0 } else { -1.0 };
    let sweep = i32::from(is_ccw);
    let mut commands = String::new();
    if delta >= TAU - 1e-7 {
        let middle = point_at(start + direction * std::f64::consts::PI);
        commands.push_str(&format!(
            " A {} {} {} 0 {} {} {} A {} {} {} 0 {} {} {}",
            f(rx),
            f(ry),
            f(rotation),
            sweep,
            f(middle.x),
            f(middle.y),
            f(rx),
            f(ry),
            f(rotation),
            sweep,
            f(first.x),
            f(first.y)
        ));
    } else {
        let last = point_at(start + direction * delta);
        commands.push_str(&format!(
            " A {} {} {} {} {} {} {}",
            f(rx),
            f(ry),
            f(rotation),
            i32::from(delta > std::f64::consts::PI),
            sweep,
            f(last.x),
            f(last.y)
        ));
    }
    let points = (0..=48)
        .map(|index| point_at(start + direction * delta * index as f64 / 48.0))
        .collect();
    (first, commands, points)
}

fn hatch_spline_points(properties: &BTreeMap<String, Value>) -> Option<Vec<Point2>> {
    let controls = spline_controls(properties);
    let degree = number(properties, "degree").unwrap_or(3.0) as usize;
    let knots = properties
        .get("knots")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_f64).collect::<Vec<_>>())
        .unwrap_or_default();
    if !controls.is_empty() && knots.len() >= controls.len() + degree + 1 {
        return adaptive_nurbs_points(&controls, &knots, degree, Matrix::IDENTITY);
    }
    None
}

fn sampled_curve(
    start: f64,
    mut end: f64,
    count: usize,
    point: impl Fn(f64) -> Point2,
) -> Vec<Point2> {
    if end < start {
        end += TAU;
    }
    if (end - start).abs() < 1e-9 {
        end = start + TAU;
    }
    (0..=count)
        .map(|i| {
            let t = i as f64 / count as f64;
            point(start + (end - start) * t)
        })
        .collect()
}

fn items_bounds(items: &[DisplayItem]) -> Option<Bounds> {
    let mut bounds = Bounds::empty();
    for item in items {
        if let Some(item_bounds) = item_effective_bounds(item) {
            bounds.include_bounds(item_bounds);
        }
    }
    bounds.valid().then_some(bounds)
}

fn item_effective_bounds(item: &DisplayItem) -> Option<Bounds> {
    item.clip
        .map_or(Some(item.bounds), |clip| item.bounds.intersection(clip))
}

fn automatic_bounds(items: &[DisplayItem], diagnostics: &mut RenderDiagnostics) -> Option<Bounds> {
    let full = items_bounds(items)?;
    let effective = items
        .iter()
        .filter_map(item_effective_bounds)
        .collect::<Vec<_>>();
    if effective.len() < ROBUST_FIT_MIN_ITEMS {
        return Some(full);
    }

    let trim = (effective.len() / ROBUST_FIT_TRIM_DIVISOR).max(1);
    if trim * 2 >= effective.len() {
        return Some(full);
    }

    let mut robust = full;
    let mut adjusted = Vec::new();
    for axis in 0..2 {
        let mut mins = effective
            .iter()
            .map(|bounds| bounds.min[axis])
            .collect::<Vec<_>>();
        let mut maxs = effective
            .iter()
            .map(|bounds| bounds.max[axis])
            .collect::<Vec<_>>();
        mins.sort_by(f64::total_cmp);
        maxs.sort_by(f64::total_cmp);
        let candidate_min = mins[trim];
        let candidate_max = maxs[effective.len() - trim - 1];
        let full_span = full.max[axis] - full.min[axis];
        let candidate_span = candidate_max - candidate_min;
        if candidate_span > 0.0 && full_span / candidate_span >= ROBUST_FIT_OUTLIER_RATIO {
            let safety_margin = candidate_span * 0.1;
            robust.min[axis] = candidate_min - safety_margin;
            robust.max[axis] = candidate_max + safety_margin;
            adjusted.push(if axis == 0 { "x" } else { "y" });
        }
    }

    if adjusted.is_empty() || !robust.valid() {
        return Some(full);
    }
    diagnostics.warnings.push(format!(
        "automatic fit ignored extreme {}-axis extents; use an explicit region to include them (full bounds: [{}, {}] to [{}, {}])",
        adjusted.join("/"),
        f(full.min[0]),
        f(full.min[1]),
        f(full.max[0]),
        f(full.max[1])
    ));
    Some(robust)
}

fn fit_output_aspect(mut bounds: Bounds, width: u32, height: u32) -> Bounds {
    let target = f64::from(width) / f64::from(height);
    let current_width = bounds.max[0] - bounds.min[0];
    let current_height = bounds.max[1] - bounds.min[1];
    let current = current_width / current_height;
    if current < target {
        let extra = (current_height * target - current_width) / 2.0;
        bounds.min[0] -= extra;
        bounds.max[0] += extra;
    } else if current > target {
        let extra = (current_width / target - current_height) / 2.0;
        bounds.min[1] -= extra;
        bounds.max[1] += extra;
    }
    bounds
}

fn layout_bounds(entity: &SourceEntity) -> Option<Bounds> {
    let min = point(&entity.properties, "LIMMIN")?;
    let max = point(&entity.properties, "LIMMAX")?;
    let bounds = Bounds {
        min: [min.x, min.y],
        max: [max.x, max.y],
    };
    bounds.valid().then_some(bounds)
}

fn svg_document(
    items: &[DisplayItem],
    region: Bounds,
    width: u32,
    height: u32,
    background: RenderBackground,
) -> Result<String, RenderError> {
    ensure_svg_size(items)?;
    let bg = match background {
        RenderBackground::Transparent => None,
        RenderBackground::Model | RenderBackground::Black => Some("#111827"),
        RenderBackground::Paper | RenderBackground::White => Some("#ffffff"),
    };
    let root_style = bg
        .map(|color| format!(" style=\"background:{color}\""))
        .unwrap_or_default();
    let view_width = region.max[0] - region.min[0];
    let view_height = region.max[1] - region.min[1];
    let stroke_width = (view_width / f64::from(width))
        .max(view_height / f64::from(height))
        .max(f64::EPSILON);
    let mut body = String::new();
    let mut clips = HashMap::<String, usize>::new();
    let mut defs = String::new();
    for item in items {
        if let Some(clip) = item.clip {
            let key = format!(
                "{:.9},{:.9},{:.9},{:.9}",
                clip.min[0], clip.min[1], clip.max[0], clip.max[1]
            );
            if !clips.contains_key(&key) {
                let id = clips.len();
                clips.insert(key, id);
                defs.push_str(&format!("<clipPath id=\"clip{id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>", f(clip.min[0]), f(clip.min[1]), f(clip.max[0]-clip.min[0]), f(clip.max[1]-clip.min[1])));
            }
        }
    }
    for item in items {
        let clip_attr = item
            .clip
            .map(|clip| {
                let key = format!(
                    "{:.9},{:.9},{:.9},{:.9}",
                    clip.min[0], clip.min[1], clip.max[0], clip.max[1]
                );
                format!(" clip-path=\"url(#clip{})\"", clips[&key])
            })
            .unwrap_or_default();
        let meta = format!(
            " data-handle=\"{}\" data-type=\"{}\" data-method=\"{}\"",
            escape(&item.handle),
            escape(&item.type_name),
            item.method
        );
        let color = if item.type_name == "AcDbWipeout" {
            bg.unwrap_or("#ffffff")
        } else if matches!(
            background,
            RenderBackground::Model | RenderBackground::Black
        ) && item.color == "#111827"
        {
            "#f9fafb"
        } else {
            item.color.as_str()
        };
        match &item.graphic {
            Graphic::Path {
                data,
                fill,
                transform,
            } => {
                let transform_attr = transform
                    .map(|matrix| {
                        format!(
                            " transform=\"{}\" vector-effect=\"non-scaling-stroke\"",
                            matrix.svg()
                        )
                    })
                    .unwrap_or_default();
                let fill_opacity = if item.type_name == "AcDbWipeout" {
                    "1"
                } else if *fill {
                    "0.18"
                } else {
                    "0"
                };
                let stroke = if item.type_name == "AcDbWipeout" {
                    "none"
                } else {
                    color
                };
                body.push_str(&format!("<g{clip_attr}{meta}><path d=\"{data}\" stroke=\"{stroke}\" stroke-width=\"{}\" fill=\"{}\" fill-rule=\"evenodd\" fill-opacity=\"{fill_opacity}\"{transform_attr}/></g>", f(stroke_width), if *fill { color } else { "none" }));
            }
            Graphic::Text {
                point,
                text,
                height,
                rotation,
                anchor,
            } => {
                let content = text
                    .lines()
                    .enumerate()
                    .map(|(index, line)| {
                        format!(
                            "<tspan x=\"0\" dy=\"{}\">{}</tspan>",
                            if index == 0 {
                                "0".to_owned()
                            } else {
                                f(*height * 1.2)
                            },
                            escape(line)
                        )
                    })
                    .collect::<String>();
                body.push_str(&format!("<text x=\"0\" y=\"0\" text-anchor=\"{anchor}\" font-family=\"sans-serif\" font-size=\"{}\" fill=\"{color}\" transform=\"translate({} {}) rotate({}) scale(1 -1)\"{clip_attr}{meta}>{content}</text>", f(*height), f(point.x), f(point.y), f(-rotation.to_degrees())));
            }
        }
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"{} {} {} {}\"{root_style}>{}<defs>{defs}</defs><g transform=\"translate(0 {}) scale(1 -1)\">{body}</g></svg>",
        f(region.min[0]),
        f(region.min[1]),
        f(view_width),
        f(view_height),
        bg.map(|color| format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{color}\"/>",
            f(region.min[0]),
            f(region.min[1]),
            f(view_width),
            f(view_height)
        ))
        .unwrap_or_default(),
        f(region.min[1] + region.max[1])
    ))
}

fn ensure_svg_size(items: &[DisplayItem]) -> Result<(), RenderError> {
    let mut estimated = 1024usize;
    for item in items {
        let content = match &item.graphic {
            Graphic::Path { data, .. } => data.len(),
            Graphic::Text { text, .. } => text.len(),
        };
        estimated = estimated
            .checked_add(content.saturating_add(320))
            .ok_or_else(|| RenderError::ResourceLimit("SVG size overflow".to_owned()))?;
        if estimated > MAX_SVG_BYTES {
            return Err(RenderError::ResourceLimit(format!(
                "estimated SVG exceeds {} MiB; request a smaller explicit region",
                MAX_SVG_BYTES / 1024 / 1024
            )));
        }
    }
    Ok(())
}

fn rasterize_svg(svg: &str, width: u32, height: u32) -> Result<Vec<u8>, RenderError> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &options)
        .map_err(|error| RenderError::Rasterization(error.to_string()))?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| RenderError::Rasterization("could not allocate output image".to_owned()))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|error| RenderError::Rasterization(error.to_string()))
}

fn path_from_points(points: &[Point2], closed: bool) -> String {
    let Some(first) = points.first() else {
        return String::new();
    };
    let mut data = format!("M {} {}", f(first.x), f(first.y));
    for point in &points[1..] {
        data.push_str(&format!(" L {} {}", f(point.x), f(point.y)));
    }
    if closed {
        data.push_str(" Z");
    }
    data
}

fn point(properties: &BTreeMap<String, Value>, key: &str) -> Option<Point2> {
    properties.get(key).and_then(value_point)
}
fn point3_xy(properties: &BTreeMap<String, Value>, key: &str) -> Option<Point2> {
    point(properties, key)
}
fn xy_point(properties: &BTreeMap<String, Value>) -> Option<Point2> {
    Some(Point2 {
        x: number(properties, "x")?,
        y: number(properties, "y")?,
    })
}
fn value_point(value: &Value) -> Option<Point2> {
    let values = value.as_array()?;
    Some(Point2 {
        x: values.first()?.as_f64()?,
        y: values.get(1)?.as_f64()?,
    })
}
fn number(properties: &BTreeMap<String, Value>, key: &str) -> Option<f64> {
    properties.get(key).and_then(Value::as_f64)
}
fn string<'a>(properties: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a str> {
    properties
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| *value != "0" && !value.is_empty())
}
fn matrix_scale(matrix: Matrix) -> f64 {
    ((matrix.a * matrix.a + matrix.b * matrix.b).sqrt()
        + (matrix.c * matrix.c + matrix.d * matrix.d).sqrt())
        / 2.0
}
fn matrix_rotation(matrix: Matrix) -> f64 {
    matrix.b.atan2(matrix.a)
}
fn f(value: f64) -> String {
    format!("{value:.6}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn clean_mtext(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if matches!(character, '{' | '}') {
            continue;
        }
        if character != '\\' {
            output.push(character);
            continue;
        }
        let Some(control) = chars.next() else {
            break;
        };
        match control {
            'P' => output.push('\n'),
            '~' => output.push(' '),
            '\\' | '{' | '}' => output.push(control),
            'S' => {
                let stacked = chars
                    .by_ref()
                    .take_while(|character| *character != ';')
                    .collect::<String>();
                output.push_str(&stacked.replace(['#', '^'], "/"));
            }
            'A' | 'C' | 'F' | 'H' | 'Q' | 'T' | 'W' | 'f' => {
                for next in chars.by_ref() {
                    if next == ';' {
                        break;
                    }
                }
            }
            _ => output.push(control),
        }
    }
    output
}

fn entity_color(entity: &SourceEntity, layers: &HashMap<String, String>) -> String {
    let color = number(&entity.properties, "color").unwrap_or(256.0) as i64;
    if color == 256 {
        string(&entity.properties, "layer")
            .and_then(|layer| layers.get(layer))
            .cloned()
            .unwrap_or_else(|| "#111827".to_owned())
    } else {
        aci_color(color, false)
    }
}

fn aci_color(index: i64, dark_background: bool) -> String {
    match index.abs() {
        1 => "#ef4444",
        2 => "#eab308",
        3 => "#22c55e",
        4 => "#06b6d4",
        5 => "#3b82f6",
        6 => "#d946ef",
        7 => {
            if dark_background {
                "#ffffff"
            } else {
                "#111827"
            }
        }
        8 => "#6b7280",
        9 => "#d1d5db",
        _ => "#111827",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entity(
        handle: &str,
        type_name: &str,
        block: Option<&str>,
        properties: Value,
    ) -> SourceEntity {
        SourceEntity {
            handle: handle.to_owned(),
            type_name: type_name.to_owned(),
            kind: "entity".to_owned(),
            properties: serde_json::from_value(properties).unwrap(),
            container_block_handle: block.map(str::to_owned),
            layout_handle: None,
        }
    }

    #[test]
    fn renders_model_geometry_to_svg() {
        let document = RenderDocument::new(vec![
            entity("C", "BLOCK_CONTROL", None, json!({"model_space":"M"})),
            entity(
                "1",
                "AcDbLine",
                Some("M"),
                json!({"start":[0,0,0],"end":[10,5,0],"color":1}),
            ),
            entity(
                "2",
                "AcDbText",
                Some("M"),
                json!({"ins_pt":[2,3],"text_value":"Room <A>","height":1}),
            ),
        ]);
        let output = document
            .render(RenderRequest {
                format: RenderFormat::Svg,
                ..serde_json::from_value(json!({})).unwrap()
            })
            .unwrap();
        let svg = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(output.data)
                .unwrap(),
        )
        .unwrap();
        assert!(svg.contains("data-handle=\"1\""));
        assert!(svg.contains("Room &lt;A&gt;"));
        assert_eq!(output.diagnostics.rendered_entities, 2);
    }

    #[test]
    fn deserializes_layout_and_viewport_targets_from_protocol_json() {
        let layout: RenderRequest = serde_json::from_value(json!({
            "target": {"kind": "layout", "layoutHandle": "2F37"}
        }))
        .unwrap();
        assert_eq!(
            layout.target,
            RenderTarget::Layout {
                layout_handle: "2F37".to_owned()
            }
        );

        let viewport: RenderRequest = serde_json::from_value(json!({
            "target": {"kind": "viewport", "viewportHandle": "2F56"}
        }))
        .unwrap();
        assert_eq!(
            viewport.target,
            RenderTarget::Viewport {
                viewport_handle: "2F56".to_owned()
            }
        );
    }

    #[test]
    fn list_views_uses_header_extents_without_compiling_model_geometry() {
        let document = RenderDocument::new(vec![
            entity("C", "BLOCK_CONTROL", None, json!({"model_space":"M"})),
            entity(
                "HEADER",
                "HEADER",
                None,
                json!({"EXTMIN":[-25,-10,0],"EXTMAX":[75,40,0]}),
            ),
        ]);

        let views = document.list_views().unwrap();
        assert_eq!(
            views[0].bounds,
            Bounds {
                min: [-25.0, -10.0],
                max: [75.0, 40.0]
            }
        );
    }

    #[test]
    fn preserves_ellipses_as_affine_svg_arcs() {
        let ellipse = entity(
            "E",
            "AcDbEllipse",
            Some("M"),
            json!({
                "center":[4,3,0],
                "sm_axis":[3,1,0],
                "axis_ratio":0.4,
                "start_angle":0,
                "end_angle":6.283185307179586
            }),
        );
        let item = display_item(&ellipse, Matrix::scale(2.0, 0.5), None, &HashMap::new()).unwrap();
        let Graphic::Path {
            data, transform, ..
        } = item.graphic
        else {
            panic!("expected a path")
        };
        assert!(data.matches(" A ").count() >= 2);
        assert!(!data.contains(" L "));
        assert_eq!(transform.unwrap().svg(), "matrix(2 0 0 0.5 0 0)");
    }

    #[test]
    fn renders_lightweight_polyline_bulges_as_arcs() {
        let properties = serde_json::from_value(json!({
            "points":[[0,0],[10,0],[10,10]],
            "bulges":[1,0,0],
            "flag":512
        }))
        .unwrap();
        let (data, bounds) = polyline_geometry(&properties, Matrix::IDENTITY).unwrap();
        assert!(data.contains(" A 5 5 0 0 1 10 0"));
        assert!(data.ends_with(" Z"));
        assert!(bounds.len() > 3);
    }

    #[test]
    fn renders_beziers_natively_and_general_nurbs_adaptively() {
        let bezier = serde_json::from_value(json!({
            "degree":3,
            "controlPoints":[
                {"point":[0,0],"weight":1},
                {"point":[2,5],"weight":1},
                {"point":[8,5],"weight":1},
                {"point":[10,0],"weight":1}
            ]
        }))
        .unwrap();
        let (graphic, _, method) = spline_geometry(&bezier, Matrix::IDENTITY).unwrap();
        assert_eq!(method, "native");
        assert!(matches!(graphic, Graphic::Path { data, .. } if data.contains(" C ")));

        let nurbs = serde_json::from_value(json!({
            "degree":2,
            "knots":[0,0,0,1,2,3,3,3],
            "controlPoints":[
                {"point":[0,0],"weight":1},
                {"point":[2,4],"weight":0.7},
                {"point":[5,-1],"weight":1.2},
                {"point":[8,3],"weight":1},
                {"point":[10,0],"weight":1}
            ]
        }))
        .unwrap();
        let (graphic, points, method) = spline_geometry(&nurbs, Matrix::IDENTITY).unwrap();
        assert_eq!(method, "adaptiveNurbs");
        assert!(points.len() > 6);
        assert!(matches!(graphic, Graphic::Path { data, .. } if data.matches(" L ").count() > 5));
    }

    #[test]
    fn preserves_curved_hatch_segments() {
        let properties = serde_json::from_value(json!({
            "is_solid_fill":true,
            "contours":[{
                "isClosed":true,
                "segments":[{
                    "type":"ellipticalArc",
                    "center":[0,0],
                    "majorAxisVector":[5,2],
                    "minorMajorRatio":0.5,
                    "startAngle":0,
                    "endAngle":3.141592653589793,
                    "isCcw":true
                },{
                    "type":"line",
                    "points":[[-5,-2],[5,2]]
                }]
            }]
        }))
        .unwrap();
        let (data, bounds, approximate) = hatch_geometry(&properties, Matrix::IDENTITY);
        assert!(data.contains(" A "));
        assert!(data.ends_with("Z "));
        assert!(!approximate);
        assert!(bounds.len() > 10);
    }

    #[test]
    fn dimensions_without_generated_blocks_do_not_draw_generic_point_fans() {
        let document = RenderDocument::new(vec![
            entity("C", "BLOCK_CONTROL", None, json!({"model_space":"M"})),
            entity(
                "1",
                "AcDbLine",
                Some("M"),
                json!({"start":[0,0,0],"end":[10,5,0]}),
            ),
            entity(
                "D",
                "AcDbRotatedDimension",
                Some("M"),
                json!({
                    "xline1_pt":[295,212,0],
                    "xline2_pt":[295,812,0],
                    "def_pt":[-143,586,0],
                    "text_midpt":[0,0]
                }),
            ),
        ]);
        let output = document
            .render(RenderRequest {
                format: RenderFormat::Svg,
                ..serde_json::from_value(json!({})).unwrap()
            })
            .unwrap();
        let svg = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(output.data)
                .unwrap(),
        )
        .unwrap();

        assert!(svg.contains("data-handle=\"1\""));
        assert!(!svg.contains("data-handle=\"D\""));
        assert_eq!(output.diagnostics.rendered_entities, 1);
        assert_eq!(
            output
                .diagnostics
                .unsupported_by_type
                .get("AcDbRotatedDimension"),
            Some(&1)
        );
    }

    #[test]
    fn dimensions_with_generated_blocks_still_render_their_graphics() {
        let document = RenderDocument::new(vec![
            entity("C", "BLOCK_CONTROL", None, json!({"model_space":"M"})),
            entity("D", "AcDbRotatedDimension", Some("M"), json!({"block":"B"})),
            entity(
                "G",
                "AcDbLine",
                Some("B"),
                json!({"start":[0,0,0],"end":[10,5,0]}),
            ),
        ]);
        let output = document
            .render(RenderRequest {
                format: RenderFormat::Svg,
                ..serde_json::from_value(json!({})).unwrap()
            })
            .unwrap();
        let svg = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(output.data)
                .unwrap(),
        )
        .unwrap();

        assert!(svg.contains("data-handle=\"G\""));
        assert_eq!(output.diagnostics.rendered_entities, 1);
        assert_eq!(output.diagnostics.fallbacks.get("generatedBlock"), Some(&1));
    }

    #[test]
    fn automatic_fit_ignores_only_extreme_sparse_extents() {
        let mut entities = vec![entity(
            "C",
            "BLOCK_CONTROL",
            None,
            json!({"model_space":"M"}),
        )];
        for index in 0..100 {
            let x = (index % 10) as f64;
            let y = (index / 10) as f64;
            entities.push(entity(
                &format!("L{index}"),
                "AcDbLine",
                Some("M"),
                json!({"start":[x,y,0],"end":[x + 1.0,y,0]}),
            ));
        }
        entities.push(entity(
            "LEFT",
            "AcDbLine",
            Some("M"),
            json!({"start":[-1_000_000,0,0],"end":[-999_999,0,0]}),
        ));
        entities.push(entity(
            "RIGHT",
            "AcDbLine",
            Some("M"),
            json!({"start":[999_999,0,0],"end":[1_000_000,0,0]}),
        ));

        let output = RenderDocument::new(entities)
            .render(RenderRequest {
                format: RenderFormat::Svg,
                ..serde_json::from_value(json!({})).unwrap()
            })
            .unwrap();

        assert!(output.rendered_region.min[0] > -100.0);
        assert!(output.rendered_region.max[0] < 100.0);
        assert!(
            output
                .diagnostics
                .warnings
                .iter()
                .any(|warning| warning.contains("automatic fit ignored extreme x-axis"))
        );
    }

    #[test]
    fn explicit_region_is_exact_and_culls_remote_items() {
        let document = RenderDocument::new(vec![
            entity("C", "BLOCK_CONTROL", None, json!({"model_space":"M"})),
            entity(
                "NEAR",
                "AcDbLine",
                Some("M"),
                json!({"start":[0,0,0],"end":[1,1,0]}),
            ),
            entity(
                "FAR",
                "AcDbLine",
                Some("M"),
                json!({"start":[1000,1000,0],"end":[1001,1001,0]}),
            ),
        ]);
        let region = Bounds {
            min: [-1.0, -1.0],
            max: [2.0, 2.0],
        };
        let output = document
            .render(RenderRequest {
                region: Some(region),
                format: RenderFormat::Svg,
                ..serde_json::from_value(json!({})).unwrap()
            })
            .unwrap();

        assert_eq!(output.rendered_region, region);
        assert_eq!(output.diagnostics.rendered_entities, 1);
    }

    #[test]
    fn wipeout_masks_with_the_render_background() {
        let document = RenderDocument::new(vec![
            entity("C", "BLOCK_CONTROL", None, json!({"model_space":"M"})),
            entity(
                "W",
                "AcDbWipeout",
                Some("M"),
                json!({
                    "pt0":[10,20,0],
                    "uvec":[4,0,0],
                    "vvec":[0,2,0],
                    "image_size":[1,1]
                }),
            ),
        ]);
        let output = document
            .render(RenderRequest {
                format: RenderFormat::Svg,
                background: RenderBackground::Black,
                ..serde_json::from_value(json!({})).unwrap()
            })
            .unwrap();
        let svg = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(output.data)
                .unwrap(),
        )
        .unwrap();

        assert!(svg.contains("data-type=\"AcDbWipeout\""));
        assert!(svg.contains("fill=\"#111827\""));
        assert!(svg.contains("fill-opacity=\"1\""));
        assert_eq!(
            output.diagnostics.fallbacks.get("rectangularWipeout"),
            Some(&1)
        );
        assert!(
            !output
                .diagnostics
                .unsupported_by_type
                .contains_key("AcDbWipeout")
        );
    }

    #[test]
    fn rejects_oversized_svg_before_serialization() {
        let item = DisplayItem {
            handle: "1".to_owned(),
            type_name: "AcDbLine".to_owned(),
            color: "#000000".to_owned(),
            graphic: Graphic::Path {
                data: "x".repeat(MAX_SVG_BYTES),
                fill: false,
                transform: None,
            },
            bounds: Bounds {
                min: [0.0, 0.0],
                max: [1.0, 1.0],
            },
            clip: None,
            method: "native",
        };

        assert!(matches!(
            ensure_svg_size(&[item]),
            Err(RenderError::ResourceLimit(_))
        ));
    }
}
