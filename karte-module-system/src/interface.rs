use crate::{ModuleId, ModuleMetadata};
use karte_mir::MirProgram;
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::DefaultHasher, BTreeSet, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FunctionExport {
    pub name: String,
    pub params: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StructFieldExport {
    pub name: String,
    pub ty: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StructExport {
    pub name: String,
    pub fields: Vec<StructFieldExport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleExports {
    pub functions: Vec<FunctionExport>,
    pub structs: Vec<StructExport>,
    pub mains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyInterfaceExport {
    pub id: String,
    pub interface_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInterfaceArtifact {
    pub module: String,
    pub manifest_hash: u64,
    pub source_fingerprint: u64,
    pub interface_hash: String,
    pub dependencies: Vec<DependencyInterfaceExport>,
    pub exports: ModuleExports,
}

pub struct ModuleInterfaceSummary {
    pub interface_hash: u64,
    pub artifact: ModuleInterfaceArtifact,
}

#[derive(Default)]
pub struct ModuleInterfaceAccumulator {
    functions: BTreeSet<FunctionExport>,
    structs: BTreeSet<StructExport>,
    mains: BTreeSet<String>,
}

impl ModuleInterfaceAccumulator {
    pub fn observe(&mut self, program: &MirProgram) {
        for (name, function) in &program.functions {
            self.functions.insert(FunctionExport {
                name: name.clone(),
                params: function.params.len(),
            });
        }
        for (name, ty) in &program.struct_types {
            let mut fields = ty
                .fields
                .iter()
                .map(|field| StructFieldExport {
                    name: field.name.clone(),
                    ty: field.field_type.clone(),
                })
                .collect::<Vec<_>>();
            fields.sort_by(|a, b| a.name.cmp(&b.name));
            self.structs.insert(StructExport {
                name: name.clone(),
                fields,
            });
        }
        if let Some(main) = &program.main_function {
            self.mains.insert(main.clone());
        }
    }

    pub fn finalize(
        self,
        module_id: &ModuleId,
        meta: &ModuleMetadata,
        dependency_interfaces: &HashMap<ModuleId, u64>,
    ) -> ModuleInterfaceSummary {
        let functions: Vec<_> = self.functions.into_iter().collect();
        let structs: Vec<_> = self.structs.into_iter().collect();
        let mains: Vec<_> = self.mains.into_iter().collect();

        let mut hasher = DefaultHasher::new();
        module_id.as_str().hash(&mut hasher);
        for sig in &functions {
            sig.hash(&mut hasher);
        }
        for sig in &structs {
            sig.hash(&mut hasher);
        }
        for main in &mains {
            main.hash(&mut hasher);
        }

        let mut dependency_list: Vec<_> = meta.dependencies.iter().cloned().collect();
        dependency_list.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for dep in &dependency_list {
            if let Some(dep_hash) = dependency_interfaces.get(dep) {
                dep_hash.hash(&mut hasher);
            }
        }

        let interface_hash = hasher.finish();
        let dependencies = dependency_list
            .into_iter()
            .map(|dep| DependencyInterfaceExport {
                id: dep.as_str().to_string(),
                interface_hash: dependency_interfaces
                    .get(&dep)
                    .map(|hash| format!("{:016x}", hash)),
            })
            .collect();

        let artifact = ModuleInterfaceArtifact {
            module: meta.module_name.clone(),
            manifest_hash: meta.manifest_hash,
            source_fingerprint: meta.source_fingerprint,
            interface_hash: format!("{:016x}", interface_hash),
            dependencies,
            exports: ModuleExports {
                functions,
                structs,
                mains,
            },
        };

        ModuleInterfaceSummary {
            interface_hash,
            artifact,
        }
    }
}

/// 将 module_id 转换为安全的文件名部分（不包含路径分隔符）
/// 对于包含路径分隔符的 module_id（如绝对路径），使用哈希值
pub fn sanitize_module_id_for_filename(module_id: &str) -> String {
    if module_id.contains('/') || module_id.contains('\\') {
        let mut hasher = DefaultHasher::new();
        module_id.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    } else {
        module_id.to_string()
    }
}

pub fn compute_module_cache_version(
    module_id: &ModuleId,
    meta: &ModuleMetadata,
    dependency_interfaces: &HashMap<ModuleId, u64>,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    module_id.as_str().hash(&mut hasher);
    meta.source_fingerprint.hash(&mut hasher);
    meta.manifest_hash.hash(&mut hasher);
    let mut deps = meta.dependencies.clone();
    deps.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    for dep in deps {
        if let Some(dep_hash) = dependency_interfaces.get(&dep) {
            dep_hash.hash(&mut hasher);
        }
    }
    hasher.finish()
}

pub fn write_module_interface_artifact(
    module_id: &ModuleId,
    artifact: &ModuleInterfaceArtifact,
) -> Result<(), String> {
    let cache_dir = Path::new("target").join(".karte-cache");
    fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("创建接口目录失败 {}: {}", cache_dir.display(), e))?;

    let safe_module_id = sanitize_module_id_for_filename(module_id.as_str());
    let filename = format!("{}.interface.json", safe_module_id);
    let file_path = cache_dir.join(&filename);
    let temp_path = cache_dir.join(format!("{}.tmp", filename));
    let payload = serde_json::to_vec_pretty(artifact)
        .map_err(|e| format!("序列化模块接口失败 {}: {}", module_id, e))?;

    fs::write(&temp_path, &payload)
        .map_err(|e| format!("写入接口临时文件失败 {}: {}", temp_path.display(), e))?;
    fs::rename(&temp_path, &file_path)
        .map_err(|e| format!("提交接口文件失败 {}: {}", file_path.display(), e))?;
    Ok(())
}

pub fn read_module_interface_artifact(
    module_id: &ModuleId,
) -> Result<ModuleInterfaceArtifact, String> {
    let cache_dir = Path::new("target").join(".karte-cache");

    let safe_module_id = sanitize_module_id_for_filename(module_id.as_str());
    let filename = format!("{}.interface.json", safe_module_id);
    let file_path = cache_dir.join(&filename);
    let payload = fs::read(&file_path)
        .map_err(|e| format!("读取接口文件失败 {}: {}", file_path.display(), e))?;
    serde_json::from_slice(&payload)
        .map_err(|e| format!("解析接口文件失败 {}: {}", file_path.display(), e))
}
