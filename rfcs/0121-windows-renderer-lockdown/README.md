# RFC 0121：Windows 的 renderer sandbox 的下一級：restricted token 和另一個 desktop

- 開始日期：2026-11-27
- 狀態：🚧 已排程：2026-11-27（第 1 段做完：renderer 的 token 是 lockdown 加 AppContainer 的，降權之後開不了任何檔案、建不了 socket；第 2 段：desktop 和記憶體上限；進度在 `progress.md`）
- 細節：`details.md`

## 摘要

Windows 的 renderer process 現在在自己的 AppContainer（沒有 capability）、job 和建立時的 mitigation 裡，啟動後自己降到 Untrusted。這份把它推到 Chromium 的 Windows renderer 那一級，再多一點：AppContainer 的 token 改從 Chromium 的 `USER_LOCKDOWN`（所有的 group 都只能拒絕、限制在 null SID）建，token 的 default DACL 照 Chromium 鎖住；renderer 放進另一個 window station 和 desktop；job 加上記憶體的上限。另外三件 Chromium 有的（降權後關掉幾個 handle、斷開和 CSRSS 的連線、不載入相容性的 shim）先探測，有用才做。

## 動機

現況（2026-11-27 的 `main`，`ecac4005`；`docs/limitations.md` 的「Windows 的 sandbox 擋不住的」）：

- renderer 讀得到開放給所有應用程式的地方（`C:\Windows`、`Program Files`、`C:\Windows\Fonts`）：看得出這台裝了哪些軟體、哪個版本的系統 DLL。Chromium 的 renderer 讀不到（`USER_LOCKDOWN`）。原本 renderer 要讀系統的字型、用 DirectWrite，疊不上這一級；renderer 改由 Browser 給字型、不再載入 `dwrite.dll` 之後才疊得上。
- renderer 綁得了本機的 port 來聽（別人連不進來）。`USER_LOCKDOWN` 底下 Winsock 起不來（探測過，沒有和 AppContainer 一起試）。
- renderer 和 Browser 在同一個 desktop 上：擋住它對使用者的視窗送訊息、裝 hook 的只有「win32k 的 system call 關掉了」一層。Chromium 的 renderer 在另一個 window station 和 desktop 上。
- renderer 的記憶體沒有上限：一頁要幾十 GB 的（例如很大的 `border-collapse` 的表格）會拖垮整台機器，不是只結束那個分頁。Chromium 的 renderer 的 job 有上限。

**目標**：

1. sandbox 裡的 renderer 開不了 `C:\Windows`、`Program Files`、`C:\ProgramData` 底下的檔案，建不了 socket；從外面看得到它的 token 是 restricted 的。
2. renderer 在 Browser 建的另一個 window station 和 desktop 上，從外面讀得到。
3. renderer 用的記憶體超過上限時，只有它結束（分頁顯示 sad tab），不拖垮機器。
4. 網頁看到的不變：sandbox 裡外和 thread 上的 renderer 的比較照舊相同（系統字型、測試字型、wasm 的文件）。
5. 探測的三件（handle、CSRSS、shim）各有結論：做了，或為什麼不做，寫在這裡。

**不在範圍內**：

- 動態產生程式碼（ACG）和 CET：和 V8 的 JIT 衝突，Chromium 只對 jitless 的 renderer 開；machopo 沒有 jitless 的模式。
- 不和別的 process 共用 CPU core（`MITIGATION_RESTRICT_CORE_SHARING`）：Chromium 預設沒開，代價是排程和效能（「之後的可能」）。
- V8 的 sandbox：三個平台共用，改的是 `bindings` 的論證，另一份。
- Linux 和 macOS：Linux 的下一級是另一份，macOS 的在「之後的可能」。

要解決的 `docs/limitations.md` 裡的限制：「隔離」的「Windows 的 sandbox 擋不住的」的 (1)（讀得到開放給所有應用程式的地方）、(2)（綁得了本機的 port，看探測）、(3)（和 Browser 在同一個 desktop 上）。

## 設計概要

給 machopo 的使用者：看不到改變。renderer 吃光記憶體時，分頁顯示 sad tab、可以重新載入，而不是整台機器變慢。

給改 machopo 的人：

- renderer 降權之後開不了任何檔案、載入不了任何 DLL：要用的 DLL（延後載入的也算）和系統的初始化（亂數、時區的資料）要在降權之前、在 process 的第一個 thread 上做完（「暖機」）。漏了的在降權之後失敗，常常是 `abort` 沒有訊息；sandbox 裡外的比較和 renderer 的測試會看到它結束。
- 降權之前要做的事都在 `shell::renderer_sandbox::enter` 的 Windows 那一段（現在只有降到 Untrusted 和兩個 mitigation），新的放那裡。

## 詳細設計

### restricted token（A）和它的 default DACL（C）

Chromium 的 renderer 啟動時是 `USER_RESTRICTED_SAME_ACCESS`（限制在全部的 SID，等於原來的權限），降權後是 `USER_LOCKDOWN`；做法是 process 的 primary token 一開始就是 lockdown 的，第一個 thread 帶著啟動時的 impersonation token，降權時 `RevertToSelf`。machopo 的 renderer 現在的 token 是 AppContainer（`CreateAppContainerToken` 從 Browser 的 token 建），沒有 restricted 的 SID。

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| T1 | **AppContainer 的 token 從 `USER_LOCKDOWN` 的 restricted token 建**（primary 是 lockdown 加 AppContainer；第一個 thread 帶著從 Chromium 的 `USER_RESTRICTED_SAME_ACCESS` 建的 AppContainer 的 token，`lower` 最後還回去；loader 不平行載入） | 兩層都在：AppContainer 擋掉的（使用者的檔案、網路、每個 renderer 分開）照舊，lockdown 再擋掉開放給所有應用程式的地方，連 Winsock 都起不來。探測過：整套 `shell` 的測試照樣通過（`details.md` 的「探測：token（第 1 段之前）」） | loader 的平行載入要關掉（`RTL_USER_PROCESS_PARAMETERS` 的 `LoaderThreads`，不在公開的 header 裡；下面的 P1、P2）；降權後延後載入的 DLL 載不進來，碰到就當掉；renderer 開不了自己的 token |
| T2 | 照 Chromium：restricted token 加 Untrusted，不用 AppContainer | 和 Chromium 一樣，探測過（`USER_LOCKDOWN` 暖機之後，測試字型的頁面相同） | 退一步：每個 renderer 一個 AppContainer 是現在比 Chromium 多的一層（renderer 之間的物件分開、網路由 AppContainer 擋），拿掉要另外補 |
| T3 | 換成 less privileged AppContainer（LPAC） | 不用 restricted token，系統的機制 | 探測過：LPAC 照樣讀得到 `C:\Windows\win.ini` 和字型的目錄，解決不了目標 1 |

2026-11-27 的看法：**T1**（探測過，做得到）。

探測找到 T1 起得來的三個條件（`details.md` 的「探測：token（第 1 段之前）」）：

- **第一個 thread 的 token 要是 restricted 的**：primary 是 restricted 的時候，沒有 restricted 的 token 只能當 identification 用（`BAD_IMPERSONATION_LEVEL`），所以照 Chromium 用 same-access（限制在全部的 SID，等於原來的權限）。
- **loader 不平行載入**：loader 的 worker thread 只有 lockdown 的 primary token，開不了不在 KnownDLLs 裡的系統 DLL（`userenv.dll`、`VCRUNTIME140.dll`），process 起不來。

  | | 做法 | 好處 | 代價 |
  | --- | --- | --- | --- |
  | P1 | **Browser 在 renderer 還暫停時寫它的 `LoaderThreads` 成 1** | 探測過；一個欄位、一次寫入，和讀環境（`sunaba::windows::environment`）同一個結構 | 欄位不在公開的 header 裡，位移寫死（x64）；Windows 改了它的位置時 renderer 安靜地又平行載入、起不來（測試會看到） |
  | P2 | renderer 的執行檔不靜態連不在 KnownDLLs 裡的 DLL（CRT 靜態連結、`userenv` 延後載入） | 不碰沒有文件的欄位 | machopo 是 Browser 和 renderer 共用的執行檔，改的是整個程式的連結；清單之後會變，漏一個就起不來 |

  2026-11-27 的看法：P1。
- 暖機現在不需要（0117 那份要的亂數，DLL 現在在啟動時就載入了），照 Chromium 留著（已決定的事項第 4 條）。

**延後載入的 DLL**：降權之後它們載不進來，碰到時 process 以例外結束（`0xC06D007E`），現在的 AppContainer 底下載得進來。renderer 現在不碰它們（整套測試通過），但這是新的「不能碰」：之後 renderer 用到一個延後載入的 DLL，只在 sandbox 裡當掉。測試要守著（renderer 的 process 降權之後載入的 DLL 的清單，或 sandbox 的測試裡碰一次）。

**default DACL**（C）：renderer 自己建的物件（event、section、有名字的物件）照 token 的 default DACL 給權限。Chromium 的 `SetLockdownDefaultDacl` 拿掉 logon session 的 SID、不給 RESTRICTED，只留使用者的 `GENERIC_ALL`：同一個使用者的其他 process 用一般的 token 還開得了，用 restricted token 的 process（別的 renderer）開不了。探測讀到的（現在的和 T1 相同）：使用者、Administrators、SYSTEM 和它自己的 AppContainer，本來就沒有 logon session 和 RESTRICTED；另一個 renderer 還要過 AppContainer 的檢查，它的 SID 不在上面。定了**不另外改**（K1，已決定的事項第 3 條），測試從外面讀它，另加「另一個 renderer 開不了」。

**暖機**（降權之前在第一個 thread 上做的）：0117 那份的探測要一次亂數（`rand_s` 第一次會載入系統的 DLL；Chromium 的 `WarmupRandomnessInfrastructure`）；這次的探測不要也起得來（上面）。

### 另一個 window station 和 desktop（B）

0117 那份的已決定的事項第 26 條沒有做它，理由是 win32k 關掉之後 renderer 沒有 thread 會連到 desktop、測不出它擋了什麼，而建它要暫時換 Browser 整個 process 的 window station。這份要做的理由：win32k 的限制是單點，被繞過時 renderer 在使用者的 desktop 上；Chromium 和 Firefox 的 renderer 都有。「測不出」改成從外面讀：process 的 `RTL_USER_PROCESS_PARAMETERS` 的 `DesktopInfo` 是它啟動時給的 window station 和 desktop 的名字，讀法和環境一樣（`sunaba::windows::environment` 讀的是同一個結構）。

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| S1 | **Browser 建一個 window station 和 desktop，所有 renderer 共用**，Browser 結束時關 | 換 window station 的那一下只有一次（第一個 renderer 啟動時）；和 Chromium 一樣（broker 建一次、留著） | renderer 之間在同一個 desktop 上（win32k 關著，彼此也碰不到） |
| S2 | 每個 renderer 一個，跟著 renderer 的壽命關 | renderer 之間也分開 | 每次啟動都要換 Browser 的 window station（其他 thread 同時在用 `user32` 時有影響，要查）；desktop 是 session 的有限資源（desktop heap） |

2026-11-27 的看法：**S1**。

### job 的記憶體上限（F）

`JOB_OBJECT_LIMIT_PROCESS_MEMORY`：renderer 的 committed memory 超過時配置失敗，mimalloc 回 null，Rust 的配置失敗處理結束 process，Browser 顯示 sad tab。

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| M1 | 照 Chromium：固定 1 TB（64 位元） | 不會誤殺正常的頁面 | 一般的機器上 commit 的上限（實體記憶體加 pagefile）先到，等於沒有：目標 3 達不到 |
| M2 | **照實體記憶體算**（例如實體記憶體的大小，最少幾 GB） | 吃光記憶體之前只結束那個 renderer | 很重的頁面在小記憶體的機器上可能被結束；數字要量 |
| M3 | 不設 | 不改 | 目標 3 達不到 |

2026-11-27 的看法：**M2**，數字在那一段量了再定。Chromium 為什麼選固定的 1 TB 沒有查（讀到的只有那個常數）。

### 先探測、有用才做（D、E、H）

| | Chromium | 要探測的 | 做的條件 |
| --- | --- | --- | --- |
| 降權後關掉幾個 handle | renderer 關 `\Device\DeviceApi`、`\Device\KsecDD`、`windows_shell_global_counters` 的 section（handle closer） | 降權後的 renderer 拿著哪些 handle（從外面列） | 有這幾個、或有其他降權前開的、之後用不到的 |
| 斷開和 CSRSS 的連線 | `SetDisconnectCsrss`，**預設沒開**（`kEnableCsrssLockdown`） | renderer 有沒有連到 csrss 的 ALPC port；斷掉之後頁面照不照常 | Chromium 預設沒開，傾向不做；探測顯示沒有壞、而且有連線時再定 |
| 不載入相容性的 shim | `SetZeroAppShim`，預設開（`kWinSboxZeroAppShim`） | 量環境變數時看到 apphelp 在 renderer 裡讀 `SHIM_*`：shim 的引擎有載入；拿掉之後照不照常 | 拿掉了沒有壞 |

### 測試

- **從外面看**（`renderer_sandbox_windows` 的整合測試，和現在讀 integrity、mitigation 一樣）：renderer 的 token 是 restricted 的、group 都只能拒絕、限制在 null SID，default DACL 沒有 logon session 和 RESTRICTED、AppContainer 是它自己的；`LoaderThreads` 是 1；現在從裡面讀自己的 token 的測試（是不是 AppContainer、integrity）改成從外面讀；它在 Browser 建的 window station 和 desktop 上（`DesktopInfo`）；它的 job 有記憶體的上限。
- **從裡面試**（`sunaba` 的整合測試的 probe）：lockdown 加 AppContainer 的 process 讀不到 `C:\Windows\win.ini`、列不了 `C:\Windows\Fonts` 和 `C:\ProgramData`、建不了 socket、綁不了本機的 port；它建的物件另一個 lockdown 的 process（另一個 AppContainer）開不了；碰到延後載入的 DLL 時 stderr 寫出是哪一個再結束；配置超過 job 的上限時被結束。
- **網頁看到的不變**：`a_sandboxed_renderer_shows_a_page_as_one_outside_does` 照舊（系統字型、測試字型），加上 wasm 的文件；`about:crash` 照 renderer 當掉處理。
- **暖機**：sandbox 的測試和整套 `shell` 的測試在 sandbox 裡跑，renderer 都沒有在降權之後結束。
- mutation test：不疊 lockdown、第一個 thread 不帶啟動用的 token 或帶沒有 restricted 的、不還回、不寫 `LoaderThreads`（renderer 起不來）、延後載入失敗的 hook 不寫、不給 desktop、不設記憶體上限。不暖機是等價的（已決定的事項第 4 條）。

### 分階段

0. 草稿和探測（這一版；`details.md`）。
1. **token**（A 和 C；T1 探測過，已決定的事項第 1 到 5 條）：`sunaba` 建 lockdown 加 AppContainer 的 token、第一個 thread 帶 same-access 加 AppContainer 的 token、寫 `LoaderThreads`；`lower` 暖機、還回；延後載入失敗的 hook；斷言舊行為的測試改過來；測試。
2. **desktop 和記憶體上限**（B 和 F）：Browser 建、關 window station 和 desktop；`sunaba` 讀 `DesktopInfo`；job 的上限和它的數字；測試。
3. **探測的三件**（D、E、H）：各自探測，做了的加進 policy 和測試，不做的寫下理由。

每一段做完可以先 merge（`rfcs/README.md` 的「階段的 merge」）。都在 Windows 上做和驗。

## 缺點

- 暖機的清單要維護：renderer 之後用到新的 DLL、新的系統初始化時，要加進暖機，不然只在 sandbox 裡壞，常常沒有訊息。
- Windows 的 sandbox 變成三層 token（AppContainer、restricted、integrity），`sunaba` 的 `unsafe` 和論證跟著變多。
- 記憶體的上限可能結束正常、只是很重的頁面。

## 理由和其他做法

- **不做的話**：被攻破的 renderer 看得到整台機器裝了什麼、綁得了 port；win32k 的限制被繞過時它在使用者的 desktop 上；失控的頁面拖垮機器。
- **只做 token**：它是這份最大的一件，但 desktop 是 0118 那份的已決定的事項第 13 條要一起做、一起驗的，而且兩件改的是同一處（`sunaba` 的啟動和 `shell` 的 Windows policy）。
- 各節的選項比較在上面；Chromium 的每一項和 machopo 的對照在 `details.md`。

## 先例

2026-11-27 讀的（`details.md` 的「Chromium」）：

- **Chromium**：renderer 是 `USER_RESTRICTED_SAME_ACCESS` 再到 `USER_LOCKDOWN`、Low 再到 Untrusted、lockdown 的 default DACL、lockdown 的 job（記憶體上限 1 TB）、另一個 window station 和 desktop、win32k 關掉，降權後關幾個 handle、不載入 shim；AppContainer 是預設關著的實驗。
- **Firefox**（0117 那份讀的）：同一套程式，content process 是它自己加的 `USER_LOCKDOWN_WITH_TRAVERSE`、Low 再到 Untrusted、另一個 desktop、win32k 關掉。

## 已決定的事項

1. **AppContainer 的 token 改從 `USER_LOCKDOWN` 的 restricted token 建（T1）**（2026-11-27，使用者決定；探測過）：primary 是 lockdown 加 AppContainer（全部的 group 只能拒絕、限制在 null SID、沒有權限）；第一個 thread 帶著從 same-access 的 restricted token 建的 AppContainer 的 token（Chromium 的啟動用的 token 加 AppContainer），`lower` 最後還回去；loader 不平行載入（怎麼做是第 2 條）。理由：唯一同時保住每個 renderer 一個 AppContainer、又讓 renderer 讀不到開放給所有應用程式的地方、建不了 socket 的；探測時整套 `shell` 的測試只有斷言舊行為的那幾個改變，頁面的截圖相同；不平行載入量不出差別（`details.md` 的「探測：token（第 1 段之前）」）。代價：依賴沒有文件寫明的組合（restricted 加 AppContainer、第一個 thread 帶啟動用的 token），Windows 改了時 renderer 起不來、sandbox 的測試會看到；降權後延後載入的 DLL 載不進來，碰到就當掉；renderer 開不了自己的 token。不選照 Chromium 拿掉 AppContainer（T2：renderer 之間的物件和網路少一層；是 T1 做不到時的退路）。不選 LPAC（T3：照樣讀得到 `C:\Windows`）。對照 Chromium：renderer 預設是 T2，AppContainer 是預設關著的實驗。
2. **loader 不平行載入：Browser 在 renderer 還暫停時把它的 `LoaderThreads` 寫成 1（P1）**（2026-11-27，使用者決定；探測過、量過）：`RTL_USER_PROCESS_PARAMETERS` 的 `LoaderThreads`（x64，+ `0x40C`），只在 lockdown 的 policy 底下寫；測試從外面讀回它。理由：lockdown 的 primary token 底下，loader 的 worker thread 開不了不在 KnownDLLs 裡的 DLL（machopo 的執行檔靜態連著 `VCRUNTIME140.dll`、`VCRUNTIME140_1.dll`、`bcryptprimitives.dll`），只在帶著啟動用的 token 的第一個 thread 上載入才起得來；量不出變慢（renderer 啟動時只載入十幾個系統的 DLL）；只改 `sunaba` 一處，壞的時候 renderer 起不來、測試看得到。代價：欄位不在公開的 header 裡，位移寫死，`sunaba` 多一處 `unsafe`（論證在它的 `soundness.md`）。不選讓執行檔不靜態連那些 DLL（P2：CRT 靜態連結改的是 Browser 和 renderer 共用的整個程式，V8 的 artifact 要同一種 CRT，`bcryptprimitives` 是標準庫連的；之後多一個靜態連結的 DLL 就在 sandbox 裡起不來）。對照 Chromium：它的 renderer 預設不用 AppContainer，碰不到這件事；它的實驗怎麼做沒有查。
3. **default DACL 不另外改，測試守著（K1）**（2026-11-27，使用者決定；探測過）：系統給 AppContainer 的 token 的 default DACL 是使用者、Administrators、SYSTEM 和它自己的 AppContainer（現在的和 T1 的相同），本來就沒有 logon session 和 RESTRICTED，就是 Chromium 的 `SetLockdownDefaultDacl` 要的樣子。測試從外面讀它（沒有 logon session、沒有 RESTRICTED、AppContainer 是它自己的），再加一個：renderer 建的物件另一個 renderer 開不了（探測只讀了 DACL，沒有實際試）。理由：內容已經是要的；另一個 renderer 要過 AppContainer 的檢查和限制在 null SID 的檢查，兩層。代價：依賴系統建 AppContainer 的 token 時給的預設，改了由測試看到。不選自己設（K2：Chromium 的做法，它的 renderer 不用 AppContainer、預設的帶著 logon session，所以要設；這裡多一段 `unsafe`，內容一樣）。不選連使用者都不給（K3：Browser 是同一個使用者，開 renderer 的 thread、讀它的記憶體、crash 時看它都會被擋）。
4. **降權之前在第一個 thread 上要一次亂數（H2）**（2026-11-27，使用者決定）：`lower` 在還回啟動用的 token 之前叫一次 `RtlGenRandom`（`rand_s` 底下的那一個）。理由：0117 那份的探測裡，`USER_LOCKDOWN` 底下 V8 第一次叫 `rand_s` 時要載入系統的 DLL，載不進來就 `abort`、沒有訊息；這次的探測不暖機也起得來，是因為那個 DLL 在這台 Windows 11 上啟動時就載入了，不是 machopo 保證的，換了 Windows 的版本或 DLL 的載入順序就會回來，而且只在 sandbox 裡壞。成本是每個 renderer 啟動時一次系統呼叫。代價：這台機器上的測試抓不到它被拿掉，mutation 是等價的，記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。不選不暖機（H1：靠的是這台機器剛好如此）。不選降權前先載入可能用到的 DLL（H3：清單要猜，會載入 renderer 不該用的，例如 `user32`；延後載入的由測試守）。對照 Chromium：`WarmupRandomnessInfrastructure`，Firefox 一樣。
5. **延後載入的 DLL：sandbox 裡的測試守著，加一個延後載入失敗的 hook（G2）**（2026-11-27，使用者決定）：降權之後 renderer 載不進延後載入的 DLL（machopo 的執行檔有 Browser 才用的視窗、繪圖、網路的那些），碰到時 process 以例外結束。`sunaba` 定義 MSVC 的延後載入讀的 `__pfnDliFailureHook2`：失敗時在 stderr 寫出是哪個 DLL、哪個函式、它在 sandbox 裡用不了，再結束。測試：probe 在 T1 底下碰一次延後載入的 `ws2_32`，stderr 有那一行。理由：測試走得到的路由 sandbox 裡的 renderer 的測試守著，走不到的在使用者那裡當掉時至少說得出原因；只在失敗時走到。代價：`sunaba` 多一處 `unsafe`（定義連結器找的符號）。不選只靠測試（G1：沒走到的路當掉時只看得到「renderer 當掉」）。不選靜態地檢查（G3：Browser 和 renderer 是同一個執行檔，分不出哪段程式只在 renderer 上跑）。對照 Chromium（照記憶，沒有對原始碼）：`chrome.exe` 也有延後載入失敗的 hook，留下 DLL 的名字、產生 crash report。

## 待決定

- 第 2 段開始前：S1 還是 S2；記憶體上限的做法和數字（M2 的話量）。
- 第 3 段開始前：三件各自做不做（探測之後定）。
- 草稿時新找到、還沒放進範圍的：Chromium 的 `kWinSboxModuleTamperingProtection` 預設開，但讀到的 `sandbox_win.cc` 沒有用到它，不知道它在 renderer 上做什麼；要不要查、放進第 3 段。

## 之後的可能

- 不和別的 process 共用 CPU core（`MITIGATION_RESTRICT_CORE_SHARING`，Windows 11 24H2 起；Chromium 預設沒開）。
- jitless 的 renderer（`--jitless`）加 ACG 和 CET。
- macOS 的下一級（照 system call 的號碼再擋一層）。
