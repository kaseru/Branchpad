//! Cây file: tải dần từng thư mục khi mở, ẩn file trong .gitignore.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use branchpad_git::Repo;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// Đường dẫn tuyệt đối.
    pub path: PathBuf,
    /// Đường dẫn tương đối so với gốc repo (dùng `/`).
    pub rel: String,
    pub is_dir: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub entry: Entry,
    pub depth: usize,
    pub expanded: bool,
}

pub struct FileTree {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    children: HashMap<PathBuf, Vec<Entry>>,
    /// Các dòng đang hiện, dựng một lần cho tới khi cây đổi (cuộn / vẽ lại không dựng lại).
    rows_cache: RefCell<Option<Rc<Vec<TreeRow>>>>,
}

fn rel_of(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

impl FileTree {
    pub fn new(repo: &Repo) -> io::Result<Self> {
        let root = repo.workdir().to_path_buf();
        let mut tree = FileTree {
            root: root.clone(),
            expanded: HashSet::new(),
            children: HashMap::new(),
            rows_cache: RefCell::new(None),
        };
        tree.load(repo, &root)?;
        Ok(tree)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn read_dir(&self, repo: &Repo, dir: &Path) -> io::Result<Vec<Entry>> {
        let mut entries = Vec::new();
        for item in std::fs::read_dir(dir)? {
            let item = item?;
            let name = item.file_name().to_string_lossy().into_owned();
            if name == ".git" || name == ".DS_Store" {
                continue;
            }
            let path = item.path();
            let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
            entries.push(Entry {
                rel: rel_of(&self.root, &path),
                name,
                path,
                is_dir,
            });
        }
        let rels: Vec<String> = entries
            .iter()
            .map(|e| if e.is_dir { format!("{}/", e.rel) } else { e.rel.clone() })
            .collect();
        match repo.ignored(&rels) {
            Ok(ignored) => {
                entries.retain(|e| {
                    !ignored.contains(&e.rel) && !ignored.contains(&format!("{}/", e.rel))
                });
            }
            Err(e) => eprintln!("branchpad: cannot check .gitignore: {e}"),
        }
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(entries)
    }

    fn load(&mut self, repo: &Repo, dir: &Path) -> io::Result<()> {
        self.rows_cache.get_mut().take();
        let entries = self.read_dir(repo, dir)?;
        self.children.insert(dir.to_path_buf(), entries);
        Ok(())
    }

    pub fn is_expanded(&self, dir: &Path) -> bool {
        self.expanded.contains(dir)
    }

    /// Mở/đóng thư mục; lần mở đầu mới đọc ổ đĩa.
    pub fn toggle(&mut self, repo: &Repo, dir: &Path) -> io::Result<()> {
        self.rows_cache.get_mut().take();
        if !self.expanded.remove(dir) {
            if !self.children.contains_key(dir) {
                self.load(repo, dir)?;
            }
            self.expanded.insert(dir.to_path_buf());
        }
        Ok(())
    }

    /// Mở mọi thư mục cha của `path` (để hiện file đang chọn).
    pub fn reveal(&mut self, repo: &Repo, path: &Path) -> io::Result<()> {
        self.rows_cache.get_mut().take();
        let mut dirs: Vec<PathBuf> = path
            .ancestors()
            .skip(1)
            .take_while(|p| p.starts_with(&self.root) && *p != self.root)
            .map(Path::to_path_buf)
            .collect();
        dirs.reverse();
        for d in dirs {
            if !self.children.contains_key(&d) {
                self.load(repo, &d)?;
            }
            self.expanded.insert(d);
        }
        Ok(())
    }

    /// Như [`rows`](Self::rows) nhưng dùng lại kết quả cho tới khi cây đổi.
    pub fn rows_shared(&self) -> Rc<Vec<TreeRow>> {
        self.rows_cache.borrow_mut().get_or_insert_with(|| Rc::new(self.rows())).clone()
    }

    /// Các dòng đang hiện (theo thứ tự hiển thị).
    pub fn rows(&self) -> Vec<TreeRow> {
        let mut out = Vec::new();
        self.push_rows(&self.root, 0, &mut out);
        out
    }

    fn push_rows(&self, dir: &Path, depth: usize, out: &mut Vec<TreeRow>) {
        let Some(children) = self.children.get(dir) else { return };
        for e in children {
            let expanded = e.is_dir && self.expanded.contains(&e.path);
            out.push(TreeRow {
                entry: e.clone(),
                depth,
                expanded,
            });
            if expanded {
                self.push_rows(&e.path, depth + 1, out);
            }
        }
    }

    /// Đọc lại các thư mục đã tải có chứa file vừa đổi (thêm/xoá/đổi tên).
    pub fn refresh_paths<'a>(
        &mut self,
        repo: &Repo,
        changed: impl IntoIterator<Item = &'a PathBuf>,
    ) {
        let mut dirs: HashSet<PathBuf> = HashSet::new();
        for p in changed {
            if let Some(parent) = p.parent() {
                if self.children.contains_key(parent) {
                    dirs.insert(parent.to_path_buf());
                }
            }
        }
        if !dirs.is_empty() {
            self.rows_cache.get_mut().take();
        }
        for d in dirs {
            if d.is_dir() {
                if let Err(e) = self.load(repo, &d) {
                    eprintln!("branchpad: cannot read {}: {e}", d.display());
                }
            } else {
                self.children.remove(&d);
                self.expanded.remove(&d);
            }
        }
    }

    /// Đọc lại toàn bộ thư mục đã tải (sau checkout, đổi .gitignore...).
    pub fn refresh_all(&mut self, repo: &Repo) {
        self.rows_cache.get_mut().take();
        let dirs: Vec<PathBuf> = self.children.keys().cloned().collect();
        for d in dirs {
            if d.is_dir() {
                if let Err(e) = self.load(repo, &d) {
                    eprintln!("branchpad: cannot read {}: {e}", d.display());
                }
            } else {
                self.children.remove(&d);
                self.expanded.remove(&d);
            }
        }
    }
}

// ---- Thao tác file ----

fn check_name(name: &str) -> io::Result<()> {
    if name.trim().is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid name: {name:?}"),
        ));
    }
    Ok(())
}

pub fn create_file(dir: &Path, name: &str) -> io::Result<PathBuf> {
    check_name(name)?;
    let path = dir.join(name);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    Ok(path)
}

pub fn create_dir(dir: &Path, name: &str) -> io::Result<PathBuf> {
    check_name(name)?;
    let path = dir.join(name);
    std::fs::create_dir(&path)?;
    Ok(path)
}

pub fn rename(path: &Path, new_name: &str) -> io::Result<PathBuf> {
    check_name(new_name)?;
    let target = path
        .parent()
        .map(|p| p.join(new_name))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "cannot rename the root"))?;
    if target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{new_name} already exists"),
        ));
    }
    std::fs::rename(path, &target)?;
    Ok(target)
}

/// Xoá vào Thùng rác (khôi phục được).
pub fn delete(path: &Path) -> io::Result<()> {
    trash::delete(path).map_err(|e| io::Error::other(e.to_string()))
}

/// Mở Finder / Explorer / trình quản lý file và chọn file.
pub fn reveal_in_finder(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        // explorer.exe trả exit code 1 dù thành công nên không kiểm tra.
        Command::new("explorer").arg(format!("/select,{}", path.display())).status()?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let status = if cfg!(target_os = "macos") {
            Command::new("open").arg("-R").arg(path).status()?
        } else {
            Command::new("xdg-open").arg(path.parent().unwrap_or(path)).status()?
        };
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other("Cannot open Finder"))
        }
    }
}
