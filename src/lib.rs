use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result, anyhow};
use bincode::{Decode, Encode, config};
use parking_lot::Mutex;
use path_slash::PathExt;
use rayon::prelude::*;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Clone, Decode, Encode, Default)]
pub struct Dir {
    size: u64,
    file: Vec<File>,
    children: Vec<String>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct File {
    typ: u8,
    name: String,
    size: u64,
}

impl Dir {
    pub fn add_file(&mut self, file: File) {
        self.size += file.size;
        self.file.push(file);
    }

    pub fn remove_file(&mut self, files: Vec<String>) {
        self.file.retain(|f| !files.contains(&f.name));
        self.size = self.file.iter().map(|f| f.size).sum();
    }

    pub fn add_child(&mut self, child: String) {
        self.children.push(child);
    }

    pub fn remove_child(&mut self, children: Vec<String>) {
        self.children.retain(|c| !children.contains(c));
    }
}

impl File {
    fn new(entry: &DirEntry) -> Result<Self> {
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = entry
            .metadata()
            .with_context(|| format!("无法获取元数据: {name}"))?;

        Ok(File {
            typ: Self::recognize_file_type(entry.path()),
            name,
            size: metadata.len(),
        })
    }

    fn recognize_file_type(file: &Path) -> u8 {
        file.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| match ext.to_lowercase().as_str() {
                "png" => 0,
                "jpg" | "jpeg" => 1,
                "webp" => 2,
                "svg" => 3,
                "gif" => 4,
                _ => 5,
            })
            .unwrap_or(5)
    }
}

pub fn map(start_path: &str) -> Result<Vec<u8>> {
    let mut tree = build_tree(start_path)?;
    let sizes = calc_size(&tree, start_path)?;
    for (path, size) in sizes {
        tree.get_mut(&path)
            .ok_or_else(|| anyhow!("目录不存在: {}", path))?
            .size = size;
    }
    let encoded = bincode::encode_to_vec(&tree, config::standard())?;
    Ok(zstd::encode_all(&encoded[..], 3)?)
}

pub fn unmap(data: &[u8]) -> Result<HashMap<String, Dir>> {
    let decompressed = zstd::decode_all(data)?;
    let (dirs, _) = bincode::decode_from_slice(&decompressed, config::standard())?;
    Ok(dirs)
}

fn build_tree(start_path: &str) -> Result<HashMap<String, Dir>> {
    let dirs = Mutex::new(HashMap::new());
    let entries: Vec<_> = WalkDir::new(start_path)
        .into_iter()
        .filter_map(Result::ok)
        .collect();

    // 添加所有目录
    entries
        .par_iter()
        .filter(|e| e.file_type().is_dir())
        .for_each(|entry| {
            let path = entry.path().to_slash_lossy().into_owned();
            dirs.lock().entry(path).or_insert_with(Dir::default);
        });

    // 处理文件和目录关系
    let error = Mutex::new(None);
    entries.par_iter().for_each(|entry| {
        if error.lock().is_some() {
            return;
        }

        let path = entry.path();
        if entry.file_type().is_file() {
            if let Some(parent) = path.parent() {
                let parent_path = parent.to_slash_lossy().into_owned();
                match File::new(entry) {
                    Ok(file) => {
                        let mut dirs_lock = dirs.lock();
                        if let Some(parent_dir) = dirs_lock.get_mut(&parent_path) {
                            parent_dir.add_file(file);
                        } else {
                            *error.lock() = Some(anyhow!("父目录不存在: {}", parent_path));
                        }
                    }
                    Err(e) => {
                        *error.lock() = Some(e);
                    }
                }
            }
        } else if entry.file_type().is_dir()
            && let Some(parent) = path.parent()
        {
            if parent.as_os_str().is_empty() {
                return;
            }

            let parent_path = parent.to_slash_lossy().into_owned();
            let dir_path = path.to_slash_lossy().into_owned();

            let mut dirs_lock = dirs.lock();
            if let Some(parent_dir) = dirs_lock.get_mut(&parent_path) {
                parent_dir.add_child(dir_path);
            } else {
                *error.lock() = Some(anyhow!("父目录不存在: {}", parent_path));
            }
        }
    });

    if let Some(err) = error.lock().take() {
        return Err(err);
    }

    Ok(dirs.into_inner())
}

pub fn calc_size(dirs: &HashMap<String, Dir>, start_path: &str) -> Result<HashMap<String, u64>> {
    let mut sizes = HashMap::new();

    // 使用枚举表示栈中的操作类型
    enum StackItem<'a> {
        Process(&'a str),   // 处理目录
        Calculate(&'a str), // 计算目录大小
    }

    let mut stack = vec![StackItem::Process(start_path)];

    while let Some(item) = stack.pop() {
        match item {
            StackItem::Process(path) => {
                let dir = dirs
                    .get(path)
                    .ok_or_else(|| anyhow!("目录不存在: {}", path))?;

                // 先推入计算操作，确保在处理完所有子目录后计算当前目录的大小
                stack.push(StackItem::Calculate(path));

                // 然后推入所有子目录的处理操作
                for child in &dir.children {
                    stack.push(StackItem::Process(child));
                }
            }
            StackItem::Calculate(path) => {
                let dir = dirs
                    .get(path)
                    .ok_or_else(|| anyhow!("目录不存在: {}", path))?;

                let mut size = dir.size;

                for child in &dir.children {
                    size += sizes
                        .get(child)
                        .ok_or_else(|| anyhow!("目录不存在: {}", path))?;
                }

                sizes.insert(path.to_string(), size);
            }
        }
    }

    Ok(sizes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map() {
        let raw = map(".").expect("映射失败");
        std::fs::write("map", raw).expect("写入文件失败");
    }

    #[test]
    fn test_unmap() {
        let data = std::fs::read("map").expect("读取文件失败");
        let dirs = unmap(&data).expect("解映射失败");
        println!("{dirs:?}");
    }
}
