# RFC 0121 的細節：Windows 的 renderer sandbox 的下一級

## Chromium

2026-11-27 讀的 Chromium 的 `main`（GitHub 的鏡像，原始碼的純文字）：`sandbox/policy/win/sandbox_win.cc`、`content/browser/renderer_host/renderer_sandboxed_process_launcher_delegate.cc`、`sandbox/win/src/security_level.h`、`sandbox/policy/features.cc`、`sandbox/win/src/restricted_token.cc`。沒讀到的：handle closer 的實作（`sandbox/win/src/handle_closer.cc` 在這個路徑不存在）、`restricted_token_utils.cc` 和建 window station 的程式（被限速）；下面標「沒讀到」的是只看到呼叫的地方。

renderer 的設定（`AddDefaultConfigForSandboxedProcess`、`GenerateConfigForSandboxedProcess` 和 renderer 的 delegate 的 `InitializeConfig`）：

- **token**：`SetTokenLevel(USER_RESTRICTED_SAME_ACCESS, USER_LOCKDOWN)`。`security_level.h` 的表：`USER_RESTRICTED_SAME_ACCESS` 限制在全部的 SID、權限全留；`USER_LOCKDOWN` 全部的 SID 都只能拒絕、限制在 null SID、沒有權限（實際列哪些 SID 在 `restricted_token_utils.cc`，沒讀到）。不給 renderer 隨機的限制 SID（`AddRestrictingRandomSid` 只有 GPU process）。
- **default DACL**：`SetLockdownDefaultDacl()`。`restricted_token.cc`：鎖住時拿掉 logon session 的 SID、不給 RESTRICTED；沒鎖時給 RESTRICTED `GENERIC_ALL`。兩種都再給使用者 `GENERIC_ALL`。
- **integrity**：Low，降權後 Untrusted（`SetDelayedIntegrityLevel`）。
- **job**：`JobLevel::kLockdown`（`security_level.h` 的說明：一個 process、job 關掉時結束、未處理的例外時結束、UI 的限制）；記憶體上限 `GetJobMemoryLimit`：renderer 固定 1 TB，只在 64 位元。
- **desktop**：`SetDesktop(Desktop::kAlternateWinstation)`（怎麼建、權限：沒讀到）。
- **建立時的 mitigation**（每種 sandbox 都有）：heap 損壞時結束、bottom-up ASLR、DEP（連同 ATL thunk）、extension point、SEHOP、只用系統的字型、不從遠端和 Low 的位置載入 DLL、間接分支預測的隔離、KTM、FSCTL。renderer 另有 win32k 關掉（`AddWin32kLockdownPolicy`，連同 `SetFakeGdiInit`：`kWinSboxNoFakeGdiInit` 預設關，所以照做）。
- **條件的**：一般的（有 JIT 的）renderer 關 CET（`MITIGATION_CET_DISABLED`，jitless 的才開）；jitless 的另加 ACG（`MITIGATION_DYNAMIC_CODE_DISABLE`）；不共用 CPU core（`kWinSboxRestrictCoreSharingOnRenderer`，預設關，Windows 11 24H2 起）。
- **降權後的 mitigation**：DLL 的搜尋順序；只許 Microsoft 簽章的 DLL（`--allow-third-party-modules` 時不開）；嚴格的 handle 檢查（`kWinSboxStrictHandleChecks`，預設開）。
- **handle**：降權後關掉 `kDeviceApi`（預設的設定）、`kKsecDD`（只有 renderer，註解說和 `cryptbase.dll` 有關）、`kWindowsShellGlobalCounters`（`AddBaseHandleClosePolicy`）；`SetDisconnectCsrss()` 在 `kEnableCsrssLockdown` 後面，預設關。
- **shim**：`SetZeroAppShim()`，`kWinSboxZeroAppShim` 預設開（WOW 和 ARM64 的模擬底下不做）。
- **環境**：`SetFilterEnvironment(true)`。
- **AppContainer**：`kRendererAppContainer`，預設關。
- 其他：一份會出問題的 DLL 的清單在啟動時卸掉（`kTroublesomeDlls`）；`kWinSboxModuleTamperingProtection` 預設開，但上面讀的檔案沒有用到它（不知道它在哪裡、對 renderer 做什麼）。

## 和 machopo 的對照

machopo 現在的（`sunaba::windows` 的 `Command::spawn` 和 `lower`，`shell::renderer_sandbox` 的 Windows 的 policy）：

| | Chromium 的 renderer | machopo 的 renderer |
| --- | --- | --- |
| token | restricted：`USER_RESTRICTED_SAME_ACCESS` 再到 `USER_LOCKDOWN` | AppContainer（沒有 capability，每個 renderer 一個），沒有 restricted 的 SID |
| default DACL | 鎖住 | 系統給 AppContainer 的 token 的 |
| integrity | Low 再到 Untrusted | 同左（renderer 自己降） |
| job | 一個 process、關掉時結束、例外時結束、UI 的限制、記憶體 1 TB | 同左，**沒有記憶體上限** |
| desktop | 另一個 window station 和 desktop | **和 Browser 同一個** |
| 建立時的 mitigation | 上面那一串，加 win32k | 同左，加 child process 的限制；只許 Microsoft 簽章的也在建立時（Chromium 在降權後） |
| 降權後 | DLL 的搜尋順序、嚴格的 handle 檢查 | 同左 |
| CET、ACG | 只有 jitless 的開 | 都沒開（沒有 jitless 的模式） |
| 關掉的 handle | `DeviceApi`、`KsecDD`、shell 的 global counters | **沒有** |
| CSRSS | 預設不斷開 | 不斷開 |
| shim | 不載入 | **載入**（量環境變數時 apphelp 在 renderer 裡讀 `SHIM_*`） |
| 環境 | 過濾 | 過濾（清單） |
| 不共用 CPU core | 預設關 | 沒有 |

## 已經知道的（0117 那份的探測）

`rfcs/0117-renderer-sandbox/details.md` 的「Windows 的探測」，2026-11-26，同一台機器：

- `USER_LOCKDOWN` 沒有暖機時 renderer 以 `0xC0000409` 結束：V8 的 `RandomNumberGenerator` 叫 `rand_s`，它第一次被呼叫時要載入系統的 DLL。暖機（第一個 thread 帶著啟動時的 token 時先要一次亂數）之後起得來。
- 啟動時的 token 只有第一個 thread 有，其他 thread 一開始就是 lockdown 的：暖機要在第一個 thread 上做。
- `USER_LOCKDOWN`、Untrusted、暖機、job 和 mitigation（沒有 AppContainer）：測試字型的頁面相同，系統字型的頁面空白，是那時的 renderer 用 DirectWrite；renderer 改由 Browser 給字型之後沒有再試。
- 從裡面試：`USER_LOCKDOWN` 擋住 `C:\Windows\win.ini`、`C:\Windows\Fonts`、`C:\ProgramData`、Winsock（起不來）、啟動 `cmd.exe`；AppContainer 擋不住前兩個；LPAC 也擋不住前兩個。
- 另一個 window station 和 desktop 加在全部都開的設定上，截圖相同；沒有確認 renderer 真的在那個 desktop 上。

## 探測：token（第 1 段之前）

2026-11-27，Windows 11（10.0.26300，x86-64），debug build，`ecac4005` 加上只為探測寫的原型（沒有 commit）：`sunaba::windows::Command::spawn` 在環境變數 `MACHOPO_SANDBOX_PROBE_LOCKDOWN` 有值時照它建 token，process 建成暫停的、給第一個 thread 啟動用的 token 再讓它跑；`lower` 最後還回（`RevertToSelf`）。`sunaba` 的整合測試的 probe（`sunaba_probe`）和 `shell` 的測試都經過這條路。看 process 的工具在這個資料夾：`tokenlook.py`（從外面讀 token：group 和它們是不是只能拒絕、限制的 SID、AppContainer、default DACL），`dlltrace.py`（debug 一個指令和它的 child，印出載入的 DLL、例外、結束碼；`--snaps` 打開 loader 的 snap，loader 的錯誤印出來）。

**哪些組合起得來**（`sunaba_probe -- nothing`，只看 process 起不起得來）：

| primary token | 第一個 thread 的 token | 結果 |
| --- | --- | --- |
| AppContainer，從 same-access 的 restricted token 建（限制在全部的 SID） | 沒有 | 起得來 |
| AppContainer，從 `USER_LOCKDOWN` 建 | 沒有 | 初始化時 `0xC0000022`（`ACCESS_DENIED`） |
| 同上 | 現在那種 AppContainer 的（沒有 restricted） | `0xC00000A5`（`BAD_IMPERSONATION_LEVEL`）：primary 是 restricted 的，沒有 restricted 的 token 只能當 identification 用 |
| 同上 | AppContainer，從 same-access 的 restricted token 建（Chromium 的啟動用的 token 加 AppContainer） | `0xC0000135`（`DLL_NOT_FOUND`）；loader 的平行載入關掉之後**起得來** |
| 反過來：先建 AppContainer 的 token，再從它建 `USER_LOCKDOWN` | 現在那種 AppContainer 的 | `CreateProcessAsUserW` 被拒絕（1314，要 `SE_ASSIGNPRIMARYTOKEN`：系統不認它是呼叫的人的 token 的子集） |
| `USER_LOCKDOWN`，Low，沒有 AppContainer（0117 那份的做法） | same-access，Low | 起得來 |

- **lockdown 的哪一部分讓 AppContainer 的 process 起不來**：只要有一個只能拒絕的 SID，或限制在 null SID，就是 `0xC0000022`；只拿掉權限沒有影響。限制的 SID 也不能多放 AppContainer 或 ALL APPLICATION PACKAGES 的 SID（`CreateRestrictedToken` 回 87），多放 RESTRICTED 或 Everyone 還是起不來。所以起得來要靠第一個 thread 帶著啟動用的 token。
- **平行載入**：loader snap 顯示第一個 thread（帶著啟動用的 token）載入的都成功，失敗的 `userenv.dll`、`VCRUNTIME140.dll` 是在 loader 的 worker thread 上載的：那些 thread 只有 lockdown 的 primary token，開不了 System32 底下不在 KnownDLLs 裡的檔案。原型在 process 還暫停時把 `RTL_USER_PROCESS_PARAMETERS` 的 `LoaderThreads`（x64，+ `0x40C`，不在公開的 header 裡）寫成 1，loader 就只在第一個 thread 上載入。不寫時真的 renderer 也起不來。沒有 AppContainer 的 lockdown 不寫也起得來（沒有查為什麼）。Chromium 怎麼處理（它的 renderer 預設不用 AppContainer，沒有這個組合）沒有查。
- **不平行載入的代價**：machopo 的執行檔當 renderer 啟動（`--type=renderer --no-sandbox`，沒有通道，載入完靜態連結的 DLL、進到 `main` 就結束），從建立到結束的時間，平行載入和 `LoaderThreads` 寫成 1 交替各 60 次（debug build）：中位數 20.06 ms 和 20.21 ms，p10、p90 也差不到 0.4 ms，在起伏之內。renderer 啟動時只載入十幾個系統的 DLL，平行讀檔省不了什麼。
- **暖機**：不另外要一次亂數，renderer 也起得來、頁面相同（0117 那時的 `rand_s` 要載入的 DLL，現在在啟動時就由 loader 載入了）。

**從裡面試**（`sunaba_probe`，現在的 sandbox 和 T1）：

| 嘗試 | 現在（AppContainer） | T1 |
| --- | --- | --- |
| 讀 `C:\Windows\win.ini` | 讀得到 | 拒絕 |
| 列 `C:\Windows\Fonts` | 列得出 | 拒絕 |
| 讀 System32 的 `kernel32.dll` | 讀得到 | 拒絕 |
| 列 `C:\Program Files` | 列得出 | 拒絕 |
| `stat` System32 的 `cmd.exe` | 可以 | 拒絕 |
| 列 `C:\ProgramData` | 拒絕 | 拒絕 |
| 綁本機的 port 來聽 | 可以 | Winsock 起不來（`WSAStartup` 回 10107） |
| 連本機、連外面 | 擋 | Winsock 起不來 |
| 啟動 `cmd.exe` | 擋（child process 的限制，367） | 擋（拒絕，5） |
| 讀自己的 token（是不是 AppContainer、integrity） | 讀得到 | 拒絕：自己的 token 開不了，測試要改成從外面讀 |
| 用延後載入的 DLL（`ws2_32`） | 載入 | process 以 `0xC06D007E`（延後載入找不到 DLL）結束 |

- 使用者的檔案、在 Temp 和 `LocalLow` 建檔案、開別的 process：兩者都擋（`sunaba` 現有的測試照樣通過）。
- **延後載入的 DLL 在降權之後載不進來**，而且是讓 process 結束的例外，不是錯誤：renderer 的程式碰到它們（現在是 Browser 才用的視窗、繪圖、網路的那些）就當掉。現在的 AppContainer 底下它們載得進來（只是 win32k 的那些用不了）。

**真的 renderer**（`shell` 的測試，renderer 都經過 T1；`tokenlook.py` 在測試跑的時候讀到它是 restricted、限制在 null SID、在 AppContainer 裡）：整套的整合測試裡失敗的只有 `renderer_sandbox_windows` 裡斷言現在的行為、而 T1 改掉的：讀得到系統的檔案和字型的目錄、網路的錯誤碼是 `WSAEACCES`（T1 底下 probe 在延後載入 `ws2_32` 時結束）、probe 從裡面讀自己的 token。sandbox 裡外和 thread 上的 renderer 畫同一頁的比較（系統字型、測試字型）通過。

**token 和 default DACL**（`tokenlook.py`）：

- 現在的：15 個 group，只有系統本來就設成只能拒絕的兩個（Administrators 那兩個）是只能拒絕的；沒有限制的 SID；AppContainer。
- T1：15 個 group 全部只能拒絕（連同使用者的 SID 和 logon session），限制在 null SID，AppContainer，Untrusted。
- **default DACL 兩者相同**：使用者、Administrators、SYSTEM、它自己的 AppContainer，各 `GENERIC_ALL`；沒有 logon session、沒有 RESTRICTED。Chromium 的 `SetLockdownDefaultDacl` 做的（拿掉 logon session、不給 RESTRICTED）已經是這樣：系統給 AppContainer 的 token 的 default DACL 本來就沒有它們。另一個 renderer 要開它的物件，要同時過一般的檢查和 AppContainer 的檢查（它的 AppContainer 的 SID 不在上面），開不了。沒有實際建一個物件讓另一個 renderer 去開。

## 探測：desktop 和記憶體上限（第 2 段之前）

2026-11-27，同一台機器（實體記憶體 63.6 GB，commit 的上限 67.6 GB），debug build，`f53ab848` 加上只為探測寫的原型（沒有 commit）：環境變數 `SUNABA_PROBE_DESKTOP` 有值時，`sunaba::windows::Command::spawn` 第一次給 policy 底下的 process 建一個 window station 和 desktop（`winsta`：新的 window station 和它上面的 desktop，照 Chromium 暫時換 Browser 的 window station；`desktop`：Browser 的 window station 上的 desktop，不用換），之後的都用它，`lpDesktop` 是它的完整名字；`SUNABA_PROBE_DACL=copy` 時照 Chromium 複製 Browser 的 window station 和 desktop 的 DACL，否則用預設的（建的人的 token 的 default DACL）。`SUNABA_PROBE_MEMORY_MB` 有值時 job 設 `JOB_OBJECT_LIMIT_PROCESS_MEMORY`，job 接一個 completion port，另一個 thread 把收到的訊息印出來。`sunaba_probe` 暫時多一個嘗試：`EnumWindows` 數它的 desktop 上的視窗。從外面看的工具（沒有放進這個資料夾）：讀 process 的 `DesktopInfo`（x64 的 `RTL_USER_PROCESS_PARAMETERS` + `0xC0`）、列它的 window station 和 desktop 的 handle（`NtQuerySystemInformation` 的 handle 表，複製過來問型別和名字）、讀它的 commit（`PeakPagefileUsage`）。

**desktop**：

- 現在：renderer 的 `DesktopInfo` 是 `WinSta0\Default`（使用者的），但它**沒有任何 window station 或 desktop 的 handle**：win32k 關著，沒有 thread 連上去。
- 四種組合（`winsta`、`desktop` × 預設的、複製的 DACL）renderer 都起得來，`DesktopInfo` 是 Browser 建的那個，頁面照常（視窗裡開 `about:demo`；`shell` 的 `renderer_sandbox_windows` 13 個測試在 `winsta` 加預設的 DACL 底下都通過，其中截圖比較 sandbox 裡外的兩個；32 個 Browser，每個的 renderer 都在它建的 desktop 上）。DACL 不影響 renderer，因為它不連上 desktop。
- **允許 win32k 的 process**（`sunaba_probe --allow-win32k -- windows`，lockdown 的 token、AppContainer、job 的 UI 限制；複製的 DACL 連得上，推測是第一個 thread 連上 desktop 時還帶著啟動用的 token，沒有查）：

  | desktop | 結果 |
  | --- | --- |
  | 使用者的（現在） | 起得來，看到 0 個視窗（job 的 UI 限制擋住別的 process 的；沒有 policy 的看到 890 個） |
  | 新的，預設的 DACL | **起不來**：`0xC0000142`（`DLL_INIT_FAILED`，user32 連不上 desktop：DACL 沒有它的 AppContainer 或 ALL APPLICATION PACKAGES） |
  | 新的，複製的 DACL | 起得來，`EnumWindows` 沒有視窗 |

  所以預設的 DACL 比 Chromium 的（複製加上 RESTRICTED 的拒絕、把 integrity 的標籤降到 renderer 的）更緊：win32k 的限制被繞過時，process 連 desktop 都連不上。Chromium 複製的理由是它的其他 process（GPU 等）和以前的 renderer 要用 win32k。
- **換 window station 的那一下**（`SetProcessWindowStation` 到新的、建 desktop、換回來）：0.11～0.42 ms（33 次）；整個建立 0.5～1 ms。建它的是 Browser 的 UI thread（`RenderProcessHost::launch` 在 UI thread 上同步啟動 renderer），它在那之前就連在使用者的 desktop 上了，換 process 的 window station 不影響已經連上的 thread；那一下之內第一次用 `user32` 的別的 thread 會連到新的 window station 上（沒有看到，也沒有在測）。Chromium 一樣在其他 thread 跑著時換（broker 第一次啟動要它的 target 時）。
- **window station 的名字**：`CreateWindowStationW` 不給名字時，名字是 `Service-0x0-<logon session 的 ID>$`，同一個 logon session 的每個呼叫拿到的是**同一個** window station（DACL 是第一個建它的人的），各個 Browser 的 desktop 名字不同（帶 Browser 的 PID）。Chromium 也不給名字，所以和同一個使用者的 Chrome 的 renderer 共用一個 window station（各自的 desktop）。window station 擁有 clipboard、atom table：renderer 連不上 win32k，用不到它們。
- 一個 Browser 一個 desktop（S1）時，desktop heap 只多用一份。

**記憶體上限**（上限 1 GB，三種長不停的頁面）：

| 頁面 | 碰到上限（job 的訊息 9，`PROCESS_MEMORY_LIMIT`） | 結果 |
| --- | --- | --- |
| JS 的物件一直加 | 1.6 s | 一個 thread 的 stack 長不了（commit 不到）、V8 印出「Fatal process out of memory」，結束碼 `0xC00000FD`（stack overflow），訊息 8（不正常的結束） |
| `ArrayBuffer`（64 MB）一直加 | 0.24 s | **renderer 沒有結束**：配置失敗是 JS 的 `RangeError`，頁面接住了照樣跑 |
| DOM（`div`）一直加 | 1.4 s | 1.9 s 後「memory allocation of 666894336 bytes failed」，結束碼 `0xC0000409`（Rust 的配置失敗 abort） |

- **結束碼說不出是記憶體**：同一個原因有三種結束碼（還有下一條的第四種），`sad_tab::Reason::of_exit` 都是「當掉」。job 的訊息 9 每次都有，而且在結束之前：Browser 要知道是上限，看 job 的 completion port（訊息排在 port 裡，不用有 thread 等著：renderer 結束時不等待地讀它就好）。Chromium 的做法是 broker 的 thread 收到訊息 9 就結束整個 job（結束碼 `SBOX_FATAL_MEMORY_EXCEEDED`，7012），對應到 `TERMINATION_STATUS_OOM`，sad tab 有記憶體不夠的標題和說明（只有 Windows 有那些字串）。照那樣做的話，上面接住 `RangeError` 的頁面也會被結束。
- **沒有上限時**：JS 的頁面停在 V8 自己的 heap 上限（commit 最多 1570 MB，「Ineffective mark-compacts near heap limit」）。結束碼是 `0xC06D007E`：V8 的 fatal 的處理要印 stack，叫 `dbghelp.dll` 的 `SymSetOptions`，它是延後載入的，降權之後載不進來，走到延後載入失敗的 hook（stderr 有那一行）。bindings 沒有設 V8 的 OOM 的處理（`set_oom_error_handler`）。DOM 的頁面沒有東西擋：140 s 長到 19.6 GB 還在長，最後會用到 commit 的上限（整台機器的配置都開始失敗）。
- 順便看到的（和這份無關）：一個段落裡超過 65536 個有樣式的 inline 元素時，parley 的 `push_style` 的 assert（「too many styles」）讓 renderer panic（結束碼 4）。

**renderer 最多 commit 多少**（`PeakPagefileUsage`，每 0.3～0.5 s 讀一次所有的 renderer；每次讀要 1 s 左右找出 renderer，活不到那麼久的沒有讀到）：

| 頁面 | renderer | 最多 |
| --- | --- | --- |
| `about:demo` | 1 | 86～101 MB |
| Speedometer 3.1（`bench.py speedometer --browser machopo`，perf profile，每個 suite 10 次） | 20 個 suite，讀到 19 個 | 349 MB（其次 331、305 MB） |
| blink_perf（`bench.py blink-perf`，`pages.txt` 的 30 頁） | 讀到 10 個 | 464 MB（其次 253、170 MB；其他 100 MB 以下） |
| 上面的 JS 的頁面，沒有上限 | 1 | 1570 MB（V8 的 heap 上限） |

正常的頁面離 1 GB 都還遠，V8 的 heap 自己停在 1.5 GB 左右；擋不住的是 DOM、layout、圖片、canvas、wasm 的記憶體這些 V8 的 heap 以外的。

## 要探測的

第 1 段之前（做了，上一節；4 的後半「實際讓另一個 renderer 開」由第 1 段的測試做了：另一個 AppContainer 的 lockdown 的 process，用 lockdown 的 token 或啟動用的 token，都開不了它的 process（process 的安全描述來自它的 default DACL），使用者的一般 process 開得了）：

1. `CreateAppContainerToken` 從 `USER_LOCKDOWN` 的 restricted token 建得出 AppContainer 的 token 嗎（T1）；第一個 thread 帶的 impersonation token 要哪一種（現在那種 AppContainer 的，或 restricted 的 same access 加 AppContainer）。
2. 疊上之後，系統字型、測試字型的頁面和 wasm 的文件和沒有 sandbox 時相同嗎；暖機除了亂數還要什麼（降權後失敗的 DLL、檔案）。
3. 從裡面試的那幾件（檔案、socket、綁本機的 port、啟動 process）在 T1 底下的結果。
4. T1 的 token 的 default DACL 長什麼樣；照 Chromium 鎖住之後，另一個 renderer（另一個 AppContainer）和 Browser 各開不開得了 renderer 建的有名字的物件。

第 2 段之前（做了，上一節）：

5. 從外面讀 `RTL_USER_PROCESS_PARAMETERS` 的 `DesktopInfo`（x64 的位移），確認 renderer 在 Browser 建的 desktop 上。
6. S1：所有 renderer 共用時，第一個 renderer 啟動時換 Browser 的 window station 的那一下，對 Browser 的 UI thread 有沒有影響（只有建 window station 時要換）。
7. 記憶體上限：renderer 超過時怎麼結束（結束碼、Browser 看到的、分頁的說明）；常見的頁面和量測工具（Speedometer、blink_perf）的 renderer 最多 commit 多少，訂 M2 的數字。

第 3 段之前：

8. 降權後的 renderer 拿著哪些 handle（從外面列 process 的 handle 和它們的型別、名字），有沒有 `\Device\DeviceApi`、`\Device\KsecDD`、shell 的 global counters 的 section。
9. renderer 有沒有連到 csrss 的 ALPC port；關掉之後頁面照不照常。
10. 不載入 shim 的做法（Chromium 的 `SetZeroAppShim` 怎麼做的，要讀 `policy_base.cc` 或 `target_process.cc`）；做了之後頁面照不照常、apphelp 還在不在。
