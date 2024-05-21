use std::sync::Arc;

use crate::{ModuleType, PackageJson, ResolvedPath};

#[derive(Debug)]
pub struct ResolvedRequestInfo {
  pub path: ResolvedPath,
  pub module_type: ModuleType,
  pub is_external: bool,
  pub package_json: Option<Arc<PackageJson>>,
}
