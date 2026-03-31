use std::path::{Path, PathBuf};

use deno_core::{
    FsModuleLoader, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader,
    ModuleSpecifier, ResolutionKind, resolve_import,
};
use deno_error::JsErrorBox;

#[derive(Debug, Clone)]
pub struct SandboxedModuleLoader {
    allowed_root: PathBuf,
}

impl SandboxedModuleLoader {
    pub fn new(allowed_root: PathBuf) -> Self {
        Self { allowed_root }
    }

    fn ensure_allowed_specifier(&self, specifier: &ModuleSpecifier) -> Result<(), JsErrorBox> {
        if specifier.scheme() != "file" {
            return Err(JsErrorBox::generic(format!(
                "runtime bundle imports must stay within the bundle root, unsupported scheme: {}",
                specifier.scheme()
            )));
        }

        let path = specifier.to_file_path().map_err(|_| {
            JsErrorBox::generic(format!("invalid file module specifier: {specifier}"))
        })?;
        let candidate = canonicalize_for_sandbox(&path).map_err(|error| {
            JsErrorBox::generic(format!(
                "failed to resolve runtime bundle import {}: {error}",
                path.display()
            ))
        })?;
        if !candidate.starts_with(&self.allowed_root) {
            return Err(JsErrorBox::generic(format!(
                "runtime bundle import is outside the bundle root: {}",
                candidate.display()
            )));
        }
        Ok(())
    }
}

impl ModuleLoader for SandboxedModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        let resolved = resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)?;
        match kind {
            ResolutionKind::MainModule | ResolutionKind::Import | ResolutionKind::DynamicImport => {
                self.ensure_allowed_specifier(&resolved)?
            }
        }
        Ok(resolved)
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        maybe_referrer: Option<&ModuleLoadReferrer>,
        options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        if let Err(error) = self.ensure_allowed_specifier(module_specifier) {
            return ModuleLoadResponse::Sync(Err(error));
        }
        FsModuleLoader.load(module_specifier, maybe_referrer, options)
    }
}

fn canonicalize_for_sandbox(path: &Path) -> std::io::Result<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "module path does not have a parent directory",
                )
            })?;
            let parent = parent.canonicalize()?;
            let file_name = path.file_name().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "module path does not have a file name",
                )
            })?;
            Ok(parent.join(file_name))
        }
        Err(error) => Err(error),
    }
}
