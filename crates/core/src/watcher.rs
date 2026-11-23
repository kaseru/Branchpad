//! Theo dõi thay đổi file bằng sự kiện hệ điều hành (FSEvents trên macOS),
//! không polling. Sự kiện dồn dập (build, checkout) được gom thành một đợt.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

/// Một đợt thay đổi.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WatchBatch {
    /// File/thư mục trong thư mục làm việc bị đổi (đường dẫn tuyệt đối).
    pub paths: BTreeSet<PathBuf>,
    /// HEAD, index, refs... trong `.git` bị đổi.
    pub git_changed: bool,
}

impl WatchBatch {
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && !self.git_changed
    }
}

/// Giữ watcher sống; drop là dừng theo dõi.
pub struct Watcher {
    _inner: RecommendedWatcher,
}

/// Gom sự kiện thành một đợt khi yên lặng đủ khoảng này (debounce cuối): build / checkout bắn hàng loạt sự kiện
/// chỉ tạo một lần làm mới thay vì nhiều lần.
const COALESCE: Duration = Duration::from_millis(300);
/// Nhưng sự kiện liên tục không được giữ đợt này quá lâu.
const MAX_WAIT: Duration = Duration::from_secs(2);

enum Kind {
    Ignore,
    Git,
    Work,
}

fn classify(path: &Path, workdir: &Path, git_dir: &Path) -> Kind {
    if let Ok(rel) = path.strip_prefix(git_dir) {
        let first = rel.components().next().map(|c| c.as_os_str().to_string_lossy());
        let noisy = matches!(
            first.as_deref(),
            Some("objects" | "logs" | "branchpad" | "hooks" | "lfs")
        );
        let lock = path.extension().is_some_and(|e| e == "lock");
        return if noisy || lock { Kind::Ignore } else { Kind::Git };
    }
    if let Ok(rel) = path.strip_prefix(workdir) {
        if rel.components().next().is_some_and(|c| c.as_os_str() == ".git") {
            return Kind::Ignore;
        }
        return Kind::Work;
    }
    Kind::Ignore
}

impl Watcher {
    /// Bắt đầu theo dõi `workdir` (đệ quy) và `git_dir`. `on_batch` được gọi
    /// trên luồng nền của watcher.
    pub fn start(
        workdir: &Path,
        git_dir: &Path,
        on_batch: impl Fn(WatchBatch) + Send + 'static,
    ) -> notify::Result<Self> {
        let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut inner = notify::recommended_watcher(tx)?;
        inner.watch(workdir, RecursiveMode::Recursive)?;
        if !git_dir.starts_with(workdir) {
            // Worktree phụ: thư mục git nằm ngoài thư mục làm việc.
            inner.watch(git_dir, RecursiveMode::Recursive)?;
        }

        let workdir = workdir.to_path_buf();
        let git_dir = git_dir.to_path_buf();
        std::thread::Builder::new()
            .name("branchpad-watch".into())
            .spawn(move || {
                let add = |batch: &mut WatchBatch, ev: notify::Result<notify::Event>| match ev {
                    Ok(ev) => {
                        for p in ev.paths {
                            match classify(&p, &workdir, &git_dir) {
                                Kind::Git => batch.git_changed = true,
                                Kind::Work => {
                                    batch.paths.insert(p);
                                }
                                Kind::Ignore => {}
                            }
                        }
                    }
                    Err(e) => eprintln!("branchpad: file watcher error: {e}"),
                };
                // recv() chờ sự kiện; kết thúc khi watcher bị drop.
                while let Ok(first) = rx.recv() {
                    let mut batch = WatchBatch::default();
                    add(&mut batch, first);
                    let deadline = std::time::Instant::now() + MAX_WAIT;
                    loop {
                        let left = deadline.saturating_duration_since(std::time::Instant::now());
                        if left.is_zero() {
                            break;
                        }
                        match rx.recv_timeout(COALESCE.min(left)) {
                            Ok(ev) => add(&mut batch, ev),
                            Err(_) => break,
                        }
                    }
                    if !batch.is_empty() {
                        on_batch(batch);
                    }
                }
            })
            .map_err(|e| notify::Error::generic(&e.to_string()))?;

        Ok(Watcher { _inner: inner })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_paths() {
        let w = Path::new("/r");
        let g = Path::new("/r/.git");
        assert!(matches!(classify(Path::new("/r/.git/HEAD"), w, g), Kind::Git));
        assert!(matches!(classify(Path::new("/r/.git/refs/heads/main"), w, g), Kind::Git));
        assert!(matches!(classify(Path::new("/r/.git/objects/ab/cd"), w, g), Kind::Ignore));
        assert!(matches!(classify(Path::new("/r/.git/index.lock"), w, g), Kind::Ignore));
        assert!(matches!(classify(Path::new("/r/src/a.rs"), w, g), Kind::Work));
    }
}
