# RFC 0105 的進度：layout 的座標模型和分段

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-19。完成日期：2026-11-19（macOS，Apple M4）。第 3 階段（入口的 break token、一個 box 好幾個 fragment）移到 multi-column 的 RFC（README 的第 23 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a：型別和 converter | 完成 | 2026-11-19 |
| 1b：inline 的行在 box 裡的位置 | 完成 | 2026-11-19 |
| 1c：table，連 collapsed border | 完成 | 2026-11-19 |
| 1d：float、list marker、line clamp、置中、slider 的 thumb | 完成 | 2026-11-19 |
| 1e：第 1 階段的檢查和文件 | 完成 | 2026-11-19 |
| 2a：style：`direction`、`unicode-bidi`、`dir`、`:dir()` | 完成 | 2026-11-19 |
| 2b：邏輯屬性照 `direction`、layout 用 `direction` | 完成 | 2026-11-19 |
| 2c：`unicode-bidi` 在 inline 內容裡 | 完成 | 2026-11-19 |
| 2d：捲動的原點、sticky、`text-overflow`、`<select>` | 完成 | 2026-11-19 |
| 2e：第 2 階段的檢查和文件 | 完成 | 2026-11-19 |
| 3：入口的 break token、一個 box 好幾個 fragment | 移到 multi-column 的 RFC | — |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 座標模型 | 邏輯和物理的換算在每種 writing mode 和 direction 都對（位移、大小、rect、四邊、距離、內容區的起點） | `cargo test -p layout --lib logical::` |
| 座標模型 | machopo 自己的演算法改用邏輯座標之後，橫排、`ltr` 的排版和改之前一樣；collapsed border 給 paint 的格線和每條邊 | `cargo test -p layout -p paint`；`python3 scripts/test.py reftest inline/ text/ block/ position/ tables/ floats/ generated/`；`python3 scripts/test.py wpt css/css-tables/ css/CSS2/tables/` |
| style | `direction`、`unicode-bidi` 的解析和 computed value，`dir`、`<bdi>`、`<bdo>` 的 hint，UA 樣式的 bidi 規則，directionality、`:dir()`、`dir=auto` | `cargo test -p style -p css -p dom`；`python3 scripts/test.py wpt html/dom/elements/global-attributes/ html/rendering/bidi-rendering/ css/css-writing-modes/` |
| style | 邏輯屬性照元素的 `direction` 對應（cascade、`getComputedStyle`、動畫） | `cargo test -p style --test it logical::`；`python3 scripts/test.py wpt css/css-logical/` |
| layout | 段落、flex、grid、table 的欄、marker、靜態位置照 `direction`，視窗照根元素的方向 | `cargo test -p layout --test it direction:: lists:: tables:: block::an_rtl_document_reaches_left` |
| bidi | `unicode-bidi` 的控制字元、強制換行前後關掉重開、行內盒子的邊界在重排之後照方向擺 | `cargo test -p layout --test it direction:: text_rects::`；`python3 scripts/test.py wpt css/CSS2/bidi-text/` |
| 捲動 | `rtl` 和 flex 反過來的軸往起點捲（容器和視窗），`scrollLeft`、`scrollX` 是負的，commit 換成 compositor 的 position | `cargo test -p layout --test it block::`；`cargo test -p renderer --lib scroll::`；`cargo test -p compositor --lib`；`python3 scripts/test.py wpt css/cssom-view/` |
| 其他 | `text-overflow`、line clamp 的省略號、sticky 的 inset、`<select>` 的清單照方向 | `cargo test -p layout --test it text_overflow:: line_clamp:: sticky::`；`cargo test -p shell --lib popup_menu`；`cargo test -p shell --test it browser_view::a_right_to_left_dropdown` |
| 視窗 | `rtl` 的頁面和容器從右邊開始、wheel 往左捲到負的位移（走真的 compositor），`about:demo` 的由右到左那一段 | `python3 scripts/test.py scenarios rtl-scroll direction`；`python3 scripts/test.py scenarios --window` |

## 做過的檢查

macOS、Apple M4，2026-11-19。

### 測試和 mutation test

每一段都照 `docs/testing.md` 跑了 mutation test（`python3 scripts/test.py mutate`，spec 是這個資料夾的 `mutants-*.toml`），共 200 個：抓到 194 個，6 個等價，記在 `docs/test-gaps.md`（inline 往上交的靜態位置用別的 writing mode；bidi 的結尾控制字元在 tag 之前或之後、強制換行前關掉的順序、只有控制字元的行；`rtl` 截行的 run 多一個 cluster 邊界；截斷的 run 在 compositor 的 hit 範圍）。第一輪沒抓到的都補了測試，主要是 layout 的測試沒直接看的資料（collapsed border 的格線、欄的背景、caption、marker、thumb 的位置只有 paint 看得到）、`rtl` 才走得到的路，和只有原點不是 0 才看得出的捲動。另外有 4 處第一輪沒抓到、查了之後是多餘的程式，拿掉了（matched properties cache 的 hash 加上方向、`:dir()` 在組合子左邊收狀態、動畫的 `visit` 再對應一次邏輯屬性、highlight 的 cascade 的對應）。

### 找到的 bug

- `animation` 原本的：轉場或 CSS 動畫在結束前 1µs 以內的 frame 結束時，`finish` 的事件時間夾到 `now`、`transitionend` 的沒有，順序顛倒（1e 找到，另外在 `main` 上修）。
- host 的方向改了，裡面的 slot 沒跟著改；改 `dir` 的路壓掉了兄弟的重算（`:not(:dir(ltr)) + div`）；keyframe 照 `ltr` 換成物理屬性（2a、2b）。
- `unicode-bidi` 放在 `RareNonInherited` 讓每個 block 都複製那一組（2a）。
- 強制換行沒有關掉、重開 embedding；只有控制字元的行有高度；保留的換行在 run 中間時插入的控制字元讓切 run 的字串 panic（WPT 的三份 crash）；行內 out-of-flow 盒子的靜態位置沒照重擺之後的（2c）。
- 2b 讓 layout 照 `direction` 之後，`::placeholder` 跟著頁面的 `direction: rtl`（2c 找到，照 Blink 的 UA 樣式修）、`rtl` 的 line clamp 的省略號還在右邊（2d 找到）。
- 視窗沒有照根元素的 `direction`，`rtl` 的文件比視窗寬的根元素從左邊排（2e 找到，`Document::PropagateStyleToViewport`）。

### WPT

- 加進清單的：`css/css-writing-modes/parsing/` 的 `direction-*`、`unicode-bidi-*`；HTML 的 directionality 和 `:dir()` 的 127 份（`dir-tests.txt`）；`html/rendering/bidi-rendering/`；`css/CSS2/bidi-text/` 和 css-writing-modes 的 `bidi-*`、`unicode-bidi-*`（85 份）；`css-overflow` 的三份 `rtl` 捲動。沒過的寫在 `.ini`：被 bidi 拆成不相連的幾段的盒子畫成一個、`plaintext` 整段當成一段、`ch` 單位、`innerText`、直排。
- 變成通過的（拿掉或改了 `.ini`）：`css-logical` 的 27 個 `rtl` 的 subtest、table 的欄從右排、`scrollLeftTop` 的 `rtl`、`scrollIntoView` 的橫排 `rtl`、`text-overflow` 和 line clamp 的 `rtl`、sticky 的 `rtl` 過度限制、`getComputedStyle` 的 `rtl` inset、scroll anchoring 的橫排 `rtl`、shadow tree 的 directionality 等。寫 machopo 沒有 `direction`、`:dir()`、bidi 的 `.ini` 都看過一遍，改成實際的原因。
- 原本碰巧通過、照方向之後變成預期失敗的：`input-text-overflow-ellipsis-rtl-scrolled`（machopo 不能捲文字框的值，和 `ltr` 的那份一樣）、`scrollIntoView-sideways-lr` 的 `rtl`（沒有 `writing-mode`）。
- 2e 跑的目錄（`css/CSS2/`、`css-position`、`css-writing-modes`、`css-overflow`、`cssom-view`、`css-flexbox`、`css-grid`、`css-text`、`css-tables`、`css-lists`、`css-logical`、`css-scroll-anchoring`、`css-shadow`、`css-variables`、`cssom`、`css-pseudo`、`html/rendering/`、`html/dom/elements/global-attributes/`，32473 項）沒有不預期的失敗。不預期的通過都是 macOS 上已知的、和這份無關的（`floats/new-fc-*`、`margin-collapse-009`、`clear-on-parent-and-child`、`text-overflow-020`、`html/rendering/widgets/` 的兩份），`text-overflow-012`、`022` 在 1d 之前就失敗，也一樣；沒有改它們的預期結果。

### 不穩定的測試

沒有新的。1e 的 `web_animations::ready_then_the_end` 是 `animation` 原本的時間問題（上面的第一個 bug），在 `main` 上修好。

### 跑過的檢查

- 每個階段做完時：`cargo test --workspace`、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`。clippy 找到的都改了寫法：`WritingModeConverter` 的 `to_*` 取 `self`（1e）；`cascade` 裡多餘的 `clone`、bidi 控制字元的 tuple 改成型別別名（2e）。
- 真視窗的情境（`scenarios --window`，2e）：70 次全部通過。
- 最後（第 2 階段 merge 回 `main`、rebase 之後）：`cargo test --workspace` 4194 個通過（57 個測試 binary），`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通過；`sanity.py` 的 `deps`、`unsafe`（動到 `ComputedStyle`）、`docs`。
- parley 的 fork 在 2c 試過一個 commit（開頭的邊界拿後面那段內容的 level），改成在 machopo 重擺之後丟掉，沒有 push、`third_party.toml` 沒有變。
- 沒有動到 GC：`matrix`、`fuzz` 照 CLAUDE.md 由每週的 CI 跑。
