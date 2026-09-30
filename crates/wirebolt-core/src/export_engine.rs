use crate::{
    Collection, DocumentId, EnvironmentVariable, MultipartPartKind, ProxyMode, Request,
    RequestAuthentication, RequestBody, RequestHeader, RequestValueField, TransportSettings,
    ValueSource, WorkspaceSnapshot,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, error::Error, fmt, path::Path, time::SystemTime};

/// Identifier of the environment whose variables apply to every request
/// (`WorkspaceDraft.globalEnvironmentID` in `WireboltKit`).
pub const GLOBAL_ENVIRONMENT_ID: &str = "global";

#[derive(Debug)]
pub struct ExportError(&'static str);
impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl Error for ExportError {}

/// The `wirebolt` block of an exported request node.
///
/// The portable legacy fields cannot carry secret references, sensitive flags,
/// content types, upload paths, proxy or transport, so Wirebolt writes its own
/// typed definition beside them and prefers it on re-import. It holds secret
/// reference names only, never secret material.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct NativeRequest {
    pub(crate) authentication: RequestAuthentication,
    /// Absent in exports written before the complete definition was included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) request: Option<NativeDefinition>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct NativeDefinition {
    pub(crate) headers: Vec<RequestHeader>,
    pub(crate) query: Vec<RequestValueField>,
    pub(crate) body: RequestBody,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) proxy_override: Option<ProxyMode>,
    pub(crate) transport: TransportSettings,
    pub(crate) inherits_workspace_transport: bool,
}

/// An exported environment: literal values and secret reference names only.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct NativeEnvironment {
    pub(crate) name: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) global: bool,
    pub(crate) variables: Vec<EnvironmentVariable>,
}

/// Encodes legacy collection v1 without resolving credential references or exporting response history.
///
/// Upload files are exported as path references and are never read.
///
/// # Errors
/// Returns an error for an invalid hierarchy.
pub fn export_legacy_v1_collection(
    collection: &Collection,
    requests: &[Request],
) -> Result<String, ExportError> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let nodes = collection_nodes(collection, requests, now.as_nanos(), false)?;
    encode_document(&collection.name, &nodes, None, now.as_secs_f64())
}

/// Exports every collection, root request and environment without resolving
/// secret references.
///
/// # Errors
/// Returns an error for an invalid hierarchy.
pub fn export_legacy_v1_workspace(snapshot: &WorkspaceSnapshot) -> Result<String, ExportError> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let mut seed = now.as_nanos();
    let mut nodes = Vec::new();
    for entry in &snapshot.collections {
        nodes.extend(collection_nodes(
            &entry.collection,
            &entry.requests,
            seed,
            entry.collection.id.as_str() == "workspace-root",
        )?);
        seed =
            seed.wrapping_add((entry.collection.groups.len() + entry.requests.len() + 1) as u128);
    }
    let environments: Vec<_> = snapshot
        .environments
        .iter()
        .map(|environment| NativeEnvironment {
            name: environment.name.clone(),
            global: environment.id.as_str() == GLOBAL_ENVIRONMENT_ID,
            variables: environment.variables.clone(),
        })
        .collect();
    encode_document(
        &snapshot.workspace.name,
        &nodes,
        Some(&environments),
        now.as_secs_f64(),
    )
}

/// Exports a single request without adding a synthetic folder.
///
/// # Errors
/// Returns an error if the request cannot be encoded.
pub fn export_legacy_v1_request(request: &Request) -> Result<String, ExportError> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let node = request_node(request, &node_id(now.as_nanos()), "")?;
    encode_document(&request.name, &[node], None, now.as_secs_f64())
}

fn encode_document(
    name: &str,
    nodes: &[Value],
    environments: Option<&[NativeEnvironment]>,
    time: f64,
) -> Result<String, ExportError> {
    let mut document = json!({"version":1,"appVersion":env!("CARGO_PKG_VERSION"),"workspaceName":name,
        "exportDate":time-978_307_200.0,"nodes":nodes,"historyItems":{},"historyFlows":{},"responseBodies":{}});
    if let Some(environments) = environments {
        document["wirebolt"] = json!({ "environments": environments });
    }
    serde_json::to_string_pretty(&document)
        .map_err(|_| ExportError("collection could not be encoded"))
}

fn node_id(value: u128) -> String {
    let digits = format!("{value:032x}");
    format!(
        "{}-{}-4{}-8{}-{}",
        &digits[..8],
        &digits[8..12],
        &digits[13..16],
        &digits[17..20],
        &digits[20..]
    )
}

fn collection_nodes(
    collection: &Collection,
    requests: &[Request],
    seed: u128,
    root: bool,
) -> Result<Vec<Value>, ExportError> {
    let make_id = |index: usize| node_id(seed.wrapping_add(index as u128));
    let root_id = make_id(0);
    let ids: BTreeMap<_, _> = collection
        .groups
        .iter()
        .enumerate()
        .map(|(i, g)| (g.id.clone(), make_id(i + 1)))
        .collect();
    let request_ids: Vec<_> = (0..requests.len())
        .map(|i| make_id(i + collection.groups.len() + 1))
        .collect();
    let group_path = |parent: Option<&DocumentId>| -> Result<String, ExportError> {
        let mut path = Vec::new();
        let mut current = parent;
        while let Some(id) = current {
            if path.len() >= collection.groups.len() {
                return Err(ExportError("collection hierarchy is cyclic"));
            }
            path.push(
                ids.get(id)
                    .ok_or(ExportError("collection folder is missing"))?
                    .as_str(),
            );
            current = collection
                .groups
                .iter()
                .find(|g| &g.id == id)
                .and_then(|g| g.parent_id.as_ref());
        }
        if !root {
            path.push(&root_id);
        }
        path.reverse();
        Ok(if path.is_empty() {
            String::new()
        } else {
            format!("/{}", path.join("/"))
        })
    };
    let children = |parent: Option<&DocumentId>| {
        let mut values: Vec<_> = collection
            .groups
            .iter()
            .filter(|g| g.parent_id.as_ref() == parent)
            .map(|g| (g.order, ids[&g.id].clone()))
            .chain(
                requests
                    .iter()
                    .zip(&request_ids)
                    .filter(|(r, _)| r.group_id.as_ref() == parent)
                    .map(|(r, id)| (r.order, id.clone())),
            )
            .collect();
        values.sort_by_key(|(order, _)| *order);
        values.into_iter().map(|(_, id)| id).collect::<Vec<_>>()
    };
    let folder = |id: &str, name: &str, path: String, children: Vec<String>| {
        json!({
            "uuid":id,"name":name,"kind":"folder","path":path,
            "folderMetadata":{"uuid":id,"folderName":name,"childIds":children},
            "historyManager":{"historyItems":[]},"responseDisplayMode":null
        })
    };
    let mut nodes = if root {
        Vec::new()
    } else {
        let mut collection_folder = folder(
            &root_id,
            &collection.name,
            format!("/{root_id}"),
            children(None),
        );
        // Tells Wirebolt to re-import this folder as a collection, not a folder.
        collection_folder["wirebolt"] = json!({"collection": true});
        vec![collection_folder]
    };
    for group in &collection.groups {
        nodes.push(folder(
            &ids[&group.id],
            &group.name,
            group_path(Some(&group.id))?,
            children(Some(&group.id)),
        ));
    }
    let mut ordered: Vec<_> = requests.iter().zip(request_ids).collect();
    ordered.sort_by_key(|(request, _)| request.order);
    for (request, id) in ordered {
        nodes.push(request_node(
            request,
            &id,
            &group_path(request.group_id.as_ref())?,
        )?);
    }
    Ok(nodes)
}

fn request_node(request: &Request, id: &str, parent_path: &str) -> Result<Value, ExportError> {
    let auth = match &request.authentication {
        RequestAuthentication::None => json!({"mode":"none"}),
        RequestAuthentication::Basic { username, password } => {
            json!({"mode":"basic","basic":{"username":source(username),"password":source(password)}})
        }
        RequestAuthentication::Bearer { token } => {
            json!({"mode":"bearer","bearer":{"token":source(token)}})
        }
        RequestAuthentication::ApiKey { .. } | RequestAuthentication::Oauth2 { .. } => {
            // Explicit extension: never pretend an advanced-auth request has no auth.
            // Wirebolt imports the typed metadata below without resolving secrets.
            json!({"mode":"wirebolt"})
        }
    };
    let method = if request.web_socket {
        json!({"custom":{"_0":"WS"}})
    } else if ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]
        .contains(&request.method.as_str())
    {
        json!({request.method.to_lowercase():{}})
    } else {
        json!({"custom":{"_0":request.method}})
    };
    let native = NativeRequest {
        authentication: request.authentication.clone(),
        request: Some(NativeDefinition {
            headers: request.headers.clone(),
            query: request.query.clone(),
            body: request.body.clone(),
            proxy_override: request.proxy_override.clone(),
            transport: request.transport.clone(),
            inherits_workspace_transport: request.inherits_workspace_transport,
        }),
    };
    Ok(json!({
        "uuid":id,"name":request.name,"note":request.note,"path":format!("{}/{id}.request",parent_path),
        "wirebolt":native,
        "kind":if request.web_socket {"websocketRequest"} else {"request"},
        "folderMetadata":null,"responseDisplayMode":null,"historyManager":{"historyItems":[]},
        "flow":{"type":{"request":{}},"error":null,"response":null,"events":[],"request":{
            "url":request.url,"method":method,"info":null,"auth":auth,
            "headers":request.headers.iter().map(|h| row(&h.name,&h.value,h.enabled)).collect::<Vec<_>>(),
            "queries":request.query.iter().map(|q| row(&q.name,&q.value,q.enabled)).collect::<Vec<_>>(),
            "body":{"contentType":body(&request.body)?}
        }}
    }))
}

/// Portable spelling of a value. Secret references become `{{name}}` so other
/// readers see a placeholder; the `wirebolt` block keeps the typed reference.
fn source(value: &ValueSource) -> String {
    match value {
        ValueSource::Literal(value) => value.clone(),
        ValueSource::Secret { secret } => format!("{{{{{}}}}}", secret.as_str()),
    }
}
fn row(name: &str, value: &ValueSource, enabled: bool) -> Value {
    json!({"key":name,"value":source(value),"isEnabled":enabled,"isBold":false,"isSubItem":false})
}
fn body(body: &RequestBody) -> Result<Value, ExportError> {
    Ok(match body {
        RequestBody::Empty => json!({"none":{}}),
        RequestBody::Json { value } => json!({"json":{"_0":value}}),
        RequestBody::Xml { value } => json!({"xml":{"_0":value}}),
        RequestBody::Html { value } => json!({"html":{"_0":value}}),
        RequestBody::Text {
            content_type,
            value,
        }
        | RequestBody::Raw {
            content_type,
            value,
        } => {
            let kind = match content_type.as_deref() {
                Some("application/octet-stream; encoding=hex") => "hex",
                Some("application/octet-stream") => "base64",
                _ => "text",
            };
            json!({kind:{"_0":value}})
        }
        RequestBody::File { path, .. } => json!({"fileURL":{"_0":path}}),
        RequestBody::FormUrlEncoded { fields } => {
            json!({"formURLEncoded":{"_0":fields.iter().map(|f| row(&f.name,&f.value,f.enabled)).collect::<Vec<_>>()}})
        }
        RequestBody::Multipart { parts } => {
            let parts: Result<Vec<_>,ExportError> = parts.iter().map(|part| {
                let bytes = match part.kind {
                    MultipartPartKind::Text => source(&part.value).into_bytes(),
                    MultipartPartKind::Binary => STANDARD.decode(source(&part.value)).map_err(|_| ExportError("multipart bytes are invalid"))?,
                    // Upload files stay on disk: only their path is exported, in the `wirebolt` block.
                    MultipartPartKind::File => Vec::new(),
                };
                let file_name = part.file_name.clone().or_else(|| {
                    part.file_path.as_deref().and_then(|path| Path::new(path).file_name()).map(|name| name.to_string_lossy().into_owned())
                });
                Ok(json!({"id":part.id,"part":part.name,"value":STANDARD.encode(bytes),"contentType":part.content_type.as_deref().unwrap_or(""),
                    "fileName":file_name.unwrap_or_default(),"isEnabled":part.enabled}))
            }).collect();
            json!({"multipartFormData":{"_0":{"parts":parts?}}})
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Group, ImportEngine, ImportFormat, MultipartPart, RequestValueField, SecretName};
    #[test]
    fn advanced_auth_exports_reimport_without_losing_secret_references() {
        let auths = [
            RequestAuthentication::ApiKey {
                placement: crate::ApiKeyPlacement::Header,
                name: "X-API-Key".into(),
                value: ValueSource::secret(SecretName::new("api.key").unwrap()),
            },
            RequestAuthentication::ApiKey {
                placement: crate::ApiKeyPlacement::Query,
                name: "key".into(),
                value: ValueSource::secret(SecretName::new("query.key").unwrap()),
            },
            RequestAuthentication::Oauth2 {
                configuration: crate::Oauth2Configuration {
                    grant: crate::Oauth2Grant::AuthorizationCodePkce,
                    authorization_url: "https://example.test/authorize".into(),
                    token_url: "https://example.test/token".into(),
                    client_id: "client".into(),
                    client_secret_reference: SecretName::new("oauth.client").unwrap(),
                    scopes: "read write".into(),
                    audience: "api".into(),
                    redirect_uri: "wirebolt://oauth/callback".into(),
                    access_token_reference: SecretName::new("oauth.token").unwrap(),
                },
            },
        ];
        for authentication in auths {
            let mut request = Request::new(
                DocumentId::new("request").unwrap(),
                "Auth",
                "GET",
                "https://example.test",
            );
            request.authentication = authentication.clone();
            let collection = Collection::new(DocumentId::new("api").unwrap(), "API".into());
            let snapshot = WorkspaceSnapshot {
                workspace: crate::Workspace::new("Auth"),
                collections: vec![crate::CollectionSnapshot {
                    collection: collection.clone(),
                    requests: vec![request.clone()],
                }],
                environments: vec![],
                problems: vec![],
            };
            for exported in [
                export_legacy_v1_request(&request),
                export_legacy_v1_collection(&collection, &[request]),
                export_legacy_v1_workspace(&snapshot),
            ] {
                let exported = exported.expect("advanced auth export");
                let imported =
                    ImportEngine::parse(ImportFormat::LegacyWorkspaceV1, &exported).unwrap();
                assert_eq!(imported.requests[0].authentication, authentication);
            }
        }
    }

    #[test]
    fn workspace_export_preserves_all_collections_and_unwrapped_root_requests() {
        let id = |value: &str| DocumentId::new(value).unwrap();
        let entry =
            |collection_id: &str, name: &str, request_name: &str| crate::CollectionSnapshot {
                collection: Collection::new(id(collection_id), name.into()),
                requests: vec![Request::new(
                    id("same-id"),
                    request_name,
                    "GET",
                    "http://localhost/",
                )],
            };
        let snapshot = WorkspaceSnapshot {
            workspace: crate::Workspace::new("All requests"),
            collections: vec![
                entry("workspace-root", "Requests", "Root request"),
                entry("first", "First", "A"),
                entry("second", "Second", "B"),
            ],
            environments: vec![],
            problems: vec![],
        };
        let exported = export_legacy_v1_workspace(&snapshot).unwrap();
        let document: Value = serde_json::from_str(&exported).unwrap();
        let nodes = document["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 5);
        let unique: std::collections::BTreeSet<_> =
            nodes.iter().map(|n| n["uuid"].as_str().unwrap()).collect();
        assert_eq!(unique.len(), nodes.len());
        assert!(!nodes.iter().any(|n| n["name"] == "Requests"));
        let root = nodes.iter().find(|n| n["name"] == "Root request").unwrap();
        assert_eq!(root["path"].as_str().unwrap().matches('/').count(), 1);
        for name in ["A", "B"] {
            let request = nodes.iter().find(|n| n["name"] == name).unwrap();
            assert_eq!(request["path"].as_str().unwrap().matches('/').count(), 2);
        }
        assert_eq!(document["workspaceName"], "All requests");
        assert_eq!(document["historyItems"], json!({}));
        let imported = ImportEngine::parse_file(
            ImportFormat::LegacyWorkspaceV1,
            &exported,
            "Export filename",
        )
        .unwrap();
        assert_eq!(imported.name, "Export filename");
        assert_eq!(imported.groups.len(), 2);
        assert_eq!(imported.requests.len(), 3);
        assert!(
            imported
                .requests
                .iter()
                .find(|r| r.name == "Root request")
                .unwrap()
                .group_source_id
                .is_none()
        );
        assert!(
            imported
                .requests
                .iter()
                .filter(|r| r.name != "Root request")
                .all(|r| r.group_source_id.is_some())
        );
        let single: Value = serde_json::from_str(
            &export_legacy_v1_request(&snapshot.collections[1].requests[0]).unwrap(),
        )
        .unwrap();
        assert_eq!(single["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(single["nodes"][0]["kind"], "request");
    }

    #[test]
    fn reimports_hierarchy_unicode_bytes_and_credential_references() {
        let mut collection = Collection::new(DocumentId::new("api").unwrap(), "API café".into());
        collection.groups.push(Group::new(
            DocumentId::new("nested").unwrap(),
            "東京".into(),
            None,
            0,
        ));
        let mut request = Request::new(
            DocumentId::new("upload").unwrap(),
            "Upload",
            "POST",
            "http://localhost/echo",
        );
        request.group_id = Some(collection.groups[0].id.clone());
        request.note = "Unicode 🚀".into();
        request
            .query
            .push(RequestValueField::enabled("q", ValueSource::literal("a&b")));
        request.authentication = RequestAuthentication::Bearer {
            token: ValueSource::secret(SecretName::new("api.token").unwrap()),
        };
        request.body = RequestBody::Multipart {
            parts: vec![MultipartPart {
                id: "bytes".into(),
                name: "file".into(),
                kind: MultipartPartKind::Binary,
                value: ValueSource::literal("AAH/"),
                file_path: None,
                file_name: Some("upload.bin".into()),
                content_type: Some("application/octet-stream".into()),
                enabled: true,
            }],
        };
        let exported = export_legacy_v1_collection(&collection, &[request]).unwrap();
        let imported = ImportEngine::parse(ImportFormat::LegacyWorkspaceV1, &exported).unwrap();
        assert_eq!(imported.name, "API café");
        assert_eq!(imported.groups[0].name, "東京");
        assert_eq!(
            imported.requests[0].group_source_id,
            Some(imported.groups[0].source_id.clone())
        );
        assert_eq!(imported.requests[0].note, "Unicode 🚀");
        assert_eq!(
            imported.requests[0].query[0].value,
            ValueSource::literal("a&b")
        );
        assert_eq!(
            imported.requests[0].authentication,
            RequestAuthentication::Bearer {
                token: ValueSource::secret(SecretName::new("api.token").unwrap())
            }
        );
        let RequestBody::Multipart { parts } = &imported.requests[0].body else {
            panic!("multipart body")
        };
        assert_eq!(parts[0].kind, MultipartPartKind::Binary);
        assert_eq!(parts[0].value, ValueSource::literal("AAH/"));
    }
}
