use crate::{read_module_interface_artifact, ModuleId, ModuleInterfaceArtifact, ModuleMetadata};
use karte_parser::{ImportDecl, ImportSpecifier};
use std::collections::{HashMap, HashSet};

/// Ensures every import references a declared dependency and only requests
/// exported symbols from that dependency.
pub fn validate_module_imports(
    module_id: &ModuleId,
    imports: &[ImportDecl],
    meta: &ModuleMetadata,
    dependency_interfaces: &HashMap<ModuleId, ModuleInterfaceArtifact>,
) -> Result<(), String> {
    if imports.is_empty() {
        return Ok(());
    }

    let declared: HashSet<&str> = meta.dependencies.iter().map(|dep| dep.as_str()).collect();

    for import in imports {
        let target_id = ModuleId::new(join_module_path(&import.path));
        if target_id.as_str().is_empty() {
            continue;
        }

        if !declared.contains(target_id.as_str()) {
            return Err(format!(
                "模块 `{}` 导入 `{}` (span {:?}) 但 manifest 未声明该依赖",
                module_id.as_str(),
                target_id.as_str(),
                import.span
            ));
        }

        if let ImportSpecifier::Symbols(symbols) = &import.specifier {
            if symbols.is_empty() {
                continue;
            }
            let interface = dependency_interfaces
                .get(&target_id)
                .cloned()
                .or_else(|| read_module_interface_artifact(&target_id).ok())
                .ok_or_else(|| {
                    format!(
                        "模块 `{}` 无法加载 `{}` 的接口工件以验证导入",
                        module_id.as_str(),
                        target_id.as_str()
                    )
                })?;

            for symbol in symbols {
                if !interface_exports_symbol(&interface, &symbol.name) {
                    return Err(format!(
                        "模块 `{}` 导入 `{}`::`{}` (span {:?}) 但依赖未导出该符号",
                        module_id.as_str(),
                        target_id.as_str(),
                        symbol.name,
                        symbol.span
                    ));
                }
            }
        }
    }

    Ok(())
}

fn join_module_path(path: &[String]) -> String {
    if path.is_empty() {
        return String::new();
    }
    path.join(".")
}

fn interface_exports_symbol(artifact: &ModuleInterfaceArtifact, name: &str) -> bool {
    artifact.exports.functions.iter().any(|f| f.name == name)
        || artifact.exports.structs.iter().any(|s| s.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FunctionExport, ModuleExports};
    use karte_diagnostics::Span;
    use karte_parser::{ImportSpecifier, ImportSymbol};

    #[test]
    fn validate_imports_accepts_declared_symbols() {
        let module_id = ModuleId::new("main");
        let meta = metadata_with_deps(&["utils"]);
        let imports = vec![make_import(&["utils"], &["add"])];
        let mut interfaces = HashMap::new();
        interfaces.insert(
            ModuleId::new("utils"),
            interface_with_functions("utils", &["add"]),
        );

        let result = validate_module_imports(&module_id, &imports, &meta, &interfaces);
        assert!(result.is_ok(), "expected validation to pass: {:?}", result);
    }

    #[test]
    fn validate_imports_rejects_missing_dependency() {
        let module_id = ModuleId::new("main");
        let meta = metadata_with_deps(&[]);
        let imports = vec![make_import(&["utils"], &["add"])];
        let interfaces = HashMap::new();

        let err = validate_module_imports(&module_id, &imports, &meta, &interfaces)
            .expect_err("expected missing dependency to fail");
        assert!(err.contains("manifest"));
    }

    #[test]
    fn validate_imports_rejects_missing_symbol() {
        let module_id = ModuleId::new("main");
        let meta = metadata_with_deps(&["utils"]);
        let imports = vec![make_import(&["utils"], &["add"])];
        let mut interfaces = HashMap::new();
        interfaces.insert(
            ModuleId::new("utils"),
            interface_with_functions("utils", &["sub"]),
        );

        let err = validate_module_imports(&module_id, &imports, &meta, &interfaces)
            .expect_err("expected missing symbol to fail");
        assert!(err.contains("未导出"));
    }

    fn make_import(path: &[&str], symbols: &[&str]) -> ImportDecl {
        let span = Span::new(0, 0);
        let path_vec = path.iter().map(|segment| segment.to_string()).collect();
        let specifier = if symbols.is_empty() {
            ImportSpecifier::EntireModule
        } else {
            ImportSpecifier::Symbols(
                symbols
                    .iter()
                    .map(|name| ImportSymbol {
                        name: (*name).to_string(),
                        alias: None,
                        span,
                    })
                    .collect(),
            )
        };

        ImportDecl {
            path: path_vec,
            alias: None,
            specifier,
            span,
        }
    }

    fn metadata_with_deps(ids: &[&str]) -> ModuleMetadata {
        ModuleMetadata {
            module_name: "main".into(),
            sources: Vec::new(),
            dependencies: ids.iter().map(|id| ModuleId::new(*id)).collect(),
            source_fingerprint: 0,
            manifest_hash: 0,
        }
    }

    fn interface_with_functions(module: &str, functions: &[&str]) -> ModuleInterfaceArtifact {
        ModuleInterfaceArtifact {
            module: module.to_string(),
            manifest_hash: 0,
            source_fingerprint: 0,
            interface_hash: "deadbeef".into(),
            dependencies: Vec::new(),
            exports: ModuleExports {
                functions: functions
                    .iter()
                    .map(|name| FunctionExport {
                        name: (*name).to_string(),
                        params: 0,
                    })
                    .collect(),
                structs: Vec::new(),
                mains: Vec::new(),
            },
        }
    }
}
