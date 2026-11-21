# multi-process 在 Windows 上的探測

2026-11-21 做的探測，不合併，不在 machopo 的 workspace 裡。在寫 Windows 的平台層之前回答三件事：送的 thread 結束時，它發出的還沒完成的寫入會怎樣；pipe 的緩衝區和一次寫多大有沒有差；大的資料放在 pipe 裡和走 region 差多少。

只有 Windows（Windows 11 10.0.26200，Intel Core i9-13900K，桌機）。直接用 windows-sys 對 overlapped 的 named pipe，沒有經過 `ipc` 的任何一層。

```powershell
cd rfcs/0004-multi-process/prototype-windows
$env:CARGO_TARGET_DIR = "../../../target/mp-prototype-windows"; cargo run --release
```

同一個執行檔：沒有參數是 host，`--child <模式>` 是 child（host 自己啟動它，pipe 的一端用繼承的、數值放在命令列）。`behaviour`、`timings`、`compare` 只跑其中一部分。跑一次約 6 秒。`src/others.rs` 是和 `interprocess` 比的那一節。

## 結果

### 送的 thread 結束時，還沒完成的寫入

一條 thread 對沒有人在讀的 pipe 寫 1 MiB（緩衝區 4 KiB，寫入會等著），然後結束或留著；100 ms 之後另一端才開始讀。

| 完成怎麼通知 | 寫的 thread | 寫入 | 另一端讀到 |
| --- | --- | --- | --- |
| event | 留著 | 讀的一端讀完時完成 | 1 MiB |
| event | 結束 | **被取消**（`ERROR_OPERATION_ABORTED`，寫了 0 bytes） | **0** |
| completion port | 留著 | 完成，port 上有一筆 1 MiB 的通知 | 1 MiB |
| completion port | 結束 | 完成，port 上有一筆 1 MiB 的通知 | 1 MiB |

handle 沒有綁 completion port 時，thread 結束會取消它發出的 I/O，訊息整則不見，而且沒有任何東西告訴讀的一端。`Transport::write` 是任何送的 thread 都會呼叫的，所以平台層不能用「event 加上誰呼叫誰寫」的做法：要嘛綁 completion port，要嘛所有的寫入都由 IO thread 發出。

### 別的 process 搶不搶得到 pipe

兩端都由 host 建立（`FILE_FLAG_FIRST_PIPE_INSTANCE`、最多一個 instance）之後：第二個 client 用同一個名字連，`ERROR_PIPE_BUSY`；用同一個名字再建一個 instance，也是 `ERROR_PIPE_BUSY`。

### child 不在了

host 關掉自己手上 child 的那一端之後，等著的讀會以 `ERROR_BROKEN_PIPE` 結束：

| 情況 | 從什麼時候算 | 多久 |
| --- | --- | --- |
| child 沒有碰它那一端就結束 | 啟動 child | 4.6 到 5.5 ms（大部分是啟動 process） |
| child 被 kill | `TerminateProcess` | 0.6 到 1.4 ms |

不需要 host 另外說「child 不在了」，也不需要等 child 的 process handle。

### 小的訊息來回

64 bytes 去、64 bytes 回，20000 次，中位數（p90）：

| pipe 的緩衝區 | µs |
| --- | --- |
| 4 KiB | 6.8（7.1） |
| 64 KiB | 6.8（8.6） |
| 1 MiB | 7.0（9.2） |

### 大的資料

送到 child、等它回 1 byte 的時間，µs，中位數（p90）。「放在 pipe 裡」的是 host 直接從自己的緩衝區寫、child 直接讀進自己的緩衝區，沒有算平台層自己要多留的那一份複製；「region」是 host 複製進一塊留著重複用的 section、送 64 bytes 的訊息、child 複製出來。完整的表（緩衝區 × 一次寫多大 × 一次讀多大）由程式印出，這裡是一次讀 64 KiB 的幾列。

| 大小 | 緩衝區 4 KiB，整則寫 | 緩衝區 64 KiB，整則寫 | 緩衝區 64 KiB，一次寫 64 KiB | 緩衝區 1 MiB，整則寫 | region |
| --- | --- | --- | --- | --- | --- |
| 64 KiB | 15.5（15.7） | 15.5（15.7） | 一樣 | 15.1（15.6） | 13.9（14.2） |
| 1 MiB | 129.8（152.1） | 125.1（147.2） | 103.9（118.4） | 123.9（146.5） | 61.4（68.8） |
| 8 MiB | 1147（1272） | 1111（1235） | 970（1077） | 1121（1339） | 693（736） |

- **緩衝區的大小幾乎沒有差**：4 KiB 到 1 MiB 相差在 15% 以內，和 p90 的抖動同一個量級。和 macOS 上的 Unix socket 不同（預設的緩衝區讓超過的訊息慢 2.8 到 4 倍）：pipe 的寫入超過緩衝區時等在那裡，讀的一端直接從寫的一端的記憶體複製。
- **一次寫 64 KiB 比整則寫快一點**（1 MiB 時約 17%，8 MiB 時約 13%），緩衝區至少 64 KiB 時才有：不超過緩衝區的寫入當場完成，寫的和讀的可以同時進行。
- **region 在 64 KiB 時和放在 pipe 裡一樣，1 MiB 時快 2 倍，8 MiB 時快 1.6 倍**。64 KiB 的門檻在 Windows 上沒有理由改。
- 兩次執行之間 64 KiB 那幾列差到 1.5 倍（15.5 和 9.9 µs；這顆 CPU 有兩種核心，看排到哪一種），兩種做法一起變、比例不變；1 MiB 以上兩次相差在 6% 以內。

### 和 `interprocess` 比

同樣的位元組走 `interprocess` 2.4.9 的 local socket（Windows 上是它自己建的 named pipe，用的是它會等的那一組 API），旁邊是這份探測的 pipe（緩衝區 64 KiB）。送到 child、等它回答（64 bytes 的回 64 bytes，其餘回 1 byte），µs，中位數（p90）；跑了兩次，這是第一次的，第二次每一格相差在 12% 以內。

| 大小 | `interprocess` | 這裡的 pipe，整則寫 | 這裡的 pipe，一次寫 64 KiB |
| --- | --- | --- | --- |
| 64 B | 9.6（10.6） | 6.9（8.4） | 7.0（8.4） |
| 64 KiB | 15.8（17.0） | 13.3（13.6） | 14.7（14.8） |
| 1 MiB | 127.8（140.2） | 126.0（142.8） | 107.6（132.9） |
| 8 MiB | 1030（1245） | 1057（1315） | 933（1039） |

- 小的訊息 `interprocess` 慢約 40%（每個來回多 2.7 µs）；64 KiB 以上和整則寫的 pipe 一樣，底下是同一個東西。
- 速度不是不選它的理由。理由還是原本的：這裡量的是它會等的 API，送的時候不等要用它 tokio 的那一組或另一條 thread；兩端都由 host 建立再讓 child 繼承、handle 的數值跟著訊息走，都是它沒有的，要自己在它外面做。

## 給平台層的

- 完成用 completion port 收，叫醒用 `PostQueuedCompletionStatus`：IO thread 只等一個 port。
- pipe 的緩衝區 64 KiB，寫入一次最多收 64 KiB、其餘留在核心的佇列裡：留著等完成的那一份複製不超過 64 KiB，也是量到比較快的一種。
- 大資料的門檻留在 64 KiB。
- child 那一端的繼承（設成可繼承、數值交給 child、啟動之後 host 關掉自己手上的）在這裡每一節都用到，沒有問題；這份探測一次只啟動一個 child，同時啟動好幾個時要一把鎖的事它沒有碰到。

## 沒有量的

- 真的通道（訊息的框架、佇列、`DuplicateHandle` 送 handle）：平台層做完之後在它上面再量一次，和 Linux 量同一組大小。
- completion port 和 event 的等待各要多久：這裡的來回都是用 event 等的。
