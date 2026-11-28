# RFC 0130：iframe 的 `sandbox`

- 開始日期：2026-11-28
- 狀態：✅ 已完成：2026-11-28
- 進度和驗收：`progress.md`
- 細節：`details.md`

RFC 0122 路線圖的 F2（M3 的第一份；0122 已完成，路線圖的那一列是背景，見它的已決定的事項第 7 條）。

## 摘要

machopo 原本不看 iframe 的 `sandbox` 屬性：sandbox 的 iframe 照樣跑 script、照樣是它網址的 origin、照樣能導覽最上層和開新分頁。這份照 HTML 和 Chromium 做 sandbox 的旗標：renderer 解析屬性、告訴 Browser，Browser 的 frame 樹記著，下一次導覽 commit 時算出那份文件的旗標和 origin（沒有 `allow-same-origin` 的是 opaque 的），renderer 照文件的旗標擋下 machopo 有的那些功能：script、導覽別的 frame 和最上層、開新視窗，被擋時照 Chrome 的原文寫 console；Browser 照 Chromium 再檢查開新視窗、改旗標和 sandbox 的 frame 的 history。

## 動機

- **網站靠它**：嵌入第三方的內容（廣告、使用者的 HTML、預覽）時，網站用 `sandbox` 不讓那份內容跑 script、碰自己的 origin、把最上層導走。machopo 照樣讓它做，網站以為的保護不存在。
- **和 Chrome 不同**：`<iframe sandbox>` 裡的頁面在 Chrome 不能跑 script，在 machopo 能。
- **WPT**：`html/browsers/sandboxing/` 和 `the-iframe-element/` 的 sandbox 測試（92 個檔案，Chrome 過 130 個 subtest 裡的 124 個），RFC 0129 的 `frame-ancestors` 也有兩個因為它而是預期的失敗。

**目標**

- `sandbox` 屬性照 HTML 和 Chromium 的 `ParseWebSandboxPolicy` 解析，`iframe.sandbox`（`DOMTokenList`，`supports()`）。
- 旗標照 Chromium 的路走：frame 的 policy 記在 Browser 的 frame 樹（父 frame 的旗標一定帶下去），導覽 commit 時算出文件的旗標和 origin。
- machopo 有的功能照文件的旗標擋（範圍待決定第 1 題）。
- WPT 能跑的跑，逐項和 Chrome 比。

**不在範圍內**

- **CSP 的 `sandbox` directive**（路線圖原本就排除；RFC 0129 的 CSP 解析器已經有了，之後接上便宜，見「之後的可能」）。
- **把 sandbox 的 frame 放進自己的 process**（Chromium 桌面版的 `IsolateSandboxedIframes`）：要 OOPIF（M4）。
- `window.open`（路線圖的 W）：彈出視窗的旗標（`allow-popups`、`allow-popups-to-escape-sandbox`）在連結開新分頁上能做的做，`window.open` 那一半等 W。
- machopo 還沒有的功能的旗標：表單送出、`alert` 這類 modal、下載、pointer lock、螢幕方向、presentation、storage access、`document.domain`（路線圖的 G）：只解析、記著，那些功能做的時候照它擋。

要解決的 `docs/limitations.md` 的限制：「iframe」的「`loading`、`sandbox`、`allow` 都還沒有（sandbox 的 iframe 照樣跑 script……）」裡 `sandbox` 的部分；「隔離」的「CSP 只有 `frame-ancestors`」裡「iframe 沒有 `sandbox`」那一句。

## 設計概要

給使用網頁的人：

- `<iframe sandbox src=…>` 裡的頁面不跑 script，origin 是 opaque 的（讀不到 cookie、`localStorage`，嵌它的頁面也碰不到它）；`allow-scripts`、`allow-same-origin` 這些 token 照 HTML 放寬。
- sandbox 的 frame 只能導覽自己和自己底下的 frame，不能把最上層導走（除非 `allow-top-navigation`），不能開新視窗（除非 `allow-popups`；開出來的新分頁帶著同樣的沙箱，除非 `allow-popups-to-escape-sandbox`）；被擋時 console 寫原因（和 Chrome 同樣的原文）。使用者自己按 Ctrl 或中鍵開的新分頁不擋，也不帶沙箱。
- 屬性改了，下一次這個 frame 導覽時才生效（照 HTML）。

給改 machopo 的人：

- renderer 解析屬性，經過新的訊息告訴 Browser；Browser 的 frame 樹為每個子 frame 記 pending 的旗標。
- commit 時 Browser 算出文件的旗標，origin 照它決定（opaque 的也決定了 loader 的參數：RFC 0128 的 opaque context，沒有 cookie），連同 commit 交給 renderer。
- renderer 這一側照文件的旗標擋：跑 script 的地方、`may_navigate`、開新視窗、autofocus；被擋時 DOM 記一條 console 的訊息（`dom::ConsoleNote`），要在 script 的位置的由 engine 補上位置。

## 詳細設計

盤點、Chromium 和規範的細節在 `details.md`。

### 解析

照 Chromium 的 `network::ParseWebSandboxPolicy`（`protocol::sandbox::parse`）：沒有屬性是沒有旗標；有屬性就是全部的旗標，每個認得的 token（不分大小寫）拿掉它的幾個。token 和旗標的對照照 Chromium 的表（`allow-scripts` 拿掉 scripts 和 automatic features，`allow-popups` 拿掉 popups 和 top navigation to custom protocols……）。不認得的 token 的訊息照 Chromium 算出來（`ParsedSandbox::error`：「'x' is an invalid sandbox flag.」），還沒寫進 console：Chrome 的那一條掛在 iframe 所在的行，machopo 的 parser 還不把元素的行交給屬性的處理（`docs/limitations.md`）。

### 旗標的路（已決定的事項第 2 條）

照 Chromium：

1. **renderer 解析、告訴 Browser**：`sandbox` 屬性改了時，renderer 解析，經過新的訊息（Chromium 的 `DidChangeFramePolicy`）告訴 Browser；子 frame 建立時 `DidCreateChildFrame` 也帶著它（Chromium 的 `CreateChildFrame` 帶著 frame policy）。子 frame 一開始的 `about:blank` 在 renderer 建立時就照旗標。
2. **Browser 的 frame 樹記著**：每個子 frame 一份 pending 的旗標，一定加上父 frame 現在的文件的旗標（Chromium 的 `SetPendingFramePolicy`，只能更嚴）；屬性改了，下一次導覽才生效。
3. **commit 時算**：Browser 算出文件的旗標（frame 的 pending 的，導覽開始時抄進 `CommitParams`；`about:blank` 和 `data:` 網址再加上發起者的文件的，照 Chromium 的 local scheme 繼承發起者的 policy），有 origin 的旗標就給 opaque 的 origin（Chromium 的 `GetOriginForURLLoaderFactoryBeforeResponse`），loader 的參數照它算（opaque 的 context，沒有 cookie）；旗標放在 `CommitNavigation` 裡交給 renderer，renderer 記在文件上。
4. **renderer 照文件的旗標擋**：script、autofocus、導覽（Blink 的 `LocalFrame::CanNavigate`，machopo 的 `history::may_navigate` 裡的 `frame::sandbox_allows_navigation`）、開新視窗（`blink::CreateNewWindow`，`frame::may_open_window`）。分層：旗標的型別和解析在 `protocol`（`protocol::sandbox`，Chromium 的 `network::mojom::WebSandboxFlags`、`ParseWebSandboxPolicy`）；DOM 只記 bit 和兩個它要的效果（origin 是新的 opaque 的、scripting 關著），旗標的意思由 `dom_api` 翻譯（`set_document_sandbox`）。engine 建新文件時就帶著旗標（`ScriptEngine::create_sandboxed_window`）：origin 是 opaque 的文件不沿用初始 `about:blank` 的 window（HTML 只在同源時沿用），V8 的 security token 也是新的；在 `DocumentLoader` 之前，parser 才照 scripting 解析 `<noscript>`。scripting 關著時：`<script>` 不跑、事件的 handler 屬性是 null、`javascript:` 網址不跑、autofocus 不算、lazy 的圖片不 lazy（HTML 的「will lazy load element steps」）。renderer 自己 commit 的 `javascript:` 文件照它取代的文件的旗標。
5. **Browser 照 Chromium 再檢查**：`OpenUrl` 分成頁面開的新視窗（`new_window`：沒有 frame 有的 target，Chromium 的 `CreateNewWindow`）和使用者的按鍵開的新分頁（Chromium 的 `OpenURL`）；沒有 `allow-popups` 的 frame 要開新視窗是壞訊息（Chromium 的 `RFH_CREATE_NEW_WINDOW_FROM_SANDBOXED_FRAME`）；改樹上沒有的 frame 的旗標是壞訊息（`FindAndVerifyChild`）。新視窗的 main frame 的 pending 旗標從開它的 frame 帶過去（`navigate_opened`，沒有 `allow-popups-to-escape-sandbox` 時；Chromium 的 `CreateNewWindow` 給新的 main frame 開它的 frame 的旗標），使用者的按鍵開的不帶。`history.go()` 的 `GoToEntryAtOffset` 帶著它的 `History` 的 frame，那個 frame 的文件沒有 `allow-top-navigation` 時（top navigation 的旗標，不是 navigation 的），只在會變的 frame 都在它底下時才走，不然什麼都不做（Chromium 的 `NavigationControllerImpl` 的 `DoesSandboxNavigationStayWithinSubtree`：只有一部分在它底下的也整個不走，免得停在沒有存在過的 history 狀態）。導覽最上層 Browser 不再檢查（Chromium 也不）。

### 擋什麼（已決定的事項第 1 條）

machopo 有的功能全部照文件的旗標擋：

- **origin**（沒有 `allow-same-origin`）：文件的 origin 是 opaque 的：`document.cookie`、`localStorage`、`sessionStorage` 照 opaque 的 origin（讀不到、丟 `SecurityError`），跨 frame 的存取照不同源，請求照 opaque 的 context（RFC 0128：initiator 是 `null`、不帶 cookie）。
- **script**（沒有 `allow-scripts`）：不跑 script（parser 的、插入的、事件的 handler、`javascript:` 網址），`<noscript>` 照 scripting 關著解析；automatic features（autofocus）一起擋。
- **導覽**：只能導覽自己和自己底下的 frame（sandboxed navigation browsing context flag）；最上層只有 `allow-top-navigation`，或 `allow-top-navigation-by-user-activation` 加上發起的 frame 的 window 有使用者的啟動時可以（Blink 的 `CanNavigate` 看發起的 frame）。`Location` 的導覽被擋時丟 `SecurityError`（Blink 的 `Location::SetLocation`），連結的 target 被擋時什麼都不做。發起者是 incumbent realm（HTML 的 Location-object navigate，Blink 傳 `incumbent_window`），網址照 entry realm 解析；原本兩者都用 entry realm，sandbox 的 frame 的 `load` handler 裡呼叫父 frame 的函式導覽父 frame 時，會被當成 sandbox 的 frame 擋下。
- **history**：沒有 `allow-top-navigation` 的 sandbox 的 frame 的 `history.back()`、`go()` 只在會變的 frame 都在它底下時才走（上面的第 5 條）。
- **開新視窗**（連結的 `target=_blank`、找不到名字的）：沒有 `allow-popups` 時不開，script 的 `click()` 也照樣寫 console；有的時候新分頁的 main frame 帶著開它的 frame 的旗標，除非有 `allow-popups-to-escape-sandbox`。使用者按 Ctrl（macOS 是 Cmd）或中鍵開的新分頁不擋、不帶旗標（Chromium 的 `OpenURL`，不經過 `CreateNewWindow`）；script 的 click 的修飾鍵本來就不算（machopo 只看 trusted 的事件，Blink 的 `FindOrCreateFrameForNavigation` 也把 sandbox 的 frame 裡 untrusted 的修飾鍵當成沒有）。

machopo 還沒有的功能（表單送出、modal、下載、pointer lock、螢幕方向、presentation、storage access、`document.domain`、`window.open`）的旗標只解析、記在文件上，那些功能做的時候照它擋。

### console 的訊息（照 Chrome 實測，`details.md` 的「Chrome 實測：被擋時的 console 訊息（第 2 段開始前）」）

被擋的 script、導覽和新視窗各寫一條，原文、`source`、`url` 和 `lineNumber` 和 Chrome 一樣：

- 被擋的 script（`<script>`、事件的 handler、`javascript:` 網址；Blink 的 `CanExecuteScripts` 準備跑 script 時）：`security` 的錯誤，掛在那份文件的網址，沒有行號。scripting 因為別的原因關著（沒有 window 的文件）不寫。
- 導覽：`javascript` 的錯誤，「Unsafe attempt to initiate navigation for frame with origin '目標的 origin' from frame with URL '發起的文件的網址'. 原因」加換行，原因分三種（最上層、祖先和兄弟、只有 `-by-user-activation` 而沒有啟動）；新視窗：`security` 的錯誤。兩者都掛在正在跑的 script 的網址和行（Blink 的 `ConsoleMessage` 抓的 `SourceLocation`），沒有 script 在跑時（使用者的點擊）掛在文件的網址。

路：DOM 的操作記下 `dom::ConsoleNote`（`Cx::add_console_note`；要掛在 script 的用 `add_console_note_at_script`，它要 engine 在操作結束時補上位置：V8 要配置，操作裡不行，和請求的 stack 同一處）；renderer 在 task 之後交給 DevTools 的 `Log`（`LogEntry::of_dom`）。原本只有 JavaScript 的警告一種（`warn_console`），併進同一條路。

### 測試

測了什麼、怎麼驗證在 `progress.md` 的「驗收」：`protocol` 的單元測試（解析、token 的對照）；`browser` 的整合測試（frame 樹的旗標、commit 時的 origin、新分頁的旗標、history、使用者的點擊）；`dom_js` 的測試（script、導覽的規則、新視窗、console 的訊息）；`shell` 的 `devtools_log`（DevTools 看到的訊息）；WPT 的 `html/browsers/sandboxing/`、`the-iframe-element/` 的 sandbox 測試逐項和 Chrome 比；每一條規則的 mutation test。

### 分階段

兩段（已決定的事項第 3 條），都做完了：

1. **旗標的路、origin 和 script**：解析、`iframe.sandbox`、新的訊息、frame 樹的 pending 旗標、commit 時的旗標和 origin、`CommitNavigation` 的欄位；origin（opaque）和 script（含 autofocus、`<noscript>`）；Browser 的「改樹上沒有的 frame 的旗標是壞訊息」。相關的 WPT 逐項和 Chrome 比。
2. **導覽和開新分頁**：導覽別的 frame、最上層和使用者的啟動；開新視窗、新分頁帶旗標；history；Browser 的「沒有 `allow-popups` 開新視窗是壞訊息」；被擋時的 console 訊息（開始前先實測 Chrome 的原文）；剩下的 WPT；收尾。

## 缺點

- **旗標只能在 machopo 有的功能上擋**：表單、modal、下載這些功能做的時候要記得照旗標擋，漏了就是逃出沙箱。
- **OOPIF 之前，sandbox 的 frame 和嵌它的頁面在同一個 renderer**：被攻破的 renderer 不受旗標限制（Chromium 桌面版把 sandbox 的 frame 放進自己的 process）。

## 理由和其他做法

- **不做的話**：網站以為嵌進來的內容被關在沙箱裡，在 machopo 不是。
- 各題的選項和不選的理由在「已決定的事項」。

## 先例

`details.md` 的「Chromium」：`services/network/public/cpp/web_sandbox_flags.cc`（解析）、`content/browser/renderer_host/frame_tree_node.cc`（`SetPendingFramePolicy`：父 frame 的旗標帶下去）、`navigation_request.cc`（commit 時的旗標和 origin）、Chromium 的 sandbox 說明（`details.md`）；HTML 的 sandboxing 一節。

## 已決定的事項

1. **machopo 有的功能全部擋，新分頁帶著旗標（A1）**（2026-11-28，使用者決定）：照「擋什麼」。理由：和 Chrome 一樣，沒有逃出沙箱的路（有 `allow-popups` 沒有 `-to-escape-sandbox` 時，開一個不帶沙箱的新分頁就是逃出）；帶旗標的 main frame 是 opaque 的，machopo 已經有這種 main frame（`data:` 網址），鎖和換 renderer 都處理過；新分頁帶旗標的路之後 `window.open`（W）也用。代價：範圍最大。不選只做 origin 和 script（A2：sandbox 的 frame 還能把最上層導走、開不帶沙箱的新分頁）。不選新分頁不帶旗標、沒有 `-to-escape-sandbox` 時一律不開（A3：和 Chrome 不同，Chrome 開一個帶沙箱的新分頁）。也決定了 F2 在 W 之前做（F2 補的是安全的洞，小、沒有依賴，W 接著用它鋪的路）。
2. **旗標由 renderer 解析、Browser 記著、commit 時算出文件的旗標和 origin（R1）**（2026-11-28，使用者決定）：照「旗標的路」。理由：和 Chromium 一樣；cookie 和 storage 照 Browser 算的 origin，不靠 renderer 自律；之後的 OOPIF、`window.open` 接得上。代價：`protocol` 多一個訊息，`CommitNavigation` 多一個欄位。老實說 OOPIF 之前嵌它的頁面和它在同一個 renderer，被攻破的 renderer 本來就拿得到頁面的 loader，Browser 的檢查現在擋的是正常的 renderer 的 bug，不是被攻破的 renderer；選它是因為 origin 一定要由 Browser 決定，到 OOPIF 時不用重做。不選只在 renderer（R2：Browser 的 frame 樹記的還是網址的 origin，它建的 loader 照樣帶 cookie，opaque 的 origin 只在 renderer 是真的）。不選 Browser 再多檢查導覽最上層（R3：Chromium 沒有；OOPIF 之前擋不到什麼）。
3. **分兩段：先旗標的路、origin 和 script，再導覽和開新分頁（G1）**（2026-11-28，使用者決定）：照「分階段」。理由：第 1 段做完就補上最重要的兩個保護、可以先 merge；第 2 段的程式在別處（`may_navigate`、開新分頁），單獨驗收，開始前先實測 Chrome 的訊息。代價：階段的檢查跑兩次。不選一段做完（G2：最重要的保護要等全部做完）。不選三段（G3：origin 和 script 共用同一條路，拆開不值得多跑一次檢查）。

4. **實作時照 Chromium 定的三件事**（2026-11-28，第 2 段；都是 Chromium 的做法，沒有另外的選項要比）：(1) 使用者的按鍵開的新分頁不是彈出視窗：sandbox 不擋、不帶旗標，`OpenUrl` 多一個 `new_window` 分開兩者（Chromium 是兩個訊息：`OpenURL`、`CreateNewWindow`）；原本打算一律照 `allow-popups` 擋，但那樣使用者在 sandbox 的 frame 裡按 Ctrl 開連結會被 Browser 當成壞訊息、結束 renderer。(2) `Location` 的發起者是 incumbent realm（上面的「導覽」）；原本的 entry realm 是沒有 sandbox 時看不出的 bug，第 2 段的規則讓它變成看得到的錯誤（父 frame 的函式被當成 sandbox 的 frame）。(3) sandbox 的 frame 的 history 照 Chromium 的 `GoToIndex` 擋（Blink 的 `History::go` 不檢查）：`GoToEntryAtOffset` 多帶 frame，不在樹上的 frame 是壞訊息。

## 待決定

沒有（已決定的事項第 1 到 4 條）。

## 之後的可能

- CSP 的 `sandbox` directive（main frame 也會被 sandbox：opaque 的最上層的鎖要先想清楚）。
- 屬性裡不認得的 token 寫 console（訊息已經算好，要 parser 把元素的行交給屬性的處理）。
- sandbox 的 frame 放進自己的 process（OOPIF 之後）。
- `window.open` 的彈出視窗帶著旗標（W）。
