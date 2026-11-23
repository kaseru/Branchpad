# Plan: Giao việc cho Claude Code CLI / Codex CLI ngay trong Branchpad

## Mục tiêu
Trong một tab repo, gõ yêu cầu ("sửa lỗi X", "đổi tên hàm Y") → Branchpad chạy `claude` hoặc `codex` **ở chế độ không tương tác** trong thư mục repo → hiện tiến trình (đang đọc/sửa file nào, chạy lệnh gì, câu trả lời) → AI xong thì người dùng xem diff ở tab Thay đổi và stage/commit hoặc discard như bình thường.

Không nhúng terminal (PTY + giả lập terminal quá nặng, đi ngược mục tiêu "nhẹ").

Đích:
- Không tốn RAM khi không dùng: không có tiến trình nền, không đọc gì cho tới khi bấm Chạy
- Không polling: đọc stdout của CLI theo từng dòng; file AI sửa hiện ra nhờ watcher FSEvents đang có
- Không quản lý API key: CLI dùng tài khoản người dùng đã đăng nhập sẵn (`claude login`, `codex login`)

## Cách chạy CLI (đối chiếu lại với bản đã cài ở bước 1)

| | Claude Code | Codex |
|---|---|---|
| Lệnh | `claude -p --output-format stream-json --verbose` (prompt qua stdin) | `codex exec --json -` (prompt qua stdin), `-C <dir>` |
| Cho sửa file không hỏi | `--permission-mode acceptEdits` (+ `--allowedTools` cho lệnh shell nếu cho phép) | `--sandbox workspace-write` (hoặc `--full-auto`) |
| Chạy tiếp phiên trước | `--resume <session_id>` | `codex exec resume <thread_id>` |
| Chọn model | `--model <tên>` | `-m <tên>` |
| Sự kiện chính | `system/init` (session_id), `assistant` (text, tool_use), `user` (tool_result), `result` (kết quả, chi phí, lỗi) | `thread.started` (thread_id), `item.started/completed` với `agent_message`, `reasoning`, `command_execution`, `file_change`, `turn.completed` (usage), `turn.failed`, `error` |

Hai CLI quy về chung một kiểu sự kiện của app, nên giao diện không cần biết đang chạy CLI nào.

## Kiến trúc

### Crate mới `crates/agent` (`branchpad-agent`)
Không phụ thuộc giao diện, giống `crates/git`: chỉ thư viện chuẩn + `serde_json`, có unit test.
- `binary.rs`: tìm đường dẫn CLI giống `git_binary()` trong `crates/git/src/runner.rs`. App mở từ Finder có PATH ngắn, nên lần lượt thử: đường dẫn trong Cài đặt → biến `BRANCHPAD_CLAUDE` / `BRANCHPAD_CODEX` → `~/.local/bin`, `~/.claude/local`, `/opt/homebrew/bin`, `/usr/local/bin`, thư mục global của npm/bun (`~/.npm-global/bin`, `~/.bun/bin`). Không thấy thì trả lỗi rõ ràng và chỉ cách cài.
- `event.rs`: `enum AgentEvent { Started { session_id }, Text(String), Thinking(String), ToolStart { id, kind: ToolKind, label }, ToolDone { id, ok, summary }, FileChanged(PathBuf), Finished { ok, summary, cost_usd: Option<f64>, tokens: Option<u64> }, Error(String) }`.
- `claude.rs`, `codex.rs`: dựng `Command` (cwd = workdir, stdin = prompt, `stdin` đóng sau khi ghi) và hàm `parse_line(&str) -> Vec<AgentEvent>`. **Dòng hoặc loại sự kiện lạ thì bỏ qua (ghi log), không lỗi**, vì định dạng có thể đổi giữa các phiên bản CLI.
- `run.rs`: `AgentRun::start(kind, opts, on_event) -> AgentRun`. Một thread đọc stdout theo dòng → parse → gọi `on_event`; một thread gom stderr (đưa vào `Error` nếu tiến trình thoát lỗi). `cancel()` gửi SIGTERM rồi SIGKILL cho cả process group (CLI hay sinh tiến trình con). `Drop` cũng huỷ, nên đóng tab repo là dừng AI.
- Unit test: parse các dòng JSON mẫu (lưu trong `crates/agent/tests/fixtures/*.jsonl`) cho cả hai CLI, gồm cả dòng hỏng/loại lạ.

### App (`crates/app`)
- `repo_view/agent.rs`: panel AI, là **tab thứ 4 ở cột trái "AI" (Cmd+4)**, cạnh Files · Thay đổi · Git. Sự kiện đi qua `futures::channel::mpsc` vào `cx.spawn_in`, cùng cách watcher đang làm ở `repo_view/mod.rs` (không polling).
  - Trên: chọn CLI (Claude / Codex), ô nhập nhiều dòng (Cmd+Enter để chạy), nút Chạy/Dừng (khoá khi đang chạy), công tắc "Chạy trong worktree mới".
  - Giữa: dòng thời gian dạng danh sách (`uniform_list`): đoạn trả lời, từng bước công cụ (icon + tên file/lệnh + trạng thái), file đã sửa (bấm vào thì mở diff ở cột giữa, dùng `Detail::Working` có sẵn).
  - Dưới: kết quả cuối, chi phí/token nếu có, nút "Hỏi tiếp" (resume cùng phiên) và "Phiên mới".
  - Đủ trạng thái: chưa có CLI (hướng dẫn cài + nút mở Cài đặt), chưa đăng nhập (hiện stderr của CLI), đang chạy, lỗi, rỗng.
- Đang chạy AI: tắt tự refresh dồn dập (gom refresh theo watcher như hiện tại là đủ); khi `Finished` thì refresh một lần và chuyển tab Thay đổi nếu có file đổi.
- Nếu trình soạn đang có tab chưa lưu trùng file AI vừa sửa: giữ nguyên cách xử lý "file bị sửa từ app khác" đang có (không tự ghi đè), hiện cảnh báo.
- i18n: `crates/app/locales/en/agent.json`; thêm phím vào README.

### Chạy trong worktree (khuyên dùng, mặc định bật)
- Bấm Chạy khi bật công tắc: tạo nhánh `ai/<slug-từ-prompt>-<giờ>` + worktree ở `<repo>-worktrees/<tên-nhánh>` bằng `Repo::add_worktree` (`crates/git/src/stash.rs`), chạy CLI trong đó, rồi mở worktree thành tab mới qua `RepoViewEvent::OpenRepository`. Phiên AI gắn với tab worktree.
- Xong việc: người dùng review, commit trong tab worktree, merge về nhánh chính bằng chức năng merge có sẵn, xoá worktree bằng mục Worktree có sẵn.
- Tắt công tắc: chạy thẳng trên thư mục đang làm. Lần đầu tắt thì hỏi xác nhận "AI sẽ sửa trực tiếp file của bạn".

### Cài đặt (`crates/core/src/settings.rs` + `settings_view.rs`)
Thêm vào `Settings` (`#[serde(default)]` nên file cũ vẫn đọc được):
- `agent_cli`: `"claude"` | `"codex"` (mặc định: cái nào tìm thấy trước)
- `claude_path`, `codex_path`: `Option<PathBuf>` ghi đè đường dẫn
- `agent_model`: `Option<String>` cho mỗi CLI
- `agent_worktree`: `bool` (mặc định `true`)
- `agent_allow_shell`: `bool` (mặc định `false`): có cho AI chạy lệnh shell không hỏi hay không (Claude: thêm `Bash` vào `--allowedTools`; Codex: sandbox `workspace-write` vẫn chặn mạng)

Màn Cài đặt có mục "AI": chọn CLI, đường dẫn (kèm trạng thái "Tìm thấy: /opt/homebrew/bin/claude, v2.x" lấy bằng `--version`), model, 2 công tắc trên.

## Các bước

### Bước 1: Lấy định dạng thật
- [ ] 👤 Người làm: trên DG_Mac cài + đăng nhập `claude` và `codex`, chạy mỗi CLI một yêu cầu nhỏ có sửa file và chạy lệnh, lưu JSONL vào `crates/agent/tests/fixtures/claude_edit.jsonl`, `codex_edit.jsonl` (kèm `--version`), gửi lên repo
- [x] Nếu chưa có fixture thật: viết fixture theo tài liệu CLI, đánh dấu cần đối chiếu lại — **Đã làm**: `crates/agent/tests/fixtures/` (claude/codex, ca thành công + lỗi, có dòng lạ), README ghi rõ chưa phải output thật

### Bước 2: Crate `branchpad-agent`
- [ ] Tạo crate, thêm vào workspace `Cargo.toml`
- [ ] `binary.rs` tìm CLI + test
- [ ] `event.rs` + `claude.rs` + `codex.rs` parse theo dòng + test bằng fixture
- [ ] `run.rs` chạy tiến trình, đọc stdout bằng thread, huỷ theo process group, `Drop` huỷ

### Bước 3: Panel AI trong tab repo
- [ ] Tab "AI" ở cột trái + Cmd+4, ô nhập, Chạy/Dừng, dòng thời gian, kết quả
- [ ] Bấm file đã sửa → diff ở cột giữa; xong việc → refresh + chuyển tab Thay đổi
- [ ] Hỏi tiếp (resume) / Phiên mới
- [ ] Trạng thái chưa cài CLI / chưa đăng nhập / lỗi / rỗng; nút khoá khi đang chạy
- [ ] i18n `agent.json`, README (phím Cmd+4, mục AI)

### Bước 4: Chạy trong worktree
- [ ] Tạo nhánh + worktree, chạy CLI trong đó, mở tab worktree, gắn phiên vào tab đó
- [ ] Hỏi xác nhận lần đầu khi chạy trực tiếp trên thư mục đang làm

### Bước 5: Cài đặt
- [ ] Thêm trường vào `Settings` + test đọc file cài đặt cũ
- [ ] Mục "AI" trong màn Cài đặt (chọn CLI, đường dẫn + phiên bản tìm thấy, model, 2 công tắc)

### Bước 6: Nghiệm thu
- [ ] 👤 Người làm: `cargo test -p branchpad-agent -p branchpad-core` và `cargo build -p branchpad` trên DG_Mac, gửi log lỗi biên dịch nếu có
- [ ] 👤 Người làm: chạy thử trên repo thật với cả Claude và Codex: sửa file, Dừng giữa chừng, Hỏi tiếp, chạy trong worktree rồi merge về, đóng tab khi đang chạy (tiến trình phải tắt hẳn: `ps` không còn `claude`/`codex`)

## Không làm (bản này)
- Terminal nhúng, chat nhiều phiên song song trong một tab repo
- Duyệt từng lệnh/từng lần sửa trước khi AI làm (CLI ở chế độ không tương tác không hỗ trợ tốt; thay bằng worktree + review diff)
- Gọi API Anthropic/OpenAI trực tiếp, lưu API key
- Lưu lịch sử phiên AI giữa các lần mở app (CLI tự lưu phiên, chỉ giữ `session_id` trong bộ nhớ)
- Làm sau nếu cần: nút "Viết commit message bằng AI", chuột phải "Nhờ AI sửa xung đột này" / "Giải thích commit này" (dùng lại `AgentRun` của plan này)

## Rủi ro
- Định dạng JSON của CLI đổi theo phiên bản → parse chịu lỗi, fixture có ghi phiên bản, màn Cài đặt hiện phiên bản đang dùng
- AI sửa nhầm code đang làm dở → mặc định chạy trong worktree
- CLI treo hoặc chạy lâu → nút Dừng diệt cả process group; không đặt timeout cứng
- Codex sandbox (Seatbelt) chặn mạng/ghi ngoài repo → báo rõ lỗi từ `turn.failed`, không tự nới quyền

## Tiêu chí nghiệm thu
- Gõ yêu cầu → thấy tiến trình chạy dần, file đổi hiện ở tab Thay đổi khi AI còn đang làm, không polling
- Dừng / đóng tab / thoát app → không còn tiến trình CLI nào
- Chạy trong worktree không đụng tới thư mục đang làm
- Thiếu CLI, chưa đăng nhập, CLI lỗi → báo rõ ràng trong panel, app không crash
