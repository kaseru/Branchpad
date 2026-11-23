//! Sự kiện chung của mọi CLI.

use std::path::PathBuf;

/// Loại công cụ AI đang dùng (để chọn icon).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Read,
    Edit,
    Command,
    Search,
    Web,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AgentEvent {
    /// Phiên đã bắt đầu; `session_id` dùng để hỏi tiếp (resume).
    Started { session_id: String },
    /// Đoạn trả lời của AI.
    Text(String),
    /// Đoạn suy nghĩ (hiện mờ).
    Thinking(String),
    /// AI bắt đầu dùng một công cụ. `name` là tên công cụ, `detail` là file / lệnh / mẫu tìm.
    ToolStart { id: String, kind: ToolKind, name: String, detail: String },
    /// Công cụ chạy xong. `summary` là một dòng tóm tắt kết quả.
    ToolDone { id: String, ok: bool, summary: String },
    /// File bị AI sửa / tạo (đường dẫn tuyệt đối hoặc tương đối với thư mục chạy).
    FileChanged(PathBuf),
    /// Hết lượt. `summary` là câu trả lời cuối hoặc lỗi.
    Finished { ok: bool, summary: String, cost_usd: Option<f64>, tokens: Option<u64> },
    /// Lỗi không làm dừng phiên (hoặc stderr khi tiến trình thoát lỗi).
    Error(String),
    /// CLI xin quyền dùng một công cụ (chỉ Claude, khi hỏi quyền trong chat):
    /// trả lời bằng [`crate::AgentRun::respond_permission`] với cùng `id` và `input`.
    PermissionRequest { id: String, tool: String, detail: String, input: serde_json::Value },
    /// CLI tự huỷ yêu cầu xin quyền (hết lượt / bị ngắt).
    PermissionCancelled { id: String },
}
