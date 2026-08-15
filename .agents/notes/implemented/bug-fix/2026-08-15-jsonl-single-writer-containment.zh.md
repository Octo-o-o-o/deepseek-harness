# Agent Note: 拒绝第二个 JSONL writer，而不是任由日志损坏

Status: implemented

[English](2026-08-15-jsonl-single-writer-containment.md) | 中文

## Problem

JSONL 后端在文档中声明「每会话一个活动 writer」，却没有任何东西强制它；而桌面应用让违反这条约束成为常态：`dshd` 与 `npx @deepseek-ai/dsh` 按设计共享 `~/.dsh`，用户在两处打开同一会话时，就有两个后端实例在写同一个文件。

损坏并不需要任何一方失败。`PersistenceCoordinator.append` 从 `this.states`（实例内存）取游标，并只针对它校验连续性（`coordinator.ts:694-704`）。两个实例 adopt 到同一存储长度后各自持有游标 `N`，于是都从 `seq = N` 开始追加批次，两次写入都会成功。随后读取端撞上重复：`format.ts:364-371` 报出 `corrupt session log: seq gap in committed region`，当该批携带 `turn/end` 时抛出，否则**丢弃该行之后的全部事件**。用户在写入时看不到任何错误，历史却已丢失。

同一个文件上还存在另外两条竞态。`appendLines` 记录写前长度，写失败时截断回该长度——这会删掉第二个 writer 在此期间已提交的数据。`repair` 截断到的偏移量由本实例读到的日志算出，一旦有外部追加，该偏移就落在对方数据之中。

本工作的早期草稿曾断言损坏需要「两个进程、同一会话、且其中一方写失败」。该判断是错的，并导致这项修复被排在了更不紧急的工作之后。

## Decision

后端按会话记录它自认为拥有的磁盘状态——`dev`、`ino` 与字节长度——并在文件不处于该状态时拒绝写入。

所有权在本实例刚刚看过或刚刚产出整个文件的位置取得：`readPrefix`（adopt）之中，以及 `materialize`（发布）之后。**在 adopt 而非首次 append 时记录**，正是该检查得以成立的关键：两个实例 adopt 到同一长度时都会记录它，第一次 append 改变了文件，第二个实例的检查随即失败，而不会在已被占用的位置追加。

`appendLines` 在写入前校验，写入后重新 `stat` 文件来更新记录，而不是把编码长度加上去——压缩会让该长度难以直接推断。`repair` 在它用于截断的同一个 handle 上校验，因此破坏性偏移不会被施加到偏移算出之后又发生变化的文件上。

拒绝是单向的。一旦观察到外部写入，coordinator 的内存游标就不再描述该文件，于是该会话被标记为 poisoned，后续写入持续被拒，直到重新打开。所有权按会话为键，因此一个会话上的冲突不影响该 home 下其余会话继续写入。

这限制的是损害范围，而不是一把锁。不执行这段代码的 writer——例如旧版 CLI——仍可能穿插写入。要彻底杜绝需要强制性 lease，那是更大的改动，此处如实记为暂缓事项，而非假装已经做到。

## Alternatives considered

**跨进程咨询锁（`proper-lockfile` 之类）。** 它同样拦不住旧版 CLI，因为咨询锁只约束参与者；而且会引入本检查没有的失败模式：笔记本休眠后把活跃锁误判为 stale、崩溃后残留锁、以及没有 fencing token。相比之下，它只对本就配合的 writer 把「检测」换成「预防」。

**只比较字节长度。** 单看长度无法区分「文件被替换」与「文件被追加」。`dev`/`ino` 在同一次 `stat` 中即可取得，不增加开销，正好补上这个缺口。

**改在 coordinator 中校验。** coordinator 与后端无关，而 SQLite 后端有自己的事务保证。把文件身份检查放进去，等于把文件语义强加给所有后端。

## Verification

`packages/session/session-persistence-jsonl/tests/jsonl.spec.ts` 新增 `single-writer containment` 套件，用两个后端实例操作同一 root：第一个 writer 扩展日志后，第二个的 append 被拒绝，且第一个 writer 的事件仍可读；被 poison 的会话以另一条消息持续拒绝；发生冲突后无关会话仍可写入；截断偏移早于外部追加的 repair 被拒绝，且已提交事件完好。

该套件已确认「因正确的原因而失败」：把 `assertWriteOwnership` 改为立即返回后，四个用例全部失败；恢复后转绿。包级全量通过（`session-persistence`、`session-persistence-jsonl` 与 `web-app` 共 508 项），说明该检查未干扰它所守护的单 writer 路径。

## Consequences

在同一会话上同时使用桌面应用与 CLI，现在会得到一次被拒绝的写入，消息中指明存在另一个 writer，而不是一个悄悄丢掉尾部的日志。只在一处打开的会话不受影响：该检查只是 append 本就要打开的 handle 上的一次 `stat`。

遇到拒绝的用户必须重新打开该会话才能继续写入。这是刻意的——另一种选择是继续信任一个已知与文件不一致的游标。
