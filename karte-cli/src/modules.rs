use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

const MANIFEST_NAME: &str = "karte.mod.toml";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModuleId(String);

impl ModuleId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct ModuleMetadata {
    pub sources: Vec<PathBuf>,
    pub dependencies: Vec<ModuleId>,
    pub source_fingerprint: u64,
}

#[derive(Debug, Clone)]
pub struct ModulePlan {
    pub entry: ModuleId,
    pub sequence: Vec<ModuleId>,
}

#[derive(Debug)]
pub enum ModuleError {
    Io(PathBuf, io::Error),
    Parse(PathBuf, toml::de::Error),
    MissingModule(String),
    MissingEntry(PathBuf),
    CycleDetected(Vec<ModuleId>),
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuleError::Io(path, err) => write!(f, "读取文件 {} 失败: {}", path.display(), err),
            ModuleError::Parse(path, err) => {
                write!(f, "解析 manifest {} 失败: {}", path.display(), err)
            }
            ModuleError::MissingModule(id) => write!(f, "依赖的模块 `{}` 未在 manifest 中定义", id),
            ModuleError::MissingEntry(path) => {
                write!(f, "无法在 manifest 中找到入口文件 {}", path.display())
            }
            ModuleError::CycleDetected(nodes) => {
                let cycle = nodes
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(f, "模块依赖存在循环: {}", cycle)
            }
        }
    }
}

impl std::error::Error for ModuleError {}

#[derive(Debug)]
pub struct ModuleGraph {
    nodes: HashMap<ModuleId, ModuleMetadata>,
    order: Vec<ModuleId>,
    manifest: Option<PathBuf>,
    source_to_module: HashMap<PathBuf, ModuleId>,
}

impl ModuleGraph {
    pub fn load_for_entry(entry: &Path) -> Result<Self, ModuleError> {
        if let Some(manifest) = find_manifest(entry) {
            ModuleGraph::from_manifest(&manifest)
        } else {
            ModuleGraph::single_file(entry)
        }
    }

    pub fn plan_for_entry(&self, entry_path: &Path) -> Result<ModulePlan, ModuleError> {
        let canonical_entry = canonicalize(entry_path)?;
        let entry_id = self
            .source_to_module
            .get(&canonical_entry)
            .cloned()
            .ok_or_else(|| ModuleError::MissingEntry(canonical_entry.clone()))?;

        let mut reachable = HashSet::new();
        self.collect_dependencies(&entry_id, &mut reachable)?;

        let sequence = self
            .order
            .iter()
            .filter(|id| reachable.contains(*id))
            .cloned()
            .collect();

        Ok(ModulePlan {
            entry: entry_id,
            sequence,
        })
    }

    pub fn metadata(&self, id: &ModuleId) -> Option<&ModuleMetadata> {
        self.nodes.get(id)
    }

    pub fn manifest_path(&self) -> Option<&Path> {
        self.manifest.as_deref()
    }

    pub fn describe(&self) -> Vec<String> {
        self.order
            .iter()
            .map(|id| {
                let meta = self.metadata(id).expect("module metadata missing");
                let dep_list = if meta.dependencies.is_empty() {
                    "∅".to_string()
                } else {
                    meta.dependencies
                        .iter()
                        .map(|dep| dep.as_str().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let files = meta
                    .sources
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{} [{} files: {}] -> deps: {}, fingerprint: {:016x}",
                    id.as_str(),
                    meta.sources.len(),
                    files,
                    dep_list,
                    meta.source_fingerprint
                )
            })
            .collect()
    }

    fn collect_dependencies(
        &self,
        id: &ModuleId,
        visited: &mut HashSet<ModuleId>,
    ) -> Result<(), ModuleError> {
        if !visited.insert(id.clone()) {
            return Ok(());
        }
        let meta = self
            .metadata(id)
            .ok_or_else(|| ModuleError::MissingModule(id.to_string()))?;
        for dep in &meta.dependencies {
            self.collect_dependencies(dep, visited)?;
        }
        Ok(())
    }

    fn from_manifest(manifest_path: &Path) -> Result<Self, ModuleError> {
        let manifest_text = fs::read_to_string(manifest_path)
            .map_err(|e| ModuleError::Io(manifest_path.to_path_buf(), e))?;
        let raw: RawManifest = toml::from_str(&manifest_text)
            .map_err(|e| ModuleError::Parse(manifest_path.to_path_buf(), e))?;

        let manifest_dir = manifest_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let mut nodes = HashMap::new();
        let mut source_map = HashMap::new();

        for module in raw.modules {
            let id = ModuleId::new(module.id);
            let mut declared_sources: Vec<PathBuf> = Vec::new();
            for source in module.sources {
                declared_sources.push(manifest_dir.join(source));
            }
            if let Some(single) = module.path {
                declared_sources.push(manifest_dir.join(single));
            }
            if let Some(dir) = module.dir {
                let dir_path = manifest_dir.join(dir);
                let collected = collect_directory_sources(&dir_path)?;
                declared_sources.extend(collected);
            }
            if declared_sources.is_empty() {
                return Err(ModuleError::Io(
                    manifest_path.to_path_buf(),
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("模块 `{}` 未声明任何 source", id.as_str()),
                    ),
                ));
            }

            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            let mut resolved_sources = Vec::new();
            for src in declared_sources {
                let resolved = canonicalize(&src)?;
                let source_text = fs::read_to_string(&resolved)
                    .map_err(|e| ModuleError::Io(resolved.clone(), e))?;
                resolved.to_string_lossy().hash(&mut hasher);
                source_text.hash(&mut hasher);
                source_map.insert(resolved.clone(), id.clone());
                resolved_sources.push(resolved);
            }

            let fingerprint = hasher.finish();
            let dependencies = module.deps.into_iter().map(ModuleId::new).collect();
            nodes.insert(
                id.clone(),
                ModuleMetadata {
                    sources: resolved_sources,
                    dependencies,
                    source_fingerprint: fingerprint,
                },
            );
        }

        let order = topological_sort(&nodes)?;
        Ok(Self {
            nodes,
            order,
            manifest: Some(manifest_path.to_path_buf()),
            source_to_module: source_map,
        })
    }

    fn single_file(path: &Path) -> Result<Self, ModuleError> {
        let resolved_path = canonicalize(path)?;
        let source_text = fs::read_to_string(&resolved_path)
            .map_err(|e| ModuleError::Io(resolved_path.clone(), e))?;
        let fingerprint = fingerprint(&resolved_path, &source_text);
        let module_id = ModuleId::new(resolved_path.to_string_lossy().to_string());
        let metadata = ModuleMetadata {
            sources: vec![resolved_path.clone()],
            dependencies: Vec::new(),
            source_fingerprint: fingerprint,
        };

        let mut nodes = HashMap::new();
        nodes.insert(module_id.clone(), metadata);

        let mut source_map = HashMap::new();
        source_map.insert(resolved_path, module_id.clone());

        Ok(Self {
            nodes,
            order: vec![module_id],
            manifest: None,
            source_to_module: source_map,
        })
    }

    /// 计算模块的分层调度计划
    /// 返回一个分层的模块列表，每一层中的模块可以并行编译
    pub fn schedule_layers(&self, plan: &ModulePlan) -> Vec<Vec<ModuleId>> {
        let mut layers = Vec::new();
        let mut visited = HashSet::new();
        let mut remaining: HashSet<ModuleId> = plan.sequence.iter().cloned().collect();

        while !remaining.is_empty() {
            let mut current_layer = Vec::new();

            // 找出所有依赖已满足的模块
            for module_id in &remaining {
                let metadata = self.nodes.get(module_id).unwrap();
                let deps_satisfied = metadata.dependencies.iter().all(|dep| {
                    // 依赖必须在之前的层中已访问，或者不在本次计划中（可能是外部依赖，暂不考虑）
                    // 这里假设plan包含了所有传递依赖
                    visited.contains(dep) || !plan.sequence.contains(dep)
                });

                if deps_satisfied {
                    current_layer.push(module_id.clone());
                }
            }

            if current_layer.is_empty() {
                // 如果还有剩余模块但找不到可执行的，说明有循环依赖（理论上在构建图时已检查）
                // 或者依赖不在plan中。为了避免死循环，将剩余所有模块作为一层（虽然可能失败）
                log::warn!("Detected potential cycle or missing dependencies in schedule_layers");
                layers.push(remaining.into_iter().collect());
                break;
            }

            // 将当前层加入结果，并标记为已访问
            // 排序以保证确定性
            current_layer.sort_by(|a, b| a.0.cmp(&b.0));

            for module_id in &current_layer {
                remaining.remove(module_id);
                visited.insert(module_id.clone());
            }

            layers.push(current_layer);
        }

        layers
    }
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    #[serde(default)]
    modules: Vec<RawModule>,
}

#[derive(Debug, Deserialize)]
struct RawModule {
    id: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    deps: Vec<String>,
}

fn find_manifest(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent().map(Path::to_path_buf)?
    };

    loop {
        let candidate = current.join(MANIFEST_NAME);
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

fn canonicalize(path: &Path) -> Result<PathBuf, ModuleError> {
    fs::canonicalize(path).map_err(|e| ModuleError::Io(path.to_path_buf(), e))
}

fn fingerprint(path: &Path, source: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    source.hash(&mut hasher);
    hasher.finish()
}

fn collect_directory_sources(dir: &Path) -> Result<Vec<PathBuf>, ModuleError> {
    if !dir.exists() {
        return Err(ModuleError::Io(
            dir.to_path_buf(),
            io::Error::new(io::ErrorKind::NotFound, "目录不存在"),
        ));
    }
    let mut files = Vec::new();
    collect_directory_sources_recursive(dir, &mut files)?;
    Ok(files)
}

fn collect_directory_sources_recursive(
    dir: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), ModuleError> {
    for entry in fs::read_dir(dir).map_err(|e| ModuleError::Io(dir.to_path_buf(), e))? {
        let entry = entry.map_err(|e| ModuleError::Io(dir.to_path_buf(), e))?;
        let path = entry.path();
        if path.is_dir() {
            collect_directory_sources_recursive(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("karte") {
            files.push(path);
        }
    }
    Ok(())
}

fn topological_sort(
    nodes: &HashMap<ModuleId, ModuleMetadata>,
) -> Result<Vec<ModuleId>, ModuleError> {
    let mut indegree = HashMap::new();
    let mut dependents: HashMap<ModuleId, Vec<ModuleId>> = HashMap::new();

    for id in nodes.keys() {
        indegree.insert(id.clone(), 0usize);
        dependents.entry(id.clone()).or_default();
    }

    for (id, meta) in nodes.iter() {
        if let Some(entry) = indegree.get_mut(id) {
            *entry = meta.dependencies.len();
        }
        for dep in &meta.dependencies {
            if !nodes.contains_key(dep) {
                return Err(ModuleError::MissingModule(dep.to_string()));
            }
            dependents.entry(dep.clone()).or_default().push(id.clone());
        }
    }

    let mut queue = VecDeque::new();
    for (id, degree) in &indegree {
        if *degree == 0 {
            queue.push_back(id.clone());
        }
    }

    let mut order = Vec::with_capacity(nodes.len());
    while let Some(node) = queue.pop_front() {
        order.push(node.clone());
        if let Some(children) = dependents.get(&node) {
            for child in children {
                if let Some(entry) = indegree.get_mut(child) {
                    *entry -= 1;
                    if *entry == 0 {
                        queue.push_back(child.clone());
                    }
                }
            }
        }
    }

    if order.len() != nodes.len() {
        return Err(ModuleError::CycleDetected(order));
    }

    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.push(format!("karte-{}-{}", prefix, nanos));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn single_file_plan_contains_entry() {
        let dir = unique_temp_dir("single");
        let file = dir.join("main.karte");
        fs::write(&file, "42").unwrap();

        let graph = ModuleGraph::single_file(&file).unwrap();
        let plan = graph.plan_for_entry(&file).unwrap();

        assert_eq!(plan.sequence.len(), 1);
        assert_eq!(plan.entry.as_str(), plan.sequence[0].as_str());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn manifest_plan_orders_dependencies() {
        let dir = unique_temp_dir("manifest");
        let helper = dir.join("helper.karte");
        fs::write(&helper, "10").unwrap();
        let main_file = dir.join("main.karte");
        fs::write(&main_file, "20").unwrap();

        let manifest = dir.join(MANIFEST_NAME);
        fs::write(
            &manifest,
            format!(
                r#"
[[modules]]
id = "helper"
path = "{}"

[[modules]]
id = "main"
path = "{}"
deps = ["helper"]
"#,
                helper.file_name().unwrap().to_string_lossy(),
                main_file.file_name().unwrap().to_string_lossy()
            ),
        )
        .unwrap();

        let graph = ModuleGraph::from_manifest(&manifest).unwrap();
        let plan = graph.plan_for_entry(&main_file).unwrap();

        assert_eq!(plan.sequence.len(), 2);
        assert_eq!(plan.entry.as_str(), "main");
        let helper_index = plan
            .sequence
            .iter()
            .position(|id| id.as_str() == "helper")
            .expect("helper missing");
        let main_index = plan
            .sequence
            .iter()
            .position(|id| id.as_str() == "main")
            .expect("main missing");
        assert!(helper_index < main_index);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn module_with_directory_sources_is_supported() {
        let dir = unique_temp_dir("multi");
        let helper = dir.join("helper.karte");
        fs::write(&helper, "let x = 1; x").unwrap();

        let main_dir = dir.join("main_mod");
        fs::create_dir_all(&main_dir).unwrap();
        let main_part1 = main_dir.join("part1.karte");
        let main_part2 = main_dir.join("part2.karte");
        fs::write(&main_part1, "let shared = 10; shared").unwrap();
        fs::write(&main_part2, "shared + 5").unwrap();

        let manifest = dir.join(MANIFEST_NAME);
        fs::write(
            &manifest,
            format!(
                r#"
[[modules]]
id = "helper"
path = "{}"

[[modules]]
id = "main"
dir = "{}"
deps = ["helper"]
"#,
                helper.file_name().unwrap().to_string_lossy(),
                main_dir.file_name().unwrap().to_string_lossy(),
            ),
        )
        .unwrap();

        let graph = ModuleGraph::from_manifest(&manifest).unwrap();
        let plan = graph.plan_for_entry(&main_part2).unwrap();

        assert_eq!(plan.entry.as_str(), "main");
        let main_meta = graph.metadata(&ModuleId::new("main")).unwrap();
        assert_eq!(main_meta.sources.len(), 2);
        assert!(main_meta.sources.iter().any(|p| p.ends_with("part1.karte")));
        assert!(main_meta.sources.iter().any(|p| p.ends_with("part2.karte")));

        fs::remove_dir_all(dir).unwrap();
    }
}
