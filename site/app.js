/* DSHD 官网交互:亮暗主题 + 中英语言切换 + 下载链接填充 + 滚动入场。
   主题:data-theme + localStorage("dshd.theme") + prefers-color-scheme(缺省跟随系统)。
   语言:data-lang + localStorage("dshd.lang"),缺省按浏览器语言(zh 开头为中文,否则英文)。
   防 FOUC 的早期设置由 index.html <head> 内联同步脚本完成(渲染前生效)。 */
(function () {
  "use strict";
  var THEME_KEY = "dshd.theme";
  var LANG_KEY = "dshd.lang";
  var root = document.documentElement;
  var TITLES = {
    zh: "DeepSeek Harness Desktop · 非官方 Unofficial — 双击即用的桌面应用",
    en: "DeepSeek Harness Desktop (Unofficial) — the official harness as a double-click desktop app"
  };

  function resolvedTheme() {
    var t = "";
    try { t = localStorage.getItem(THEME_KEY) || ""; } catch (e) {}
    if (t === "light" || t === "dark") return t;
    var mql = window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)");
    return mql && mql.matches ? "dark" : "light";
  }

  function resolvedLang() {
    var l = "";
    try { l = localStorage.getItem(LANG_KEY) || ""; } catch (e) {}
    if (l === "zh" || l === "en") return l;
    return (navigator.language || "zh").toLowerCase().indexOf("zh") === 0 ? "zh" : "en";
  }

  function applyLang(lang) {
    root.setAttribute("data-lang", lang);
    root.setAttribute("lang", lang === "zh" ? "zh-CN" : "en");
    try { localStorage.setItem(LANG_KEY, lang); } catch (e) {}
    if (TITLES[lang]) document.title = TITLES[lang];
    // 语言按钮显示"可切换到的目标语言"(中文态显 EN,英文态显中)
    document.querySelectorAll("[data-lang-toggle]").forEach(function (btn) {
      btn.textContent = lang === "zh" ? "EN" : "中";
    });
  }

  document.addEventListener("DOMContentLoaded", function () {
    applyLang(resolvedLang());

    // 主题切换按钮:亮/暗间切换并持久化(点击后不再跟随系统)
    document.querySelectorAll("[data-theme-toggle]").forEach(function (btn) {
      btn.addEventListener("click", function () {
        var next = resolvedTheme() === "dark" ? "light" : "dark";
        root.setAttribute("data-theme", next);
        try { localStorage.setItem(THEME_KEY, next); } catch (e) {}
      });
    });

    // 语言切换按钮:zh 与 en 互切
    document.querySelectorAll("[data-lang-toggle]").forEach(function (btn) {
      btn.addEventListener("click", function () {
        var cur = root.getAttribute("data-lang") || resolvedLang();
        applyLang(cur === "zh" ? "en" : "zh");
      });
    });

    // 下载链接与版本号填充(常量在 release.js,全站唯一发布配置)
    if (window.RELEASE) {
      var r = window.RELEASE;
      document.querySelectorAll("[data-dl-mac]").forEach(function (a) {
        if (r.MAC && r.MAC !== "#") a.setAttribute("href", r.MAC);
      });
      document.querySelectorAll("[data-dl-win]").forEach(function (a) {
        if (r.WIN && r.WIN !== "#") a.setAttribute("href", r.WIN);
      });
      document.querySelectorAll("[data-version]").forEach(function (el) {
        el.textContent = r.VERSION;
      });
    }

    // 滚动入场:进入视口加 is-visible(CSS 配合 [data-reveal])
    if ("IntersectionObserver" in window) {
      var io = new IntersectionObserver(function (entries) {
        entries.forEach(function (e) {
          if (e.isIntersecting) { e.target.classList.add("is-visible"); io.unobserve(e.target); }
        });
      }, { rootMargin: "0px 0px -8% 0px", threshold: 0.04 });
      document.querySelectorAll("[data-reveal]").forEach(function (el) { io.observe(el); });
    } else {
      document.querySelectorAll("[data-reveal]").forEach(function (el) { el.classList.add("is-visible"); });
    }
  });
})();
