# RFC 0118：字型服務（系統字型由 Browser 給 renderer）

- 開始日期：2026-11-26
- 狀態：🚧 已排程：2026-11-27（第 1 到 4 段做完；第 5 段：Windows 的 restricted token，放在這份還是另一份待決定；進度在 `progress.md`）
- 細節：`details.md`

## 摘要

renderer 現在自己向作業系統問有哪些字型、自己開字型檔（parley 的 fontique：Linux 經過 fontconfig，macOS 經過 CoreText，Windows 經過 DirectWrite），所以三個平台的 sandbox 都為它留了一塊：Linux 放行字型的目錄，macOS 放行字型的目錄和系統的字型服務，Windows 因為 DirectWrite 在更緊的 token 底下不能用而停在 AppContainer。這份把「問系統」搬到 Browser：renderer 的 fontique 換成一個向 Browser 問的後端，Browser 用現在同一份 fontique 回答，字型檔由 Browser 開好、把開著的檔案交給 renderer。做完之後 renderer 不開任何字型的檔案、不碰系統的字型服務，三個平台的 sandbox 各收緊一級。fontique 的 fork 不改；family 的名字的清單在 renderer 啟動時給，其餘用到才同步問 Browser；三個平台都做，分段，Windows 先（已決定的事項第 1 到 4 條）。

## 動機

現況（2026-11-26 的 `main`，`d8f79813`）：

- renderer 的文字只經過 `layout` 的 `TextContext`（`crates/layout/src/text.rs`、`font_families.rs`），它拿著 fontique 的 `Collection`（有哪些 family、每個 family 的字型、fallback）和 `SourceCache`（字型檔的內容，照路徑讀）。
- fontique 向系統要三樣東西，都是用到才要：family 的名字的清單（建立時）、某個 family 的每個字型（第一次用到那個 family 時；會打開每個字型檔讀它的屬性）、某個 script 和語言的 fallback 是哪個 family（第一次遇到那個 script 時）。字型檔的內容在第一次用那個字型時照路徑 mmap。
- 為了這些，sandbox 留著的（`docs/limitations.md` 的「隔離」）：
  - Linux：字型檔的目錄整個可讀（系統的和使用者自己的；放在裡面的其他檔案也讀得到）；鎖之前要先初始化 fontconfig，所以鎖不是 `main` 的第一件事；Browser 啟動時要列一次字型的目錄、經命令列交給每個 renderer；Browser 執行期間新裝的字型要重開才看得到。
  - macOS：字型的四處目錄可讀，連得到系統的字型服務（`com.apple.fonts`：系統的一個 process，renderer 能對它送訊息）；字型管理軟體裝在別處的字型列得到、讀不到。
  - Windows：renderer 讀得到開放給所有應用程式的地方（`C:\Windows`、`Program Files`）。Chromium 的 renderer 用更緊的 token（什麼都開不了），machopo 用不了，因為 DirectWrite 的系統字型在它底下整頁空白。

**目標**：

1. renderer process 不開任何字型檔、不查系統的字型清單、不連系統的字型服務：這些只有 Browser 做。
2. 畫出來的文字不變：同一頁在做完前後的截圖相同（三個平台，系統字型）。
3. 不變慢到看得出來：Speedometer 和文字多的頁面的載入時間在雜訊裡；renderer 啟動不多等。
4. sandbox 跟著收緊，每個平台一步：
   - Linux：不放行字型的目錄，鎖回到 `main` 的第一件事，命令列的字型目錄和 Browser 啟動時的那一輪列舉拿掉。
   - macOS：profile 拿掉字型的目錄和 `com.apple.fonts`。
   - Windows：在 AppContainer 上疊 Chromium 的 restricted token（`USER_LOCKDOWN`）。
5. `--single-process`、`--test-fonts`、Browser 自己的 UI 的文字照舊（它們不經過這條路）。

**不在範圍內**：

- 網頁自己帶的字型（`@font-face` 的 `url()`）：它們本來就經過網路的那條路，renderer 拿到的是位元組。
- 改 fallback 的規則、family 名字的對應（`font_families.rs`）：這份只換「誰去問系統」，問的內容和答案不變。
- 環境變數的過濾：另一件事（renderer sandbox 那份 RFC 的「之後的可能」）；字型服務做完之後它的清單可以拿掉字型用的那幾個。

要解決的 `docs/limitations.md` 裡的限制：「Linux 的 sandbox 擋不住的」的 (2)、「macOS 的 sandbox 擋不住的」的 (2)、「Windows 的 sandbox 擋不住的」的 (1)；Linux 上 Browser 執行期間新裝的字型要重開才看得到的那一條（看選的做法）。

## 設計概要

給 machopo 的使用者：看不到改變。

給改 machopo 的人：

- renderer 這一側要字型時照舊用 `TextContext`；它底下的 fontique 不再碰系統，改成問 Browser。在 renderer 裡直接開字型檔、叫 CoreText、DirectWrite、fontconfig 的程式，做完之後在 sandbox 裡會失敗。
- Browser 多一個回答字型問題的地方（Chromium 的 `FontDataService`），它擁有唯一一份「問系統」的 fontique。
- 測試字型（`--test-fonts`）是編在執行檔裡的，不經過字型服務。

## 詳細設計

下面每一節是選項和比較；定了的寫在「已決定的事項」，沒有對應的條目的「看法」是 2026-11-26 的。fontique 現在的結構和 Chromium、Firefox 的做法（都對過原始碼）在 `details.md`。

### 誰來決定「這個名字是哪個字型」「這個字用哪個字型」

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| A1 | **Browser 回答，renderer 快取**：renderer 第一次用到一個 family、一個 fallback 時同步問 Browser，之後記著。Browser 用 fontique 的系統後端回答 | 答案和現在一樣（同一段程式算的，只是換了 process）；renderer 這一側很薄；系統的字型變了，Browser 下一次回答就是新的 | 每個 renderer process 對每個 family、每種 script 各有一次同步的來回；新的分頁（新的 process）從頭再問 |
| A2 | **整份清單放在共用記憶體**：Browser 啟動時把所有 family、字型的屬性、每個字型有哪些字（character map）建成一張表，每個 renderer 唯讀地 map 它，比對和 fallback 都在 renderer 裡算 | 平常沒有同步的來回 | 要設計和維護一張跨 process 的表（配置、版本、延後填入）；fallback 改成 renderer 自己照字找，和現在問系統的答案不一定一樣（現在 Windows 問 DirectWrite、macOS 問 CoreText）；Browser 啟動時要讀每個字型檔 |
| A3 | **A1，但 family 的名字在啟動時一次給**：名字的清單（幾百個字串）放在 renderer 的第一則訊息裡，其餘照 A1 | 合 fontique 現在的結構（它一建立就要名字的表），fork 改得最少；「這個名字在不在」不用問；renderer 不用自己建清單（現在每個 renderer 1.5 ms 以上，Linux 十幾 ms）；其餘同 A1 | 第一則訊息多幾十 KB；Browser 執行期間新裝的字型，已經開著的 renderer 看不到（新的分頁看得到） |

看法：**A3**。對照：Chromium 是 A1（`FontDataService`：`MatchFamilyName`、`MatchFamilyNameCharacter` 都是同步的，renderer 有一層 LRU 的快取；它說「取全部 family 的名字」這個呼叫「strongly discouraged」，因為 Skia 的比對是照名字問、不用清單，而 fontique 是先有名字的清單再比對）。Firefox 是 A2（它自己的說明：共用的清單裡有 family、字型的屬性和 character map，由 parent process 建，content process 唯讀；fallback 在 content process 裡走遍清單，還沒有 character map 的 family 先跳過、之後補上再重排）。A2 換到的是沒有同步的來回，要付的是一套新的資料結構和一份和系統不一定一致的 fallback。量過了（`details.md` 的「現在一頁問幾次、一次來回多久（量的）」）：一般的頁面每個 renderer process 大約 10 個問題、最多的一頁 31 個，一次同步的來回約 27 微秒，合計不到 1 ms，所以 A2 省下的量不出來。A1 和 A3 的差別也不在次數（「這個名字在不在」一頁只問 0 到 3 次），在 fontique 的結構：它的 `Collection` 一建立就要有名字和編號的對照表，照名字比對都從那張表開始；A1 要把這張表也改成用到才問，fork 要改得更深。

### fontique 怎麼接

fontique 的系統後端是編譯時選的一個型別（`backend::SystemFonts`：fontconfig、CoreText、DirectWrite 各一份），`Collection` 向它要三樣：名字的清單和 generic family 的對應（建立時）、`family(id)`、`fallback(key)`。沒有讓外面換掉它的地方。

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| Q1 | **fork 加一個「字型來源由外面給」的介面**：`Collection` 可以用一個實作了那三樣的物件建（trait），系統後端是它的一種實作 | renderer 那一側只要實作這個 trait（問 Browser）；`Collection` 其餘的行為（延後載入、快取、比對）不變；介面小，寫成可以送上游的樣子 | 改 fork：fontique 的 `FamilyInfo`、`FontInfo` 現在要從字型的內容建，介面要讓外面給得出這兩樣；上游收不收不知道 |
| Q2 | **不動 fork**：renderer 的 `Collection` 不帶系統字型（`system_fonts: false`），machopo 在上面一層照需要把字型註冊進去（`register_fonts`）、把 fallback 填進去（`set_fallbacks`） | fork 不多一個 commit；原型做過，畫出來相同 | fallback 是 parley 在 shaping 裡面才問的，machopo 要在 shaping 之前先掃文字有哪些 script、先填好；註冊一個 family 要它每個字型的內容，交過來的檔案比現在開的多（一頁 35 到 41 個，現在 3 到 12 個）；`.ttc` 裡的字型只能照它自己的名字和屬性註冊 |

看法（2026-11-26，原型之後改的）：**先照 Q2**。原本的看法是 Q1，理由是 Q2 要把 parley 裡面的一段邏輯複製到外面；讀了 parley 問 fallback 的地方，要複製的只是「這段文字有哪些 script，加上漢字」，做了原型（`details.md` 的「不改 fork 的原型」）：五頁的截圖都和現在逐 byte 相同，fork 一行沒動。Q2 留下的風險：猜漏了是安靜的錯（少一個 fallback、畫出豆腐），parley 改了問法時 machopo 要跟著改；用一個測試守（同一組文字，兩種 `Collection` 排出來的字形逐一比）。原本擔心 `.ttc` 裡的字型不能帶著系統給的名字和屬性註冊；查了之後（`details.md` 的「照檔案註冊和系統說的對不對得上（每個平台）」）：macOS 的後端本來就是自己掃檔案，照檔案註冊一定一樣；Windows 上整台機器比過，`.ttc` 的都對得上，對不上的是「一個檔案在系統裡是好幾個 family」（各註冊一次就好）和 family 的其他名字（名字由 machopo 照 Browser 的清單自己對，不靠 `Collection` 的名字表）；Linux 上整台機器比過（2026-11-27，Fedora 45），可變字型的 `.ttc` 也都對得上，fork 不用改。

### Browser 和 renderer 之間問什麼

照 A3 和 Q1，介面就是 fontique 後端的那三樣，加上字型檔的內容：

| 問 | 什麼時候 | 答 |
| --- | --- | --- |
| （不用問）family 的名字、generic family 是哪幾個 | renderer 啟動，在第一則訊息裡 | 名字的清單，每個名字一個編號；generic family 對到的編號 |
| 這個 family 有哪些字型 | 第一次用到這個 family | 每個字型：哪個檔案（一個編號）、檔案裡的第幾個、它的屬性（或讓 renderer 從內容讀，見下面） |
| 這個 script、這個語言的 fallback 是哪個 family | 第一次遇到這個 script | 一個 family 的編號，或沒有 |
| 這個檔案的內容 | 第一次用到這個檔案裡的字型 | 開著的檔案（renderer 自己 mmap），或共用記憶體 |

- 問的都是同步的：排版要當場知道。renderer 已經有同步呼叫（`scheduler.sync_call`，現在三處在用）。
- 每一種答案 renderer 記著，同一個 process 不問第二次（fontique 的 `Collection` 本來就記 family 和 fallback，`SourceCache` 記內容）。
- **Browser 不信 renderer 給的編號**：每個都查表，查不到就是壞訊息（「Process 的邊界」第 6 條）。renderer 拿不到路徑，只有編號（Chromium 舊的 Linux 字型服務把路徑也給了 renderer，它自己的註解說要補上這個洞）。
- fontique 的 fallback 是照 script 和語言問的（一種 script 一次），不是照每個字：比 Chromium 照字元問的少很多次。

### 字型檔的內容怎麼過去

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| D1 | **開著的檔案**：Browser 開好字型檔，把檔案的 handle（Unix 的 fd）跟著訊息交給 renderer，renderer 自己 mmap | 不複製；sandbox 不擋已經開著的檔案；和現在的記憶體用量一樣（同一個檔案的頁面各 process 共用） | `ipc` 現在跟著訊息過去的是共用記憶體，要多一種「開著的檔案」（三個平台；macOS 的通道是 Mach 訊息，fd 要包成 port）；renderer 拿到的是整個檔案的讀取權 |
| D2 | **複製進共用記憶體**：Browser 讀檔、放進一塊共用記憶體交過去 | `ipc` 不用新的東西 | 每個用到的字型檔在記憶體裡多一份（CJK 的字型一個十幾 MB）；Browser 要管這些區域什麼時候放掉 |
| D3 | **Browser mmap 檔案，把那塊對應交過去** | 不複製 | 「一塊 mmap 的檔案」在三個平台上變成能交給別的 process 的東西，做法各不相同，等於 D1 的另一種寫法 |

看法：**D1**。對照 Chromium：`FontDataService` 優先給檔案的 handle（它的註解：「avoiding copy of the content of the file」），拿不到檔案時才複製進共用記憶體；renderer map 了之後不再放掉。

### 字型的屬性誰讀

fontique 建一個 family 時要每個字型的寬度、樣式、粗細、可變軸、有哪些字的索引：現在是打開檔案讀出來的。

| | 做法 | 說明 |
| --- | --- | --- |
| P1 | renderer 拿到檔案之後自己讀 | 用現在同一段程式；代價是「這個 family 有哪些字型」的答案要連同每個字型的檔案一起給，一個 family 有十幾個檔案時一次交十幾個 |
| P2 | Browser 讀好、把屬性放在答案裡 | renderer 只在真的用到某個字型時才要它的檔案；代價是 fontique 的 `FontInfo` 要能從外面給的屬性建（fork 多一點） |

看法：照 Q2 的話只能是 **P1**（`register_fonts` 要內容）；P2 要改 fork。原型量到的代價是每個 renderer 多交二三十個檔案。

### 各平台做完之後收緊什麼

每個平台一步，各自可以先 merge：

- **Linux**：policy 不再放行字型的目錄；renderer 的 `main` 不再先初始化 fontconfig，進 sandbox 回到第一件事；`--font-directory`、Browser 啟動時列目錄的那一輪拿掉。時區和其他零星的讀取（`/etc/localtime`……）不在這份。
- **macOS**：profile 拿掉字型的四處目錄和 `com.apple.fonts`。要確認的：CoreText 在 Browser 裡回答 fallback 之後，renderer 自己拿到字型的內容能不能照樣 shaping（fontique 的 shaping 不經過 CoreText，預期可以；探測）。
- **Windows**：renderer 不再載入 `dwrite.dll`。之後才能在 AppContainer 上疊 restricted token：那是 sandbox 的機制，不是字型的事，但它是做這份的主要理由之一；放在這份的最後一段，還是另開一份，待決定。

### 不經過字型服務的

- `--test-fonts`：內建的兩個字型，照舊。
- `--single-process` 和在測試的 process 裡建的分頁：renderer 是 Browser 的 thread，直接用系統後端，照舊。
- Browser 的 UI（`ui` crate、網址列）：在 Browser 裡，照舊。
- `--no-sandbox` 的 renderer process：走字型服務還是照舊直接問系統，待決定（看法：一樣走字型服務，少一條只有除錯時才走的路）。

### 測試

- **畫出來不變**：同一組頁面（各種 script、粗細、generic family、找不到的名字、fallback）在 sandbox 裡外、做完前後的截圖相同；三個平台各自的 `a_sandboxed_renderer_shows_a_page_as_one_outside_does` 已經在比 sandbox 裡外。
- **renderer 真的不碰字型了**：每個平台的「做不到」的測試加上字型的目錄（讀不到）、系統的字型服務（連不到）；Windows 上 renderer 載入的 DLL 裡沒有 `dwrite.dll`。
- **介面**：renderer 給壞的編號時 Browser 當成壞訊息；同一個問題不問第二次（數同步呼叫的次數）；Browser 結束或不回答時 renderer 不卡住。
- **fork**：fontique 的新介面在 fork 裡有自己的測試（一個假的來源）。
- mutation test；量測：同步呼叫的次數和時間（一頁文字多的頁面、Speedometer 使用者要求時）。

### 分階段

0. 草稿、量測和原型（做完，2026-11-26 到 27；`details.md`）：一頁問幾次、一次來回多久；不改 fork 的原型；照檔案註冊和系統說的對不對得上；定了第 1 到 4 條。
1. `ipc` 多一種跟著訊息過去的東西（開著的檔案）。先做 Windows 的；Linux 和 macOS 的在各自的那一段。
2. **Windows**：Browser 的字型服務和 renderer 那一側（這兩塊不分平台，在這一段做出來）；有 sandbox 的 renderer 切過去，畫出來要和切之前相同；renderer 不再載入 `dwrite.dll`。
3. **Linux**：`ipc` 的檔案；renderer 切過去；policy 不放行字型的目錄，進 sandbox 回到 `main` 的第一件事，`--font-directory` 和 Browser 啟動時列目錄的那一輪拿掉。先跑 `fontcheck.rs` 看 `.ttc`。
4. **macOS**：`ipc` 的檔案（file descriptor 包成 port）；renderer 切過去；profile 拿掉字型的目錄和 `com.apple.fonts`。先探測：Browser 回答 fallback 之後，renderer 不連系統的字型服務時排版對不對。
5. Windows 在 AppContainer 上疊 restricted token（放在這份還是另一份，待決定）。

每一段在它的平台上做和驗，做完可以先 merge（`rfcs/README.md` 的「階段的 merge」）：還沒切的平台照舊自己問系統。

## 缺點

- 排版多了同步的來回：renderer 的 main thread 等 Browser。次數有上限（每個 process、每個 family 和 script 一次），但第一頁的第一次排版會集中遇到。
- Browser 多一塊要一直在的服務，它卡住時每個 renderer 的排版都卡住。
- fork 多一個 commit，上游不收的話要一直帶著。
- `ipc` 多一種 handle，三個平台各有一段新的 `unsafe` 和論證。

## 理由和其他做法

- **不做的話**：三個平台的 sandbox 都停在現在這一級，Windows 上不了 Chromium 的 token。
- **只做 Windows**（另外兩個平台照舊讀字型的目錄）：Windows 是唯一「不做就上不了下一級」的；但字型的路分成兩種之後，文字的 bug 要在兩種底下各看一次，而 Linux 和 macOS 能拿掉的那幾條放行也是真的收緊。看法是三個平台都做，分段。
- 各節的選項比較在上面。

## 先例

2026-11-26 對過原始碼（`details.md`）：

- **Chromium**：Windows 和 Linux 用 `FontDataService`（2024 年起；五個同步的呼叫，答案是開著的檔案或共用記憶體，renderer 有 LRU 的快取）；Linux 的 fallback 和 render style 還走舊的 `FontService`；macOS 沒有字型服務，renderer 直接讀字型的目錄、連系統的字型服務、自己叫 CoreText。
- **Firefox**：共用記憶體裡的一份字型清單，parent 建、content 唯讀，fallback 在 content process 裡算。

## 已決定的事項

1. **fontique 的 fork 不改：renderer 的 `Collection` 不帶系統字型，由 machopo 照需要填（Q2）**（2026-11-26，使用者決定；先做了原型和整台機器的比對才定的，`details.md` 的「不改 fork 的原型」「照檔案註冊和系統說的對不對得上（每個平台）」）：family 在 machopo 把 CSS 的 `font-family` 對到名字的那一步向 Browser 要、用 `register_fonts` 註冊；generic family 用到時 `set_generic_families`；fallback 在文字交給 parley 之前掃它的字元，每一種還沒問過的 script 加上漢字，問好之後 `set_fallbacks`。跟著定的兩點：單一字型的檔案，在系統裡屬於幾個 family 就各註冊一次，帶著系統給的 family 名和屬性；名字對到哪個 family 由 machopo 照 Browser 給的清單自己對，不靠排版用的 `Collection` 的名字表。理由：原型的五頁截圖和現在逐 byte 相同；Windows 上 104 個 family 逐一比過都對得上；macOS 的後端本來就是自己掃檔案，照檔案註冊一定一樣；量到的代價是每個 renderer 多交二三十個字型檔，合計不到 1 ms；fork 不多一個要長期帶著、每次跟上游都可能衝突的 commit。代價：字型的屬性只能由 renderer 從檔案讀（P1），一個 family 第一次用到時它全部的檔案都要交過來；「掃文字有哪些 script」和 parley 問 fallback 的方式綁在一起，parley 改了問法時 machopo 要跟著改（猜漏了是少一個 fallback、畫出豆腐，不會報錯），用一個測試守：同一組文字，machopo 填的和直接問系統的兩種 `Collection` 排出來的字形逐一比。不選在 fork 加「字型來源由外面給」的介面（Q1：fallback 用到才問、沒有猜的問題，可以只要用到的那幾個粗細；但要一個寫成能送上游的 commit，上游收不收不知道）。之後才會回頭改 fork 的情況：Linux 上被 fontconfig 的設定改過名字或粗細的 `.ttc`（2026-11-27 在 Fedora 45 的預設設定上量過，對得上；使用者自己的設定改了它們時才會對不上，那時加的是「註冊檔案裡的某一個字型、帶上指定的名字和屬性」的小函式）。
2. **Browser 回答，renderer 記著；family 的名字的清單在 renderer 啟動時一次給（A3）**（2026-11-27，使用者決定）：renderer 的第一則訊息裡有每個名字（含同一個 family 的其他寫法）指到哪個 family、generic family 各是哪幾個；排版遇到一個名字時自己查表，有才向 Browser 要那個 family 的字型；遇到一種新的 script 時問 fallback 是哪個 family；要到的都記著，同一個 process 不問第二次。問的都是同步的（renderer 現有的 `sync_call`）。理由：答案和現在一樣（Browser 用現在同一份 fontique 的系統後端回答）；量到一般的頁面每個 process 約 10 個問題、最多的一頁 31 個，一次來回約 27 微秒，合計不到 1 ms（`details.md`）；名字本來就要由 machopo 自己對（第 1 條），啟動時給齊比邊問邊長直接；不在這台機器上的名字（CSS 的字型清單常常列一串）不用各問一次；renderer 不用自己建字型的清單。代價：Browser 執行期間新裝的字型，已經開著的分頁看不到，新的分頁才看得到（現在 macOS 和 Windows 上看得到，Linux 上看不到）；字型變了時通知 renderer 重拿清單記在「之後的可能」。不選全部用到才問（A1，Chromium：新裝的字型立刻看得到，不在的名字每個多一次來回）。不選整份清單放共用記憶體（A2，Firefox：省下的是那不到 1 ms，要一套跨 process 的表，而且 fallback 改成 renderer 自己照字找，答案會和系統的不同）。
3. **字型檔的內容：Browser 開好，把開著的檔案交給 renderer，renderer 自己 mmap（D1）**（2026-11-27，使用者決定）：`ipc` 的通道底層本來就帶得了這種東西（Linux 是 file descriptor、Windows 是 handle、macOS 是 Mach port），訊息這一層多一種「開著的檔案」；macOS 把 file descriptor 包成 port、另一側還原。renderer 拿到的是那個檔案的讀取權，拿不到路徑；map 了之後不放（已經交出去的字型還指著它）。理由：不多用記憶體（同一個字型檔在每個 process 裡是同一批頁面，用到才載入）；三個平台的 sandbox 都不擋已經開著的檔案。不選複製進共用記憶體（D2：不改 fork 之後一個 renderer 要三四十個字型檔，中日韓的一個十幾到三十 MB，每個分頁都多一份）。Chromium 的 `FontDataService` 優先給檔案的 handle，拿不到檔案時才複製；machopo 的系統字型都有路徑，那條退路不做。要驗的：macOS 上 sandbox 裡的 renderer 還原那個 port、mmap 它。
4. **三個平台都做，分段，Windows 先（R1）**（2026-11-27，使用者決定）：Browser 的字型服務和 renderer 那一側不分平台（Browser 用 fontique 回答，哪個平台由 fontique 自己知道），做一次；每個平台是「分階段」的一段：那個平台的 renderer 切過去、sandbox 跟著收緊、在那個平台上驗。Windows 先：它是唯一不做就上不了下一級 sandbox 的，原型和比對也是在它上面做的。理由：只切一個平台的話，文字的路永遠分成「經過 Browser 的」和「直接問系統的」兩種，之後每個文字的問題都要分兩種看、兩條路都要測試。代價：Linux 和 macOS 的那兩段要在那兩台機器上做；macOS 那一段收益最小、未知最多（Chromium 在 macOS 上沒有字型服務，renderer 直接讀字型的目錄、連系統的字型服務；`ipc` 在 macOS 上傳檔案沒驗過）。做完之後 macOS 的 renderer 比 Chromium 的緊。不選只做 Windows（R2：工作量最少、都驗過）。不選 Windows 和 Linux、macOS 不做（R3：對齊 Chromium）。
5. **開著的檔案是訊息的值裡的一種欄位（F1）**（2026-11-27，第 1 段開始時，使用者決定）：`ipc` 多一個欄位的型別，序列化成「這則訊息跟著的第幾個 handle」，codec 的 `Place` 和 `Resolve` 各多一個方法；同一個 process 裡不經過 codec 時它就是檔案本身。理由：和 `data_pipe::Consumer`、`SharedBuffer` 同一個做法，協定的型別直接寫它，號碼由 codec 對，不會由協定對錯；Mojo 也是這樣（`mojo_base.mojom.ReadOnlyFile` 是訊息裡的欄位，handle 照索引帶過去）。不選 `send` 另外帶一串檔案、收的一側另外拿（F2：`ipc` 改得少，但協定要自己對號碼，對錯了是安靜的錯）。跟著定的：map 在 `ipc` 裡做（`unsafe` 只在那裡），renderer 拿到的是 map 好的唯讀位元組。論證的前提是 map 著的期間檔案的內容不變：fontique 現在自己 map 系統字型時也是這個前提；Windows 上由 kernel 守住（Browser 開檔時不讓別人寫，map 著的檔案不能截短），Linux 和 macOS 上是假設，寫在 `crates/ipc/soundness.md`。
6. **Windows 上交的是檔案的 handle（W1）**（2026-11-27，使用者決定）：只留讀的權限，renderer 這一側的 `ipc` 自己建 mapping。理由：三個平台交的都是開著的檔案，意思一樣；和 Chromium 一樣（`FontDataService` 和舊的 DirectWrite proxy 交的都是檔案的 handle）。代價：renderer 能用 handle 查到檔案的路徑、`ReadFile` 它；照路徑它在 AppContainer 裡開不了。不選 Browser 先建好唯讀的 section、只交 map 的權限（W2：權限最少，但 Windows 和另外兩個平台不同，長度要另外跟著訊息送）。
7. **一則訊息最多 16 個 handle 不改，字型服務分批交檔案（H1）**（2026-11-27，使用者決定）：回答一個 family 時給每個字型的資料和檔案的編號，renderer 對還沒拿到的檔案，每次要最多 16 個。理由：上限定多大都擋不住一個 family 的檔案比它多（Linux 上整套不是可變字型的 Noto Sans 有幾十個檔案），協定本來就要能分批；照編號要的話，同一個檔案被好幾個 family 用到（`Segoe UI Variable` 的三個光學尺寸）也只交一次，Chromium 的檔案也帶一個編號，為的是同一件事。核心收 handle 的規則和 Windows 的表頭都不用改。不選把上限提高到 64 或 256（H2，Mojo 是 256：多數的 family 一次交完，但核心最多留住的 handle 跟著變多，大的 family 還是要分批）。
8. **三種 renderer 都走字型服務（N1）**（2026-11-27，第 2 段開始時，使用者決定）：sandbox 裡的 renderer process、`--no-sandbox` 的 renderer process、`--single-process` 的 renderer thread 都向 Browser 問；只有測試用的內建字型（Ahem、Lato）不問。理由：只有一條路，`--single-process` 和 `--no-sandbox` 的測試跑的就是真的那條；同一個 process 裡的不經過 codec，開著的檔案直接搬過去。不選只有 sandbox 裡的走、其他的照舊自己問系統（N2：改得少，但兩條路一直並存，除錯時的模式和真的不同）。
9. **Browser 那一側在自己的一條 thread 上回答（T2）**（2026-11-27，使用者決定）：系統字型的 `Collection` 只屬於這條字型 thread；renderer 的問題在 IO thread 上檢查過之後轉給它，它直接回答 renderer。理由：第一次問一個 family 時 fontique 要打開它的每個檔案讀屬性（原型量到一個 renderer 合計 3 到 4 ms），在 IO thread 上做會讓所有 renderer 的訊息跟著等，而 IO thread 上的程式不等任何東西；`Collection` 有一個擁有者，不用鎖。代價：每個問題多轉一手。Chromium 一樣：`FontDataService` 跑在一個允許等待的背景 sequence 上。不選在 IO thread 上直接回答（T1：cookie 是這樣答的，最快，但開檔的那幾 ms 卡住所有通道）。不選 UI thread（T3：Browser 忙時 renderer 的排版跟著等）。
10. **generic family 用到哪一種才要哪一種（G2）**（2026-11-27，使用者決定）：啟動時的名字清單裡已經有每種 generic 是哪幾個 family，用到時照那幾個 family 要，和一般的 family 同一條路。理由：不用另外的機制；沒用到的（`cursive`、`fantasy`……）不交檔案。不選第一次排版時全部要來（G1，原型的做法：少幾次來回，每個 renderer 多交十幾個檔案）。
11. **名字的清單整個 Browser 只列一次**（2026-11-27，第 2 段做的時候，使用者決定）：字型 thread 在第一個 renderer 啟動時列出系統的字型，之後的每個 renderer 都拿這一份，family 和檔案的編號也是這一份的。所以 Browser 執行期間新裝的字型，新的分頁也看不到，要重開 machopo：比第 2 條寫的代價（已經開著的分頁看不到，新的分頁看得到）多一點。理由：照第 2 條的話每啟動一個 renderer 就要重列一次（DirectWrite 重新拿系統的字型集合），清單變了時建一份新的、舊的 renderer 繼續問舊的那份，字型 thread 要同時留著幾份、編號要分得出是哪一份的；這和「字型變了時通知 renderer 重拿清單」（之後的可能）是同一件事，到那時一起做。不選每啟動一個 renderer 就重列。

12. **macOS 上交的是 fileport（M1）**（2026-11-27，第 4 段開始時，使用者決定）：Browser 用 `fileport_makeport` 把開著的檔案的 file descriptor 包成一個 port 的 send right，跟著訊息搬過去；renderer 用 `fileport_makefd` 還原成自己的 file descriptor，之後和另外兩個平台一樣由 `ipc` map 成唯讀的位元組。理由：第 3 條寫的就是這樣（三個平台交的都是開著的檔案），`ipc` 平台層的「交出檔案、還原檔案」的形狀不用改；Chromium 的 Mojo 在 macOS 上交 file descriptor 也是這樣（`ChannelMac`）。探測（2026-11-27，這台 Mac）：同一組文字，Browser 的字型 thread 回答的和 renderer 自己用 CoreText 找的排出來逐一相同；renderer 的 `Collection` 不帶系統字型，fontique 不建 CoreText 的後端。代價：兩個函式是私有的 API（不在 SDK 的 header 裡，`mach2` 沒有，在 `ipc` 裡自己宣告；Chromium 和 XPC 都在用）；renderer 拿到的 file descriptor 查得到檔案的路徑（`F_GETPATH`）、讀得到那個檔案，和 Windows 的第 6 條一樣，照路徑它在 sandbox 裡開不了。不選檔案的 memory entry（M2：Browser 先唯讀地 map、用 `mach_make_memory_entry_64` 建只能讀的 entry 交過去：不用私有的 API、renderer 拿不到路徑，和 `ipc` 的共用記憶體同一種做法；但 macOS 和另外兩個平台不同，長度要另外跟著訊息送，平台層的介面要改，和 Windows 不選 W2 的理由一樣）。

第 2 段開始時查到、不用決定的兩件：worker 裡沒有文字（machopo 沒有 `OffscreenCanvas`，worker 不排版），之後有了走同一條路；machopo 不把 `lang` 交給 parley（沒有 `StyleProperty::Locale`），所以 fallback 一律照 script、不帶語言，和現在一樣，這一段照這樣做。

## 待決定

探測之前要定的四條都定了（已決定的事項第 1 到 4 條），第 1 段的三條也定了（第 5 到 7 條）。每一段開始前只定那一段的。

第 2 段的五件定了（第 8 到 10 條，和它們後面的兩件），做的時候多出的一件也定了（第 11 條）。第 4 段的一件也定了（第 12 條）。第 3 段沒有要決定的事：`.ttc` 對得上（`details.md`），fd 照第 3、5 條跟著訊息過去（`SCM_RIGHTS`，region 本來就這樣交；Chromium 的 `ChannelPosix` 也是），K11 在 Linux 上和 macOS 一樣是假設（第 5 條已經寫了）。

做的時候照設計細化、不用決定的：Browser 檢查檔案的編號只看「字型 thread 給過這個編號」，不看「給過這個 renderer」：每個檔案都能經過問它的 family 拿到，照 renderer 分開記換不到什麼，還要每個 renderer 一張表。

之後的段落：

- Windows 的 restricted token 放在這份的最後一段，還是另一份。

## 之後的可能

- 字型裝了、移除了時 Browser 通知 renderer 重拿名字的清單，開著的分頁也看得到新的字型。
- 字型的清單放進共用記憶體（A2），如果量到同步的來回是問題。
- renderer 啟動時先把常用的 family 問好（Chromium 有一個預設關著的 prewarm）。
- `lang` 影響選字型（`StyleProperty::Locale`）：掃文字時連語言一起，fallback 的 key 帶語言。
- worker 裡的文字（`OffscreenCanvas`）：走同一條路，worker 的 thread 也要能同步地問。
- `src: local()` 照名字找系統字型：現在有沒有、走哪條路還沒查；Chromium 是另一個呼叫（`MatchLocalFont`）。
