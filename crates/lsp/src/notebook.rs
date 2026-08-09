//! Language Server Protocol 3.17 notebook document synchronization.
//!
//! The `lsp-types` revision used by Zed predates notebook synchronization, so
//! these types live here until that dependency can be upgraded without
//! affecting the rest of the editor. They intentionally mirror the protocol
//! structures instead of introducing editor-specific notebook concepts.

use crate::{
    InitializeParams, ServerInfo, TextDocumentContentChangeEvent, TextDocumentIdentifier,
    TextDocumentItem, Uri, VersionedTextDocumentIdentifier, notification, request,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookDocumentClientCapabilities {
    pub synchronization: NotebookDocumentSyncClientCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookDocumentSyncClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamic_registration: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_summary_support: Option<bool>,
}

impl Default for NotebookDocumentClientCapabilities {
    fn default() -> Self {
        Self {
            synchronization: NotebookDocumentSyncClientCapabilities {
                dynamic_registration: Some(false),
                execution_summary_support: Some(true),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookDocumentSyncOptions {
    pub notebook_selector: Vec<NotebookDocumentSyncSelector>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save: Option<bool>,
    // Present when the server returns registration options instead of plain
    // sync options. It does not affect static capability matching.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookDocumentSyncSelector {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notebook: Option<NotebookSelector>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cells: Option<Vec<NotebookCellLanguage>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NotebookSelector {
    Type(String),
    Filter(NotebookDocumentFilter),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookDocumentFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notebook_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotebookCellLanguage {
    pub language: String,
}

impl NotebookDocumentSyncOptions {
    pub fn supports(&self, notebook_type: &str, cell_languages: &[String]) -> bool {
        self.notebook_selector.iter().any(|selector| {
            let notebook_matches = match selector.notebook.as_ref() {
                Some(NotebookSelector::Type(kind)) => kind == "*" || kind == notebook_type,
                Some(NotebookSelector::Filter(filter)) => filter
                    .notebook_type
                    .as_deref()
                    .is_none_or(|kind| kind == "*" || kind == notebook_type),
                None => true,
            };
            let cells_match = selector.cells.as_ref().is_none_or(|cells| {
                cells.iter().any(|cell| {
                    cell.language == "*"
                        || cell_languages
                            .iter()
                            .any(|language| language == &cell.language)
                })
            });
            notebook_matches && cells_match
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NotebookCellKind(pub u8);

impl NotebookCellKind {
    pub const MARKUP: Self = Self(1);
    pub const CODE: Self = Self(2);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSummary {
    pub execution_order: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookCell {
    pub kind: NotebookCellKind,
    pub document: Uri,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_summary: Option<ExecutionSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookDocument {
    pub uri: Uri,
    pub notebook_type: String,
    pub version: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
    pub cells: Vec<NotebookCell>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotebookDocumentIdentifier {
    pub uri: Uri,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionedNotebookDocumentIdentifier {
    pub uri: Uri,
    pub version: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidOpenNotebookDocumentParams {
    pub notebook_document: NotebookDocument,
    pub cell_text_documents: Vec<TextDocumentItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidChangeNotebookDocumentParams {
    pub notebook_document: VersionedNotebookDocumentIdentifier,
    pub change: NotebookDocumentChangeEvent,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookDocumentChangeEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cells: Option<NotebookDocumentCellChanges>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookDocumentCellChanges {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structure: Option<NotebookDocumentCellChangeStructure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<NotebookCell>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_content: Option<Vec<NotebookDocumentCellContentChanges>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookDocumentCellChangeStructure {
    pub array: NotebookCellArrayChange,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub did_open: Option<Vec<TextDocumentItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub did_close: Option<Vec<TextDocumentIdentifier>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookCellArrayChange {
    pub start: u32,
    pub delete_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cells: Option<Vec<NotebookCell>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotebookDocumentCellContentChanges {
    pub document: VersionedTextDocumentIdentifier,
    pub changes: Vec<TextDocumentContentChangeEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidSaveNotebookDocumentParams {
    pub notebook_document: NotebookDocumentIdentifier,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidCloseNotebookDocumentParams {
    pub notebook_document: NotebookDocumentIdentifier,
    pub cell_text_documents: Vec<TextDocumentIdentifier>,
}

pub enum DidOpenNotebookDocument {}
impl notification::Notification for DidOpenNotebookDocument {
    type Params = DidOpenNotebookDocumentParams;
    const METHOD: &'static str = "notebookDocument/didOpen";
}

pub enum DidChangeNotebookDocument {}
impl notification::Notification for DidChangeNotebookDocument {
    type Params = DidChangeNotebookDocumentParams;
    const METHOD: &'static str = "notebookDocument/didChange";
}

pub enum DidSaveNotebookDocument {}
impl notification::Notification for DidSaveNotebookDocument {
    type Params = DidSaveNotebookDocumentParams;
    const METHOD: &'static str = "notebookDocument/didSave";
}

pub enum DidCloseNotebookDocument {}
impl notification::Notification for DidCloseNotebookDocument {
    type Params = DidCloseNotebookDocumentParams;
    const METHOD: &'static str = "notebookDocument/didClose";
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InitializeResult {
    pub capabilities: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_info: Option<ServerInfo>,
}

pub enum Initialize {}
impl request::Request for Initialize {
    type Params = Value;
    type Result = InitializeResult;
    const METHOD: &'static str = "initialize";
}

pub fn initialize_params_with_notebook_support(
    params: InitializeParams,
) -> Result<Value, serde_json::Error> {
    let mut params = serde_json::to_value(params)?;
    let capabilities = params
        .get_mut("capabilities")
        .and_then(Value::as_object_mut)
        .expect("InitializeParams always serializes capabilities as an object");
    capabilities.insert(
        "notebookDocument".into(),
        serde_json::to_value(NotebookDocumentClientCapabilities::default())?,
    );
    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InitializeParams;

    #[test]
    fn advertises_notebook_document_synchronization() {
        let params = initialize_params_with_notebook_support(InitializeParams::default()).unwrap();
        assert_eq!(
            params["capabilities"]["notebookDocument"]["synchronization"],
            serde_json::json!({
                "dynamicRegistration": false,
                "executionSummarySupport": true
            })
        );
    }

    #[test]
    fn parses_and_matches_server_notebook_selectors() {
        let options: NotebookDocumentSyncOptions = serde_json::from_value(serde_json::json!({
            "notebookSelector": [{
                "notebook": { "notebookType": "jupyter-notebook" },
                "cells": [{ "language": "python" }]
            }],
            "save": true
        }))
        .unwrap();

        assert!(options.supports("jupyter-notebook", &["python".into()]));
        assert!(!options.supports("jupyter-notebook", &["rust".into()]));
        assert!(!options.supports("quarto", &["python".into()]));
    }

    #[test]
    fn serializes_notebook_open_notification_shape() {
        let notebook_uri: Uri = "file:///workspace/example.ipynb".parse().unwrap();
        let cell_uri: Uri = "file:///workspace/.example-cell-1.py".parse().unwrap();
        let params = DidOpenNotebookDocumentParams {
            notebook_document: NotebookDocument {
                uri: notebook_uri,
                notebook_type: "jupyter-notebook".into(),
                version: 0,
                metadata: None,
                cells: vec![NotebookCell {
                    kind: NotebookCellKind::CODE,
                    document: cell_uri.clone(),
                    metadata: None,
                    execution_summary: None,
                }],
            },
            cell_text_documents: vec![TextDocumentItem::new(
                cell_uri,
                "python".into(),
                0,
                "value = 1".into(),
            )],
        };

        let value = serde_json::to_value(params).unwrap();
        assert_eq!(
            value["notebookDocument"]["notebookType"],
            "jupyter-notebook"
        );
        assert_eq!(value["notebookDocument"]["cells"][0]["kind"], 2);
        assert_eq!(value["cellTextDocuments"][0]["languageId"], "python");
    }
}
