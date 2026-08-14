# dshd 桌面端 Bug 复核与修复方案

> 范围：`apps/desktop`（Tauri 2 shell + Node sidecar）及其与 web 侧（`packages/bundle/web-app`、`packages/client/connection`）的握手合约。
> 方法：静态审查全部 Rust 源码（19 个模块）、前端、打包脚本，并与 `packages/subprocess/subprocess-local`、`packages/shell` 的对应实现交叉验证。每个结论都回读了源码确认，不只依赖第一轮印象。
> 环境说明：本审查环境无法运行 `cargo test`（PATH 无 cargo、沙箱不执行直调二进制），Windows 侧运行验证按 README 约定归 CI 所有；因此每个修复方案都附带了待补的测试清单。

---

## Bug 1（严重）：关停升级只看直接子进程 + reader 无界 join —— 退出时可能永久挂起

### 证据

- `process.rs::shutdown_tree`：升级条件 `if tree.is_alive()`，而 `ChildTree::is_alive` 是 `matches!(self.child.try_wait(), Ok(None))` —— 只观察直接子进程（node）。grace 循环期间 node 一退出循环即停，即使进程组里还有活着的成员，也不会再发 `SIGKILL`。
- Unix `signal_terminate` 用 `killpg(pgid, SIGTERM)`；忽略或阻塞 SIGTERM 的组成员会活下来（进程泄漏），且不再有第二档信号。
- `sidecar.rs::SidecarProcess::shutdown`：`shutdown_tree(...)` 之后 `for reader in readers.drain(..) { let _ = reader.join(); }`。stdout reader 的 `drain_rest`、stderr reader 的 `drain_to_log` 都是**无超时的阻塞读**：只要还有进程持有管道写端，EOF 永远不来，join 永久阻塞。
- 管道写端持有者是真实存在的：`packages/subagent/subagent-acp/src/run.ts`、`packages/subagent/subagent-claude-code/src/process.ts`、`packages/subagent/subagent-codex/src/run.ts` 均以 `stderr: 'inherit'` 生成子进程 —— 这些孙进程继承的正是 shell 给 sidecar 的 stderr 管道。Claude Code / Codex 还会继续生成自己的进程树。
- 更完整的存活路径：subprocess-local 以 `detached: true` 生成树（自成进程组，不在 `killpg(pgid, …)` 射程内），靠 sidecar 自己的 shutdown（含 host-exit 强制终结）清理。node 在 5s grace 内没完成清理而被 SIGKILL 时，这些树带着继承的 stderr 写端存活。
- 阻塞链：托盘 Quit / `RunEvent::Exit` → `AppState::request_stop` → `supervisor.request_stop` → `process.shutdown(5s)` → `reader.join()` 永不返回 → `join_boot()` 挂起 → 窗口已关但进程永不退出。cmd+Q、`install` 的 orphan 分支（`stopping` 已置位时）同样受影响。Windows 上 job 分配失败（`job: None`）的降级路径同病。

### 为什么不会被高估

即使所有同组孙进程都死在组信号下，`setsid` 逃逸者仍可持有管道写端；且"node 收到 TERM 但关停超过 grace"不是对抗性场景，是正常压力场景。挂起的前提链每一环都有生产代码支撑。

### 标准解法（两半，缺一不可）

**A. 升级依据改为"组存活"，而非直接子进程存活。**

仓库内标准实现是 `packages/subprocess/subprocess-local/src/spawn.ts` 的 `treeAlive()` / `kill()` / `graceTimer`：用 `process.kill(-pid, 0)` 探测组存活，且升级计时器"在直接子进程结算后仍然有效"（其注释原文：the escalation must survive direct-child settlement — the leader dying does not mean the tree died）。Rust 侧对应：

```rust
/// killpg(pgid, 0)：0 或 EPERM = 组仍有成员（EPERM 表示存在但无权，视为存活），ESRCH = 组不存在。
fn group_alive(pgid: i32) -> bool {
    let rc = unsafe { libc::kill(-pgid, 0) };
    if rc == 0 { return true; }
    matches!(std::io::Error::last_os_error().raw_os_error(), Some(libc::EPERM))
}
```

`ChildTree::is_alive`（Unix）改为组探测；`shutdown_tree` 的循环与升级结构不变 —— grace 内组活就一直等，grace 后组还活着就 `killpg(SIGKILL)`，再等现有 2s 有界窗口，最后 `reap()`。macOS / Linux 语义一致（spawn 时 `process_group(0)` 保证 pgid == child pid）。Windows 的 `is_alive` 保持直接子进程判定不变：job 存在时 `TerminateJobObject` 已杀全树，job 为 None 的降级路径只有直接子进程可观测（记为已知限制，由 Part B 的有界 join 兜底）。

可选加固（同 subprocess-local 的 `linuxProcessGroupHasLiveMembers`）：Linux 上组只剩僵尸时 `/proc` 扫描判定为死，避免多等一个 grace。非必需 —— 该情形只多等有界时间，不会挂起。升级窗口为 grace + 2s，pgid 复用的概率窗口与 subprocess-local 的既定取舍一致。

**B. reader 必须可取消，join 必须有界。**

std 没有 join-with-timeout，所以让 reader 在"EOF 或停止标志"二者之一到达时退出，且 ready-line 等待并入同一个循环：

- Unix：spawn 时对 stdout/stderr fd 设 `fcntl(F_SETFL, O_NONBLOCK)`；reader 循环 `read_line`，`WouldBlock` 时检查 `Arc<AtomicBool>` 停止标志（置位则退出），否则 sleep 20ms。`wait_for_ready_line` 的纯解析逻辑（`parse_ready_line`）保持原样单测，循环体并入 reader（WouldBlock 不再被当作 `ReadyError::Io`，而是重试条件）。
- Windows（管道不可非阻塞）：spawn 时 `DuplicateHandle` 复制读句柄留档；shutdown 时 `CancelIoEx(dup_handle, null)` 使阻塞的 `ReadFile` 以 `ERROR_OPERATION_ABORTED` 失败，reader 退出。job 为 None 的降级路径同样受此保护。
- `shutdown()` 顺序：`shutdown_tree(...)`（组存活升级）→ 置停止标志 + `CancelIoEx` → join（此时有界）。先杀树再取消读取，保住能拿到的最后日志行。

明确不采用：`std::mem::forget(handle)` 泄漏 reader 线程 —— 托盘应用进程常驻，泄漏的线程与 fd 会留存，不标准。明确记入边界：setsid 逃逸树不在组信号射程，归 sidecar 自己的 shutdown 负责（其 subprocess-local 已有 host-exit 强制终结）；shell 的职责是**不被它们挂死**。

### 测试

- 单测（注入假组存活）：grace 内"组活、直接子进程已死"→ 必须 SIGKILL；组死 → 不升级。
- 集成（Unix，cargo test）：`sh -c "trap '' TERM; sleep 30 & exit 0"` 以 `process_group(0)` 启动 → `shutdown_tree`（200ms grace）→ 断言 `group_alive(pgid) == false` 且总耗时 < grace + 2s + ε。镜像 subprocess-local 的 TERM-trapping 用例。
- reader 取消：fake node 脚本用 `detached: true` 生成 TERM 忽略孙进程并继承 stdout，打印 ready 后 leader 退出 → `process.shutdown(5s)` 必须 ≤ ~8s 返回（孙进程随后由测试清理）。Windows 同用例由 CI 跑。

---

## Bug 2（严重）：Windows 上孤儿 sidecar 回收永不生效

### 证据

- `pid.rs::command_of`：无平台分支地执行 `ps -p <pid> -o command=`。Windows 没有 `ps`；装了 Git for Windows 也只有 MSYS 版 `ps`，其 pid 空间与 Windows pid 不通，恒查无结果 → `output().ok()?` → `None`。
- 因此 `reap_stale_sidecar` 在 Windows 上只删 pid 文件、从不 kill；`terminate_pid` 的 `#[cfg(not(unix))]` `taskkill /T /F` 分支是死代码。
- 后果：dshd 崩溃后 Node sidecar 永久残留（端口、SQLite/session 写者、内存），下次启动也回收不掉。README 声称"两边都匹配才回收"，Windows 上实为"永不回收"。

### 标准解法：pid + 启动时间双重身份（PostgreSQL `postmaster.pid` 先例）

命令行匹配的根本缺陷是跨平台读取命令行没有稳定 API（Windows 官方 API 不提供命令行查询；`ps` 依赖外部二进制且实现各异）。启动时间才是 PID 复用问题的标准答案（PostgreSQL 的 postmaster.pid 正是 pid + starttime）：

- 写入时（`write_sidecar_pid`，子进程刚 spawn 还活着）读一次启动时间：
  - Linux：`/proc/<pid>/stat` 第 22 字段 starttime（clock ticks）+ `/proc/stat` 的 `btime` → `epoch = btime + starttime / HZ`（HZ 取 `sysconf(_SC_CLK_TCK)`，通常 100）换算成绝对 epoch 秒（重启安全，不再依赖"开机以来的 ticks"）。
  - macOS：`proc_pidinfo(pid, PROC_PIDTBSDINFO, …)` 的 `pbi_start_tvsec/tvusec`（libproc 公开 API，`extern "C"` 声明 + `#[link(name = "proc")]`）。
  - Windows：`OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, …)` + `GetProcessTimes` 的 creation `FILETIME`。
- pid 文件格式升级为三行 `pid\nbin\nstarttime_epoch_ms`，记录绝对时间（三平台统一、跨重启可比；`bin` 保留作诊断信息）。
- 读取启动时间失败（子进程秒死）时按旧两行格式写 → 两行旧记录一律"不可回收，只删文件"（保守，与现状"无脚本记录不可回收"一致；升级后首次启动对升级前残留的孤儿放行一次，记入文档）。
- 回收时重读启动时间，相等才 kill；`command_of` / `ps` 依赖整体删除。Windows 上 `OpenProcess` 失败（跨用户/提权进程）同样视为"不可验证 → 不杀"，fail-safe。
- Unix kill 升级为进程组语义：旧 sidecar 也是组组长（spawn 时 `process_group(0)`），`terminate_pid` 改为 `killpg(pid, SIGTERM)` → 等待 ≤5s → `killpg(pid, SIGKILL)`（与正常关停同一升级标准），取代现在的单 pid TERM + 200ms 裸 KILL。

### 测试

- 格式往返、旧两行记录不可回收。
- 用自身 pid + 错误 starttime → 不杀且删文件；真实 `sleep` 记录真 starttime → 回收成功（Unix）；Windows 由 CI。
- 现有 `a_foreign_dsh_web_at_the_recorded_pid_survives` 改写为 starttime 不匹配语义。

---

## Bug 3（中）：release 构建的 panic hook 是死代码

### 证据

- `Cargo.toml` `[profile.release] panic = "abort"`；`logs.rs::install_panic_hook` 依赖 `std::panic::set_hook`，该 hook 只在 unwinding 下运行 —— abort 下从不触发，发布版 `$DSH_HOME/logs/crash.log` 永远为空，与 README "panic log" 一栏直接矛盾。

### 标准解法

- 方案 A（推荐）：删除 `panic = "abort"`，恢复 unwinding —— hook 生效，panic 记录落盘后再按默认行为终止进程；在 `lto + opt-level = "s" + strip` 下体积代价可忽略。桌面应用把 panic 写成可读日志是行业标准做法。
- 方案 B：保留 abort，删除 hook 与 README 一栏，依赖系统级崩溃报告（macOS DiagnosticReports / Windows WER）。省代码，但失去 `$DSH_HOME` 内可随日志打包的诊断。
- 不采用"保留 hook 但只对 debug 生效并改文档"—— 语义误导。

顺带确认：无其他 profile 覆盖该项；改动后 README 同步更新。

---

## Bug 4（中）：导航之后 boot 失败无可见错误

### 证据

- `lib.rs::show_error` 依赖 `window.eval("window.__DSH_SHOW_ERROR__ && …")`，该全局只在 splash（`frontend/index.html`）定义。`navigate_to_sidecar` 成功后，`wait_desktop_client_ready` 失败（实际中最常见的失败点：Web UI 起不来或超慢）→ `Err` → `show_error` 在 sidecar 源上 eval 为 no-op；此时 sidecar 已被 `request_stop` 杀死，用户面对死页面，零提示。
- 次要竞态：splash 脚本在 `</body>` 前，极快的失败（`resolve_node` 等）可能先于脚本定义就 eval。

### 标准解法

- `run()` 里在启动 boot 线程前捕获 splash 资源 URL（`window.url()`；macOS/Linux 为 `tauri://localhost`，Windows 为 `http://tauri.localhost`，捕获而非硬编码，跨平台成立），存入 `AppState`。
- `show_error` / `show_migration` 统一流程：`window.navigate(splash_url)` → 轮询 `window.url()` 回到资源源（有界 5s）→ 再 eval。失败前从未导航时该 navigate 是幂等 no-op；捕获失败（`url()` 返回 Err）时退化为直接 eval（仅覆盖导航前的失败路径）。
- 把 `__DSH_SHOW_ERROR__` / `__DSH_SHOW_MIGRATION__` 的定义移到 `<head>`（消除定义竞态）。
- 测试：沿用仓库已有的故障注入先例（`DSH_DESKTOP_MIGRATE_FAIL=1`），加 `DSH_DESKTOP_BOOT_FAIL=client-ready`（注入到 `wait_desktop_client_ready`），smoke 脚本断言错误页可见，使该路径可脚本化。

---

## Bug 5（低）：文档与实现漂移

- README 表格：Windows 锁写作 `exclusive share_mode(0)`，实际是"全共享打开 + `LockFileEx` 字节范围锁"（`lock.rs` 注释自述：share_mode(0) 会让 chokidar 的 `fs.watch` EBUSY 杀死 Node，已弃用）。"Known limits" 一节 "share_mode(0) lock … not locally verified" 同样过时。
- `process.rs` 模块文档："Unix uses a process group; Windows is a stub" —— Windows 已实现 Job Object，不再是 stub。
- `sidecar.rs::SidecarProcess::shutdown` 文档："grace: drain window matching the Node process-shutdown contract (5s)" —— Windows 上 `signal_terminate` 是立即 `JobObject::terminate()`，没有优雅排空档。
- 修复：四处同步到实现；Bug 1 落地后 shutdown 语义变化一并更新。

---

## 附录：第一轮审查的"缺口"清单（次要，仅登记）

| 缺口 | 状态 |
| --- | --- |
| Windows 环境白名单缺 `APPDATA`/`LOCALAPPDATA`/`COMSPEC`/`PATHEXT`/`ProgramFiles`/`USERNAME` 等；pwsh-local 依赖 `env.ProgramFiles`（有兜底） | 待决策 |
| Unix 白名单缺 `SSH_AUTH_SOCK`（git over ssh）、`TERM`、`SSL_CERT_FILE`/`NODE_EXTRA_CA_CERTS` | 待决策 |
| 非竞态 TTL：`BOOTSTRAP_TTL_MS=30s` 自 sidecar 启动起算，shell 前置预算最坏 35s | 建议改为从首次 consume 起算 |
| `sidecar.log` 仅启动时轮转，单次运行无上限 | 低 |
| `desktop-state.json` 非原子写、损坏后静默回退且永不重写 | 低 |
| macOS 关窗隐藏后 dock 点击不恢复；托盘菜单英文硬编码 | 低 |
| `migration-backup-*` 迁移成功后不清理；迁移提示一闪而过 | 低 |

---

## 二次复核记录（本文件写出后）

对文件逐条断言重新回读源码核验，并修正三处：

1. **修正 Bug 4 的资源协议说明**：原稿把 Linux 的 asset protocol 写成 `http://tauri.localhost`，错误。Tauri 2 实为 macOS/Linux `tauri://localhost`、仅 Windows 用 `http://tauri.localhost`（WebView2 要求 http 风格 scheme）。修复方案本身"捕获 `window.url()` 而非硬编码"不受影响，但示例必须准确。
2. **补全 Bug 2 的 Linux 换算细节**：`epoch = btime + starttime / HZ`，HZ 取 `sysconf(_SC_CLK_TCK)`（通常 100），避免实现时再踩"ticks 与秒混淆"的坑。
3. **补全 Bug 1 的平台边界**：明确 Windows 的 `is_alive` 保持直接子进程判定（job 已杀全树；job=None 降级路径由 Part B 有界 join 兜底），并注明升级窗口 grace+2s 内 pgid 复用概率与 subprocess-local 既定取舍一致。

其余断言复核结果（全部命中源码）：

- `process.rs` `is_alive = matches!(self.child.try_wait(), Ok(None))`、`killpg` 组信号、2s 有界 reap 窗口；
- `sidecar.rs` shutdown 的 `readers.drain(..).join()`、`drain_rest`/`drain_to_log` 无超时阻塞读；
- 三个 subagent 包生产代码的 `stderr: 'inherit'`（subagent-acp/src/run.ts、subagent-claude-code/src/process.ts、subagent-codex/src/run.ts）；
- subprocess-local 的 `treeAlive()`（`process.kill(-pid, 0)`、ESRCH→死、EPERM→活）与"升级计时器在直接子进程结算后仍有效"注释；
- `pid.rs` 的 `ps` 无平台分支、`#[cfg(not(unix))]` taskkill 分支不可达；
- `Cargo.toml` `[profile.release] panic = "abort"`（Cargo.toml:33）；
- `lib.rs::show_error` 的 eval 依赖 splash 全局、`frontend/index.html` 脚本位于 body 末尾；
- README 锁表格与 Known limits 的 `share_mode(0)` 旧文案、`lock.rs` 弃用原因注释；
- `process.rs` 模块文档 "Windows is a stub"、`sidecar.rs::shutdown` 的 5s grace 注释与 Windows 实际语义不符。

复核结论：五个 bug 的定性、证据链与修复方案维持不变；方案均有仓库内先例（subprocess-local 的组存活升级）或行业先例（PostgreSQL postmaster.pid 的 pid+starttime）支撑，无需再降级或重写。

---

## 落地记录（2026-08-14，真实 Windows 11 主机）

五个 bug 按上述方案修复完毕并全部通过 `cargo fmt --check`、`cargo clippy -D warnings`、`cargo test`（53/53，五连跑）与 web-app vitest（24/24）。与原方案的三处偏离及两项新发现：

1. **Bug 3 为误报，未改代码。** `rustc -C panic=abort` 实验证明 abort 策略下 panic hook 照常运行（hook 先写 crash.log 再 abort），"hook 只在 unwinding 下运行"的前提不成立。保留 `panic = "abort"`：它还让 panic 的 boot 线程把进程一并带走，而不是让窗口永远停在 splash。
2. **Bug 2 的创建时间身份仅用于 Windows。** macOS 盲写 `proc_pidinfo` FFI、Linux 写 `/proc` 解析，去替换一个本来就能用的 `ps` 匹配，风险大于收益；坏掉的平台只有 Windows，故 pid 文件第三行（创建时间 `FILETIME`）与比对仅存在于此。Unix 保留 `ps` 命令行匹配，`terminate_pid` 按 review 升级为 killpg TERM→5s→KILL。Windows 真机测试验证：活 ping + 匹配创建时间 → 回收；不匹配 → 存活。
3. **Bug 1 的 reader 取消按方案落地（Unix `O_NONBLOCK` + 停止标志、Windows `DuplicateHandle`+`CancelIoEx`），另加 2 秒看门狗 join 作为取消本身失败时的最后手段**（宁可泄漏线程也不挂起退出）。Unix 组升级测试镜像 subprocess-local 的 TERM-trap 用例（本机为 Windows，该用例归 Unix/CI）。
4. **新发现（并发修复会话贡献）：** GUI 子系统下 `node.exe`/`taskkill` 子进程会闪现控制台窗口，已由 `CREATE_NO_WINDOW`（`hide_child_console`）修复并另有 Agent Note 记录。该项修复与 `CREATE_NO_WINDOW` + 继承控制台句柄在并发下可能令 `taskkill` spawn 失败的边界相叠加，是 `windows_identity_governs_reaping` 首轮全量并行失败的原因——`taskkill` 现带显式 null stdio，五连跑稳定。
5. **附录决策落地：** env 白名单扩充（Windows 布局/工具变量 + `TZ`/`TERM`/`SSH_AUTH_SOCK`/CA bundle）已做；nonce TTL 取 120 秒（覆盖壳的完整启动预算，含杀毒首扫），未采用"从首次 consume 起算"；`desktop-state.json` 原子写、运行中日志轮转、dock 点击恢复、托盘菜单 i18n、迁移备份清理仍登记为低优先级未做。

另修复（第一轮清单）：记录的 workspace 目录不存在时回退 `~/Documents`；`DSH_WORKSPACE` 覆盖不再持久化；日志轮转移到持锁之后；`/bin/sh` 测试加 `#[cfg(unix)]` 门控（Windows `cargo test` 从红转绿）；sidecar 中途死亡由监督线程回到 splash 页展示退出状态；启动失败统一"导航回 splash + 幂等重试 eval"，钩子定义移入 `<head>`，`DSH_DESKTOP_BOOT_FAIL=client-ready` 提供脚本化注入。
