use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use base64::Engine;
use dwg_libredwg::{LibreDwgFactory, describe_supported_type, list_supported_types};
use dwg_worker_core::{
    BackendFactory, Bounds, DwgDocument, FilterOperator, GetObjectsRequest, Projection,
    PropertyFilter, QueryMode, QueryObjectsRequest, QueryScope, QuerySpace, RelationDirection,
    RelationFilter, RenderBackground, RenderFormat, RenderRequest, RenderTarget,
    SetEntityPropertiesRequest, SortDirection, SortSpec, StdioHandler, WorkerError,
};
use serde_json::json;

fn libredwg_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_libredwg() -> std::sync::MutexGuard<'static, ()> {
    libredwg_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testData/house_plan.dwg")
}

fn dyn_blocks_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testData/dyn-blocks.dwg")
}

fn table_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testData/table.dwg")
}

fn dxf_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testData/render-smoke.dxf")
}

fn push_binary_group_code(bytes: &mut Vec<u8>, width: u8, code: u16) {
    if width == 1 && code < 255 {
        bytes.push(code as u8);
    } else if width == 1 {
        bytes.push(0xff);
        bytes.extend_from_slice(&code.to_le_bytes());
    } else {
        bytes.extend_from_slice(&code.to_le_bytes());
    }
}

fn push_binary_string(bytes: &mut Vec<u8>, width: u8, code: u16, value: &str) {
    push_binary_group_code(bytes, width, code);
    bytes.extend_from_slice(value.as_bytes());
    bytes.push(0);
}

fn push_binary_i16(bytes: &mut Vec<u8>, width: u8, code: u16, value: i16) {
    push_binary_group_code(bytes, width, code);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_binary_f64(bytes: &mut Vec<u8>, width: u8, code: u16, value: f64) {
    push_binary_group_code(bytes, width, code);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn binary_dxf(version: &str, group_code_width: u8) -> Vec<u8> {
    let mut bytes = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
    push_binary_string(&mut bytes, group_code_width, 0, "SECTION");
    push_binary_string(&mut bytes, group_code_width, 2, "HEADER");
    push_binary_string(&mut bytes, group_code_width, 9, "$ACADVER");
    push_binary_string(&mut bytes, group_code_width, 1, version);
    if group_code_width == 2 {
        push_binary_string(&mut bytes, group_code_width, 9, "$ENDCAPS");
        push_binary_i16(&mut bytes, group_code_width, 280, 0);
    }
    push_binary_string(&mut bytes, group_code_width, 0, "ENDSEC");
    push_binary_string(&mut bytes, group_code_width, 0, "SECTION");
    push_binary_string(&mut bytes, group_code_width, 2, "ENTITIES");
    for offset in [0.0, 1.0, 2.0] {
        push_binary_string(&mut bytes, group_code_width, 0, "LINE");
        push_binary_string(&mut bytes, group_code_width, 8, "0");
        push_binary_f64(&mut bytes, group_code_width, 10, offset);
        push_binary_f64(&mut bytes, group_code_width, 20, 0.0);
        push_binary_f64(&mut bytes, group_code_width, 30, 0.0);
        push_binary_f64(&mut bytes, group_code_width, 11, offset + 1.0);
        push_binary_f64(&mut bytes, group_code_width, 21, 1.0);
        push_binary_f64(&mut bytes, group_code_width, 31, 0.0);
    }
    push_binary_string(&mut bytes, group_code_width, 0, "ENDSEC");
    push_binary_string(&mut bytes, group_code_width, 0, "EOF");
    bytes
}

fn contains_2d_point(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(items) => {
            if items.len() == 2 && items.iter().all(serde_json::Value::is_number) {
                return true;
            }
            items.iter().any(contains_2d_point)
        }
        serde_json::Value::Object(items) => items.values().any(contains_2d_point),
        _ => false,
    }
}

#[test]
fn block_reference_catalog_marks_only_supported_properties_writable() {
    let block_reference = describe_supported_type("AcDbBlockReference")
        .expect("block reference type should be described");
    let writable = block_reference
        .properties
        .iter()
        .filter(|property| property.writable)
        .map(|property| property.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(writable, vec!["ins_pt", "scale", "rotation"]);
    assert!(
        block_reference
            .properties
            .iter()
            .find(|property| property.name == "rotation")
            .and_then(|property| property.description.as_deref())
            .is_some_and(|description| description.contains("radians"))
    );
    assert!(
        block_reference
            .properties
            .iter()
            .find(|property| property.name == "ins_pt")
            .and_then(|property| property.description.as_deref())
            .is_some_and(|description| description.contains("OCS"))
    );
}

#[test]
fn block_reference_properties_can_be_changed_in_memory() {
    let _guard = lock_libredwg();
    let mut document = LibreDwgFactory
        .open(&fixture_path())
        .expect("fixture should open");

    let updated = document
        .set_entity_properties(SetEntityPropertiesRequest {
            handle: "2AD".to_owned(),
            properties: BTreeMap::from([
                ("ins_pt".to_owned(), json!([101.0, 202.0, 3.0])),
                ("rotation".to_owned(), json!(1.25)),
                ("scale".to_owned(), json!([-2.0, -2.0, -2.0])),
            ]),
            projection: Projection::Full,
            select: Some(vec![
                "ins_pt".to_owned(),
                "rotation".to_owned(),
                "scale".to_owned(),
                "scale_flag".to_owned(),
            ]),
        })
        .expect("block reference transform should update");

    assert!(updated.dirty);
    assert_eq!(
        updated.item.properties["ins_pt"],
        json!([101.0, 202.0, 3.0])
    );
    assert_eq!(updated.item.properties["rotation"], json!(1.25));
    assert_eq!(updated.item.properties["scale"], json!([-2.0, -2.0, -2.0]));
    assert_eq!(updated.item.properties["scale_flag"], json!(2));

    let error = document
        .set_entity_properties(SetEntityPropertiesRequest {
            handle: "2AD".to_owned(),
            properties: BTreeMap::from([("block_header".to_owned(), json!("CA"))]),
            projection: Projection::Summary,
            select: None,
        })
        .expect_err("block definition handle must remain read-only");
    assert!(matches!(error, WorkerError::PropertyNotWritable(_)));

    let error = document
        .set_entity_properties(SetEntityPropertiesRequest {
            handle: "2AD".to_owned(),
            properties: BTreeMap::from([
                ("ins_pt".to_owned(), json!([9.0, 9.0, 9.0])),
                ("scale".to_owned(), json!([1.0, 0.0, 1.0])),
            ]),
            projection: Projection::Summary,
            select: None,
        })
        .expect_err("all property values must validate before mutation");
    assert!(matches!(error, WorkerError::InvalidPropertyValue(_)));
    let unchanged = document
        .get_objects(GetObjectsRequest {
            handles: vec!["2AD".to_owned()],
            projection: Projection::Full,
            select: Some(vec!["ins_pt".to_owned()]),
        })
        .expect("updated entity should remain readable");
    assert_eq!(
        unchanged.items[0].properties["ins_pt"],
        json!([101.0, 202.0, 3.0])
    );
}

#[test]
fn unsafe_block_reference_transforms_are_rejected_before_mutation() {
    let _guard = lock_libredwg();
    let mut document = LibreDwgFactory
        .open(&dyn_blocks_fixture_path())
        .expect("dynamic-block fixture should open");

    let error = document
        .set_entity_properties(SetEntityPropertiesRequest {
            handle: "298".to_owned(),
            properties: BTreeMap::from([("scale".to_owned(), json!([1.0, 2.0, 1.0]))]),
            projection: Projection::Summary,
            select: None,
        })
        .expect_err("uniform-only block must reject non-uniform scale");
    assert!(matches!(error, WorkerError::InvalidPropertyValue(_)));

    let error = document
        .set_entity_properties(SetEntityPropertiesRequest {
            handle: "298".to_owned(),
            properties: BTreeMap::from([("rotation".to_owned(), json!(1.25))]),
            projection: Projection::Summary,
            select: None,
        })
        .expect_err("attributed block reference must reject transforms");
    assert!(
        matches!(error, WorkerError::MutationFailed(message) if message.contains("attributes"))
    );
}

#[test]
fn house_plan_lists_model_layout_and_viewport_render_targets() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&fixture_path())
        .expect("fixture should open");
    let views = document
        .list_render_views()
        .expect("render views should load");

    assert!(views.iter().any(|view| view.id == "model"));
    assert!(views.iter().any(|view| view.id == "layout:2F37"));
    assert!(views.iter().any(|view| view.id == "viewport:2F56"));
    assert!(!views.iter().any(|view| view.id == "viewport:2F3E"));
}

#[test]
fn house_plan_renders_model_svg_and_layout_png() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&fixture_path())
        .expect("fixture should open");

    let svg = document
        .render_view(RenderRequest {
            target: RenderTarget::Model,
            region: None,
            width: 320,
            height: 240,
            format: RenderFormat::Svg,
            background: RenderBackground::Paper,
            padding: 0.02,
        })
        .expect("model SVG should render");
    let svg_bytes = base64::engine::general_purpose::STANDARD
        .decode(svg.data)
        .expect("SVG should be base64 encoded");
    assert!(svg_bytes.starts_with(b"<svg"));
    let svg_text = std::str::from_utf8(&svg_bytes).expect("SVG should be UTF-8");
    let bulged_polyline = svg_text
        .split("data-handle=\"2A75\"")
        .nth(1)
        .expect("fixture bulged polyline should render");
    assert!(
        bulged_polyline
            .split("</g>")
            .next()
            .expect("polyline group should close")
            .contains(" A ")
    );
    assert!(svg.diagnostics.rendered_entities > 1_000);

    let png = document
        .render_view(RenderRequest {
            target: RenderTarget::Layout {
                layout_handle: "2F37".to_owned(),
            },
            region: None,
            width: 320,
            height: 240,
            format: RenderFormat::Png,
            background: RenderBackground::Paper,
            padding: 0.02,
        })
        .expect("layout PNG should render");
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(png.data)
        .expect("PNG should be base64 encoded");
    assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(png.diagnostics.rendered_entities > 1_000);

    let region = Bounds {
        min: [350.0, 650.0],
        max: [525.0, 850.0],
    };
    let viewport_region = document
        .render_view(RenderRequest {
            target: RenderTarget::Viewport {
                viewport_handle: "2F56".to_owned(),
            },
            region: Some(region),
            width: 320,
            height: 240,
            format: RenderFormat::Svg,
            background: RenderBackground::Paper,
            padding: 0.02,
        })
        .expect("viewport region should render");
    assert_eq!(viewport_region.rendered_region, region);
}

#[test]
fn sample_dxf_opens_and_renders_model_svg() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&dxf_fixture_path())
        .expect("DXF fixture should open");
    let output = document
        .render_view(RenderRequest {
            target: RenderTarget::Model,
            region: None,
            width: 320,
            height: 240,
            format: RenderFormat::Svg,
            background: RenderBackground::Paper,
            padding: 0.02,
        })
        .expect("DXF model SVG should render");
    let svg = base64::engine::general_purpose::STANDARD
        .decode(output.data)
        .expect("SVG should be base64 encoded");
    assert!(svg.starts_with(b"<svg"));
    assert!(output.diagnostics.rendered_entities > 0);
}

#[test]
fn binary_dxf_group_code_layouts_open_and_query() {
    let _guard = lock_libredwg();

    for (name, version, group_code_width) in [
        ("r12", "AC1009", 1),
        ("r13", "AC1012", 2),
        ("r14", "AC1014", 2),
        ("r2000", "AC1015", 2),
    ] {
        let path = std::env::temp_dir().join(format!(
            "dwg-mcp-binary-dxf-{name}-{}.dxf",
            std::process::id()
        ));
        fs::write(&path, binary_dxf(version, group_code_width))
            .expect("binary DXF fixture should be written");
        let result = LibreDwgFactory.open(&path);
        fs::remove_file(&path).expect("binary DXF fixture should be removed");
        let document = result.unwrap_or_else(|error| panic!("{name} binary DXF: {error}"));
        let lines = document
            .query_objects(QueryObjectsRequest {
                type_name: Some("AcDbLine".to_owned()),
                generic_type: None,
                where_clauses: Vec::new(),
                scope: None,
                relations: Vec::new(),
                sort: Vec::new(),
                mode: QueryMode::Full,
                projection: Projection::Full,
                select: None,
                limit: 10,
                cursor: None,
            })
            .unwrap_or_else(|error| panic!("{name} binary DXF query: {error}"));
        assert_eq!(lines.total, 3, "{name} binary DXF line count");
    }
}

#[test]
fn table_fixture_reports_proxy_preview_table_data() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&table_fixture_path())
        .expect("fixture should open");

    let tables = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbTable".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: None,
            relations: Vec::new(),
            sort: Vec::new(),
            mode: QueryMode::Full,
            projection: Projection::Full,
            select: None,
            limit: 5,
            cursor: None,
        })
        .expect("table query should work");

    assert_eq!(tables.total, 1);
    let table = tables.items.first().expect("table should be returned");
    assert_eq!(table.handle, "64");
    assert_eq!(table.type_name, "AcDbTable");
    assert_eq!(table.properties.get("layer"), Some(&json!("10")));
    assert_eq!(table.properties.get("preview_exists"), Some(&json!(true)));
    assert_eq!(table.properties.get("preview_size"), Some(&json!(1532)));
    assert_eq!(
        table.properties.get("table_extraction_source"),
        Some(&json!("proxy_preview"))
    );
    assert_eq!(table.properties.get("num_rows"), Some(&json!(2)));
    assert_eq!(table.properties.get("num_cols"), Some(&json!(2)));
    assert_eq!(table.properties.get("row_heights"), Some(&json!([15, 15])));
    assert_eq!(table.properties.get("col_widths"), Some(&json!([40, 60])));
    assert_eq!(
        table.properties.get("cell_texts"),
        Some(&json!([["A1", "B1"], ["A2", "B2"]]))
    );
    assert_eq!(
        table.properties.get("cells"),
        Some(&json!([
            {"row": 0, "column": 0, "text": "A1", "position": [18.21428571428571, -8.75, 0]},
            {"row": 0, "column": 1, "text": "B1", "position": [67.85714285714285, -8.75, 0]},
            {"row": 1, "column": 0, "text": "A2", "position": [17.857142857142854, -23.75, 0]},
            {"row": 1, "column": 1, "text": "B2", "position": [67.5, -23.75, 0]}
        ]))
    );
}

#[test]
fn house_plan_reports_expected_entity_and_layer_counts() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&fixture_path())
        .expect("fixture should open");

    let entities = document
        .query_objects(QueryObjectsRequest {
            type_name: None,
            generic_type: None,
            where_clauses: vec![PropertyFilter {
                property: "kind".to_owned(),
                op: FilterOperator::Eq,
                value: Some(json!("entity")),
                values: Vec::new(),
            }],
            scope: None,
            relations: Vec::new(),
            sort: Vec::new(),
            mode: QueryMode::Count,
            projection: Projection::Summary,
            select: None,
            limit: 100,
            cursor: None,
        })
        .expect("entity count should work");
    assert_eq!(entities.total, 3891);

    let layers = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbLayerTableRecord".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: None,
            relations: Vec::new(),
            sort: Vec::new(),
            mode: QueryMode::Count,
            projection: Projection::Summary,
            select: None,
            limit: 100,
            cursor: None,
        })
        .expect("layer count should work");
    assert_eq!(layers.total, 60);

    let layer_rows = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbLayerTableRecord".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: None,
            relations: Vec::new(),
            sort: Vec::new(),
            mode: QueryMode::Summary,
            projection: Projection::Summary,
            select: None,
            limit: 5,
            cursor: None,
        })
        .expect("layer query should work");
    assert_eq!(layer_rows.total, 60);
    assert!(
        layer_rows
            .items
            .iter()
            .any(|item| item.properties.get("name") == Some(&json!("0")))
    );
    assert!(
        layer_rows
            .items
            .iter()
            .all(|item| item.properties.contains_key("ownerhandle"))
    );
}

#[test]
fn house_plan_lists_expected_types_from_the_file() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&fixture_path())
        .expect("fixture should open");

    let type_names = document
        .list_types()
        .into_iter()
        .map(|item| item.type_name)
        .collect::<Vec<_>>();

    assert_eq!(
        type_names,
        vec![
            "APPID_CONTROL",
            "AcDbArc",
            "AcDbAttributeDefinition",
            "AcDbBlockBegin",
            "AcDbBlockEnd",
            "AcDbBlockReference",
            "AcDbBlockTableRecord",
            "AcDbCircle",
            "AcDbDictionary",
            "AcDbDictionaryWithDefault",
            "AcDbDimStyleTable",
            "AcDbDimStyleTableRecord",
            "AcDbEllipse",
            "AcDbFace",
            "AcDbFaceRecord",
            "AcDbHatch",
            "AcDbLayerTableRecord",
            "AcDbLayout",
            "AcDbLine",
            "AcDbLinetypeTableRecord",
            "AcDbMText",
            "AcDbMaterial",
            "AcDbMlineStyle",
            "AcDbPlotSettings",
            "AcDbPoint",
            "AcDbPolyFaceMesh",
            "AcDbPolyFaceMeshVertex",
            "AcDbPolyline",
            "AcDbRadialDimension",
            "AcDbRegAppTableRecord",
            "AcDbRotatedDimension",
            "AcDbSortentsTable",
            "AcDbTableStyle",
            "AcDbText",
            "AcDbTextStyleTableRecord",
            "AcDbTrace",
            "AcDbViewTableRecord",
            "AcDbViewport",
            "AcDbViewportTableRecord",
            "AcDbVisualStyle",
            "AcDbXrecord",
            "BLOCK_CONTROL",
            "DictionaryVariables",
            "HEADER",
            "LAYER_CONTROL",
            "LTYPE_CONTROL",
            "PLACEHOLDER",
            "SEQEND",
            "STYLE_CONTROL",
            "UCS_CONTROL",
            "VIEW_CONTROL",
            "VPORT_CONTROL",
        ]
    );
}

#[test]
fn house_plan_exposes_header_settings_as_a_synthetic_record() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&fixture_path())
        .expect("fixture should open");

    let header = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("HEADER".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: None,
            relations: Vec::new(),
            sort: Vec::new(),
            mode: QueryMode::Full,
            projection: Projection::Full,
            select: None,
            limit: 5,
            cursor: None,
        })
        .expect("header query should work");

    assert_eq!(header.total, 1);
    let header = header
        .items
        .first()
        .expect("header record should be returned");
    assert_eq!(header.handle, "HEADER");
    assert_eq!(header.kind, "header");
    assert_eq!(header.type_name, "HEADER");
    assert!(
        header
            .properties
            .get("HANDSEED")
            .and_then(|value| value.as_str())
            .is_some()
    );
    assert!(
        header
            .properties
            .get("CLAYER")
            .and_then(|value| value.as_str())
            .is_some()
    );
    assert!(
        header
            .properties
            .get("MEASUREMENT")
            .and_then(|value| value.as_i64())
            .is_some()
    );
    assert!(
        header
            .properties
            .get("INSUNITS")
            .and_then(|value| value.as_i64())
            .is_some()
    );
}

#[test]
fn house_plan_query_objects_supports_scope_sort_and_range_filters() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&fixture_path())
        .expect("fixture should open");

    let inside_block = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbBlockReference".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: Some(QueryScope {
                block_handle: Some("10F".to_owned()),
                ..QueryScope::default()
            }),
            relations: Vec::new(),
            sort: vec![SortSpec {
                property: "handle".to_owned(),
                direction: SortDirection::Asc,
            }],
            mode: QueryMode::Summary,
            projection: Projection::Summary,
            select: None,
            limit: 10,
            cursor: None,
        })
        .expect("block scope should work");
    assert_eq!(inside_block.total, 3);
    assert_eq!(
        inside_block
            .items
            .iter()
            .map(|item| item.handle.as_str())
            .collect::<Vec<_>>(),
        vec!["130", "131", "138"]
    );

    let model_space_entities = document
        .query_objects(QueryObjectsRequest {
            type_name: None,
            generic_type: None,
            where_clauses: vec![PropertyFilter {
                property: "kind".to_owned(),
                op: FilterOperator::Eq,
                value: Some(json!("entity")),
                values: Vec::new(),
            }],
            scope: Some(QueryScope {
                space: Some(QuerySpace::ModelSpace),
                ..QueryScope::default()
            }),
            relations: Vec::new(),
            sort: Vec::new(),
            mode: QueryMode::Count,
            projection: Projection::Summary,
            select: None,
            limit: 100,
            cursor: None,
        })
        .expect("model space scope should work");
    assert_eq!(model_space_entities.total, 2177);

    let paper_space_entities = document
        .query_objects(QueryObjectsRequest {
            type_name: None,
            generic_type: None,
            where_clauses: vec![PropertyFilter {
                property: "kind".to_owned(),
                op: FilterOperator::Eq,
                value: Some(json!("entity")),
                values: Vec::new(),
            }],
            scope: Some(QueryScope {
                layout_handle: Some("2F37".to_owned()),
                ..QueryScope::default()
            }),
            relations: Vec::new(),
            sort: Vec::new(),
            mode: QueryMode::Count,
            projection: Projection::Summary,
            select: None,
            limit: 100,
            cursor: None,
        })
        .expect("layout scope should work");
    assert_eq!(paper_space_entities.total, 192);

    let rotated_inserts = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbBlockReference".to_owned()),
            generic_type: None,
            where_clauses: vec![PropertyFilter {
                property: "rotation".to_owned(),
                op: FilterOperator::Gt,
                value: Some(json!(4.0)),
                values: Vec::new(),
            }],
            scope: None,
            relations: Vec::new(),
            sort: vec![
                SortSpec {
                    property: "rotation".to_owned(),
                    direction: SortDirection::Desc,
                },
                SortSpec {
                    property: "handle".to_owned(),
                    direction: SortDirection::Asc,
                },
            ],
            mode: QueryMode::Summary,
            projection: Projection::Summary,
            select: None,
            limit: 10,
            cursor: None,
        })
        .expect("range filter should work");
    assert_eq!(rotated_inserts.total, 60);
    assert_eq!(rotated_inserts.items[0].handle, "2AD");
}

#[test]
fn house_plan_query_objects_supports_relation_filters() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&fixture_path())
        .expect("fixture should open");

    let inserts_of_named_block = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbBlockReference".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: None,
            relations: vec![RelationFilter {
                property: "block_header".to_owned(),
                direction: RelationDirection::Outgoing,
                target_type_name: Some("AcDbBlockTableRecord".to_owned()),
                target_generic_type: None,
                where_clauses: vec![PropertyFilter {
                    property: "name".to_owned(),
                    op: FilterOperator::Eq,
                    value: Some(json!("WDQ_JAMB")),
                    values: Vec::new(),
                }],
            }],
            sort: vec![
                SortSpec {
                    property: "rotation".to_owned(),
                    direction: SortDirection::Desc,
                },
                SortSpec {
                    property: "handle".to_owned(),
                    direction: SortDirection::Asc,
                },
            ],
            mode: QueryMode::Summary,
            projection: Projection::Summary,
            select: None,
            limit: 10,
            cursor: None,
        })
        .expect("outgoing relation filter should work");
    assert_eq!(inserts_of_named_block.total, 36);
    assert_eq!(inserts_of_named_block.items[0].handle, "2AD");

    let referenced_blocks = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbBlockTableRecord".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: None,
            relations: vec![RelationFilter {
                property: "block_header".to_owned(),
                direction: RelationDirection::Incoming,
                target_type_name: Some("AcDbBlockReference".to_owned()),
                target_generic_type: None,
                where_clauses: vec![PropertyFilter {
                    property: "rotation".to_owned(),
                    op: FilterOperator::Gt,
                    value: Some(json!(4.0)),
                    values: Vec::new(),
                }],
            }],
            sort: vec![SortSpec {
                property: "handle".to_owned(),
                direction: SortDirection::Asc,
            }],
            mode: QueryMode::Summary,
            projection: Projection::Summary,
            select: None,
            limit: 10,
            cursor: None,
        })
        .expect("incoming relation filter should work");
    assert_eq!(referenced_blocks.total, 7);
    assert_eq!(referenced_blocks.items[0].handle, "CA");
}

#[test]
fn house_plan_exposes_insertion_points_for_block_references_and_text() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&fixture_path())
        .expect("fixture should open");

    let block_reference = document
        .get_objects(GetObjectsRequest {
            handles: vec!["2AD".to_owned()],
            projection: Projection::Full,
            select: Some(vec!["ins_pt".to_owned()]),
        })
        .expect("block reference should load");
    assert!(block_reference.missing_handles.is_empty());
    let insert_point = block_reference.items[0]
        .properties
        .get("ins_pt")
        .and_then(|value| value.as_array())
        .expect("block reference ins_pt should be present");
    assert_eq!(insert_point.len(), 3);

    let first_text = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbText".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: None,
            relations: Vec::new(),
            sort: Vec::new(),
            mode: QueryMode::Handles,
            projection: Projection::Summary,
            select: None,
            limit: 1,
            cursor: None,
        })
        .expect("text query should work");
    assert!(!first_text.handles.is_empty());

    let text = document
        .get_objects(GetObjectsRequest {
            handles: vec![first_text.handles[0].clone()],
            projection: Projection::Full,
            select: Some(vec!["ins_pt".to_owned()]),
        })
        .expect("text should load");
    assert!(text.missing_handles.is_empty());
    let text_insert_point = text.items[0]
        .properties
        .get("ins_pt")
        .and_then(|value| value.as_array())
        .expect("text ins_pt should be present");
    assert_eq!(text_insert_point.len(), 2);
}

#[test]
fn house_plan_exposes_lwpolyline_points_arrays() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&fixture_path())
        .expect("fixture should open");

    let first_polyline = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbPolyline".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: None,
            relations: Vec::new(),
            sort: Vec::new(),
            mode: QueryMode::Handles,
            projection: Projection::Summary,
            select: None,
            limit: 1,
            cursor: None,
        })
        .expect("polyline query should work");
    assert!(!first_polyline.handles.is_empty());

    let polyline = document
        .get_objects(GetObjectsRequest {
            handles: vec![first_polyline.handles[0].clone()],
            projection: Projection::Full,
            select: None,
        })
        .expect("polyline should load in full mode");
    assert!(polyline.missing_handles.is_empty());
    assert!(!polyline.items[0].properties.contains_key("points"));

    let polyline = document
        .get_objects(GetObjectsRequest {
            handles: vec![first_polyline.handles[0].clone()],
            projection: Projection::Full,
            select: Some(vec!["num_points".to_owned(), "points".to_owned()]),
        })
        .expect("polyline should load");
    assert!(polyline.missing_handles.is_empty());

    let properties = &polyline.items[0].properties;
    let points = properties
        .get("points")
        .and_then(|value| value.as_array())
        .expect("polyline points should be present");
    let num_points = properties
        .get("num_points")
        .and_then(|value| value.as_i64())
        .expect("polyline num_points should be present");
    assert_eq!(points.len(), num_points as usize);
    assert!(points.iter().all(|point| {
        point
            .as_array()
            .map(|item| item.len() == 2)
            .unwrap_or(false)
    }));
}

#[test]
fn house_plan_full_hatches_include_contours_with_point_data() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&fixture_path())
        .expect("fixture should open");

    let hatch_handles = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbHatch".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: None,
            relations: Vec::new(),
            sort: Vec::new(),
            mode: QueryMode::Handles,
            projection: Projection::Summary,
            select: None,
            limit: 10,
            cursor: None,
        })
        .expect("hatch query should work");
    assert!(!hatch_handles.handles.is_empty());

    let hatches = document
        .get_objects(GetObjectsRequest {
            handles: hatch_handles.handles.clone(),
            projection: Projection::Full,
            select: None,
        })
        .expect("hatches should load in full mode");
    assert!(hatches.missing_handles.is_empty());

    let hatch = hatches
        .items
        .iter()
        .find(|item| {
            item.properties
                .get("contours")
                .and_then(|value| value.as_array())
                .is_some_and(|contours| !contours.is_empty())
        })
        .expect("expected at least one hatch with contours");

    let contours = hatch
        .properties
        .get("contours")
        .and_then(|value| value.as_array())
        .expect("contours should be present");
    assert!(!contours.is_empty());
    assert!(contains_2d_point(&hatch.properties["contours"]));

    let hatch_with_counts = document
        .get_objects(GetObjectsRequest {
            handles: vec![hatch.handle.clone()],
            projection: Projection::Full,
            select: Some(vec!["num_paths".to_owned(), "contours".to_owned()]),
        })
        .expect("hatch should load with explicit contour selection");
    assert!(hatch_with_counts.missing_handles.is_empty());
    let properties = &hatch_with_counts.items[0].properties;
    let num_paths = properties
        .get("num_paths")
        .and_then(|value| value.as_i64())
        .expect("num_paths should be present");
    let selected_contours = properties
        .get("contours")
        .and_then(|value| value.as_array())
        .expect("selected contours should be present");
    assert_eq!(selected_contours.len(), num_paths as usize);
}

#[test]
fn supported_types_and_properties_cover_3d_polylines_and_angular_dimensions() {
    let _guard = lock_libredwg();
    let supported = list_supported_types().expect("supported types should parse");
    let supported_names = supported
        .into_iter()
        .map(|item| item.type_name)
        .collect::<Vec<_>>();

    assert!(supported_names.contains(&"AcDb3dPolyline".to_owned()));
    assert!(supported_names.contains(&"AcDb3PointAngularDimension".to_owned()));
    assert!(supported_names.contains(&"HEADER".to_owned()));

    let polyline_3d =
        describe_supported_type("AcDb3dPolyline").expect("3D polyline type should exist");
    let polyline_properties = polyline_3d
        .properties
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();
    assert!(polyline_properties.contains(&"first_vertex".to_owned()));
    assert!(polyline_properties.contains(&"last_vertex".to_owned()));
    assert!(polyline_properties.contains(&"seqend".to_owned()));
    assert!(polyline_properties.contains(&"curve_type".to_owned()));

    let angular =
        describe_supported_type("AcDb3PointAngularDimension").expect("angular dimension type");
    let angular_properties = angular
        .properties
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();
    assert!(angular_properties.contains(&"xline1_pt".to_owned()));
    assert!(angular_properties.contains(&"xline2_pt".to_owned()));
    assert!(angular_properties.contains(&"center_pt".to_owned()));
    assert!(angular_properties.contains(&"user_text".to_owned()));
    assert!(angular_properties.contains(&"dimstyle".to_owned()));

    let dictionary =
        describe_supported_type("AcDbDictionary").expect("dictionary type should exist");
    let dictionary_properties = dictionary
        .properties
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();
    assert!(dictionary_properties.contains(&"items".to_owned()));
    assert!(dictionary_properties.contains(&"item_handles".to_owned()));

    let xrecord = describe_supported_type("AcDbXrecord").expect("xrecord type should exist");
    let xrecord_properties = xrecord
        .properties
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();
    assert!(xrecord_properties.contains(&"xdata".to_owned()));

    let hatch = describe_supported_type("AcDbHatch").expect("hatch type should exist");
    let hatch_properties = hatch
        .properties
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();
    assert!(hatch_properties.contains(&"contours".to_owned()));

    let header = describe_supported_type("HEADER").expect("header type should exist");
    let header_properties = header
        .properties
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();
    assert!(header_properties.contains(&"DWGCODEPAGE".to_owned()));
    assert!(header_properties.contains(&"HANDSEED".to_owned()));
    assert!(header_properties.contains(&"MEASUREMENT".to_owned()));
}

#[test]
fn dyn_blocks_exposes_dynamic_block_history_dictionaries_and_xrecords() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&dyn_blocks_fixture_path())
        .expect("fixture should open");

    let dynamic_block_reference = document
        .get_objects(GetObjectsRequest {
            handles: vec!["CBD".to_owned()],
            projection: Projection::Full,
            select: Some(vec![
                "xdicobjhandle".to_owned(),
                "block_header".to_owned(),
                "block_representation_dict_handle".to_owned(),
                "app_data_cache_handle".to_owned(),
                "enhanced_block_data_handle".to_owned(),
                "enhanced_block_data_xrecords".to_owned(),
            ]),
        })
        .expect("block reference should load");
    assert!(dynamic_block_reference.missing_handles.is_empty());
    assert_eq!(
        dynamic_block_reference.items[0]
            .properties
            .get("xdicobjhandle"),
        Some(&json!("CBE"))
    );
    assert_eq!(
        dynamic_block_reference.items[0]
            .properties
            .get("block_header"),
        Some(&json!("CD8"))
    );
    assert_eq!(
        dynamic_block_reference.items[0]
            .properties
            .get("block_representation_dict_handle"),
        Some(&json!("CF2"))
    );
    assert_eq!(
        dynamic_block_reference.items[0]
            .properties
            .get("app_data_cache_handle"),
        Some(&json!("CF4"))
    );
    assert_eq!(
        dynamic_block_reference.items[0]
            .properties
            .get("enhanced_block_data_handle"),
        Some(&json!("D13"))
    );
    assert_eq!(
        dynamic_block_reference.items[0]
            .properties
            .get("enhanced_block_data_xrecords"),
        Some(&json!(["D14", "D17", "D18", "D15", "D16"]))
    );

    let dictionaries = document
        .get_objects(GetObjectsRequest {
            handles: vec![
                "CBE".to_owned(),
                "CF2".to_owned(),
                "CF4".to_owned(),
                "D13".to_owned(),
            ],
            projection: Projection::Full,
            select: Some(vec![
                "items".to_owned(),
                "item_handles".to_owned(),
                "ownerhandle".to_owned(),
                "numitems".to_owned(),
            ]),
        })
        .expect("history dictionaries should load");
    assert!(dictionaries.missing_handles.is_empty());

    let by_handle = dictionaries
        .items
        .iter()
        .map(|item| (item.handle.as_str(), &item.properties))
        .collect::<std::collections::HashMap<_, _>>();

    assert_eq!(
        by_handle["D13"].get("items"),
        Some(&json!({"1": "D18", "8": "D15", "9": "D16"}))
    );
    assert_eq!(by_handle["CBE"].get("item_handles"), Some(&json!(["CF2"])));
    assert_eq!(
        by_handle["CF2"].get("item_handles"),
        Some(&json!(["CF3", "CF4"]))
    );
    assert_eq!(by_handle["CF4"].get("item_handles"), Some(&json!(["D13"])));
    assert_eq!(
        by_handle["D13"].get("item_handles"),
        Some(&json!(["D14", "D17", "D18", "D15", "D16"]))
    );

    let history_xrecords = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbXrecord".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: None,
            relations: vec![RelationFilter {
                property: "item_handles".to_owned(),
                direction: RelationDirection::Incoming,
                target_type_name: Some("AcDbDictionary".to_owned()),
                target_generic_type: None,
                where_clauses: vec![PropertyFilter {
                    property: "handle".to_owned(),
                    op: FilterOperator::Eq,
                    value: Some(json!("D13")),
                    values: Vec::new(),
                }],
            }],
            sort: vec![SortSpec {
                property: "handle".to_owned(),
                direction: SortDirection::Asc,
            }],
            mode: QueryMode::Full,
            projection: Projection::Full,
            select: Some(vec!["xdata".to_owned(), "ownerhandle".to_owned()]),
            limit: 10,
            cursor: None,
        })
        .expect("history xrecords should be reachable");
    assert_eq!(history_xrecords.total, 5);
    assert_eq!(
        history_xrecords
            .items
            .iter()
            .map(|item| item.handle.as_str())
            .collect::<Vec<_>>(),
        vec!["D14", "D15", "D16", "D17", "D18"]
    );
    assert_eq!(
        history_xrecords.items[0].properties.get("ownerhandle"),
        Some(&json!("D13"))
    );
    assert_eq!(
        history_xrecords.items[0].properties.get("xdata"),
        Some(&json!([
            [1071, 18597260],
            [1071, 25303744],
            [70, 25],
            [70, 104],
            [10, [-16.450129447944846, -0.09901143873563002, 0]],
            [10, [1982.9324090756895, -0.09901143873566041, 0]],
            [10, [0, 0, -1]]
        ]))
    );
    assert_eq!(
        history_xrecords.items[1].properties.get("xdata"),
        Some(&json!([
            [1071, 6895636],
            [1071, 9291323],
            [70, 25],
            [70, 104],
            [40, 0]
        ]))
    );
}

#[test]
fn dyn_blocks_exposes_evaluation_graph_nodes_and_edges() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&dyn_blocks_fixture_path())
        .expect("fixture should open");

    let eval_graphs = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbEvalGraph".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: None,
            relations: Vec::new(),
            sort: Vec::new(),
            mode: QueryMode::Handles,
            projection: Projection::Summary,
            select: None,
            limit: 10,
            cursor: None,
        })
        .expect("eval graphs should be queryable");
    assert!(!eval_graphs.handles.is_empty());

    let first_handle = eval_graphs.handles[0].clone();
    let eval_graph = document
        .get_objects(GetObjectsRequest {
            handles: vec![first_handle.clone()],
            projection: Projection::Full,
            select: Some(vec![
                "first_nodeid".to_owned(),
                "first_nodeid_copy".to_owned(),
                "num_nodes".to_owned(),
                "num_edges".to_owned(),
                "nodes".to_owned(),
                "edges".to_owned(),
                "ownerhandle".to_owned(),
            ]),
        })
        .expect("eval graph should load");
    assert!(eval_graph.missing_handles.is_empty());
    let props = &eval_graph.items[0].properties;

    assert!(props.get("first_nodeid").is_some());
    assert!(props.get("first_nodeid_copy").is_some());
    assert!(props.get("num_nodes").is_some());
    assert!(props.get("num_edges").is_some());

    let nodes = props
        .get("nodes")
        .and_then(|v| v.as_array())
        .expect("nodes array");
    assert!(!nodes.is_empty());
    let first_node = nodes[0].as_object().expect("node object");
    assert!(first_node.contains_key("id"));
    assert!(first_node.contains_key("edge_flags"));
    assert!(first_node.contains_key("nextid"));
    assert!(first_node.contains_key("evalexpr"));
    assert!(first_node.contains_key("node"));

    let edges = props
        .get("edges")
        .and_then(|v| v.as_array())
        .expect("edges array");
    assert!(!edges.is_empty());
    let first_edge = edges[0].as_object().expect("edge object");
    assert!(first_edge.contains_key("id"));
    assert!(first_edge.contains_key("nextid"));
    assert!(first_edge.contains_key("e1"));
    assert!(first_edge.contains_key("e2"));
    assert!(first_edge.contains_key("e3"));
    assert!(first_edge.contains_key("out_edge"));

    assert_eq!(nodes.len(), props["num_nodes"].as_i64().unwrap() as usize);
    assert_eq!(edges.len(), props["num_edges"].as_i64().unwrap() as usize);
}

#[test]
fn dyn_blocks_exposes_block_action_connections() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&dyn_blocks_fixture_path())
        .expect("fixture should open");

    let actions = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbBlockMoveAction".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: None,
            relations: Vec::new(),
            sort: Vec::new(),
            mode: QueryMode::Handles,
            projection: Projection::Summary,
            select: None,
            limit: 10,
            cursor: None,
        })
        .expect("block move actions should be queryable");
    assert!(!actions.handles.is_empty());

    let action = document
        .get_objects(GetObjectsRequest {
            handles: vec![actions.handles[0].clone()],
            projection: Projection::Full,
            select: Some(vec!["connections".to_owned()]),
        })
        .expect("block action should load");
    assert!(action.missing_handles.is_empty());

    let connections = action.items[0]
        .properties
        .get("connections")
        .and_then(|value| value.as_array())
        .expect("connections array");
    assert_eq!(connections.len(), 2);
    assert!(connections.iter().all(|connection| {
        connection.get("index").is_some()
            && connection.get("code").is_some()
            && connection
                .get("name")
                .and_then(|name| name.as_str())
                .is_some_and(|name| !name.is_empty())
    }));
}

#[test]
fn dyn_blocks_exposes_block_parameter_connections() {
    let _guard = lock_libredwg();
    let document = LibreDwgFactory
        .open(&dyn_blocks_fixture_path())
        .expect("fixture should open");

    let parameters = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbBlockLinearParameter".to_owned()),
            generic_type: None,
            where_clauses: Vec::new(),
            scope: None,
            relations: Vec::new(),
            sort: Vec::new(),
            mode: QueryMode::Handles,
            projection: Projection::Summary,
            select: None,
            limit: 10,
            cursor: None,
        })
        .expect("block linear parameters should be queryable");
    assert!(!parameters.handles.is_empty());

    let parameter = document
        .get_objects(GetObjectsRequest {
            handles: vec![parameters.handles[0].clone()],
            projection: Projection::Full,
            select: Some(vec!["connections".to_owned()]),
        })
        .expect("block parameter should load");
    assert!(parameter.missing_handles.is_empty());

    let connections = parameter.items[0]
        .properties
        .get("connections")
        .and_then(|value| value.as_array())
        .expect("connections array");
    assert!(connections.len() >= 4);
    assert!(connections.iter().all(|connection| {
        connection.get("property").is_some()
            && connection.get("index").is_some()
            && connection.get("code").is_some()
            && connection
                .get("name")
                .and_then(|name| name.as_str())
                .is_some_and(|name| !name.is_empty())
    }));
}

#[test]
fn worker_lists_types_with_regex_and_pagination() {
    let _guard = lock_libredwg();
    let mut server = StdioHandler::new(LibreDwgFactory);
    let input = [
        json!({
            "id": 1,
            "method": "openFile",
            "params": {"path": fixture_path()}
        }),
        json!({
            "id": 2,
            "method": "listTypes",
            "params": {
                "regex": "^AcDb3(PointAngularDimension|dPolyline)$",
                "limit": 1
            }
        }),
        json!({
            "id": 3,
            "method": "listTypes",
            "params": {
                "regex": "^AcDb3(PointAngularDimension|dPolyline)$",
                "limit": 1,
                "cursor": "1"
            }
        }),
        json!({
            "id": 4,
            "method": "listFileTypes",
            "params": {
                "regex": "^AcDbBlock",
                "limit": 2
            }
        }),
    ]
    .into_iter()
    .map(|value| value.to_string())
    .collect::<Vec<_>>()
    .join("\n");

    let mut output = Vec::new();
    server
        .serve(Cursor::new(format!("{input}\n")), &mut output)
        .expect("server should respond");

    let responses = String::from_utf8(output)
        .expect("utf8")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid json"))
        .collect::<Vec<_>>();

    assert_eq!(responses[0]["result"]["backend"], json!("libredwg-native"));

    assert_eq!(responses[1]["result"]["total"], json!(2));
    assert_eq!(responses[1]["result"]["nextCursor"], json!("1"));
    assert_eq!(
        responses[1]["result"]["items"][0]["typeName"],
        json!("AcDb3PointAngularDimension")
    );

    assert_eq!(responses[2]["result"]["total"], json!(2));
    assert_eq!(
        responses[2]["result"]["nextCursor"],
        serde_json::Value::Null
    );
    assert_eq!(
        responses[2]["result"]["items"][0]["typeName"],
        json!("AcDb3dPolyline")
    );

    assert_eq!(responses[3]["result"]["total"], json!(4));
    assert_eq!(responses[3]["result"]["nextCursor"], json!("2"));
    assert_eq!(
        responses[3]["result"]["items"]
            .as_array()
            .expect("items")
            .iter()
            .map(|item| item["typeName"].as_str().expect("type name"))
            .collect::<Vec<_>>(),
        vec!["AcDbBlockBegin", "AcDbBlockEnd"]
    );
}
