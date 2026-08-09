use super::*;

/// A cell participating in an LSP notebook document.
#[derive(Clone)]
pub struct NotebookCellDescriptor {
    pub id: String,
    pub kind: lsp::notebook::NotebookCellKind,
    pub language_id: String,
    pub file_extension: String,
    pub buffer: Entity<Buffer>,
}

impl NotebookCellDescriptor {
    pub fn code(
        id: impl Into<String>,
        language_id: impl Into<String>,
        file_extension: impl Into<String>,
        buffer: Entity<Buffer>,
    ) -> Self {
        Self {
            id: id.into(),
            kind: lsp::notebook::NotebookCellKind::CODE,
            language_id: language_id.into(),
            file_extension: file_extension.into(),
            buffer,
        }
    }
}

/// Keeps a notebook and all of its cell documents registered with language
/// servers for as long as the notebook editor is alive.
#[derive(Clone)]
pub struct OpenNotebookDocumentHandle(Entity<OpenNotebookDocument>);

struct OpenNotebookDocument {
    registration_id: u64,
    notebook_uri: lsp::Uri,
    cells: Vec<Entity<Buffer>>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone)]
pub(super) struct RegisteredNotebookCell {
    pub kind: lsp::notebook::NotebookCellKind,
    pub language_id: String,
    pub uri: lsp::Uri,
    pub buffer: Entity<Buffer>,
}

#[derive(Clone)]
pub(super) struct RegisteredNotebook {
    pub registration_id: u64,
    pub uri: lsp::Uri,
    pub notebook_type: String,
    pub version: i32,
    pub cells: Vec<RegisteredNotebookCell>,
    pub opened_in_servers: HashSet<LanguageServerId>,
}

impl RegisteredNotebook {
    pub fn cell_languages(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|cell| cell.language_id.clone())
            .collect()
    }
}

fn safe_cell_file_name(notebook_name: &str, cell_id: &str, extension: &str) -> String {
    let notebook_name = notebook_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let cell_id = cell_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!(".{notebook_name}.cell-{cell_id}.{extension}")
}

impl LspStore {
    /// Registers an LSP 3.17 notebook document and its cell text documents.
    ///
    /// Cell buffers receive stable, notebook-derived URIs. The files do not
    /// exist on disk; associating them with the worktree lets the existing LSP
    /// request pipeline reuse its mature position and response mapping while
    /// notebook-capable servers receive the proper notebook lifecycle.
    pub fn register_notebook_document(
        &mut self,
        project_path: ProjectPath,
        notebook_type: String,
        cells: Vec<NotebookCellDescriptor>,
        cx: &mut Context<Self>,
    ) -> Option<OpenNotebookDocumentHandle> {
        let local = self.as_local_mut()?;
        let worktree = local
            .worktree_store
            .read(cx)
            .worktree_for_id(project_path.worktree_id, cx)?;
        let notebook_abs_path = worktree.read(cx).absolutize(&project_path.path);
        let notebook_uri = lsp::Uri::from_file_path(&notebook_abs_path).ok()?;
        local.next_notebook_registration_id = local
            .next_notebook_registration_id
            .checked_add(1)
            .expect("notebook LSP registration id overflowed");
        let registration_id = local.next_notebook_registration_id;
        let notebook_entry_id = worktree
            .read(cx)
            .entry_for_path(&project_path.path)
            .map(|entry| entry.id);
        let notebook_name = project_path
            .path
            .file_stem()
            .unwrap_or("notebook")
            .to_string();
        let parent = project_path.path.parent().unwrap_or(RelPath::empty());

        let mut registered_cells = Vec::with_capacity(cells.len());
        for descriptor in cells {
            let file_name = safe_cell_file_name(
                &notebook_name,
                &descriptor.id,
                descriptor.file_extension.trim_start_matches('.'),
            );
            let cell_path = parent
                .join(RelPath::from_unix_str(&file_name).ok()?)
                .into_arc();
            let cell_abs_path = worktree.read(cx).absolutize(&cell_path);
            let cell_uri = lsp::Uri::from_file_path(&cell_abs_path).ok()?;
            descriptor.buffer.update(cx, |buffer, cx| {
                buffer.file_updated(
                    Arc::new(File {
                        worktree: worktree.clone(),
                        path: cell_path,
                        disk_state: language::DiskState::New,
                        entry_id: notebook_entry_id,
                        is_local: true,
                        is_private: true,
                    }),
                    cx,
                );
            });
            registered_cells.push(RegisteredNotebookCell {
                kind: descriptor.kind,
                language_id: descriptor.language_id,
                uri: cell_uri,
                buffer: descriptor.buffer,
            });
        }

        let registered_notebook = RegisteredNotebook {
            registration_id,
            uri: notebook_uri.clone(),
            notebook_type,
            version: 0,
            cells: registered_cells.clone(),
            opened_in_servers: HashSet::default(),
        };
        for cell in &registered_cells {
            local
                .notebook_cells
                .insert(cell.buffer.read(cx).remote_id(), notebook_uri.clone());
        }
        local
            .notebook_documents
            .insert(notebook_uri.clone(), registered_notebook);

        let mut subscriptions = Vec::with_capacity(registered_cells.len());
        for cell in &registered_cells {
            let buffer_id = cell.buffer.read(cx).remote_id();
            local.registered_buffers.insert(buffer_id, 1);
            subscriptions.push(cx.subscribe(&cell.buffer, |this, buffer, event, cx| {
                this.on_buffer_event(buffer, event, cx);
            }));
            local.register_buffer_with_language_servers(&cell.buffer, HashSet::default(), cx);
        }

        let handle = OpenNotebookDocumentHandle(cx.new(|_| {
            OpenNotebookDocument {
                registration_id,
                notebook_uri: notebook_uri.clone(),
                cells: registered_cells
                    .iter()
                    .map(|cell| cell.buffer.clone())
                    .collect(),
                _subscriptions: subscriptions,
            }
        }));
        cx.observe_release(&handle.0, move |lsp_store, notebook, cx| {
            lsp_store.close_notebook_document(
                notebook.registration_id,
                &notebook.notebook_uri,
                &notebook.cells,
                cx,
            );
        })
        .detach();
        log::info!(
            "[notebook::lsp] registered notebook {} as generation {} with {} cells",
            notebook_uri.as_str(),
            registration_id,
            registered_cells.len()
        );
        Some(handle)
    }

    pub fn save_notebook_document(
        &mut self,
        handle: &OpenNotebookDocumentHandle,
        cx: &mut Context<Self>,
    ) {
        let notebook_uri = handle.0.read(cx).notebook_uri.clone();
        let Some(local) = self.as_local() else {
            return;
        };
        let Some(notebook) = local.notebook_documents.get(&notebook_uri) else {
            return;
        };
        for server_id in &notebook.opened_in_servers {
            let Some(server) = local.running_language_server_for_id(*server_id) else {
                continue;
            };
            if server
                .notebook_document_sync()
                .and_then(|options| options.save)
                .unwrap_or(false)
            {
                server
                    .notify::<lsp::notebook::DidSaveNotebookDocument>(
                        lsp::notebook::DidSaveNotebookDocumentParams {
                            notebook_document: lsp::notebook::NotebookDocumentIdentifier {
                                uri: notebook_uri.clone(),
                            },
                        },
                    )
                    .ok();
            }
        }
    }

    fn close_notebook_document(
        &mut self,
        registration_id: u64,
        notebook_uri: &lsp::Uri,
        cells: &[Entity<Buffer>],
        cx: &mut Context<Self>,
    ) {
        let buffer_ids = cells
            .iter()
            .map(|buffer| buffer.read(cx).remote_id())
            .collect::<Vec<_>>();
        {
            let Some(local) = self.as_local_mut() else {
                return;
            };
            if local
                .notebook_documents
                .get(notebook_uri)
                .is_some_and(|notebook| notebook.registration_id != registration_id)
            {
                log::info!(
                    "[notebook::lsp] ignored stale close for notebook {} generation {}",
                    notebook_uri.as_str(),
                    registration_id
                );
                return;
            }
            let Some(notebook) = local.notebook_documents.remove(notebook_uri) else {
                return;
            };
            log::info!(
                "[notebook::lsp] closing notebook {} generation {}",
                notebook_uri.as_str(),
                registration_id
            );
            let cell_text_documents: Vec<lsp::TextDocumentIdentifier> = notebook
                .cells
                .iter()
                .map(|cell| lsp::TextDocumentIdentifier {
                    uri: cell.uri.clone(),
                })
                .collect();
            for server_id in &notebook.opened_in_servers {
                if let Some(server) = local.running_language_server_for_id(*server_id) {
                    server
                        .notify::<lsp::notebook::DidCloseNotebookDocument>(
                            lsp::notebook::DidCloseNotebookDocumentParams {
                                notebook_document: lsp::notebook::NotebookDocumentIdentifier {
                                    uri: notebook_uri.clone(),
                                },
                                cell_text_documents: cell_text_documents.clone(),
                            },
                        )
                        .ok();
                }
            }
            for cell in &notebook.cells {
                let buffer_id = cell.buffer.read(cx).remote_id();
                if let Some(snapshots) = local.buffer_snapshots.get(&buffer_id) {
                    for server_id in snapshots.keys() {
                        if let Some(server) = local.running_language_server_for_id(*server_id)
                            && !notebook.opened_in_servers.contains(server_id)
                        {
                            server.unregister_buffer(cell.uri.clone());
                        }
                    }
                }
            }
            for buffer_id in &buffer_ids {
                local.notebook_cells.remove(buffer_id);
                local.registered_buffers.remove(buffer_id);
                local.buffers_opened_in_servers.remove(buffer_id);
                local.buffer_snapshots.remove(buffer_id);
            }
        }
        for buffer_id in buffer_ids {
            self.lsp_data.remove(&buffer_id);
            self.buffer_reload_tasks.remove(&buffer_id);
        }
    }
}
