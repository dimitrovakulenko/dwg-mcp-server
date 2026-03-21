use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;
use thiserror::Error;

use crate::model::{
    FilterOperator, GetObjectsRequest, GetObjectsResult, IndexedObject, Projection, PropertyFilter,
    QueryMode, QueryObjectsRequest, QueryObjectsResult, QueryScope, QuerySpace, RelationDirection,
    RelationFilter, SortDirection, SortSpec, TypeDefinition,
};

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("document is not open")]
    DocumentNotOpen,
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("invalid cursor: {0}")]
    InvalidCursor(String),
    #[error("unknown type: {0}")]
    UnknownType(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),
    #[error("open file failed: {0}")]
    OpenFailed(String),
}

pub trait DwgDocument {
    fn list_types(&self) -> Vec<TypeDefinition>;
    fn describe_type(&self, type_name: &str) -> Result<TypeDefinition, WorkerError>;
    fn get_objects(&self, request: GetObjectsRequest) -> Result<GetObjectsResult, WorkerError>;
    fn query_objects(
        &self,
        request: QueryObjectsRequest,
    ) -> Result<QueryObjectsResult, WorkerError>;
}

pub trait BackendFactory {
    type Document: DwgDocument;

    fn backend_name(&self) -> &'static str;
    fn open(&self, path: &Path) -> Result<Self::Document, WorkerError>;
    fn list_supported_types(&self) -> Result<Vec<TypeDefinition>, WorkerError>;
    fn describe_supported_type(&self, type_name: &str) -> Result<TypeDefinition, WorkerError>;
}

#[derive(Clone, Debug)]
struct PreparedRelationFilter {
    property: String,
    direction: RelationDirection,
    target_indices: Vec<usize>,
    target_handles: HashSet<String>,
}

#[derive(Clone, Debug)]
pub struct IndexedDocument {
    types_by_name: HashMap<String, TypeDefinition>,
    types_by_generic: HashMap<String, String>,
    types_by_alias: HashMap<String, String>,
    objects: Vec<IndexedObject>,
    indices_by_handle: HashMap<String, usize>,
    ordered_indices: Vec<usize>,
    indices_by_type: HashMap<String, Vec<usize>>,
    indices_by_generic: HashMap<String, Vec<usize>>,
    indices_by_kind: HashMap<String, Vec<usize>>,
    exact_value_index: HashMap<String, HashMap<String, Vec<usize>>>,
    indices_by_block: HashMap<String, Vec<usize>>,
    indices_by_layout: HashMap<String, Vec<usize>>,
    indices_by_space: HashMap<QuerySpace, Vec<usize>>,
}

impl IndexedDocument {
    pub fn new(types: Vec<TypeDefinition>, mut objects: Vec<IndexedObject>) -> Self {
        let mut types_by_name = HashMap::new();
        let mut types_by_generic = HashMap::new();
        let mut types_by_alias = HashMap::new();

        for ty in types {
            types_by_generic.insert(ty.generic_type.clone(), ty.type_name.clone());
            for alias in &ty.aliases {
                types_by_alias.insert(alias.clone(), ty.type_name.clone());
            }
            types_by_name.insert(ty.type_name.clone(), ty);
        }

        let mut block_layouts = HashMap::new();
        let mut model_space_block_handle = None::<String>;
        let mut paper_space_block_handle = None::<String>;

        for object in &objects {
            if object.type_name == "AcDbBlockTableRecord" {
                if let Some(layout) = object.value_for_property("layout").and_then(Value::as_str) {
                    if layout != "0" {
                        block_layouts.insert(object.handle.clone(), layout.to_owned());
                    }
                }
            }

            if object.type_name == "BLOCK_CONTROL" {
                model_space_block_handle = object
                    .value_for_property("model_space")
                    .and_then(Value::as_str)
                    .filter(|handle| *handle != "0")
                    .map(str::to_owned);
                paper_space_block_handle = object
                    .value_for_property("paper_space")
                    .and_then(Value::as_str)
                    .filter(|handle| *handle != "0")
                    .map(str::to_owned);
            }
        }

        for object in &mut objects {
            if let Some(block_handle) = object.container_block_handle.as_deref() {
                object.layout_handle = block_layouts.get(block_handle).cloned();
                object.space = if model_space_block_handle
                    .as_deref()
                    .is_some_and(|handle| handle == block_handle)
                {
                    Some(QuerySpace::ModelSpace)
                } else if paper_space_block_handle
                    .as_deref()
                    .is_some_and(|handle| handle == block_handle)
                {
                    Some(QuerySpace::PaperSpace)
                } else {
                    None
                };
            }
        }

        let mut indices_by_handle = HashMap::new();
        let mut ordered_indices = Vec::with_capacity(objects.len());
        let mut indices_by_type = HashMap::new();
        let mut indices_by_generic = HashMap::new();
        let mut indices_by_kind = HashMap::new();
        let mut exact_value_index: HashMap<String, HashMap<String, Vec<usize>>> = HashMap::new();
        let mut indices_by_block = HashMap::new();
        let mut indices_by_layout = HashMap::new();
        let mut indices_by_space = HashMap::new();

        for (index, object) in objects.iter().enumerate() {
            ordered_indices.push(index);
            indices_by_handle.insert(object.handle.clone(), index);
            indices_by_type
                .entry(object.type_name.clone())
                .or_insert_with(Vec::new)
                .push(index);
            indices_by_generic
                .entry(object.generic_type.clone())
                .or_insert_with(Vec::new)
                .push(index);
            indices_by_kind
                .entry(object.kind.clone())
                .or_insert_with(Vec::new)
                .push(index);

            if let Some(block_handle) = object.container_block_handle.as_deref() {
                indices_by_block
                    .entry(block_handle.to_owned())
                    .or_insert_with(Vec::new)
                    .push(index);
            }

            if let Some(layout_handle) = object.layout_handle.as_deref() {
                indices_by_layout
                    .entry(layout_handle.to_owned())
                    .or_insert_with(Vec::new)
                    .push(index);
            }

            if let Some(space) = object.space {
                indices_by_space
                    .entry(space)
                    .or_insert_with(Vec::new)
                    .push(index);
            }

            for (property_name, value) in &object.full_properties {
                if let Some(key) = Self::canonical_value_key(value) {
                    exact_value_index
                        .entry(property_name.clone())
                        .or_default()
                        .entry(key)
                        .or_default()
                        .push(index);
                }
            }
        }

        Self {
            types_by_name,
            types_by_generic,
            types_by_alias,
            objects,
            indices_by_handle,
            ordered_indices,
            indices_by_type,
            indices_by_generic,
            indices_by_kind,
            exact_value_index,
            indices_by_block,
            indices_by_layout,
            indices_by_space,
        }
    }

    fn resolve_type_name<'a>(
        &'a self,
        type_name: Option<&'a str>,
        generic_type: Option<&'a str>,
    ) -> Result<Option<&'a str>, WorkerError> {
        if let Some(type_name) = type_name {
            if self.types_by_name.contains_key(type_name) {
                return Ok(Some(type_name));
            }

            if let Some(mapped) = self.types_by_alias.get(type_name) {
                return Ok(Some(mapped.as_str()));
            }

            if let Some(mapped) = self.types_by_generic.get(type_name) {
                return Ok(Some(mapped.as_str()));
            }

            return Err(WorkerError::UnknownType(type_name.to_owned()));
        }

        if let Some(generic_type) = generic_type {
            if let Some(mapped) = self.types_by_generic.get(generic_type) {
                return Ok(Some(mapped.as_str()));
            }

            return Err(WorkerError::UnknownType(generic_type.to_owned()));
        }

        Ok(None)
    }

    fn default_select_for(&self, type_name: &str) -> &[String] {
        self.types_by_name
            .get(type_name)
            .map(|ty| ty.default_select.as_slice())
            .unwrap_or(&[])
    }

    fn parse_cursor(cursor: Option<&str>) -> Result<usize, WorkerError> {
        let Some(cursor) = cursor else {
            return Ok(0);
        };

        cursor
            .parse::<usize>()
            .map_err(|_| WorkerError::InvalidCursor(cursor.to_owned()))
    }

    fn object(&self, index: usize) -> &IndexedObject {
        &self.objects[index]
    }

    fn property_value<'a>(&'a self, object: &'a IndexedObject, property: &str) -> Option<&'a Value> {
        match property {
            "handle" | "kind" | "typeName" | "type_name" | "genericType" | "generic_type" => {
                None
            }
            _ => object.value_for_property(property),
        }
    }

    fn property_string<'a>(&'a self, object: &'a IndexedObject, property: &str) -> Option<&'a str> {
        self.property_value(object, property).and_then(Value::as_str)
    }

    fn canonical_value_key(value: &Value) -> Option<String> {
        serde_json::to_string(value).ok()
    }

    fn indexed_candidates_for_filter(&self, filter: &PropertyFilter) -> Option<Vec<usize>> {
        match filter.op {
            FilterOperator::Eq => filter
                .value
                .as_ref()
                .and_then(|value| self.indices_for_property_value(filter.property.as_str(), value)),
            FilterOperator::In => {
                let mut seen = HashSet::new();
                let mut indices = Vec::new();
                for value in &filter.values {
                    if let Some(items) =
                        self.indices_for_property_value(filter.property.as_str(), value)
                    {
                        for index in items {
                            if seen.insert(index) {
                                indices.push(index);
                            }
                        }
                    }
                }
                Some(indices)
            }
            FilterOperator::Contains
            | FilterOperator::Gt
            | FilterOperator::Gte
            | FilterOperator::Lt
            | FilterOperator::Lte => None,
        }
    }

    fn indices_for_property_value(&self, property: &str, value: &Value) -> Option<Vec<usize>> {
        match property {
            "handle" => value
                .as_str()
                .and_then(|handle| self.indices_by_handle.get(handle).copied())
                .map(|index| vec![index]),
            "kind" => value
                .as_str()
                .and_then(|kind| self.indices_by_kind.get(kind).cloned()),
            "typeName" | "type_name" => value
                .as_str()
                .and_then(|type_name| self.resolve_type_name(Some(type_name), None).ok().flatten())
                .and_then(|type_name| self.indices_by_type.get(type_name).cloned()),
            "genericType" | "generic_type" => value
                .as_str()
                .and_then(|generic_type| {
                    self.resolve_type_name(None, Some(generic_type))
                        .ok()
                        .flatten()
                })
                .and_then(|type_name| self.indices_by_type.get(type_name).cloned()),
            _ => {
                let key = Self::canonical_value_key(value)?;
                self.exact_value_index
                    .get(property)
                    .and_then(|values| values.get(&key).cloned())
            }
        }
    }

    fn intersect_preserving_order(&self, indexed_sets: Vec<Vec<usize>>) -> Vec<usize> {
        if indexed_sets.is_empty() {
            return self.ordered_indices.clone();
        }

        let sets = indexed_sets
            .into_iter()
            .map(|indices| indices.into_iter().collect::<HashSet<_>>())
            .collect::<Vec<_>>();

        self.ordered_indices
            .iter()
            .copied()
            .filter(|index| sets.iter().all(|set| set.contains(index)))
            .collect()
    }

    fn prepare_relation_filter(
        &self,
        relation: &RelationFilter,
    ) -> Result<PreparedRelationFilter, WorkerError> {
        let target_indices = self.candidate_indices_for_query(
            relation.target_type_name.as_deref(),
            relation.target_generic_type.as_deref(),
            &relation.where_clauses,
            None,
            &[],
        )?;
        let target_handles = target_indices
            .iter()
            .map(|index| self.object(*index).handle.clone())
            .collect::<HashSet<_>>();

        Ok(PreparedRelationFilter {
            property: relation.property.clone(),
            direction: relation.direction,
            target_indices,
            target_handles,
        })
    }

    fn indexed_candidates_for_prepared_relation(
        &self,
        relation: &PreparedRelationFilter,
    ) -> Option<Vec<usize>> {
        match relation.direction {
            RelationDirection::Outgoing => {
                let mut seen = HashSet::new();
                let mut indices = Vec::new();
                for handle in &relation.target_handles {
                    if let Some(items) =
                        self.indices_for_property_value(&relation.property, &Value::String(handle.clone()))
                    {
                        for index in items {
                            if seen.insert(index) {
                                indices.push(index);
                            }
                        }
                    }
                }
                Some(indices)
            }
            RelationDirection::Incoming => {
                let mut seen = HashSet::new();
                let mut indices = Vec::new();
                for target_index in &relation.target_indices {
                    if let Some(value) =
                        self.property_value(self.object(*target_index), &relation.property)
                    {
                        self.collect_referenced_object_indices(value, &mut seen, &mut indices);
                    }
                }
                Some(indices)
            }
        }
    }

    fn collect_referenced_object_indices(
        &self,
        value: &Value,
        seen: &mut HashSet<usize>,
        indices: &mut Vec<usize>,
    ) {
        match value {
            Value::String(handle) => {
                if let Some(index) = self.indices_by_handle.get(handle).copied() {
                    if seen.insert(index) {
                        indices.push(index);
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.collect_referenced_object_indices(item, seen, indices);
                }
            }
            _ => {}
        }
    }

    fn candidate_indices_for_query(
        &self,
        type_name: Option<&str>,
        generic_type: Option<&str>,
        filters: &[PropertyFilter],
        scope: Option<&QueryScope>,
        relations: &[PreparedRelationFilter],
    ) -> Result<Vec<usize>, WorkerError> {
        let resolved_type = self.resolve_type_name(type_name, generic_type)?;
        let mut indexed_sets = Vec::new();

        if let Some(type_name) = resolved_type {
            indexed_sets.push(self.indices_by_type.get(type_name).cloned().unwrap_or_default());
        } else if let Some(generic_type) = generic_type {
            indexed_sets.push(
                self.indices_by_generic
                    .get(generic_type)
                    .cloned()
                    .unwrap_or_default(),
            );
        }

        if let Some(scope) = scope {
            if let Some(space) = scope.space {
                indexed_sets.push(
                    self.indices_by_space
                        .get(&space)
                        .cloned()
                        .unwrap_or_default(),
                );
            }
            if let Some(layout_handle) = scope.layout_handle.as_deref() {
                indexed_sets.push(
                    self.indices_by_layout
                        .get(layout_handle)
                        .cloned()
                        .unwrap_or_default(),
                );
            }
            if let Some(block_handle) = scope.block_handle.as_deref() {
                indexed_sets.push(
                    self.indices_by_block
                        .get(block_handle)
                        .cloned()
                        .unwrap_or_default(),
                );
            }
            if let Some(owner_handle) = scope.owner_handle.as_deref() {
                indexed_sets.push(
                    self.indices_for_property_value("ownerhandle", &Value::String(owner_handle.to_owned()))
                        .unwrap_or_default(),
                );
            }
        }

        for filter in filters {
            if let Some(indices) = self.indexed_candidates_for_filter(filter) {
                indexed_sets.push(indices);
            }
        }

        for relation in relations {
            if let Some(indices) = self.indexed_candidates_for_prepared_relation(relation) {
                indexed_sets.push(indices);
            }
        }

        let resolved_type_name = resolved_type;
        Ok(self
            .intersect_preserving_order(indexed_sets)
            .into_iter()
            .filter(|index| {
                let object = self.object(*index);
                resolved_type_name.is_none_or(|type_name| object.type_name == type_name)
                    && generic_type.is_none_or(|item| object.generic_type == item)
                    && scope.is_none_or(|scope| self.matches_scope(object, scope))
                    && filters.iter().all(|filter| self.matches_filter(object, filter))
                    && relations
                        .iter()
                        .all(|relation| self.matches_prepared_relation(object, relation))
            })
            .collect())
    }

    fn matches_scope(&self, object: &IndexedObject, scope: &QueryScope) -> bool {
        scope.space.is_none_or(|space| object.space == Some(space))
            && scope
                .layout_handle
                .as_deref()
                .is_none_or(|layout_handle| object.layout_handle.as_deref() == Some(layout_handle))
            && scope
                .block_handle
                .as_deref()
                .is_none_or(|block_handle| {
                    object.container_block_handle.as_deref() == Some(block_handle)
                })
            && scope
                .owner_handle
                .as_deref()
                .is_none_or(|owner_handle| {
                    self.property_string(object, "ownerhandle") == Some(owner_handle)
                })
    }

    fn matches_filter(&self, object: &IndexedObject, filter: &PropertyFilter) -> bool {
        match filter.property.as_str() {
            "handle" => self.match_value(
                "handle",
                &Value::String(object.handle.clone()),
                filter,
            ),
            "kind" => self.match_value("kind", &Value::String(object.kind.clone()), filter),
            "typeName" | "type_name" => self.match_value(
                "typeName",
                &Value::String(object.type_name.clone()),
                filter,
            ),
            "genericType" | "generic_type" => self.match_value(
                "genericType",
                &Value::String(object.generic_type.clone()),
                filter,
            ),
            property => self
                .property_value(object, property)
                .is_some_and(|value| self.match_value(property, value, filter)),
        }
    }

    fn match_value(&self, property: &str, value: &Value, filter: &PropertyFilter) -> bool {
        match filter.op {
            FilterOperator::Eq => filter
                .value
                .as_ref()
                .is_some_and(|expected| self.value_matches(property, value, expected)),
            FilterOperator::In => filter
                .values
                .iter()
                .any(|candidate| self.value_matches(property, value, candidate)),
            FilterOperator::Contains => {
                let Some(needle) = filter.value.as_ref().and_then(Value::as_str) else {
                    return false;
                };

                if let Some(haystack) = value.as_str() {
                    haystack
                        .to_ascii_lowercase()
                        .contains(&needle.to_ascii_lowercase())
                } else {
                    false
                }
            }
            FilterOperator::Gt => filter
                .value
                .as_ref()
                .and_then(|expected| self.compare_values(property, value, expected))
                .is_some_and(|ordering| ordering == Ordering::Greater),
            FilterOperator::Gte => filter
                .value
                .as_ref()
                .and_then(|expected| self.compare_values(property, value, expected))
                .is_some_and(|ordering| ordering == Ordering::Greater || ordering == Ordering::Equal),
            FilterOperator::Lt => filter
                .value
                .as_ref()
                .and_then(|expected| self.compare_values(property, value, expected))
                .is_some_and(|ordering| ordering == Ordering::Less),
            FilterOperator::Lte => filter
                .value
                .as_ref()
                .and_then(|expected| self.compare_values(property, value, expected))
                .is_some_and(|ordering| ordering == Ordering::Less || ordering == Ordering::Equal),
        }
    }

    fn value_matches(&self, property: &str, left: &Value, right: &Value) -> bool {
        if left == right {
            return true;
        }

        if let Some(items) = left.as_array() {
            return items
                .iter()
                .any(|item| self.value_matches(property, item, right));
        }

        self.compare_values(property, left, right)
            .is_some_and(|ordering| ordering == Ordering::Equal)
    }

    fn compare_values(&self, property: &str, left: &Value, right: &Value) -> Option<Ordering> {
        match (left, right) {
            (Value::Number(left), Value::Number(right)) => {
                left.as_f64()?.partial_cmp(&right.as_f64()?)
            }
            (Value::String(left), Value::String(right)) => {
                Some(self.compare_strings(property, left, right))
            }
            (Value::Bool(left), Value::Bool(right)) => Some(left.cmp(right)),
            _ => None,
        }
    }

    fn compare_strings(&self, _property: &str, left: &str, right: &str) -> Ordering {
        match (Self::parse_handle_value(left), Self::parse_handle_value(right)) {
            (Some(left), Some(right)) => left.cmp(&right),
            _ => left.cmp(right),
        }
    }

    fn parse_handle_value(value: &str) -> Option<u64> {
        if value.is_empty() || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return None;
        }

        u64::from_str_radix(value, 16).ok()
    }

    fn matches_prepared_relation(
        &self,
        object: &IndexedObject,
        relation: &PreparedRelationFilter,
    ) -> bool {
        match relation.direction {
            RelationDirection::Outgoing => self
                .property_value(object, &relation.property)
                .is_some_and(|value| self.value_matches_target_handles(value, &relation.target_handles)),
            RelationDirection::Incoming => relation.target_indices.iter().any(|index| {
                self.property_value(self.object(*index), &relation.property)
                    .is_some_and(|value| self.value_contains_handle(value, &object.handle))
            }),
        }
    }

    fn value_matches_target_handles(
        &self,
        value: &Value,
        target_handles: &HashSet<String>,
    ) -> bool {
        match value {
            Value::String(handle) => target_handles.contains(handle),
            Value::Array(items) => items
                .iter()
                .any(|item| self.value_matches_target_handles(item, target_handles)),
            _ => false,
        }
    }

    fn value_contains_handle(&self, value: &Value, handle: &str) -> bool {
        match value {
            Value::String(current) => current == handle,
            Value::Array(items) => items.iter().any(|item| self.value_contains_handle(item, handle)),
            _ => false,
        }
    }

    fn sort_indices(&self, indices: &mut [usize], sort: &[SortSpec]) {
        if sort.is_empty() {
            return;
        }

        indices.sort_by(|left, right| self.compare_objects(*left, *right, sort));
    }

    fn compare_objects(&self, left: usize, right: usize, sort: &[SortSpec]) -> Ordering {
        for spec in sort {
            let ordering = self.compare_property_for_sort(left, right, &spec.property);
            if ordering != Ordering::Equal {
                return match spec.direction {
                    SortDirection::Asc => ordering,
                    SortDirection::Desc => ordering.reverse(),
                };
            }
        }

        left.cmp(&right)
    }

    fn compare_property_for_sort(
        &self,
        left_index: usize,
        right_index: usize,
        property: &str,
    ) -> Ordering {
        let left_object = self.object(left_index);
        let right_object = self.object(right_index);

        let special = match property {
            "handle" => Some(self.compare_strings(property, &left_object.handle, &right_object.handle)),
            "kind" => Some(left_object.kind.cmp(&right_object.kind)),
            "typeName" | "type_name" => Some(left_object.type_name.cmp(&right_object.type_name)),
            "genericType" | "generic_type" => {
                Some(left_object.generic_type.cmp(&right_object.generic_type))
            }
            _ => None,
        };
        if let Some(ordering) = special {
            return ordering;
        }

        let left = self.sort_value(left_object, property);
        let right = self.sort_value(right_object, property);

        match (left, right) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(left), Some(right)) => self
                .compare_values(property, left, right)
                .unwrap_or(Ordering::Equal),
        }
    }

    fn sort_value<'a>(&'a self, object: &'a IndexedObject, property: &str) -> Option<&'a Value> {
        self.property_value(object, property)
    }
}

impl DwgDocument for IndexedDocument {
    fn list_types(&self) -> Vec<TypeDefinition> {
        let mut types: Vec<_> = self.types_by_name.values().cloned().collect();
        types.sort_by(|left, right| left.type_name.cmp(&right.type_name));
        types
    }

    fn describe_type(&self, type_name: &str) -> Result<TypeDefinition, WorkerError> {
        let resolved = self.resolve_type_name(Some(type_name), None)?;
        let Some(resolved) = resolved else {
            return Err(WorkerError::UnknownType(type_name.to_owned()));
        };

        self.types_by_name
            .get(resolved)
            .cloned()
            .ok_or_else(|| WorkerError::UnknownType(type_name.to_owned()))
    }

    fn get_objects(&self, request: GetObjectsRequest) -> Result<GetObjectsResult, WorkerError> {
        let mut items = Vec::new();
        let mut missing_handles = Vec::new();
        let select = request.select.as_deref();

        for handle in request.handles {
            if let Some(index) = self.indices_by_handle.get(&handle).copied() {
                let object = self.object(index);
                let default_select = self.default_select_for(&object.type_name);
                items.push(object.project(request.projection, select, default_select));
            } else {
                missing_handles.push(handle);
            }
        }

        Ok(GetObjectsResult {
            items,
            missing_handles,
        })
    }

    fn query_objects(
        &self,
        request: QueryObjectsRequest,
    ) -> Result<QueryObjectsResult, WorkerError> {
        let start = Self::parse_cursor(request.cursor.as_deref())?;
        let limit = request.limit.max(1);
        let select = request.select.as_deref();
        let prepared_relations = request
            .relations
            .iter()
            .map(|relation| self.prepare_relation_filter(relation))
            .collect::<Result<Vec<_>, _>>()?;

        let mut matches = self.candidate_indices_for_query(
            request.type_name.as_deref(),
            request.generic_type.as_deref(),
            &request.where_clauses,
            request.scope.as_ref(),
            &prepared_relations,
        )?;
        self.sort_indices(&mut matches, &request.sort);

        let total = matches.len();
        let end = total.min(start.saturating_add(limit));
        let page = if start >= total {
            &matches[0..0]
        } else {
            &matches[start..end]
        };
        let next_cursor = match request.mode {
            QueryMode::Count => None,
            _ => (end < total).then(|| end.to_string()),
        };

        let mut result = QueryObjectsResult {
            total,
            next_cursor,
            handles: Vec::new(),
            items: Vec::new(),
        };

        match request.mode {
            QueryMode::Count => {}
            QueryMode::Handles => {
                result.handles = page
                    .iter()
                    .map(|index| self.object(*index).handle.clone())
                    .collect();
            }
            QueryMode::Summary | QueryMode::Full => {
                let projection = match request.mode {
                    QueryMode::Full => Projection::Full,
                    _ => request.projection,
                };
                result.items = page
                    .iter()
                    .map(|index| {
                        let object = self.object(*index);
                        let default_select = self.default_select_for(&object.type_name);
                        object.project(projection, select, default_select)
                    })
                    .collect();
            }
        }

        Ok(result)
    }
}
