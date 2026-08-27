mod schema;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::Path;
use std::sync::OnceLock;

use dwg_render_core::{RenderDocument, RenderOutput, RenderRequest, RenderView, SourceEntity};
use dwg_worker_core::{
    BackendFactory, DwgDocument, GetObjectsRequest, GetObjectsResult, IndexedDocument,
    IndexedObject, QueryObjectsRequest, QueryObjectsResult, SetEntityPropertiesRequest,
    SetEntityPropertiesResult, TypeDefinition, WorkerError,
};
use serde_json::{Value, json};

use schema::SchemaCatalog;
pub use schema::{describe_supported_type, list_supported_types};

const DWG_ERR_CRITICAL_STATUS: i32 = 128;
const HEADER_OBJECT_HANDLE: &str = "HEADER";
const HEADER_TYPE_NAME: &str = "HEADER";

pub struct LibreDwgFactory;

impl BackendFactory for LibreDwgFactory {
    type Document = LibreDwgDocument;

    fn backend_name(&self) -> &'static str {
        "libredwg-native"
    }

    fn open(&self, path: &Path) -> Result<Self::Document, WorkerError> {
        LibreDwgDocument::open(path)
    }

    fn list_supported_types(&self) -> Result<Vec<TypeDefinition>, WorkerError> {
        list_supported_types()
    }

    fn describe_supported_type(&self, type_name: &str) -> Result<TypeDefinition, WorkerError> {
        describe_supported_type(type_name)
    }
}

pub struct LibreDwgDocument {
    indexed: IndexedDocument,
    render: OnceLock<RenderDocument>,
    #[cfg(feature = "native")]
    _native: NativeDocument,
}

impl LibreDwgDocument {
    fn open(path: &Path) -> Result<Self, WorkerError> {
        #[cfg(feature = "native")]
        {
            let native = NativeDocument::open(path)?;
            let indexed = build_indexed_document(&native)?;
            Ok(Self {
                indexed,
                render: OnceLock::new(),
                _native: native,
            })
        }

        #[cfg(not(feature = "native"))]
        {
            let _ = path;
            Err(WorkerError::BackendUnavailable(
                "dwg-libredwg was built without native LibreDWG support".to_owned(),
            ))
        }
    }
}

impl DwgDocument for LibreDwgDocument {
    fn list_types(&self) -> Vec<TypeDefinition> {
        self.indexed.list_types()
    }

    fn describe_type(&self, type_name: &str) -> Result<TypeDefinition, WorkerError> {
        self.indexed.describe_type(type_name)
    }

    fn get_objects(&self, request: GetObjectsRequest) -> Result<GetObjectsResult, WorkerError> {
        self.indexed.get_objects(request)
    }

    fn query_objects(
        &self,
        request: QueryObjectsRequest,
    ) -> Result<QueryObjectsResult, WorkerError> {
        self.indexed.query_objects(request)
    }

    fn set_entity_properties(
        &mut self,
        request: SetEntityPropertiesRequest,
    ) -> Result<SetEntityPropertiesResult, WorkerError> {
        #[cfg(feature = "native")]
        {
            self.set_native_entity_properties(request)
        }

        #[cfg(not(feature = "native"))]
        {
            let _ = request;
            Err(WorkerError::BackendUnavailable(
                "dwg-libredwg was built without native LibreDWG support".to_owned(),
            ))
        }
    }

    fn list_render_views(&self) -> Result<Vec<RenderView>, WorkerError> {
        self.render_document()
            .list_views()
            .map_err(worker_render_error)
    }

    fn render_view(&self, request: RenderRequest) -> Result<RenderOutput, WorkerError> {
        self.render_document()
            .render(request)
            .map_err(worker_render_error)
    }
}

fn worker_render_error(error: dwg_render_core::RenderError) -> WorkerError {
    match error {
        dwg_render_core::RenderError::ResourceLimit(message) => WorkerError::ResourceLimit(message),
        error => WorkerError::InvalidRequest(error.to_string()),
    }
}

impl LibreDwgDocument {
    fn render_document(&self) -> &RenderDocument {
        self.render.get_or_init(|| {
            RenderDocument::new(
                self.indexed
                    .objects()
                    .iter()
                    .map(|object| SourceEntity {
                        handle: object.handle.clone(),
                        type_name: object.type_name.clone(),
                        kind: object.kind.clone(),
                        properties: object.full_properties.clone(),
                        container_block_handle: object.container_block_handle.clone(),
                        layout_handle: object.layout_handle.clone(),
                    })
                    .collect(),
            )
        })
    }

    #[cfg(feature = "native")]
    fn set_native_entity_properties(
        &mut self,
        request: SetEntityPropertiesRequest,
    ) -> Result<SetEntityPropertiesResult, WorkerError> {
        let SetEntityPropertiesRequest {
            handle,
            properties,
            projection,
            select,
        } = request;
        if properties.is_empty() {
            return Err(WorkerError::InvalidRequest(
                "properties must not be empty".to_owned(),
            ));
        }

        let current = self
            .indexed
            .get_objects(GetObjectsRequest {
                handles: vec![handle.clone()],
                projection: dwg_worker_core::Projection::Full,
                select: None,
            })?
            .items
            .into_iter()
            .next()
            .ok_or_else(|| WorkerError::EntityNotFound(handle.clone()))?;
        let type_definition = self.indexed.describe_type(&current.type_name)?;
        let mut ins_pt = None;
        let mut rotation = None;
        let mut scale = None;

        for (name, value) in &properties {
            let definition = type_definition
                .properties
                .iter()
                .find(|definition| definition.name == *name)
                .ok_or_else(|| {
                    WorkerError::PropertyNotFound(format!("{} on {}", name, current.type_name))
                })?;
            if !definition.writable {
                return Err(WorkerError::PropertyNotWritable(format!(
                    "{} on {}",
                    name, current.type_name
                )));
            }

            match name.as_str() {
                "ins_pt" => ins_pt = Some(point3_value(name, value, false)?),
                "rotation" => rotation = Some(number_value(name, value)?),
                "scale" => scale = Some(point3_value(name, value, true)?),
                _ => {
                    return Err(WorkerError::PropertyNotWritable(format!(
                        "{} on {}",
                        name, current.type_name
                    )));
                }
            }
        }

        let handle_value = u64::from_str_radix(&handle, 16)
            .map_err(|_| WorkerError::InvalidRequest(format!("invalid handle: {handle}")))?;
        let ins = ins_pt.unwrap_or([0.0; 3]);
        let scale_value = scale.unwrap_or([0.0; 3]);
        let status = unsafe {
            libredwg_sys::bridge_dwg_insert_set_transform(
                self._native.raw,
                handle_value,
                ins_pt.is_some(),
                ins[0],
                ins[1],
                ins[2],
                rotation.is_some(),
                rotation.unwrap_or(0.0),
                scale.is_some(),
                scale_value[0],
                scale_value[1],
                scale_value[2],
            )
        };
        match status {
            libredwg_sys::BridgeDwgInsertSetTransformStatus_BRIDGE_DWG_INSERT_SET_TRANSFORM_OK => {}
            libredwg_sys::BridgeDwgInsertSetTransformStatus_BRIDGE_DWG_INSERT_SET_TRANSFORM_HAS_ATTRIBUTES => {
                return Err(WorkerError::MutationFailed(format!(
                    "cannot update {handle}: block references with attributes are not supported"
                )));
            }
            libredwg_sys::BridgeDwgInsertSetTransformStatus_BRIDGE_DWG_INSERT_SET_TRANSFORM_NON_UNIFORM_SCALE => {
                return Err(WorkerError::InvalidPropertyValue(
                    "scale must be uniform for this block definition".to_owned(),
                ));
            }
            libredwg_sys::BridgeDwgInsertSetTransformStatus_BRIDGE_DWG_INSERT_SET_TRANSFORM_MISSING_BLOCK => {
                return Err(WorkerError::MutationFailed(format!(
                    "cannot update {handle}: block definition is unavailable"
                )));
            }
            _ => {
                return Err(WorkerError::MutationFailed(format!(
                    "failed to update {handle}"
                )));
            }
        }

        self.indexed = build_indexed_document(&self._native)?;
        self.render.take();
        let item = self
            .indexed
            .get_objects(GetObjectsRequest {
                handles: vec![handle.clone()],
                projection,
                select,
            })?
            .items
            .into_iter()
            .next()
            .ok_or_else(|| WorkerError::EntityNotFound(handle))?;

        Ok(SetEntityPropertiesResult { item, dirty: true })
    }
}

fn number_value(name: &str, value: &Value) -> Result<f64, WorkerError> {
    value
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| WorkerError::InvalidPropertyValue(format!("{name} must be a finite number")))
}

fn point3_value(name: &str, value: &Value, reject_zero: bool) -> Result<[f64; 3], WorkerError> {
    let values = value
        .as_array()
        .filter(|values| values.len() == 3)
        .ok_or_else(|| {
            WorkerError::InvalidPropertyValue(format!(
                "{name} must be an array of three finite numbers"
            ))
        })?;
    let point = [
        number_value(name, &values[0])?,
        number_value(name, &values[1])?,
        number_value(name, &values[2])?,
    ];
    if reject_zero && point.contains(&0.0) {
        return Err(WorkerError::InvalidPropertyValue(
            "scale components must be non-zero".to_owned(),
        ));
    }
    Ok(point)
}

#[cfg(feature = "native")]
struct NativeDocument {
    raw: *mut libredwg_sys::Dwg_Data,
}

#[cfg(feature = "native")]
impl NativeDocument {
    fn open(path: &Path) -> Result<Self, WorkerError> {
        let c_path = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| WorkerError::OpenFailed("path contains NUL byte".to_owned()))?;

        let raw = unsafe { libredwg_sys::bridge_dwg_data_new() };
        if raw.is_null() {
            return Err(WorkerError::OpenFailed(
                "failed to allocate native DWG document".to_owned(),
            ));
        }

        let status = unsafe { libredwg_sys::bridge_dwg_data_read_file(raw, c_path.as_ptr()) };

        if status >= DWG_ERR_CRITICAL_STATUS {
            unsafe {
                libredwg_sys::bridge_dwg_data_free(raw);
            }
            return Err(WorkerError::OpenFailed(format!(
                "libredwg returned error code {status} while opening {}",
                path.display()
            )));
        }

        Ok(Self { raw })
    }
}

#[cfg(feature = "native")]
impl Drop for NativeDocument {
    fn drop(&mut self) {
        unsafe {
            libredwg_sys::bridge_dwg_data_free(self.raw);
        }
    }
}

#[cfg(feature = "native")]
fn build_indexed_document(native: &NativeDocument) -> Result<IndexedDocument, WorkerError> {
    let schema = SchemaCatalog::load()?;
    let object_count = unsafe { libredwg_sys::bridge_dwg_data_num_objects(native.raw) } as usize;
    let mut indexed_objects = Vec::with_capacity(object_count + 1);
    let mut type_properties: HashMap<String, BTreeSet<String>> = HashMap::new();

    for index in 0..object_count {
        let Some(indexed) = (unsafe { parse_native_object(native.raw, index, schema)? }) else {
            continue;
        };
        indexed_objects.push(indexed);
    }

    augment_dynamic_block_history_properties(&mut indexed_objects);
    augment_polyline_vertex_properties(&mut indexed_objects);

    if let Some(indexed) = unsafe { parse_header_variables(native.raw, schema)? } {
        indexed_objects.push(indexed);
    }

    for indexed in &indexed_objects {
        type_properties
            .entry(indexed.type_name.clone())
            .or_default()
            .extend(indexed.full_properties.keys().cloned());
    }

    let mut types = type_properties
        .into_iter()
        .map(|(type_name, properties)| {
            let observed = properties.into_iter().collect::<Vec<_>>();
            schema.type_definition_for_observed(&type_name, &observed)
        })
        .collect::<Vec<_>>();
    types.sort_by(|left, right| left.type_name.cmp(&right.type_name));

    Ok(IndexedDocument::new(types, indexed_objects))
}

fn augment_dynamic_block_history_properties(objects: &mut [IndexedObject]) {
    let indices_by_handle = objects
        .iter()
        .enumerate()
        .map(|(index, object)| (object.handle.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut updates = Vec::<(usize, Vec<(String, Value)>)>::new();

    for (index, object) in objects.iter().enumerate() {
        if object.type_name != "AcDbBlockReference" {
            continue;
        }

        let Some(extension_dictionary_handle) = property_string(object, "xdicobjhandle") else {
            continue;
        };
        let Some(extension_dictionary) =
            object_by_handle(objects, &indices_by_handle, extension_dictionary_handle)
        else {
            continue;
        };
        let extension_children = property_handle_array(extension_dictionary, "item_handles");
        let Some(block_representation_dict_handle) = extension_children.first().cloned() else {
            continue;
        };
        let Some(block_representation_dict) = object_by_handle(
            objects,
            &indices_by_handle,
            &block_representation_dict_handle,
        ) else {
            continue;
        };

        let mut derived = vec![(
            "block_representation_dict_handle".to_owned(),
            Value::String(block_representation_dict_handle.clone()),
        )];

        for child_handle in property_handle_array(block_representation_dict, "item_handles") {
            let Some(child) = object_by_handle(objects, &indices_by_handle, &child_handle) else {
                continue;
            };

            match child.type_name.as_str() {
                "AcDbBlockRepresentationData" => {
                    derived.push((
                        "block_representation_data_handle".to_owned(),
                        Value::String(child_handle),
                    ));
                }
                "AcDbDictionary" => {
                    derived.push((
                        "app_data_cache_handle".to_owned(),
                        Value::String(child_handle.clone()),
                    ));

                    if let Some(app_data_cache) =
                        object_by_handle(objects, &indices_by_handle, &child_handle)
                    {
                        let enhanced_handles =
                            property_handle_array(app_data_cache, "item_handles");
                        if let Some(enhanced_handle) = enhanced_handles.first() {
                            derived.push((
                                "enhanced_block_data_handle".to_owned(),
                                Value::String(enhanced_handle.clone()),
                            ));

                            if let Some(enhanced_block_data) =
                                object_by_handle(objects, &indices_by_handle, enhanced_handle)
                            {
                                let xrecord_handles =
                                    property_handle_array(enhanced_block_data, "item_handles");
                                if !xrecord_handles.is_empty() {
                                    derived.push((
                                        "enhanced_block_data_xrecords".to_owned(),
                                        Value::Array(
                                            xrecord_handles
                                                .into_iter()
                                                .map(Value::String)
                                                .collect(),
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if !derived.is_empty() {
            updates.push((index, derived));
        }
    }

    for (index, derived) in updates {
        if let Some(object) = objects.get_mut(index) {
            for (name, value) in derived {
                object.full_properties.insert(name, value);
            }
            object.summary_properties = select_summary_properties(&object.full_properties);
        }
    }
}

#[cfg(feature = "native")]
fn augment_polyline_vertex_properties(objects: &mut [IndexedObject]) {
    let indices_by_handle = objects
        .iter()
        .enumerate()
        .map(|(index, object)| (object.handle.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut updates = Vec::<(usize, Vec<(String, Value)>)>::new();

    for (index, object) in objects.iter().enumerate() {
        if !matches!(
            object.type_name.as_str(),
            "AcDb2dPolyline" | "AcDb3dPolyline"
        ) {
            continue;
        }

        let Some(first_vertex_handle) = property_string(object, "first_vertex") else {
            continue;
        };
        let last_vertex_handle = property_string(object, "last_vertex");

        let mut current_vertex_handle = Some(first_vertex_handle.to_owned());
        let mut visited = HashSet::new();
        let mut vertex_handles = Vec::new();
        let mut vertices = Vec::new();

        while let Some(handle) = current_vertex_handle.take() {
            if !visited.insert(handle.clone()) {
                break;
            }

            let Some(vertex) = object_by_handle(objects, &indices_by_handle, &handle) else {
                break;
            };

            if vertex.type_name == "SEQEND" {
                break;
            }

            if property_string(vertex, "ownerhandle") != Some(object.handle.as_str()) {
                break;
            }

            if let Some(point) = vertex.value_for_property("point") {
                vertices.push(point.clone());
            }
            vertex_handles.push(handle.clone());

            if Some(handle.as_str()) == last_vertex_handle {
                break;
            }

            current_vertex_handle = property_string(vertex, "next_entity").and_then(|next| {
                if next.is_empty() || next == "0" {
                    None
                } else {
                    Some(next.to_owned())
                }
            });
        }

        if !vertices.is_empty() {
            updates.push((
                index,
                vec![
                    ("vertices".to_owned(), Value::Array(vertices)),
                    (
                        "vertex_handles".to_owned(),
                        Value::Array(vertex_handles.into_iter().map(Value::String).collect()),
                    ),
                ],
            ));
        }
    }

    for (index, derived) in updates {
        if let Some(object) = objects.get_mut(index) {
            for (name, value) in derived {
                object.full_properties.insert(name, value);
            }
            object.summary_properties = select_summary_properties(&object.full_properties);
        }
    }
}

#[cfg(feature = "native")]
unsafe fn parse_native_object(
    dwg: *mut libredwg_sys::Dwg_Data,
    index: usize,
    _schema: &SchemaCatalog,
) -> Result<Option<IndexedObject>, WorkerError> {
    let object = unsafe { libredwg_sys::bridge_dwg_object_at(dwg, index as _) };
    if object.is_null() {
        return Ok(None);
    }

    let raw_type_name = c_string_to_owned(unsafe { libredwg_sys::bridge_dwg_object_name(object) })
        .ok_or_else(|| WorkerError::OpenFailed(format!("object at index {index} has no type")))?;
    let kind = if unsafe { libredwg_sys::bridge_dwg_object_is_entity(object) } {
        "entity"
    } else {
        "object"
    };
    let supported = describe_supported_type(&raw_type_name).ok();
    let type_name = supported
        .as_ref()
        .map(|item| item.type_name.clone())
        .unwrap_or_else(|| raw_type_name.clone());
    let generic_type = supported
        .as_ref()
        .map(|item| item.generic_type.clone())
        .unwrap_or_else(|| schema::to_generic_name(&type_name));
    let handle = format!("{:X}", unsafe {
        libredwg_sys::bridge_dwg_object_handle_value(object)
    });
    let container_block_handle = if kind == "entity" {
        let owner_handle = unsafe { libredwg_sys::bridge_dwg_entity_owner_handle(object) };
        (owner_handle != 0).then(|| format!("{owner_handle:X}"))
    } else {
        None
    };

    let mut full_properties = unsafe { read_properties_for_object(object, supported.as_ref()) };
    full_properties.extend(unsafe { read_special_properties(object, &raw_type_name) });
    if type_name == "AcDbWipeout" {
        if let Some(Value::Object(value)) = unsafe {
            read_json_property(
                object,
                libredwg_sys::bridge_dwg_object_wipeout_geometry_json,
            )
        } {
            full_properties.extend(value);
        }
    }
    let summary_properties = select_summary_properties(&full_properties);

    Ok(Some(IndexedObject {
        handle,
        kind: kind.to_owned(),
        type_name,
        generic_type,
        summary_properties,
        full_properties,
        container_block_handle,
        layout_handle: None,
        space: None,
    }))
}

#[cfg(feature = "native")]
unsafe fn parse_header_variables(
    dwg: *mut libredwg_sys::Dwg_Data,
    schema: &SchemaCatalog,
) -> Result<Option<IndexedObject>, WorkerError> {
    let Some(supported) = schema.describe_type(HEADER_TYPE_NAME) else {
        return Err(WorkerError::BackendUnavailable(
            "HEADER type definition is missing from the schema catalog".to_owned(),
        ));
    };

    let full_properties = unsafe { read_header_properties(dwg, &supported) };
    if full_properties.is_empty() {
        return Ok(None);
    }

    let summary_properties = select_summary_properties(&full_properties);
    Ok(Some(IndexedObject {
        handle: HEADER_OBJECT_HANDLE.to_owned(),
        kind: "header".to_owned(),
        type_name: HEADER_TYPE_NAME.to_owned(),
        generic_type: supported.generic_type,
        summary_properties,
        full_properties,
        container_block_handle: None,
        layout_handle: None,
        space: None,
    }))
}

#[cfg(feature = "native")]
unsafe fn read_header_properties(
    dwg: *mut libredwg_sys::Dwg_Data,
    supported: &TypeDefinition,
) -> BTreeMap<String, Value> {
    let mut properties = BTreeMap::new();

    for property in &supported.properties {
        if !property.queryable {
            continue;
        }

        if let Some(value) = unsafe { read_header_field_value(dwg, &property.name) } {
            properties.insert(property.name.clone(), value);
        }
    }

    properties
}

#[cfg(feature = "native")]
unsafe fn read_properties_for_object(
    object: *const libredwg_sys::Dwg_Object,
    supported: Option<&TypeDefinition>,
) -> BTreeMap<String, Value> {
    let mut properties = BTreeMap::new();

    let Some(supported) = supported else {
        return properties;
    };

    for property in &supported.properties {
        if !property.queryable {
            continue;
        }

        if let Some(value) = unsafe { read_field_value(object, &property.name) } {
            properties.insert(property.name.clone(), value);
        }
    }

    properties
}

#[cfg(feature = "native")]
unsafe fn read_special_properties(
    object: *const libredwg_sys::Dwg_Object,
    raw_type_name: &str,
) -> BTreeMap<String, Value> {
    let mut properties = BTreeMap::new();

    match raw_type_name {
        "DICTIONARY" | "DICTIONARYWDFLT" => {
            if let Some(value) = unsafe {
                read_json_property(
                    object,
                    libredwg_sys::bridge_dwg_object_dictionary_items_json,
                )
            } {
                properties.insert("items".to_owned(), value);
            }
            if let Some(value) = unsafe {
                read_json_property(
                    object,
                    libredwg_sys::bridge_dwg_object_dictionary_item_handles_json,
                )
            } {
                properties.insert("item_handles".to_owned(), value);
            }
        }
        "XRECORD" => {
            if let Some(value) = unsafe {
                read_json_property(object, libredwg_sys::bridge_dwg_object_xrecord_xdata_json)
            } {
                properties.insert("xdata".to_owned(), value);
            }
        }
        raw_type_name if raw_type_name.starts_with("BLOCK") => {
            if let Some(value) = unsafe {
                read_json_property(
                    object,
                    libredwg_sys::bridge_dwg_object_block_connections_json,
                )
            } {
                properties.insert("connections".to_owned(), value);
            }
        }
        "EVALUATION_GRAPH" => {
            if let Some(value) = unsafe {
                read_json_property(
                    object,
                    libredwg_sys::bridge_dwg_object_evaluation_graph_nodes_json,
                )
            } {
                properties.insert("nodes".to_owned(), value);
            }
            if let Some(value) = unsafe {
                read_json_property(
                    object,
                    libredwg_sys::bridge_dwg_object_evaluation_graph_edges_json,
                )
            } {
                properties.insert("edges".to_owned(), value);
            }
        }
        "HATCH" => {
            if let Some(value) = unsafe {
                read_json_property(object, libredwg_sys::bridge_dwg_object_hatch_contours_json)
            } {
                properties.insert("contours".to_owned(), value);
            }
        }
        "LWPOLYLINE" => {
            if let Some(Value::Object(value)) = unsafe {
                read_json_property(
                    object,
                    libredwg_sys::bridge_dwg_object_lwpolyline_geometry_json,
                )
            } {
                properties.extend(value);
            }
        }
        "SPLINE" => {
            if let Some(Value::Object(value)) = unsafe {
                read_json_property(object, libredwg_sys::bridge_dwg_object_spline_geometry_json)
            } {
                properties.extend(value);
            }
        }
        "TABLE" => {
            if let Some(Value::Object(value)) = unsafe {
                read_json_property(object, libredwg_sys::bridge_dwg_object_proxy_table_json)
            } {
                properties.extend(value);
            }
        }
        _ => {}
    }

    properties
}

#[cfg(feature = "native")]
unsafe fn read_field_value(
    object: *const libredwg_sys::Dwg_Object,
    field_name: &str,
) -> Option<Value> {
    let c_field = CString::new(field_name).ok()?;
    let mut raw = std::mem::MaybeUninit::<libredwg_sys::BridgeDwgFieldValue>::zeroed();
    let ok = unsafe {
        libredwg_sys::bridge_dwg_object_read_field(object, c_field.as_ptr(), raw.as_mut_ptr())
    };
    if ok {
        let mut raw = unsafe { raw.assume_init() };
        let value = bridge_field_value_to_json(&raw);
        unsafe {
            libredwg_sys::bridge_dwg_field_value_free(&mut raw);
        }
        if value.is_some() {
            return value;
        }
    }

    let raw = unsafe { libredwg_sys::bridge_dwg_object_read_field_json(object, c_field.as_ptr()) };
    if raw.is_null() {
        return None;
    }

    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        libredwg_sys::bridge_dwg_string_free(raw);
    }
    serde_json::from_str(&text).ok()
}

#[cfg(feature = "native")]
unsafe fn read_header_field_value(
    dwg: *mut libredwg_sys::Dwg_Data,
    field_name: &str,
) -> Option<Value> {
    let c_field = CString::new(field_name).ok()?;
    let mut raw = std::mem::MaybeUninit::<libredwg_sys::BridgeDwgFieldValue>::zeroed();
    let ok = unsafe {
        libredwg_sys::bridge_dwg_header_read_field(dwg, c_field.as_ptr(), raw.as_mut_ptr())
    };
    if !ok {
        return None;
    }

    let mut raw = unsafe { raw.assume_init() };
    let value = bridge_field_value_to_json(&raw);
    unsafe {
        libredwg_sys::bridge_dwg_field_value_free(&mut raw);
    }
    value
}

#[cfg(feature = "native")]
unsafe fn read_json_property(
    object: *const libredwg_sys::Dwg_Object,
    reader: unsafe extern "C" fn(*const libredwg_sys::Dwg_Object) -> *mut c_char,
) -> Option<Value> {
    let raw = unsafe { reader(object) };
    if raw.is_null() {
        return None;
    }

    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        libredwg_sys::bridge_dwg_string_free(raw);
    }

    serde_json::from_str(&text).ok()
}

#[cfg(feature = "native")]
fn bridge_field_value_to_json(value: &libredwg_sys::BridgeDwgFieldValue) -> Option<Value> {
    match value.kind {
        x if x == libredwg_sys::BridgeDwgFieldKind_BRIDGE_DWG_FIELD_STRING as i32 => {
            let text = c_string_to_owned(value.string_value)?;
            Some(Value::String(text))
        }
        x if x == libredwg_sys::BridgeDwgFieldKind_BRIDGE_DWG_FIELD_HANDLE as i32 => {
            Some(Value::String(format!("{:X}", value.handle_value)))
        }
        x if x == libredwg_sys::BridgeDwgFieldKind_BRIDGE_DWG_FIELD_INTEGER as i32 => {
            Some(json!(value.integer_value))
        }
        x if x == libredwg_sys::BridgeDwgFieldKind_BRIDGE_DWG_FIELD_DOUBLE as i32 => {
            Some(json!(value.double_value))
        }
        x if x == libredwg_sys::BridgeDwgFieldKind_BRIDGE_DWG_FIELD_BOOL as i32 => {
            Some(json!(value.integer_value != 0))
        }
        x if x == libredwg_sys::BridgeDwgFieldKind_BRIDGE_DWG_FIELD_POINT2D as i32 => {
            Some(json!([value.point_x, value.point_y]))
        }
        x if x == libredwg_sys::BridgeDwgFieldKind_BRIDGE_DWG_FIELD_POINT3D as i32 => {
            Some(json!([value.point_x, value.point_y, value.point_z]))
        }
        x if x == libredwg_sys::BridgeDwgFieldKind_BRIDGE_DWG_FIELD_TIME as i32 => Some(json!({
            "days": value.time_days,
            "milliseconds": value.time_ms,
            "value": value.time_value,
        })),
        _ => None,
    }
}

fn select_summary_properties(properties: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    let preferred = [
        "name",
        "tag",
        "text",
        "text_value",
        "ACADVER",
        "DWGCODEPAGE",
        "MEASUREMENT",
        "INSUNITS",
        "LUNITS",
        "HANDSEED",
        "CLAYER",
        "num_rows",
        "num_cols",
        "cell_texts",
        "layer",
        "ownerhandle",
        "xdicobjhandle",
        "block_header",
        "enhanced_block_data_handle",
        "enhanced_block_data_xrecords",
        "layout",
        "base_pt",
        "ins_pt",
        "rotation",
        "scale",
        "color",
        "numitems",
        "item_handles",
        "xdata_size",
    ];

    let mut selected = BTreeMap::new();
    for property_name in preferred {
        if let Some(value) = properties.get(property_name) {
            selected.insert(property_name.to_owned(), value.clone());
        }
        if selected.len() >= 5 {
            break;
        }
    }

    if selected.is_empty() {
        for (name, value) in properties {
            if selected.len() >= 5 {
                break;
            }
            if is_summary_friendly(value) {
                selected.insert(name.clone(), value.clone());
            }
        }
    }

    selected
}

fn is_summary_friendly(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => true,
        Value::Array(items) => items.len() <= 8 && items.iter().all(is_summary_friendly),
        Value::Object(items) => items.len() <= 4,
    }
}

fn c_string_to_owned(value: *const std::os::raw::c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }

    Some(
        unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn object_by_handle<'a>(
    objects: &'a [IndexedObject],
    indices_by_handle: &HashMap<String, usize>,
    handle: &str,
) -> Option<&'a IndexedObject> {
    indices_by_handle
        .get(handle)
        .and_then(|index| objects.get(*index))
}

fn property_string<'a>(object: &'a IndexedObject, property: &str) -> Option<&'a str> {
    object.value_for_property(property).and_then(Value::as_str)
}

fn property_handle_array(object: &IndexedObject, property: &str) -> Vec<String> {
    object
        .value_for_property(property)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::augment_polyline_vertex_properties;
    use dwg_worker_core::IndexedObject;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    fn object(
        handle: &str,
        type_name: &str,
        properties: impl IntoIterator<Item = (&'static str, Value)>,
    ) -> IndexedObject {
        IndexedObject {
            handle: handle.to_owned(),
            kind: "entity".to_owned(),
            type_name: type_name.to_owned(),
            generic_type: String::new(),
            summary_properties: BTreeMap::new(),
            full_properties: properties
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
            container_block_handle: None,
            layout_handle: None,
            space: None,
        }
    }

    #[test]
    fn augment_polyline_vertex_properties_builds_vertices_for_3d_polylines() {
        let mut objects = vec![
            object(
                "P1",
                "AcDb3dPolyline",
                [("first_vertex", json!("V1")), ("last_vertex", json!("V2"))],
            ),
            object(
                "V1",
                "AcDb3dPolylineVertex",
                [
                    ("ownerhandle", json!("P1")),
                    ("next_entity", json!("V2")),
                    ("point", json!([1.0, 2.0, 3.0])),
                ],
            ),
            object(
                "V2",
                "AcDb3dPolylineVertex",
                [
                    ("ownerhandle", json!("P1")),
                    ("next_entity", json!("S1")),
                    ("point", json!([4.0, 5.0, 6.0])),
                ],
            ),
            object("S1", "SEQEND", [("ownerhandle", json!("P1"))]),
        ];

        augment_polyline_vertex_properties(&mut objects);

        let polyline = &objects[0];
        assert_eq!(
            polyline.full_properties.get("vertex_handles"),
            Some(&json!(["V1", "V2"]))
        );
        assert_eq!(
            polyline.full_properties.get("vertices"),
            Some(&json!([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]))
        );
    }
}
