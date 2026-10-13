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

  // 要逐个打开的对话框，以及各自打开后应出现的标题。
  // 打开对话框会触发各自的加载逻辑（读取可转换版本、资源包清单、备份列表），
  // 这些一旦抛错，界面表现就是「点了没反应」。
  var DIALOG_BUTTONS = ["方块替换", "版本转换", "重复文件筛查", "备份管理", "资源包管理"];
  var DIALOG_TITLES = {
    "方块替换": "方块替换",
    "版本转换": "转换游戏版本",
    "重复文件筛查": "重复文件筛查",
    "备份管理": "备份管理",
    "资源包管理": "资源包管理",
  };
  var DIALOGS = [];
  // 已尝试过的对话框名（与 DIALOGS 里带结论的字符串分开：
  // 第一版用 DIALOGS.includes(name) 判断，而里面存的是「名字=结论」，
  // 于是永远判为未处理，同一个对话框被反复点开（实测死循环 79 次）。
  var dialogTried = [];
  var pendingCheck = null;
  var dialogsDone = false;

  function fake(cmd) {
    // 把调用过的命令记进文档标题：dump-dom 时能直接看到「到底有没有发起扫描」，
    // 不用去猜无头浏览器里的执行情况
    CALLS.push(String(cmd));
    document.title = "CALLS:" + CALLS.join("|");
    // 目录选择框：点击「添加目录」后前端会调它，必须回一个字符串路径，
    // 否则 typeof dir !== "string" 直接 return，列表永远是空的
    if (String(cmd).indexOf("dialog") >= 0) return Promise.resolve("C:/测试目录");
    // 系统主题：App.vue 走的是 `invoke("plugin:window|theme")`，
    // 不是 window.__TAURI__.window。这里显式回一个值，让它走正常路径，
    // 而不是每次渲染都落进「读不到主题」的兜底分支。
    if (cmd === "plugin:window|theme") return Promise.resolve("light");
    switch (cmd) {
      case "scan_litematics":
        // 返回多条：列表能否出现纵向滚动条取决于内容是否超出可视高度，
        // 只给一条时永远测不出滚动（表格内部高度足够容纳一行）。
        // 标题里的「28速单区块简单熔炉」仍是第一条，后续断言依赖它。
        var many = [summary];
        for (var i = 1; i <= 40; i++) {
          var s2 = {};
          for (var k in summary) s2[k] = summary[k];
          s2.fileName = "填充投影" + i + ".litematic";
          s2.path = "C:/测试目录/填充投影" + i + ".litematic";
          s2.name = "填充投影" + i;
          many.push(s2);
        }
        return Promise.resolve({ items: many, skippedDirs: 0, skippedPaths: [] });
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
      // 各功能对话框打开时会调的命令，必须回合法结构，
      // 否则前端会走到 catch 分支、对话框内一片空白
      case "convert_targets": return Promise.resolve([
        { index: 0, name: "1.20.1", dataVersion: 3465 },
        { index: 1, name: "1.19.4", dataVersion: 3337 },
      ]);
      case "scan_duplicates": return Promise.resolve({ groups: [], unreadable: [] });
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

  // 自动走一遍真实交互：点「添加目录」让列表出来，再点第一行让详情面板出来，
  // 最后依次打开五个功能对话框。
  //
  // 为什么要逐个开对话框：打开时的加载逻辑（读取可转换版本、资源包清单等）
  // 一旦抛错，界面表现就是「点了没反应」——正是最难排查的一类问题。
  // 只截主界面看不到这些。
  //
  // 判据用「正文里是否出现文件名 / 数据版本」，不能用 `tbody tr`：
  // Arco 的空状态也渲染成一个 tr，用它会误判成「已有数据」而停止，
  // 结果一次都不点（上面这两个判据是实测踩出来的）。
  var tries = 0, state = "wait", clicked = false;

  // 布局几何：确认左侧目录栏撑到底、表格内部可滚动、拖动条存在。
  //
  // 为什么必须量几何而不是看源码：这三个问题（底部留白、无滚动条、
  // 拖动条不可用）在「构建成功、界面也渲染出来了」的情况下全都照样存在，
  // 只有真实浏览器里的盒子尺寸能判定。
  function measureLayout() {
    var q = function (s) { return document.querySelector(s); };
    var sb = q(".sidebar"), wb = q(".workbench"), sp = q(".splitter"), dt = q(".detail");
    if (!sb || !wb || !sp || !dt) return "LAYOUT:missing";
    var rSb = sb.getBoundingClientRect(), rWb = wb.getBoundingClientRect();
    var rDt = dt.getBoundingClientRect();
    // 表格滚动容器：Arco 的纵向滚动落在 .arco-table-body 上
    var sc = sb.querySelector(".arco-table-body") || sb.querySelector(".arco-table-container");
    var canScroll = sc ? (sc.scrollHeight > sc.clientHeight + 1) : false;
    var scCls = sc ? String(sc.className).split(" ")[0] : "none";
    // 目录栏底边与工作区/详情栏底边之差：应为 0
    var gapWb = Math.round(rWb.bottom - rSb.bottom);
    var gapDt = Math.round(rDt.bottom - rSb.bottom);
    // 目录栏宽度，供拖动测试比对
    var wBefore = Math.round(rSb.width);
    return "LAYOUT:"
      + "gapWb=" + gapWb
      + ",gapDt=" + gapDt
      + ",canScroll=" + canScroll
      + ",scCls=" + scCls
      + ",w=" + wBefore
      + ",spW=" + Math.round(sp.getBoundingClientRect().width)
      + ",spCursor=" + window.getComputedStyle(sp).cursor
      + ",spRole=" + (sp.getAttribute("role") || "-")
      + ",spTab=" + (sp.getAttribute("tabindex") || "-");
  }

  var layoutResult = "";
  var dragState = null;

  // 用真实指针事件拖动分隔条，确认宽度确实随之改变。
  //
  // 分两拍：pointerup 之后不能立刻量宽度。Vue 对 ref 的 DOM 更新是异步的，
  // 同一次任务里读到的仍是改动前的宽度（第一版就是这样量出 Δ0 的假失败）。
  function startDrag(dx) {
    var sp = document.querySelector(".splitter");
    var sb = document.querySelector(".sidebar");
    if (!sp || !sb) { dragState = "none"; return; }
    var before = Math.round(sb.getBoundingClientRect().width);
    var box = sp.getBoundingClientRect();
    var x = box.left + box.width / 2, y = box.top + 40;
    var mk = function (type, cx) {
      return new PointerEvent(type, {
        bubbles: true, cancelable: true, composed: true,
        pointerId: 1, pointerType: "mouse", isPrimary: true,
        button: type === "pointerdown" ? 0 : -1, buttons: type === "pointerup" ? 0 : 1,
        clientX: cx, clientY: y
      });
    };
    sp.dispatchEvent(mk("pointerdown", x));
    // 移到 window 上：实现把 pointermove 挂在 window，
    // 这条路径同时验证了「拖出窄条后仍继续」这一要点
    window.dispatchEvent(mk("pointermove", x + dx));
    window.dispatchEvent(mk("pointerup", x + dx));
    // 拖动应已结束：再发一次移动不应继续改变宽度
    window.dispatchEvent(mk("pointermove", x + dx + 200));
    dragState = { before: before, x: x };
  }

  // 下一拍量结果
  function finishDrag() {
    var sb = document.querySelector(".sidebar");
    if (!sb || !dragState || dragState === "none") return "DRAG:none";
    var after = Math.round(sb.getBoundingClientRect().width);
    var r = "DRAG:before=" + dragState.before + ",after=" + after
          + ",delta=" + (after - dragState.before)
          + ",movedAfterUp=" + dragState.movedAfterUp;
    dragState = null;
    return r;
  }

  function status() {
    document.title = "CALLS:" + CALLS.join("|")
      + " SCAN:" + state + " DIALOGS:" + DIALOGS.join(",") + " tries=" + tries
      + " " + (layoutResult || "LAYOUT:pending");
  }
  status();
  var timer = setInterval(function () {
    tries++;
    var body = document.body ? document.body.textContent || "" : "";
    var hasFile = body.indexOf("28速单区块简单熔炉") >= 0;
    var hasDetail = body.indexOf("3465") >= 0;

    if (!hasFile) {
      var btn = Array.prototype.slice.call(document.querySelectorAll("button"))
        .filter(function (b) { return (b.textContent || "").indexOf("添加目录") >= 0; })[0];
      if (btn && !clicked) { clicked = true; btn.click(); state = "clicked"; }
      else if (!btn) { state = "no-button"; }
      status();
    } else if (!hasDetail) {
      var row = Array.prototype.slice.call(document.querySelectorAll("tbody tr"))
        .filter(function (r) { return (r.textContent || "").indexOf("28速单区块简单熔炉") >= 0; })[0];
      if (row && state !== "row-clicked") { row.click(); state = "row-clicked"; }
      else if (!row) { state = "no-row"; }
      status();
    } else if (!dialogsDone) {
      // 分两步：先点开，**等到下一个 tick** 再核对。
      // Vue 的 DOM 更新是异步的，点完立刻查询时弹层还没插进文档，
      // 会把正常打开的对话框误判成「标题缺失」（实测三个被误判）。
      if (pendingCheck) {
        // 判据：文档里是否出现了该对话框的标题元素，**且它确实显示着**。
        //
        // 三个坑（都实测踩到）：
        //  1. 关掉的 Arco 弹层仍留在文档里，容器带 `display: none`。
        //     只查标题元素会被「已关闭的弹层」命中——把打开逻辑改成一进来就
        //     throw，检查照样通过（这正是第三次变异没被抓到的原因）。
        //     因此必须沿祖先链确认没有被 display:none 挡住。
        //  2. 不能在整页正文里找标题：「备份管理」等四个标题与工具栏按钮同名，
        //     按钮在任何情况下都在，等于永远命中。
        //  3. 「备份管理」是 a-drawer（抽屉）不是 a-modal，标题在
        //     `.arco-drawer-title` 里，只查 modal 会把它判成「未出现」。
        function shown(node) {
          var n = node;
          while (n && n !== document.body) {
            var st = window.getComputedStyle(n);
            if (st.display === "none" || st.visibility === "hidden") return false;
            n = n.parentElement;
          }
          return true;
        }
        var titleEls = document.querySelectorAll(".arco-modal-title, .arco-drawer-title");
        var hit = false;
        for (var ti = 0; ti < titleEls.length; ti++) {
          var t = titleEls[ti];
          if ((t.textContent || "").indexOf(DIALOG_TITLES[pendingCheck]) >= 0 && shown(t)) {
            hit = true;
          }
        }
        DIALOGS.push(pendingCheck + (hit ? "=开" : "=未出现"));
        // 关掉：点关闭按钮，否则下一个对话框会被叠住点不到
        var closeBtns = document.querySelectorAll(
          ".arco-modal-close-btn, .arco-drawer-close-btn");
        if (closeBtns.length) closeBtns[closeBtns.length - 1].click();
        pendingCheck = null;
        status();
        return;
      }

      // 依次打开每个功能对话框；打开后立刻关闭，再看下一个
      var pending = DIALOG_BUTTONS.filter(function (n) { return !dialogTried.includes(n); })[0];
      if (!pending) {
        dialogsDone = true;
        state = "dialog-checked";
        // 对话框全部核对完，主界面已稳定：此时量布局最可靠
        layoutResult = measureLayout();
        // 拖动分两拍：这一拍发起，下一拍量结果
        startDrag(120);
        status();
        return;
      }
      dialogTried.push(pending);
      var dlg = Array.prototype.slice.call(document.querySelectorAll("button"))
        .filter(function (b) { return (b.textContent || "").indexOf(pending) >= 0; })[0];
      if (!dlg) {
        DIALOGS.push(pending + "=未找到按钮");
        status();
        return;
      }
      if (dlg.disabled) {
        DIALOGS.push(pending + "=按钮禁用");
        status();
        return;
      }
      dlg.click();
      pendingCheck = pending;
      status();
    } else if (dragState) {
      // 上一拍完成了对话框核对并发起了拖动：现在量结果。
      // 多等一拍是必须的——Vue 的 DOM 更新在微任务里，同拍读到的是旧宽度。
      layoutResult += " " + finishDrag();
      status();
      clearInterval(timer);
    } else {
      clearInterval(timer);
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
        # 窗口尺寸取用户实际使用的大小（1827x1190）：目录栏需要「撑到底」与
        # 「出现滚动条」这两点都和可视高度直接相关，用偏小的窗口量不出真实表现。
        shot = subprocess.run(
            common + ["--hide-scrollbars", "--window-size=1827,1190",
                      f"--screenshot={SHOT}", url],
            capture_output=True, timeout=180,
        )
        print(f"截图退出码 {shot.returncode}")

        # 尺寸必须与截图一致：布局几何是在这一趟里量的，
        # 用默认的 800x600 会让「撑到底」「出现滚动条」都量成错的
        dump = subprocess.run(common + ["--window-size=1827,1190", "--dump-dom", url],
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
        shim_state = m.group(1).strip() if m else "(无)"
        print(f"替身状态: {shim_state}")

        # 逐个核对功能对话框是否真的打开了。
        # 标题形如 `... DIALOGS:方块替换=开,版本转换=开 tries=8`，
        # 末项会带上后面的 tries，必须先把这段元信息切掉，
        # 否则最后一项的结论会变成「开 tries=8」而被判为失败。
        dialogs = re.search(r"DIALOGS:([^S]*)", shim_state)
        dlg_text = dialogs.group(1).strip() if dialogs else ""
        dlg_text = re.split(r"\s+tries=", dlg_text)[0]
        dlg_results = [d.strip() for d in dlg_text.split(",") if d.strip()]
        print("\n=== 功能对话框能否打开 ===")
        dlg_extra = []
        for d in dlg_results:
            name, _, verdict = d.partition("=")
            ok_d = verdict == "开"
            dlg_extra.append(ok_d)
            print(f"  {name:<14} {verdict if verdict else '(未记录)'}")
        # 五个对话框都应有记录，且都必须真的渲染出标题
        if len(dlg_results) != 5:
            dlg_extra.append(False)
            print(f"  只有 {len(dlg_results)} 个对话框被检查，应为 5 个")

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

        # ---- 布局几何：左侧目录栏的撑高、滚动与拖动 ----
        print("\n=== 左侧目录栏布局 ===")
        lay = re.search(r"LAYOUT:([^\s<]*)", visible)
        dra = re.search(r"DRAG:([^\s<]*)", visible)
        layout_ok = True

        def claim(label, cond, detail=""):
            nonlocal layout_ok
            layout_ok = layout_ok and cond
            print(f"  {label:<34} {'是' if cond else '否'}{detail}")

        if not lay or lay.group(1) == "pending" or lay.group(1) == "missing":
            print(f"  未能取到布局数据：{lay.group(1) if lay else '(无)'}")
            layout_ok = False
        else:
            fields = {}
            for part in lay.group(1).split(","):
                k, _, v = part.partition("=")
                fields[k] = v
            gap_wb = int(fields.get("gapWb", "9999"))
            gap_dt = int(fields.get("gapDt", "9999"))
            # 目录栏底边应与工作区/详情栏底边齐平（容 1px 边框取整误差）
            claim("目录栏延伸到底部（无空白）", abs(gap_wb) <= 1, f"  与工作区差 {gap_wb}px")
            claim("目录栏与右侧栏等高", abs(gap_dt) <= 1, f"  与详情栏差 {gap_dt}px")
            claim("表格内部出现纵向滚动", fields.get("canScroll") == "true",
                  f"  容器 {fields.get('scCls')}")
            claim("拖动条存在且有宽度", int(fields.get("spW", "0")) > 0,
                  f"  宽 {fields.get('spW')}px")
            claim("拖动条光标为 col-resize", fields.get("spCursor") == "col-resize")
            claim("拖动条可聚焦（键盘可用）", fields.get("spTab") == "0",
                  f"  role={fields.get('spRole')} tabindex={fields.get('spTab')}")
            print(f"  目录栏宽度 {fields.get('w')}px")

        if not dra or dra.group(1) == "none":
            print("  未能取到拖动结果")
            layout_ok = False
        else:
            df = {}
            for part in dra.group(1).split(","):
                k, _, v = part.partition("=")
                df[k] = v
            delta = int(df.get("delta", "0"))
            # 向右拖 120px，宽度应确实增加（夹取范围内应接近 120）
            claim("拖动能改变目录栏宽度", delta > 50,
                  f"  {df.get('before')}px -> {df.get('after')}px（Δ{delta}）")

        results.append(layout_ok)

        ok = all(results) and all(dlg_extra)
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
