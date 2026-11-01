//! Batched editor syntax bridge; token and indentation policy lives in Rust.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use wirebolt_app::{
    domain::syntax::{
        self, SyntaxChunk, SyntaxIndent, SyntaxLanguage, SyntaxToken, SyntaxTokenKind,
    },
    response::ResponseTextRange,
};
#[uniffi::remote(Enum)]
pub enum SyntaxLanguage {
    Json,
    Xml,
    Html,
    Http,
    Plain,
}
#[uniffi::remote(Enum)]
pub enum SyntaxTokenKind {
    Keyword,
    Number,
    String,
    Key,
    Url,
}
#[uniffi::remote(Record)]
pub struct SyntaxToken {
    pub range: ResponseTextRange,
    pub kind: SyntaxTokenKind,
}
#[uniffi::remote(Record)]
pub struct SyntaxIndent {
    pub range: ResponseTextRange,
    pub columns: u64,
}
#[uniffi::remote(Record)]
pub struct SyntaxChunk {
    pub range: ResponseTextRange,
    pub tokens: Vec<SyntaxToken>,
    pub indents: Vec<SyntaxIndent>,
}
#[uniffi::export]
pub fn app_syntax_highlight(source: String, language: SyntaxLanguage) -> Vec<SyntaxChunk> {
    syntax::highlight(&source, language)
}
