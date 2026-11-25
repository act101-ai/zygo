//! The conformance input model (K§6.2.1, K§6.2.3) and check items 1-9 (K§6.2.5).

use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub capability: Vec<Capability>,
    #[serde(default)]
    pub bar_exception: Vec<BarException>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub id: String,
    pub name: String,
    pub tier: Tier,
    pub standard: String,
    pub criterion: Vec<Criterion>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Core,
    Standard,
    Extra,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarException {
    pub client: String,
    pub criterion: String,
    pub reason: String,
    pub approved: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradeFile {
    pub client: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub cell: Vec<CellUrl>,
    #[serde(default)]
    pub grade: Vec<GradeEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellUrl {
    pub capability: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradeEntry {
    pub criterion: String,
    pub status: Status,
    #[serde(default)]
    pub evidence: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Status {
    #[serde(rename = "met")]
    Met,
    #[serde(rename = "not-met")]
    NotMet,
    #[serde(rename = "n/a")]
    Na,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Client {
    pub id: String,
    pub name: String,
    pub version: String,
    pub supported: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fault {
    pub item: u8,
    pub text: String,
}

impl Registry {
    pub fn load(path: &Path) -> Result<Registry, String> {
        todo!()
    }

    pub fn parse(text: &str) -> Result<Registry, String> {
        todo!()
    }
}

impl GradeFile {
    pub fn load(path: &Path) -> Result<GradeFile, String> {
        todo!()
    }

    pub fn parse(text: &str) -> Result<GradeFile, String> {
        todo!()
    }
}

fn parse_evidence(e: &str) -> Result<(), String> {
    todo!()
}

pub fn evidence(root: &Path, e: &str) -> Result<Evidence, String> {
    todo!()
}

pub fn check(root: &Path, reg: &Registry, grades: &[GradeFile], clients: &[Client]) -> Vec<Fault> {
    todo!()
}

/// CF-2 replaces this with derive::bar(reg, g).failing
fn failing_core(reg: &Registry, g: &GradeFile) -> Vec<String> {
    todo!()
}

pub fn clients(root: &Path) -> Result<Vec<Client>, String> {
    todo!()
}
