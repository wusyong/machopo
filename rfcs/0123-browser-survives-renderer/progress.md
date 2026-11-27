# RFC 0123 的進度：renderer 送什麼都不能讓 Browser 結束

開始日期：2026-11-27。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點、探測（`details.md` 的「第 0 步的探測」「Chromium 怎麼做」）、已決定的事項第 1 到 6 條 | 做完 | 2026-11-27 |
| 1a. `protocol::viz` 的 `CompositorFrameSinkMsg::check`、`MAX_QUADS`、`MAX_RENDER_PASSES`；Browser 的 IO thread 解開就檢查、不過的是 `BadMessageReason::FrameSink`；Viz 收到的每個 frame 照同樣的檢查、只有 root 嵌別的 surface；renderer 的 compositor 守上限、debug build 送出前檢查 | 做完 | 2026-11-27 |
| 1b. 測試、第 1 段的檢查和文件 | 做完 | 2026-11-27 |
| 2. wgpu 的錯誤接住；換掉 `create_buffer_init` | 還沒開始 | |
| 3. renderer 的 tile 的記憶體預算；量測；Viz 的 shared image 的上限 | 還沒開始 | |
| 4. panic 的 lint；Viz 的 fuzzer；CI | 還沒開始 | |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 說不通的訊息 | 每一種（沒有 pass、root 的輸出是空的或 NaN、pass、quad、shared quad state 超過上限（剛好在上限的收）、quad 的 state 和 resource 不是 frame 的、resource 的 id 重複、resource 的像素和大小不合或帶著 shared image 又帶像素、超過 `MAX_TEXTURE_SIZE`、renderer 嵌 surface、`CreateSharedImage` 的大小和像素）被擋；誠實的 frame、NaN 和無限大的座標收 | `cargo test -p protocol --lib viz::` |
| 壞訊息 | renderer process 送說不通的 frame、shared image、嵌別人 surface 的 frame 時，分頁收到 `BadMessageReason::FrameSink`、process 被結束、Viz 沒收到；說得通的到 Viz、從綁給它的 sink | `cargo test -p browser --test it renderer_process::what_viz_is_sent_that_cant_be_right_ends_its_renderer` |
| Viz | 只有 root 的 frame 嵌別的 surface；說不通的 frame 丟掉、上一個留著；帶像素的 shared image 的 resource 整個 frame 不收、沒建過的 mailbox 不畫 | `cargo test -p viz --lib`（`a_frame_that_cant_be_right_is_dropped`、`a_frame_of_bad_resources_is_dropped`、`a_shared_image_nobody_made_isnt_drawn`） |
| compositor | quad 超過上限時丟掉最底下的、沒有 quad 的 state 拿掉，剩下的 Viz 收、每個 quad 的 state 不變；只有 state 超過時也一樣；在上限以內不動；每個測試的 frame 都過檢查（debug build 的 assert） | `cargo test -p compositor --lib` |

## 做過的檢查

### 0（2026-11-27，Fedora 45，Linux 7.3.3，x86-64，AMD Radeon 890M 和 llvmpipe）

探測的結果在 `details.md` 的「第 0 步的探測」；探測的程式是 `probe.rs`、`probe_shell.rs`，跑完從 `viz` 和 `shell` 拿掉了。

### 1a 和 1b（2026-11-27，同一台）

- `cargo test -p protocol --lib viz::`、`cargo test -p viz`、`cargo test -p compositor`、`cargo test -p browser --test it renderer_process::` 通過；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。
- 和決定的表不同的一處：shared quad state 的上限是 `MAX_QUADS` 加 1，不是「quad 數加 1」：renderer 的 compositor 每個有 clip 的 layer 給一個 state，那個 layer 的 tile 一個都沒畫時 state 照樣在（`details.md` 的「renderer 的 compositor 送多少」），照 quad 數會讓誠實的 frame 變成壞訊息。
- 加了決定的表沒有的一項：renderer 的 frame 嵌別的 surface 是壞訊息。讀程式找到的：Viz 以前照收任何 sink 的 `Material::Surface`，renderer 能把別的分頁（或 Browser 的 UI）的畫面嵌進自己的 frame；renderer 的 compositor 從來不嵌（`set_surface_layers` 只有 Browser 的 UI 用）。之後有 OOPIF 時照 Chromium 的 frame sink 的階層（`RegisterFrameSinkHierarchy`）放行。
- RFC 的「測試」寫的 `shell` 的整合測試（假的 renderer 送不合格的 frame，看 Browser 還在、另一個分頁照畫）沒有寫：`machopo` 的執行檔裡的 renderer 送不出說不通的 frame，要假的 renderer 就得在執行檔裡留一條測試用的路。改成 `browser` 的整合測試：假的 renderer process（`renderer_process::child` 的角色）送，看分頁收到壞訊息、process 被結束、Viz 沒收到；Viz 沒收到，其他分頁畫什麼就和它無關。
- **mutation test**（`python3 scripts/test.py mutate rfcs/0123-browser-survives-renderer/mutants-1.toml`）：32 個全部抓到。`protocol` 的檢查每一條拿掉、每個上限多一個和少一個、quad 只數 root 的、嵌 surface 一律放行、shared image 的大小和像素不檢查、送來的 frame 不檢查（`protocol` 的 `viz::` 測試）；IO thread 不檢查、把 renderer 當成能嵌 surface、壞訊息不告訴分頁、壞訊息照樣送給 Viz（`renderer_process::what_viz_is_sent_that_cant_be_right_ends_its_renderer`）；Viz 不檢查、任何 sink 都能嵌、root 也不能嵌（`viz` 的 `display::`）；compositor 不守上限、只看 quad、不丟 quad、quad 的 state 沒有跟著換號、留著所有的 state（`a_frame_keeps_within_what_viz_takes`）。跑完 32 個 `from` 都在原處。
- 沒有放進 mutation test 的兩處，只靠審查（`docs/test-gaps.md`）：compositor 的 `generate_compositor_frame` 呼叫 `fit_to_limits`（測試沒有超過 262,144 個 quad 的頁面：一個可見的 tile 一個 quad，要 26 萬個 tile）；送出前的 debug build 的 assert（誠實的 compositor 不會觸發，拿掉它沒有測試會失敗）。
- 第 1 段做完時：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（新的測試的 `single_match` 改掉之後）、`cargo test --workspace --no-fail-fast`（5006 個通過、0 個失敗、1 個 ignored）通過。
