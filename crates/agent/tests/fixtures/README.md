# Fixture JSONL của CLI

Mỗi file là output (stdout) của một lần chạy:

- `claude_*.jsonl`: `claude -p --output-format stream-json --verbose --permission-mode acceptEdits`
- `codex_*.jsonl`: `codex exec --json -c sandbox_mode="workspace-write" -`

**Đây là fixture viết theo tài liệu**, chưa chép từ lần chạy thật (máy viết code chỉ xem `--help` của
`claude` 2.1.328 và `codex` 0.155.0, không chạy để khỏi tốn quota). Có cố ý chèn một dòng loại lạ và
một dòng không phải JSON để kiểm tra parser chịu lỗi.

Thay bằng output thật: chạy lệnh trên trong một repo nháp với yêu cầu nhỏ có sửa file và chạy lệnh,
lưu stdout đè vào file tương ứng, ghi phiên bản CLI vào đây, rồi chạy `cargo test -p branchpad-agent`.
