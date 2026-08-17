# Agent Note: 桌面 WKWebView 呈现 Safari.app 身份

Status: implemented

[English](2026-08-17-desktop-wkwebview-safari-identity.md) | 中文

## Problem

会话 composer 只在 `isSafariBrowser` 看到 Apple vendor 与 `Version/… Safari/…` 形式的 user-agent 时，才会恢复 Safari 陈旧的原生 textarea 布局（[textarea 恢复](2026-08-13-safari-textarea-soft-wrap-reflow.md)）。macOS 上的 dshd 把该 GUI 放在 WKWebView 里。系统 WebView 会报告 `vendor = Apple Computer, Inc.` 和 AppleWebKit token，但没有 `Version/` 或 `Safari/` token，因此恢复逻辑从不运行。该恢复所针对的 Backspace 跨软换行缺陷是 WebKit 文本控件的问题，而 WKWebView 用的就是这个引擎。

## Decision

macOS 主窗口设置一条钉死的 User-Agent：保留实测到的 AppleWebKit token，并补上 Safari.app 的 `Version/26.5 Safari/605.1.15` 形式。`WebviewWindowBuilder::user_agent` 在窗口构建前生效。`isSafariBrowser` 不变：未加修饰的 Apple web view 仍返回 false，所有非桌面组合继续使用宿主真实的 identity。

该字符串作为 `MACOS_SAFARI_WEBVIEW_USER_AGENT` 放在 `apps/desktop/src-tauri/src/webview_identity.rs` 中，并与 `packages/client/ui-conversation/tests/safari.client.spec.ts` 中对应夹具字节相同。Windows WebView2 是 Chromium，不使用该值。

## Alternatives considered

**把 `isSafariBrowser` 放宽到每一个 Apple web view。** 被否决：该函数是共享的。把任何 `vendor === Apple` 的 WebKit 视图都当成 Safari，会改变本 fork 不得触碰的 `dsh web` 嵌入；现有的 "Apple web view" 夹具正是为了让该情况保持 false。

**从 `desktop-bootstrap.ts` 伪造 `navigator.userAgent`。** 该引导脚本仅在设置了桌面环境变量时运行，fork 红线可以成立。但它仍会与第一次 composer 布局和脚本注入竞态，并且对页面谎报 WebView 已经拥有的字段。检测函数读的就是原生 `user_agent`。

**让检测保持 false，接受 dshd 里的光标缺陷。** 被否决：RC7 已为这一引擎家族交付恢复逻辑，而桌面宿主就是 WKWebView。

## Consequences

macOS 上的 dshd 呈现 Safari.app 的 token 形式，因此一次违反 textarea 溢出不变量的原生缩短，会支付与 Safari.app 相同的四次强制布局。`dsh web` 在 Safari.app 中本来就是 true；在 Chrome 中仍为 false。本壳之外未加修饰的 WKWebView 仍被排除。恢复逻辑仍然只在原生缩短信号与溢出不匹配同时存在时运行。

## Testing

`cargo test webview_identity` 断言钉死字符串在 `Safari/<digit>` 之前带有 `Version/<digit>`，且不含 iOS 其他浏览器 token。`pnpm exec vitest run packages/client/ui-conversation/tests/safari.client.spec.ts` 覆盖未加修饰的 macOS WKWebView（false）与钉死 token 形式（true）。用该 `customUserAgent` 构造的宿主 WKWebView 会报告相同的 `navigator.userAgent`，且仍为 `vendor = Apple Computer, Inc.`，`isSafariBrowser` 接受该 identity。

未覆盖：在正在运行的 dshd 窗口内用 Backspace 跨过软换行阈值。该路径需要会抢焦点的 GUI 自动化；在可自动化的 Safari/WKWebView 泳道出现之前，由组件恢复测试与 identity 门控代替。
