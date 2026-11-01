//! File ingress and recent-workspace policy. Native panels supply paths and locale lists.
use std::path::Path;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceImportFile {
    Bruno,
    Document {
        source: String,
        format: wirebolt_core::ImportFormat,
    },
    Failure {
        message: String,
    },
}
#[must_use]
pub fn display_name(format: wirebolt_core::ImportFormat) -> String {
    match format {
        wirebolt_core::ImportFormat::Curl => "cURL",
        wirebolt_core::ImportFormat::Har => "HAR",
        wirebolt_core::ImportFormat::LegacyWorkspaceV1 => "Wirebolt JSON",
        wirebolt_core::ImportFormat::PostmanV2 => "Postman Collection v2",
        wirebolt_core::ImportFormat::Insomnia => "Insomnia",
        wirebolt_core::ImportFormat::Bruno => "Bruno collection",
        wirebolt_core::ImportFormat::BrunoFolder => "Bruno collection folder",
    }
    .into()
}
#[must_use]
pub fn unreadable(name: &str) -> String {
    format!(
        "“{name}” couldn’t be read. Check that it’s a UTF-8 text file you have permission to open."
    )
}
#[must_use]
pub fn unrecognized(name: &str, contents: &str, formats: &str) -> String {
    let head: String = contents.chars().take(4096).collect();
    if head.contains("\"openapi\"")
        || head.contains("\"swagger\"")
        || head
            .lines()
            .take(40)
            .any(|l| l.starts_with("openapi:") || l.starts_with("swagger:"))
    {
        return format!(
            "“{name}” is an OpenAPI or Swagger document. OpenAPI import isn’t supported yet."
        );
    }
    format!("“{name}” isn’t a format Wirebolt can import. Supported formats: {formats}.")
}
#[must_use]
pub fn read_import(
    path: &Path,
    preferred: Option<wirebolt_core::ImportFormat>,
    formats: &str,
) -> WorkspaceImportFile {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let extension = path.extension().unwrap_or_default().to_string_lossy();
    if path.is_dir() || extension.eq_ignore_ascii_case("bru") {
        return if path.is_dir() && !crate::domain::imports::is_bruno_collection(path) {
            WorkspaceImportFile::Failure {
                message: format!(
                    "“{name}” isn’t a Bruno collection folder. Choose the folder that contains bruno.json."
                ),
            }
        } else {
            WorkspaceImportFile::Bruno
        };
    }
    let Ok(source) = std::fs::read_to_string(path) else {
        return WorkspaceImportFile::Failure {
            message: unreadable(&name),
        };
    };
    let Some(format) = crate::domain::imports::detect(&extension, &source).or(preferred) else {
        return WorkspaceImportFile::Failure {
            message: unrecognized(&name, &source, formats),
        };
    };
    WorkspaceImportFile::Document { source, format }
}
#[must_use]
pub fn recent(path: &str, paths: Vec<String>) -> Vec<String> {
    std::iter::once(path.to_owned())
        .chain(
            paths
                .into_iter()
                .filter(|p| p != path && Path::new(p).exists()),
        )
        .take(10)
        .collect()
}
#[must_use]
pub fn default_name(path: &Path, built_in: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let name = name.trim();
    if path == built_in || name.is_empty() || name == "/" {
        "Wirebolt".into()
    } else {
        name.into()
    }
}
#[must_use]
pub fn read_document(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceSelectionProblem {
    NotFound,
    AlreadyExists,
}
#[must_use]
pub fn selection_problem(path: &Path, mode: &str) -> Option<WorkspaceSelectionProblem> {
    let exists = path.join("wirebolt.toml").exists();
    match (mode, exists) {
        ("open", false) => Some(WorkspaceSelectionProblem::NotFound),
        ("create", true) => Some(WorkspaceSelectionProblem::AlreadyExists),
        _ => None,
    }
}
