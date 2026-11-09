# RFC 0042 的進度：表單的簿記改成元素層級

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-13。完成：2026-10-13，Fedora 44（Linux x86_64，AMD Ryzen AI 9 HX 370；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0042` 上做兩段，各自做完時 merge 回 `main`；原本的第 3 段量過之後不做（README 的「已決定的事項」第 4 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 `FormChange` 改成元素層級：`dom` 記元素，`listed_element`、`select` 照著處理，select 每個 batch 一次；量測 | 完成 | 2026-10-13 |
| 2 `FormStateChange` 改成元素層級，fieldset 只在 legend 進出時記；量測 | 完成 | 2026-10-13 |
| 3 插入時的走訪合成一趟 | 量過之後不做（第 4 條） | 2026-10-13 |

## 驗收

### `dom` 記什麼

| 項目 | 怎麼驗證 |
| --- | --- |
| 插入、移除記下子樹裡每個 form owner 和 select 可能有關的元素（form、listed element、option，出現過 `form` attribute 之後加上有 id 的），照樹的順序，不記其他的和 shadow tree 裡的；移除的帶子樹的根和原本的 parent | `cargo test -p dom --test it tree::form_elements_are_recorded_one_by_one` |
| form 的 pseudo-class 的紀錄：子樹裡每個會被比對的元素，`<legend>`（不是其他的 child）進出 fieldset 記那個 fieldset，textarea 的 child 記 textarea | `cargo test -p dom --test it tree::form_state_elements_are_recorded_one_by_one` |

### `dom_api` 的結果

| 項目 | 怎麼驗證 |
| --- | --- |
| form owner、radio 的組、`<select>` 的 selectedness、form 的 pseudo-class 照舊；同一個呼叫裡又插入一次的是最後插入的（不是樹的順序上最後的） | `cargo test -p dom_api --test it forms::`（`inserted_again_later_in_one_call`、`a_subtree_changed_twice_in_one_call`、`disabled_and_enabled` 等） |
| 行為不變 | `cargo test --workspace`；`python3 scripts/test.py wpt html/semantics/forms/ dom/nodes/ css/selectors/ html/semantics/selectors/ shadow-dom/` |

## 做過的檢查

- **做了什麼**：
  - 第 1 段：`dom` 的 `FormChange::Inserted(element)`、`Removed { element, root, parent }` 每筆一個元素，`record_form_elements` 在 `insert_before` 接上之後、移除拆開之前沿 child、sibling 的 edge 走那時的子樹（不配置）。`dom_api` 的 `listed_element::update_all` 不再走子樹：`not_yet` 是「這個元素在後面還有 `Inserted`」；form 被移走時外面的元素每個被移走的子樹重設一次，`form` attribute 指的元素一個 batch 每棵樹一次（`Batch`）；`select` 的 `inserted`、`removed`、`selects_around` 換成 `option_inserted`、`option_removed`，option 清單改了的 select 在 batch 的最後各跑一次 selectedness setting。select 本身被插入不記：它的 option 連同它插入時各自找到它。
  - 第 2 段：讀了 Blink 的 `HTMLFieldSetElement::ChildrenChanged`（每次 children 改變重新判斷各個 legend 的後代，不是整個 fieldset），之後決定第 10 到 12 條。`dom` 的 `FormStateChange::Inserted(element)`、`Removed { element, parent }`、`Legends(fieldset)`，走子樹抽成 `for_each_inclusive_descendant`。`form_states` 每筆一個元素（`node_by_id`，不建 handle），`Legends` 走各個 legend 的子樹；`children_changed` 拿掉。
  - 第 3 段（量過之後不做）：第 2 段之後的 frame pointer 的 `perf`（5 個 iteration）：jQuery 的 `insert_before` 佔 renderer main thread 12.7%，其中 `adoption_targets` 4.6%、`handle_of` 1.5%、self（含 inline 的兩趟表單走訪）1.4%、`record_frame_owners` 0.7%；React-Complex-DOM 的 `insert_before` 0.8%。
  - 文件：`crates/dom/README.md`（form owner 和 form 的 pseudo-class 的紀錄）、`crates/dom/soundness.md`（表單的走訪）、`crates/dom_api/README.md`（`listed_element`、`select`、`form_states`）、`docs/testing.md`（等價的 mutation）。
- **量測**（`perf stat -e instructions:u,cycles:u`，整個 kun process，5 個 iteration；前一個狀態的 `main` 放在 `../kun-main` 的 worktree 編，兩邊輪流各量兩次）：

  | Suite | 第 1 段（`main` 是 `55d426fa`） | 第 2 段（`main` 是 `a2da1098`，含 RFC 0041 的 `calc()`） |
  | --- | --- | --- |
  | TodoMVC-jQuery | 62.9、61.9 G → 57.8、57.6 G（−7.5%），cycles −6% | 57.8、57.1 G → 55.2、55.0 G（−4%），cycles 20.6、20.1 G → 19.7、20.1 G |
  | TodoMVC-JavaScript-ES5 | 20.0、20.1 G → 18.7、22.2 G（起伏裡） | 20.3、20.5 G → 20.4、19.3 G（起伏裡） |
  | TodoMVC-React-Complex-DOM | 51.5、51.8 G → 51.1、51.0 G（−1%） | 53.4、55.2 G → 55.2、55.6 G（起伏裡，偏高一點） |

  時間（`ci.py speedometer --browser kun --rounds 1`，第 1 段，兩邊輪流各兩次）：jQuery 734.4、760.4 → 697.4、703.4 ms（−5% 到 −8%）；ES5 197.6、161.8 → 150.6、179.2 ms（兩邊都是雙峰，看不出）。
- **測試**：`dom` 的 `tree::form_elements_are_recorded_one_by_one`、`tree::form_state_elements_are_recorded_one_by_one`；`dom_api` 的 `forms::inserted_again_later_in_one_call`。
- **mutation test**：
  - `mutants-1.toml`（15 條）：第一次漏了「元素永遠不算已經插入」（option 在自己的紀錄也跳過「取消其他的」，batch 最後的 selectedness setting 留下樹的順序上最後一個，測試裡兩者剛好一樣），補了「插到前面」的斷言；之後 15 條都抓到（`--keep-going`，96 秒）。
  - `mutants-2.toml`（8 條，91 秒）：全部抓到。
  - 等價、沒放進 spec 的：同一棵樹的 `form` attribute 重設、同一個被移走的子樹的 form 不去重（再做一次，每個元素拿到一樣的 owner），記進 `docs/testing.md`。
- **跑過的**（第 2 段之後；第 1 段 merge 前也在 rebase 到 RFC 0041 的第 2 段之後跑過一次）：`cargo test --workspace --no-fail-fast` 43 個 binary、2526 個通過；`cargo fmt --check`（連同 `sdk/lua/generator`）；`cargo clippy --workspace --all-targets -- -D warnings`；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`；`test.py wpt html/semantics/forms/ dom/nodes/ css/selectors/ html/semantics/selectors/ shadow-dom/`：1017 個測試、20500 個檢查。
- **不穩定的測試**（不是這份造成的，原因還沒找到）：上面那組 WPT 一起平行跑時，`focus-dynamic-type-change-on-blur.html`（TIMEOUT）和 `focus-dynamic-type-change.html`（`:focus-visible` 不成立的 FAIL）時有時無。第 2 段跑了 5 次：前者 5 次、後者 4 次；`main`（第 1 段）跑了 4 次：前者 4 次、後者 2 次。只跑 `html/semantics/forms/` 兩次、單獨跑後者 3 次都通過。兩邊都會，只在負載重時：可能和 page 的 system focus 或 focus ring 的判斷在等待中被改變有關，要再查。
- **沒跑的**：`cargo clean`（使用者決定這次不跑）。
