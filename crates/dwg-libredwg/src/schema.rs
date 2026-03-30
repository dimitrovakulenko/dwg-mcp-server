use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use dwg_worker_core::{PropertyDefinition, TypeDefinition, WorkerError};

static CATALOG: OnceLock<Result<SchemaCatalog, WorkerError>> = OnceLock::new();

#[derive(Clone, Debug)]
struct SchemaType {
    source_name: String,
    canonical_name: String,
    aliases: Vec<String>,
    properties: Vec<PropertyDefinition>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SchemaCatalog {
    types_by_source: HashMap<String, SchemaType>,
    lookup_to_source: HashMap<String, String>,
    ordered_sources: Vec<String>,
}

impl SchemaCatalog {
    pub(crate) fn load() -> Result<&'static Self, WorkerError> {
        match CATALOG.get_or_init(Self::load_from_vendor) {
            Ok(catalog) => Ok(catalog),
            Err(error) => Err(WorkerError::BackendUnavailable(error.to_string())),
        }
    }

    fn load_from_vendor() -> Result<Self, WorkerError> {
        let classes_path = vendor_src_root().join("classes.c");
        let dynapi_path = vendor_src_root().join("dynapi.c");

        let classes = fs::read_to_string(&classes_path).map_err(|error| {
            WorkerError::BackendUnavailable(format!(
                "failed to read {}: {error}",
                classes_path.display()
            ))
        })?;
        let dynapi = fs::read_to_string(&dynapi_path).map_err(|error| {
            WorkerError::BackendUnavailable(format!(
                "failed to read {}: {error}",
                dynapi_path.display()
            ))
        })?;

        let mut ordered_sources = parse_type_names(&classes)?;
        ordered_sources.sort();
        ordered_sources.dedup();

        let aliases_by_source = parse_aliases(&dynapi)?;
        let common_entity_properties =
            parse_common_field_definitions(&dynapi, "_dwg_object_entity_fields[]")?;
        let common_object_properties =
            parse_common_field_definitions(&dynapi, "_dwg_object_object_fields[]")?;
        let properties_by_source = parse_field_definitions(&dynapi)?;

        let mut types_by_source = HashMap::new();
        let mut lookup_to_source = HashMap::new();

        for source_name in &ordered_sources {
            let aliases = aliases_by_source.get(source_name).cloned().unwrap_or_default();
            let canonical_name = canonical_name(source_name, &aliases);
            let is_entity = aliases.iter().any(|alias| alias == "AcDbEntity");
            let specific_properties = properties_by_source
                .get(source_name)
                .cloned()
                .unwrap_or_default();
            let properties = merge_schema_property_sets(
                if is_entity {
                    &common_entity_properties
                } else {
                    &common_object_properties
                },
                &specific_properties,
            );
            let properties = extend_custom_properties(source_name, &canonical_name, properties);

            let schema_type = SchemaType {
                source_name: source_name.clone(),
                canonical_name: canonical_name.clone(),
                aliases: aliases.clone(),
                properties,
            };

            lookup_to_source.insert(source_name.clone(), source_name.clone());
            lookup_to_source.insert(canonical_name, source_name.clone());
            for alias in aliases {
                lookup_to_source.insert(alias, source_name.clone());
            }

            types_by_source.insert(source_name.clone(), schema_type);
        }

        Ok(Self {
            types_by_source,
            lookup_to_source,
            ordered_sources,
        })
    }

    pub(crate) fn list_supported_types(&self) -> Vec<TypeDefinition> {
        let mut items = self
            .ordered_sources
            .iter()
            .filter_map(|source_name| self.general_type_definition(source_name))
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.type_name.cmp(&right.type_name));
        items
    }

    pub(crate) fn describe_type(&self, type_name: &str) -> Option<TypeDefinition> {
        self.general_type_definition(type_name)
    }

    pub(crate) fn type_definition_for_observed(
        &self,
        observed_type_name: &str,
        observed_property_names: &[String],
    ) -> TypeDefinition {
        self.resolve_schema_type(observed_type_name)
            .map(|item| {
                let mut aliases = item.aliases.clone();
                if item.canonical_name != observed_type_name
                    && !aliases.iter().any(|alias| alias == &item.canonical_name)
                {
                    aliases.push(item.canonical_name.clone());
                }

                TypeDefinition {
                    type_name: observed_type_name.to_owned(),
                    generic_type: to_generic_name(&item.canonical_name),
                    description: (item.source_name != observed_type_name).then(|| {
                        format!(
                            "LibreDWG source type {} (canonical type {})",
                            item.source_name, item.canonical_name
                        )
                    }),
                    aliases,
                    default_select: choose_default_select(observed_property_names),
                    properties: merge_properties(&item.properties, observed_property_names),
                }
            })
            .unwrap_or_else(|| inferred_type_definition(observed_type_name, observed_property_names))
    }

    fn general_type_definition(&self, lookup_name: &str) -> Option<TypeDefinition> {
        self.resolve_schema_type(lookup_name).map(|item| {
            let aliases = item
                .aliases
                .iter()
                .filter(|alias| *alias != &item.canonical_name)
                .cloned()
                .collect::<Vec<_>>();

            TypeDefinition {
                type_name: item.canonical_name.clone(),
                generic_type: to_generic_name(&item.canonical_name),
                description: (item.canonical_name != item.source_name)
                    .then(|| format!("LibreDWG source type {}", item.source_name)),
                aliases,
                default_select: choose_default_select(
                    &item
                        .properties
                        .iter()
                        .map(|property| property.name.clone())
                        .collect::<Vec<_>>(),
                ),
                properties: item.properties.clone(),
            }
        })
    }

    fn resolve_schema_type(&self, lookup_name: &str) -> Option<&SchemaType> {
        let source_name = self.lookup_to_source.get(lookup_name)?;
        self.types_by_source.get(source_name)
    }
}

pub fn list_supported_types() -> Result<Vec<TypeDefinition>, WorkerError> {
    Ok(SchemaCatalog::load()?.list_supported_types())
}

pub fn describe_supported_type(type_name: &str) -> Result<TypeDefinition, WorkerError> {
    SchemaCatalog::load()?
        .describe_type(type_name)
        .ok_or_else(|| WorkerError::UnknownType(type_name.to_owned()))
}

fn inferred_type_definition(type_name: &str, observed_property_names: &[String]) -> TypeDefinition {
    TypeDefinition {
        type_name: type_name.to_owned(),
        generic_type: to_generic_name(type_name),
        description: None,
        aliases: Vec::new(),
        default_select: choose_default_select(observed_property_names),
        properties: observed_property_names
            .iter()
            .cloned()
            .map(|name| PropertyDefinition {
                name,
                value_kind: "unknown".to_owned(),
                description: None,
                queryable: true,
                reference_target: None,
            })
            .collect(),
    }
}

fn vendor_src_root() -> PathBuf {
    if let Some(configured) = std::env::var_os("LIBREDWG_SOURCE_ROOT") {
        return PathBuf::from(configured);
    }

    let manifest_candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("third_party/libredwg/src");
    if manifest_candidate.exists() {
        return manifest_candidate;
    }

    if let Ok(executable) = std::env::current_exe() {
        if let Some(app_root) = executable.parent().and_then(|parent| parent.parent()) {
            let runtime_candidate = app_root.join("third_party/libredwg/src");
            if runtime_candidate.exists() {
                return runtime_candidate;
            }
        }
    }

    manifest_candidate
}

fn parse_type_names(content: &str) -> Result<Vec<String>, WorkerError> {
    let fixed = extract_names_from_array(content, "static const char *const _dwg_type_names_fixed[] =")?;
    let variable =
        extract_names_from_array(content, "static const char *const _dwg_type_names_variable[] =")?;

    Ok(fixed.into_iter().chain(variable).collect())
}

fn extract_names_from_array(content: &str, marker: &str) -> Result<Vec<String>, WorkerError> {
    let start = content.find(marker).ok_or_else(|| {
        WorkerError::BackendUnavailable(format!("failed to locate `{marker}` in classes.c"))
    })?;
    let tail = &content[start..];
    let brace_start = tail.find('{').ok_or_else(|| {
        WorkerError::BackendUnavailable(format!("failed to locate array body for `{marker}`"))
    })?;
    let body = &tail[brace_start + 1..];
    let end = body.find("};").ok_or_else(|| {
        WorkerError::BackendUnavailable(format!("failed to close array body for `{marker}`"))
    })?;

    Ok(body[..end]
        .lines()
        .filter_map(extract_first_quoted)
        .collect())
}

fn parse_aliases(content: &str) -> Result<HashMap<String, Vec<String>>, WorkerError> {
    let marker = "static const struct _name_subclasses dwg_name_subclasses[] = {";
    let body = extract_block(content, marker)?;
    let mut aliases = HashMap::new();

    for line in body.lines() {
        let parts = extract_quoted_strings(line);
        if parts.len() < 2 {
            continue;
        }

        aliases.insert(parts[0].clone(), parts[1..].to_vec());
    }

    Ok(aliases)
}

fn parse_field_definitions(
    content: &str,
) -> Result<HashMap<String, Vec<PropertyDefinition>>, WorkerError> {
    let mut properties_by_type = HashMap::new();
    let mut current_type = None::<String>;
    let mut current_properties = Vec::new();

    for line in content.lines() {
        if let Some(type_name) = line
            .trim()
            .strip_prefix("static const Dwg_DYNAPI_field _dwg_")
            .and_then(|tail| tail.strip_suffix("_fields[] = {"))
        {
            current_type = Some(type_name.to_owned());
            current_properties.clear();
            continue;
        }

        if line.trim() == "};" {
            if let Some(type_name) = current_type.take() {
                properties_by_type.insert(type_name, current_properties.clone());
                current_properties.clear();
            }
            continue;
        }

        let Some(_) = current_type else {
            continue;
        };

        let parts = extract_quoted_strings(line);
        if parts.len() < 2 {
            continue;
        }

        let field_name = &parts[0];
        if field_name == "parent" {
            continue;
        }

        current_properties.push(PropertyDefinition {
            name: field_name.clone(),
            value_kind: classify_field_kind(&parts[1]),
            description: None,
            queryable: is_queryable_field(field_name, &parts[1]),
            reference_target: parts[1].contains('H').then(|| "handle".to_owned()),
        });
    }

    if properties_by_type.is_empty() {
        return Err(WorkerError::BackendUnavailable(
            "failed to parse field definitions from dynapi.c".to_owned(),
        ));
    }

    Ok(properties_by_type)
}

fn parse_common_field_definitions(
    content: &str,
    marker_suffix: &str,
) -> Result<Vec<PropertyDefinition>, WorkerError> {
    let marker = format!("static const Dwg_DYNAPI_field {marker_suffix} = {{");
    let body = extract_block(content, &marker)?;
    let properties = parse_properties_block(body);
    if properties.is_empty() {
        return Err(WorkerError::BackendUnavailable(format!(
            "failed to parse common field definitions from `{marker}`"
        )));
    }

    Ok(properties)
}

fn extract_block<'a>(content: &'a str, marker: &str) -> Result<&'a str, WorkerError> {
    let start = content.find(marker).ok_or_else(|| {
        WorkerError::BackendUnavailable(format!("failed to locate `{marker}` in dynapi.c"))
    })?;
    let tail = &content[start..];
    let brace_start = tail.find('{').ok_or_else(|| {
        WorkerError::BackendUnavailable(format!("failed to locate block body for `{marker}`"))
    })?;
    let body = &tail[brace_start + 1..];
    let end = body.find("};").ok_or_else(|| {
        WorkerError::BackendUnavailable(format!("failed to close block body for `{marker}`"))
    })?;
    Ok(&body[..end])
}

fn parse_properties_block(body: &str) -> Vec<PropertyDefinition> {
    let mut properties = Vec::new();

    for line in body.lines() {
        let parts = extract_quoted_strings(line);
        if parts.len() < 2 {
            continue;
        }

        let field_name = &parts[0];
        if field_name == "parent" || field_name == "dwg" || field_name == "eed" {
            continue;
        }

        properties.push(PropertyDefinition {
            name: field_name.clone(),
            value_kind: classify_field_kind(&parts[1]),
            description: None,
            queryable: is_queryable_field(field_name, &parts[1]),
            reference_target: parts[1].contains('H').then(|| "handle".to_owned()),
        });
    }

    properties
}

fn extract_first_quoted(line: &str) -> Option<String> {
    extract_quoted_strings(line).into_iter().next()
}

fn extract_quoted_strings(line: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut chars = line.char_indices();

    while let Some((start_index, ch)) = chars.next() {
        if ch != '"' {
            continue;
        }

        let mut end_index = None;
        for (candidate, next) in chars.by_ref() {
            if next == '"' {
                end_index = Some(candidate);
                break;
            }
        }

        let Some(end_index) = end_index else {
            break;
        };

        parts.push(line[start_index + 1..end_index].to_owned());
    }

    parts
}

fn canonical_name(source_name: &str, aliases: &[String]) -> String {
    aliases
        .iter()
        .rev()
        .find(|alias| !is_generic_alias(alias))
        .cloned()
        .unwrap_or_else(|| source_name.to_owned())
}

fn is_generic_alias(alias: &str) -> bool {
    matches!(
        alias,
        "AcDbEntity"
            | "AcDbObject"
            | "AcDbSymbolTable"
            | "AcDbSymbolTableRecord"
            | "AcDbDimension"
            | "AcDbModelerGeometry"
    )
}

fn classify_field_kind(raw_kind: &str) -> String {
    if matches!(raw_kind, "CMC" | "CMTC" | "ENC") {
        "color".to_owned()
    } else if raw_kind.contains('H') {
        "handle".to_owned()
    } else if raw_kind.starts_with('T') {
        "string".to_owned()
    } else if raw_kind.contains("2D") || raw_kind.contains("3D") || raw_kind.contains("2R")
        || raw_kind.contains("3R")
    {
        "point".to_owned()
    } else if raw_kind.contains('*') {
        "array".to_owned()
    } else if matches!(raw_kind, "B" | "BB") {
        "boolean".to_owned()
    } else {
        "number".to_owned()
    }
}

fn is_queryable_field(field_name: &str, raw_kind: &str) -> bool {
    if field_name == "parent" || field_name.starts_with("unknown") {
        return false;
    }

    if !raw_kind.contains('*') {
        return true;
    }

    matches!(
        raw_kind,
        "2RD*" | "2BD*" | "2DPOINT*" | "3RD*" | "3BD*" | "3DPOINT*" | "BE*"
    )
}

fn merge_properties(
    schema_properties: &[PropertyDefinition],
    observed_property_names: &[String],
) -> Vec<PropertyDefinition> {
    if observed_property_names.is_empty() {
        return schema_properties.to_vec();
    }

    let mut merged = Vec::new();
    let mut seen = BTreeSet::new();

    for property_name in observed_property_names {
        if let Some(schema_property) = schema_properties
            .iter()
            .find(|property| property.name == *property_name)
        {
            merged.push(schema_property.clone());
        } else {
            merged.push(PropertyDefinition {
                name: property_name.clone(),
                value_kind: "unknown".to_owned(),
                description: None,
                queryable: true,
                reference_target: None,
            });
        }

        seen.insert(property_name.clone());
    }

    for property in schema_properties {
        if seen.insert(property.name.clone()) {
            merged.push(property.clone());
        }
    }

    merged
}

fn merge_schema_property_sets(
    common_properties: &[PropertyDefinition],
    specific_properties: &[PropertyDefinition],
) -> Vec<PropertyDefinition> {
    let mut merged = common_properties.to_vec();
    let mut seen = merged
        .iter()
        .map(|property| property.name.clone())
        .collect::<BTreeSet<_>>();

    for property in specific_properties {
        if seen.insert(property.name.clone()) {
            merged.push(property.clone());
        }
    }

    merged
}

fn extend_custom_properties(
    source_name: &str,
    canonical_name: &str,
    mut properties: Vec<PropertyDefinition>,
) -> Vec<PropertyDefinition> {
    let mut push_if_missing = |property: PropertyDefinition| {
        if !properties.iter().any(|item| item.name == property.name) {
            properties.push(property);
        }
    };

    if matches!(source_name, "DICTIONARY" | "DICTIONARYWDFLT")
        || matches!(canonical_name, "AcDbDictionary" | "AcDbDictionaryWithDefault")
    {
        push_if_missing(PropertyDefinition {
            name: "items".to_owned(),
            value_kind: "object".to_owned(),
            description: Some(
                "Dictionary entry name to referenced object handle mapping.".to_owned(),
            ),
            queryable: false,
            reference_target: None,
        });
        push_if_missing(PropertyDefinition {
            name: "item_handles".to_owned(),
            value_kind: "array".to_owned(),
            description: Some(
                "Ordered list of referenced object handles from this dictionary.".to_owned(),
            ),
            queryable: true,
            reference_target: Some("handle".to_owned()),
        });
    }

    if source_name == "XRECORD" || canonical_name == "AcDbXrecord" {
        push_if_missing(PropertyDefinition {
            name: "xdata".to_owned(),
            value_kind: "array".to_owned(),
            description: Some(
                "Raw xrecord payload as [groupCode, value] tuples.".to_owned(),
            ),
            queryable: false,
            reference_target: None,
        });
    }

    if source_name == "POLYLINE_2D"
        || source_name == "POLYLINE_3D"
        || matches!(canonical_name, "AcDb2dPolyline" | "AcDb3dPolyline")
    {
        push_if_missing(PropertyDefinition {
            name: "vertices".to_owned(),
            value_kind: "array".to_owned(),
            description: Some(
                "Ordered vertex coordinates resolved from the polyline vertex chain.".to_owned(),
            ),
            queryable: false,
            reference_target: None,
        });
        push_if_missing(PropertyDefinition {
            name: "vertex_handles".to_owned(),
            value_kind: "array".to_owned(),
            description: Some("Ordered handles of vertex entities in the polyline chain.".to_owned()),
            queryable: true,
            reference_target: Some("handle".to_owned()),
        });
    }

    properties
}

fn choose_default_select(property_names: &[String]) -> Vec<String> {
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    let available = property_names.iter().cloned().collect::<Vec<_>>();
    let preferred = [
        "name",
        "tag",
        "text",
        "text_value",
        "layer",
        "ownerhandle",
        "xdicobjhandle",
        "block_header",
        "layout",
        "base_pt",
        "ins_pt",
        "rotation",
        "color",
        "numitems",
        "item_handles",
        "xdata_size",
    ];

    for property_name in preferred {
        if available.iter().any(|candidate| candidate == property_name)
            && seen.insert(property_name.to_owned())
        {
            selected.push(property_name.to_owned());
        }
    }

    for property_name in available {
        if selected.len() >= 5 {
            break;
        }

        if seen.insert(property_name.clone()) {
            selected.push(property_name);
        }
    }

    selected
}

pub(crate) fn to_generic_name(type_name: &str) -> String {
    let trimmed = type_name.strip_prefix("AcDb").unwrap_or(type_name);
    if trimmed
        .chars()
        .all(|ch| ch.is_ascii_uppercase() || ch == '_' || ch.is_ascii_digit())
    {
        return trimmed.to_ascii_lowercase();
    }

    let mut output = String::new();
    let mut previous_was_separator = false;

    for (index, ch) in trimmed.chars().enumerate() {
        if ch == '_' || ch == '-' || ch == ' ' {
            if !previous_was_separator && !output.is_empty() {
                output.push('_');
            }
            previous_was_separator = true;
            continue;
        }

        if ch.is_ascii_uppercase() && index > 0 && !previous_was_separator {
            output.push('_');
        }

        output.push(ch.to_ascii_lowercase());
        previous_was_separator = false;
    }

    output
}
