# RFC 0118 的細節：字型服務

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## machopo 現在的字型怎麼走

2026-11-26 讀的（`main`，`d8f79813`；parley 的 fork 是 `third_party.toml` 釘的那個 commit）。

- renderer 裡碰 fontique 的只有 `crates/layout/src/text.rs`（`TextContext`、`SystemFonts`）和 `crates/layout/src/font_families.rs`（一個 CSS 的 `font-family` 清單要向 `Collection` 問哪些名字）。Browser 的 UI 另外有自己的（`crates/ui/src/text.rs`）。
- `TextContext` 一個 renderer 的 main thread 一份（`render_thread.rs` 建；`--test-fonts` 時是只有內建兩個字型、不帶系統字型的 `Collection`）。Linux 上系統字型的那一份是 renderer 的 `main` 在進 sandbox 之前建的（`SystemFonts::find`），再一路交到 main thread。
- `TextContext::new(true)`（內建字型）另外用在 canvas 的文字量測和幾個暫時的地方：不碰系統。

### fontique 向系統要什麼

`fontique/src/backend/mod.rs` 照平台選一個 `SystemFonts`（`fontconfig.rs`、`coretext.rs`、`dwrite.rs`、`android.rs`，或什麼都沒有的那一個）。`Collection`（`collection/mod.rs`）對它做的事：

| 什麼時候 | 呼叫 | 各平台做什麼 |
| --- | --- | --- |
| 建 `Collection`（`system_fonts: true`） | `SystemFonts::new()`，之後讀它的 `name_map`（family 的名字和編號）和 `generic_families` | fontconfig：初始化、讀設定和 cache；CoreText：掃一遍系統的字型；DirectWrite：`GetSystemFontCollection`、`GetSystemFontFallback` |
| 第一次 `family(id)` | `SystemFonts::family(id)`，結果記在 `Collection` 裡 | 回 `FamilyInfo`：每個字型一個 `FontInfo`（來源、檔案裡的第幾個、寬度、樣式、粗細、可變軸、character map 的索引）。`FontInfo::from_source` 對照路徑的來源是打開檔案、mmap、讀屬性 |
| 第一次遇到一個 script 和語言 | `SystemFonts::fallback(key)`，結果記在 `fallback_cache` | fontconfig：照語言問；CoreText：`CTFontCreateForString`（要系統的字型服務）；DirectWrite：`IDWriteFontFallback::MapCharacters`，用那個 script 的一段樣本文字 |
| 第一次用一個字型的內容 | `SourceCache::get(source)` | 照路徑開檔、mmap；`SourceKind` 另有 `Memory(Blob)`（註冊進來的字型） |

- `SystemFonts` 的函式都是 `pub(crate)`，`Collection` 裡拿著的是 `Option<System>`：沒有讓外面給一個不同的來源的地方。
- `Collection` 另外有給外面用的：`register_fonts(blob, override)`（從內容註冊，回 family 的編號）、`set_fallbacks`、`append_fallbacks`、`set_generic_families`。
- fallback 的 key 是（script，語言）：fontique 只追蹤一部分語言（中日韓的幾種），其餘只看 script。

### `ipc` 現在有的

- renderer 到 Browser 的同步呼叫：`renderer` 的 scheduler 的 `sync_call`（`task_queue.rs`），等的時候照 scheduler 的規則；現在三處在用（`render_thread/documents.rs`）。
- 跟著訊息過去的 handle：一則最多 16 個，只能從 host 給 child；現在過去的是共用記憶體的區域。Windows 上另有 `spawn_handing`（啟動時交給 child 的檔案）。「跟著一則訊息過去的開著的檔案」三個平台都還沒有。

## 現在一頁問幾次、一次來回多久（量的）

2026-11-26，Windows 11（10.0.26300，x86-64），debug build，`d8f79813`。fork 的 fontique 暫時加了計數（沒有 commit，量完還原）：`Collection` 每向系統後端要一樣東西就在 stderr 印一行和花的時間。`machopo --remote-debugging-pipe --headless` 載入一頁（RFC 0117 的 `probe_windows.py`），數 renderer 的那幾行。

| 頁面 | 要 family | 要 fallback | 開字型檔 | 問名字（不在的） | 系統那一側總共花的 |
| --- | --- | --- | --- | --- | --- |
| 探測的頁面（中日韓、阿拉伯文、希伯來文、泰文、emoji、四種 generic family） | 11 | 8 | 12 | 0 | 4.6 ms |
| sqlite 的 about 頁（`tests/sites/`） | 2 | 4 | 4 | 1（0） | 5.5 ms（一個 family 4.2 ms） |
| Gutenberg 的一本書 | 2 | 4 | 4 | 0 | 1.8 ms |
| CERN 的第一個網站 | 2 | 4 | 3 | 0 | 1.6 ms |
| TodoMVC | 5 | 4 | 8 | 3（2） | 2.9 ms |
| `about:blank` | 1 | 2 | 1 | 0 | 1.0 ms |

- 一般的頁面大約 10 個問題，刻意逼出 fallback 的那一頁 31 個。每個 renderer process 只問這一輪（之後都在它自己的快取裡）。
- 系統回答一個問題多半不到 1 ms（最久的一個 family 4.2 ms）；這段時間現在就在付，搬到 Browser 之後一樣要付。
- 建字型的清單（`SystemFonts::new`，DirectWrite）一次 1.5 到 1.8 ms；現在每個 renderer 建兩次（為什麼是兩次沒有查）。Linux 上 fontconfig 的初始化量過是 13 到 19 ms（renderer sandbox 那份 RFC 的紀錄）。名字的清單由 Browser 給的話，renderer 這一段就沒有了。
- 「問名字」比預期少得多：這幾頁的 `font-family` 都短。一個名字不在時多的是一次來回。

**一次同步的來回**：頁面的 script 連續讀 `document.cookie` 5000 次（每次都是 renderer 到 Browser 的同步呼叫），有 sandbox 的 4 次量到每次 26.6 到 28.4 微秒，`--no-sandbox` 26.5 微秒；留在 renderer 裡的呼叫（`document.title`）是 0.2 微秒。Browser 那時是閒的。

所以多出來的成本（同步的來回本身）：一般的頁面 10 個問題約 0.3 ms，最多的那一頁 31 個約 0.9 ms，每個 renderer process 一次。沒量到的：Browser 的那條 thread 在忙時要等多久（cookie 是哪條 thread 回答的沒有查）；把開著的檔案交過去多花多少；macOS 和 Linux 上的數字；release build。

## 不改 fork 的原型

2026-11-26，同一台，debug build。`prototype.patch`（這個資料夾；套在 `d8f79813` 的 `crates/layout` 上，沒有進 repo）：renderer 的 `TextContext` 裡放兩個 `Collection`，一個帶系統字型（當作 Browser），一個不帶（排版用的）。後者只用 fontique 給外面的函式填：

- **family**：machopo 把 CSS 的 `font-family` 對到名字的那一步（`resolve_families`）之前，對每個名字向「Browser」要那個 family 的每個字型（哪個檔案、第幾個、粗細、樣式、寬度），拿檔案的內容 `register_fonts`。只有一個字型的檔案帶著系統給的 family 名和屬性註冊（`FontInfoOverride`），所以分組和系統的一樣（Windows 上 DirectWrite 把 Arial Black 算成 Arial 的一個粗細，照檔案自己的名字就不會）；一個檔案好幾個字型的（`.ttc`）不帶，照它們自己的名字。
- **generic family**：第一次用到時把每一種對到的 family 都要來、`set_generic_families`。
- **fallback**：文字交給 parley 之前（三處：inline 的排版、SVG 的一行、canvas 的文字）掃一遍它的字元，每一種還沒問過的 script，加上漢字，向「Browser」問、註冊、`set_fallbacks`。依據是 parley 問 fallback 的地方（`parley/src/shape/mod.rs` 的 `begin_segment`）：它問的是每一段的 script 加上這段文字的語言，另外固定問漢字的；一段的 script 是那段文字裡某個字元的 script（沒有特定 script 的字元跟前面的；`parley_engine` 的 `itemize.rs`）。原型用 `icu_properties` 取字元的 script。

結果：

| 頁面 | 截圖和現在比 | 問的 script | 註冊的 family | 交過來的檔案（其中 `.ttc`） | 原型這一側花的 |
| --- | --- | --- | --- | --- | --- |
| 探測的頁面 | 相同 | 7 | 11 | 41（4） | 4.3 ms |
| sqlite 的 about 頁 | 相同 | 3 | 10 | 39（4） | 3.7 ms |
| Gutenberg 的一本書 | 相同 | 3 | 9 | 35（4） | 3.3 ms |
| CERN 的第一個網站 | 相同 | 3 | 9 | 35（4） | 3.6 ms |
| TodoMVC | 相同 | 3 | 11 | 35（4） | 3.5 ms |

- 五頁的截圖都和直接問系統時逐 byte 相同；沒有一個 family 的名字對不上，沒有一個 family 被拆成兩個。
- **每一種 script 的 fallback 都先問好**（不掃文字，162 個 key）也試了：截圖一樣相同，但每個 renderer 要 40 ms、註冊 66 個檔案，不能用。所以要掃文字。
- **交過來的檔案比現在多**：現在一頁開 3 到 12 個字型檔，原型 35 到 41 個。註冊一個 family 要它每個字型的內容（Segoe UI 一個 family 十幾個檔案），generic family 又是一次全部要來。花的時間和現在同一級（現在系統那一側 1.6 到 5.5 ms），因為現在 fontique 建一個 family 時也是把每個檔案打開讀屬性；多的是每個檔案要經過通道交過來、在 renderer 裡 map 著。generic family 可以改成用到哪一種才要哪一種。
- 沒試到的：有 `lang` 的文字（fallback 的 key 帶語言；原型一律不帶）；在 macOS 和 Linux 上跑；五頁以外的頁面；可變字型的軸在排版時取的值。`.ttc` 和名字的問題在下一節。

### 照檔案註冊和系統說的對不對得上（每個平台）

原型留下的疑問是「照檔案註冊出來的 family，和系統後端說的一不一樣」。2026-11-26 讀了三個後端，並在 Windows 上把整台機器的字型比了一次。

- **macOS：一定一樣**。`backend/coretext.rs` 只向 CoreText 要字型檔的路徑（`CTFontCollection` 的每個 descriptor 的 URL，加上 `Library/Fonts` 底下的檔案），family 的名字、分組和屬性是 `scan::ScannedCollection::from_paths` 自己掃檔案讀的；`register_fonts` 用的是同一個 `scan` 模組。所以在 macOS 上照檔案註冊（不帶任何覆寫）得到的就是系統後端的那一份，`.ttc` 也一樣。沒有在 Mac 上跑過，這是讀程式得到的。
- **Windows：系統說了算，量過**。`backend/dwrite.rs` 的 family 和屬性來自 DirectWrite。`fontcheck.rs`（這個資料夾；一個獨立的小程式的 `main.rs`，依賴 fontique 和 `read-fonts`，不在 workspace 裡）把這台機器上每一個系統 family 照原型的方式註冊，逐一比：104 個 family（132 個名字）、220 個字型（156 個在單一字型的檔案裡，64 個在 `.ttc` 裡）。
  - `.ttc` 裡的 64 個字型：名字、分組、粗細、樣式、寬度全部對得上。
  - 對不上的 7 個 family 是另一種情況：**一個檔案在系統裡是好幾個 family**（可變字型的光學尺寸：`Segoe UI Variable` 的 Display、Text、Small 是同一個檔案，`Sitka` 的六個也是）。原型一個檔案只註冊一次，掛在第一個問到的名字底下，其餘的名字找不到。改成「單一字型的檔案，在系統裡屬於幾個 family 就各註冊一次」（多註冊 12 次）之後，0 個對不上。
  - **family 的其他名字**（28 個：同一個 family 在別的語言的寫法，例如 `ＭＳ 明朝` 是 `MS Mincho`）照檔案註冊的那一份不認得。所以名字到 family 的對應不能靠排版用的那個 `Collection` 的名字表：要由 machopo 照 Browser 給的名字的清單自己對（清單裡每個名字指到哪個 family），`FontFallbackMap::resolve` 現在是問 `Collection` 的。
- **Linux：系統說了算，量過**。`backend/fontconfig.rs` 的 family 名字、粗細、寬度、樣式、檔案裡的第幾個都來自 fontconfig 的 pattern。單一字型的檔案帶著 fontconfig 給的名字和屬性註冊，和 Windows 一樣；`.ttc`（Noto CJK 那一類）照檔案自己的。fontconfig 的設定可以改名、可以把粗細換算成別的值，所以 2026-11-27 在 Fedora 45（kernel 7.3.3，fontconfig 的預設設定，578 個字型）上用 `fontcheck.rs` 比了一次：114 個 family（155 個名字）、306 個字型（276 個在單一字型的檔案裡，30 個在 `.ttc` 裡：`NotoSansCJK-VF.ttc`、`NotoSerifCJK-VF.ttc`、`NotoSansMonoCJK-VF.ttc` 這三個可變字型，每個檔案五個語言的 family），0 個對不上；沒有「一個檔案在系統裡是好幾個 family」的單一字型檔案。照檔案註冊的那一份不認得的其他名字 40 個（fontconfig 給的另一種寫法，例如 `Source Code Pro Light` 是 `Source Code Pro`、`STIX Two Math` 是 `STIX`），和 Windows 一樣由 Browser 給的名字的清單對。使用者自己的 fontconfig 設定改了 `.ttc` 的名字或粗細時會對不上，沒試。

## Chromium 的做法

2026-11-26 讀的（`main`）。有兩套並存：新的 `FontDataService` 在 Windows 一律用（`content/common/features.cc` 的 `IsFontDataServiceEnabled()` 在 Windows 直接回 true），在 Linux 和 ChromeOS 預設開；舊的 Linux `FontService` 還負責 fallback 和 render style；Windows 的 `DWriteFontProxy` 在有 sandbox 的 renderer 裡已經不是 font manager（`renderer_main_platform_delegate_win.cc`：「Fallback to only DWrite if running in single process mode」）。

### `FontDataService`（`components/services/font_data/`）

- **介面**（`public/mojom/font_data_service.mojom`）：五個呼叫，都是同步的：`MatchFamilyName(family_name, style)`、`MatchFamilyNameCharacter(family_name, style, bcp47s, character)`（fallback）、`GetAllFamilyNames()`（註解：「Usage of this function is strongly discouraged」）、`LegacyMakeTypeface`、`MatchLocalFont(font_unique_name)`（`src: local()`，在一個預設關著的 feature 後面）。
- **過去的資料**：`union TypefaceData { ReadOnlySharedMemoryRegion region; TypefaceFile font_file; }`；`TypefaceFile` 是唯讀的檔案 handle 加一個編號（「unique identifier ... to avoid remapping the same file multiple times」）；另有 `ttc_index`、可變軸的位置、要不要合成粗體和斜體。失敗分「這組 family 和 style 沒有」和「沒有這個 family」。
- **Browser 那一側**（`font_data_service_impl.cc`）：用 Skia 的預設 font manager 回答；跑在一個會等待的背景 sequence 上；優先給檔案的 handle（「avoiding copy of the content of the file」），拿不到檔案時把內容複製進共用記憶體（最多留 128 個）；路徑對編號、沒有的 family 都有快取。
- **renderer 那一側**（`content/child/font_data/font_data_manager.{h,cc}`）：一個 `SkFontMgr`，「may be called on any thread」，每個 sequence 自己的連線；快取：字型（LRU 128，鍵是 family、粗細、寬度、斜度；失敗也記）、沒有的 family（64）、map 過的檔案（照編號）和區域；map 了之後不放掉（「never unmapped or replaced, as we have already given out typeface objects」）。**fallback 那個呼叫在這一層沒有快取**（Blink 上面有沒有，沒讀）。
- 介面的註解沒有寫設計的理由，只有一個 bug 的編號。

### 舊的 Linux `FontService`（`components/services/font/`）

- 六個呼叫：`MatchFamilyName`、`OpenStream(id)`（回唯讀的檔案 handle）、`FallbackFontForCharacter(character, locale)`、`FontRenderStyleForStrike`、`MatchFontByPostscriptNameOrFullFontName`、PDF 用的一個。
- 理由（介面的註解）：「This service needs full filesystem access because fonts can be configured to live anywhere on the filesystem... hiding all filesystem details」。但字型的身分裡帶著路徑，也送給 renderer，旁邊的 TODO：「plug this system path leak」。
- renderer 那一側：`FontLoader` 是一個 `SkFontConfigInterface`；同步是 renderer 自己做出來的（專用的 thread 送出去，呼叫的一方等一個 event）；檔案在 renderer 裡 mmap；字型的快取 42 個（註解：Wikipedia 的頁面用到 21 個 fallback 字型，取兩倍）。
- fallback：Blink 的 `FontCache::GetFontForCharacter` 經 `WebSandboxSupportLinux`，照字元有一份每個 process 的快取（「Blink calls GetFallbackFontForCharacter frequently, so the results are cached」）。每個新的 fallback 字型是兩次同步的來回（fallback、`OpenStream`）。

### Windows 的 `DWriteFontProxy`（現在只給 `--single-process`）

- 六個同步的呼叫：`FindFamily`、`GetFamilyCount`、`GetFamilyNames`、`GetFontFileHandles(family_index)`（那個 family 的每個檔案的 handle：「We pass handles for every path as the sandbox blocks direct access to font files in the renderer」）、`MatchUniqueFont`、`MapCharacters`。
- renderer 那一側是自己的 `IDWriteFontCollection`：DirectWrite 還在 renderer 裡，只是字型集合和檔案從 Browser 來。一個 family 第一次載入大約兩到四次同步的來回（照程式數的）。handle 故意不關（當成 key 用）。
- fallback 的快取：鍵是基底的 family 和語言，值是最近用過的最多 10 個 family，命中與否在 renderer 裡照每個字看；註解警告語言分不出中日韓時快取會被錯的字型汙染。

### macOS：沒有字型服務

- `sandbox/policy/mac/common.sb` 的 `allow-font-access`：放行讀三處字型的目錄、查 `com.apple.FontObjectsServer` 和 `com.apple.fonts`，下載的字型用系統發的 extension。
- fallback 在 renderer 裡：`fonts/mac/font_cache_mac.mm` 的 `GetSubstituteFont` 叫 `CTFontCreateForString`。
- 以前把字型的內容從 Browser 經共用記憶體送過去的那段（`content/common/mac/font_loader`）已經不在了。

## Firefox 的做法

2026-11-26 讀的（GitHub 上 `mozilla-firefox/firefox` 的 `main`）：

- 共用的字型清單（`gfx/thebes/SharedFontList-impl.h`）：「collection of data that lives in shared memory... metadata needed for CSS font-matching (a list of all the available font families and their faces, style properties, etc, as well as character coverage)」。紀錄有 family、face（樣式、一個照平台不同的描述：PostScript 名、路徑或 fontconfig 的 pattern、檔案裡的第幾個、character map）、`local()` 的名字、別名。表裡沒有字型檔的內容。
- 「The data is updated only by the parent process; content processes have read-only access」。只增不減的配置，分成好幾塊共用記憶體；字型裝了或移除時整份重建，用世代的編號分。
- 延後填入：啟動時只有 family 的名字（那幾塊的 handle 跟著啟動 process 的參數）；之後有同步的（拿新的一塊、初始化一個 family）和不同步的（content process 自己讀了某個字型的 character map 之後交給 parent，「so that the parent process can share the map with all processes」）。
- fallback 不經過 IPC：走遍清單裡的 family 找有那個字的；character map 還沒載的 family 先跳過、開始載，載完之後 parent 讓大家重排。

沒有從原始碼確認的：content process 怎麼拿到字型檔的內容（照記憶：它照描述自己開檔，sandbox 放行字型的目錄）。

## 對 machopo 的意思

- Chromium 新的介面和 fontique 的後端幾乎是同一個形狀：照名字問一個 family、照字（fontique 是照 script）問 fallback、拿檔案。差別是 Skia 的比對是「給我這個名字、這個樣式最合的那個字型」，fontique 是「給我這個 family 的全部字型，我自己比」；所以 machopo 的問題是照 family 問，答案是那個 family 的字型的清單。
- Chromium 在 macOS 上沒有做這件事，renderer 的 sandbox 也就一直放行字型的目錄和字型服務。machopo 在 macOS 上做的話是比 Chromium 緊；值不值得是「待決定」的範圍那一條。
- Chromium 的經驗裡和這份有關的幾點：檔案 map 了不放（已經交出去的字型物件還指著它）；失敗也要快取；renderer 不該拿到路徑；每個新的 fallback 字型兩次來回被認為是可以接受的。

## 沒有查到的

- Chromium 的 Browser 在哪裡把這幾個服務接到 renderer；Blink 上層的 fallback 快取；`FontDataService` 的設計理由和它量到的效能。
- Firefox 各平台的 fallback（`PlatformGlobalFontFallback`）。
- machopo 的 worker 裡的文字怎麼拿字型；`src: local()` 現在有沒有。
- fontique 的上游對「字型來源由外面給」有沒有討論過。
