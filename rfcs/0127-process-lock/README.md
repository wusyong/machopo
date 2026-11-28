# RFC 0127：process lock 和 Browser 的檢查

- 開始日期：2026-11-28
- 狀態：✅ 已完成：2026-11-28（Linux：Fedora 45）
- 進度和驗收：`progress.md`
- 細節：`details.md`

RFC 0122 路線圖的 B（M2 的第二份）。

## 摘要

跨網站的導覽已經換 renderer（RFC 0124）：一個 renderer 的一生裡只裝過一個最上層的網站，Browser 也記著是哪一個（`RenderProcessHost` 的 `SiteInfo`）。但 Browser 還沒有拿它檢查過任何東西。這份把它變成 process lock：一份文件是誰的只算一次（`SiteInfo::of_document`），換不換 renderer 和檢查都用它；Browser 每次交給 renderer 拿資料的權限時（文件的 loader、storage 的 area、commit），都確認那份資料的最上層網站就是這個 renderer 鎖的網站；對不上就是壞訊息，結束那個 renderer。另外，renderer 自己說了算的幾個地方改成照 Browser 給的東西檢查，被攻破的 renderer 就不能用它們拿到別的網站的 origin：blob URL 的 origin、從頁面開的新分頁的發起者。最後用一個測試用的「被攻破的 renderer」逐一確認。

## 動機

RFC 0124 讓 process 裡只有一個最上層網站的東西，但 Browser 的檢查還是照文件的（loader 的 origin、frame 樹上的 origin），沒有照 process 檢查。盤點時找到的（`details.md` 的「盤點」）：

- **blob URL 的 origin 沒有人對**（RFC 0122 的已決定的事項第 4 條已經點名）：`RegisterBlobUrl` 只帶網址字串，Browser 照收。如果有一份登記了同一個網址，新的會蓋掉它，不管那一份是哪個 renderer 登記的。導覽到 blob URL 時，任何 renderer 登記的都拿得到，新文件的 origin 就是網址裡寫的那一個。所以被攻破的 renderer 可以這樣做：登記 `blob:https://b.example/<任意>`，內容自己寫，再把 main frame 導覽過去。Browser 照網址的網站換到一個 b.example 的新 renderer，它寫的 HTML 在 b.example 的 origin 底下跑，帶著 b.example 的 cookie 和 storage。這是現在最大的洞，0124 的換 process 擋不住，反而把它送進了乾淨的 process。
- **Browser 自己算的東西沒有安全網**：loader 的 `IsolationInfo`、storage key、commit 的 origin 都是 Browser 從 frame 樹算的，現在沒有地方確認它們和 renderer 的網站一致。放錯 process 的 bug（例如 0124 的規則漏了一種導覽）不會被發現，只會悄悄把一個網站的資料交給另一個網站的 process。
- **0124 的規則有兩種導覽會把別的網站放進來**（`site_info::swaps`）。鎖一加上，它們就變成壞訊息，所以要先改規則：
  - 重新整理不換 renderer，不管最後的網址。重新整理時伺服器 redirect 到別的網站，新文件就在舊網站的 renderer 裡 commit。
  - `about:blank` 沒有網站，一律留在原來的 renderer，但它繼承發起者的 origin。b.example 的頁面裡有一個 c.example 的 iframe，iframe 把最上層導覽到 `about:blank` 時，main frame 的新文件的 origin 是 c.example，卻在 b.example 的 renderer 裡。
- **renderer 說了算的地方**：
  - 請求可以不帶發起者。現在用「有沒有發起者」決定要不要走 CORS，所以不帶就沒有 CORS，也沒有 `Origin` header。這一條在鎖底下拿不到多的資料，留給 ORB 那一份（已決定的事項第 4 條）。
  - `ChangeStorage` 帶的網址照轉給其他 renderer 的 `storage` 事件。收到的都是同一個 storage key 的文件，Chromium 也不檢查，所以照舊（已決定的事項第 8 條）。
  - 頁面從連結開的新分頁沒有發起者，算成 Browser 自己的導覽。這一條不用攻破 renderer：`target=_blank` 的連結開到別的網站時，`SameSite=Strict` 的 cookie 照送（`net` 的 `same_site_initiator`：沒有發起者就是 strict），Chromium 不送。

**目標**

- `RenderProcessHost` 有一個一輩子不變的鎖。Browser 給 renderer 的每一個 loader、每一個 storage area、每一次 commit，最上層的網站都要合鎖，不合就是壞訊息。
- 不會再有「renderer 自己說是哪個 origin」的訊息：blob URL、新分頁的發起者，都照 Browser 給的 loader 或 frame 檢查。
- 一個測試用的被攻破的 renderer，送每一種偽造的訊息，確認每一種都結束它（或被拒絕），並且確認正常的瀏覽一次都沒有踩到鎖。

**不在範圍內**

- **ORB、`Cross-Origin-Resource-Policy`**（路線圖的 N）：被攻破的 renderer 照樣可以用 `no-cors` 把別的網站沒有 cookie 的回應要進來。請求一定要帶發起者、CORS 照 `mode` 決定，也在 N 做（已決定的事項第 4 條）：ORB 要知道 `no-cors` 請求的發起者，可以不帶的話，被攻破的 renderer 拿掉它就躲過 ORB。
- **`Sec-Fetch-*`**（Fetch Metadata）：machopo 現在一個都不送。它是網頁看得到的新功能，有自己的 WPT（`fetch/metadata/`），另開一份，排在 ORB 那一份之後（已決定的事項第 5 條）。
- **嵌在頁面裡的跨網站 iframe**：照 L1（RFC 0122 的已決定的事項第 4 條），它和頁面在同一個 process，鎖是最上層的網站，所以 iframe 的東西照樣在裡面。被攻破的 renderer 可以拿 iframe 的身分發起導覽，這要等 OOPIF。
- `window.open`、`BrowsingInstance`、`SiteInstance`（路線圖的 W）。

要解決的 `docs/limitations.md` 的限制：沒有。這份補的洞都沒有寫進 limitations：blob URL 的 origin 只記在 RFC 0122 的盤點裡，新分頁的 `SameSite` 是這次盤點才找到的。新寫進去的：L1 留下的缺口（上面那條 iframe），在「iframe」的「跨網站的 iframe 和頁面在同一個 renderer」。

## 設計概要

給使用網頁的人：沒有看得到的變化，只有一個例外。從連結開到別的網站的新分頁不再帶 `SameSite=Strict` 的 cookie，和 Chrome 一樣。另外，重新整理之後落在別的網站時（伺服器 redirect），新頁面在新的 renderer 裡。

給改 machopo 的人：

- `RenderProcessHost` 的 `site_info` 變成鎖：第一次寫入之後就不再改。它還沒有值時（新分頁的第一個 renderer，還沒 commit 過網頁），renderer 只能拿到 opaque 的東西。
- 新加一個「renderer 向 Browser 要資料」的訊息時，要說它照什麼檢查（`docs/architecture.md` 的「Process 的邊界」 寫成規則）。做法是讓它帶著 Browser 給過的東西（loader、area、frame），由 Browser 從那裡取 origin；不要讓 renderer 自己報 origin 或網址。
- 鎖的比較是 `site_info` 的兩個函式：`admits`（renderer 收不收一份文件，換不換和 commit 的檢查共用）、`allows`（交出的 loader 和 storage area 的最上層網站），單元測試照表對。

## 詳細設計

### 鎖

- **是什麼**：就是 RFC 0124 的 `SiteInfo`，記在 `RenderProcessHost` 上（Chromium 的 `ProcessLock::FromSiteInfo`）。它可以是一個網站（`Site`）、`File`、一個內建頁（`BuiltIn`），或還沒有（`None`）。opaque 的網站（使用者輸入的 `data:`、`blob:null/…`）也是一種 `Site`，每次都不同。
- **什麼時候定**：照 0124，`WebContents::commit` 送出 main frame 的第一份有網站的文件之前，也就是 renderer 載入它之前。`set_site_info` 改成只接受「原本沒有」或「同一個」，不同的就是 Browser 的 bug：不送 commit，結束 renderer（「失敗的時候」）。

### 一份文件是誰的：只算一次（已決定的事項第 1 條）

`SiteInfo::of_url` 換成 `SiteInfo::of_document`，從網址和 commit 時的 origin 算出「這份文件的內容是誰的」。Chromium 的 `SiteInfo` 也是這樣，網址算不出時用 origin 補（`GetPossiblyOverriddenOriginFromUrl`）：

| 文件 | `SiteInfo` |
| --- | --- |
| 網頁 | origin 的網站 |
| `about:blank`，繼承了 origin | 發起者的網站（Chromium 也讓它跟著發起者的 `SiteInstance`） |
| `about:blank`，opaque | 沒有：留在原來的 renderer，鎖不變。它是空的，裡面沒有任何人的內容 |
| `blob:` | 網址裡的 origin 的網站 |
| `data:` | 自己的 opaque 網站，每次 commit 都是新的 |
| 錯誤頁 | 要去的網址的網站。內容是 Browser 的，origin 是 opaque 的，拿不到資料。Chromium 把錯誤頁放在自己的 process（鎖是 `kUnreachableWebDataURL`），這裡不跟：沒有安全上的好處，還多一次啟動 |
| `file:` | `File` |
| 內建頁 | 照名字（`BuiltIn`） |

用到它的地方都呼叫同一個函式，所以換的決定和檢查不會對不上：

- **換不換**：0124 的 `swaps` 換成「renderer 的鎖收不收這份文件」（`site_info::admits`）：鎖還沒有、文件沒有網站、或兩者相同時不換，其他都換。commit 時的檢查用的是同一個比較，所以 Browser 不會把文件放進它自己會拒絕的 renderer。
- **重新整理、上一頁、下一頁**：歷史紀錄的每一筆記著它 commit 時的 `SiteInfo`（main frame 的 `FrameNavigationEntry::site_info`：`WebContents::commit` 把算好的交給 controller 的 `committing_site`，renderer 回報 commit 時記進那一筆；同一份文件的 `pushState` 的那一筆跟著；`kept_site` 取回）。Chromium 的 `NavigationEntry` 記著它的 `SiteInstance`，也是這個道理。走回同一筆、最後的網址又沒變時沿用它，不重算，所以 `data:` 的重新整理還是同一個網站，不用寫「opaque 的除外」。重新整理之後被 redirect 到別的網址時，最後的文件是新的，照新的算，該換就換。
- **之後的特例**（`window.open`、iframe 的 `sandbox`、COOP）：都只改 `of_document` 這一個地方。

### 檢查什麼、在哪裡（已決定的事項第 2 條）

在 Browser **交出權限的那一刻**檢查，不是每一則訊息都查。理由是 renderer 之後拿資料都要靠這些權限：loader 擋著請求和 cookie，area 擋著 storage。權限交出時合鎖，用權限拿到的東西就都合鎖。

| 交出的 | 在哪裡 | 比的是 |
| --- | --- | --- |
| loader | `RenderProcessHost::register_loader`（commit 和 worker 都經過它） | loader 的 `TrustedParams` 的最上層網站 |
| storage area | `WebContents::open_storage_area` | `StorageKey` 的最上層網站 |
| commit | `WebContents::commit` | main frame 是鎖收不收文件的 `SiteInfo::of_document`，和換不換用同一個比較；子 frame 是它的最上層網站，也就是 main frame 的 |

比較的規則（`allows`，最上層網站是 `top`）：

- `top` 是 opaque 的：任何鎖都允許（包括還沒有鎖的）。opaque 的最上層網站底下沒有 cookie（site for cookies 是空的）；它底下的 iframe 的 storage 分區每次都是新的 opaque 網站，只有這個 process 裡的文件碰得到。所以給哪個 process 都拿不到別人的東西。錯誤頁就靠這一條：它的 origin 是 opaque 的，放在它要去的網址的 renderer 裡（第 1 條）。
- `file:` 的文件：它的 origin 是 opaque 的（`url` crate 的，machopo 的 `file:` 文件都是），最上層網站也就是 opaque 的，照上一條給任何 renderer；它底下沒有 cookie、沒有 storage，讀本機檔案另外照 commit 給的權限（`RenderProcessHost::set_file_access`）。草稿原本寫「鎖要是 `File`」，實作時照這個改了：最上層網站分不出 `file:` 和其他 opaque 的。
- 其他：鎖要是同一個 `Site`。還沒有鎖的 renderer 一律不允許，這就是 Chromium 的 citadel 檢查在 machopo 的樣子：每個 renderer 遲早都會鎖，鎖之前只裝得了 opaque 的文件。

### renderer 說了算的地方

每一條都改成照 Browser 給過的東西：

- **blob URL**（已決定的事項第 3 條）：`RegisterBlobUrl` 多帶它的 loader（worker 的是 worker 的 loader）。網址裡的 origin 要等於 loader 的 origin；opaque 的 loader 只能登記 `blob:null/…`。網址已經被別的 renderer 登記過的，是壞訊息；同一個 renderer 重複登記照舊蓋過去。`RevokeBlobUrl` 照舊只撤自己的。登記的 origin 已經照 loader 檢查過、loader 又合鎖，導覽到它時照網址的網站換 renderer 就是對的。
- **`ChangeStorage` 的網址**：不檢查（已決定的事項第 8 條）。它只到同一個 storage key 的文件，被攻破的 renderer 偽造它只騙得到和自己同源、同鎖的頁面。
- **從頁面開的新分頁**：`OpenUrl` 改成帶發起者，就是那個 frame 的 origin，由 Browser 從 frame 樹取，renderer 不報。新分頁的導覽照「頁面發起的」處理：`SameSite`、`file:` 的權限，網址列照 RFC 0125 是新分頁的特例。這也是 Chromium 的 `OpenURLParams::initiator_origin`。

### 失敗的時候（已決定的事項第 6 條）

- renderer 偽造的訊息（別的 origin 的 blob URL、蓋掉別人的網址、別的 frame 開的新分頁）照其他壞訊息處理，各有自己的理由。
- Browser 自己算的檢查（commit、`register_loader`、`open_storage_area` 時最上層的網站不合鎖）也當成壞訊息：不交出那個權限，結束那個 renderer，分頁顯示 sad tab，理由是 `BadMessageReason::ProcessLockMismatch`。log 寫出鎖和要的網站（Chromium 的 crash key `expected_process_lock`、`requested_origin`）。第 1 條之後這只會是 Browser 的 bug，或 renderer 的訊息讓 Browser 算錯；Chromium 對 `CanCommitOriginAndUrl` 的失敗也是結束 renderer，不管是誰的錯。Browser 不 panic（`browser` 的 clippy 規則）。

### 測試

測試的清單、mutation test 的結果在 `progress.md`。

- `site_info` 照表的單元測試：每一種文件的 `of_document`，每一種鎖的 `admits` 和 `allows`；`render_process_host` 的：鎖在第一個網站之後不變、不合鎖的 loader 不登記，blob URL 的每一種登記。
- 整合測試：重新整理之後落在別的網站換 renderer（測試伺服器的路由加了 `Route::then`：第一次回這個，之後回下一個）；繼承了跨網站 iframe 的 origin 的 `about:blank` 換到那個網站的 renderer；`data:` 的重新整理、上一頁回到它沿用 entry 的網站；新分頁的 `SameSite` 和 `about:blank` 的 origin。
- 被攻破的 renderer：`crates/browser/tests/it/renderer_process.rs` 的 `rogue_on_commit` 讀 Browser 送的 commit、拿它的 loader，登記別的網站的 blob URL、別人的網址，兩種都被結束；不在樹上的 frame 開的新分頁在壞訊息的清單裡。
- 鎖的安全網（`WebContents` 裡鎖拒絕時不交出、結束 renderer 的那幾支）從外面走不到：換不換和檢查是同一個比較，frame 樹在換 renderer 時清成一個 opaque 的 main frame，renderer 送什麼都做不出不合鎖的東西。草稿想用 `RendererLauncher::Detached` 造出錯的狀態，做不到；比較本身有單元測試，那幾支記在 `docs/test-gaps.md` 的「只靠審查」。
- 正常的瀏覽不踩鎖：`cargo test --workspace`、情境（`--window` 也是）、WPT 照常通過。
- WPT 的 `cookies/samesite/` 幾乎都要 `window.open`（`tests/wpt/include.txt` 因此不收），沒有跑；新分頁的 `SameSite` 用整合測試守。

### 分階段

兩段（已決定的事項第 7 條），都做完了：

1. **renderer 說了算的地方**：blob URL 照 loader 檢查（第 3 條）、`OpenUrl` 帶發起者（新分頁的 `SameSite`）；被攻破的 renderer 的角色；`docs/architecture.md` 的「Process 的邊界」 寫明「要資料的訊息一定帶 Browser 給過的權限」。
2. **鎖**：`of_document` 和歷史紀錄的 `SiteInfo`、「鎖收不收」取代 `swaps`（重新整理、`about:blank`）、`allows`、交出權限時的三個檢查、`set_site_info` 只寫一次、`ProcessLockMismatch` 和它的測試；收尾。

## 缺點

- **一次失敗就結束分頁**：如果 0124 的規則還有漏掉的導覽，現在是悄悄放錯 process，之後會變成 sad tab。這正是要的（錯被看見），但使用者會先看到。
- **L1 的保證要自己論證**：Chromium 沒有「鎖最上層的網站、iframe 還在同一個 process」的形態。opaque 的最上層網站允許給任何 process 這一條，是靠「opaque 的最上層網站底下沒有 cookie，storage 的分區只屬於一個 process」成立的。之後如果 opaque 的文件可以拿到某種資料，這一條要重新看。

## 理由和其他做法

- **不做的話**：blob URL 的洞照樣在，0124 換 process 換得再乾淨，被攻破的 renderer 還是拿得到任何網站的 origin。
- **每則訊息都照 process 檢查**（Chromium 的 `CanAccessDataForOrigin` 的做法）：IO thread 要看得到鎖，每個請求多一次比較。machopo 的 loader 本來就把請求綁在權限上，在交出權限時檢查一次就夠了（已決定的事項第 2 條）。
- 各題的選項和不選的理由在「已決定的事項」。

## 先例

- **Chromium**（`details.md` 的「Chromium」）：`ChildProcessSecurityPolicyImpl` 記每個 process 的 `ProcessLock`，在 process 載入內容之前定，之後不變（`SetProcessLock` 的 `CHECK`）。它有兩層：
  - 交出權限時綁好：`URLLoaderFactoryParams` 的 `request_initiator_origin_lock`、`BlobURLStoreImpl` 的 `renderer_origin_`、`RestrictedCookieManager` 綁在 origin 上。
  - 使用時再查：`CanAccessDataForOrigin` 檢查 jail 和 citadel，`CanCommitOriginAndUrl` 檢查 commit。

  失敗時結束 renderer。`--single-process` 不鎖。
- **machopo 不同的地方**：鎖的是最上層的網站（L1）；`--single-process` 一樣鎖（0124 讓 thread 的 renderer 也換）；只在交出權限時檢查。

## 已決定的事項

1. **一份文件是誰的只算一次，換不換和檢查都用它（R4）**（2026-11-28，使用者決定）：`SiteInfo::of_document`（網址加 commit 時的 origin）算出文件的 `SiteInfo`。換不換是「renderer 的鎖收不收它」，和 commit、交出權限時的檢查用同一個比較。歷史紀錄的每一筆記著 commit 時的 `SiteInfo`，重新整理和上一頁、下一頁沿用它。繼承了 origin 的 `about:blank` 照發起者的網站。錯誤頁照它要去的網站，不放進自己的 process（「一份文件是誰的：只算一次」）。理由：鎖一加上，0124 的規則有兩處會把別的網站放進 renderer（重新整理之後的 redirect、繼承了跨網站 iframe 的 origin 的 `about:blank`），根源是換不換照網址算、檢查照 origin 算，兩個函式各自算，之後每多一種特例都可能再對不上；算一次、兩邊共用，Browser 就不會把文件放進它自己會拒絕的 renderer。這是 Chromium 的結構：每次導覽算一次 `SiteInfo`，process lock 從它來（`ProcessLock::FromSiteInfo`），`NavigationEntry` 記著 `SiteInstance`。不選只在 `swaps` 補那兩處（R1：改得最少，但兩個函式還是各算各的）。不選 Chromium 的 opaque 文件的策略（R3：錯誤頁自己的 process、`data:` 的 nonce；`data:` 本來就每次一個新的網站，錯誤頁拿不到資料，放在哪裡和兩個函式對不上無關，還多一次啟動）。代價：歷史紀錄的每一筆多一個欄位，0124 的 `swaps` 改寫。
2. **在交出權限時檢查（C1）**（2026-11-28，使用者決定）：Browser 在 `register_loader`、`open_storage_area` 和 commit 時確認最上層的網站合鎖；之後的請求、cookie、storage 照舊只照 loader 和 area 檢查，IO thread 不用知道鎖。理由：renderer 要資料只有 loader 和 area 兩條路，權限交出時合鎖，用它拿到的就都合鎖；每則訊息不多花，也不和 loader 的檢查重複。代價：正確性靠「每一種資料都經過一個權限」，新加的訊息不綁權限就漏了：在 `docs/architecture.md` 的「Process 的邊界」 寫明「renderer 要資料的訊息一定帶 Browser 給過的權限（loader、area、frame），origin 從權限取」，被攻破的 renderer 的測試對每一種訊息送一次偽造的。不選每則訊息都照鎖檢查（C2，Chromium 的 `CanAccessDataForOrigin`：多一層，但 IO thread 要共用鎖的狀態、每則訊息多一次比較；Chromium 需要它，是因為它有很多不經過 factory 的介面，沒有鎖的 process 也要擋）。之後有了不經過 loader 的資料，或 renderer 能裝好幾個分頁時再看（「之後的可能」）。
3. **blob URL 照 loader 檢查，網址照舊由 renderer 產生（B1）**（2026-11-28，使用者決定）：`RegisterBlobUrl` 多帶 loader（worker 的是 worker 的），網址裡的 origin 要等於 loader 的 origin，opaque 的 loader 只能登記 `blob:null/…`；網址已經被別的 renderer 登記過的是壞訊息。理由：一個 renderer 只能登記它自己文件的 origin 的網址、搶不到別人的，和 Browser 產生網址擋住的一樣；loader 已經合鎖（第 2 條），登記的 origin 就合鎖；`URL.createObjectURL` 照舊不用等 Browser。網址裡的 UUID 還是 renderer 選的，但它不是安全上的屏障：選了也蓋不掉別人的。和 Chromium 的 `BlobUrlIsValid` 的檢查一樣（網址的 origin 等於 store 的）。不選 Browser 產生網址（B2，Chromium 的 `BlobURLStoreImpl::Register` 用 `renderer_origin_` 產生：`createObjectURL` 是同步的 API，每次多一次同步的往返，renderer 那一側也要改，多擋的只有 UUID）。
4. **請求一定帶發起者，留給 ORB 那一份（I2）**（2026-11-28，使用者決定）：這份不改請求的發起者（renderer 對 `no-cors` 的子資源照舊不帶，`net` 照「有沒有發起者」決定 CORS）；N 開始時一定要做：renderer 的每個請求都帶發起者、不帶是壞訊息、CORS 照 `mode`（Chromium 的 `CorsURLLoaderFactory`：給 renderer 的 factory 的請求沒有 `request_initiator` 是 `NOTREACHED`），記在 RFC 0122 的 `roadmap.md` 的 N。理由：在鎖底下它拿不到多的資料：跨網站的請求本來就沒有 cookie，而沒有 ORB 時被攻破的 renderer 直接用 `no-cors` 就拿得到同樣的回應；同網站的請求這個 renderer 本來就合法拿得到，少了 `Origin` header 伺服器也不會多給。真正要它的是 ORB，CORS 怎麼改和 ORB 對回應的處理一起設計比較好。不選這份就做（I1：要動 renderer 的每一種請求和 `net` 的 CORS，是這份最大的改動，在這份卻沒有安全上的收穫）。
5. **`Sec-Fetch-*` 另開一份，排在 ORB 之後（S1）**（2026-11-28，使用者決定）：記在 RFC 0122 的 `roadmap.md`，代號 FM，依賴 N，不在任何里程碑裡；B 那一列的範圍拿掉 `Sec-Fetch-Site`。理由：machopo 現在一個都不送，這是網頁看得到的新功能，WPT 有整個 `fetch/metadata/`；`Sec-Fetch-Site` 要知道每個請求的發起者，那是 N 做的（第 4 條）；它是伺服器自願用的 CSRF 訊號，不是 Browser 這一側的隔離，不影響 M2 的保證。renderer 送的 `sec-` header 已經擋了（`protocol` 的禁止 header 清單），和 Chromium 一樣（`"Forbidden Sec- header from renderer"`），缺的只有 Browser 自己送。不選這份做（S2：發起者還沒一定帶，`no-cors` 的子資源算不出，等於推翻第 4 條）。不選併進 N（S3：N 本來就不小）。
6. **鎖的檢查失敗是壞訊息，結束 renderer（F1）**（2026-11-28，使用者決定）：commit、`register_loader`、`open_storage_area` 時最上層的網站不合鎖，不交出那個權限，結束 renderer、分頁顯示 sad tab，理由是 `BadMessageReason::ProcessLockMismatch`，log 寫出鎖和要的網站。理由：錯一定被看見：整合測試和情境裡的 sad tab 會讓測試失敗，使用者看到 sad tab 而不是放錯 process 的資料；和其他壞訊息走同一條路。Chromium 對 `CanCommitOriginAndUrl` 的失敗也結束 renderer，另外 `DumpWithoutCrashing` 回報（machopo 沒有 crash 回報，用 log）。代價：Browser 的 bug 也讓使用者看到 sad tab。不選只拒絕那一件、renderer 留著（F2：錯被藏進 log，測試大多照樣通過；commit 被拒時 renderer 的狀態不一定一致）。
7. **分兩段：先補 renderer 說了算的地方，再做鎖（G1）**（2026-11-28，使用者決定）：照「分階段」。理由：最嚴重的洞（blob URL 拿到任何網站的 origin）和新分頁的 `SameSite` 只靠 loader 的檢查就補得起來，不用等鎖，第一段做完就能 merge 回 `main`；鎖那一段改的是 RFC 0124 的換不換，風險集中在第二段、單獨驗收。代價：階段的檢查（`cargo test --workspace`、fmt、clippy、`--window` 的情境）跑兩次。不選鎖先做（G2：草稿原本的順序，最嚴重的洞等到第二段才補，第二段的檢查其實不需要鎖）。不選一段做完（G3：補洞和改換不換的規則混在同一次審查裡，洞補得晚）。
8. **`ChangeStorage` 的網址不檢查（第 1 段實作時改的）**（2026-11-28，使用者決定）：原本照草稿要檢查它是 area 的 origin 的網址、不是就是壞訊息。實作時發現：Chromium 的 Browser 不檢查（`StorageAreaImpl` 的 `Put`、`Delete`、`DeleteAll` 把 `source` 原樣轉給 observer）；收到 `storage` 事件的都是同一個 storage key 的文件，也就是同一個 origin、同一個鎖底下，偽造它只騙得到和自己同源的頁面，拿不到資料；要檢查得讓 storage thread 能回報壞訊息（現在只有 IO thread 能），多接一條跨 thread 的路。不選檢查、不合是壞訊息（比 Chromium 嚴，多一條路）。不選不合時丟掉網址（把錯藏起來，和第 6 條的方向相反）。

## 待決定

沒有（已決定的事項第 1 到 8 條）。

## 之後的可能

- `Sec-Fetch-*`（已決定的事項第 5 條，路線圖的 FM）。
- OOPIF 之後鎖改成照 frame 的網站，比較的規則跟著換，交出權限的檢查點不變。
- 每則訊息都照 process 檢查的第二層（C2），在 renderer 能裝好幾個分頁、或有了不經過 loader 的資料時。
