# `ipc` 的 soundness

這份文件說明 `ipc` 為什麼守得住它要保證的事：要保證什麼、信任誰、依賴 kernel 的哪些行為、用什麼方式守住、哪裡還守不住，以及修改時要審查什麼。`ipc` 是 workspace 裡除了 `bindings` 以外唯一可以寫 `unsafe` 的 crate（共用記憶體的存取和平台層的系統呼叫）；API、每一塊怎麼運作、測試怎麼跑在 `crates/ipc/README.md`。

## 結論

**目標**有兩層：

- **記憶體安全**：不寫 `unsafe` 的程式碼只透過 `ipc` 的 API，不可能造成 undefined behavior，不管通道另一端的 process 送什麼、在共用記憶體裡寫什麼。
- **通道的性質**（下面的 G1 到 G9）：不被信任的對端能做到的最壞的事，是讓它自己的那條通道被關掉。

**這是論證，不是證明。** 機械化證明過的只有兩小塊算術（池子選哪一塊、管道的額度，Verus）；協定用小的模型窮舉過（TLA+、loom）；其餘靠 fuzz、Miri、跨 process 的測試、mutation test 和人工審查。kernel 的行為證明不了，寫成假設，每一條有測試在那個平台上守著。

**分兩層是為了能驗證**：核心（`src/core/`）不做系統呼叫、不分平台（`python3 scripts/sanity.py deps` 檢查），所以能整個放進 Miri、fuzzer 和 loom；平台層（`src/platform/`）只做四件事（搬位元組和 handle、等、建和 map 共用記憶體、啟動時交接一端（通道的一端，或被 pipe 驅動的 process 的兩端），Windows 上啟動時也把 child 放進 job；macOS 和 Linux 上另有一個當場結束自己這個 process 的函式；三個平台都另有一個讀磁碟剩下多少空間的函式，給 HTTP cache 算它的大小），裡面沒有對訊息內容的判斷。

## 信任誰

- **host 不信任 child**（Browser 對 renderer）：child 送的每個位元組、在共用記憶體裡寫的每個位元組都可能是惡意的。
- **child 信任 host**：host 啟動它、決定它的環境，本來就能對它做任何事。child 這一側有幾處因此不檢查，列在「不變式」的 I5。
- **同一個使用者的其他 process**：不是通道的對端，但碰得到有名字的東西（macOS 的 bootstrap 名字、Windows 的 pipe 名字）和同一個使用者擁有的物件。靠的是下面的假設。renderer 在作業系統的 sandbox 裡時碰不到這些（那是 `sunaba` 和 renderer 的 policy 的事）；這裡守的是沒有 sandbox 的 process。

## 要保證的性質

| | 性質 | 由哪裡守住 | 怎麼驗證 |
| --- | --- | --- | --- |
| G1 | **記憶體**：不管對方在共用記憶體裡寫什麼、在訊息裡給什麼編號和長度，這一端讀寫的都在它自己 map 的那一塊裡面 | `core/region.rs`：訊息提到的編號和長度由 `Table::named` 檢查（超過是 `OutsideRegion`、不在表裡是 `UnknownRegion`），`Table::read` 和訊息帶來的 `SharedBuffer` 都經過它；之後從 `SharedBuffer` 讀的每一段（`copy_out`、`chunks_out`）再和訊息說的長度比一次。`Pool::put` 寫的和 `Pool::take` 交出去的那一塊由 `core/fit.rs` 的 `best_fit` 選，夠長是證明過的。mapping 不會比物件長：I2 和假設 K1 | `core/region.rs` 的單元測試；`crates/ipc/tests/it/shared_buffer.rs`；`region` fuzzer（對著一個簡單的模型，開 ASan）；核心在 Miri 底下；`fit.rs` 的 Verus 證明；三個平台各自的 `no_more_than_a_region_has_is_mapped` |
| G2 | **壞的輸入**：從通道讀到的任何位元組，結果只有兩種：照規則處理，或當成壞訊息關掉通道。不 panic、不卡住、不 stack overflow，用掉的記憶體有上限 | `core/frame.rs`：長度上限（`MAX_PAYLOAD_LEN`）、照實際收到的量配置、序號、handle 的個數；`codec.rs`：巢狀深度上限（`MAX_DEPTH`）；`core/channel.rs`：目的地要認得；`endpoint.rs`：上層解不出來就是壞訊息 | `frame`、`endpoint` 兩個 fuzzer（任何輸入都不 panic，怎麼切結果都一樣）；`crates/ipc/tests/it/codec.rs`；`crates/ipc/tests/it/endpoint.rs` 裡亂送的對端；跨 process 的 `what_a_child_may_not_send_closes_its_channel` |
| G3 | **handle**：沒有權限的一端送的訊息，不會讓收的一端留下任何 handle；有權限的一端送的，每個 handle 最多變成值裡的一個檔案，沒被拿走的跟著訊息關掉 | `core/frame.rs` 的 `Decoder`：對方不能附時，有 handle 到了就是 `HandlesNotAllowed`，到了的全部丟掉；個數超過是 `TooManyHandles`。交給核心的 handle 一定是 kernel 搬進來的：I4。給上層的訊息的 handle 由 `endpoint.rs` 照值說的第幾個交出去，交過的就不在了，再提到或提到沒有的是 `UnknownHandle`；不是檔案的是 `Rejected`（macOS 上 memory entry 和 fileport 互相不認：`a_regions_handle_is_no_file_and_a_files_port_no_region`） | `frame` fuzzer（沒有權限的一端留不下 handle，留著的不比到了的多）；跨 process 的 `what_a_child_may_not_send_closes_its_channel`；macOS 的 `a_receive_right_isnt_a_handle`；Windows 的 `a_child_gives_no_handle_and_a_host_takes_no_value_for_one`；值裡的檔案：`crates/ipc/tests/it/file_in_message.rs` |
| G4 | **region 的歸屬**：一塊 region 只在建它時指定的那條通道的表裡；收回之後不在任何表裡 | 池子和表是每個端點自己的（`endpoint.rs`），兩個方向的 region 都由能附 handle 的一端編號；對方提到沒給過它的、還沒寫過的、還兩次的、這一端拿去寫還沒送出去的（I10）都是壞訊息（`UnknownRegion`、`RegionNotOut`）。一個 Browser 對好幾個 renderer 時「不交給別的 renderer」由上層守，`ipc` 這一層只有一條通道 | `crates/ipc/tla/Regions.tla`；`region` fuzzer；`crates/ipc/tests/it/endpoint.rs` |
| G5 | **不被卡住**：送訊息不等對方；對方不讀時，這一端的佇列到上限就關掉通道 | `core/channel.rs`：`send` 不等，收不下的排隊，超過 `MAX_QUEUED_BYTES` 是 `Gone::QueueFull`；平台的 `write` 不等（I6） | `core/channel.rs` 的單元測試；loom、shuttle；跨 process 的 `a_child_that_isnt_reading_delays_nothing_for_good` |
| G6 | **順序**：一條通道上，收的一端交出訊息的順序就是送的一端送的順序，不重複、不漏 | `core/channel.rs`：一把鎖守著編號和寫出去的順序；`core/frame.rs`：序號不連續是 `OutOfOrder` | loom（每一種交錯）、shuttle；`crates/ipc/tla/Channel.tla`；`frame` fuzzer |
| G7 | **斷線**：對方不在了（結束、當掉、只關掉一半、送了壞訊息）時，這一端剛好報告一次，之後不再交出它的訊息 | `core/channel.rs` 的 `Receiver`：理由是第一個發生的，只報告一次；平台說對方不在了靠假設 K4 | `crates/ipc/tla/Channel.tla`；loom、shuttle；跨 process 的 `a_child_that_exits_or_is_killed_is_heard_by_its_host`、`a_host_that_closes_is_heard_by_its_child`、`a_child_gone_before_it_took_its_end_is_heard_once_the_host_says_so` |
| G8 | **同步的呼叫**：回答只到等它的那個呼叫；通道斷了時等的一方醒來、當成失敗 | 不在 `ipc` 裡：由用它的那一層守 | 那一層的測試 |
| G9 | **data pipe**：讀到的位元組就是寫的、照順序、不重複；送出去、對方還沒說讀了的量不超過管道的容量；對方說讀了的比送的多是壞訊息 | `core/credit.rs`（算術是證明過的）、`core/pipe.rs`（`PipeOverRead`、`PipeOverrun`） | `credit.rs` 的 Verus 證明；`crates/ipc/tla/Pipe.tla`；`pipe`、`endpoint` fuzzer；`crates/ipc/tests/it/endpoint.rs`；管道放在訊息的值裡過去的那一段：`crates/ipc/tests/it/pipe_in_message.rs` |

## 假設

kernel 和標準庫的行為，`ipc` 證明不了。每一條有測試在那個平台上守著：在某個版本上不成立時，那個測試會失敗。標「實測」的是文件沒有寫明、在開發機的版本上試出來的，換版本時最先要重看。

| | 假設 | macOS | Linux | Windows |
| --- | --- | --- | --- | --- |
| K1 | **共用記憶體的物件建好之後長度不變**，拿著 handle 的人改不了：mapping 不會被抽掉 | memory entry 的大小是建立時定的 | memfd 封了 `F_SEAL_SHRINK`、`F_SEAL_GROW`，而且不能再加 seal；`map` 只收封了「不能縮短」的。測試：`a_region_keeps_its_length_and_its_seals`、`what_isnt_a_region_isnt_mapped` | page file 的 section 不能改大小 |
| K2 | **只能讀的 handle 變不成可寫的**（三個平台跨 process 的測試：`a_child_cant_map_to_write_what_the_host_sends_it`） | 沒有寫入權限的 memory entry，`mach_vm_map` 要可寫會失敗。測試：`a_handle_to_read_cant_be_mapped_to_write`、`a_mapping_to_read_stays_one` | `F_SEAL_FUTURE_WRITE` 是檔案的，不是 fd 的：經過 `/proc` 重開、`mprotect` 都不行（Linux 5.1 起）。測試：`a_handle_to_read_cant_be_written_however_it_is_held`、`the_peers_mapping_to_read_stays_one` | handle 只有 `FILE_MAP_READ`，而 section 的清單不准任何人重開、也不准擁有者改清單（實測，Windows 11 10.0.26200）。測試：`a_handle_to_read_cant_be_mapped_to_write`、`a_handle_to_read_cant_be_made_one_to_write` |
| K3 | **收到的 handle 是 kernel 搬進來的**，對方編不出一個指向這個 process 自己的東西的 handle | right 只經過訊息的 descriptor 進來。測試：`rights_cross_in_order_and_are_the_receivers`、`a_message_laid_out_otherwise_is_refused_with_what_it_brought` | fd 只經過 `SCM_RIGHTS` 進來；放不下時 kernel 會說（`MSG_CTRUNC`）。測試：`handles_go_with_their_message_and_leave_the_writers_hands`、`more_handles_than_a_message_brings_are_the_peer_gone` | 不成立：handle 是數值。所以只有 host 送、child 收（I4、I5） |
| K4 | **對方不在了一定會知道**，包括它當掉、被 kill | dead-name 和 no-senders 的通知，任一個到了就算。測試：`the_peers_port_dying_alone_is_the_peer_gone`、`the_last_right_to_this_port_going_alone_is_the_peer_gone`。child 還沒拿它那一端就結束時兩個都不來，要 host 說（`child_gone`） | 對方最後一個 fd 關掉時讀到串流的結尾；host 啟動 child 之後就關掉自己手上 child 的那一端。測試：`a_peer_gone_is_told_after_what_it_sent` | child 結束時 kernel 關掉它的那一端，host 的讀失敗；host 啟動完就關掉自己的 client 那一端。測試：`an_end_dropped_is_an_end_gone` |
| K5 | **通道的另一端只有對方**：別的 process 讀寫不了這條通道 | 訊息的來源看 kernel 寫的 audit token，整個比對；通知只認 kernel 送的。測試：`messages_from_a_process_that_isnt_the_peer_are_nothing`、`a_notification_the_kernel_didnt_send_is_nothing`。child 的 port 經過 rendezvous 拿，server 照 audit token 認人、每個 process 只答一次（`crates/ipc/tests/it/rendezvous.rs`） | `socketpair` 沒有名字；child 那一端的 close-on-exec 只在那個 child 裡、fork 之後拿掉。測試：`a_child_has_its_end_and_nothing_else_of_its_hosts` | pipe 的名字第一次用、只有一個 instance、建好時 client 已經連著，第二個連不上。測試：`a_pipes_ends_are_these_two_alone`。啟動 child 的那一小段有缺口（`docs/limitations.md` 的「平台和建置」） |
| K6 | **送失敗的訊息不留下東西** | 送不出去的訊息被 kernel 退回成收到的樣子，`mach_msg_destroy` 放掉的剛好是多出來的（實測，macOS 26）。測試：`a_send_that_finds_the_queue_full_leaves_everything_as_it_was` | `sendmsg` 被拒絕時 fd 還是送的一方的 | 寫入失敗時，已經複製進 child 的 handle 由 host 在 child 裡關掉。測試：`a_message_that_doesnt_go_leaves_no_handle_in_the_child` |
| K7 | **out-of-line 收到的記憶體是這個 process 自己的一份**（只有 macOS 用） | 送的一方那幾頁的 copy-on-write，之後它再改也看不到。測試：`what_is_sent_out_of_line_is_the_bytes_as_they_were_when_sent` | | |
| K8 | **進行中的 I/O 的規則**（只有 Windows 用） | | | 在 completion port 上的 handle，發出操作的 thread 結束時操作不會被取消（`a_write_outlives_the_thread_that_started_it`）；每筆進行中的操作恰好留下一個通知，被取消的也是；設了 `FILE_SKIP_COMPLETION_PORT_ON_SUCCESS` 之後當場完成的不留（`what_goes_on_holds_its_end_until_its_completion_is_taken`、`what_is_done_at_once_holds_nothing`） |
| K9 | **`std::env::remove_var` 在 Windows 上多條 thread 也安全**（標準庫的文件寫明） | | 不成立，所以 Linux 的 child 不拿掉變數 | `inherited` 靠它 |
| K11 | **map 著的檔案的內容不變**：沒有人寫它、截短它（三個平台交檔案） | 假設，沒有東西擋：檔案沒有不讓別人寫的開法。交的是系統的字型，系統的是 root 的、使用者自己裝的是使用者的；renderer 自己 map 系統字型時（fontique）也是這個前提 | 同 macOS：沒有強制的檔案鎖（`flock`、lease 都只是約定或通知），交的是系統的字型 | `ReadOnlyFile::open` 開檔時不讓別人寫（只允許讀和刪除），這個限制在檔案的任何一個 handle 還開著時都在（兩端的 handle 是同一個檔案物件），`MappedFile` 一直拿著它的 handle；map 著的檔案不能截短（文件寫明）。測試：`nobody_writes_an_open_file_but_it_may_be_renamed`、跨 process 的 `a_child_maps_the_files_its_host_sends_as_its_own`（host 放掉自己的之後，child 的還擋著） |
| K10 | **job 最後一個 handle 關掉時，kernel 結束裡面的 process**，而一個 process 怎麼結束，它的 handle 都會被關掉（只有 Windows 用） | | | 設了 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`（文件寫明）。測試：`children_share_a_job_and_end_when_it_closes`、`a_hosts_children_go_with_it_whatever_they_are_doing`。正在結束的 process 放不進 job，而那時它還不算結束（實測，Windows 11 10.0.26300：最久差 16 ms），所以被拒絕時最多等 1 秒看它是不是正在結束。測試：`a_child_that_is_exiting_needs_no_job` |

macOS 那一層的測試在 `src/platform/macos/` 各檔的單元測試和 `crates/ipc/tests/it/mach_channel.rs`；Linux 的在 `src/platform/linux/` 和 `crates/ipc/tests/it/linux_channel.rs`；Windows 的在 `src/platform/windows/` 和 `crates/ipc/tests/it/windows_channel.rs`；三個平台共用的跨 process 測試是 `crates/ipc/tests/it/process_channel.rs`。

## 不變式

`ipc` 自己要維持的，改程式時不能破壞。

| | 不變式 | 怎麼守 |
| --- | --- | --- |
| I1 | **核心不做系統呼叫**，也不依賴平台的 crate | `python3 scripts/sanity.py deps` |
| I2 | **`Memory` 的實作**：值活著的期間 `ptr()` 起 `len()` 個位元組可讀（放進 `Pool` 的還要可寫）、不搬動、不比底下的物件長 | `unsafe trait`：每個實作有自己的 `// SAFETY:`（下面的表）；平台的 `map` 在 map 之前確認長度（Linux 先看 seal 再量；macOS 和 Windows 由 kernel 拒絕） |
| I3 | **對方可能在寫的共用記憶體只被複製出來**，不當成有不變量的型別、不借成 `&[u8]` 或 `&T` 交出去 | 對方寫的 region（表裡的）只有三個讀的地方，都在 `core/region.rs`：`Table::read`（整段複製到 `Vec`）、`copy_out`（複製到呼叫的人給的 slice）、`chunks_out`（一段一段以值交出去，給只能寫、拿不到 slice 的目的地）。`SharedBuffer` 拿著 region 時只提供後兩種，`as_bytes` 回 `None`。複製出來之後才解碼。審查；`crates/ipc/tests/it/shared_buffer.rs` |
| I4 | **交給核心的 handle 是 kernel 為這則訊息搬進來的**，不是對方寫的數值 | macOS、Linux：假設 K3。Windows：child 寫的位元組沒有這一層的表頭，host 那一端沒有把數值包成 handle 的程式，所以對方不能附 handle 的一端看不到任何數值（`a_child_gives_no_handle_and_a_host_takes_no_value_for_one`） |
| I5 | **child 信任 host 的地方只有這幾處** | Windows 的 child 把 host 寫的數值包成 handle（`End::next`）、不檢查收到的 section 是哪一種；三個平台的 child 照環境變數或 rendezvous 拿它那一端；被 pipe 驅動的 process 收下 3、4 或命令列指名的 handle（`inherited_pipe`）；child 把 host 給它寫的 region 借成 slice 來畫（I10），相信 host 不寫那一塊；child 相信 host 給的檔案是 `ReadOnlyFile::open` 開的（K11 在 Windows 上靠開檔的方式），Windows 和 Linux 的 child 把 host 說是檔案的 handle 當成檔案（Linux 上 region 的那一邊不信：`map` 只收封住不能縮短的 memfd）。新增一處要寫在這裡 |
| I6 | **平台的函式除了 `wait` 都不等**（啟動 child 的 `spawn` 不算：不在核心的鎖底下呼叫，Windows 上 child 放不進 job 時最多等它 1 秒） | 審查；`core/channel.rs` 在自己的鎖底下呼叫 `write` |
| I7 | **一條通道只有一條 thread 讀**（IO thread） | `Endpoint` 交給迴圈時是搬進去的，只能交一次；Linux 的 waiter 拒絕已經被別的 waiter 等著的一端（`an_end_has_one_waiter`），Windows 的 pipe 只上得了一個 completion port |
| I8 | **Windows 上進行中的操作讓它的通道活著**，直到它的通知從 port 取走：那之前 kernel 隨時會寫它的 `OVERLAPPED` 和緩衝區 | `hold`、`let_go` 和 `wait` 裡的 `Arc::from_raw` 成對（下面的表）。waiter 丟掉時還在進行的不釋放：刻意洩漏 |
| I10 | **這一端寫的 region 只在一個時候是 slice：被拿出池子（`Pool::take`）、還沒交給訊息（`Pool::lend`）的期間，而且只給拿的那一個人**（`shared_buffer::Writer`） | 那段期間池子記著它被拿著（`held`）：不會再被 `take` 或 `put` 選到，對方說「讀完了」是壞訊息（`RegionNotOut`，不然下一個拿的人會寫同一塊）。`Writer` 變成 `SharedBuffer` 時被消耗掉，之後沒有東西能寫它；`SharedBuffer` 還給池子只有一次（訊息沒送出去時由送的那一步還，`sent` 旗標讓它自己 drop 時不再還）。對方那一側的 mapping 不寫這一塊：Browser 寫的 region，renderer 拿到的是唯讀的（K1）；renderer 寫的 region，Browser 的程式只讀（I5 的方向相反：這裡是 child 信任 host）。`core/region.rs` 和 `crates/ipc/tests/it/shared_buffer.rs` 的測試（`a_region_its_taker_still_writes_isnt_the_peers_to_free`）；Miri；審查 |
| I11 | **值裡的 region 和管道一樣，只在同一條 thread 的同一次序列化或解碼裡交接**：`SharedBuffer` 把它的編號經過一個 thread-local 交給 codec，codec 把訊息提到的 region 經過另一個交給值 | codec 不是這條通道的（別的序列化器、別的通道）時編號留在原地，`SharedBuffer` 的 region 還是它的；解碼時交出之後再清一次。記憶體安全不靠這一條（能寫 region 的 `Writer` 在這之前已經沒有了）：放錯的後果是一塊 region 不被還回來，或訊息被拒絕。`crates/ipc/tests/it/shared_buffer.rs` 的 `a_region_crosses_once_and_only_its_own_channel` |
| I12 | **值裡的檔案和管道一樣，只在同一條 thread 的同一次序列化或解碼裡交接**：`ReadOnlyFile` 把另一個 handle 經過一個 thread-local 交給 codec，codec 把訊息的 handle 經過同一個交給值 | `ReadOnlyFile` 的 `Serialize` 之後不管 codec 拿走了沒有都清掉，解碼時交出之後再清一次，所以別的序列化器碰不到上一次留下的；值自己的 handle 不交出去（送的是另一個）。`crates/ipc/tests/it/file_in_message.rs` 的 `only_the_codec_sends_a_file_and_only_to_an_end_that_attaches`。放錯的後果是一個檔案被當成別的值的，不是記憶體錯誤 |
| I9 | **值裡的管道只在同一條 thread 的同一次序列化或解碼裡交接**：`Consumer` 經過一個 thread-local 交給 codec（或從 codec 交給值），交接的兩邊一定把它拿走或還回去，不留給下一個值 | `data_pipe` 的 `Serialize`（codec 沒拿走就還給值）、codec 解碼時交出之後再清一次；`crates/ipc/tests/it/pipe_in_message.rs` 的「只有 codec 序列化」「冒用名字的型別」兩個測試。沒有 `unsafe`：放錯地方的後果是一條管道提早結束，不是記憶體錯誤 |

## 每一處 `unsafe`

每一處在程式裡都有 `// SAFETY:`；這裡是同樣的論證，集中在一處方便審查。新增或改動一處時兩邊一起改。

### 核心和 `TimeTicks`

| 地方 | 為什麼安全 |
| --- | --- |
| `core/region.rs` 的 `unsafe trait Memory` | 實作的人保證：值活著的期間 `ptr()` 起 `len()` 個位元組可讀（放進 `Pool` 的還要可寫），兩者不變；這個 process 裡沒有別的參照指進那一段。`Arc<M>` 的實作只是轉給裡面的值，`Arc` 讓它活著、不搬動 |
| `core/region.rs` 的 `Pool::put`（複製進 region） | 目的地是 `best_fit` 選的那一塊，它的長度至少是要寫的長度（Verus 證明的後置條件），`Memory` 保證那一段可寫；來源是這個 process 自己的 slice，和 region 不重疊 |
| `core/region.rs` 的 `copy_out`（從 region 複製到呼叫的人的 slice） | 位置加長度先和記憶體的長度比過（溢位或超過就不複製）；目的地是呼叫的人自己的 `&mut [u8]`，和 region 不重疊。對方可能同時在寫：同 `Table::read` |
| `core/region.rs` 的 `chunks_out`、`ChunksOut::next`（一段一段以值讀出來） | 建的時候確認「位置加段數乘每段的長度」在記憶體裡面（乘和加都檢查溢位），之後每拿一段就少算一段，所以每次讀的 `N` 個位元組都在裡面；借著記憶體的生命週期，讀的期間 mapping 還在。讀成 `[u8; N]`：位元組不用對齊、任何內容都合法。對方可能同時在寫：同 `Table::read` |
| `core/region.rs` 的 `bytes_mut`、`bytes`，和 `shared_buffer.rs` 的 `Writer::bytes_mut`、`Writer::bytes` | I10：region 是池子裡的（可寫），長度在拿的時候和它比過；被拿著、還沒交給訊息的期間這個 process 裡只有這個 `Writer` 碰得到它，`&mut self` 和 `&self` 讓同時只有一個可寫的 slice、或幾個只讀的；對方的 mapping 不寫它。mapping 的內容是 0 或之前寫的，都是初始化過的位元組。`Writer` 自己拿著 mapping 的 `Arc`，通道結束之後 slice 還是有效 |
| `core/region.rs` 的 `Table::read`（從 region 複製出來、`set_len`） | 長度先和 region 的長度比過（`Table::named`，超過的是 `OutsideRegion`）；目的地是剛 `reserve` 過的多餘容量，`set_len` 只涵蓋剛寫的那幾個位元組。對方可能同時在寫那一塊：讀到的是位元組，不當成有不變量的型別，複製出來之後才交給上層 |
| `core/region.rs` 的 `HeapMemory`（`Box::into_raw`、`from_raw`、`Send`、`Sync`） | 指標來自 `new` 裡的 `Box::into_raw`，只在 `drop` 還回去一次；配置是這個值自己的，經過 `Memory` 的讀寫都是整段位元組的複製，兩端同時複製和跨 process 的 mapping 是同一種情況 |
| `time_ticks.rs` 的 `QueryPerformanceFrequency`、`QueryPerformanceCounter`（只在 Windows） | 傳進去的指標指向一個活著的 `i64`，呼叫只寫它、不留著它；Windows XP 以後這兩個呼叫不會失敗。頻率讀成 0 時當成 1，不會除以 0 |

### 跟著訊息的檔案（`file.rs`）

| 地方 | 為什麼安全 |
| --- | --- |
| `file.rs` 的 `ReadOnlyFile::map`（`memmap2::Mmap::map`） | 只 map 成唯讀，`MappedFile` 拿著檔案，mapping 活多久檔案就開多久，借出去的 `&[u8]` 活不過 `MappedFile`。memmap2 要求的是 map 著的期間檔案不被別人寫或截短：Windows 上 K11；macOS 和 Linux 上是假設（K11：交的是沒有人寫的系統字型）。長度 0 的檔案是空的 mapping（memmap2 不呼叫系統） |

### macOS（`platform/macos/`）

| 地方 | 為什麼安全 |
| --- | --- |
| `platform/macos/memory.rs` 的 `mach_make_memory_entry_64`（`entry`） | 傳進去的是活到呼叫結束的兩個區域變數的指標（大小、回來的名字）；`from` 是這個 task 自己的 mapping 的位址或沒有。回 `KERN_SUCCESS` 才把名字當成 right（`SendRight::from_raw`：新的 send right，只有這裡拿著） |
| `platform/macos/memory.rs` 的 `mach_vm_map`（`map`） | 位址由 kernel 選（`VM_FLAGS_ANYWHERE`），寫進活到呼叫結束的區域變數；不蓋掉這個 task 已有的東西。成功才建 `Mapping` |
| `platform/macos/memory.rs` 的 `SendRight`（`from_raw`、`Drop` 的 `mach_port_deallocate`） | `from_raw` 是 `unsafe fn`：呼叫的人保證名字是這個 task 的一個 send right、沒有別人會放掉它。`Drop` 放掉一次；`into_raw` 用 `forget` 把放掉的責任交出去 |
| `platform/macos/memory.rs` 的 `Mapping`（`Drop` 的 `mach_vm_deallocate`、`Send`、`Sync`、`Memory`） | 範圍是 `map` 得到的那一段，只在 `drop` 解除一次。位址在每條 thread 上都一樣；經過 `Memory` 的讀寫都是整段位元組的複製。`Memory` 的保證：從 `map` 到 `drop` 都 map 著、不搬動、可讀；放進 `Pool` 的是 map 成可寫的（`create(_, false)` 的那一個、`map(_, _, true)` 的）。指標用 `with_exposed_provenance_mut` 從位址做出來：記憶體不是 Rust 配置的 |
| `platform/macos/port.rs` 的三種 right（`from_raw`、`Drop`、`ReceiveRight::new`、`make_send`、`tell_when_no_senders`） | `from_raw` 都是 `unsafe fn`：呼叫的人保證名字是這個 task 的那一種 right、沒有別人會放掉。每種的 `Drop` 用對應的呼叫放掉一次（send 和 send-once 是 `mach_port_deallocate`，receive 是 `mach_port_mod_refs` 減一）；`into_raw` 用 `forget` 交出責任。其餘的呼叫都是對自己的 receive right 做的，寫的是活到呼叫結束的區域變數。`mach_port_request_notification` 是自己宣告的（mach2 沒有），照 `<mach/mach_port.h>` 的原型 |
| `platform/macos/message.rs` 的 `Buffer::bytes`、`bytes_mut` | 把 `Vec<u32>` 看成位元組：字都初始化過，`u32` 的位元組就是位元組、任何位元組也都是合法的 `u32`，長度是 vector 的位元組數 |
| `platform/macos/message.rs` 的 `mach_msg`（送） | 緩衝區裡是一則完整的訊息（大小就是 header 寫的那個、在 vector 的範圍內），對齊和 header 一樣；它提到的 right 是呼叫的人交出來的（`into_raw` 之後才寫進去） |
| `platform/macos/message.rs` 的 `mach_msg_destroy`（送失敗、`discard`、收到排法不對的） | 訊息沒送出去時，它要搬的 right 還是這個 task 的，各放掉一次；只是複製的那個（`To::Port`）kernel 不會動。收到的訊息是 kernel 剛寫進緩衝區的完整的一則，`parse` 失敗時還沒有任何 right 被包成型別，所以每個只放掉一次 |
| `platform/macos/message.rs` 的 `mach_msg`（收）、`Parsed::own` | 緩衝區是 `capacity` 位元組，kernel 不會寫超過。`own` 只在整則訊息 `parse` 通過之後才把名字包成 right：receive right 是訊息搬給這個 task 的，其餘的是 send right（`parse` 不放行別種），reply 是 send-once（`parse` 檢查過） |
| `platform/macos/message.rs` 的 `audit_token_to_pid`（libbsm） | 傳值進去，讀其中一個字 |
| `platform/macos/message.rs` 的 `try_send`（`mach_msg` 和失敗時的 `mach_msg_destroy`） | 緩衝區裡是一則完整的訊息；right 是複製的（呼叫的人的不動），out-of-line 的記憶體是 `body`，活到呼叫結束，kernel 做 copy-on-write、不從這個 task 解除。送失敗時 kernel 把訊息退回成收到的樣子（每個 right 多一個計數、記憶體重新 map 一份、descriptor 改成要解除）：`mach_msg_destroy` 放掉的剛好是這些。這個行為是實測的（macOS 26），有測試守著：佇列滿時連送 64 次，位址空間和 right 的計數都不變 |
| `platform/macos/message.rs` 的 `Buffer::body`（out-of-line 的那一支）、`Mapped` 的 `Drop` | kernel 為這一則訊息 map 進來的 `len` 位元組，是送的一方那幾頁的 copy-on-write 的一份，只有這個 task 碰得到，所以可以當成 `&[u8]`（和共用的 region 不同）；借出去的 slice 活不過 `Received`，`Received` drop 時解除一次 |
| `platform/macos/port.rs` 的 `request`、`PortSet`、`raise_queue_limit`、`dead_name_told` | 都是對這個 task 自己拿著的名字做的呼叫，讀寫的是活到呼叫結束的區域變數。`mach_port_insert_member`、`mach_port_set_attributes` 是自己宣告的（mach2 沒有），照 `<mach/mach_port.h>` 的原型。`dead_name_told` 放掉的是通知多給的那一個計數，值自己的那一個留著 |
| `platform/macos/fileport.rs` 的 `fileport_makeport`、`fileport_makefd`、`File::from_raw_fd` | 兩個都是 kernel 的私有 API（`<sys/fileport.h>` 不在 SDK 裡，mach2 沒有），照 XNU 的原型自己宣告；Chromium 的 Mojo 和 XPC 也用它們交 descriptor。`fileport_makeport` 讀的是呼叫期間開著的 descriptor，port 的名字寫進活到呼叫結束的區域變數；回 0、名字不是 null 才包成 right（新的 send right，只有這裡拿著）。port 自己拿著那個開著的檔案，descriptor 關掉也不影響。`fileport_makefd` 對這個 task 拿著的 right 做，成功時給一個新的 descriptor（kernel 設好 close-on-exec，測試 `a_files_port_makes_a_file_open_as_it_was_and_closed_on_exec`），只有這次呼叫拿到，才包成 `File`；不是 fileport 的 right 回 -1 |
| `platform/macos/rendezvous.rs` 的 `bootstrap_check_in`、`bootstrap_look_up` | 傳進去的是活到呼叫結束的 C 字串和一個放名字的區域變數。成功才把名字包成 right：check-in 給的是 service 的 receive right，look-up 給的是 send right |

### Linux（`platform/linux/`）

| 地方 | 為什麼安全 |
| --- | --- |
| `platform/linux/channel.rs` 的 `spawn`（`pre_exec` 和裡面的 `BorrowedFd::borrow_raw`） | closure 在 child 裡、fork 和 exec 之間跑，那裡只能做 async-signal-safe 的事（fork 的當下別的 thread 可能拿著任何鎖）：它只做一次 `fcntl` 的系統呼叫，不配置記憶體（錯誤只帶一個號碼）。那個號碼在 child 裡是開著的：`theirs` 在 host 活到 `spawn` 回來之後，fork 時還在。借用只到那一次呼叫結束 |
| `platform/linux/channel.rs` 的 `inherited`（`BorrowedFd::borrow_raw`、`OwnedFd::from_raw_fd`） | 先借來問它是什麼（號碼沒開著的話兩個呼叫都是錯誤）；確認是 Unix stream socket、不是 0 到 2，才收下。號碼是 `spawn` 寫的、替這個 process 留著開的，這個 process 裡沒有別人拿過它，而且只收一次（`TAKEN`），所以只有這個值會關它。環境變數被改成指向別的 Unix stream socket 時，收下的是那一條：做得到這件事的人本來就決定這個 process 怎麼啟動 |
| `platform/linux/memory.rs` 的 `Mapping`（`Memory`） | 裡面是 memmap2 的 `MmapRaw`：從 `map` 到 drop 都 map 著、不搬動、可讀；放進 `Pool` 的是 map 成可寫的（`create(_, false)` 的那一個、`map(_, _, true)` 的）。範圍一直在檔案裡面：`map` 只接受封住「不能縮短」的檔案，而且在看到 seal 之後才量長度；`create` 自己建的在 map 之前就封了 |

### macOS 和 Linux 共用的（`platform/unix.rs`）

| 地方 | 為什麼安全 |
| --- | --- |
| `platform/unix.rs` 的 `install`（`libc::sigaction`、`sigemptyset`） | 兩個結構都是清成 0 的區域變數（全 0 是合法的 `sigaction`：沒有旗標、空的 mask、預設的動作），活到呼叫結束；`sigaction` 只讀一個、寫另一個。裝上的 `handle` 是 `extern "C" fn(c_int)`，沒有 `SA_SIGINFO` 時 `sa_sigaction` 要的就是這種，而且它只做 handler 能做的事（下一列） |
| `platform/unix.rs` 的 `handle`（`libc::write`、讀寫 `errno`）、`errno_location`（`__errno_location`、`__error`） | handler 在任何一條 thread、任何兩個指令之間跑：`write` 和讀寫 `errno` 是 POSIX 列為 async-signal-safe 的，不配置、不拿鎖。socket 在 `install` 之前就設好、之後一直開著（`into_raw_fd`，沒有人關它），nonblocking，所以 handler 不會等；寫的 byte 是活到 `write` 回來的區域變數。`errno` 是這條 thread 的，寫完放回原本的值，被打斷的程式之後讀得到它自己的。`errno` 的位址由 C library 自己的 accessor 給，不帶參數、一直可以呼叫，在 thread 活著時有效 |
| `platform/unix.rs` 的 `terminate_current_process_immediately`（`libc::_exit`） | `_exit` 接受任何代碼、不回來，不讀也不寫這個 process 的記憶體：它就是結束 process 的系統呼叫，kernel 停掉每一條 thread、關掉 process 開著的東西。呼叫之後這個 process 沒有程式再跑，所以沒有東西會看到沒被 drop 的值。不跑 exit handler 和 destructor 正是用它的理由（其他 thread 還在跑），不是安全上的問題：留下的只是沒寫出去的緩衝區 |

### 被 pipe 驅動的 process 的兩端（`platform/pipe.rs`）

| 地方 | 為什麼安全 |
| --- | --- |
| macOS 和 Linux 的 `check`（`BorrowedFd::borrow_raw`） | 借來只做一次 `fstat`，問它是什麼；號碼沒開著時那個呼叫是錯誤（`EBADF`）。借用只到那一次呼叫結束。和 Linux 的通道的 `inherited` 同一個做法 |
| macOS 和 Linux 的 `inherited_pipe`（`OwnedFd::from_raw_fd`） | 3 和 4 剛確認開著、是 pipe 或 socket；它們是啟動這個 process 的人為這件事留著開的，這個 process 裡沒有別人拿過它們（呼叫的人在開任何東西之前先拿）。一個 process 只問一次（`TAKEN`，在看之前就設），所以只有這兩個值會關它們。啟動的人在 3、4 放了別的 pipe 時，收下的是那兩個：能這樣做的人本來就決定這個 process 怎麼啟動（I5） |
| macOS 和 Linux 的 `spawn_with_pipe`（`pre_exec` 和裡面的 `libc::dup2`） | closure 在 child 裡、fork 和 exec 之間跑，只能做 async-signal-safe 的事：它只做兩次 `dup2` 的系統呼叫，不配置記憶體（錯誤只帶一個號碼）。兩個來源在 child 裡是開著的：擁有它們的值在 host 活到 `spawn` 回來之後，fork 時還在；兩者都在 4 以上（不是的先複製過去），所以不會是對方的目標、`dup2` 也不會因為來源和目標相同而留下 close-on-exec |
| Windows 的 `named`（`GetFileType`） | 只查那個數值，不是 handle 時它自己會說 |
| Windows 的 `inherited_pipe`（`OwnedHandle::from_raw_handle`） | 兩個數值是啟動這個 process 的人（可信，I5）為這件事放進來、在命令列上指名的；剛確認過都是 pipe、不是同一個，而且一個 process 只問一次（`TAKEN`），所以各自只被包一次。命令列被別人改成這個 process 自己的另一個 pipe 時，那個 pipe 會被當成這條、之後被關掉：能改命令列的人本來就是啟動它的人 |

### Windows（`platform/windows/`）

| 地方 | 為什麼安全 |
| --- | --- |
| `platform/windows/memory.rs` 的 `section`（`InitializeAcl`、`AddAccessDeniedAce`、`InitializeSecurityDescriptor`、`SetSecurityDescriptorDacl`、`CreateFileMappingW`） | 每個呼叫寫的都是活到呼叫結束的區域變數：清單是 64 個位元組的陣列（以 `u32` 配置，kernel 要它對齊），放得下表頭和一項；descriptor 指向那個陣列，兩者都活到 `CreateFileMappingW` 回來。沒有檔案、沒有名字。回來的不是 null 才包成 `OwnedHandle`（新的 handle，只有這裡拿著） |
| `platform/windows/memory.rs` 的 `with_rights`（`DuplicateHandle`） | 來源是借來的、呼叫期間開著的 handle，兩個 process 都是自己，結果寫進活到呼叫結束的區域變數。成功才包成 `OwnedHandle` |
| `platform/windows/memory.rs` 的 `MapViewOfFile`（`map`） | 位址由 kernel 選，不蓋掉這個 process 已有的東西；handle 在呼叫期間開著。成功才建 `Mapping` |
| `platform/windows/memory.rs` 的 `Mapping`（`Drop` 的 `UnmapViewOfFile`、`Send`、`Sync`、`Memory`）、`page` 的 `GetSystemInfo` | 和 macOS 的 `Mapping` 同一個論證：範圍是 `map` 得到的那一段，只在 `drop` 解除一次；位址在每條 thread 上都一樣，經過 `Memory` 的讀寫都是整段位元組的複製；放進 `Pool` 的是 map 成可寫的。`GetSystemInfo` 只寫一個全零也合法的區域變數，不會失敗 |
| `platform/windows/channel.rs` 的 `Operation`、`End` 的兩個指標（`Send`、`Sync`、`Drop`） | 每個通道一筆讀、一筆寫的操作，各是自己的一塊配置（`Box::into_raw`），和通道同時建立、同時釋放，位址不變。讀的那一筆只有 IO thread 碰，寫的那一筆只在 `writing` 的鎖底下碰，而且都只在它不在進行中的時候：進行中時 kernel 隨時會寫 `OVERLAPPED`（和讀的緩衝區），這段時間沒有任何參照指著它們。`Drop` 只在計數歸零時跑，而進行中的操作拿著一個計數，所以釋放時沒有操作在進行 |
| `platform/windows/channel.rs` 的 `hold`、`let_go`、`wait` 裡的 `Arc::from_raw` | 發出操作之前多拿一個計數（`Arc::into_raw` 之後不留指標；在呼叫之前拿，因為進行中的操作可能在呼叫回來之前就完成、通知就被取走）。呼叫回來時已經結束的（當場完成，或失敗），不會有通知，當場還回去：pipe 在加入迴圈時設了 `FILE_SKIP_COMPLETION_PORT_ON_SUCCESS`，設不成就加不進去，所以當場完成的一定沒有通知。回 `ERROR_IO_PENDING` 的，恰好一個通知，waiter 取到時用 `Arc::from_raw` 接手那一個計數。指標是操作裡記的 `end`，建立通道時從 `Arc::into_raw` 得到、之後不變。port 上只有這個 waiter 的通道的 pipe，另外只有它自己的打斷會 post、而且不帶 `OVERLAPPED`，所以帶 `OVERLAPPED` 的通知一定是其中一筆操作的，位址就是操作的位址（`repr(C)`，`OVERLAPPED` 是第一個欄位）。`kind` 和 `end` 建立之後不變，讀它們不和 kernel 搶 |
| `platform/windows/channel.rs` 的 `ReadFile`、`WriteFile`（`End::read`、`write`） | pipe 的鎖拿著，handle 在呼叫期間開著；回報位元組數的區域變數活到呼叫結束。緩衝區和 `OVERLAPPED` 是操作自己的；操作進行中時被剛拿的計數留到通知取走，當場結束時呼叫回來之後 kernel 就不再碰它們。讀進去的是緩衝區裡還沒用的那一段；寫出去的是這次組好的那一段，長度在 64 KiB 以內 |
| `platform/windows/channel.rs` 的 `wait` 借出去的位元組（`slice::from_raw_parts`） | 通道的讀的緩衝區裡的一段，`wait` 自己拿著通道的 `Arc`。這時沒有讀在進行，而下一筆讀只有這條 thread 會發、在借出去的這次呼叫回來之後：借出去的期間沒有人寫它 |
| `platform/windows/channel.rs` 的 `End::next` 把數值包成 handle（`OwnedHandle::from_raw_handle`） | 只有 child 那一端做，數值是 host 寫的，host 是可信的：它為這則訊息把 handle 放進這個 process、只說一次。host 寫錯（同一個數值兩次）時 child 會把同一個 handle 關兩次。host 那一端沒有這段程式 |
| `platform/windows/channel.rs` 的 `DuplicateHandle`（`to_duplicate_into`、`give`、`take_back`） | handle 都是借來的、呼叫期間開著的，結果寫進活到呼叫結束的區域變數。`take_back` 關的是 `give` 剛放進 child 的那幾個，child 還沒被告知有它們；沒有目標的 `DUPLICATE_CLOSE_SOURCE` 只在來源的 process 裡關掉 |
| `platform/windows/channel.rs` 的 `inherited`、`end_named`（`std::env::remove_var`、`GetFileType`、`OwnedHandle::from_raw_handle`、`SetHandleInformation`） | `remove_var`：標準庫寫明它在 Windows 上單一和多條 thread 的程式裡都安全（改環境是系統的一個呼叫，別的 thread 可以同時讀）；這個模組只在 Windows 上編。`GetFileType` 只查那個數值，不是 handle 時它自己會說。包成 `OwnedHandle` 的數值是 host（可信）為這件事放進這個 process 的、剛確認過是 pipe，而且 `inherited` 的旗標讓它只被包一次。變數被別人設成這個 process 自己的另一個 pipe 的數值時，那個 pipe 會被當成通道、之後被關掉：能設 child 的環境的人本來就是啟動它的人 |
| `platform/windows/channel.rs` 的 `named_pipe`、`IocpWaiter` 的 port（`CreateNamedPipeW`、`CreateFileW`、`ConnectNamedPipe`、`CreateIoCompletionPort`、`PostQueuedCompletionStatus`、`GetQueuedCompletionStatus`、`CancelIoEx`） | 名字是以 0 結尾、活到呼叫結束的陣列。回來的 handle 檢查過才包成 `OwnedHandle`（新的，只有這裡拿著）。其餘的呼叫都是對自己拿著、呼叫期間開著的 handle 做的，寫的是活到呼叫結束的區域變數 |
| `platform/windows/process.rs` 的 `terminate_current_process_immediately`（`GetCurrentProcess`、`TerminateProcess`） | `GetCurrentProcess` 回來的是不用關、一直開著的 pseudo-handle。`TerminateProcess` 對它不讀也不寫這個 process 的記憶體：kernel 停掉每一條 thread、關掉 process 開著的東西，之後這個 process 沒有程式再跑，所以沒有東西會看到沒被 drop 的值。萬一失敗回來了，`abort` 照樣結束。不跑 DLL 的 detach 和 destructor 正是用它的理由，和 Unix 的 `_exit` 一樣 |
| `platform/windows/disk.rs` 的 `available_disk_space`（`GetDiskFreeSpaceExW`） | 名字是以 0 結尾、活到呼叫結束的陣列，裡面沒有別的 0（有的話不呼叫，是 `InvalidInput`：會變成另一個比較短的路徑）；結果寫進活到呼叫結束的區域變數，另外兩個輸出是 null（文件寫明可以不要）。不拿、不留任何 handle |
| `platform/windows/job.rs`（`CreateJobObjectW`、`OwnedHandle::from_raw_handle`、`SetInformationJobObject`、`AssignProcessToJobObject`、`WaitForSingleObject`） | 建 job 時兩個指標都是 null（沒有名字、不被繼承），回來的不是 null 才包成 `OwnedHandle`（新的，只有這裡拿著）。limits 是活到呼叫結束的區域變數，型別和長度照那個 class 要的。放進 job 和等待用的是借來的、呼叫期間開著的 handle；kernel 自己檢查是不是 process、權限夠不夠 |

## 怎麼驗證的

指令和每一組測試涵蓋什麼在 `crates/ipc/README.md` 的「測試」。

| 方法 | 守什麼 | 什麼時候跑 |
| --- | --- | --- |
| `cargo test -p ipc`：單元測試、惡意的對端、跨 process 的測試、fuzzer 找到過的輸入（`crates/ipc/fuzz/regressions/`） | 全部；每個平台的假設 | 每次；CI 每天在 Linux 上跑，macOS 和 Windows 的平台層只在開發機上跑 |
| fuzz（`python3 scripts/test.py ipc fuzz`：`endpoint`、`frame`、`pipe`、`region` 四個 target，開 ASan） | G1、G2、G3、G9 | 改核心時在本機 |
| Miri（`python3 scripts/test.py ipc miri`：核心的單元測試，和端點經過同一個 process 裡的平台的幾個測試，包括 `shared_buffer` 的） | 核心的 `unsafe`（G1、I3、I10） | 改核心時在本機 |
| loom、shuttle（`python3 scripts/test.py ipc models`） | G5、G6、G7：送出的佇列和它的鎖、通道的結束 | 改到這幾處時在本機 |
| TLA+（`python3 scripts/test.py ipc tla`：三個模型，各自再拿掉每一部分確認找得到反例） | G4、G6、G7、G9 的協定 | 改協定時，模型一起改 |
| Verus（`python3 scripts/test.py ipc verus`） | `core/fit.rs`、`core/credit.rs` 的算術 | 改這兩個檔案時 |
| mutation test（`python3 scripts/test.py mutate`） | 每一個檢查真的有測試守著 | 每一段工作做完時 |
| `python3 scripts/sanity.py unsafe`、`deps` | `unsafe` 不出 `ipc`；I1 | 每一段工作做完時 |

Miri 和 fuzzer 碰不到平台層（系統呼叫）：那一層靠跨 process 的測試、mutation test 和審查。

## 還守不住的

列在 `docs/test-gaps.md` 和 `docs/limitations.md` 的「平台和建置」，這裡只列和論證有關的：

- **共用記憶體被對方同時寫**：這一端用 `ptr::copy_nonoverlapping` 和 `read_unaligned` 讀對方可能正在寫的位元組。Rust 的模型裡這是和另一個 process 的 data race；`ipc` 把它當成「讀到任意位元組」處理（I3），這也是用共用記憶體的程式通常的做法，但不是語言保證的。
- **這一端寫 region 的 slice 靠對方不寫那一塊**（I10）：renderer 這一側靠的是 Browser 的程式不寫它給出去的 region，不是 kernel。同一個 process 裡的平台（`local`，測試用）兩端共用同一塊記憶體：一端太早說「讀完了」而另一端還拿著那塊 region 的 `SharedBuffer` 時，是同一個 process 裡的 data race；只有故意寫成那樣的測試做得到，現有的測試沒有。
- **macOS 和 Windows 的平台層 CI 不跑**，fuzz、Miri、loom、TLA+、Verus 也只在本機跑：改了而沒有跑時沒有東西會發現。
- **K5 對「同一個使用者的其他 process」的部分只靠審查**：Windows 的 pipe 建立時的保護，和 `spawn_with_pipe` 啟動 child 時的繼承（經過標準庫，靠一把鎖）。`spawn` 啟動的 child 只繼承指定的 handle，由 `sunaba` 的 `Command` 保證，有測試。
- **Windows 的 `hold` 和 `let_go` 成對**：弄壞是釋放後被 kernel 寫，測試看不出來，沒有列成 mutant。
- **標「實測」的假設**只在一個版本的系統上試過。
- **K11 只在 Windows 上由 kernel 守住**：macOS 和 Linux 上檔案交過去之後，有寫入權限的人（系統字型是 root 的，使用者自己裝的是使用者的）改了它，map 著的位元組就變了，截短了它的話讀到被截掉的那幾頁時 renderer 當掉。fontique 自己 map 系統字型時也是這個前提。

## 修改時要審查的

- **新增 `unsafe`**：寫 `// SAFETY:`，在上面的表加一列；如果靠 kernel 的行為，加一條假設和守它的測試。
- **改 `core/region.rs` 的存取，或 `shared_buffer.rs`**：I3 還成立嗎（有沒有把對方能寫的位元組借出去）；I10 還成立嗎（`held` 的每一個轉換、誰能把一塊 region 還給池子、`Writer` 以外有沒有東西拿得到可寫的 slice）；長度的檢查在複製之前嗎。跑 `region` fuzzer 和 Miri。
- **改訊息的格式或檢查**（`core/frame.rs`、`core/control.rs`、`codec.rs`）：每一個上限還在嗎；跑 `frame`、`endpoint` fuzzer；協定變了的話改 TLA+ 的模型。
- **改送出的佇列、鎖或通道的結束**（`core/channel.rs`）：跑 loom 和 shuttle；鎖底下不能等（I6）。
- **新的平台，或改平台的 `Platform`、`Waiter`**：對過 K1 到 K6 每一條在那個平台靠什麼、有沒有測試；`crates/ipc/tests/it/process_channel.rs` 不改就要過；handle 如果是數值，照 Windows 的做法守 I4。
- **child 這一側新增一處不檢查 host 的**：寫進 I5。
- **改 `file.rs`**：`open` 在 Windows 上還是不讓別人寫嗎（K11）；`MappedFile` 還拿著檔案嗎；`Serialize` 之後 thread-local 還清掉嗎（I12）。新的平台交檔案時，對過 K11 在那個平台靠什麼。
- **改 `platform/pipe.rs`**：收下的兩端在任何東西啟動之前設成 close-on-exec 或不可繼承嗎（不然 child 的 child 拿著 pipe，client 等不到結尾）；Windows 上設成可繼承的那一段在 `starting_a_child` 的鎖底下嗎；`pre_exec` 裡只有 async-signal-safe 的呼叫嗎。`crates/ipc/tests/it/inherited_pipe.rs` 守這幾條。
- **改 Windows 的 job**：handle 不能被繼承、不能複製給別的 process、host 活著時不能關：前兩種讓 child 在 host 不在之後留著，最後一種把活著的 child 全部結束（K10）。
- **升級作業系統的版本**：重跑那個平台的 `cargo test -p ipc`，標「實測」的假設先看。
