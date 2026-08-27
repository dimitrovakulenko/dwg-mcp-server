# DWG Rendering Architecture

## Purpose

Add read-only image rendering for opened DWG documents. Rendering must support:

- model space, paper-space layouts, and individual layout viewports;
- a caller-selected rectangular region;
- broad 2D entity coverage with explicit fallbacks;
- SVG and PNG output through MCP;
- useful diagnostics when a drawing cannot be rendered exactly.

The initial goal is a useful 2D preview, not pixel-perfect AutoCAD rendering or
photorealistic 3D rendering.

## Public MCP tools

### `dwg.list_render_views`

Lists the renderable views of an opened document. This avoids requiring clients
to reconstruct layout and viewport relationships with object queries.

Input:

```json
{
  "documentId": "document-id"
}
```

Result:

```json
{
  "documentId": "document-id",
  "views": [
    {
      "id": "model",
      "kind": "model",
      "name": "Model",
      "bounds": {
        "min": [0.0, 0.0],
        "max": [50000.0, 30000.0]
      }
    },
    {
      "id": "layout:2F37",
      "kind": "layout",
      "name": "Ground Floor",
      "layoutHandle": "2F37",
      "bounds": {
        "min": [0.0, 0.0],
        "max": [420.0, 297.0]
      },
      "viewportHandles": ["301A", "301B"]
    },
    {
      "id": "viewport:301A",
      "kind": "viewport",
      "name": "Ground Floor / viewport 301A",
      "layoutHandle": "2F37",
      "viewportHandle": "301A",
      "bounds": {
        "min": [-5000.0, -3000.0],
        "max": [5000.0, 3000.0]
      }
    }
  ]
}
```

All handles remain uppercase hexadecimal strings. Bounds are calculated from
renderable geometry when practical; stored DWG extents may be used as a fallback
and identified in diagnostics.

### `dwg.render_view`

Renders one target from an opened document.

Input:

```json
{
  "documentId": "document-id",
  "target": {
    "kind": "layout",
    "layoutHandle": "2F37"
  },
  "region": {
    "min": [50.0, 30.0],
    "max": [250.0, 180.0]
  },
  "width": 1600,
  "height": 1200,
  "format": "png",
  "background": "paper",
  "padding": 0.02
}
```

`target` is exactly one of:

```json
{"kind": "model"}
{"kind": "layout", "layoutHandle": "2F37"}
{"kind": "viewport", "viewportHandle": "301A"}
```

Input rules:

- `region` is optional. Its coordinates are in the selected target's coordinate
  system.
- A model region uses the rendered model view plane.
- A layout region uses paper-space drawing units.
- A viewport region uses its projected model view coordinates.
- When `region` is omitted, the target is fitted to the output. Extreme sparse
  extents are excluded only when they are at least 20 times larger than the
  central 98% of entity bounds; diagnostics report the full bounds so a caller
  can request them explicitly.
- Automatic fit expands the region to the output aspect ratio. An explicit
  `region` is returned unchanged.
- `width` and `height` are pixel dimensions and have server-enforced limits.
- `format` is `png` or `svg`.
- `background` is `model`, `paper`, `transparent`, `white`, or `black`.
- `padding` is a fraction of the fitted region and defaults to `0.02`.

The MCP result contains an image content block. PNG uses `image/png`; SVG uses
`image/svg+xml`. Structured content contains metadata and diagnostics but not a
second copy of the encoded image:

```json
{
  "documentId": "document-id",
  "mimeType": "image/png",
  "width": 1600,
  "height": 1200,
  "renderedRegion": {
    "min": [50.0, 30.0],
    "max": [250.0, 180.0]
  },
  "renderedEntities": 3841,
  "fallbacks": {
    "generatedBlock": 72,
    "proxyGraphics": 14,
    "approximation": 3
  },
  "unsupportedByType": {
    "AcDb3dSolid": 3
  },
  "warnings": []
}
```

## Runtime flow

```text
dwg.render_view
  -> Python MCP host
  -> renderView worker request
  -> native DWG adapter
  -> compiled render document
  -> target compositor
  -> SVG backend
  -> optional PNG rasterization
  -> MCP image content and diagnostics
```

The Python host validates the MCP input, routes by `documentId`, and constructs
the MCP result. Geometry extraction, layout composition, and image generation
remain in the per-document Rust worker. The server does not expose temporary
paths or write rendered files beside the source DWG.

## Rust boundaries

Add a renderer-independent crate, tentatively `dwg-render-core`, containing:

- render request and response models;
- geometry, transforms, clipping, and bounds;
- the display-list model;
- model, layout, and viewport composition;
- SVG generation and PNG rasterization;
- coverage diagnostics.

`dwg-libredwg` converts LibreDWG-owned DWG or DXF data into owned render data. It must not
return native pointers across the FFI boundary. `dwg-worker-core` owns the
worker protocol extension and invokes rendering through the existing document
trait. The Python host remains unaware of native DWG structures.

## Compiled render document

Rendering uses a document-level scene representation separate from the query
index:

```text
RenderDocument
  styles
    layers
    linetypes
    text styles
    dimension styles
  blocks
    block handle -> compiled symbol
  model space
    entity instances and spatial bounds
  layouts
    paper settings
    paper-space entity instances
    viewport definitions
  diagnostics
```

The scene is compiled lazily on the first rendering request and cached for the
lifetime of the opened document. Block symbols and reusable geometry are
compiled once. Camera-dependent projection, clipping, and output scaling are
performed for each request.

The query index and render document may share owned source properties later,
but rendering must not be implemented by repeatedly calling the public query
API.

## Display list

Entity adapters emit a small backend-independent command set:

```text
StrokePath
FillPath
TextRun
RasterImage
BlockInstance
PushTransform / PopTransform
PushClip / PopClip
```

Each command retains:

- source entity handle and type;
- layer handle;
- conservative bounds;
- resolved visual properties;
- rendering method: native, generated block, proxy graphics, or approximation.

SVG is the canonical output backend because it directly represents paths,
text, transforms, reused block symbols, and clipping. PNG is rasterized in
memory from the generated SVG so the two formats use identical geometry.

Curve geometry is preserved as far as SVG permits: circles, circular arcs,
ellipses, elliptical arcs, lightweight-polyline bulges, and quadratic/cubic
Bezier segments use native SVG arc or Bezier commands under the original affine
transform. General and rational NURBS are evaluated with de Boor's algorithm and
adaptively subdivided; these entities report `adaptiveNurbs` in diagnostics.

## Entity rendering strategy

The fallback order for each graphical entity is:

1. A typed native adapter when one is implemented and reliable.
2. The entity's generated block representation when present.
3. Its stored proxy graphics when present and valid.
4. A documented approximation when it conveys useful geometry.
5. An unsupported diagnostic.

Fallbacks must be visible in result diagnostics. Malformed data must skip only
the affected entity when safe, rather than failing the complete render.

### Native geometry

Initial typed adapters cover:

- LINE, XLINE, and RAY;
- ARC, CIRCLE, and ELLIPSE;
- LWPOLYLINE and 2D/3D POLYLINE;
- SPLINE;
- POINT;
- SOLID, TRACE, and 3DFACE;
- HATCH and MPOLYGON;
- WIPEOUT;
- IMAGE when its referenced file is already authorized and available.

Shared geometry code handles object-coordinate-system transforms, extrusion,
polyline bulges and widths, closed contours, curve flattening, and conservative
bounds. Flattening tolerance is derived from output pixels and the selected
view, not a fixed drawing-unit value.

### Blocks and attributes

Each block definition is compiled once as a reusable symbol. INSERT and MINSERT
apply translation, rotation, scale, array offsets, and extrusion transforms.
Nested inserts use cycle detection and a recursion limit.

Attached ATTRIB entities override or supplement block ATTDEF text. Dynamic
block references should render their effective anonymous block representation
when available.

### Dimensions

Dimension entities commonly reference an anonymous generated block containing
their final lines, arrows, and text. Rendering that block is the preferred path
for linear, aligned, rotated, radial, diametric, angular, ordinate, arc, and
large-radial dimensions.

Semantic reconstruction is a fallback for missing generated blocks. It is added
per dimension type and uses resolved dimension style, measurement formatting,
text placement, extension lines, and arrowhead blocks.

### Text

A shared text engine handles TEXT, MTEXT, ATTRIB, ATTDEF, dimension text,
leaders, and table text. It resolves text styles, alignment, rotation, width,
oblique angle, wrapping, and supported MTEXT formatting.

The packaged server includes deterministic fallback fonts. Missing SHX fonts
initially map to a fallback font and produce a warning. Native SHX glyph support
can be added without changing the display-list interface.

### Proxy graphics

LibreDWG exposes binary proxy graphics on entities. A bounds-checked decoder
converts supported proxy opcodes into the same display-list commands. This is
the principal fallback for custom objects and entity types whose semantic DWG
representation is incomplete.

The decoder must enforce payload, record-count, point-count, recursion, and
allocation limits before reading native-owned memory.

### External content

XREFs, raster images, PDF/DGN/DWF underlays, and fonts can refer to external
files. Rendering never broadens file authorization:

- paths are resolved through the host's existing access-control policy;
- missing or unauthorized references produce diagnostics;
- no reference may cause arbitrary network access;
- recursive XREF loading uses cycle and depth limits.

External reference rendering is not required for the first vertical slice.

## Model-space rendering

The default model target uses a top view of model space. The renderer computes
geometry bounds, applies the optional region, preserves aspect ratio, and maps
the result into the requested pixel dimensions.

Future camera parameters can extend the model target without changing layout or
viewport semantics. Initial 3D entities are projected as wireframe where useful;
hidden-line removal, lighting, materials, and ACIS surface rendering are outside
the initial scope.

## Layout rendering

A layout is composed in paper-space coordinates:

1. Resolve paper dimensions, margins, origin, units, and plot rotation.
2. Render entities owned by the layout's paper-space block.
3. Resolve enabled VIEWPORT entities belonging to the layout.
4. For each model viewport, project model-space content into its paper-space
   rectangle or custom clipping boundary.
5. Apply viewport-specific frozen layers and visibility.
6. Respect viewport stacking, paper-space draw order, and viewport-border layer
   visibility.
7. Clip the completed layout to the requested paper-space region.

The special paper-space viewport representing the sheet itself is not treated
as a model-space window.

## Viewport transforms

A viewport transform uses:

- paper-space center, width, and height;
- model-space view target and direction;
- model-space view center and view height;
- twist angle;
- front and back clipping planes;
- perspective lens settings when enabled;
- rectangular or referenced custom clipping boundary;
- viewport-frozen layers.

Orthographic projection is implemented first. Perspective projection can be
added behind the same camera interface. Unsupported camera modes must produce a
warning instead of silently using an unrelated view.

Rendering an individual viewport omits surrounding paper-space entities and
returns only that viewport's projected model content.

## Regions and spatial selection

Every compiled entity or block instance has conservative bounds. A spatial
index selects candidate geometry intersecting the requested region.

For layout crops, the renderer intersects the paper region with each viewport
clip, maps that intersection back into the viewport's model view, and queries
only relevant model entities. Geometry with uncertain bounds remains eligible
so cropping does not incorrectly hide valid content.

The returned `renderedRegion` reports the actual region after aspect-ratio and
padding adjustments.

## Visual properties

Property resolution follows DWG inheritance:

- entity values;
- BYBLOCK values from the containing insert;
- BYLAYER values from the entity's layer;
- viewport layer overrides where available;
- layout plot settings and optional plot style information.

The renderer should preserve colors, visibility, lineweight, linetype, linetype
scale, fill, and transparency when supported. Output options may intentionally
override these, for example monochrome rendering, but defaults should reflect
the drawing.

## Limits and safety

Server-controlled limits include:

- at most 16,777,216 output pixels;
- at most 250,000 drawable display items per request;
- at most 32 MiB of estimated SVG before serialization or PNG rasterization;
- render timeout;
- maximum entity, point, path, and text-run counts;
- maximum block/XREF recursion depth;
- maximum proxy payload and decoded command counts;
- maximum curve subdivision depth.

An explicit region culls nonintersecting display items while blocks are
expanded, so callers can render useful parts of a drawing that is too dense for
a complete-view response. Limit failures use the deterministic
`resource_limit` worker error code and never silently truncate geometry.

Rendering remains read-only. The worker does not execute drawing-provided code,
shell commands, macros, or network requests. Native failures remain isolated to
the existing per-document worker process.

## Diagnostics

Each render reports:

- rendered entity count;
- fallback counts by rendering method;
- unsupported counts by source type;
- malformed or skipped entity handles, capped to a small sample;
- missing fonts and external references;
- stale-extents or approximate-camera warnings.

Diagnostics are deterministic and concise enough for an agent to decide whether
the image is trustworthy or another view should be requested.

## Verification

Tests are layered:

1. Unit tests for transforms, bounds, clipping, OCS conversion, bulges, and
   property inheritance.
2. Display-list snapshots for one fixture per entity adapter.
3. SVG structural tests for handles, paths, clips, transforms, and text.
4. PNG visual regression tests with perceptual tolerance.
5. MCP stdio tests confirming image content, metadata, limits, and errors.
6. Fixture tests for model regions, multiple layouts, multiple viewports,
   rotated/scaled viewports, custom clips, and viewport-frozen layers.

The future external verification tool should compare both whole images and
targeted crops. A render manifest records the fixture, target, region, size,
renderer version, fallback counts, and unsupported entities for every baseline.

## Delivery sequence

1. Add worker protocol models, MCP tool schemas, and image result handling.
2. Implement the display list, bounds, SVG backend, and PNG rasterization.
3. Render model-space regions with basic curves and polylines.
4. Add blocks, attributes, visual property inheritance, and text.
5. Add layout discovery and paper-space rendering.
6. Add viewport transforms, clipping, and viewport layer visibility.
7. Add generated dimension blocks and semantic dimension fallbacks.
8. Generalize proxy graphics decoding and expand remaining entity adapters.
9. Integrate visual verification and publish coverage diagnostics.

The first usable vertical slice is steps 1–3. Later steps improve coverage
without changing the public tool contract or display-list architecture.
