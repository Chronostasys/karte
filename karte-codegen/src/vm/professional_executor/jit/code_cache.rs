//! 代码缓存管理
//!
//! 负责缓存编译后的函数，提供高效的查找和管理功能

use super::compiler_trait::CompiledFunction;
use std::collections::HashMap;

/// 代码缓存
/// 
/// 管理编译后的函数，提供缓存、查找和清理功能
#[derive(Debug)]
pub struct CodeCache {
    /// 缓存的函数 (函数名 -> 编译后的函数)
    functions: HashMap<String, CompiledFunction>,
    
    /// 函数调用计数器 (用于热点检测)
    call_counts: HashMap<String, u32>,
    
    /// 总缓存大小（字节）
    total_size: usize,
    
    /// 最大缓存大小限制
    size_limit: usize,
    
    /// 缓存命中统计
    cache_hits: u64,
    
    /// 缓存未命中统计
    cache_misses: u64,
}

impl CodeCache {
    /// 创建新的代码缓存
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            call_counts: HashMap::new(),
            total_size: 0,
            size_limit: 64 * 1024 * 1024, // 默认64MB限制
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    /// 创建带大小限制的代码缓存
    pub fn with_size_limit(size_limit: usize) -> Self {
        Self {
            functions: HashMap::new(),
            call_counts: HashMap::new(),
            total_size: 0,
            size_limit,
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    /// 获取函数
    pub fn get(&mut self, function_name: &str) -> Option<CompiledFunction> {
        if let Some(function) = self.functions.get(function_name) {
            // 增加调用计数
            *self.call_counts.entry(function_name.to_string()).or_insert(0) += 1;
            self.cache_hits += 1;
            Some(function.clone())
        } else {
            self.cache_misses += 1;
            None
        }
    }

    /// 检查是否包含函数
    pub fn contains(&self, function_name: &str) -> bool {
        self.functions.contains_key(function_name)
    }

    /// 插入编译后的函数
    pub fn insert(&mut self, function_name: String, compiled_function: CompiledFunction) -> Result<(), String> {
        let function_size = compiled_function.code_size();
        
        // 检查是否会超出大小限制
        if self.total_size + function_size > self.size_limit {
            // 尝试清理一些旧的函数
            self.evict_lru_functions(function_size)?;
        }

        // 如果函数已存在，先移除旧版本
        if let Some(old_function) = self.functions.remove(&function_name) {
            self.total_size -= old_function.code_size();
        }

        // 插入新函数
        self.total_size += function_size;
        self.functions.insert(function_name.clone(), compiled_function);
        
        // 初始化调用计数
        self.call_counts.insert(function_name, 0);

        Ok(())
    }

    /// 移除函数
    pub fn remove(&mut self, function_name: &str) -> Option<CompiledFunction> {
        if let Some(function) = self.functions.remove(function_name) {
            self.total_size -= function.code_size();
            self.call_counts.remove(function_name);
            Some(function)
        } else {
            None
        }
    }

    /// 清理所有缓存
    pub fn clear(&mut self) {
        self.functions.clear();
        self.call_counts.clear();
        self.total_size = 0;
        self.cache_hits = 0;
        self.cache_misses = 0;
    }

    /// 获取函数调用次数
    pub fn get_call_count(&self, function_name: &str) -> u32 {
        self.call_counts.get(function_name).copied().unwrap_or(0)
    }

    /// 增加函数调用次数
    pub fn increment_call_count(&mut self, function_name: &str) {
        *self.call_counts.entry(function_name.to_string()).or_insert(0) += 1;
    }

    /// 获取缓存统计信息
    pub fn get_statistics(&self) -> CacheStatistics {
        let hit_rate = if self.cache_hits + self.cache_misses > 0 {
            self.cache_hits as f64 / (self.cache_hits + self.cache_misses) as f64 * 100.0
        } else {
            0.0
        };

        CacheStatistics {
            total_functions: self.functions.len(),
            total_size: self.total_size,
            size_limit: self.size_limit,
            cache_hits: self.cache_hits,
            cache_misses: self.cache_misses,
            hit_rate,
            usage_ratio: self.total_size as f64 / self.size_limit as f64 * 100.0,
        }
    }

    /// 获取热点函数列表（按调用次数排序）
    pub fn get_hot_functions(&self, limit: usize) -> Vec<(String, u32)> {
        let mut functions: Vec<_> = self.call_counts.iter()
            .map(|(name, count)| (name.clone(), *count))
            .collect();
        
        // 按调用次数降序排序
        functions.sort_by(|a, b| b.1.cmp(&a.1));
        
        functions.into_iter().take(limit).collect()
    }

    /// 驱逐最少使用的函数以释放空间
    fn evict_lru_functions(&mut self, needed_space: usize) -> Result<(), String> {
        let mut functions_by_usage: Vec<_> = self.call_counts.iter()
            .map(|(name, count)| (name.clone(), *count))
            .collect();
        
        // 按调用次数升序排序（最少使用的在前）
        functions_by_usage.sort_by(|a, b| a.1.cmp(&b.1));
        
        let mut freed_space = 0;
        let mut to_remove = Vec::new();
        
        for (function_name, _) in functions_by_usage {
            if let Some(function) = self.functions.get(&function_name) {
                freed_space += function.code_size();
                to_remove.push(function_name);
                
                if freed_space >= needed_space {
                    break;
                }
            }
        }
        
        if freed_space < needed_space {
            return Err(format!(
                "无法释放足够空间：需要 {} 字节，最多只能释放 {} 字节",
                needed_space, freed_space
            ));
        }
        
        // 移除选中的函数
        for function_name in to_remove {
            self.remove(&function_name);
        }
        
        Ok(())
    }

    /// 获取所有函数名称
    pub fn function_names(&self) -> Vec<String> {
        self.functions.keys().cloned().collect()
    }

    /// 获取缓存大小信息
    pub fn size_info(&self) -> (usize, usize) {
        (self.total_size, self.size_limit)
    }

    /// 设置大小限制
    pub fn set_size_limit(&mut self, new_limit: usize) -> Result<(), String> {
        if new_limit < self.total_size {
            // 需要清理一些函数
            let excess = self.total_size - new_limit;
            self.evict_lru_functions(excess)?;
        }
        
        self.size_limit = new_limit;
        Ok(())
    }
}

impl Default for CodeCache {
    fn default() -> Self {
        Self::new()
    }
}

/// 缓存统计信息
#[derive(Debug, Clone)]
pub struct CacheStatistics {
    /// 总函数数量
    pub total_functions: usize,
    /// 总缓存大小（字节）
    pub total_size: usize,
    /// 大小限制（字节）
    pub size_limit: usize,
    /// 缓存命中次数
    pub cache_hits: u64,
    /// 缓存未命中次数
    pub cache_misses: u64,
    /// 命中率（百分比）
    pub hit_rate: f64,
    /// 使用率（百分比）
    pub usage_ratio: f64,
}

impl CacheStatistics {
    /// 打印统计信息
    pub fn print(&self) {
        println!("=== 代码缓存统计 ===");
        println!("函数数量: {}", self.total_functions);
        println!("缓存大小: {:.2} MB / {:.2} MB", 
                 self.total_size as f64 / (1024.0 * 1024.0),
                 self.size_limit as f64 / (1024.0 * 1024.0));
        println!("使用率: {:.1}%", self.usage_ratio);
        println!("命中次数: {}", self.cache_hits);
        println!("未命中次数: {}", self.cache_misses);
        println!("命中率: {:.1}%", self.hit_rate);
    }
} 