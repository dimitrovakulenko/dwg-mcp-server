use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use dwg_worker_core::{
    BackendFactory, DwgDocument, GetObjectsRequest, IndexedDocument, IndexedObject, Projection,
    PropertyDefinition, PropertyFilter, QueryMode, QueryObjectsRequest, QueryScope, QuerySpace,
    RelationDirection, RelationFilter, SortDirection, SortSpec, TypeDefinition, WorkerError,
    StdioHandler,
};
use serde_json::{Value, json};

#[derive(Clone)]
struct StaticFactory {
    documents: HashMap<PathBuf, IndexedDocument>,
}

impl BackendFactory for StaticFactory {
    type Document = IndexedDocument;

    fn backend_name(&self) -> &'static str {
        "static-test"
    }

    fn open(&self, path: &Path) -> Result<Self::Document, WorkerError> {
        self.documents.get(path).cloned().ok_or_else(|| {
            WorkerError::OpenFailed(format!("fixture not found: {}", path.display()))
        })
    }

    fn list_supported_types(&self) -> Result<Vec<TypeDefinition>, WorkerError> {
        Ok(sample_document().list_types())
    }

    fn describe_supported_type(&self, type_name: &str) -> Result<TypeDefinition, WorkerError> {
        sample_document().describe_type(type_name)
    }
}

fn sample_document() -> IndexedDocument {
    let types = vec![
        TypeDefinition {
            type_name: "AcDbBlockReference".to_owned(),
            generic_type: "block_reference".to_owned(),
            description: Some("Block insert".to_owned()),
            aliases: Vec::new(),
            default_select: vec![
                "name".to_owned(),
                "block_header".to_owned(),
                "layer".to_owned(),
            ],
            properties: vec![
                PropertyDefinition {
                    name: "name".to_owned(),
                    value_kind: "string".to_owned(),
                    description: Some("Block name".to_owned()),
                    queryable: true,
                    reference_target: None,
                },
                PropertyDefinition {
                    name: "block_header".to_owned(),
                    value_kind: "handle".to_owned(),
                    description: Some("Referenced block definition".to_owned()),
                    queryable: true,
                    reference_target: Some("BLOCK_HEADER".to_owned()),
                },
                PropertyDefinition {
                    name: "layer".to_owned(),
                    value_kind: "handle".to_owned(),
                    description: Some("Owning layer".to_owned()),
                    queryable: true,
                    reference_target: Some("LAYER".to_owned()),
                },
            ],
        },
        TypeDefinition {
            type_name: "BLOCK_HEADER".to_owned(),
            generic_type: "block_definition".to_owned(),
            description: Some("Block table record".to_owned()),
            aliases: vec!["AcDbBlockTableRecord".to_owned()],
            default_select: vec!["name".to_owned()],
            properties: vec![PropertyDefinition {
                name: "name".to_owned(),
                value_kind: "string".to_owned(),
                description: Some("Block definition name".to_owned()),
                queryable: true,
                reference_target: None,
            }],
        },
    ];

    let block_header = IndexedObject {
        handle: "728".to_owned(),
        kind: "object".to_owned(),
        type_name: "BLOCK_HEADER".to_owned(),
        generic_type: "block_definition".to_owned(),
        summary_properties: BTreeMap::from([(String::from("name"), json!("M-WC"))]),
        full_properties: BTreeMap::from([
            (String::from("name"), json!("M-WC")),
            (String::from("description"), json!("Toilet block")),
        ]),
        container_block_handle: None,
        layout_handle: None,
        space: None,
    };

    let insert_1 = IndexedObject {
        handle: "15001".to_owned(),
        kind: "entity".to_owned(),
        type_name: "AcDbBlockReference".to_owned(),
        generic_type: "block_reference".to_owned(),
        summary_properties: BTreeMap::from([
            (String::from("name"), json!("M-WC")),
            (String::from("block_header"), json!("728")),
            (String::from("layer"), json!("84")),
        ]),
        full_properties: BTreeMap::from([
            (String::from("name"), json!("M-WC")),
            (String::from("block_header"), json!("728")),
            (String::from("layer"), json!("84")),
            (String::from("rotation"), json!(0.0)),
        ]),
        container_block_handle: Some("MODEL".to_owned()),
        layout_handle: Some("LAYOUT1".to_owned()),
        space: Some(QuerySpace::ModelSpace),
    };

    let insert_2 = IndexedObject {
        handle: "15002".to_owned(),
        kind: "entity".to_owned(),
        type_name: "AcDbBlockReference".to_owned(),
        generic_type: "block_reference".to_owned(),
        summary_properties: BTreeMap::from([
            (String::from("name"), json!("M-WC")),
            (String::from("block_header"), json!("728")),
            (String::from("layer"), json!("88")),
        ]),
        full_properties: BTreeMap::from([
            (String::from("name"), json!("M-WC")),
            (String::from("block_header"), json!("728")),
            (String::from("layer"), json!("88")),
            (String::from("rotation"), json!(1.57)),
        ]),
        container_block_handle: Some("BLOCK_A".to_owned()),
        layout_handle: None,
        space: None,
    };

    IndexedDocument::new(types, vec![block_header, insert_1, insert_2])
}

#[test]
fn get_objects_preserves_order_and_reports_missing_handles() {
    let document = sample_document();
    let result = document
        .get_objects(GetObjectsRequest {
            handles: vec!["15002".to_owned(), "missing".to_owned(), "728".to_owned()],
            projection: Projection::Summary,
            select: None,
        })
        .expect("get_objects should work");

    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].handle, "15002");
    assert_eq!(result.items[1].handle, "728");
    assert_eq!(result.missing_handles, vec!["missing"]);
}

#[test]
fn query_objects_supports_filtering_count_and_pagination() {
    let document = sample_document();
    let query = QueryObjectsRequest {
        type_name: Some("AcDbBlockReference".to_owned()),
        generic_type: None,
        where_clauses: vec![PropertyFilter {
            property: "block_header".to_owned(),
            op: dwg_worker_core::FilterOperator::Eq,
            value: Some(json!("728")),
            values: Vec::new(),
        }],
        scope: None,
        relations: Vec::new(),
        sort: Vec::new(),
        mode: QueryMode::Handles,
        projection: Projection::Summary,
        select: None,
        limit: 1,
        cursor: None,
    };

    let first_page = document.query_objects(query.clone()).expect("first page");
    assert_eq!(first_page.total, 2);
    assert_eq!(first_page.handles, vec!["15001"]);
    assert_eq!(first_page.next_cursor.as_deref(), Some("1"));

    let second_page = document
        .query_objects(QueryObjectsRequest {
            cursor: Some("1".to_owned()),
            ..query
        })
        .expect("second page");
    assert_eq!(second_page.handles, vec!["15002"]);
    assert_eq!(second_page.next_cursor, None);
}

#[test]
fn query_objects_supports_scope_relations_range_filters_and_sorting() {
    let document = sample_document();
    let result = document
        .query_objects(QueryObjectsRequest {
            type_name: Some("AcDbBlockReference".to_owned()),
            generic_type: None,
            where_clauses: vec![PropertyFilter {
                property: "rotation".to_owned(),
                op: dwg_worker_core::FilterOperator::Gt,
                value: Some(json!(1.0)),
                values: Vec::new(),
            }],
            scope: Some(QueryScope {
                block_handle: Some("BLOCK_A".to_owned()),
                ..QueryScope::default()
            }),
            relations: vec![RelationFilter {
                property: "block_header".to_owned(),
                direction: RelationDirection::Outgoing,
                target_type_name: Some("BLOCK_HEADER".to_owned()),
                target_generic_type: None,
                where_clauses: vec![PropertyFilter {
                    property: "name".to_owned(),
                    op: dwg_worker_core::FilterOperator::Eq,
                    value: Some(json!("M-WC")),
                    values: Vec::new(),
                }],
            }],
            sort: vec![SortSpec {
                property: "rotation".to_owned(),
                direction: SortDirection::Desc,
            }],
            mode: QueryMode::Summary,
            projection: Projection::Summary,
            select: None,
            limit: 10,
            cursor: None,
        })
        .expect("query should work");

    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].handle, "15002");
}

#[test]
fn list_types_supports_regex_and_pagination() {
    let factory = StaticFactory {
        documents: HashMap::new(),
    };
    let mut handler = StdioHandler::new(factory);

    let first_page = serde_json::to_value(handler.handle_request(
        serde_json::from_value(json!({
            "id": 1,
            "method": "listTypes",
            "params": {
                "regex": "(?i)block",
                "limit": 1
            }
        }))
        .expect("request"),
    ))
    .expect("json");
    assert_eq!(first_page["result"]["total"], json!(2));
    assert_eq!(first_page["result"]["nextCursor"], json!("1"));
    assert_eq!(
        first_page["result"]["items"][0]["typeName"],
        json!("AcDbBlockReference")
    );

    let second_page = serde_json::to_value(handler.handle_request(
        serde_json::from_value(json!({
            "id": 2,
            "method": "listTypes",
            "params": {
                "regex": "(?i)block",
                "limit": 1,
                "cursor": "1"
            }
        }))
        .expect("request"),
    ))
    .expect("json");
    assert_eq!(second_page["result"]["total"], json!(2));
    assert_eq!(second_page["result"]["nextCursor"], Value::Null);
    assert_eq!(second_page["result"]["items"][0]["typeName"], json!("BLOCK_HEADER"));
}

#[test]
fn server_round_trips_json_rpc_requests() {
    let path = PathBuf::from("/fixtures/amsterdam.dwg");
    let mut documents = HashMap::new();
    documents.insert(path.clone(), sample_document());

    let factory = StaticFactory { documents };
    let mut handler = StdioHandler::new(factory);
    let input = [
        json!({"id": 1, "method": "openFile", "params": {"path": path}}),
        json!({"id": 2, "method": "listTypes", "params": {"regex": "^AcDbBlock", "limit": 1}}),
        json!({
            "id": 3,
            "method": "listFileTypes",
            "params": {"regex": "definition"}
        }),
        json!({
            "id": 4,
            "method": "queryObjects",
            "params": {
                "genericType": "block_reference",
                "mode": "summary",
                "whereClauses": [
                    {"property": "layer", "op": "eq", "value": "84"}
                ]
            }
        }),
    ]
    .into_iter()
    .map(|value| value.to_string())
    .collect::<Vec<_>>()
    .join("\n");

    let mut output = Vec::new();
    handler
        .serve(Cursor::new(format!("{input}\n")), &mut output)
        .expect("handler should respond");

    let responses = String::from_utf8(output)
        .expect("utf8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("valid json"))
        .collect::<Vec<_>>();

    assert_eq!(responses.len(), 4);
    assert_eq!(responses[0]["result"]["backend"], json!("static-test"));
    assert_eq!(responses[1]["result"]["total"], json!(2));
    assert_eq!(responses[1]["result"]["nextCursor"], json!("1"));
    assert_eq!(
        responses[1]["result"]["items"][0]["typeName"],
        json!("AcDbBlockReference")
    );
    assert_eq!(responses[2]["result"]["total"], json!(1));
    assert_eq!(
        responses[2]["result"]["items"][0]["typeName"],
        json!("BLOCK_HEADER")
    );
    assert_eq!(responses[3]["result"]["items"][0]["handle"], json!("15001"));
    assert_eq!(
        responses[3]["result"]["items"][0]["properties"]["layer"],
        json!("84")
    );
}
