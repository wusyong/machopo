# RFC 0074 的進度：整數運算寫明溢位時怎麼辦，信任邊界上設上限

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-22，Fedora 44。在 `0074` 上做。

完成日期：2026-10-22，Fedora 44（AMD Ryzen AI 9 HX 370）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 基礎：`base` crate（`numerics`、`SequenceNumber`、`AtomicSequenceNumber`）、lint 的設定、`sanity.py numerics`、CI 和 `CLAUDE.md`；改寫 `dom`、`bindings`、`protocol`；`fetch_add` 的 18 處（第 12 條） | 完成 | 2026-10-22 |
| 2 網頁能讓程式結束的：canvas 對 GPU、`drawImage` 的 canvas 來源、表格和 grid、其他網頁碰得到的溢位（第 5 節的 1、2、5、6；第 13、14 條） | 完成 | 2026-10-22 |
| 3 邊界的驗證和範圍：viz 收 frame 時驗證、CSS 長度和字級的範圍、`calc()` 的非有限值、automation 的視窗大小（第 5 節的 3、4、7；第 15 到 17 條） | 完成 | 2026-10-22 |
| 4 其餘的 crate 分四批改寫（4a 繪圖、4b style 和 layout、4c DOM、4d 其他），刪掉 `NOT_CONVERTED`，更新第 6 節的文件（第 18、19 條） | 完成 | 2026-10-22 |

## 驗收

**lint 和檢查**

| 項目 | 怎麼驗證 |
| --- | --- |
| 四個 lint 在 workspace 打開，每個 crate 都受管（測試除外）；例外都是帶理由的 `#[expect]`；atomic 的 `fetch_add` 被擋 | `cargo clippy --workspace --all-targets -- -D warnings`（CI 每次 push） |
| 每個根有測試的那一行，`allow` 只在那裡，`expect` 有理由，沒有 crate 整個不受管 | `python3 scripts/sanity.py numerics` 和它的測試 `python3 scripts/sanity_numerics_test.py`（10 個；CI 的 `sanity` job 兩個都跑） |

**`base`**

| 項目 | 怎麼驗證 |
| --- | --- |
| float 轉整數飽和、NaN 是 0；四種取整；整數之間（十種，包括 `isize`、`usize`）飽和；f64 轉最接近的 f32 | `cargo test -p base`（`floats_saturate_and_nan_is_zero`、`rounding`、`integers_saturate`、`f64_to_the_nearest_f32`） |
| `SequenceNumber`、`AtomicSequenceNumber` 依序發號、用完時 panic、跨 thread 不重複 | `cargo test -p base`（`one_after_another`、`running_out_panics`、`running_out_panics_on_any_thread`、`no_number_twice_across_threads`） |
| `gfx::Size::checked_area`、`checked_byte_size` 在乘積溢位時是 `None` | `cargo test -p gfx --lib checked_area_and_byte_size` |

**網頁能讓程式結束或卡住的**

| 項目 | 怎麼驗證 |
| --- | --- |
| 寬或高超過 8192 的 canvas 不當成 layer、畫進 tile，畫出來一樣；16385 寬的 canvas 不再讓 wgpu panic | `cargo test -p renderer --lib compositing::tests::a_canvas_too_big_for_a_texture_is_painted_into_the_page`；`cargo test -p shell --test it -- limits::a_canvas_too_big_for_a_texture` |
| 大小不合法的 canvas 當 `drawImage`、`createPattern` 的來源：不配置、不畫，圖案什麼都不畫（Chrome 152） | `cargo test -p dom_api --test it -- canvas_images::a_canvas_of_no_pixels_draws_nothing` |
| 表格只看跨列的 cell 找空欄：一百萬欄、一萬列的表格很快排完；交錯的 rowspan、colspan 的位置和 Chrome 152 一樣 | `cargo test -p layout --test it -- tables::many_columns_and_many_rows tables::cells_skip_the_columns_spanned_from_above` |
| `grid-template-areas` 超過 65535 格：computed value 照原樣（Chrome 152）、名字用 hash map 找、交給 taffy 時飽和 | `cargo test -p css --lib grid::`；`cargo test -p layout --test it -- grid::a_template_past_taffys_tracks` |
| `double` 的邊框不溢位（現在也先被夾到 CSS 的範圍） | `cargo test -p paint --test it -- artifact::shades_and_double_lines_follow_blink` |
| `<select size>` 很大時 PageUp、PageDown 到最遠的那一個 | `cargo test -p dom_api --test it -- select_type::a_page_past_the_list` |
| timer 的 handle 到 `i32::MAX` 繞回 1、跳過還在用的 | `cargo test -p dom --test it -- timers::handles_go_round_past_those_in_use` |
| `popup_menu` 很大的滾輪位移捲到最後一頁 | `cargo test -p shell --lib popup_menu::` |

**邊界的驗證和範圍**

| 項目 | 怎麼驗證 |
| --- | --- |
| Viz 丟掉像素和大小對不上、或超過 texture 上限的 frame；這種 shared image 不建 | `cargo test -p viz --lib -- display::tests::a_frame_of_bad_resources_is_dropped wgpu_renderer::drawing_tests::a_shared_image_of_pixels_that_dont_fit_isnt_made` |
| CSS 長度和百分比夾到 ±33554428、字級到 0 到 10000、NaN 是 0；`calc()` 的 `infinity`、`NaN`、除以 0，specified 和 computed 都和 Chrome 152 一樣；函式算完才夾 | `cargo test -p style --test it -- calc::huge_and_non_finite_values_as_chrome_has_them`；`cargo test -p css --lib calc::tests::the_biggest_angle_is_whole_turns`；`cargo test -p computed_style` |
| 這些值經過 layout、文字和 paint 不 panic | `cargo test -p shell --test it -- limits::lengths_past_any_float` |
| `sizes` 裡算出 NaN 或無限大的長度跳過 | `cargo test -p css --lib media::`；WPT 的 `html/semantics/embedded-content/the-img-element/sizes/` |
| automation 的視窗大小每邊不超過 8192 實體像素 | `cargo test -p shell --test it -- automation_pipe::the_window_has_bounds_to_set` |
| WPT：`css/css-values/calc-infinity-nan-computed.html`、`calc-infinity-nan-serialize-length.html` | `python3 scripts/test.py wpt css/css-values` |

## 做過的檢查

2026-10-22，Fedora 44（AMD Ryzen AI 9 HX 370），四段都在這台上做。

- **做了什麼**（設計和理由見 `README.md`）：
  - 第 1 段：`base` crate；lint 的設定；每個根的測試的那一行和推行用的 `"not converted yet"`；`sanity.py numerics` 和它的測試、CI、`CLAUDE.md`；`dom`（104 處，版本號經過 `version::bump`）、`bindings`（8）、`protocol`（6）；`fetch_add` 的 18 處（產生 ID 的換成 `AtomicSequenceNumber`）；`gfx::Size` 的 checked 的面積。
  - 第 2 段：第 5 節的 1、2、5、6（上面的驗收表）。
  - 第 3 段：第 5 節的 3、4、7，`calc()` 的非有限值（第 17 條）。
  - 第 4 段：其餘 21 個 crate 約 1145 處，四批各一個 commit；`f32_from_f64` 搬進 `base`；resource id 和 frame token 改成 u64（第 19 條）；`NOT_CONVERTED` 刪掉；第 6 節的文件。改寫由平行的 agent 照 crate 或檔案分工，回報會改變行為、要選做法的地方，看過、統一不一致的寫法（統計用 `wrapping_add`）再 commit。
- **實作時改的**（都寫回 `README.md`）：`next()` 叫 `get_next()`（clippy 不准和 `Iterator::next` 同名）；`clippy.toml` 用泛型的 `Atomic::fetch_add`（別名的路徑 clippy 認不得）；推行時還沒改的 crate 的根不放測試的那一行（`duplicated_attributes`）；texture 的上限放在 `protocol::viz`；`grid-template-areas` 交給 taffy 時才夾；`setTimeout` 照舊回傳 `u32`；clamp 的 helper 放在 `computed_style`；`line-height` 的數字也要夾；角度的上限是 f32 裡最接近 Blink 的常數、是 360 的倍數的值；`sizes` 的非有限值跳過（第 3 段漏了，4c 的 WPT 抓到）。
- **和 Chrome 152 比對**（scratchpad 的 `probe_*.py`，同一段 script 跑在兩邊）：`drawImage`、`createPattern` 的無效 canvas；表格的位置；`grid-template-areas` 的大小；`<select>` 的 PageDown（Blink 的 `int` 溢位時 Chrome 跳到不一定的地方，kun 不照它）；很大的視窗；CSS 長度、字級、`calc()` 的 specified 和 computed。差別記在 `docs/limitations.md`：幾個大的值的加總（Chrome 的 `LayoutUnit`）、calc 是 f32、數字和角度的 calc 寫值不寫函式（原本就有）、grid 只到 taffy 的 10000 條 track、視窗大小、timer 的 handle 共用一個計數器。
- **找到的問題**：盤點的（`details.md`）都修了；改寫時另外修了字型的 bounding box 寬度（兩個 `i16` 相減溢位）、collapsed border 和像素的配置大小。原本就有的：`shell` 的 `scenario.rs` 的 `trailing_numbers` 碰到多位元組的空白（U+3000）時 `+ 1` 不在字元邊界上、切字串會 panic（和溢位無關，4d 的 fork 看到的），收尾時改成加上那個空白的長度，`scenario::tests::trailing_numbers_after_any_white_space` 守著。收尾的一致性檢查另外找到 `gpu_context.rs` 的錯誤訊息少了續行的 `\`、中間多一串空白，修了。
- **Mutation test**（scratchpad 的 `m74/stage1.toml`、`stage2.toml`、`stage3.toml`）：
  - 第 1 段 29 條：漏的補了測試（`clamp_floor` 的 2.7、`heap_tasks::due_at_next_due`）或換成不等價的寫法；兩條等價（`chunks::place` 的另一種一對一排法、64 位元上的 `checked_area`），記進 `docs/test-gaps.md`。
  - 第 2 段 23 條：漏的三條，兩條等價、把程式改寫得更直接（沒畫過的像素只配置透明的、表格的兩個分支合成一個），一條補了 Chrome 152 實測的表格；Viz 拒絕小的 adapter 測不到，記進 `docs/test-gaps.md`。
  - 第 3 段 31 條：漏的四條補了測試（收負值的 margin 的 NaN、合不起來的 `calc(1e30em - 1e30px)`、`opacity` 的欄位）或拿掉多餘的檢查；字級的 NaN 檢查等價，記進 `docs/test-gaps.md`。
  - 第 4 段：行為不變的改寫是等價的 mutation，沒有逐一跑（記在 `docs/test-gaps.md` 的「只靠審查」）；`popup_menu` 的修正弄壞過、測試失敗。
- **WPT**：`calc-interpolation.html` 的 `calc(infinity * 1px)` 幾項更新了預期結果（Blink 插值時保留 infinity，kun 用夾過的 computed value）；`calc-infinity-nan-computed.html`、`calc-infinity-nan-serialize-length.html` 加進清單；canvas 的兩份 `2d.composite.grid` 拿掉了過時的 `.ini`（`object-fit` 之後就通過）。跑過的目錄：`css/css-tables css/css-grid html/canvas html/semantics css/css-backgrounds css/css-values css/css-fonts css/css-transforms css/css-sizing css/css-text dom webidl selection uievents css/cssom-view FileAPI`。
- **不穩定的測試**：
  - `shell` 的 `wasm_reftests::w2_order`：測試假設第二個 wasm script 在第一個的 `start` 等的 50 ms 內編好，整個 workspace 一起跑時不成立；改成等到第二個 script 加的 c 出現（另一個 commit，`w2-a.wasm` 重編）。
  - WPT 的 `moving-between-documents` 的兩份 after-prepare：很忙的機器上失敗、單獨通過，很可能是測試用真實時間的計時器在 script 載入前一秒搬它；`.ini` 標成 known intermittent、寫明原因。
  - WPT 的 `css/css-transforms/group/svg-transform-nested-007.html` 在這台上失敗（邊緣 160 個像素差 1），RFC 開始前（`313ff39d`）也一樣，`.ini` 沒改。
- **跑過的檢查**：每段結束時 `cargo test --workspace --no-fail-fast`（最後一次 3587 個通過，跳過 3 個；5 分 18 秒）、`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy、`sanity.py` 的六項（`docs` 只有根目錄 README 引用的 V8 build 目錄在這台不存在）和 `sanity_numerics_test.py`；`test.py scenarios` 68 份、`test.py reftest` 189 份；`test.py automation` 53 份裡 49 份通過，失敗的 4 份是 `docs/limitations.md` 記過的、這台 Linux 的視窗大小帶小數的，`pipe --window` 通過。
