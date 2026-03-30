use std::path::PathBuf;
use std::io::Cursor;
use std::sync::{Mutex, OnceLock};

use dwg_libredwg::{LibreDwgFactory, describe_supported_type, list_supported_types};
use dwg_worker_core::{
    BackendFactory, DwgDocument, FilterOperator, GetObjectsRequest, PropertyFilter, Projection,
    QueryMode, QueryObjectsRequest, QueryScope, QuerySpace, RelationDirection, RelationFilter,
    SortDirection, SortSpec, StdioHandler,
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
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testData/house_plan.dwg")
}

fn dyn_blocks_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testData/dyn-blocks.dwg")
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
    assert!(
        points
            .iter()
            .all(|point| point.as_array().map(|item| item.len() == 2).unwrap_or(false))
    );
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
    assert_eq!(
        by_handle["CBE"].get("item_handles"),
        Some(&json!(["CF2"]))
    );
    assert_eq!(
        by_handle["CF2"].get("item_handles"),
        Some(&json!(["CF3", "CF4"]))
    );
    assert_eq!(
        by_handle["CF4"].get("item_handles"),
        Some(&json!(["D13"]))
    );
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

    let nodes = props.get("nodes").and_then(|v| v.as_array()).expect("nodes array");
    assert!(!nodes.is_empty());
    let first_node = nodes[0].as_object().expect("node object");
    assert!(first_node.contains_key("id"));
    assert!(first_node.contains_key("edge_flags"));
    assert!(first_node.contains_key("nextid"));
    assert!(first_node.contains_key("evalexpr"));
    assert!(first_node.contains_key("node"));

    let edges = props.get("edges").and_then(|v| v.as_array()).expect("edges array");
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
    assert_eq!(responses[2]["result"]["nextCursor"], serde_json::Value::Null);
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
