//! GPU 张量管理
//!
//! GpuTensor 表示 GPU 显存中的一块数据。
//! 底层通过 CUDA Driver API 的 cuMemAlloc/cuMemFree 管理。

use crate::ffi;

/// GPU 张量 — 存储在 GPU 全局内存中的多维数组
#[derive(Debug)]
pub struct GpuTensor {
    /// GPU 设备地址
    pub device_ptr: u64,
    /// 元素类型大小（字节）
    pub elem_size: usize,
    /// 形状（各维度大小）
    pub shape: Vec<usize>,
    /// 总字节数
    pub nbytes: usize,
}

impl GpuTensor {
    /// 在 GPU 上分配零初始化的张量
    pub fn zeros(elem_size: usize, shape: &[usize]) -> Option<Self> {
        let nbytes: usize = shape.iter().product::<usize>() * elem_size;
        if nbytes == 0 {
            return None;
        }

        // 在实际实现中调用 cuMemAlloc
        // 目前返回 None 表示无 GPU 支持
        if !ffi::is_cuda_available() {
            return None;
        }

        // GPU 分配逻辑占位
        // let device_ptr = unsafe { cu_mem_alloc(nbytes as u64) };
        None
    }

    /// 从 host 数据创建 GPU 张量（Host → Device 拷贝）
    pub fn from_host(data: &[u8], elem_size: usize, shape: &[usize]) -> Option<Self> {
        let tensor = Self::zeros(elem_size, shape)?;
        // 在实际实现中调用 cuMemcpyHtoD
        // unsafe { cu_memcpy_h2d(tensor.device_ptr, data.as_ptr(), data.len()); }
        Some(tensor)
    }

    /// 将 GPU 张量数据拷贝回 host（Device → Host 拷贝）
    pub fn to_host(&self) -> Vec<u8> {
        let mut buf = vec![0u8; self.nbytes];
        // 在实际实现中调用 cuMemcpyDtoH
        // unsafe { cu_memcpy_d2h(buf.as_mut_ptr(), self.device_ptr, self.nbytes); }
        buf
    }

    /// 元素总数
    pub fn num_elements(&self) -> usize {
        self.shape.iter().product()
    }
}

impl Drop for GpuTensor {
    fn drop(&mut self) {
        if self.device_ptr != 0 {
            // 在实际实现中调用 cuMemFree
            // unsafe { cu_mem_free(self.device_ptr); }
        }
    }
}

/// CPU 回退张量 — 在无 GPU 时用于模拟执行
#[derive(Debug, Clone)]
pub struct CpuTensor {
    /// 数据存储（i64 数组，底层可以是任意类型）
    pub data: Vec<i64>,
    /// 形状
    pub shape: Vec<usize>,
}

impl CpuTensor {
    pub fn zeros(shape: &[usize]) -> Self {
        let n: usize = shape.iter().product();
        Self {
            data: vec![0; n],
            shape: shape.to_vec(),
        }
    }

    pub fn from_data(data: Vec<i64>, shape: &[usize]) -> Self {
        Self {
            data,
            shape: shape.to_vec(),
        }
    }

    pub fn num_elements(&self) -> usize {
        self.data.len()
    }

    /// 计算多维索引到一维偏移
    pub fn offset(&self, indices: &[usize]) -> usize {
        let mut offset = 0;
        for (i, &idx) in indices.iter().enumerate() {
            offset = offset * self.shape[i] + idx;
        }
        offset
    }

    /// 获取元素
    pub fn get(&self, indices: &[usize]) -> i64 {
        self.data[self.offset(indices)]
    }

    /// 设置元素
    pub fn set(&mut self, indices: &[usize], value: i64) {
        let off = self.offset(indices);
        self.data[off] = value;
    }
}
