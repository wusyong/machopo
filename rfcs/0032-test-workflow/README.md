# RFC 0032：測試流程：第三方依賴的同步、本機的測試和每天的 CI

- 開始日期：2026-10-10
- 狀態：✅ 已完成：2026-10-10（「已決定的事項」第 1 到 20 條；只在 Windows 11 上做；整套的測試由使用者設定的每天的 CI 跑；驗收在 `progress.md`）
- 細節：`details.md`
- 進度：`progress.md`

## 摘要

測試越來越多（`#[test]` 從 2026-10-01 的 269 個到 10-10 的 1880 個），也在三台機器（Windows、macOS、Fedora）上跑。這份 RFC 做兩件事，都是流程和腳本，不改 kun 的行為：

1. **`ci.py third-party` 的同步**：從「檢查、只 clone 缺少的」改成真的同步，像 Chromium 的 `gclient sync`：缺的 clone、commit 對不上的 fetch 再 checkout 到 kun 釘的 commit；checkout 裡有自己的工作（未提交的改動、沒 push 的 commit）時不動它，說明原因並停下。`test.py` 的每個子指令開始前先同步；fork 改了，就在同一次改動裡更新 kun 的 pin。
2. **本機只跑相關的測試，整套交給每天的 CI**：CI（使用者設定，每天一次）跑整套的 `matrix`、`fuzz`、`wpt`、`wdspec`、`reftest`、`scenarios`；做 RFC 時本機不跑 `matrix`、`fuzz`、`wdspec`，`wpt`、`reftest`、`scenarios` 只跑改到的部分；每段工作做完只跑改到的 crate 的 `cargo test -p`，整個 workspace 在階段做完時跑。merge 不等 CI。

草稿原本還有時間紀錄、排程、nextest 和 mutation test 的範圍：前兩項在草稿時拿掉（第 1、2 條），nextest 和 mutation test 量過之後拿掉（第 15 條，量的結果在 `progress.md`）。量的時候找到並修好兩個不穩定的測試，其中一個是 kun 的 bug（`IdleDeadline.timeRemaining()` 的浮點誤差）。

## 動機

**現況（2026-10-10）**：

- **三台機器的 `../third_party` 對不上**：macOS 上的 `fuzz` 一開始就停在 `../third_party` 的檢查：rusty_v8 fork 的 checkout 在 `5c8e7a1`，kun 釘的是另一台（Fedora）改成的 `8703527`，那台的 fork 還沒有 `5c8e7a1`，最後要改 kun 的 pin、請 Fedora 那台手動 fetch（`rfcs/0025-test-binaries/progress.md` 的「做過的檢查」）。Windows 這台的 parley fork 在 `4cc636c`（fork 上只改了 `README.machopo.md`），kun 釘的還是 `8f8f6a4`；這台也沒有 ../third_party/speedometer。`ci.py third-party` 只檢查、只 clone 缺少的，已經存在但對不上的 checkout 要人自己 fetch、checkout；它也檢查所有專案，所以這台的 `fuzz` 會因為用不到的 speedometer 不在而停下。
- **測試在長，每一段要跑的也在長**：`#[test]` 的數目，10-01 是 269 個、10-03 是 495、10-05 是 758、10-07 是 1218、10-09 是 1677、10-10 是 1880（`origin/main` 每天最後的 commit，`git grep -c '#\[test\]'`）。照原本的 `CLAUDE.md`，改了 DOM API、style、layout、文字、paint 或 machopodriver 就跑整套的 `test.py wpt`（2179 行的清單，約 2 分鐘），常常一段好幾次；動到 GC 的 RFC 完成時要編 debug 的 V8、跑 `matrix` 和 `fuzz`（幾十分鐘以上），RFC 0030、0025 都因此留到之後才在 macOS 上手動補跑。整套的 WPT 在本機和其他測試一起跑時 32 個 process 搶 CPU，這次量到的不穩定的測試都和負載有關。
- **沒有 CI**（原本 `docs/testing.md` 的「CI」）：整套的測試只在改到它們涵蓋的部分時才在開發機上跑。

**目標**：

- 每台機器用一個指令就讓 `../third_party` 和 kun 釘的一致，不會弄丟 fork 上的工作；長時間的測試不會跑到一半才因為對不上而停下。
- 做 RFC 時本機只跑和改動相關的測試，整套由每天的 CI 跑，等待的時間最少。

**不在範圍內**：

- CI 本身：使用者設定，每天跑一次（第 2、16 條）。
- 時間紀錄、排程、nextest、mutation test 的範圍和 `--jobs`（第 1、2、15 條）。
- 找出慢的測試、訂時間預算；改寫慢的測試。nextest 的實測列出的最慢的測試在 `progress.md`，留給之後。
- V8、Skia 改成動態連結（不做）。
- 改測試本身的內容（量的時候找到的兩個不穩定的測試除外）。

## 設計概要

**每台機器**：

- `python3 scripts/ci.py third-party` 讓 `../third_party` 的每個專案都在 kun 釘的 commit 上：缺的 clone，對不上的 fetch、checkout。fork 的 checkout 裡有你自己的工作時，它不動那個專案，印出原因（哪些檔案沒提交、哪個 commit 沒 push）和怎麼做（提交並 push 之後更新 kun 的 pin，或先切回 pin），exit code 非 0。
- `test.py` 的每個子指令（`matrix`、`fuzz`、`wpt`、`wdspec`、`reftest`、`scenarios`、`automation`、`mutate`）開始前自動跑一次同步；同步失敗就不開始。已經一致時只是幾個本機的 git 指令，不用網路。
- 改 fork 時：在 fork 上 commit，同時把 kun 的工作目錄裡的 `third_party.toml` 改成那個 commit（不用先 commit）；同步讀工作目錄裡的 pin，所以不會切走你的 commit。push 時先 push fork、再 push kun。

**做 RFC 的人**（`CLAUDE.md` 的「測試」）：

| 測試 | 本機 | 每天的 CI |
| --- | --- | --- |
| `cargo test` | 每段工作做完：改到的 crate（`cargo test -p`）；階段做完：`--workspace` | `--workspace` |
| `sanity.py`、fmt、clippy | 照舊（`sanity.py` 每段，fmt 和 clippy 階段做完） | 由使用者決定 |
| mutation test | 照舊 | — |
| `test.py wpt` | 改到的部分的目錄（`test.py wpt dom/events/`） | 整套 |
| `test.py reftest`、`scenarios` | 改到的部分（`reftest` 的路徑、`scenarios` 的名稱） | 整套 |
| `test.py wdspec`、`matrix`、`fuzz` | 不跑（要重現 CI 的失敗時才跑） | 整套 |
| `test.py automation` | 照舊（動到視窗那一層、RFC 和階段驗收時；CI 沒有螢幕） | — |

CI 上壞掉的，看到就修；merge 不等 CI（第 18 條）。

## 詳細設計

### `ci.py third-party` 的同步

每個專案（`third_party.toml` 和 `.cargo/config.toml` 的 rusty_v8 的 tag，照現在的 `third_party_pins`）照這個順序：

1. **不存在**：clone 到 pin（照現在的 `--clone`：shallow、WPT 不轉換換行、V8 的 submodule）。`--clone` 這個選項拿掉，預設就做；不留「只檢查」的模式（第 4 條）。
2. **已經在 pin 上**：不動。
3. **不在 pin 上**：
   - **有自己的工作就停下**（第 3 條）：工作目錄有未提交的改動（`git status --porcelain --untracked-files=no`：untracked 的檔案不算，fork 裡常有 build 的產物），或 HEAD 是遠端沒有的 commit：向 `origin` fetch HEAD 的 commit，遠端給不出來就是沒 push。不用「有沒有遠端的 branch 包含它」判斷：`ci.py` 的 clone 是淺的、直接 fetch commit，本機沒有遠端的 branch，每個 checkout 都會被當成有沒 push 的工作。印出是哪一種、怎麼處理，不動這個專案，最後 exit code 非 0；其他專案照樣同步。
   - **沒有的話**：本機沒有 pin 的 commit 就 fetch（照現在的做法：commit 直接 fetch，tag 照 tag），再 `git checkout --detach <pin>`；rusty_v8 照樣確認 pin 在 release 的 tag 之後只改了預編的 V8 不包含的東西（現在的檢查）。
4. 印出每個專案做了什麼（clone、切換、不動、停下）。

- **只讀工作目錄的 pin**：`third_party.toml` 沒 commit 的改動也算，所以改 fork 時先改 pin 就不會被切走。
- **淺 clone**：照現在的做法 clone 的是淺的；要 fetch 新的 pin 時照樣淺 fetch。
- **`test.py` 的每個子指令開始前**（第 5 條）：都會編 kun，kun 經由路徑依賴 rusty_v8 和 parley，所以每個子指令一開始就呼叫同步（`fuzz` 現在的檢查換成它）；失敗時不開始。`cargo test` 不經過 `test.py`，不同步。
- **fork 改了就更新 pin**：寫進 `CLAUDE.md` 的「第三方的 fork」：fork 的 `kun` 分支每多一個 commit（包括只改 `README.machopo.md` 的），kun 這邊在同一次改動裡更新 `third_party.toml`，push 的順序是先 fork、後 kun。pin 照舊是精確的 commit，不放寬成「只差 README 也算」。
- 這台的 parley（`4cc636c`）照這條在這一段更新 pin。

### 本機的測試和每天的 CI

- **每天的 CI**：使用者設定，每天跑一次整套的 `test.py matrix`、`fuzz`、`wpt`、`wdspec`、`reftest`、`scenarios` 和 `cargo test --workspace`（第 16 條）。它們都經過 `test.py`，開始前先同步 `../third_party`（第 1 段）。
- **每段工作做完**（第 17 條）：改到的 crate 的 `cargo test -p`（一個或幾個）和 `sanity.py`。改的是底層的 crate（`dom`、`bindings`）時，下游被弄壞的會在階段做完時的 `--workspace`、或隔天的 CI 發現。不選「受影響的 crate」（照依賴算出 `-p` 的清單，Chromium 的 `analyze`）：改底層時清單接近全部，省不了多少，還要一個腳本。不選照舊每段 `--workspace`：多 10 到 20 秒，整套有 CI。
- **階段做完**：`cargo test --workspace`、fmt、clippy，照舊。
- **本機跑相關的部分**（第 16 條）：改了 style、layout、文字、paint、raster：`test.py reftest <路徑>` 和 `test.py wpt <目錄>`（WPT 的 reftest）；改了 DOM API、WPT 的清單或預期結果：`test.py wpt <相關的目錄>`；改了 machopodriver、`kun --automation-pipe`、`protocol::automation` 或 `tests/wpt/wptrunner_machopo/`：`test.py wpt` 的一個小目錄（確認這條路通），和改了 wdspec 的清單或預期結果時的那幾個 `test.py wdspec <路徑>`；寫了或改了情境：`test.py scenarios <名稱>`。預期結果要更新時照舊用 `MACHOPO_UPDATE_EXPECTATIONS` 跑那幾個。
- **給 `test.py wpt`、`wdspec` 的路徑只跑清單裡的**（第 20 條）：清單在那些路徑底下的行，或在清單的某一行底下的路徑；`--all` 照路徑全部跑。
- **本機不跑**：整套的 `wpt`、`wdspec`、`matrix`、`fuzz`；要重現 CI 的失敗時才跑那一項（`matrix`、`fuzz` 要 debug 的 V8，第一次要完整編一次）。動到 GC 的 RFC 完成時，不在本機跑 `matrix` 和 `fuzz`：merge 之後看 CI 的結果（第 19 條）。
- **merge 不等 CI**（第 18 條）：CI 上壞掉的看到就修，和其他的 bug 一樣排；照舊不用重跑蓋過不穩定的測試。
- **文件**：`CLAUDE.md` 的「測試」、`docs/testing.md` 的「測試的種類」和「CI」、`docs/README.md` 的「一份 RFC 做完時」的驗證。

### 測試

- 同步：手動確認（第 6 條；`docs/test-gaps.md` 的「只做過手動確認」加一列）：這台的實際狀況（parley 在 `4cc636c`、kun 釘的是 `8f8f6a4`，speedometer 不在）跑一次；在一個 fork 裡留一個未提交的改動、做一個沒 push 的 commit，確認同步不切走它們、印出原因；`third_party.toml` 在工作目錄裡改過、沒 commit 的 pin 會被讀到。
- 測試的規則：文件的改動，`sanity.py docs`。CI 由使用者設定，不在這份驗收。
- 每一段：`CLAUDE.md` 的檢查清單。

### 分階段

1. **`ci.py third-party` 的同步**：同步、`test.py` 的子指令開始前的同步、fork 的 pin 的規則、這台的 parley 的 pin。三台機器都會用到，先做。
2. **本機的測試規則配合每天的 CI**：`CLAUDE.md`、`docs/testing.md`、`docs/README.md`。原本的第 2 段（nextest 的實測）量完 Windows 之後拿掉，第 3 段（mutation test 的範圍）在開始時拿掉（第 15 條）。

## 缺點

- **同步會切換 `../third_party` 的 checkout**：只在沒有自己的工作時切；但切走一個乾淨的、已經 push 的 checkout，之前在看的 commit 要自己記得。
- **壞掉的發現得比較晚**：整套的測試每天跑一次，merge 不等它；改動和 CI 發現之間，`main` 上可能有一天是壞的。改了底層的 crate 時，下游的 `cargo test` 也到階段做完時才跑。
- **CI 不在這個 repo 的驗收裡**：CI 的設定由使用者維護；它停了或沒跑，整套的測試就沒有人跑。

## 理由和其他做法

完整的比較在 `details.md` 的「其他做法」。

- **同步，但不動有自己的工作的 checkout**：只檢查的話，每台機器都要人照著錯誤訊息手動 fetch、checkout（這次 Fedora、Windows 都碰到）；無條件地切換則可能把 fork 上沒 push 的工作切走。`gclient sync` 遇到有改動的依賴也是預設拒絕。
- **整套的測試交給 CI，本機只跑相關的**：時間的大頭是整套的 WPT（每次約 2 分鐘，一段好幾次）、debug 的 V8 和 `matrix`、`fuzz`（幾十分鐘以上），不是 `cargo test --workspace`（25 到 35 秒）；本機和其他測試一起跑整套時也會因為搶 CPU 而不穩定。Chromium 也是送出前只跑受影響的測試，完整的在 CI 上跑。
- **nextest 和 mutation test 的範圍不做**（第 15 條）：nextest 在 Windows 上比 `cargo test` 慢 27%；mutation test 掃過 814 條，只有約 4% 是改字串或改同一處的，而且多半是行為，規則減不了。
- **不做**：`../third_party` 照樣要人手動對齊；做 RFC 時照樣一段跑好幾次整套的 WPT，GC 的 RFC 照樣要等 debug 的 V8。

## 先例

- **Chromium 的 `DEPS` 和 `gclient sync`**：每個第三方專案釘一個 commit，`gclient sync` 讓每台機器一致；依賴有本機的改動時預設不動它（要 `--force` 或 `--reset`）。kun 的 `third_party.toml` 和 `ci.py third-party` 是同一個做法，這份補上真的同步和「開始前同步」。
- **Chromium 的 CQ 和 CI**：送出前的 CQ 只跑受影響的測試（`analyze` 依 build graph 選），完整的測試在 CI 的 waterfall 和每天的 build 上跑；kun 照「本機跑相關的、整套的每天在 CI 上跑」，但沒有送出前的 CQ（merge 不等 CI）。
- **V8**：GC 的 stress 設定（`--variants`）在 CI 上跑，不在每個開發者的機器上；kun 的 `matrix`、`fuzz` 同樣交給 CI。

## 已決定的事項

2026-10-10 確認：

1. **範圍：`ci.py third-party` 的同步、nextest 的實測和 mutation test 的範圍**。草稿原本還有時間紀錄（`test.py` 記每次跑的時間、`test.py times` 加總，只放在本機）和排程，都拿掉：效益不確定、要維護的腳本多。找出慢的測試和訂預算先不做；V8 和 Skia 的動態連結不做。
2. **整套測試由 CI 驗證，CI 不在這份的範圍**：`matrix`、`fuzz`、`wpt`、`wdspec` 交給使用者另外建立的 CI。`CLAUDE.md` 裡什麼時候在本機跑它們的規則，等 CI 建好之後再改。

2026-10-10 確認（第 1 段開始前）：

3. **什麼算「自己的工作」**：未提交的改動（untracked 的檔案不算），或 HEAD 是遠端沒有的 commit（向遠端 fetch 那個 commit，給不出來就是沒 push）。有的話不切換那個專案，說明原因，exit code 非 0。不選用「遠端的 branch 包含它」：淺 clone 沒有遠端的 branch，會把每個 checkout 都當成有工作。
4. **`ci.py third-party` 預設就同步**：拿掉 `--clone`，不留「只檢查」的模式：沒有東西需要只檢查，`ci.py third-party` 本來就該讓每個依賴一致。
5. **`test.py` 的每個子指令開始前都同步**：每個子指令都會編 kun，kun 經由路徑依賴 rusty_v8 和 parley；已經一致時只是幾個本機的 git 指令。不選只有長時間的那幾個：短的照樣會編到對不上的 fork。`cargo test` 不經過 `test.py`，不同步。
6. **同步用手動確認，不另外寫腳本的自動測試**：照 `sanity.py` 的先例（`docs/test-gaps.md` 的「只做過手動確認」）：這台的實際狀況，加上在 fork 裡留未提交的改動、做沒 push 的 commit。不選在 scripts/tests/ 寫 `unittest`：多一套測試和跑它的規則，只為了一個同步。

2026-10-10 確認（第 2 段開始前）：

7. **nextest 用 `cargo install cargo-nextest --locked` 裝**：從 crates.io、用 `rust-toolchain.toml` 的 toolchain 編。不選 nexte.st 的預編執行檔：快，但多一個要信任的下載來源。
8. **量法照 RFC 0025**：編好之後 `cargo test --workspace` 和 `cargo nextest run --workspace` 各跑 3 次取中位數；另外記下最慢的 20 個測試、測試數是否相同、每個測試一個 process 時 V8 初始化的成本。`harness = false` 的 `scenarios`、`speedometer` 用 nextest 的篩選排除，照舊用 `cargo test` 跑。
9. **macOS 的數字由使用者在 Mac 上量**：同樣的指令寫進 `progress.md`。
10. **換不換不設門檻**（第 15 條：不換）：量完把兩邊的數字和確認的結果交給使用者決定（「待決定」第 1 題）。

2026-10-10 確認（第 3 段開始前；macOS 的量測同時進行）：

11. **先不做 `test.py mutate --jobs`**（第 15 條：第 3 段整段不做）（原本「待決定」第 2 題）：規則一實施，每份 RFC 的 mutant 會少很多，平行的好處跟著縮小；`--jobs` 要 worktree，每個 job 第一次完整編一次（約 170 秒）、每個 target 目錄數 GB。規則先上，下一份 RFC 看 mutation test 還花多少時間，太慢再做。不選現在做：這份裡最複雜的腳本，效益要等規則生效之後才知道。
12. **規則**（第 15 條取代：不做）（改寫 `docs/testing.md` 的「Mutation test」）：寫這份 RFC 新的或改到的行為，優先寫風險高的（GC 和 `unsafe`、跨 process 的訊息、安全的檢查、錯誤處理的分支）；不寫 log 和錯誤訊息的字串、註解、只影響效能的、明顯等價的；每條驗一件事、同一行最多一條，測試指令用最小的一組，全部跑完再跑一次 `cargo test --workspace`。不設條數的上限。
13. **用 RFC 0030 第 4 段的 33 條驗證規則**（做了，結果讓第 15 條決定不做）：照新規則逐條分類（留下的、不寫的、為什麼），寫進 `progress.md`；只分類、不重跑。
14. **`CLAUDE.md` 指向規則**（第 15 條取代：不做）：「新的行為都要有測試，並且跑 mutation test」加上「照 `docs/testing.md` 的規則挑 mutant」，規則只寫在 `docs/testing.md`。

2026-10-10 確認（第 2 段的 Windows 量完之後、第 3 段開始時）：

15. **第 2 段（nextest）和第 3 段（mutation test 的範圍）不做**：nextest 在 Windows 上比 `cargo test` 慢 27%（每個測試一個 process，每個都初始化 V8），不換，macOS 不量了；mutation test 掃過 RFC 0026 到 0030 的 814 條，只改字串或註解的 15 條、改同一處的 21 條，而且多半是行為（例如 `groove` 讀回來變 `ridge`、同一個條件的兩個方向），「縮小範圍」的規則減不了，「同一行最多一條」反而會漏；`--jobs` 也不做。量的結果在 `progress.md`。取而代之的是第 2 段：本機的測試規則配合每天的 CI。
16. **整套的測試由每天的 CI 跑，本機只跑相關的部分**：CI 由使用者設定，每天跑一次 `matrix`、`fuzz`、`wpt`、`wdspec`、`reftest`、`scenarios`（和 `cargo test --workspace`）。做 RFC 時本機不跑 `matrix`、`fuzz`、`wdspec` 和整套的 `wpt`；`wpt`、`reftest`、`scenarios` 只跑改到的部分。`automation` 照舊（CI 沒有螢幕）。
17. **每段工作做完只跑改到的 crate 的 `cargo test -p`**，整個 workspace 在階段做完時跑（和 fmt、clippy 一起）。不選「受影響的 crate」（照依賴算出清單）：改底層時接近全部，還要一個腳本。不選照舊每段 `--workspace`：多 10 到 20 秒，整套有 CI。
18. **merge 不等 CI**：CI 上壞掉的看到就修。
19. **動到 GC 的 RFC 完成時不在本機跑 `matrix`、`fuzz`**：merge 之後看 CI 的結果；要重現 CI 的失敗時才在本機跑。

2026-10-10 確認（模擬一份 GC 型別的 RFC 時）：

20. **`test.py wpt`、`wdspec` 給了路徑時只跑清單裡的**：清單（`include.txt`、`wdspec-include.txt`）在那些路徑底下的行，或在清單的某一行底下的路徑；不在清單裡的印出來、不跑；`--all` 照路徑全部跑（試跑還沒列進清單的測試）。原本給路徑時照路徑全部跑：目錄裡還有清單沒列的測試（要 `Worker`、觸控筆……），沒有預期結果，全部算成和預期不同，第 16 條的「只跑相關的目錄」照做會很吵（模擬時 `html/user-activation/`、`webmessaging/` 74 秒、exit code 1；改了之後 23 秒、0 個和預期不同）。也擋掉一個碰過的坑：試跑整個目錄時加了 `MACHOPO_UPDATE_EXPECTATIONS`，替清單以外的測試寫出多餘的 `.ini`。

## 待決定

無。

## 之後的可能

- 找出慢的測試、訂時間預算（`progress.md` 的 nextest 的實測列出了最慢的 20 個），把慢的端到端測試改成可控的時鐘或移到下層。
- 限制端到端測試同時跑的數量（libtest 的 thread 數，或之後的 nextest 的 test group）：`shell` 和 `browser` 的端到端測試同時跑得越多，越容易拉長和時間有關的等待（第 2 段的 `webdriver` 那一個）；Chromium 的 test launcher 也限制 job 數。
- 受影響的測試（Chromium 的 `analyze`）：照改了哪些 crate，只跑依賴它們的 crate 的測試（第 17 條沒選）。
- CI 在每次 push 時跑一份快的（`cargo test --workspace`、`sanity.py`），merge 前等它（第 18 條之後可以重新考慮）。
- 時間紀錄、排程、nextest、mutation test 的 `--jobs`（拿掉的部分，第 1、15 條）。
