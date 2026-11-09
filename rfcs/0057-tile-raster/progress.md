# RFC 0057 的進度：tile 的 raster：空間索引、單色 tile、raster worker

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-18。完成：2026-10-18，macOS（Apple M4：4 個效能核心、6 個節能核心；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0057` 上做，六段一起審核、merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 量測工具（`ci.py raster`）和基準 | 完成 | 2026-10-18 |
| 2 內容範圍外的 tile 不 raster（B0） | 完成，第 3 段取代 | 2026-10-18 |
| 3 空間索引；查不到 op 的 tile 不 raster | 完成 | 2026-10-18 |
| 4 單色的 tile（B1） | 完成 | 2026-10-18 |
| 5 raster worker（rayon 的專用 pool） | 完成 | 2026-10-18 |
| 6 量測和決定：每個 tile 在它的 thread 上做完、重複用 buffer，raster worker 預設不開 | 完成 | 2026-10-18 |

## 驗收

### display list 和 raster

| 項目 | 怎麼驗證 |
| --- | --- |
| 每個 op 的 visual rect：畫的 op 是給的範圍，沒給的是到處；`finalize` 之後狀態 op 是它管到的畫圖 op 的聯集（`Save` 和它的 `Restore` 一樣、巢狀的算進外層、transform 和 clip 到所在範圍結束、沒配對的 `Save` 到最後）；帶 filter 或 blend mode 的 `SaveLayer` 裡每個 op 都是到處，`SaveLayerAlpha` 和只有 alpha 的不是；`push_list_at` 把子 list 的範圍移過來 | `cargo test -p compositor_paint a_state_ops_visual_rect_is_what_it_applies_to what_may_draw_anywhere_meets_every_rect a_list_pushed_at_an_offset_is_drawn_there` |
| 靜態的 R-tree（STR）：查到的和逐一比對的一樣、照順序；每個節點最多 11 個子節點、包住它們，每個子節點只有一個父節點；只碰到邊和空的範圍不算 | `cargo test -p compositor_paint rtree::` |
| 只有 layer 的 list 建索引（`DisplayItemList::new`），`record()` 的不建、`ops_in` 給全部；canvas 的 list、SVG 圖片的 record、分頁列和工具列畫進 Browser 的 layer 的 list 是 `record()` | 同第一列的測試；審查 |
| raster 只播 tile 的範圍（外擴一個裝置像素、換回 CSS px）查到的 op：1000 份隨機的 display list（巢狀的 save、group、drop shadow、clip、平移縮放和旋轉、各種畫的 op、大的填色、沒帶範圍的 `DrawColor`、沒關的 save）和隨機的 tile、三種縮放、兩種清除色，和整份播逐像素相同，播的 op 不到一半；判斷是單色時每個像素都是那個顏色（空白的和蓋滿的都出現夠多次）；`playback_ops` 只播給它的 op | `cargo test -p raster the_ops_a_tile_finds_draw_it_as_all_of_them only_the_ops_given_are_played` |
| `color_if_solid_in_rect`：沒畫東西時是清除色（不透明或透明時；半透明的不知道）；蓋滿 tile 的不透明填色（`Src` 也算）是它的顏色，半透明的清除色上也是；差一點沒蓋滿、半透明、有 shader、模糊、虛線、描邊、其他 blend mode 的放棄；幾何只差一個像素的當沒畫；後蓋的算數；`DrawColor` 填滿 clip（其他 blend mode 放棄）；整個在 clip 外的當沒畫，`Restore` 結束 clip；是矩形的 `ClipRRect` 照 clip 算，有圓角的放棄；平移和縮放照算，旋轉放棄；碰到 tile 的圓角矩形、group 放棄；影子的模糊多算一個像素；空的路徑當沒畫 | `cargo test -p compositor_paint a_tile_is_of_one_color_if_its_ops_say_so` |
| `playback_into`：重複用的 buffer 只留最後一個 tile，大小是那個 tile 的 | `cargo test -p raster a_buffer_reused_holds_only_the_last_tile` |
| 共用的 `RasterCache`：8 條 thread 同時用同一份畫同一張圖，畫出來一樣，表裡只有一筆 | `cargo test -p raster threads_share_a_cache` |

### compositor

| 項目 | 怎麼驗證 |
| --- | --- |
| `prepare_tiles`：判斷得出單色的 tile 不 raster（查不到 op 的是清除色、蓋滿的不透明矩形是它的顏色，兩塊內容之間的空白也是），畫出來和 raster 了一樣；清除色半透明、或 op 說不出來時照舊 raster、照舊掃像素，raster 完是單色的 tile 也保留；debug log 的 `skipped=` | `cargo test -p compositor a_tile_outside_the_content_is_not_rastered a_tile_of_one_color_is_kept_until_it_changes` |
| debug build 照樣 raster 判斷是單色的 tile、比對 | reftest、WPT 和情境用 debug build 跑；拿掉比對的 mutation 測不出來（`docs/test-gaps.md` 的「測試守不住的地方」） |
| raster worker：同樣的 tile 用 1 條和 4 條 thread 畫，quad、resource 的編號和像素都一樣，每個 tile 都有 `draw_info`；1 條時不建 pool，4 條時建；`LayerTreeSettings` 的預設是 1；tile 在畫它的 thread 上上傳到 GPU | `cargo test -p compositor raster_threads_raster_as_the_compositor_thread_does tiles_are_rastered_on_the_compositor_thread_unless_asked losing_the_gpu_rasters_again_the_tiles_that_were_on_it` |
| `kun --num-raster-threads N`：解析（1 以上的整數）、`kun --help` 列出、交給 headless 的選項；經過 `RendererFlags` 到 renderer 的 `LayerTreeSettings`：帶 2 時有 2 條 thread 在 raster，不帶時只有 compositor thread | `cargo test -p shell --bin kun`、`python3 scripts/raster_test.py RunTests` |

### 量測工具（`ci.py raster`）

| 項目 | 怎麼驗證 |
| --- | --- |
| 從 trace 讀 Compositor thread 的 `PrepareTiles`、所有 raster 的 thread 的 `raster::playback`（raster thread 的名字截成 `CompositorTileW`，照 track 分）和有幾條、main frame（只算開始在那一段裡的）；從 compositor 的 debug log 讀 raster 了幾個 tile、幾個是單色、幾個沒 raster（去掉顏色碼、只算那一段的）；改大小縮小再放回來；報告寫明條數；不認得的頁面拒絕 | `python3 scripts/raster_test.py ReadingTests` |
| kun（dev profile、headless）在一個高、窄的頁面上捲動和改大小：兩段都 raster 了 tile、log 數得到、段落右邊的 tile 沒有 raster、改大小時有 main frame；`--raster-threads 2` 傳給 kun | `python3 scripts/raster_test.py RunTests` |

### 量測的結果（2026-10-18，macOS，Apple M4，接電源）

`python3 scripts/ci.py raster`（headless、device scale factor 2），第 1 段的 `a804d150` 和完成時的版本，同一個時段：checkout `a804d150` 量完再回來。「每次更新」是 `PrepareTiles` 的牆上時間的平均。

| 頁面 | 動作 | 第 1 段 | 完成（預設 1 條） | 完成（`--raster-threads 9`） |
| --- | --- | --- | --- | --- |
| Gutenberg | 捲動（合計） | 479 ms | 90 ms（−81%） | 82 ms |
| Gutenberg | 改大小（每次更新） | 67.7 ms | 7.7 ms（−89%） | 3.1 ms |
| SQLite | 改大小（每次更新） | 3.6 ms | 1.9 ms（−47%） | 1.7 ms |
| NewsSite-Next | 捲動（合計；最長的一次） | 1116 ms；106 ms | 675 ms（−40%）；101 ms | 174 ms；22 ms |
| NewsSite-Next | 改大小（每次更新；最長的一次） | 135.5 ms；244 ms | 103.4 ms（−24%）；172 ms | 24.5 ms；76 ms |

真的視窗（`--window`，`PrepareTiles` 另外有上傳到 GPU 的時間；第 6 段改之前的版本，9 條）：Gutenberg 捲動 963 → 369 ms、改大小每次更新 74.9 → 11.1 ms；NewsSite 捲動 1417 → 487 ms、改大小每次更新 152 → 36 ms。

`python3 scripts/ci.py speedometer --against main`（`main` 的 `6998f648`，20 個 default suite，各 3 輪、每輪 10 個 iteration；RFC 0053 的 A/A 是時間的幾何平均 +0.3%、單一 suite −2.9% 到 +2.4%，記憶體 −1.2%、單一 suite −17.8% 到 +7.4%）：

| 版本 | 時間（幾何平均） | 記憶體峰值（幾何平均） |
| --- | --- | --- |
| 第 5 段（9 條 raster thread） | +1.2% | +9.0%（20 個都變多，+2.1% 到 +33.4%） |
| 完成（預設 1 條） | **+0.5%** | **−0.1%**（−3.7% 到 +10.9%） |

## 做過的檢查

2026-10-18，macOS（Apple M4）。

- **量測工具**（第 1 段）：`scripts/raster.py` 沿用 `speedometer.py` 的 `CdpPipe`、`Server`、`kun_binary`、trace 的 reader。試跑時發現 headless 的 device scale factor 是 1，這台的真的視窗是 2：SQLite 改大小時 raster 182 對 520 ms，加了 `--scale`，headless 預設 2。headless 和真的視窗量到的相近（真的視窗另外有上傳）。trace 裡 thread 的名字截成 15 個位元組，所有 raster thread 都叫 `CompositorTileW`：照名字找 `CompositorTileWorker` 找不到，RunTests 抓到，改成照 track 分。
- **範圍是不是保守的**（第 2 段）：layer 的 list 的範圍來自 paint 的 `visual_rect`，raster invalidation 和 layer 的大小本來就靠它。逐一看過：盒子的陰影帶 3σ 的模糊、文字的範圍是字形的墨水、描邊的路徑帶 miter 和方形端點、`DrawRecord` 和 `DrawImage` 是 `dst`；effect 只有 `SaveLayerAlpha`。Browser 的工具列和分頁列沒有 transform。會不保守的有兩種：沒帶範圍的畫圖 op、帶 filter 或 blend mode 的 `SaveLayer`，都當成到處（canvas 的 list 有，但它不經過 tile）。`PaintFlags` 的 `blur` 在 `op_bounds` 裡只有 `DrawShadow` 算進去，目前只有它用。`raster` 的一個測試在 `SetMatrix` 底下用 `push_op`（範圍照沒轉換的幾何算），索引找不到那個方塊，改成給 layer 空間的範圍；正式的程式沒有這種用法。
- **找到的 bug**：R-tree 起先上一層的節點只記第一個子節點、子節點到下一個節點的第一個為止，但上一層往上建時也被重新排序，範圍就錯了；隨機的查詢測試抓到，改成每個 entry 記 `start..end`。
- **單色 tile 的組成**（第 4 段，暫時在 `prepare_tiles` 加一行 debug log，量完還原）：NewsSite-Next 6160 個單色的 tile 裡，3884 個（63%）是一個蓋滿的不透明 `DrawRect`，555 個是 `SetMatrix`、`ClipRect`、`SetMatrix`、`DrawRect` 而顏色是清除色（被 clip 切掉），85 個是一個 `DrawRect` 而顏色是清除色（只有查詢多算的那一個像素碰到），其餘約 1600 個混著 `DrawPath`、`DrawImage`、`DrawGlyphs`；沒有半透明的顏色。Gutenberg 的 432 個全是清除色、查到的是 `DrawGlyphs`。
- **測試的調整**：判斷單色之後，原本用一個蓋滿的矩形的 `a_tile_of_one_color_is_kept_until_it_changes` 改成沒有圓角的 `DrawRRect`（raster 出來是單色、op 說不出來）；`a_tile_outside_the_content_is_not_rastered` 的對照組從「不帶範圍的透明矩形」（判斷用幾何看穿了它）改成包在一個蓋住 layer 的 `ClipPath` 裡。
- **raster worker 和 Speedometer**（第 5、6 段）：
  - 條數（`--raster-threads`，同一份程式）：NewsSite 改大小每次更新 1、2、4、6、9 條是 106.8、65.0、43.0、33.4、26.4 ms，捲動合計 700、415、275、231、202 ms；Gutenberg、SQLite 4 條以上持平。
  - 平行的效率：每次 `raster::playback` 的平均，NewsSite 1、4、9 條是 0.36、0.48、0.67 ms，SQLite（很輕的文字 tile）是 0.025、0.071、0.206 ms。查字型改成每條 thread 自己記著、不拿鎖：沒有變；改成重複用 buffer：SQLite 0.189 ms，也幾乎沒有變。所以不是 `RasterCache` 的鎖，也不是配置記憶體；比較可能是節能核心和 thread 互相排擠（span 的時間包含被排程器暫停的時間），還沒查證。
  - Speedometer：第 5 段對第 4 段 `--against 8d7f5565` 是時間 +1.0%、記憶體 +9.9%（20 個都變多），退步來自 raster worker。第 5 段對 `main` 時 TodoMVC-Vue 慢了 11%，對第 4 段時只 +1.1%：這個 suite 在不同次量測之間就差 2 ms，是雜訊。每個 tile 在它的 thread 上做完之後，5 個記憶體漲最多的 suite（Vue、ES6-Webpack、NewsSite-Next、Lit、ES5）對第 4 段是時間 +2.0%、記憶體 +2% 到 +16%（之前 +12% 到 +27%）；加上重複用 buffer 是時間 +1.4%、記憶體 −2.3% 到 +20.2%；4 條時時間 +1.2%、記憶體 +2.9% 到 +21.1%，分不出差別。所以預設改成 1 條（「已決定的事項」第 15 條），完成時的 M3 見上面。
- **mutation test**，每條各自弄壞一處，跑完檔案都還原：
  - `mutants-1.toml`（12 條，量測工具）：第一次沒抓到「phase 之後才開始的也算」，測試補了一個 phase 之後的 slice；其他都抓到。
  - `mutants-2.toml`（15 條，第 2 段的 `drawn_bounds`）：全部抓到；它描述的程式第 3 段由索引取代、拿掉了。
  - `mutants-3.toml`（20 條，範圍、R-tree、只播查到的 op）：全部抓到。「全部照播」畫出來一樣，等價測試抓不到，補了 `only_the_ops_given_are_played`。最後一條（「只有一個 op 的 tile 也跳過」）改的那行第 4 段取代了。
  - `mutants-4.toml`（22 條，單色的判斷）：第一次沒抓到「影子的模糊不多算一個像素」，測試算位置時忘了形狀本身的寬，改了之後抓到；其他都抓到。
  - `mutants-5.toml`（17 條，raster worker、快取、開關、工具）：全部抓到。其中「預設用全部的處理器」「Browser 的 UI 也用 renderer 的條數」兩條改的程式第 6 段拿掉了。
  - `mutants-6.toml`（6 條，buffer、在 worker 上上傳、預設 1 條）：全部抓到。每條 thread 重複用 buffer 本身（換成每次配置新的畫出來一樣）和 debug build 的比對測不出來，記進 `docs/test-gaps.md` 的「測試守不住的地方」。
- **reftest、WPT、情境**：每段都跑了 `python3 scripts/test.py reftest`（167 個）、`scenarios`（56 個）和相關的 `wpt` 目錄（最多 3384 個測試：`css/css-backgrounds`、`css/filter-effects`、`css/css-overflow`、`css/css-transforms`、`css/CSS2`、`css/css-text`、`css/css-flexbox`、`css/css-grid`、`html/canvas/element/drawing-images-to-the-canvas`），沒有非預期的結果，debug build 的比對一次都沒觸發。過程中查到的、和這份 RFC 無關的：
  - `canvas/c4-paths`、`text-overflow-012`、`020`、`022`、CSS2 的 `new-fc-beside-adjoining-float`、`new-fc-separates-from-float-2`、`clear-on-parent-and-child`：在 macOS 上透明的 layer 和白色的反鋸齒捨入不同，改了預期結果（`docs/limitations.md` 的「Compositor、layer 和 input」）。拿掉 mimalloc 也一樣。
  - `python3 scripts/test.py automation` 的 `position-relative`：情境開 800×4100 的視窗，這台的螢幕放不下（系統把視窗限制在 891 pt 高），`position-relative` 和 `position-absolute` 改成先捲動，兩種跑法都通過。
- **完成時的驗證**：`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings` 通過（第 5 段時 clippy 抓到測試裡還傳 `&mut RasterCache`，`cargo clippy --fix` 修好）；`cargo test --workspace --no-fail-fast` 3228 個通過、0 個失敗；`raster_test.py` 9 個通過；`sanity.py` 的 `deps`、`boundary`（`TILE_PIXELS` 是 thread-local，列進例外）、`unsafe`、`tests`、`docs` 通過。rebase 到 `main` 的 `4c7919c1`（RFC 0056：每個模式都用 wgpu 畫、拿掉 `SoftwareRenderer`；RFC 0058 的草稿）之後重跑：`cargo fmt --check`、clippy 通過，`cargo test --workspace --no-fail-fast` 3240 個通過、0 個失敗，`raster_test.py`、`sanity.py` 的五項通過；`test.py reftest`（168 個）、`scenarios`（56 個）、`wpt css/CSS2 css/css-overflow css/css-backgrounds css/filter-effects`（2349 個測試）沒有非預期的結果：Viz 改用 wgpu 合成之後，照平台分的那幾個預期結果仍然成立。
