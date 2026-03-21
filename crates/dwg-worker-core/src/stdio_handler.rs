use std::io::{BufRead, Write};
use std::path::PathBuf;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use crate::backend::{BackendFactory, DwgDocument, WorkerError};
use crate::protocol::{
    CloseFileResult, DescribeTypeParams, GetObjectsParams, HealthResult, ListFileTypesResult,
    ListTypesParams, ListTypesResult, OpenFileParams, OpenFileResult, QueryObjectsParams,
    RequestEnvelope, ResponseEnvelope, ResponseError,
};

pub struct StdioHandler<F: BackendFactory> {
    factory: F,
    document: Option<F::Document>,
}

impl<F: BackendFactory> StdioHandler<F> {
    pub fn new(factory: F) -> Self {
        Self {
            factory,
            document: None,
        }
    }

    pub fn serve<R: BufRead, W: Write>(&mut self, reader: R, mut writer: W) -> std::io::Result<()> {
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let response = match serde_json::from_str::<RequestEnvelope>(&line) {
                Ok(request) => self.handle_request(request),
                Err(error) => ResponseEnvelope {
                    id: 0,
                    result: None,
                    error: Some(ResponseError {
                        code: "invalid_request".to_owned(),
                        message: error.to_string(),
                    }),
                },
            };

            serde_json::to_writer(&mut writer, &response)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }

        Ok(())
    }

    pub fn handle_request(&mut self, request: RequestEnvelope) -> ResponseEnvelope {
        let result = match request.method.as_str() {
            "health" => self.to_response(
                request.id,
                &HealthResult {
                    status: "ok",
                    backend: self.factory.backend_name().to_owned(),
                    document_open: self.document.is_some(),
                },
            ),
            "openFile" => self.handle_open_file(request.id, request.params),
            "closeFile" => self.handle_close_file(request.id),
            "listTypes" => self.handle_list_types(request.id, request.params),
            "listFileTypes" => self.handle_list_file_types(request.id, request.params),
            "describeType" => self.handle_describe_type(request.id, request.params),
            "getObjects" => self.handle_get_objects(request.id, request.params),
            "queryObjects" => self.handle_query_objects(request.id, request.params),
            _ => self.error_response(
                request.id,
                WorkerError::InvalidRequest(format!("unknown method {}", request.method)),
            ),
        };

        result
    }

    fn handle_open_file(&mut self, id: u64, params: Value) -> ResponseEnvelope {
        let params: OpenFileParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return self.error_response(id, WorkerError::InvalidRequest(error.to_string()));
            }
        };

        match self.factory.open(PathBuf::from(&params.path).as_path()) {
            Ok(document) => {
                self.document = Some(document);
                self.to_response(
                    id,
                    &OpenFileResult {
                        backend: self.factory.backend_name().to_owned(),
                        path: params.path,
                    },
                )
            }
            Err(error) => self.error_response(id, error),
        }
    }

    fn handle_close_file(&mut self, id: u64) -> ResponseEnvelope {
        let closed = self.document.take().is_some();
        self.to_response(id, &CloseFileResult { closed })
    }

    fn handle_list_types(&mut self, id: u64, params: Value) -> ResponseEnvelope {
        let params: ListTypesParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return self.error_response(id, WorkerError::InvalidRequest(error.to_string()));
            }
        };

        match self
            .factory
            .list_supported_types()
            .and_then(|items| self.paginate_types(items, &params))
        {
            Ok(result) => self.to_response(id, &result),
            Err(error) => self.error_response(id, error),
        }
    }

    fn handle_list_file_types(&mut self, id: u64, params: Value) -> ResponseEnvelope {
        let params: ListTypesParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return self.error_response(id, WorkerError::InvalidRequest(error.to_string()));
            }
        };

        match self.require_document() {
            Ok(document) => match self.paginate_types(document.list_types(), &params) {
                Ok(result) => self.to_response(
                    id,
                    &ListFileTypesResult {
                        total: result.total,
                        next_cursor: result.next_cursor,
                        items: result.items,
                    },
                ),
                Err(error) => self.error_response(id, error),
            },
            Err(error) => self.error_response(id, error),
        }
    }

    fn handle_describe_type(&mut self, id: u64, params: Value) -> ResponseEnvelope {
        let params: DescribeTypeParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return self.error_response(id, WorkerError::InvalidRequest(error.to_string()));
            }
        };

        match self.factory.describe_supported_type(&params.type_name) {
            Ok(item) => self.to_response(id, &item),
            Err(error) => self.error_response(id, error),
        }
    }

    fn handle_get_objects(&mut self, id: u64, params: Value) -> ResponseEnvelope {
        let params: GetObjectsParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return self.error_response(id, WorkerError::InvalidRequest(error.to_string()));
            }
        };

        match self
            .require_document()
            .and_then(|document| document.get_objects(params))
        {
            Ok(result) => self.to_response(id, &result),
            Err(error) => self.error_response(id, error),
        }
    }

    fn handle_query_objects(&mut self, id: u64, params: Value) -> ResponseEnvelope {
        let params: QueryObjectsParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return self.error_response(id, WorkerError::InvalidRequest(error.to_string()));
            }
        };

        match self
            .require_document()
            .and_then(|document| document.query_objects(params))
        {
            Ok(result) => self.to_response(id, &result),
            Err(error) => self.error_response(id, error),
        }
    }

    fn require_document(&self) -> Result<&F::Document, WorkerError> {
        self.document.as_ref().ok_or(WorkerError::DocumentNotOpen)
    }

    fn paginate_types(
        &self,
        mut items: Vec<crate::model::TypeDefinition>,
        params: &ListTypesParams,
    ) -> Result<ListTypesResult, WorkerError> {
        items.sort_by(|left, right| left.type_name.cmp(&right.type_name));

        if let Some(pattern) = params.regex.as_deref() {
            let regex = Regex::new(pattern)
                .map_err(|error| WorkerError::InvalidRequest(error.to_string()))?;
            items.retain(|item| {
                regex.is_match(&item.type_name)
                    || regex.is_match(&item.generic_type)
                    || item.aliases.iter().any(|alias| regex.is_match(alias))
            });
        }

        let total = items.len();
        let start = Self::parse_cursor(params.cursor.as_deref())?;
        let limit = params.limit.max(1);
        let end = total.min(start.saturating_add(limit));
        let page = if start >= total {
            Vec::new()
        } else {
            items[start..end].to_vec()
        };

        Ok(ListTypesResult {
            total,
            next_cursor: (end < total).then(|| end.to_string()),
            items: page,
        })
    }

    fn parse_cursor(cursor: Option<&str>) -> Result<usize, WorkerError> {
        let Some(cursor) = cursor else {
            return Ok(0);
        };

        cursor
            .parse::<usize>()
            .map_err(|_| WorkerError::InvalidCursor(cursor.to_owned()))
    }

    fn to_response<T: Serialize>(&self, id: u64, payload: &T) -> ResponseEnvelope {
        match serde_json::to_value(payload) {
            Ok(result) => ResponseEnvelope {
                id,
                result: Some(result),
                error: None,
            },
            Err(error) => self.error_response(id, WorkerError::InvalidRequest(error.to_string())),
        }
    }

    fn error_response(&self, id: u64, error: WorkerError) -> ResponseEnvelope {
        let code = match error {
            WorkerError::DocumentNotOpen => "document_not_open",
            WorkerError::InvalidRequest(_) => "invalid_request",
            WorkerError::InvalidCursor(_) => "invalid_cursor",
            WorkerError::UnknownType(_) => "unknown_type",
            WorkerError::Unsupported(_) => "unsupported",
            WorkerError::BackendUnavailable(_) => "backend_unavailable",
            WorkerError::OpenFailed(_) => "open_failed",
        };

        ResponseEnvelope {
            id,
            result: None,
            error: Some(ResponseError {
                code: code.to_owned(),
                message: error.to_string(),
            }),
        }
    }
}
