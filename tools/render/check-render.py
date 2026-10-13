#!/usr/bin/env python3
"""用本地 HTTP 服务 + 无头浏览器渲染真实前端产物，验证界面确实能画出来。

为什么需要这个
  vitest 用 jsdom 挂载组件，能验证逻辑与文案，但**看不到真实浏览器里的
  渲染结果**。构建配置写错（base 路径不对、资源加载失败、模块加载报错）
  时 `vite build` 依然成功、dist 也生成了，打开却是一片空白——这正是
  「功能全都能用不了」这类问题最常见的表现，而单元测试抓不到。
  这里直接加载构建产物、跑一遍真实交互，从渲染结果里核对界面文案。

为什么不能直接用 file://
  Vite 产物以 ES module 加载（type="module"）。浏览器对 file:// 下的
  module 请求一律按 CORS 失败处理，页面只加载到注入的替身脚本、
  <div id="app"> 始终为空——截图是一张白图。必须经 http 提供。

做法
  把带替身的渲染页写进 dist/（这样 /assets/... 这类绝对路径能被同一个
  服务解析），起一个临时静态服务，用无头浏览器打开、截图、导出 DOM，
  再从**剔除 script 之后**的 DOM 里核对文案。
  替身脚本会模拟 Tauri 的 invoke，并自动完成
  「添加目录 → 扫描 → 选中第一行」这条真实交互路径。

局限
  验证的是「前端产物能否正确渲染与交互」，**不能替代 Tauri 窗口内的真实
  运行**：原生对话框、文件系统写入、实际 3D 渲染都不在覆盖范围内。

用法
  python tools/render/check-render.py
  # 需要先 pnpm build 生成 dist/
  # 浏览器可用环境变量 CHROME_PATH 指定，未指定时自动在常见位置查找
"""
import http.server
import os
import re
import socket
import subprocess
import sys
import threading

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))  # tools/render -> 仓库根
DIST = os.path.join(ROOT, "dist")
SHOT = os.path.join(ROOT, "..", "_temp", "frontend_render.png")
PAGE_NAME = "__render__.html"

# 无头浏览器的常见位置（Windows / Linux / macOS）
BROWSER_CANDIDATES = [
    os.environ.get("CHROME_PATH", ""),
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
]

SHIM = r"""
<script>
(function () {
  var summary = {
    fileName: "28速单区块简单熔炉.litematic",
    dirName: "测试目录",
    path: "C:/测试目录/28速单区块简单熔炉.litematic",
    fileSize: 2978, formatVersion: 6,
    dataVersion: 3465, mcVersionName: "1.20.1",
    name: "28速单区块简单熔炉", author: "黑山大叔",
    description: "单区块熔炉阵列，用于测试解析与预览",
    timeCreated: 1700000000000, timeModified: 1700000000000,
    enclosing: [16, 12, 16], totalBlocks: 1234, totalVolume: 3072,
    regionCount: 1, entityCount: 3, tileEntityCount: 2, pendingTicks: 0,
    hasPreview: false, error: ""
  };
  var scene = {
    batches: [], totalVertices: 0, totalTriangles: 0,
    enclosing: [16, 12, 16], origin: [0, 0, 0],
    mcVersionName: "1.20.1", dataVersion: 3465, unresolvedBlocks: 0
  };
  var analysis = {
    blocks: [], entities: [], tileEntities: [],
    nonAirTotal: 1234, airTotal: 1838,
    distinctBlockTypes: 18, distinctStates: 26
  };
  var CALLS = [];
  function fake(cmd) {
    // 把调用过的命令记进文档标题：dump-dom 时能直接看到「到底有没有发起扫描」，
    // 不用去猜无头浏览器里的执行情况
    CALLS.push(String(cmd));
    document.title = "CALLS:" + CALLS.join("|");
    // 目录选择框：点击「添加目录」后前端会调它，必须回一个字符串路径，
    // 否则 typeof dir !== "string" 直接 return，列表永远是空的
    if (String(cmd).indexOf("dialog") >= 0) return Promise.resolve("C:/测试目录");
    switch (cmd) {
      case "scan_litematics":
        return Promise.resolve({ items: [summary], skippedDirs: 0, skippedPaths: [] });
      case "peek_schematic": return Promise.resolve(summary);
      case "analyze_schematic": return Promise.resolve(analysis);
      case "build_scene": return Promise.resolve(scene);
      case "mc_version_name": return Promise.resolve("1.20.1");
      case "list_block_versions": return Promise.resolve([
        { index: 0, name: "1.20.1", dataVersion: 3465, blockCount: 1234, isOverlay: false }
      ]);
      case "list_packs": return Promise.resolve({ packs: [], loadFailures: [], skipped: [] });
      case "list_backups": return Promise.resolve([]);
      case "read_operation_log": return Promise.resolve([]);
      default: return Promise.resolve([]);
    }
  }
  window.__TAURI_INTERNALS__ = {
    invoke: function (cmd) { return fake(cmd); },
    transformCallback: function (cb) { return cb; },
    convertFileSrc: function (p) { return p; },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } }
  };
  window.__TAURI__ = {
    core: { invoke: function (cmd) { return fake(cmd); } },
    event: { listen: function () { return Promise.resolve(function () {}); } },
    window: { getCurrentWindow: function () { return {
      minimize: function () {}, toggleMaximize: function () {},
      close: function () {}, startDragging: function () {},
      isMaximized: function () { return Promise.resolve(false); },
      onResized: function () { return Promise.resolve(function () {}); }
    }; } }
  };

  // 自动走一遍真实交互：点「添加目录」让列表出来，再点第一行让详情面板出来。
  // 只截空状态的话，列表、详情与统计是否真能画出来都无从判断。
  //
  // 判据用「正文里是否出现文件名 / 数据版本」，不能用 `tbody tr`：
  // Arco 的空状态也渲染成一个 tr，用它会误判成「已有数据」而停止，
  // 结果一次都不点（上面这两个判据是实测踩出来的）。
  var tries = 0, state = "wait", clicked = false;
  function status() {
    document.title = "CALLS:" + CALLS.join("|") + " SCAN:" + state + " tries=" + tries;
  }
  status();
  var timer = setInterval(function () {
    tries++;
    var body = document.body ? document.body.textContent || "" : "";
    var hasFile = body.indexOf("28速单区块简单熔炉") >= 0;
    var hasDetail = body.indexOf("3465") >= 0;

    if (hasFile && hasDetail) { state = "detail-shown"; status(); clearInterval(timer); return; }
    if (hasFile) {
      var row = Array.prototype.slice.call(document.querySelectorAll("tbody tr"))
        .filter(function (r) { return (r.textContent || "").indexOf("28速单区块简单熔炉") >= 0; })[0];
      if (row && state !== "row-clicked") { row.click(); state = "row-clicked"; }
      else if (!row) { state = "no-row"; }
      status();
    } else {
      var btn = Array.prototype.slice.call(document.querySelectorAll("button"))
        .filter(function (b) { return (b.textContent || "").indexOf("添加目录") >= 0; })[0];
      if (btn && !clicked) { clicked = true; btn.click(); state = "clicked"; }
      else if (!btn) { state = "no-button"; }
      status();
    }
    if (tries > 80) { state = "timeout"; status(); clearInterval(timer); }
  }, 100);
})();
</script>
"""


def find_browser():
    for p in BROWSER_CANDIDATES:
        if p and os.path.exists(p):
            return p
    return None


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *a):
        pass


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def main():
    if not os.path.isdir(DIST) or not os.path.exists(os.path.join(DIST, "index.html")):
        print(f"未找到构建产物 {DIST}/index.html，请先执行 pnpm build")
        return 2

    browser = find_browser()
    if not browser:
        print("未找到无头浏览器。可用 CHROME_PATH 环境变量指定，查找过：")
        for p in BROWSER_CANDIDATES:
            if p:
                print(f"  {p}")
        return 2
    print(f"浏览器: {browser}")

    index = open(os.path.join(DIST, "index.html"), encoding="utf-8").read()
    if "<head>" in index:
        html = index.replace("<head>", "<head>" + SHIM, 1)
    else:
        html = SHIM + index
    render_path = os.path.join(DIST, PAGE_NAME)
    with open(render_path, "w", encoding="utf-8") as f:
        f.write(html)

    port = free_port()
    handler = lambda *a, **k: Quiet(*a, directory=DIST, **k)
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{port}/{PAGE_NAME}"
    print(f"临时服务: {url}")

    ok = False
    try:
        common = [
            browser, "--headless=new", "--disable-gpu", "--no-sandbox",
            "--virtual-time-budget=9000",
        ]
        os.makedirs(os.path.dirname(SHOT), exist_ok=True)
        shot = subprocess.run(
            common + ["--hide-scrollbars", "--window-size=1400,900",
                      f"--screenshot={SHOT}", url],
            capture_output=True, timeout=180,
        )
        print(f"截图退出码 {shot.returncode}")

        dump = subprocess.run(common + ["--dump-dom", url],
                              capture_output=True, timeout=180)
        dom = dump.stdout.decode("utf-8", "replace")
        dom_path = os.path.join(os.path.dirname(SHOT), "render_dom.html")
        with open(dom_path, "w", encoding="utf-8") as f:
            f.write(dom)

        # 关键词检查必须在**去掉 script 之后**做：替身脚本本身就写在页面里，
        # 包含文件名、目录名等字样，直接在整份 DOM 里搜会把「替身里写了」
        # 误当成「界面渲染出来了」——列表渲染坏掉时这些词照样命中。
        visible = re.sub(r"(?is)<script\b.*?</script>", " ", dom)
        print(f"DOM {len(dom)} 字节，去掉 script 后 {len(visible)} 字节 -> {dom_path}")
        m = re.search(r"CALLS:([^<]*)", visible)
        print(f"替身状态: {m.group(1).strip() if m else '(无)'}")

        print("\n=== 界面文案是否真的渲染出来（已剔除脚本）===")
        results = []

        def need(kw, label=None):
            hit = kw in visible
            results.append(hit)
            print(f"  {(label or kw):<24} {'是' if hit else '否'}")

        for kw in ["添加目录", "方块替换", "版本转换", "重复文件筛查", "备份管理", "资源包管理"]:
            need(kw)
        # 自动交互之后，这些必须真的出现
        need("28速单区块简单熔炉", "列表里的文件名")
        need("1.20.1", "制作版本")
        need("3465", "详情里的数据版本")
        need("黑山大叔", "详情里的作者")
        for kw in ["信息", "方块", "实体"]:
            need(kw)
        need("扫描完成", "扫描状态")
        # 选中文件后，两处空状态引导都应当消失
        for kw in ["点击「添加目录」选择投影所在文件夹", "选择一个投影文件以查看详情"]:
            gone = kw not in visible
            results.append(gone)
            print(f"  {'不应再出现：' + kw:<24} {'仍在（错误）' if not gone else '已消失（正确）'}")

        ok = all(results)
        size = os.path.getsize(SHOT) if os.path.exists(SHOT) else 0
        print(f"\n截图: {SHOT}  {size} 字节")
        print(f"DOM 体积 {len(visible)} 字节（空白页约 3.4 KB，正常渲染应显著更大）")
    finally:
        srv.shutdown()
        if os.path.exists(render_path):
            os.remove(render_path)

    print("\n渲染检查：" + ("全部通过" if ok else "存在失败项"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
