# RFC 0061 的進度：bindings 的效能

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-19。完成：2026-10-20，Fedora 44（AMD Ryzen AI 9 HX 370；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0061` 上做；第 1、2 段做完時各自 merge 回 `main`（`b45a01d4`、`ef83f611`）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 路上多做的事：wrapper 的 template 照種類查、overload 只在有的成員上找、特殊操作照介面存著、少配置 | 完成 | 2026-10-19 |
| 2 針對量出來的大頭（第 1 條）：節點記住種類、overload 照成員的索引、呼叫的收尾先看一眼；DOM 狀態的查找查過不做 | 完成 | 2026-10-19 |
| 3 專用的 thunk 或 canvas 2D 的 Fast API call | 量過，不做（第 2 條） | 2026-10-20 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| wrapper | 新的 wrapper 是它最衍生的 interface 的實例（照種類拿 template） | `cargo test -p dom_js`；`test.py wpt dom/ webidl/ html/dom/` |
| 成員的呼叫 | overload 的 form 照參數選，function 的 `length` 是最短的 form 的；只有標成 overload 的成員才找，照介面和成員的索引拿 | `cargo test -p dom_js`（canvas 的 `drawImage` 這類） |
| | receiver 照 interface 能收的型別 unwrap；static 方法的 receiver 是 window | `cargo test -p dom_js --test it -- bindings::` |
| | 可變參數、省略的 optional 參數照舊 | `cargo test -p dom_js` |
| indexed、named | getter、setter、deleter、`length`、supported names 照 interface 的索引找，結果和用名稱找一樣 | `cargo test -p dom_js`；`test.py wpt dom/ html/dom/` |
| 節點的種類 | 第一次算完記在節點上，之後直接讀；debug build 照名稱再算一次比對 | 所有 debug build 的測試（`dom_api`、`dom_js`、WPT 經過 `Object::kind` 的都比對） |
| 呼叫的收尾 | 只在有事要做時跑：元素的變化、表單狀態、mutation observer 和 custom element 的 microtask、`selectionchange` 各自都會讓它跑 | `cargo test -p dom --test it -- selections::a_scheduled_selectionchange_is_a_calls_effect custom_elements::reactions_are_queued`；`cargo test -p dom_api -p dom_js` |
| 效能 | 和開始前比：TodoMVC-jQuery 的 cycle 約 −13%、ES5 約 −7%、Charts-chartjs 約 −4%；bindings 佔全部 cycle 13.4% → 11.5% | `ci.py speedometer --sample --against main --suites …`（使用者要求時量，數字在「做過的檢查」） |

## 做過的檢查

**完成時（2026-10-20，Fedora 44）**：第 2 段的程式（`ef83f611`）做完時跑過 `cargo test --workspace`（149 秒）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`（修掉一處排版）和 `sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`，之後只改了文件：`sanity.py docs` 通過。沒有動到 GC 的 `unsafe`（節點的種類是一個 `Cell<u16>`，`#[no_trace]`），`matrix`、`fuzz` 照常由 CI 跑。

**WPT**：第 1 段 `dom/`、`webidl/`、`html/dom/`、`html/canvas/element/`（1888 個測試），第 2 段再加 `custom-elements/`、`selection/`（2278 個測試），都沒有非預期的失敗。第 1 段時 `html/dom/reflection-metadata.html` 的 `base.href`、`base.target` 和 `dom/nodes/Node-cloneNode.html` 的 `createElement(base)` 非預期地通過，是 `main` 的 `bd691df4`（`HTMLBaseElement` 的 `href`、`target`）沒有一起更新預期結果，和這份 RFC 無關，merge 時更新（`8a8742e8`）。

**mutation test**（spec 不放 repo）25 條：
- 第 1 段（`cargo test -q -p dom_js`）14 條都抓到：新的 wrapper 拿第一個介面的 template、沒有成員被標成 overload、呼叫不找 overload、function 的 `length` 不看 overload、overload 的 bit 留在索引裡、indexed setter、named setter、supported names 找不到、named deleter 是 getter、`length` 是 indexed getter、每個介面的特殊操作都是第一個的、receiver 照下一個介面的型別 unwrap、可變參數少第一個、省略的 optional 參數照樣轉換。
- 第 2 段（`cargo test -q -p dom_api -p dom_js`，後來加上 `-p dom`）11 條：一開始抓到 7 條（節點的種類記錯、還沒記就當成記了、overload 都記在第一個成員下、收尾從不做事、元素的變化、表單狀態的變化、mutation observer 的 microtask 不算收尾的事）。沒抓到的 4 條：
  - `selectionchange`、custom element 的 backup microtask 不算收尾的事：`dom_api`、`dom_js` 的測試沒有只留下這一項的呼叫（`selectionchange` 的端對端測試在 `shell`；目前帶 `[CEReactions]` 的成員都用自己的 element queue，backup queue 只有不帶的才用到）。補了 `dom` 的 `selections::a_scheduled_selectionchange_is_a_calls_effect` 和 `custom_elements::reactions_are_queued` 裡 `has_call_effects` 的斷言，重跑抓到。
  - 呼叫時照下一個成員找 overload：JS 只裝 overload 的第一個 form，下一個成員是同一組的另一個 form，結果一樣（寫錯的 mutation）；改成照下一個 interface 找，抓到。
  - 已經排了 mutation observer microtask 還算要排：多排的那個執行時待處理的 observer 已經被前一個取走，什麼都不做，callback 和順序都看不出差別（等價）。

**取樣**（`ci.py speedometer --sample --against main`，各輪流三次）：

| suite | 第 1 段 cycle | 指令 | 第 2 段 cycle | 指令 |
| --- | ---: | ---: | ---: | ---: |
| TodoMVC-jQuery | −8.6% | −8.0% | −4.9% | −2.1% |
| TodoMVC-JavaScript-ES5 | −5.2% | −3.8% | −2.1% | −3.1% |
| Charts-chartjs | −0.9% | −2.6% | −2.9% | −3.2% |

- 第 1 段：jQuery 的 `memcmp` 1387 → 426、`new_wrapper` 的介面查找 546 → 0、`indexed_item` 的介面查找 546 → 0；chartjs 的 `convert_args` 382 → 181、`overloads` 107 → 25。
- 第 2 段：jQuery 的 `element_kind` 226 → 0（新的 `node_kind` 自己 198，幾乎都在建 wrapper 時第一次判斷）、`V8Host::mutate` 263 → 24；chartjs 的 `V8Host::mutate` 360 → 42，`update_elements`、`queue_selectionchange_tasks`、`update_form_states` 都到 0。
- 兩段乘起來約 jQuery 的 cycle −13%、指令 −10%，ES5 −7%、−7%，chartjs −4%、−6%（兩輪各自的基準，近似）。
- 2026-10-20 對 20 個 suite 再取樣一次，決定第 3 段（`details.md` 的「第 2 段之後」）。

**跑過的測試**：第 1 段 `cargo test -p dom_api -p dom_js -p dom_wasm`（366 秒），階段做完時 `cargo test --workspace`（271 秒）；第 2 段 `cargo test -p dom -p dom_api -p dom_js -p dom_wasm`（55 秒），階段做完時 `cargo test --workspace`（149 秒）。clippy 乾淨；`cargo fmt --check` 各修掉一兩處排版。第 1 段的 `sanity.py boundary` 抓到兩個新的 static（`templates.rs` 的 `RECEIVERS`、`properties.rs` 的 `SPECIALS`，同一個 process 的 renderer 共用）：都是從靜態的宣告表算出來、不會變的，和 `dom_api` 的 `GROUPS` 一樣加進 `STATICS_ALLOWED`。
