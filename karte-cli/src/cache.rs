use karte_ir_codec::{IrDisplay, IrParse};
use karte_lir::LirProgram;
use karte_mir::MirProgram;
use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct CompilationCache {
    root: PathBuf,
    enabled: bool,
    version_tag: String,
}

impl CompilationCache {
    pub fn new() -> Self {
        Self::new_with_root(PathBuf::from("target/.karte-cache"))
    }

    pub fn new_with_root(root: PathBuf) -> Self {
        let _ = fs::create_dir_all(&root);
        let enabled = true;
        Self {
            root,
            enabled,
            version_tag: format!(
                "{}-{}",
                env!("CARGO_PKG_VERSION"),
                option_env!("KARTE_CACHE_TAG").unwrap_or("default")
            ),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn eligible_filename(&self, filename: &str) -> bool {
        filename != "input" && Path::new(filename).exists()
    }

    pub fn make_key(
        &self,
        filename: &str,
        source_fingerprint: u64,
        interface_hash: u64,
        module_id: Option<&str>,
        optimization_level: crate::OptimizationLevel,
    ) -> String {
        let mut hasher = DefaultHasher::new();
        self.version_tag.hash(&mut hasher);
        filename.hash(&mut hasher);
        source_fingerprint.hash(&mut hasher);
        interface_hash.hash(&mut hasher);
        if let Some(id) = module_id {
            id.hash(&mut hasher);
        }
        format!("{:?}", optimization_level).hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    pub fn load(&self, key: &str) -> Option<(MirProgram, LirProgram)> {
        let path = self.root.join(format!("{key}.cache"));
        let data = fs::read_to_string(path).ok()?;
        let (mir_text, lir_text) = data.split_once("\n===LIR===\n")?;
        let mir = match MirProgram::parse_ir(mir_text) {
            Ok(mir) => mir,
            Err(_) => {
                self.invalidate(key);
                return None;
            }
        };
        let lir = match LirProgram::parse_ir(lir_text) {
            Ok(lir) => lir,
            Err(_) => {
                self.invalidate(key);
                return None;
            }
        };
        Some((mir, lir))
    }

    pub fn store(&self, key: &str, mir: &MirProgram, lir: &LirProgram) {
        let path = self.root.join(format!("{key}.cache"));
        let content = format!("{}\n===LIR===\n{}", mir.to_ir_string(), lir.to_ir_string());
        if let Ok(mut file) = File::create(path) {
            let _ = file.write_all(content.as_bytes());
        }
    }

    fn invalidate(&self, key: &str) {
        let path = self.root.join(format!("{key}.cache"));
        let _ = fs::remove_file(path);
    }

    pub fn get(&self, key: &CacheKey) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let path = self.root.join(format!("{}.lir", key.as_str()));
        fs::read_to_string(path).ok()
    }

    pub fn put(&self, key: CacheKey, content: String) {
        if !self.enabled {
            return;
        }
        let path = self.root.join(format!("{}.lir", key.as_str()));
        let _ = fs::write(path, content);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey(String);

impl CacheKey {
    pub fn new(module_id: &str, source_fingerprint: u64) -> Self {
        let mut hasher = DefaultHasher::new();
        module_id.hash(&mut hasher);
        source_fingerprint.hash(&mut hasher);
        Self(format!("{:016x}", hasher.finish()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
