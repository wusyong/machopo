# RFC 0092 的進度：taffy 和 parley 的 fork：跟著上游、修改留成 patch、減少 workaround 的成本

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-10，Windows 11。在 `0092` 上做。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 taffy fork 的基礎設施：fork、path 依賴、patch 的匯出和檢查、parley 現在的 patch、文件 | 完成 | 2026-11-10 |
| 2 計數 | 還沒開始 | |
| 3 重寫 parley 的 fork | 還沒開始 | |
| 4 taffy 跟上上游的 main | 還沒開始 | |
| 5 machopo 裡有成本的 workaround | 還沒開始 | |
| 6 搬進 taffy 的 fork | 還沒開始 | |
| 7 上游的草稿 | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| taffy 的 fork 的 `machopo` 分支是上游的 v0.15.0 加上 `README.machopo.md` 的 commit，source 和 crates.io 的 0.15.0 一樣 | `diff -r` 比 fork 的 `src/` 和 cargo 的 registry 裡的 `taffy-0.15.0/src/`、`Cargo.toml` 和 `Cargo.toml.orig`：一樣；`Cargo.lock` 只少了 taffy 的 `source` 和 `checksum`，依賴的版本不變 |
| machopo 用 path 依賴編 fork，排版不變 | `cargo test -p layout`、`cargo test --workspace` |
| `ci.py patches` 匯出：一個 commit 一個 patch、不編號、沒有簽名、完整的 blob id、內容每次一樣、換掉舊的、CRLF 的檔案和二進位檔照原樣、shallow 的 checkout 先加深、不是從 `upstream_rev` 長出來的不匯出、checkout 不在 pin 上時不匯出 | `python3 scripts/ci_patches_test.py`（`Patches`、`Commands`） |
| `ci.py third-party` 檢查 patch：沒匯出的 commit、被改過的、套不上的（指出是哪一個）、有 patch 卻沒有 `upstream_rev` 都報出來；checkout 的 index 和檔案不動；shallow 的 checkout 只拿 `upstream_rev` 這一個 commit | `python3 scripts/ci_patches_test.py`；在這台機器上 `python3 scripts/ci.py third-party` 對 taffy 和 parley 通過 |
| shallow 和完整的 checkout 匯出的 patch 一樣 | parley 的 checkout 原本是 shallow 的，第一次匯出時加深；之後在完整的 checkout 再匯出一次，每個檔案的 md5 不變 |

## 做過的檢查

**第 1 段**（2026-11-10，Windows 11）：

- **fork**：`gh repo fork DioxusLabs/taffy` 建 `wusyong/taffy`；`machopo` 分支從 v0.15.0（`eb645069`）開出來，一個 commit（`bfa9eda2`，`README.machopo.md`），push 到 fork。
- **source 一樣**：fork 的 `src/` 和 crates.io 的 0.15.0 逐檔一樣，`Cargo.toml` 和 crates.io 的 `Cargo.toml.orig` 一樣。第一次比的時候每個檔案都不同：這台機器的 `core.autocrlf=true` 讓 clone 的檔案變成 CRLF；`core.autocrlf false` 之後重新 checkout 就一樣了。所以這一段沒有跑 reftest 和 WPT：編出來的程式和之前同一份 source、同樣的依賴。
- **patch**：taffy 1 個、parley 16 個（parley 現在的 commit，原樣匯出；第 3 段重寫）。`patches/` 標成 `-text`：上游有 CRLF 的檔案（taffy 的 `CONTRIBUTING.md`、parley 的幾個字型的 LICENSE），改到時 patch 帶著 CR，換成 LF 就套不上。
- **新的測試**：`scripts/ci_patches_test.py`，15 個，加進 CI 的 `lint`。
- **mutation test**（`mutants-1.toml`，`python3 scripts/test.py mutate rfcs/0092-taffy-parley-forks/mutants-1.toml`）：16 條全部抓到。第一次跑 17 條、3 條沒抓到：
  - 拿掉 `--binary`：等價，`format-patch` 本來就給二進位的 diff。改成不傳這個選項，註解寫明，這條 mutation 拿掉。
  - 檢查用 checkout 自己的 index：測試的 index 本來就等於 HEAD，用自己的 index 讀樹、套 patch 之後還是一樣。改成先 stage 一個改動，確認檢查之後 staged 的內容還在。
  - 抓 `upstream_rev` 時連同它的歷史：結果一樣，只是多下載。補上 shallow 的 checkout 沒有 `upstream_rev` 的 parent 的斷言。
- **跑過的檢查**：`cargo test -p layout`；`cargo fmt --check`；`cargo clippy --workspace --all-targets -- -D warnings`；`python3 scripts/sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`python3 scripts/ci.py third-party`；`cargo test --workspace`（55 個 test binary，3739 個通過、1 個 ignored；`bindings` 的 `gc_platform` 印出的 panic 是測試本來就要的）。
