/* DSHD 官网下载配置 —— 全站唯一需要改的发布常量。
   替换占位后重新部署(wrangler pages deploy site --project-name dshd --branch main)即可。

   安装包托管在 Cloudflare R2 桶 dshd-releases,经自定义域 dshd-dl.octoooo.com 分发
   (不用 pub-*.r2.dev:该子域有速率限制,Cloudflare 不建议用于生产分发)。
   object key 形如 v<版本>/<构建提交短号>/<文件名>:同版本重打包也换 key,不覆盖旧对象。
   该域的安装包带 max-age=14400,覆盖同名对象后 CDN 仍会发旧包最多 4 小时,
   换 key 才能让新包立即生效(也让每个 URL 恒指同一份二进制)。
   同一批文件在 GitHub Releases 亦有分发:
     https://github.com/Octo-o-o-o/deepseek-harness-desktop/releases/tag/dshd-v0.1.12

   macOS 0.1.12 构建自 c1ff6cd816(上游 0.1.0-rc.7)。Windows 安装器仍是上一轮
   e1cc57dc0f 的 0.1.0 包,本版未重打。

   SHA-256(与 GitHub Release 页所列一致,供用户核对下载完整性):
     dshd-0.1.12-arm64.dmg     4baaf6c00463170ce14e6ede33b0d7b0467f1e0897176ffe67b38b76e430050f
     dshd_0.1.0_x64-setup.exe  6c7455ab921358349a683efed326872e52c2b706320125843ea04419a257cc6a

   占位 "#" 表示尚未提供:按钮渲染为当前页锚点,不会跳空域名。 */
/* 必须挂在 window 上:app.js 读的是 window.RELEASE。
   顶层 const/let 只进入脚本全局作用域,不会成为 window 的属性,那样填充逻辑会被整块跳过。 */
window.RELEASE = {
  VERSION: "v0.1.12",
  MAC: "https://dshd-dl.octoooo.com/v0.1.12/c1ff6cd8/dshd-0.1.12-arm64.dmg", // macOS Apple Silicon(.dmg)
  WIN: "https://dshd-dl.octoooo.com/v0.1.0/e1cc57dc/dshd_0.1.0_x64-setup.exe" // Windows x64(NSIS .exe)
};
