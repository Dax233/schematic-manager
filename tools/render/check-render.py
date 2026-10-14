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

# 列表条数的两轮取值。
#   多行：验证表体撑到底后仍能纵向滚动
#   少行：验证内容少时表体不收缩（Arco 内层不继承父高度，只有少行才暴露）
# 用同一个条数会漏掉其中一类问题，因此两轮都跑。
FILLER_ROWS = 40
FEW_ROWS = 3

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
  // 一个朝向相机的**单片四边形**，用于核验几何真的被画出来了。
  //
  // 为什么不用立方体：立方体测不出绕序问题。若六面绕序全反，近面被
  // FrontSide 剔除，但**远面**此时正好正面朝向相机而被画出，轮廓与颜色
  // 几乎不变——实测两种绕序的不透明像素数完全相同（4857）。
  // 单片四边形没有「远面」可替代：绕序正确才可见，反了就是 0 个像素，
  // 信号明确。
  //
  // 相机由 fitCamera 放在 (d,d,d) 方向（见 SceneViewer 与 scene3d.ts），
  // 因此四边形取 XY 平面、法线 +Z，保证相机在它的正面一侧。
  //
  // 不能再用空 batches：空场景没有三角形，永远画出 0 个像素，
  // 「绕序反了 → 面被剔除 → 看穿方块」这类几何缺陷完全测不到——
  // 上一版正是空桩，因此渲染检查全绿而用户实际看到透视方块。
  function quadScene() {
    // 角点顺序按「与后端相同的索引规则能得出 +Z 法线」来定。
    //
    // 这里刻意沿用后端那条规则（0-2-1 / 0-3-2），而不是另写一套「看起来
    // 逆时针」的顺序：若两边规则不同，后端改绕序时这个替身不会跟着
    // 失效，检查就失去意义。下面的角点顺序是让该规则产出 +Z 法线的唯一
    // 排法（cross(c2-c0, c1-c0) = +Z）。
    var v = [[0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]];
    var pos = [], uv = [], sh = [], idx = [];
    for (var k = 0; k < 4; k++) {
      pos.push(v[k][0], v[k][1], v[k][2]);
      uv.push(k === 0 || k === 3 ? 0 : 16, k < 2 ? 0 : 16);
      sh.push(1.0, 1.0, 1.0);
    }
    // 与后端 mesh::build 相同的索引规则
    idx.push(0, 2, 1, 0, 3, 2);
    // 4×4 纯色贴图（不透明橙），便于按像素判断画的是它
    var rgba = [];
    for (var i = 0; i < 4 * 4; i++) rgba.push(255, 128, 0, 255);
    return {
      batches: [{
        texture: 0,
        positions: pos, uvs: uv, colors: sh, indices: idx,
        textureWidth: 4, textureHeight: 4, textureRgba: rgba
      }],
      totalVertices: 4,
      totalTriangles: 2,
      enclosing: [1, 1, 0], origin: [0, 0, 0],
      mcVersionName: "1.20.1", dataVersion: 3465, unresolvedBlocks: 0
    };
  }
  var scene = quadScene();
  var analysis = {
    blocks: [], entities: [], tileEntities: [],
    nonAirTotal: 1234, airTotal: 1838,
    distinctBlockTypes: 18, distinctStates: 26
  };
  var CALLS = [];

  // 列表条数由检查脚本按需注入：内容少时才是「表体收缩」暴露的场景，
  // 内容多时才是「能滚动」的场景，两轮各测一次。
  var FILLER_ROWS = window.__SM_FILLER_ROWS__;
  if (typeof FILLER_ROWS !== "number") FILLER_ROWS = 40;

  // 要逐个打开的对话框，以及各自打开后应出现的标题。
  // 打开对话框会触发各自的加载逻辑（读取可转换版本、资源包清单、备份列表），
  // 这些一旦抛错，界面表现就是「点了没反应」。
  // 注意：「重复文件筛查」**不在**此列——按用户要求它已改为内联扫描
  // 地址栏里已勾选的目录，不再弹窗。留在列表里会让脚本一直等一个
  // 永远不出现的对话框。
  var DIALOG_BUTTONS = ["方块替换", "版本转换", "备份管理", "资源包管理"];
  var DIALOG_TITLES = {
    "方块替换": "方块替换",
    "版本转换": "转换游戏版本",
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
    // 交给 status() 统一写标题，不要在这里直接覆盖。
    //
    // 直接写 "CALLS:..." 会把 status() 写的 SCAN/DIALOGS/tries 一起冲掉，
    // 而检查脚本正靠那段判断流程走到了哪一步。以前恰好是 status() 最后写，
    // 所以没暴露；新增的 save_ui_state 会在最后再调一次 invoke，
    // 于是标题只剩 CALLS，脚本读不到对话框结论（实测 0 个对话框被检查）。
    // status 是同一 IIFE 里的函数声明，会被提升，这里可以安全调用。
    if (typeof status === "function") status();
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
        for (var i = 1; i <= FILLER_ROWS; i++) {
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
      // 界面状态：契约是 string | null。默认「从未保存过」，
      // 回数组会让 App 走进 JSON 解析失败分支并弹出警告（替身类型给错）。
      case "load_ui_state": return Promise.resolve(null);
      case "save_ui_state": return Promise.resolve(undefined);
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

    // 表体是否撑到底。
    //
    // 必须单独量这一层：外层 .sidebar / .arco-table 即使撑到底，
    // Arco 内层的 .arco-table-container 也不继承父高度，会按内容收缩。
    // 「内容少」时才暴露——外层全对而表体只有 370px，下面一大片空白。
    // 只量外层会得出「全部正常」的错误结论，因此这里量最内层表体。
    //
    // 目录栏底部还有一条多选操作条，因此「该到哪」不是 sidebar 底边，
    // 而是那条操作条的上边。直接和 sidebar 底边比会把操作条本身的高度
    // 算成空白（实测差 34px），得到假失败。
    var bodyEl = sb.querySelector(".arco-table-body");
    var footEl = sb.querySelector(".sidebar__checks");
    var contentBottom = footEl
      ? footEl.getBoundingClientRect().top
      : rSb.bottom;
    var gapBody = bodyEl
      ? Math.round(contentBottom - bodyEl.getBoundingClientRect().bottom)
      : 9999;
    var bodyH = bodyEl ? Math.round(bodyEl.getBoundingClientRect().height) : 0;
    // 表体高度应约等于「目录栏高 - 表头高」，而不是内容高度
    var minBodyH = Math.round(rSb.height * 0.5);

    // 底部操作条必须与目录栏底边齐平（否则它下面会留空白）
    var gapFoot = footEl
      ? Math.round(rSb.bottom - footEl.getBoundingClientRect().bottom)
      : 0;

    // ---- 表头竖向分割线 ----
    // 没有分割线时，列宽虽可拖却看不出各列边界在哪。逐列取 border-right-width：
    // 除最后一列外都应有 1px（最后一列右边就是容器边框，两条线叠一起会显粗）。
    var ths = sb.querySelectorAll(".arco-table-header th");
    var dividerOk = 0;
    for (var i = 0; i < ths.length; i++) {
      var bw = parseFloat(window.getComputedStyle(ths[i]).borderRightWidth) || 0;
      var isLast = i === ths.length - 1;
      if (isLast ? bw === 0 : bw > 0) dividerOk++;
    }

    // ---- 列宽拖动与复位 ----
    // 拖动后名称列应明显变宽；点复位后应回到原宽。
    // 复位这一点过去是坏的：Arco 把拖动结果记在自己的 columnWidth 里，
    // 优先级高于 columns.width（table-col-group.js: columnWidth[d] || item.width），
    // 因此外部清状态改不动它，必须重建组件。
    var colW = function () {
      var t = sb.querySelectorAll(".arco-table-header th"), a = [];
      for (var j = 0; j < t.length; j++) a.push(Math.round(t[j].getBoundingClientRect().width));
      return a;
    };
    var wCols = colW();
    var handle = sb.querySelector(".arco-table-column-handle");
    var dragged = false;
    if (handle) {
      var hb = handle.getBoundingClientRect();
      var hx = Math.round(hb.left + hb.width / 2), hy = Math.round(hb.top + hb.height / 2);
      handle.dispatchEvent(new MouseEvent("mousedown", {
        bubbles: true, cancelable: true, clientX: hx, clientY: hy }));
      window.dispatchEvent(new MouseEvent("mousemove", {
        bubbles: true, cancelable: true, clientX: hx + 90, clientY: hy }));
      window.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, cancelable: true }));
      dragged = true;
    }

    // 表头是否被横向挤出可视区。
    // 名称列加宽后各列合计可能超过目录栏宽度，最后一列会被截到外面，
    // 用户反而「看不到更多内容」——与加宽的目的相反，因此必须检查。
    //
    // 注意：拖动句柄有 4~8px 宽且绝对定位，可能让 scrollWidth 略大于
    // clientWidth。那种情况不是「列被挤出」，因此用「列宽合计」判断，
    // 而不是直接看是否有滚动条。
    //
    // 表头与表体各有一个 colgroup，因此**只取第一个**：两个都加会把
    // 合计算成两倍（实测 1626px 而非 813px），得到假失败。
    var colGroup = sb.querySelector(".arco-table-header colgroup");
    var cols = (colGroup || sb).querySelectorAll("col");
    var declSum = 0;
    for (var ci = 0; ci < cols.length; ci++) {
      var cm = /width:\s*([\d.]+)px/.exec(cols[ci].getAttribute("style") || "");
      if (cm) declSum += Number(cm[1]);
    }
    var boxW = Math.round(rSb.width);
    // 留 2px 取整余量
    var fits = declSum <= boxW + 2;

    // ---- 3D 视图真的画出东西了吗 ----
    // 这一段防的是「几何绕序反了 → 所有面被 FrontSide 剔除 → 看穿方块」：
    // 那种情况下布局全对、画布尺寸也对，只有像素是空的。
    // 因此直接读画布的像素：不透明像素应占相当比例（实心立方体的轮廓）。
    var pix = { opaque: 0, total: 0, ratio: 0 };
    var cv = sb ? document.querySelector(".scene-viewer canvas") : null;
    if (cv) {
      try {
        var gl = cv.getContext("webgl2") || cv.getContext("webgl");
        if (gl) {
          var w = cv.width, h = cv.height;
          var buf = new Uint8Array(w * h * 4);
          gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, buf);
          var op = 0;
          for (var pi = 3; pi < buf.length; pi += 4) {
            if (buf[pi] > 200) op++;
          }
          pix = { opaque: op, total: w * h, ratio: +(op / (w * h)).toFixed(4) };
        }
      } catch (e) {
        pix = { opaque: -1, total: -1, ratio: -1, err: String(e) };
      }
    }

    // ---- 预览区尺寸与是否越界 ----
    // 要求：靠左、200×200、渲染图形不得超出范围。
    // 画布曾实测 987px 而容器只有 862px：three.js 的 setSize 只改 canvas 的
    // width/height 属性、不改 CSS 尺寸，未约束时它就按属性尺寸撑出去。
    var stage = document.querySelector(".preview__stage");
    var cvEl = document.querySelector(".scene-viewer canvas");
    var stBox = stage ? stage.getBoundingClientRect() : null;
    var cvBox = cvEl ? cvEl.getBoundingClientRect() : null;
    var previewOk = false, canvasOverflow = false;
    if (stBox && cvBox) {
      // 预览舞台改为自适应：核对它是正方形、且明显大于原来的 400（说明真的放大了）
      previewOk = Math.abs(Math.round(stBox.width) - Math.round(stBox.height)) <= 1
        && Math.round(stBox.width) >= 400;
      canvasOverflow =
        cvBox.left < stBox.left - 1 || cvBox.right > stBox.right + 1 ||
        cvBox.top < stBox.top - 1 || cvBox.bottom > stBox.bottom + 1;
    }

    // 保存按钮必须无需滚动就能点到。
    // 信息面板与预览舞台同为 400px，而「基本信息 + 三个输入框 + 多行描述」
    // 实测 485px：操作行若跟着内容一起滚，按钮会落到折叠线以下 113px，
    // 用户得先滚动才能保存——等于按钮藏起来了。
    var actEl = document.querySelector(".preview__actions");
    var actBtn = actEl ? actEl.querySelector("button") : null;
    var infoEl2 = document.querySelector(".preview__info");
    var saveOk = false, saveGap = -9999;
    if (actBtn && infoEl2) {
      var ab = actBtn.getBoundingClientRect(), ib = infoEl2.getBoundingClientRect();
      saveGap = Math.round(ab.bottom - ib.bottom);
      saveOk = ab.height > 0 && ab.bottom <= ib.bottom + 1 && ab.top >= ib.top;
    }

    // ---- 方块栏（用户要求：中文名/列宽调整/明显分割线/自适应到最低位置/导出表格）----
    // 先切到「方块」页签，否则量的是「信息」页签
    var blockTab = null;
    var tabTitles = document.querySelectorAll(".detail__tabs .arco-tabs-header-title");
    for (var ti = 0; ti < tabTitles.length; ti++) {
      if ((tabTitles[ti].textContent || "").trim() === "方块") { blockTab = tabTitles[ti]; break; }
    }
    if (blockTab) blockTab.click();
    var bTable = document.querySelector(".block-table");
    var bPane = bTable ? bTable.closest(".arco-tabs-pane") : null;
    var bThs = bTable ? bTable.querySelectorAll("thead th") : [];
    var bBorder = 0;
    for (var bi = 0; bi < bThs.length; bi++) {
      if ((parseFloat(getComputedStyle(bThs[bi]).borderRightWidth) || 0) > 0) bBorder++;
    }
    var bBox = bTable ? bTable.getBoundingClientRect() : null;
    var bPaneBox = bPane ? bPane.getBoundingClientRect() : null;
    // 表格底边距页签底边：0 表示真的撑到底
    var bGap = (bBox && bPaneBox) ? Math.round(bPaneBox.bottom - bBox.bottom) : -9999;
    var bHandles = bTable ? bTable.querySelectorAll(".arco-table-column-handle").length : 0;
    var bExport = false;
    var bCopy = false;
    var bBtns = document.querySelectorAll(".block-head button");
    for (var bj = 0; bj < bBtns.length; bj++) {
      var bt = (bBtns[bj].textContent || "").trim();
      if (bt.indexOf("导出表格") >= 0) bExport = true;
      if (bt.indexOf("复制表格") >= 0) bCopy = true;
    }

    // ---- 新界面要素 ----
    // 1) 选中行是否有高亮类（用户要求点左侧文件时该条目变蓝）
    var selRow = sb.querySelector("tbody tr.row-selected");
    var selOk = !!selRow;
    // 2) 信息区是否在预览舞台**右侧**（用户要求信息显示在预览图右边）
    var infoEl = document.querySelector(".preview__info");
    var infoBox = infoEl ? infoEl.getBoundingClientRect() : null;
    var infoRight = !!(infoBox && stBox && infoBox.left >= stBox.right - 2);
    // 3) 描述框是否为多行 textarea（用户要求文本框大一些方便多行）
    var descArea = document.querySelector(".preview__form textarea");
    var descRows = descArea ? Number(descArea.getAttribute("rows") || 0) : 0;
    var descH = descArea ? Math.round(descArea.getBoundingClientRect().height) : 0;
    // 4) 描述标签是否带（备注）注明
    var labelOk = (document.querySelector(".preview__form") || {}).textContent
      ? document.querySelector(".preview__form").textContent.indexOf("（备注）") >= 0 : false;

    // ---- 预览控制条是否被挤压 ----
    // 控制条内容合计约 840px，不换行时 flex 会把单选按钮压到 47px 宽，
    // 导致「正面」竖排成两行（高度 44px）。这里用按钮宽高判断。
    var barBtn = document.querySelector(".preview__bar .arco-radio-button");
    var barBox = barBtn ? barBtn.getBoundingClientRect() : null;
    var barOk = barBox ? Math.round(barBox.height) <= 30 : false;
    // 控制条应紧贴舞台底部：信息列比舞台高时若任其撑开，控制条会被推下去，
    // 预览与按钮之间留下大片空白（实测被推到 637px vs 舞台底 484px）。
    var barEl = document.querySelector(".preview__bar");
    var stageBottom = stBox ? stBox.bottom : 0;
    var gapBar = barEl ? Math.round(barEl.getBoundingClientRect().top - stageBottom) : 9999;

    return "LAYOUT:"
      + "gapWb=" + gapWb
      + ",gapDt=" + gapDt
      + ",canScroll=" + canScroll
      + ",scCls=" + scCls
      + ",w=" + wBefore
      + ",spW=" + Math.round(sp.getBoundingClientRect().width)
      + ",spCursor=" + window.getComputedStyle(sp).cursor
      + ",spRole=" + (sp.getAttribute("role") || "-")
      + ",spTab=" + (sp.getAttribute("tabindex") || "-")
      + ",gapBody=" + gapBody
      + ",bodyH=" + bodyH
      + ",minBodyH=" + minBodyH
      + ",gapFoot=" + gapFoot
      + ",stageW=" + (stBox ? Math.round(stBox.width) : -1)
      + ",stageH=" + (stBox ? Math.round(stBox.height) : -1)
      + ",stageLeft=" + (stBox ? Math.round(stBox.left) : -1)
      + ",detailLeft=" + Math.round(rDt.left)
      + ",canvasW=" + (cvBox ? Math.round(cvBox.width) : -1)
      + ",previewOk=" + previewOk
      + ",canvasOverflow=" + canvasOverflow
      + ",barOk=" + barOk
      + ",gapBar=" + gapBar
      + ",selOk=" + selOk
      + ",bTh=" + bThs.length
      + ",bBorder=" + bBorder
      + ",bHandles=" + bHandles
      + ",bGap=" + bGap
      + ",bExport=" + bExport
      + ",bCopy=" + bCopy
      + ",saveOk=" + saveOk
      + ",saveGap=" + saveGap
      + ",infoRight=" + infoRight
      + ",infoLeft=" + (infoBox ? Math.round(infoBox.left) : -1)
      + ",stageRight=" + (stBox ? Math.round(stBox.right) : -1)
      + ",descH=" + descH
      + ",descRows=" + descRows
      + ",labelOk=" + labelOk
      + ",barBtnH=" + (barBox ? Math.round(barBox.height) : -1)
      + ",barBtnW=" + (barBox ? Math.round(barBox.width) : -1)
      + ",pixOpaque=" + pix.opaque
      + ",pixTotal=" + pix.total
      + ",pixRatio=" + pix.ratio
      + ",pixErr=" + (pix.err ? String(pix.err).slice(0, 40) : "-")
      + ",declSum=" + declSum
      + ",boxW=" + boxW
      + ",fits=" + fits
      + ",thCount=" + ths.length
      + ",dividerOk=" + dividerOk
      + ",handleCount=" + sb.querySelectorAll(".arco-table-column-handle").length
      + ",tdFont=" + (function () {
          var td = sb.querySelector(".arco-table-td");
          return td ? window.getComputedStyle(td).fontSize : "";
        })()
      + ",detailFont=" + (function () {
          var el = document.querySelector(".detail .arco-table-td");
          return el ? window.getComputedStyle(el).fontSize : "";
        })()
      + ",dragged=" + dragged
      + ",wCols0=" + wCols.join("|");
  }

  /** 测列宽拖动与复位。自包含：自己记录拖动前宽度作为基准，
   *  不依赖其它步骤的采样——目录栏一被拖动，所有列宽都会重算，
   *  跨步骤比较必然得到假失败。 */
  function resetWidthsAndMeasure() {
    var sb = document.querySelector(".sidebar");
    var read = function () {
      var t = sb.querySelectorAll(".arco-table-header th"), a = [];
      for (var j = 0; j < t.length; j++) a.push(Math.round(t[j].getBoundingClientRect().width));
      return a.join("|");
    };
    var base = read();

    // 用真实表头拖动句柄把名称列拖宽。
    //
    // mousedown 必须打在**名称列**的手柄上（第一个句柄属于名称列），
    // 并且要等一拍再量：Arco 的 handleThMouseMoving 挂在 window 上，
    // 但列宽变化经 Vue 渲染是异步的。
    var handles = sb.querySelectorAll(".arco-table-column-handle");
    var handle = handles.length ? handles[0] : null;
    // mousedown 必须打在**名称列**的手柄上（第一个句柄属于名称列）。
    //
    // 宽度必须在**之后**的拍子里读：列宽变化经 Vue 渲染是异步的，
    // 同拍读到仍是拖动前的值（实测得到「352 -> 352」的假失败）。
    var handles = sb.querySelectorAll(".arco-table-column-handle");
    var handle = handles.length ? handles[0] : null;
    if (handle) {
      var hb = handle.getBoundingClientRect();
      var hx = Math.round(hb.left + hb.width / 2), hy = Math.round(hb.top + hb.height / 2);
      handle.dispatchEvent(new MouseEvent("mousedown", {
        bubbles: true, cancelable: true, button: 0, clientX: hx, clientY: hy }));
      window.dispatchEvent(new MouseEvent("mousemove", {
        bubbles: true, cancelable: true, button: 0, clientX: hx + 90, clientY: hy }));
      window.dispatchEvent(new MouseEvent("mouseup", {
        bubbles: true, cancelable: true, button: 0, clientX: hx + 90, clientY: hy }));
    }

    var btns = sb.querySelectorAll("button"), reset = null;
    for (var k = 0; k < btns.length; k++) {
      if ((btns[k].textContent || "").indexOf("列宽复位") >= 0) reset = btns[k];
    }
    if (!reset) return Promise.resolve("RESET:no-button");

    return new Promise(function (done) {
      // 第一拍：读拖动后的宽度（此时 Vue 已渲染）
      setTimeout(function () {
        var dragged = read();
        reset.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
        // 第二拍：读复位后的宽度（重建组件同样是异步的）
        setTimeout(function () {
          done("RESET:base=" + base + ",dragged=" + dragged + ",afterReset=" + read());
        }, 80);
      }, 80);
    });
  }

  var layoutResult = "";
  var dragState = null;
  /** 列宽核对只跑一次：pending 为空会一直是空，需要显式守卫 */
  var widthChecked = false;

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
    // 再发一次移动，验证拖动已结束。
    //
    // 这里**不**在本拍量宽度：Vue 的更新是异步的，同拍读到的仍是拖动前的值，
    // 拿两次同样过期的值比较会「通过得毫无意义」。松手后不再跟随这一条
    // 由前端单元测试（「拖动会改变宽度，且 pointerup 之后不再跟随」）
    // 逐拍量取验证，这里只负责确认真实布局下的拖动效果。
    window.dispatchEvent(mk("pointermove", x + dx + 200));
    dragState = { before: before, x: x };
  }

  // 下一拍量结果
  function finishDrag() {
    var sb = document.querySelector(".sidebar");
    if (!sb || !dragState || dragState === "none") return "DRAG:none";
    var after = Math.round(sb.getBoundingClientRect().width);
    var r = "DRAG:before=" + dragState.before + ",after=" + after
          + ",delta=" + (after - dragState.before);
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
        // 只执行一次：pending 在所有对话框试完后会一直是 undefined，
        // 不加守卫的话每到一拍都会重跑一遍（会反复重置拖动态）。
        //
        // 注意守卫**不能直接 return 到底**：列宽核对是异步的，
        // 它 resolve 之后会调 startDrag 设好 dragState，
        // 那次结果要由下面的 dragState 分支来量。直接 return 会让
        // 拖动态永远没人处理，表现就是「未能取到拖动结果」。
        if (widthChecked) {
          if (dragState) {
            layoutResult += " " + finishDrag();
            status();
            clearInterval(timer);
          }
          return;
        }
        widthChecked = true;
        state = "dialog-checked";
        // 对话框全部核对完，主界面已稳定：此时量布局最可靠。
        // 先测列宽拖动与复位，此时目录栏还是默认宽度。
        //
        // 顺序很重要：若先拖目录栏分隔条，列宽会因重新分配而全部变化，
        // 之后与「复位后」比较就得到假失败（实测复位后 405px vs 默认 352px，
        // 差别全部来自目录栏变宽，而不是复位没生效）。
        layoutResult = measureLayout();
        resetWidthsAndMeasure().then(function (r) {
          layoutResult += " " + r;
          // 最后拖动目录栏分隔条，同样分两拍量结果
          startDrag(120);
          status();
        });
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

    def make_page(rows):
        """生成一份带替身的页面，并注入列表条数。

        条数必须可变：内容少时才是「表体收缩、底部留白」暴露的场景；
        内容多时才是「表体可滚动」的场景。固定用其中一个条数会漏掉另一类问题。
        """
        html = index.replace("<head>", "<head>" + SHIM, 1) if "<head>" in index else SHIM + index
        inject = f"<script>window.__SM_FILLER_ROWS__={rows};</script>"
        return html.replace("<head>", "<head>" + inject, 1) if "<head>" in html else inject + html

    html = make_page(FILLER_ROWS)
    render_path = os.path.join(DIST, PAGE_NAME)
    with open(render_path, "w", encoding="utf-8") as f:
        f.write(html)

    # 「内容少」那一轮的页面：单独一份，避免与主页面互相干扰
    FEW_PAGE = "__few__.html"
    few_path = os.path.join(DIST, FEW_PAGE)
    with open(few_path, "w", encoding="utf-8") as f:
        f.write(make_page(FEW_ROWS))

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
            "--virtual-time-budget=20000",
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
        # 四个对话框都应有记录，且都必须真的渲染出标题。
        # 数量必须与 DIALOG_BUTTONS 一致：写死数字时改了列表会漏改这里，
        # 表现是「明明都开了却报数量不符」。
        # DIALOG_BUTTONS 定义在注入页面的 JS 里，Python 侧取不到，
        # 因此这里重复一份数量。两处必须同步：改了一处没改另一处，
        # 表现就是「明明都开了却报数量不符」。
        EXPECTED_DIALOGS = 4
        if len(dlg_results) != EXPECTED_DIALOGS:
            dlg_extra.append(False)
            print(
                f"  只有 {len(dlg_results)} 个对话框被检查，"
                f"应为 {EXPECTED_DIALOGS} 个"
            )

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

        # 先跑「内容少」那一轮：Arco 内层容器不继承父高度，
        # 只有几条数据时表体会按内容收缩、底部露出大片空白。
        # 上面那趟用的是 40 行，正好把这个形态掩盖掉，因此必须单独测。
        few_dump = subprocess.run(
            common + ["--window-size=1827,1190", "--dump-dom",
                      f"http://127.0.0.1:{port}/{FEW_PAGE}"],
            capture_output=True, timeout=180)
        few_visible = re.sub(r"(?is)<script\b.*?</script>", " ",
                             few_dump.stdout.decode("utf-8", "replace"))
        few_lay = re.search(r"LAYOUT:([^\s<]*)", few_visible)
        few_ok = True

        def few_claim(label, cond, detail=""):
            nonlocal few_ok
            few_ok = few_ok and cond
            print(f"  {label:<34} {'是' if cond else '否'}{detail}")

        print(f"  （本组用 {FEW_ROWS} 条数据，模拟「投影很少」时的界面）")
        if not few_lay or few_lay.group(1) in ("pending", "missing"):
            print("  未能取到少行数据的布局")
            few_ok = False
        else:
            ff = {}
            for part in few_lay.group(1).split(","):
                k, _, v = part.partition("=")
                ff[k] = v
            f_gap_body = int(ff.get("gapBody", "9999"))
            f_body_h = int(ff.get("bodyH", "0"))
            f_min = int(ff.get("minBodyH", "0"))
            few_claim("少行时表体仍撑到底", abs(f_gap_body) <= 2,
                      f"  与栏底差 {f_gap_body}px")
            few_claim("少行时表体不按内容收缩", f_body_h >= f_min,
                      f"  表体 {f_body_h}px，至少应 {f_min}px")
        results.append(few_ok)

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
            # 表体也必须撑到底：外层全对、内层收缩是「内容少才暴露」的形态，
            # 只查外层会漏掉它。这里直接量最内层表体的高度与底边。
            gap_body = int(fields.get("gapBody", "9999"))
            body_h = int(fields.get("bodyH", "0"))
            min_body_h = int(fields.get("minBodyH", "0"))
            claim("表体撑到底部（不留大片空白）", abs(gap_body) <= 2,
                  f"  与栏底差 {gap_body}px")
            claim("表体高度占满可用空间", body_h >= min_body_h,
                  f"  表体 {body_h}px，至少应 {min_body_h}px（按内容收缩会远小于它）")
            # 底部操作条自身也必须贴底，否则它下面留一条空白
            claim("底部操作条贴住栏底", abs(int(fields.get("gapFoot", "9999"))) <= 2,
                  f"  与栏底差 {fields.get('gapFoot')}px")
            # 表头竖向分割线：每列都要有（末列除外），否则看不出列边界在哪
            th_count = int(fields.get("thCount", "0"))
            divider_ok = int(fields.get("dividerOk", "0"))
            claim("表头每列都有竖向分割线", th_count > 0 and divider_ok == th_count,
                  f"  {divider_ok}/{th_count} 列有边框（末列应为 0，其余 1px）")
            # 拖动手柄：Arco 不给最后一列手柄（它被当作剩余空间）。
            # 这里补了一个 1px 占位列放在最后，使**真正的每一列都能拖**，
            # 因此手柄数 = 数据列数（含被隐藏的占位列）即可。
            claim("每列都有列宽拖动手柄", int(fields.get("handleCount", "0")) >= th_count - 2,
                  f"  {fields.get('handleCount')} 个手柄 / {th_count} 个表头（末列为占位列）")
            # 目录栏字号应比右侧详情区小（同宽下显示更多内容）
            td_font = fields.get("tdFont", "")
            detail_font = fields.get("detailFont", "")
            claim("目录栏字号小于详情区", td_font == "12px" and detail_font not in ("", td_font),
                  f"  目录栏 {td_font} / 详情区 {detail_font or '无对照'}")
            print(f"  目录栏宽度 {fields.get('w')}px")
            print(f"  各列宽度 {fields.get('wCols0')}")
            # 3D 视图必须真的画出几何。
            #
            # 替身给的是一个朝向相机的单片四边形（见 SHIM 里的 quadScene）：
            # 绕序正确 → 被 FrontSide 画出 → 有不透明像素；绕序反了 →
            # 整个面作为背面被剔除 → 0 个像素。这条正是「透视方块」的
            # 回归防线：上一版替身用空 batches，画布永远是 0 个像素，
            # 因此渲染检查全绿而用户实际看到看穿。
            pix_opaque = int(fields.get("pixOpaque", "-1"))
            pix_ratio = fields.get("pixRatio", "-1")
            claim("3D 视图真的画出了几何（绕序是否朝外）", pix_opaque > 100,
                  f"  不透明像素 {pix_opaque}/40000（占比 {pix_ratio}）；"
                  f"为 0 说明面被当成背面剔除，即看穿方块")
            # 预览舞台必须是 400×400 且靠左（其左缘对齐详情栏左缘）
            stage_w = int(fields.get("stageW", "-1"))
            stage_h = int(fields.get("stageH", "-1"))
            stage_left = int(fields.get("stageLeft", "-1"))
            detail_left = int(fields.get("detailLeft", "-1"))
            claim("预览区为正方形且不小于 400", abs(stage_w - stage_h) <= 1 and stage_w >= 400,
                  f"  实际 {stage_w}×{stage_h}")
            # 容许 --preview__main 的 8px 内边距：舞台在详情栏左缘内侧一点，
            # 这是有意的留白，不是「没有靠左」
            claim("预览区靠左（贴住详情栏左缘）", abs(stage_left - detail_left) <= 12,
                  f"  舞台左缘 {stage_left} / 详情栏左缘 {detail_left}")
            # 渲染图形不得超出舞台。曾实测画布 987px 而容器 862px
            claim("渲染图形未超出预览区", fields.get("canvasOverflow") == "false",
                  f"  画布宽 {fields.get('canvasW')}px / 舞台宽 {stage_w}px")
            # 控制条不应被挤压到文字竖排：曾实测单选按钮 47×44（「正面」竖排）
            bar_h = int(fields.get("barBtnH", "-1"))
            claim("控制条紧贴预览底部（未被信息列推远）",
                  abs(int(fields.get("gapBar", "9999"))) <= 40,
                  f"  距舞台底 {fields.get('gapBar')}px")
            claim("选中的文件行有高亮", fields.get("selOk") == "true",
                  "  命中 .row-selected")
            claim("信息区在预览图右侧", fields.get("infoRight") == "true",
                  f"  信息区左缘 {fields.get('infoLeft')} / 舞台右缘 {fields.get('stageRight')}")
            # 描述框要多行够大（没有 rows 属性时按实际高度判断）
            desc_h = int(fields.get("descH", "0"))
            claim("描述框为多行大输入框", desc_h >= 100,
                  f"  textarea 高 {desc_h}px（要求方便多行显示）")
            claim("描述标签标注了（备注）", fields.get("labelOk") == "true")
            # 方块栏：分割线、拖动手柄、自适应到底、导出按钮
            b_th = int(fields.get("bTh", "0"))
            b_border = int(fields.get("bBorder", "0"))
            # 最后一列是 1px 占位列、不应有边框，因此断言「除了最后一列都有」
            claim("方块栏表头每列都有竖向分割线",
                  b_th > 0 and b_border >= b_th - 1,
                  f"  {b_border}/{b_th} 列有边框（末列为占位列，应为 {b_th - 1}）")
            claim("方块栏可拖动调整列宽",
                  int(fields.get("bHandles", "0")) >= 2,
                  f"  拖动手柄 {fields.get('bHandles')} 个")
            # 表格底边贴住页签底边：>2px 说明下方留了空白，即「没自适应到最低位置」
            claim("方块栏表格撑满到页签底部",
                  abs(int(fields.get("bGap", "9999"))) <= 2,
                  f"  表格底边距页签底边 {fields.get('bGap')}px")
            claim("方块栏有导出与复制表格按钮",
                  fields.get("bExport") == "true" and fields.get("bCopy") == "true")
            claim("保存按钮无需滚动即可点到", fields.get("saveOk") == "true",
                  f"  按钮底边相对面板底边 {fields.get('saveGap')}px（>0 表示被挤出可视范围）")
            claim("预览控制条未被挤压（按钮不竖排）", fields.get("barOk") == "true",
                  f"  视角按钮 {fields.get('barBtnW')}×{bar_h}px；高度超过 30 说明文字折行了")
            # 与「加宽名称以显示更多内容」的目的相反。
            decl_sum = int(fields.get("declSum", "0"))
            box_w = int(fields.get("boxW", "0"))
            claim("各列未被横向挤出可视区", fields.get("fits") == "true",
                  f"  列宽合计 {decl_sum}px / 目录栏 {box_w}px")

        # 列宽拖动与复位
        rst = re.search(r"RESET:([^\s<]*)", visible)
        if not rst:
            print("  未能取到列宽复位结果")
            layout_ok = False
        elif rst.group(1).startswith("no-button"):
            print("  未找到「列宽复位」按钮")
            layout_ok = False
        else:
            rf = {}
            for part in rst.group(1).split(","):
                k, _, v = part.partition("=")
                rf[k] = v
            base = [int(x) for x in rf.get("base", "").split("|") if x]
            dragged = [int(x) for x in rf.get("dragged", "").split("|") if x]
            after = [int(x) for x in rf.get("afterReset", "").split("|") if x]
            # 拖动必须真的改变了列宽，否则下面的复位断言没有意义
            moved = len(base) > 1 and len(dragged) > 1 and base[1] != dragged[1]
            claim("拖动表头能改变列宽", moved,
                  f"  名称列 {base[1] if len(base) > 1 else '?'}px -> "
                  f"{dragged[1] if len(dragged) > 1 else '?'}px")
            claim("「列宽复位」能恢复默认宽度", bool(base) and after == base,
                  f"  复位后 {after} / 默认 {base}")

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
        if os.path.exists(few_path):
            os.remove(few_path)

    print("\n渲染检查：" + ("全部通过" if ok else "存在失败项"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
