import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import ArcoVue, { Message } from "@arco-design/web-vue";

// Tauri 的全部出口都换掉：测试跑在 jsdom 里，没有 Tauri 运行时。
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    minimize: vi.fn(),
    toggleMaximize: vi.fn(),
    close: vi.fn(),
    startDragging: vi.fn(),
    isMaximized: vi.fn().mockResolvedValue(false),
    onResized: vi.fn().mockResolvedValue(() => {}),
  }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
  save: vi.fn().mockResolvedValue(null),
}));
vi.mock("@tauri-apps/plugin-fs", () => ({ writeFile: vi.fn() }));

// SceneViewer 依赖 WebGL，jsdom 没有。挂载测试关心的是外壳界面，
// 因此用空组件替代，避免把 WebGL 缺失误判成界面故障。
vi.mock("../components/SceneViewer.vue", () => ({
  default: {
    name: "SceneViewer",
    template: "<div class='scene-viewer-stub' />",
    methods: { exportPng: () => null, applyPreset: () => {} },
  },
}));

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import App from "../App.vue";
import type { SchematicSummary, SceneData, DuplicateGroup, ConvertItem } from "../types";

const mockInvoke = vi.mocked(invoke);
const mockOpen = vi.mocked(open);

/** 一个可用的投影摘要，字段与后端 SchematicSummary 对齐
 *
 * 必须标注类型：不加标注时它不是「赋给有类型变量的对象字面量」，
 * TS 的多余属性检查不会生效，夹具与 DTO 漂移了编译器也不会报错。
 */
const summary: SchematicSummary = {
  path: "/p/测试.litematic",
  fileName: "测试.litematic",
  dirName: "/p",
  fileSize: 2048,
  error: "",
  formatVersion: 6,
  dataVersion: 3465,
  mcVersionName: "1.20.1",
  name: "测试投影",
  author: "作者",
  description: "",
  timeCreated: 0,
  timeModified: 0,
  enclosing: [16, 10, 16],
  totalBlocks: 520,
  totalVolume: 2560,
  regionCount: 1,
  entityCount: 0,
  tileEntityCount: 0,
  pendingTicks: 0,
  hasPreview: false,
};

/** 挂载外壳界面 */
function mountApp() {
  return mount(App, { global: { plugins: [ArcoVue] }, attachTo: document.body });
}

/** 一个可用的场景数据，字段与后端 SceneData 对齐 */
const scene: SceneData = {
  batches: [],
  totalVertices: 0,
  totalTriangles: 0,
  enclosing: [16, 10, 16],
  origin: [0, 0, 0],
  mcVersionName: "1.20.1",
  dataVersion: 3465,
  unresolvedBlocks: 0,
};

/** 让 invoke 按命令名返回**结构正确**的结果。
 *
 * 未列出的命令一律返回空数组是不够的：需要对象的命令拿到数组后，
 * 界面在访问字段时才会抛错，表现为「测试通过但留下未处理错误」。
 * 因此这里把会返回对象/标量的命令都显式列出。
 */
function stubInvoke(overrides: Record<string, unknown> = {}) {
  mockInvoke.mockImplementation((async (cmd: string) => {
    if (cmd in overrides) return overrides[cmd];
    switch (cmd) {
      case "scan_litematics":
        return { items: [summary], skippedDirs: 0, skippedPaths: [] };
      case "build_scene":
        return scene;
      case "analyze_schematic":
        return {
          blocks: [],
          entities: [],
          tileEntities: [],
          nonAirTotal: 0,
          airTotal: 0,
          distinctBlockTypes: 0,
          distinctStates: 0,
        };
      case "list_block_versions":
        return [];
      case "list_packs":
        // 返回 PackListDto（后端类型），不是数组：App.vue 打开资源包面板时
        // 立刻读 r.packs / r.skipped / r.loadFailures，给数组会在
        // r.skipped.length 处抛 TypeError，被 catch 变成「读取资源包清单失败」。
        return { packs: [], skipped: [], loadFailures: [] };
      case "list_backups":
      case "read_operation_log":
        return [];
      case "mc_version_name":
        return "1.20.1";
      case "peek_schematic":
        return summary;
      default:
        // 未预期的命令返回空数组；若某个流程因此抛错，会以未处理错误的形式
        // 暴露出来，而不是被静默吞掉
        return [];
    }
  }) as never);
}

beforeEach(() => {
  mockInvoke.mockReset();
  mockOpen.mockReset();
  mockOpen.mockResolvedValue(null);
  document.body.innerHTML = "";
});

describe("界面外壳", () => {
  it("能挂载且不抛异常", () => {
    // 这是最关键的一条：若 App.vue 在挂载时抛错，用户看到的就是空白窗口，
    // 表现正是「功能都不能用」
    expect(() => mountApp()).not.toThrow();
  });

  it("工具栏列出全部功能入口", () => {
    const w = mountApp();
    const text = w.text();
    for (const label of [
      "添加目录",
      "方块替换",
      "版本转换",
      "重复文件筛查",
      "备份管理",
      "资源包管理",
    ]) {
      expect(text, `工具栏应含「${label}」`).toContain(label);
    }
    w.unmount();
  });

  it("未选中文件时，依赖选中项的按钮被禁用", () => {
    const w = mountApp();
    const buttons = w.findAll("button");
    const find = (label: string) => buttons.find((b) => b.text().includes(label));
    // 这两个功能以「有选中的投影文件」为前提，无选中时应禁用而不是可点后报错
    expect(find("方块替换")?.attributes("disabled"), "方块替换应禁用").toBeDefined();
    expect(find("版本转换")?.attributes("disabled"), "版本转换应禁用").toBeDefined();
    // 与选中项无关的入口应始终可用
    expect(find("添加目录")?.attributes("disabled")).toBeUndefined();
    expect(find("备份管理")?.attributes("disabled")).toBeUndefined();
    w.unmount();
  });

  it("未选中文件时显示空状态提示，而不是空白页", () => {
    const w = mountApp();
    // 页签只在选中文件后渲染；未选中时必须有明确的引导文案，
    // 否则用户面对的是一个不知从何下手的界面
    expect(w.text()).toContain("选择一个投影文件以查看详情");
    w.unmount();
  });

  it("初始不应向打开弹窗的功能发命令", () => {
    // 挂载即触发一批查询会把界面拖慢；这些应当按需触发
    mountApp();
    const names = mockInvoke.mock.calls.map((c) => c[0]);
    for (const cmd of ["list_packs", "search_blocks", "convert_targets", "scan_duplicates"]) {
      expect(names, `初始不应调用 ${cmd}`).not.toContain(cmd);
    }
  });
});

describe("主要操作流程", () => {
  it("点「添加目录」→ 选目录 → 列出投影 → 选中后可编辑元数据", async () => {
    stubInvoke();
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();

    // 点按钮应触发展开目录选择
    const add = w.findAll("button").find((b) => b.text().includes("添加目录"));
    expect(add, "应能定位到「添加目录」按钮").toBeTruthy();
    await add!.trigger("click");
    await flushPromises();

    expect(mockOpen).toHaveBeenCalled();
    const names = mockInvoke.mock.calls.map((c) => c[0]);
    expect(names, "选择目录后应扫描该目录").toContain("scan_litematics");

    // 扫描结果应出现在列表里
    expect(w.text(), "列表应显示扫描到的文件").toContain("测试.litematic");

    // ---- 选中并真的编辑、保存 ----
    // 之前这条测试只验证到「列表显示了文件」，与标题里承诺的
    // 「可编辑元数据」并不相符：编辑框、保存按钮、写回的参数都没被检查。
    await w.findAll("tr").find((r) => r.text().includes("测试.litematic"))!.trigger("click");
    await flushPromises();

    // 元数据输入框应带出文件里的现有值
    const inputs = w.findAll("input");
    const nameInput = inputs.find((i) => (i.element as HTMLInputElement).value === "测试投影");
    expect(nameInput, "投影名称输入框应带出原值").toBeTruthy();

    // 未改动时「保存」不可用，改动后才可用
    const saveBtn = () => w.findAll("button").find((b) => b.text().includes("保存"));
    expect(saveBtn()?.attributes("disabled"), "未改动时不应可保存").toBeDefined();

    await nameInput!.setValue("改过的名字");
    await flushPromises();
    expect(saveBtn()?.attributes("disabled"), "改动后应可保存").toBeUndefined();

    // 保存：应把编辑框里的值原样写给后端，并在成功后重新读取
    let saved: unknown = null;
    mockInvoke.mockImplementation((async (cmd: string, args: Record<string, unknown>) => {
      if (cmd === "save_metadata") {
        saved = args;
        return { backupPath: "/p/备份/测试.litematic.bak" };
      }
      switch (cmd) {
        case "scan_litematics":
          return { items: [summary], skippedDirs: 0, skippedPaths: [] };
        case "build_scene":
          return scene;
        case "analyze_schematic":
          return {
            blocks: [], entities: [], tileEntities: [],
            nonAirTotal: 0, airTotal: 0, distinctBlockTypes: 0, distinctStates: 0,
          };
        default:
          return [];
      }
    }) as never);

    await saveBtn()!.trigger("click");
    await flushPromises();

    expect(saved, "应调用 save_metadata").toBeTruthy();
    // 参数是平铺的：{ path, name, author, description }（见 api.ts 的 saveMetadata）
    const args = saved as { path: string; name: string; author: string; description: string };
    expect(args.path, "应写回被选中的那个文件").toBe(summary.path);
    expect(args.name, "编辑框里的名字应原样提交").toBe("改过的名字");
    expect(args.author, "未改动的字段应保持原值").toBe("作者");

    // 保存结果必须告知用户备份位置，否则改动看起来像是没发生
    expect(document.body.textContent, "应提示备份路径").toContain("备份");
    w.unmount();
  });

  it("选中投影后，详情区出现信息/方块/实体三个页签", async () => {
    stubInvoke();
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();

    // 点列表里的文件行使其成为选中项
    const row = w.findAll("tr").find((r) => r.text().includes("测试.litematic"));
    expect(row, "应能找到该文件所在的行").toBeTruthy();
    await row!.trigger("click");
    await flushPromises();

    const text = w.text();
    for (const tab of ["信息", "方块", "实体"]) {
      expect(text, `选中后应出现「${tab}」页签`).toContain(tab);
    }
    w.unmount();
  });

  it("扫描到文件后「方块替换」「版本转换」变为可用", async () => {
    stubInvoke();
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();

    const row = w.findAll("tr").find((r) => r.text().includes("测试.litematic"));
    await row!.trigger("click");
    await flushPromises();

    const btn = (label: string) => w.findAll("button").find((b) => b.text().includes(label));
    expect(btn("方块替换")?.attributes("disabled"), "选中后应可用").toBeUndefined();
    expect(btn("版本转换")?.attributes("disabled"), "选中后应可用").toBeUndefined();
    w.unmount();
  });
});

describe("重新扫描后的选中项", () => {
  /** 点工具栏的「添加目录」并等扫描完成 */
  async function addDir(w: ReturnType<typeof mountApp>, dir: string) {
    mockOpen.mockResolvedValue(dir);
    const btn = w.findAll("button").find((b) => b.text().includes("添加目录"));
    expect(btn, "应能定位到「添加目录」按钮").toBeTruthy();
    await btn!.trigger("click");
    await flushPromises();
  }

  it("换到另一个目录后，不再显示上一个目录的选中项", async () => {
    // 这条防的是一个真实缺陷：pickDirectory 只替换 files 而不处理选中项，
    // 于是详情面板继续显示**已不在列表中**的文件，「保存元数据」会把改动
    // 写到那个路径上，而用户在列表里选中不了它，看不出改动写到了哪里。
    const other: SchematicSummary = {
      ...summary,
      path: "/q/另一个.litematic",
      fileName: "另一个.litematic",
      dirName: "/q",
      name: "另一个投影",
    };

    stubInvoke();
    const w = mountApp();
    await addDir(w, "/p");
    // 选中第一个目录里的文件
    const row = w.findAll("tr").find((r) => r.text().includes("测试.litematic"));
    expect(row, "应能找到该文件所在的行").toBeTruthy();
    await row!.trigger("click");
    await flushPromises();
    expect(w.text(), "选中后应出现详情页签").toContain("信息");

    // 换一个目录重新扫描，新列表里没有原选中项
    stubInvoke({
      scan_litematics: { items: [other], skippedDirs: 0, skippedPaths: [] },
    });
    await addDir(w, "/q");

    expect(w.text(), "旧目录的文件不应还留在列表里").not.toContain("测试.litematic");
    expect(w.text(), "应显示新目录的文件").toContain("另一个.litematic");
    expect(
      w.text(),
      "选中项已不在列表中，应回到空状态而不是继续显示旧文件的详情",
    ).toContain("选择一个投影文件以查看详情");

    // 依赖选中项的按钮必须同步禁用，否则点下去会作用到旧路径
    const btn = (label: string) => w.findAll("button").find((b) => b.text().includes(label));
    expect(btn("方块替换")?.attributes("disabled"), "切换后应禁用").toBeDefined();
    expect(btn("版本转换")?.attributes("disabled"), "切换后应禁用").toBeDefined();
    w.unmount();
  });

  it("同一目录重新扫描时保留选中项", async () => {
    // 反过来的情形：重新扫描当前目录（例如文件没变），选中项应当保留，
    // 不能一律清空——那会让用户每次刷新都丢掉正在查看的文件。
    stubInvoke();
    const w = mountApp();
    await addDir(w, "/p");
    await w.findAll("tr").find((r) => r.text().includes("测试.litematic"))!.trigger("click");
    await flushPromises();
    expect(w.text()).toContain("信息");

    await addDir(w, "/p");

    expect(w.text(), "同一目录重扫后应仍显示该文件").toContain("测试.litematic");
    expect(w.text(), "选中项应保留").toContain("信息");
    const btn = (label: string) => w.findAll("button").find((b) => b.text().includes(label));
    expect(btn("方块替换")?.attributes("disabled"), "选中项保留，按钮应可用").toBeUndefined();
    w.unmount();
  });
});

describe("对话框失败时的提示", () => {
  it("打开目录选择框失败时给出提示，而不是静默无反应", async () => {
    // 这条防的是：pickDirectory 把 `await open(...)` 放在 try 之外，
    // 插件报错时这个 Promise 会静默 reject——用户点了「添加目录」
    // 却什么都没发生，也收不到任何提示。
    //
    // 断言用 spy 而不是查 document.body：Arco 的 Message 通过 portal 渲染，
    // 在同一次运行里先跑过别的用例后，它的容器状态会让文本断言变得不稳定
    // （本条曾单独通过、全量运行却失败）。直接断言「确实报告了错误」
    // 既精确又不依赖 portal 的内部实现。
    stubInvoke();
    mockOpen.mockRejectedValue(new Error("对话框不可用"));
    const errSpy = vi.spyOn(Message, "error");
    const w = mountApp();

    const btn = w.findAll("button").find((b) => b.text().includes("添加目录"));
    expect(btn, "应能定位到「添加目录」按钮").toBeTruthy();
    await btn!.trigger("click");
    await flushPromises();

    const reported = errSpy.mock.calls.map((c) => String(c[0]));
    expect(
      reported.some((t) => t.includes("打开目录选择框失败")),
      `应报告选择框打开失败，实际报告了：${JSON.stringify(reported)}`,
    ).toBe(true);
    // 失败时不应误报正在扫描
    expect(w.text(), "不应进入扫描状态").not.toContain("正在扫描");

    errSpy.mockRestore();
    w.unmount();
  });
});

describe("重复文件筛查", () => {
  /** 在整份文档里按文案找按钮。
   *
   * Arco 的弹窗内容会被 teleport 到 body，因此 wrapper.findAll 看不到它
   * 内部的元素，必须查 document。
   */
  function findButton(label: string): HTMLButtonElement | undefined {
    return [...document.querySelectorAll("button")].find((b) =>
      (b.textContent ?? "").includes(label),
    );
  }

  it("「可回收」按组内单份大小计算，不依赖左侧列表", async () => {
    // 这条防的是一个真实缺陷：可回收原先从 files.value 反查文件大小，
    // 而该列表只含当前已扫描的目录，与筛查目录常常不同，反查失败时
    // 静默按 0 处理，于是「单个大小」有值而「可回收」恒为 0 B。
    const size = 4096;
    const paths = ["/a/x.litematic", "/b/x.litematic", "/c/x.litematic"];
    stubInvoke({
      // 注意：筛查目录故意与左侧列表的目录不同，正是原缺陷的触发条件
      scan_duplicates: {
        groups: [{ size, paths }],
        scanned: paths.length,
        totalBytes: size * paths.length,
        unreadable: [],
        dir: "C:/其他目录",
      },
    });

    const w = mountApp();
    // 打开弹窗
    const open = w.findAll("button").find((b) => b.text().includes("重复文件筛查"));
    await open!.trigger("click");
    await flushPromises();

    // 筛查目录必须先填：runDuplicateScan 会拒绝空目录。
    // 这里直接写组件的响应式状态而不是模拟输入框键入，避免依赖 Arco
    // 输入组件的内部实现；被验证的仍是「结果表格如何显示」这段逻辑。
    (w.vm as unknown as { dupDir: string }).dupDir = "C:/其他目录";
    await flushPromises();

    const start = findButton("开始筛查");
    expect(start, "弹窗里应有「开始筛查」").toBeTruthy();
    start!.click();
    await flushPromises();

    // 三个副本、保留一份 -> 单份 4096 字节（"4.0 KB"），可回收 8192 字节（"8.0 KB"）。
    // 按 formatSize 的规则断言确切文本：显示 "0 B" 就是原先的缺陷复现。
    const body = document.body.textContent ?? "";
    expect(body, "应显示重复组").toContain("x.litematic");
    expect(body, "单个大小应为 4.0 KB").toContain("4.0 KB");
    expect(
      body,
      `「可回收」应为 8.0 KB（两个多余副本），显示 0 B 即为缺陷复现。实际：${body.slice(0, 600)}`,
    ).toContain("8.0 KB");
    w.unmount();
  });
});

describe("版本转换", () => {
  function findButton(label: string): HTMLButtonElement | undefined {
    return [...document.querySelectorAll("button")].find((b) =>
      (b.textContent ?? "").includes(label),
    );
  }

  it("分析失败后不应再显示「可以直接转换」", async () => {
    // 这条防的是：分析失败的 catch 只清空条目、不复位 convAnalyzed，
    // 于是模板把「已分析且条目为空」当成「方块都存在」，展示绿色成功提示，
    // 且确定按钮可用，点下去会用空条目执行转换。
    let analyzeCalls = 0;
    mockInvoke.mockImplementation((async (cmd: string) => {
      switch (cmd) {
        case "scan_litematics":
          return { items: [summary], skippedDirs: 0, skippedPaths: [] };
        case "convert_targets":
          return [{ index: 0, name: "1.19.2", dataVersion: 3120 }];
        case "analyze_conversion":
          analyzeCalls += 1;
          // 第一次成功且无缺口（显示成功提示），第二次失败
          if (analyzeCalls === 1) return [];
          throw new Error("模拟分析失败");
        // 选中文件后会加载场景与详情；必须返回结构正确的对象，
        // 否则界面在访问字段时抛错，表现为「测试通过但留下未处理错误」
        case "build_scene":
          return scene;
        case "analyze_schematic":
          return {
            blocks: [],
            entities: [],
            tileEntities: [],
            nonAirTotal: 0,
            airTotal: 0,
            distinctBlockTypes: 0,
            distinctStates: 0,
          };
        case "list_block_versions":
          return [];
        default:
          return [];
      }
    }) as never);

    const w = mountApp();
    // 目录选择框要返回一个路径，否则不会进入扫描流程
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await w.findAll("tr").find((r) => r.text().includes("测试.litematic"))!.trigger("click");
    await flushPromises();

    await w.findAll("button").find((b) => b.text().includes("版本转换"))!.trigger("click");
    await flushPromises();

    // 选一个目标版本，否则分析按钮会提示「请先选择目标版本」
    const select = document.querySelector(".arco-select-view, .arco-select") as HTMLElement | null;
    if (select) {
      select.click();
      await flushPromises();
      const opt = [...document.querySelectorAll(".arco-select-option")].find((o) =>
        (o.textContent ?? "").includes("1.19.2"),
      ) as HTMLElement | undefined;
      if (opt) {
        opt.click();
        await flushPromises();
      }
    }

    const analyze = findButton("分析差异");
    expect(analyze, "弹窗里应有「分析差异」").toBeTruthy();
    analyze!.click();
    await flushPromises();
    expect(document.body.textContent, "首次分析成功应给出成功提示").toContain(
      "可以直接转换",
    );

    analyze!.click();
    await flushPromises();
    expect(
      document.body.textContent,
      "分析失败后不应残留「可以直接转换」的成功提示",
    ).not.toContain("可以直接转换");
    w.unmount();
  });
});

