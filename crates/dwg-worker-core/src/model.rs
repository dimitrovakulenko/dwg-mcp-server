use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PropertyDefinition {
    pub name: String,
    pub value_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub queryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_target: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TypeDefinition {
    pub type_name: String,
    pub generic_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_select: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<PropertyDefinition>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Projection {
    #[default]
    Summary,
    Full,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum QueryMode {
    Count,
    Handles,
    #[default]
    Summary,
    Full,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum FilterOperator {
    Eq,
    In,
    Contains,
    Gt,
    Gte,
    Lt,
    Lte,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PropertyFilter {
    pub property: String,
    pub op: FilterOperator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Value>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum QuerySpace {
    ModelSpace,
    PaperSpace,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryScope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space: Option<QuerySpace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_handle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_handle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_handle: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SortDirection {
    #[default]
    Asc,
    Desc,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SortSpec {
    pub property: String,
    #[serde(default)]
    pub direction: SortDirection,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RelationDirection {
    #[default]
    Outgoing,
    Incoming,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelationFilter {
    pub property: String,
    #[serde(default)]
    pub direction: RelationDirection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_type_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_generic_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub where_clauses: Vec<PropertyFilter>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ObjectRecord {
    pub handle: String,
    pub kind: String,
    pub type_name: String,
    pub generic_type: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexedObject {
    pub handle: String,
    pub kind: String,
    pub type_name: String,
    pub generic_type: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub summary_properties: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub full_properties: BTreeMap<String, Value>,
    #[serde(default, skip_serializing)]
    pub container_block_handle: Option<String>,
    #[serde(default, skip_serializing)]
    pub layout_handle: Option<String>,
    #[serde(default, skip_serializing)]
    pub space: Option<QuerySpace>,
}

impl IndexedObject {
    pub fn value_for_property(&self, property: &str) -> Option<&Value> {
        match property {
            "handle" => None,
            "kind" => None,
            "typeName" | "type_name" => None,
            "genericType" | "generic_type" => None,
            _ => self
                .full_properties
                .get(property)
                .or_else(|| self.summary_properties.get(property)),
        }
    }

    pub fn project(
        &self,
        projection: Projection,
        select: Option<&[String]>,
        default_select: &[String],
    ) -> ObjectRecord {
        let mut properties = BTreeMap::new();

        if let Some(select) = select {
            for property in select {
                if let Some(value) = self
                    .full_properties
                    .get(property)
                    .or_else(|| self.summary_properties.get(property))
                {
                    properties.insert(property.clone(), value.clone());
                }
            }
        } else {
            match projection {
                Projection::Summary => {
                    for property in default_select {
                        if let Some(value) = self
                            .summary_properties
                            .get(property)
                            .or_else(|| self.full_properties.get(property))
                        {
                            if suppress_by_default(property, value) {
                                continue;
                            }
                            properties.insert(property.clone(), value.clone());
                        }
                    }
                }
                Projection::Full => {
                    for (property, value) in &self.full_properties {
                        if suppress_by_default(property, value) {
                            continue;
                        }
                        properties.insert(property.clone(), value.clone());
                    }
                }
            }
        }

        ObjectRecord {
            handle: self.handle.clone(),
            kind: self.kind.clone(),
            type_name: self.type_name.clone(),
            generic_type: self.generic_type.clone(),
            properties,
        }
    }
}

fn suppress_by_default(property: &str, value: &Value) -> bool {
    matches!(property, "points" | "vertices" | "vertex_handles")
        || is_coordinate_tuple_array(value)
}

fn is_coordinate_tuple_array(value: &Value) -> bool {
    let Value::Array(items) = value else {
        return false;
    };
    if items.is_empty() {
        return false;
    }

    items.iter().all(is_coordinate_tuple)
}

fn is_coordinate_tuple(value: &Value) -> bool {
    let Value::Array(tuple) = value else {
        return false;
    };

    (tuple.len() == 2 || tuple.len() == 3) && tuple.iter().all(Value::is_number)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GetObjectsRequest {
    pub handles: Vec<String>,
    #[serde(default)]
    pub projection: Projection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GetObjectsResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ObjectRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_handles: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueryObjectsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generic_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub where_clauses: Vec<PropertyFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<QueryScope>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<RelationFilter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<SortSpec>,
    #[serde(default)]
    pub mode: QueryMode,
    #[serde(default)]
    pub projection: Projection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<Vec<String>>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

fn default_limit() -> usize {
    100
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueryObjectsResult {
    pub total: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ObjectRecord>,
}
