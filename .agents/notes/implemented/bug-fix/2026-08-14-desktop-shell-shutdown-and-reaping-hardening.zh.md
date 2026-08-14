# Agent Note：加固桌面壳的关停、孤儿 sidecar 回收与启动失败可见性

Status: implemented

[English](2026-08-14-desktop-shell-shutdown-and-reaping-hardening.md) | 中文

## 问题

Review 的四项发现（证据链见 `apps/desktop/DESKTOP-REVIEW.md`）指向同一主题：壳过度信任了顺利路径。关停升级只观察直接子进程，活得比组长久的进程组收不到 SIGKILL；日志 reader 线程无期限 join，任何持有管道写端的幸存者都会把托盘退出永久挂死。孤儿 sidecar 回收无条件执行 `ps -p <pid> -o command=`，而 Windows 上没有 `ps`（或只有不兼容的 MSYS 版），回收逻辑恰好在最容易留下孤儿的平台上是死代码。WebView 导航到 sidecar 之后的启动失败会在一个从未定义 `window.__DSH_SHOW_ERROR__` 的页面上求值，用户面对死页面毫无反馈；导航前的快速失败也可能跑赢 splash 页自身的脚本加载。sidecar 中途死亡则完全没有监督。

## 决策

升级依据改为进程组存活探测（`killpg(pgid, 0)`，EPERM 视为存活），镜像 `dsh-subprocess-local` 的 `treeAlive()`；探测内部先收割直接子进程，僵尸组长不能让已死的组继续应答。reader 变为可取消、join 有界：Unix 在 spawn 时把管道设为非阻塞，reader 在 `WouldBlock` 重试之间轮询 `Arc<AtomicBool>` 停止标志；Windows 复制管道读句柄，关停时调用 `CancelIoEx` 中止挂起的读；最终的看门狗 join 宁可泄漏卡死的 reader 也不挂起退出——取消是机制，看门狗是最后手段。孤儿回收先证明身份再杀：Unix 保留原有的 `ps` 命令行匹配；Windows 的 pid 文件增加第三行记录进程创建时间（`GetProcessTimes`），回收时与重新 `OpenProcess` 读到的值比对——pid 复用会改变创建时间，复用的 pid 永远不匹配。回收在 Unix 上杀整组（TERM、5 秒、KILL），在 Windows 上以显式 null stdio 运行 `taskkill /T /F`（继承的控制台句柄加 `CREATE_NO_WINDOW` 在并发下可能令 spawn 失败）。启动失败先导航回捕获的 splash URL 再求值错误钩子，用幂等 eval 重试穿过页面加载竞态；`DSH_DESKTOP_BOOT_FAIL=client-ready` 让导航后路径可脚本化；启动成功后由监督线程轮询 supervisor，意外退出时在 splash 页展示退出状态。伴随修复：已不存在的记录 workspace 回退到 `~/Documents` 而不是令 spawn 失败；`DSH_WORKSPACE` 覆盖值绝不持久化进 `desktop-state.json`；日志轮转移到持有 home 锁之后；sidecar 环境白名单补上 Windows 布局与工具变量（`APPDATA`、`COMSPEC`、`PATHEXT`、`ProgramFiles` 等）以及 `TZ`、`TERM`、`SSH_AUTH_SOCK` 与 CA bundle 覆盖，因为 agent 拉起的每个工具子进程继承的正是这份环境。Web bootstrap nonce 的 TTL 从 30 秒提到 120 秒：该窗口覆盖壳的整个启动预算，首启在实时杀毒扫描下仅 ready line 之前就可能花掉 15 秒。

## 备选方案

**泄漏 reader 线程而不是取消。** 否决为机制：dshd 是常驻托盘应用，会话中途的退出监督可能触发 `request_stop` 而应用继续运行，被阻塞的 reader 会存活整个进程生命周期。只保留为看门狗的最后手段。

**全平台 pid+创建时间身份（PostgreSQL `postmaster.pid` 设计）。** 否决为跨平台承诺：macOS 需要盲写 `proc_pidinfo` FFI、Linux 需要 `/proc` 解析，去替换一个本来就能用的 `ps` 匹配；坏掉的平台是 Windows，因此创建时间身份仅用于 Windows，记录格式按平台塑形。

**移除 `panic = "abort"` 让 crash 日志钩子生效。** 实验后否决：abort 策略下 panic 钩子同样运行（`rustc -C panic=abort` 验证），钩子从来不是死代码；abort 还能让 panic 的 boot 线程把进程一并带走，而不是让窗口停在 splash 上。

**从首次 consume 起算提高 nonce TTL。** 否决：consume 就是一次性使用；需要变宽的是它之前的窗口。

## 后果

托盘退出、ctrl-c 与退出监督汇入同一条有界关停路径（grace + 2 秒收割 + 2 秒 reader 期限）。Windows 回收现在能杀掉经过身份核验的孤儿、拒绝无法核验的记录；本次改动之前写入的 pid 文件会被只删不杀地丢弃一次。`cargo test` 在真实 Windows 主机全绿（身份测试对活着的 ping 分别做回收与放过），Unix 的组升级测试镜像 `dsh-subprocess-local` 的 TERM-trap 用例。Unix 关停有排空窗口；Windows 从来没有、现在也没有——记为已知限制而不是粉饰。
