# Branchpad

App macOS gốc, nhẹ, viết bằng Rust: git làm trên máy (không cần tài khoản) như Fork, kèm trình soạn file gọn như VSCode.

## Cấu trúc

| Crate | Việc |
|---|---|
| `crates/git` (`branchpad-git`) | Gọi lệnh `git` có sẵn trên máy, đọc kết quả: status, log, đồ thị nhánh, diff + stage theo đoạn, nhánh/remote/tag, stash, worktree, merge/rebase/cherry-pick/revert/reset, patch, rebase tương tác, xung đột, blame, grep. Chỉ dùng thư viện chuẩn, có unit test. |
| `crates/core` (`branchpad-core`) | Không phụ thuộc giao diện: cây file tải dần (ẩn file .gitignore), theo dõi file bằng FSEvents (không polling), tìm file mờ (Cmd+P), đánh dấu dòng thêm/sửa/xoá, chú thích dòng, lưu cài đặt. |
| `crates/agent` (`branchpad-agent`) | Chạy Claude Code CLI / Codex CLI không tương tác trong repo, đọc sự kiện JSON theo dòng (không polling), dừng được cả nhóm tiến trình. Chỉ dùng thư viện chuẩn + serde_json, có unit test. |
| `crates/app` (`branchpad`) | Giao diện GPUI + gpui-component: tab repo, cột trái (Files · Thay đổi · Git), giữa (đồ thị + danh sách commit, dưới là chi tiết/diff), phải (trình soạn nhiều tab). |

## Build và chạy (macOS)

Cần Rust stable và Xcode (Metal):

```sh
cargo test -p branchpad-git -p branchpad-core   # unit test phần không có giao diện
cargo run -p branchpad -- .                      # chạy thử, mở repo hiện tại
cargo build --release -p branchpad               # bản release
cargo install cargo-bundle && cargo bundle --release -p branchpad   # tạo Branchpad.app
```

Mở từ dòng lệnh: `ln -s /Applications/Branchpad.app/Contents/MacOS/branchpad /usr/local/bin/branchpad`, rồi `branchpad .`
(app tự tách khỏi terminal; đặt `BRANCHPAD_FOREGROUND=1` để chạy ngay trong terminal và xem log).

Git: app dùng `git` của Homebrew nếu có, không thì `/usr/bin/git`; ép đường dẫn khác bằng `BRANCHPAD_GIT`.
SSH chạy với `BatchMode=yes` (trừ khi bạn tự đặt `core.sshCommand`/`GIT_SSH_COMMAND`), nên dùng key trong ssh-agent/keychain và không bao giờ hỏi mật khẩu.

## Phím tắt

| Phím | Việc |
|---|---|
| Cmd+O / Cmd+Shift+W | Mở / đóng repo |
| Cmd+Alt+← / → | Repo trước / sau |
| Cmd+P | Mở file nhanh |
| Cmd+Shift+F | Tìm chữ trong cả thư mục |
| Cmd+S / Cmd+W | Lưu / đóng tab file |
| Cmd+/ | Bật/tắt chú thích dòng |
| Ctrl+G | Đến dòng |
| Cmd+Enter | Commit |
| Cmd+R | Tải lại |
| Cmd+Shift+P / Cmd+Shift+U | Pull / Push |
| Cmd+Alt+F | Fetch |
| Cmd+Alt+S | Stash thay đổi |
| Cmd+B | Nhánh mới |
| Cmd+1 / 2 / 3 | Tab Files / Thay đổi / Git |
| Cmd+4 | Hiện/ẩn panel AI (cột phải, cạnh trình soạn) |
| Cmd+Shift+A / Cmd+Shift+Z | Stage hết / Bỏ stage hết |
| Cmd+Alt+B | Hiện/ẩn trình soạn |
| Cmd+Alt+H | Hiện/ẩn cột lịch sử + chi tiết (mặc định ẩn ở tab Files) |
| Cmd+, | Cài đặt & chọn giao diện / chỉnh font |
| Esc | Đóng hộp nổi |

Trong tab Thay đổi (khi không gõ ở ô nhập): ↑/↓ chuyển file, Cmd+A chọn hết, Cmd+click chọn thêm, Shift+click chọn dải, Space stage/bỏ stage file đã chọn, Backspace/Delete bỏ thay đổi file đã chọn (có hỏi xác nhận); Cmd+Backspace làm việc này ở bất cứ đâu trong repo; chuột phải file có Stage/Bỏ stage/Discard.

## AI (Claude Code / Codex)

Panel **AI** ở cột phải, bên phải trình soạn (nút **AI** ở góc phải thanh tab của trình soạn, hoặc Cmd+4) giao việc cho `claude` hoặc `codex` đã cài và đăng nhập sẵn trên máy (app không lưu API key).
Gõ yêu cầu, Enter để chạy (Shift+Enter xuống dòng): panel hiện AI đang đọc/sửa file nào, chạy lệnh gì; bấm file đã sửa để xem diff rồi stage/commit hoặc discard. Mặc định AI sửa thẳng thư mục đang làm. Bật "Chạy trong worktree mới" trong Cài đặt thì mỗi phiên mới làm trên nhánh `ai/…` ở `<repo>-worktrees/`; nút **Mở worktree** mở nó thành tab repo để review và commit.
Mỗi phiên là một tab trong panel (icon cho biết Claude hay Codex); **+** mở phiên mới, **Hỏi tiếp** chạy tiếp cùng phiên, **Dừng** hoặc đóng tab phiên tắt hẳn CLI (đóng tab repo cũng vậy).
App tự tìm CLI ở `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`, thư mục npm/bun…; ép đường dẫn bằng `BRANCHPAD_CLAUDE` / `BRANCHPAD_CODEX`.
Claude chạy với `--permission-mode acceptEdits`; Codex chạy trong sandbox `workspace-write` (chỉ ghi trong repo, chặn mạng).

Hoàn tác/làm lại, tìm/thay trong file, thụt lề tự động dùng sẵn của bộ soạn gpui-component (Cmd+Z, Cmd+Shift+Z, Cmd+F).
