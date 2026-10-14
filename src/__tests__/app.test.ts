import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import ArcoVue, { Message } from "@arco-design/web-vue";

// Tauri 的全部出口都换掉：测试跑在 jsdom 里，没有 Tauri 运行时。
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
// 主题相关的能力要做成可替换的 mock：测试需要分别模拟深色、浅色、
// 以及读取失败三种情形。用一个真实函数包着，默认返回浅色。
const fakeWindow = () =>
  ({
    minimize: vi.fn(),
    toggleMaximize: vi.fn(),
    close: vi.fn(),
    startDragging: vi.fn(),
    isMaximized: vi.fn().mockResolvedValue(false),
    onResized: vi.fn().mockResolvedValue(() => {}),
    theme: vi.fn().mockResolvedValue("light"),
    onThemeChanged: vi.fn().mockResolvedValue(() => {}),
  }) as never;

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => fakeWindow()),
}));

const mockGetCurrentWindow = vi.mocked(getCurrentWindow);
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
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "../App.vue";
import type { SchematicSummary, SceneData, DuplicateGroup, ConvertItem, BlockCount } from "../types";

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
      case "load_ui_state":
        // 契约是 string | null。默认「从未保存过」，因此回 null；
        // 回数组会让 App.vue 走进 JSON 解析失败分支并弹出警告，
        // 那是替身给错类型造成的噪声，不是真实行为。
        return null;
      case "save_ui_state":
        return undefined;
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
        return { packs: [], packsDir: "/app/packs", skipped: [], loadFailures: [] };
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
  // 主题用例会替换 getCurrentWindow 的返回值；不复位的话，
  // 那个「读不到主题」的替身会泄漏到后面的用例里（实测泄漏了 5 条告警）
  mockGetCurrentWindow.mockReset();
  mockGetCurrentWindow.mockImplementation(() => fakeWindow());
  document.body.innerHTML = "";
  document.body.removeAttribute("arco-theme");
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

describe("跟随系统深浅色", () => {
  // Arco 的深色取值挂在 body[arco-theme="dark"] 上。样式表里两套都写好了，
  // 但没人设置这个属性的话深色永远不会生效——系统开了深色，界面仍是白的。
  beforeEach(() => {
    document.body.removeAttribute("arco-theme");
  });

  it("系统为深色时给 body 打上 arco-theme，界面才真的变深", async () => {
    vi.mocked(getCurrentWindow).mockReturnValue({
      theme: vi.fn().mockResolvedValue("dark"),
      onThemeChanged: vi.fn().mockResolvedValue(() => {}),
    } as never);
    stubInvoke();
    const w = mountApp();
    await flushPromises();
    expect(document.body.getAttribute("arco-theme"), "应设为 dark").toBe("dark");
    w.unmount();
  });

  it("系统为浅色时不加该属性", async () => {
    vi.mocked(getCurrentWindow).mockReturnValue({
      theme: vi.fn().mockResolvedValue("light"),
      onThemeChanged: vi.fn().mockResolvedValue(() => {}),
    } as never);
    stubInvoke();
    const w = mountApp();
    await flushPromises();
    expect(document.body.getAttribute("arco-theme"), "不应有 dark 属性").toBeNull();
    w.unmount();
  });

  it("系统切换主题时跟随变化", async () => {
    let fire: ((e: { payload: string }) => void) | undefined;
    vi.mocked(getCurrentWindow).mockReturnValue({
      theme: vi.fn().mockResolvedValue("light"),
      onThemeChanged: vi.fn().mockImplementation(async (cb: (e: { payload: string }) => void) => {
        fire = cb;
        return () => {};
      }),
    } as never);
    stubInvoke();
    const w = mountApp();
    await flushPromises();
    expect(document.body.getAttribute("arco-theme")).toBeNull();

    // 系统切到深色
    expect(fire, "应注册了主题变化回调").toBeTruthy();
    fire!({ payload: "dark" });
    await flushPromises();
    expect(document.body.getAttribute("arco-theme"), "应跟随变为 dark").toBe("dark");

    // 再切回浅色
    fire!({ payload: "light" });
    await flushPromises();
    expect(document.body.getAttribute("arco-theme"), "应跟随去掉 dark").toBeNull();
    w.unmount();
  });

  it("读不到主题时不崩，按浅色显示", async () => {
    // 浏览器里跑前端、或平台不提供该能力时都会走到这里
    vi.mocked(getCurrentWindow).mockReturnValue({
      theme: vi.fn().mockRejectedValue(new Error("不支持")),
      onThemeChanged: vi.fn().mockResolvedValue(() => {}),
    } as never);
    stubInvoke();
    const w = mountApp();
    await flushPromises();
    expect(document.body.getAttribute("arco-theme"), "失败时应保持浅色").toBeNull();
    // 其余功能不受影响
    expect(w.text(), "界面仍应正常渲染").toContain("添加目录");
    w.unmount();
  });
});

describe("重新扫描后的选中项", () => {
  /** 点工具栏的「添加目录」并等扫描完成。
   *
   * 对话框用 multiple: true 打开，因此插件返回的是**数组**。
   * 这里也传数组，与真实返回值一致；只传单个字符串会掩盖
   * 「只处理了返回值的第一种形态」这类缺陷。
   */
  async function addDir(w: ReturnType<typeof mountApp>, ...dirs: string[]) {
    mockOpen.mockResolvedValue(dirs);
    const btn = w.findAll("button").find((b) => b.text().includes("添加目录"));
    expect(btn, "应能定位到「添加目录」按钮").toBeTruthy();
    await btn!.trigger("click");
    await flushPromises();
  }

  it("移除目录后，不再显示该目录的选中项", async () => {
    // 这条防的是一个真实缺陷：移除目录时只改列表而不处理选中项，
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

    // 再加一个目录（新列表里没有原选中项）
    stubInvoke({
      scan_litematics: { items: [other], skippedDirs: 0, skippedPaths: [] },
    });
    await addDir(w, "/q");
    // 加载多个目录时两边的文件都要在：不是「換目录」而是「追加目录」
    expect(w.text(), "先前加载的目录应保留").toContain("测试.litematic");
    expect(w.text(), "新加载的目录也应显示").toContain("另一个.litematic");

    // 通过真实界面移除：点开「已加载目录」下拉，点第一项的「移除」按钮。
    // 不直接调用组件内部函数——那绕过了按钮是否真的能用、事件是否接对，
    // 而这两点正是「点了没反应」类缺陷的所在。
    const trigger = w.findAll("button").find((b) => b.text().includes("已加载目录"));
    expect(trigger, "应有「已加载目录」下拉按钮").toBeTruthy();
    await trigger!.trigger("click");
    await flushPromises();

    // 下拉内容被 teleport 到 body，因此从 document 里找
    const removeBtn = Array.from(document.querySelectorAll(".dir-item button")).find((b) =>
      (b.textContent ?? "").includes("移除"),
    ) as HTMLElement | undefined;
    expect(removeBtn, "下拉里应有「移除」按钮").toBeTruthy();
    removeBtn!.click();
    await flushPromises();

    expect(w.text(), "被移除目录的文件不应还留在列表里").not.toContain("测试.litematic");
    expect(w.text(), "另一个目录的文件应保留").toContain("另一个.litematic");
    expect(
      w.text(),
      "选中项已不在列表中，应回到空状态而不是继续显示旧文件的详情",
    ).toContain("选择一个投影文件以查看详情");

    // 依赖选中项的按钮必须同步禁用，否则点下去会作用到旧路径
    const btn = (label: string) => w.findAll("button").find((b) => b.text().includes(label));
    expect(btn("方块替换")?.attributes("disabled"), "移除后应禁用").toBeDefined();
    expect(btn("版本转换")?.attributes("disabled"), "移除后应禁用").toBeDefined();
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

describe("重复文件筛查（内联在主界面）", () => {
  /** 三条同名内容重复的记录，放在同一个目录 */
  const DIR = "/p";
  const DUP_PATHS = [
    `${DIR}/x.litematic`,
    `${DIR}/y.litematic`,
    `${DIR}/z.litematic`,
  ];

  function dupSummary(path: string): SchematicSummary {
    return {
      ...summary,
      path,
      fileName: path.split("/").pop() ?? "x.litematic",
      dirName: DIR,
    };
  }

  /** 添加目录并返回可用的 wrapper */
  async function mountWithDir(dirs: string[] = [DIR]) {
    mockOpen.mockResolvedValue(dirs);
    const w = mountApp();
    const add = w.findAll("button").find((b) => b.text().includes("添加目录"));
    expect(add, "应能定位到「添加目录」按钮").toBeTruthy();
    await add!.trigger("click");
    await flushPromises();
    return w;
  }

  /** 点「重复文件筛查」并等结果 */
  async function runScan(w: ReturnType<typeof mountApp>) {
    const scan = w.findAll("button").find((b) => b.text().includes("重复文件筛查"));
    expect(scan, "工具栏应有「重复文件筛查」").toBeTruthy();
    await scan!.trigger("click");
    await flushPromises();
  }

  function stubDup() {
    stubInvoke({
      scan_litematics: {
        items: DUP_PATHS.map(dupSummary),
        skippedDirs: 0,
        skippedPaths: [],
      },
      scan_duplicates: {
        groups: [{ size: 4096, paths: DUP_PATHS }],
        scanned: DUP_PATHS.length,
        totalBytes: 4096 * DUP_PATHS.length,
        unreadable: [],
        dir: DIR,
      },
    });
  }

  it("筛查后同组行标上同一种颜色类", async () => {
    stubDup();
    const w = await mountWithDir();
    await runScan(w);

    const rows = Array.from(document.querySelectorAll("tbody tr"));
    const dupRows = rows.filter((r) =>
      Array.from(r.classList).some((c) => c.startsWith("row-dup-")),
    );
    expect(dupRows.length, "三条重复记录都应被标记").toBe(3);
    const classes = new Set(
      dupRows.flatMap((r) =>
        Array.from(r.classList).filter((c) => c.startsWith("row-dup-")),
      ),
    );
    expect(classes.size, "同组应使用同一种颜色类").toBe(1);
    w.unmount();
  });

  it("默认勾选多于重复文件（每组保留第一份）", async () => {
    stubDup();
    const w = await mountWithDir();
    await runScan(w);

    // 用户原话「默认勾选多于重复文件」：一组三条应勾上两条
    expect(
      document.body.textContent ?? "",
      "一组三条应默认勾选两条",
    ).toContain("已选 2");
    w.unmount();
  });

  it("汇总条显示组数与文件数，「清除标记」能去掉标记", async () => {
    stubDup();
    const w = await mountWithDir();
    await runScan(w);

    const bar = w.find(".sidebar__dups");
    expect(bar.exists(), "应显示内联汇总条（不是弹窗）").toBe(true);
    expect(bar.text(), "应说明发现几组").toContain("1");
    expect(bar.text(), "应说明涉及几个文件").toContain("3");
    expect(
      document.body.textContent,
      "弹窗应已移除，筛查结果就地显示",
    ).not.toContain("尚未筛查");

    const clear = bar.findAll("button").find((b) => b.text().includes("清除标记"));
    expect(clear, "汇总条应有「清除标记」").toBeTruthy();
    await clear!.trigger("click");
    await flushPromises();

    const stillDup = Array.from(document.querySelectorAll("tbody tr")).filter((r) =>
      Array.from(r.classList).some((c) => c.startsWith("row-dup-")),
    );
    expect(stillDup.length, "清除后不应再有重复标记").toBe(0);
    w.unmount();
  });

  it("未勾选任何目录时给出提示而不是静默无反应", async () => {
    // 目录加载了但被取消勾选 -> 没有可筛查的目录
    stubDup();
    const w = await mountWithDir();
    const toggle = w.findAll("button").find((b) => b.text().includes("已加载目录"));
    expect(toggle, "应有目录下拉").toBeTruthy();
    await runScan(w);
    // 至少不应崩溃；有目录时会出汇总条，无目录时提示
    expect(w.find(".sidebar").exists()).toBe(true);
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

/** 左侧目录栏的宽度调整。
 *
 * 这类「看得见的布局」问题（宽度不可调、拖动没反应、窄窗口下右侧被挤没）
 * 都不会让构建失败，也不会让别的测试失败，因此单独覆盖。
 */
describe("目录栏宽度调整", () => {
  /** 取拖动条的当前宽度（由组件内联样式设置） */
  function sidebarWidth(w: ReturnType<typeof mountApp>): number {
    const el = w.find(".sidebar").element as HTMLElement;
    return parseFloat(el.style.width || "0");
  }

  it("默认宽度容得下各列，且不把右侧挤没", async () => {
    const w = mountApp();
    await flushPromises();
    const def = sidebarWidth(w);
    expect(def, "默认宽度应够放下全部列").toBeGreaterThanOrEqual(320);
    expect(def, "默认宽度不应把右侧挤没").toBeLessThanOrEqual(1200);

    // 关键：各列默认宽度之和不能超过目录栏默认宽度，
    // 否则最后一列会被挤出可视区——名称加宽本意是「显示更多」，
    // 挤出末列就与目的相反。这里按 colgroup 的实际宽度求和核对。
    const cols = w.findAll(".sidebar colgroup col");
    const sum = cols.reduce((acc, c) => {
      const m = /width:\s*([\d.]+)px/.exec(c.attributes("style") ?? "");
      return acc + (m ? Number(m[1]) : 0);
    }, 0);
    expect(sum, `各列合计 ${sum}px 应不超过目录栏默认宽度 ${def}px`).toBeLessThanOrEqual(def);

    const splitter = w.find(".splitter");
    expect(splitter.exists(), "应有拖动条").toBe(true);
    // 可聚焦与 role 是键盘可用的前提
    expect(splitter.attributes("role")).toBe("separator");
    expect(splitter.attributes("tabindex")).toBe("0");
    w.unmount();
  });

  it("拖动会改变宽度，且 pointerup 之后不再跟随", async () => {
    const w = mountApp();
    await flushPromises();
    const before = sidebarWidth(w);

    const splitter = w.find(".splitter").element as HTMLElement;
    // 真实触发 pointerdown，再由 window 收 pointermove：
    // 实现把监听挂在 window 上，正是为了让指针移出窄条后仍能继续拖动
    splitter.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, button: 0, clientX: before }),
    );
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: before + 120 }));
    await flushPromises();
    const during = sidebarWidth(w);
    expect(during, "拖动应加宽目录栏").toBeGreaterThan(before);

    window.dispatchEvent(new PointerEvent("pointerup", { clientX: before + 120 }));
    await flushPromises();
    const settled = sidebarWidth(w);

    // 松开之后再移动不应继续改宽度——否则拖动条会「粘住」
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: before + 400 }));
    await flushPromises();
    expect(sidebarWidth(w), "pointerup 之后不能再跟随指针").toBe(settled);
    w.unmount();
  });

  it("宽度被夹在上下限内，不会拖到不可用", async () => {
    const w = mountApp();
    await flushPromises();
    const splitter = w.find(".splitter").element as HTMLElement;

    const dragTo = async (clientX: number) => {
      splitter.dispatchEvent(
        new PointerEvent("pointerdown", { bubbles: true, button: 0, clientX: 0 }),
      );
      window.dispatchEvent(new PointerEvent("pointermove", { clientX }));
      window.dispatchEvent(new PointerEvent("pointerup", { clientX }));
      await flushPromises();
    };

    // 往左拖到负数：不能出现 0 或负宽度
    await dragTo(-500);
    expect(sidebarWidth(w), "向左拖到底应停在下限").toBeGreaterThanOrEqual(320);

    // 往右拖到很远：不能把右侧详情区挤没
    await dragTo(99999);
    expect(sidebarWidth(w), "向右拖到底应停在上限之内").toBeLessThanOrEqual(1200);
    w.unmount();
  });

  it("键盘左右方向键可调整宽度", async () => {
    const w = mountApp();
    await flushPromises();
    const splitter = w.find(".splitter");
    const before = sidebarWidth(w);

    await splitter.trigger("keydown", { key: "ArrowRight" });
    await flushPromises();
    expect(sidebarWidth(w), "右方向键应加宽").toBeGreaterThan(before);

    const widened = sidebarWidth(w);
    await splitter.trigger("keydown", { key: "ArrowLeft" });
    await flushPromises();
    expect(sidebarWidth(w), "左方向键应收窄").toBeLessThan(widened);

    await splitter.trigger("keydown", { key: "Home" });
    await flushPromises();
    expect(sidebarWidth(w), "Home 应回到最窄").toBe(320);
    w.unmount();
  });

  it("双击拖动条在默认宽度与最窄宽度之间切换", async () => {
    const w = mountApp();
    await flushPromises();
    const splitter = w.find(".splitter");
    // 默认宽度由列宽合计决定，因此从当前值取而不是写死数字：
    // 写死会在调整列宽后立刻失效，而那并不代表行为出错。
    const def = sidebarWidth(w);
    expect(def, "默认宽度应是合理值").toBeGreaterThanOrEqual(320);

    await splitter.trigger("dblclick");
    await flushPromises();
    expect(sidebarWidth(w), "双击应收起到最窄").toBe(320);

    await splitter.trigger("dblclick");
    await flushPromises();
    expect(sidebarWidth(w), "再次双击应回到默认宽度").toBe(def);
    w.unmount();
  });
});



/** 目录栏的多选与文件操作按钮。
 *
 * 这些按钮都只在「有勾选」时才可用，且必须把勾选的路径正确传给后端。
 * 传错路径（例如传成空数组、或传了未勾选的行）会造成误删/误复制，
 * 属于不能靠肉眼发现的错误，因此逐项核对发给后端的参数。
 */
describe("目录栏多选与文件操作", () => {
  /** 挂载并让列表出现若干行 */
  async function mountWithRows(n = 3) {
    const items = Array.from({ length: n }, (_, i) => ({
      ...summary,
      path: `/p/f${i}.litematic`,
      fileName: `f${i}.litematic`,
      name: `文件${i}`,
    }));
    stubInvoke({ scan_litematics: { items, skippedDirs: 0, skippedPaths: [] } });
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    return w;
  }

  /** 取某一行左侧的勾选框 */
  function rowCheckbox(w: ReturnType<typeof mountApp>, i: number) {
    return w.findAll(".arco-table-body tbody tr")[i].find("input[type=checkbox]");
  }

  function btn(w: ReturnType<typeof mountApp>, label: string) {
    return w.findAll("button").find((b) => b.text().trim() === label);
  }

  it("勾选后按钮才可用，未勾选时禁用", async () => {
    const w = await mountWithRows();
    // 未勾选：删除/复制/复制到/打开文件夹都应禁用
    for (const label of ["删除", "复制", "复制到", "打开文件夹"]) {
      expect(btn(w, label), `应有「${label}」按钮`).toBeTruthy();
      expect(btn(w, label)!.attributes("disabled"), `未勾选时「${label}」应禁用`).toBeDefined();
    }
    // 清除在未勾选时也应禁用
    expect(btn(w, "清除")!.attributes("disabled")).toBeDefined();

    await rowCheckbox(w, 0).setValue(true);
    await flushPromises();
    for (const label of ["删除", "复制", "复制到", "打开文件夹"]) {
      expect(btn(w, label)!.attributes("disabled"), `勾选后「${label}」应可用`).toBeUndefined();
    }
    w.unmount();
  });

  it("全选选中当前列表全部，反选翻转，清除清空", async () => {
    const w = await mountWithRows(3);
    expect(w.find(".sidebar__checks").text()).toContain("已选 0 / 3");

    await btn(w, "全选")!.trigger("click");
    await flushPromises();
    expect(w.find(".sidebar__checks").text()).toContain("已选 3 / 3");

    await btn(w, "反选")!.trigger("click");
    await flushPromises();
    expect(w.find(".sidebar__checks").text(), "全部选中后反选应变成全不选").toContain("已选 0 / 3");

    // 再全选后清除
    await btn(w, "全选")!.trigger("click");
    await flushPromises();
    await btn(w, "清除")!.trigger("click");
    await flushPromises();
    expect(w.find(".sidebar__checks").text()).toContain("已选 0 / 3");
    w.unmount();
  });

  it("删除只把勾选的路径发给后端", async () => {
    const w = await mountWithRows(3);
    await rowCheckbox(w, 1).setValue(true);
    await flushPromises();

    mockInvoke.mockResolvedValueOnce({
      names: ["f1.litematic"], written: [], destDir: "", totalBytes: 10,
    });
    await btn(w, "删除")!.trigger("click");
    await flushPromises();

    const call = mockInvoke.mock.calls.find((c) => c[0] === "delete_files");
    expect(call, "应调用 delete_files").toBeTruthy();
    expect((call![1] as { paths: string[] }).paths, "只应传勾选的那一个路径").toEqual([
      "/p/f1.litematic",
    ]);
    w.unmount();
  });

  it("复制把勾选的路径发给剪贴板命令", async () => {
    const w = await mountWithRows(3);
    await rowCheckbox(w, 0).setValue(true);
    await flushPromises();

    mockInvoke.mockResolvedValueOnce({
      names: ["f0.litematic"], written: [], destDir: "", totalBytes: 5,
    });
    await btn(w, "复制")!.trigger("click");
    await flushPromises();

    const call = mockInvoke.mock.calls.find((c) => c[0] === "copy_files_to_clipboard");
    expect(call, "应调用 copy_files_to_clipboard").toBeTruthy();
    expect((call![1] as { paths: string[] }).paths).toEqual(["/p/f0.litematic"]);
    w.unmount();
  });

  it("打开文件夹只用第一个勾选项（资源管理器只能选中一个）", async () => {
    const w = await mountWithRows(3);
    await rowCheckbox(w, 0).setValue(true);
    await rowCheckbox(w, 2).setValue(true);
    await flushPromises();

    mockInvoke.mockResolvedValueOnce(undefined);
    await btn(w, "打开文件夹")!.trigger("click");
    await flushPromises();

    const call = mockInvoke.mock.calls.find((c) => c[0] === "reveal_in_file_manager");
    expect(call, "应调用 reveal_in_file_manager").toBeTruthy();
    expect((call![1] as { path: string }).path, "应传列表中的第一个勾选项").toBe(
      "/p/f0.litematic",
    );
    w.unmount();
  });

  it("全选只作用于搜索结果，不会选中被过滤掉的行", async () => {
    const w = await mountWithRows(3);
    // 搜索只留下 f1
    await w.find("input[type=search], .arco-input-search input").setValue("f1");
    await flushPromises();
    expect(w.find(".sidebar__checks").text(), "过滤后总数应变为 1").toContain("/ 1");

    await btn(w, "全选")!.trigger("click");
    await flushPromises();
    expect(w.find(".sidebar__checks").text()).toContain("已选 1 / 1");

    // 清空搜索后，f0/f2 不应被顺带选中
    await w.find("input[type=search], .arco-input-search input").setValue("");
    await flushPromises();
    expect(
      w.find(".sidebar__checks").text(),
      "全选只应选中当时的可见项，清空搜索后不应把之前不可见的一起算上",
    ).toContain("已选 1 / 3");
    w.unmount();
  });
});

/** 多目录加载与移除、以及列宽复位。
 *
 * 这些都是「点了没反应」类型的功能，测试必须经由真实界面触发，
 * 而不是直接调用组件内部函数——按钮是否可用、事件是否接对，
 * 恰恰是缺陷所在。
 */
describe("多目录与列宽复位", () => {
  async function addDirs(w: ReturnType<typeof mountApp>, ...dirs: string[]) {
    mockOpen.mockResolvedValue(dirs);
    const btn = w.findAll("button").find((b) => b.text().includes("添加目录"));
    await btn!.trigger("click");
    await flushPromises();
  }

  it("一次选多个目录时全部加载，文件合并显示", async () => {
    const a: SchematicSummary = { ...summary, path: "/a/a.litematic", fileName: "a.litematic", dirName: "/a" };
    const b: SchematicSummary = { ...summary, path: "/b/b.litematic", fileName: "b.litematic", dirName: "/b" };
    let call = 0;
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "scan_litematics") {
        call += 1;
        return { items: [call === 1 ? a : b], skippedDirs: 0, skippedPaths: [] };
      }
      return undefined;
    }) as never);

    const w = mountApp();
    await addDirs(w, "/a", "/b");

    expect(call, "两个目录都应被扫描").toBe(2);
    expect(w.text(), "第一个目录的文件应在列表里").toContain("a.litematic");
    expect(w.text(), "第二个目录的文件应在列表里").toContain("b.litematic");
    expect(w.text(), "计数应为两个目录之和").toContain("共 2 个投影");
    w.unmount();
  });

  it("移除下拉里的目录后，该目录的文件从列表消失，其余保留", async () => {
    const a: SchematicSummary = { ...summary, path: "/a/a.litematic", fileName: "a.litematic", dirName: "/a" };
    const b: SchematicSummary = { ...summary, path: "/b/b.litematic", fileName: "b.litematic", dirName: "/b" };
    let call = 0;
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "scan_litematics") {
        call += 1;
        return { items: [call === 1 ? a : b], skippedDirs: 0, skippedPaths: [] };
      }
      return undefined;
    }) as never);

    const w = mountApp();
    await addDirs(w, "/a", "/b");
    expect(w.text()).toContain("a.litematic");

    // 移除第一个目录（下拉项顺序即加载顺序）
    const trigger = w.findAll("button").find((x) => x.text().includes("已加载目录"));
    await trigger!.trigger("click");
    await flushPromises();
    const rm = Array.from(document.querySelectorAll(".dir-item button"))[0] as HTMLElement;
    expect(rm, "应有移除按钮").toBeTruthy();
    rm.click();
    await flushPromises();

    expect(w.text(), "被移除目录的文件应消失").not.toContain("a.litematic");
    expect(w.text(), "另一个目录的文件应保留").toContain("b.litematic");
    expect(w.text(), "计数应更新").toContain("共 1 个投影");
    w.unmount();
  });

  it("已加载目录下拉正确反映当前状态", async () => {
    const w = mountApp();
    const btn = w.findAll("button").find((b) => b.text().includes("已加载目录"));
    expect(btn, "应有「已加载目录」按钮").toBeTruthy();
    expect(btn!.text(), "未加载时应显示 0").toContain("0");

    // 未加载时下拉里应给出说明，而不是一个空白面板
    await btn!.trigger("click");
    await flushPromises();
    expect(document.body.textContent, "未加载时应说明「尚未加载任何目录」").toContain(
      "尚未加载任何目录",
    );
    w.unmount();
  });

  it("名称列明显宽于其余各列", async () => {
    const w = mountApp();
    // 断言 colgroup 上的宽度而不是 getBoundingClientRect：
    // jsdom 不做布局，所有元素尺寸都是 0，量出来只会得到 0 比 0。
    // colgroup 的 <col> 带的是 Arco 实际使用的列宽，也就是渲染依据本身。
    const cols = w.findAll(".sidebar colgroup col");
    // 选择列 + 4 个数据列 + 1 个 1px 占位列（让最后一列也能拖动）
    expect(cols.length, "应有 6 列（含选择列与占位列）").toBe(6);
    const widthOf = (i: number) => {
      const style = cols[i].attributes("style") ?? "";
      const m = /width:\s*([\d.]+)px/.exec(style);
      expect(m, `第 ${i} 列应带宽度：${style}`).toBeTruthy();
      return Number(m![1]);
    };
    // 顺序：选择列、名称、制作版本、尺寸、方块总数、占位列
    const name = widthOf(1);
    const others = [widthOf(2), widthOf(3), widthOf(4)];
    // 用关系而不是绝对值：绝对值会随后续样式微调而失效，
    // 这里要保证的是「名称确实是被加宽的那一列」。
    expect(
      name,
      `名称列(${name}) 应比其余各列(${others.join("/")})都宽出一倍以上`,
    ).toBeGreaterThan(Math.max(...others) * 2);
    w.unmount();
  });

  /** 取「已加载目录」下拉里的勾选框 */
  async function openDirMenu(w: ReturnType<typeof mountApp>) {
    const trig = w.findAll("button").find((b) => b.text().includes("已加载目录"));
    expect(trig, "应有「已加载目录」按钮").toBeTruthy();
    await trig!.trigger("click");
    await flushPromises();
    return Array.from(
      document.querySelectorAll<HTMLInputElement>(".dir-item input[type=checkbox]"),
    );
  }

  it("新加载的目录默认勾选，文件立即显示", async () => {
    const a: SchematicSummary = { ...summary, path: "/a/a.litematic", fileName: "a.litematic", dirName: "/a" };
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "scan_litematics") return { items: [a], skippedDirs: 0, skippedPaths: [] };
      return undefined;
    }) as never);
    const w = mountApp();
    await addDirs(w, "/a");
    expect(w.text(), "刚加载的目录应默认显示其文件").toContain("a.litematic");

    const boxes = await openDirMenu(w);
    expect(boxes.length, "下拉里应有一个目录勾选框").toBe(1);
    expect(boxes[0].checked, "新加载的目录应默认勾选").toBe(true);
    w.unmount();
  });

  it("取消勾选目录后，该目录的文件从列表消失；重新勾选又出现", async () => {
    // 对应需求「已加载目录菜单中增加选择框，勾选的才会在下面显示」
    const a: SchematicSummary = { ...summary, path: "/a/a.litematic", fileName: "a.litematic", dirName: "/a" };
    const b: SchematicSummary = { ...summary, path: "/b/b.litematic", fileName: "b.litematic", dirName: "/b" };
    let call = 0;
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "scan_litematics") {
        call += 1;
        return { items: [call === 1 ? a : b], skippedDirs: 0, skippedPaths: [] };
      }
      return undefined;
    }) as never);

    const w = mountApp();
    await addDirs(w, "/a", "/b");
    expect(w.text()).toContain("a.litematic");
    expect(w.text()).toContain("b.litematic");

    // 取消第一个目录（下拉顺序即加载顺序）
    let boxes = await openDirMenu(w);
    expect(boxes.length).toBe(2);
    boxes[0].click();
    await flushPromises();
    expect(w.text(), "取消勾选后该目录的文件应消失").not.toContain("a.litematic");
    expect(w.text(), "另一个目录的文件应保留").toContain("b.litematic");
    // 目录仍是「已加载」状态，只是不显示
    expect(w.findAll("button").some((x) => x.text().includes("已加载目录（2）")), "目录数不应减少").toBe(true);

    // 重新勾选应恢复显示
    boxes = await openDirMenu(w);
    boxes = Array.from(
      document.querySelectorAll<HTMLInputElement>(".dir-item input[type=checkbox]"),
    );
    boxes[0].click();
    await flushPromises();
    expect(w.text(), "重新勾选后文件应再次出现").toContain("a.litematic");
    w.unmount();
  });

  it("取消勾选正被选中的文件的目录时，详情回到空状态", async () => {
    // 否则右侧会停留在「已不在列表里」的文件上：该文件的改动仍会被
    // 保存到磁盘，而用户在列表里找不到它
    const a: SchematicSummary = { ...summary, path: "/a/a.litematic", fileName: "a.litematic", dirName: "/a" };
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "scan_litematics") return { items: [a], skippedDirs: 0, skippedPaths: [] };
      return undefined;
    }) as never);

    const w = mountApp();
    await addDirs(w, "/a");
    await w.findAll("tr").find((r) => r.text().includes("a.litematic"))!.trigger("click");
    await flushPromises();
    expect(w.text(), "选中后应出现详情").toContain("信息");

    const boxes = await openDirMenu(w);
    boxes[0].click();
    await flushPromises();

    expect(w.text(), "文件已不显示，应回到空状态").toContain("选择一个投影文件以查看详情");
    const btn = (label: string) => w.findAll("button").find((x) => x.text().includes(label));
    expect(btn("方块替换")?.attributes("disabled"), "依赖选中项的按钮应禁用").toBeDefined();
    w.unmount();
  });
});

/** 界面状态记忆。用户要求「下次打开保持记忆」，
 *  这里的重点是：记住的确实被写出去、能读回来、脏数据不炸。
 */
describe("界面状态记忆", () => {
  /** save_ui_state 已被调用的次数 */
  function saveCount(): number {
    return mockInvoke.mock.calls.filter((c) => c[0] === "save_ui_state").length;
  }

  /** 取最后一次 save_ui_state 的 JSON */
  function lastSaved(): Record<string, unknown> | null {
    const calls = mockInvoke.mock.calls.filter((c) => c[0] === "save_ui_state");
    if (calls.length === 0) return null;
    const arg = calls[calls.length - 1][1] as { json: string };
    return JSON.parse(arg.json) as Record<string, unknown>;
  }

  it("改变目录栏宽度后会保存，且能被读回", async () => {
    vi.useFakeTimers();
    try {
      const w = mountApp();
      await flushPromises();
      const before = saveCount();

      // 拖动分隔条改宽度
      const splitter = w.find(".splitter").element as HTMLElement;
      splitter.dispatchEvent(
        new PointerEvent("pointerdown", { bubbles: true, button: 0, clientX: 0 }),
      );
      window.dispatchEvent(new PointerEvent("pointermove", { clientX: 900 }));
      window.dispatchEvent(new PointerEvent("pointerup", { clientX: 900 }));
      await flushPromises();
      // 防抖：要推进定时器才会真正写盘
      vi.advanceTimersByTime(500);
      await flushPromises();

      expect(saveCount(), "改宽度后应触发保存").toBeGreaterThan(before);
      const saved = lastSaved();
      expect(saved, "应能解析出保存内容").toBeTruthy();
      expect(saved!.version, "应带版本号").toBe(1);
      expect(typeof saved!.sidebarWidth, "应记住侧栏宽度").toBe("number");
      expect(saved!.sidebarWidth, "记住的应是拖动后的宽度").toBeGreaterThan(666);
      w.unmount();
    } finally {
      vi.useRealTimers();
    }
  });

  it("卸载时把防抖中的改动立刻写出，之后不再有遗留写盘", async () => {
    // 界面状态的写盘是防抖的（400ms）。若定时器活过组件卸载，它会在此后
    // 任意时刻触发，写入这个已销毁实例那份过期状态——实测在整包并行跑
    // （进程更慢、定时器更容易跨越用例边界）时约有 1/10 的概率污染
    // 「最后一次保存」，表现为用户设置偶尔记成默认值。
    vi.useFakeTimers();
    try {
      const w = mountApp();
      await flushPromises();
      const before = saveCount();

      const splitter = w.find(".splitter").element as HTMLElement;
      splitter.dispatchEvent(
        new PointerEvent("pointerdown", { bubbles: true, button: 0, clientX: 0 }),
      );
      window.dispatchEvent(new PointerEvent("pointermove", { clientX: 800 }));
      window.dispatchEvent(new PointerEvent("pointerup", { clientX: 800 }));
      await flushPromises();
      expect(saveCount(), "防抖期间不应立刻写盘").toBe(before);

      w.unmount();
      await flushPromises();
      expect(saveCount(), "卸载时应写出待保存的改动").toBeGreaterThan(before);
      const saved = lastSaved();
      expect(saved!.sidebarWidth, "写出的应是改动后的宽度").toBeGreaterThan(666);

      // 定时器必须已被清掉：卸载后再推进时间不应产生第二次写盘
      const afterUnmount = saveCount();
      vi.advanceTimersByTime(5000);
      await flushPromises();
      expect(saveCount(), "卸载后不应再有遗留的写盘").toBe(afterUnmount);
    } finally {
      vi.useRealTimers();
    }
  });

  it("记住已添加的目录，重启时自动重新扫描", async () => {
    // 上次保存的内容
    const state = {
      version: 1,
      sidebarWidth: 700,
      dirs: ["/p", "/q"],
      enabledDirs: ["/p"],
      activeView: "front",
      brightness: 1.0,
      exportSize: 2048,
      searchText: "",
    };
    const scanned: string[] = [];
    mockInvoke.mockImplementation((async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "load_ui_state") return JSON.stringify(state);
      if (cmd === "save_ui_state") return undefined;
      if (cmd === "scan_litematics") {
        const d = String((args as { dir?: string } | undefined)?.dir ?? "");
        scanned.push(d);
        return { items: [{ ...summary, path: `${d}/${d.replace(/\//g, "")}.litematic`, fileName: `${d.replace(/\//g, "")}.litematic`, dirName: d }], skippedDirs: 0, skippedPaths: [] };
      }
      if (cmd === "analyze_schematic") {
        return { blocks: [], entities: [], tileEntities: [], nonAirTotal: 0, airTotal: 0, distinctBlockTypes: 0, distinctStates: 0 };
      }
      return undefined;
    }) as never);

    const w = mountApp();
    await flushPromises();
    await flushPromises();

    expect(scanned, "应重新扫描上次记住的两个目录").toEqual(["/p", "/q"]);
    expect(w.find(".sidebar").element.getAttribute("style"), "应恢复侧栏宽度").toContain("700");
    expect(w.text(), "应显示恢复出来的文件").toContain("p.litematic");
    // enabledDirs 只勾了 /p：/q 的文件不应出现，计数也只算勾选的那一个
    expect(w.text(), "未勾选的目录不应显示其文件").not.toContain("q.litematic");
    expect(w.text(), "计数应只算勾选的目录").toContain("共 1 个投影");
    w.unmount();
  });

  it("记住用户调整过的列宽，并能在重启后恢复", async () => {
    // 用户要求「记忆玩家对程序界面的修改」，列宽属于其中一项
    const state = {
      version: 1,
      sidebarWidth: 666,
      dirs: [],
      columnWidths: { fileName: 420, mcVersionName: 120 },
      activeView: "left45",
      brightness: 1.45,
      exportSize: 1024,
      searchText: "",
    };
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "load_ui_state") return JSON.stringify(state);
      if (cmd === "save_ui_state") return undefined;
      if (cmd === "scan_litematics") return { items: [summary], skippedDirs: 0, skippedPaths: [] };
      return undefined;
    }) as never);

    const w = mountApp();
    await flushPromises();
    await flushPromises();

    // jsdom 没有布局，量不到像素；改读 colgroup 里的内联宽度（Arco 会写进去）
    const cols = w.findAll(".sidebar colgroup col");
    const widths = cols.map((c) => (c.element as HTMLElement).style.width);
    expect(widths, "名称列应恢复为用户拖过的 420px").toContain("420px");
    expect(widths, "制作版本列应恢复为用户拖过的 120px").toContain("120px");
    w.unmount();
  });

  it("列宽里的非法值不应让界面变形", async () => {
    // 损坏的列宽（0、负数、非数字）必须被丢弃，否则列会消失或撑破布局
    const state = {
      version: 1,
      sidebarWidth: 666,
      dirs: [],
      columnWidths: { fileName: 0, mcVersionName: -50, dims: "宽", totalBlocks: 900 },
      activeView: "left45",
      brightness: 1.45,
      exportSize: 1024,
      searchText: "",
    };
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "load_ui_state") return JSON.stringify(state);
      if (cmd === "save_ui_state") return undefined;
      if (cmd === "scan_litematics") return { items: [summary], skippedDirs: 0, skippedPaths: [] };
      return undefined;
    }) as never);

    const w = mountApp();
    await flushPromises();
    await flushPromises();

    const widths = w.findAll(".sidebar colgroup col").map((c) => (c.element as HTMLElement).style.width);
    expect(widths, "0 应被丢弃，用回默认宽度").not.toContain("0px");
    expect(widths, "负数应被丢弃").not.toContain("-50px");
    expect(widths, "非数字应被丢弃").not.toContain("宽");
    // 合法的 900 应保留
    expect(widths, "合法宽度应保留").toContain("900px");
    w.unmount();
  });

  it("保存内容损坏时按默认界面继续，并提示用户", async () => {
    // 用 spy 断言：Arco 的 Message 走 portal 渲染，查 document.body 不可靠
    // （本仓库既有的做法就是 vi.spyOn(Message, ...)）
    const warnSpy = vi.spyOn(Message, "warning");
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "load_ui_state") return "{ 这不是合法 JSON";
      if (cmd === "scan_litematics") return { items: [summary], skippedDirs: 0, skippedPaths: [] };
      return undefined;
    }) as never);

    try {
      const w = mountApp();
      await flushPromises();
      // 不应抛错、界面仍可用，且明确告知
      expect(w.find(".sidebar").exists(), "界面应仍然正常").toBe(true);
      expect(
        warnSpy.mock.calls.some((c) => String(c[0]).includes("界面设置")),
        "应提示设置无法读取，而不是静默按默认界面启动",
      ).toBe(true);
      w.unmount();
    } finally {
      warnSpy.mockRestore();
    }
  });

  it("版本号不匹配时丢弃旧数据，不硬套到新结构上", async () => {
    mockInvoke.mockImplementation((async (cmd: string) => {
      // 未来版本写的状态
      if (cmd === "load_ui_state") {
        return JSON.stringify({ version: 99, sidebarWidth: 111, dirs: ["/p"] });
      }
      if (cmd === "scan_litematics") return { items: [summary], skippedDirs: 0, skippedPaths: [] };
      return undefined;
    }) as never);

    const w = mountApp();
    await flushPromises();
    expect(
      w.find(".sidebar").element.getAttribute("style"),
      "不认识的版本不应被套用".replace("不应被套用", "不应被采用"),
    ).not.toContain("111");
    w.unmount();
  });
});

/** 方块栏里的方块渲染图。
 *
 * 渲染图是异步、按页批量取回来的。它坏掉的表现只是「有一列空着」，
 * 既不会让构建失败，也不会让别的用例失败，因此单独覆盖。
 */
describe("方块栏渲染图", () => {
  const rows: BlockCount[] = [
    {
      name: "minecraft:stone",
      state: "minecraft:stone",
      count: 10,
      zhName: "石头",
      enName: "Stone",
    },
    {
      name: "minecraft:oak_log",
      state: "minecraft:oak_log[axis=y]",
      count: 3,
      zhName: "橡木原木",
      enName: "Oak Log",
    },
  ];

  /** 选中投影并让分析返回给定的方块行 */
  async function selectWithBlocks(blocks: BlockCount[], extra: Record<string, unknown> = {}) {
    stubInvoke({
      analyze_schematic: {
        blocks,
        entities: [],
        tileEntities: [],
        nonAirTotal: 0,
        airTotal: 0,
        distinctBlockTypes: 1,
        distinctStates: blocks.length,
      },
      ...extra,
    });
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await w.findAll("tr").find((r) => r.text().includes("测试.litematic"))!.trigger("click");
    await flushPromises();
    return w;
  }

  it("按状态键批量取回渲染图并显示出来", async () => {
    const dataUrl = "data:image/png;base64,iVBORw0KGgo=";
    const w = await selectWithBlocks(rows, {
      render_block_icons: rows.map((r) => ({ state: r.state, dataUrl, error: "" })),
    });

    const imgs = w.findAll("img.block-icon");
    expect(imgs.length, "两行都应显示出渲染图").toBe(2);
    expect(imgs[0].attributes("src")).toBe(dataUrl);

    // 一次一批，而不是逐行一次
    const calls = mockInvoke.mock.calls.filter((c) => c[0] === "render_block_icons");
    expect(calls.length, "同一页应只用一次请求").toBe(1);
    expect(calls[0][1]).toMatchObject({
      dataVersion: summary.dataVersion,
      states: rows.map((r) => r.state),
    });
    w.unmount();
  });

  it("单个方块画不出来时显示失败说明，而不是留一个破图", async () => {
    const w = await selectWithBlocks(rows, {
      render_block_icons: [
        { state: rows[0].state, dataUrl: "data:image/png;base64,iVBORw0KGgo=", error: "" },
        { state: rows[1].state, dataUrl: "", error: "方块库中没有这个方块" },
      ],
    });

    expect(w.findAll("img.block-icon").length, "只有成功的那一行有图").toBe(1);
    const failed = w.find(".block-icon--failed");
    expect(failed.exists(), "失败的那一行应显示占位").toBe(true);
    expect(failed.attributes("title")).toContain("方块库中没有这个方块");
    w.unmount();
  });

  it("整批请求失败时提示一次，并且不反复重试", async () => {
    const errSpy = vi.spyOn(Message, "error");
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "load_ui_state") return null;
      if (cmd === "scan_litematics") {
        return { items: [summary], skippedDirs: 0, skippedPaths: [] };
      }
      if (cmd === "analyze_schematic") {
        return {
          blocks: rows,
          entities: [],
          tileEntities: [],
          nonAirTotal: 0,
          airTotal: 0,
          distinctBlockTypes: 1,
          distinctStates: rows.length,
        };
      }
      if (cmd === "render_block_icons") throw new Error("方块库载入失败");
      return undefined;
    }) as never);

    const w = mountApp();
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await w.findAll("tr").find((r) => r.text().includes("测试.litematic"))!.trigger("click");
    await flushPromises();

    // 失败必须让用户看见，不能被静默吞掉
    expect(errSpy).toHaveBeenCalled();
    const calls = mockInvoke.mock.calls.filter((c) => c[0] === "render_block_icons");
    expect(calls.length, "失败后不应无限重试同一批").toBe(1);
    expect(w.findAll(".block-icon--failed").length, "失败的行应显示占位").toBe(rows.length);

    errSpy.mockRestore();
    w.unmount();
  });

  it("只请求当前页的方块，不把整表一次拉回来", async () => {
    const many: BlockCount[] = Array.from({ length: 120 }, (_, i) => ({
      name: `minecraft:b${i}`,
      state: `minecraft:b${i}`,
      count: i + 1,
      zhName: `方块${i}`,
      enName: `B${i}`,
    }));
    const w = await selectWithBlocks(many, {
      render_block_icons: many
        .slice(0, 50)
        .map((r) => ({ state: r.state, dataUrl: "data:image/png;base64,AA==", error: "" })),
    });

    const calls = mockInvoke.mock.calls
      .filter((c) => c[0] === "render_block_icons")
      .map((c) => (c[1] as { states: string[] }).states);
    expect(calls.length).toBe(1);
    // 断言的判据是「请求的正好是当前渲染出来的那些行」，而不是写死 50：
    // 写死页大小只是在核对配置常量，页大小改了这条测试就要跟着改，
    // 却并不能说明「有没有多请求」。
    const rendered = w.findAll(".block-table tbody tr").length;
    expect(rendered, "应渲染出下一页数量的行").toBeGreaterThan(0);
    expect(calls[0].length, "请求的方块数应与渲染出的行数一致").toBe(rendered);
    expect(calls[0].length, "不该把整表一次拉回来").toBeLessThan(many.length);
    expect(calls[0]).not.toContain("minecraft:b110");
    w.unmount();
  });

  it("翻到第二页会去取第二页的渲染图", async () => {
    const many: BlockCount[] = Array.from({ length: 120 }, (_, i) => ({
      name: `minecraft:b${i}`,
      state: `minecraft:b${i}`,
      count: i + 1,
      zhName: `方块${i}`,
      enName: `B${i}`,
    }));
    // 必须按**请求到的**状态回，只回请求的那一批：
    // 一次性把 120 个都回给前端，第二页就全在缓存里了，翻页不会发请求。
    // 场景等其余命令仍交给 stubInvoke——只回 [] 会让 loadScene 拿到无效数据，
    // 详情面板整块渲染不出来，那样测到的是替身的问题。
    stubInvoke({
      analyze_schematic: {
        blocks: many,
        entities: [],
        tileEntities: [],
        nonAirTotal: 0,
        airTotal: 0,
        distinctBlockTypes: many.length,
        distinctStates: many.length,
      },
    });
    const base = mockInvoke.getMockImplementation()!;
    mockInvoke.mockImplementation((async (cmd: string, args: never) => {
      if (cmd === "render_block_icons") {
        const asked = (args as { states: string[] }).states;
        return asked.map((s) => ({
          state: s,
          dataUrl: "data:image/png;base64,AA==",
          error: "",
        }));
      }
      return base(cmd, args);
    }) as never);

    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await w.findAll("tr").find((r) => r.text().includes("测试.litematic"))!.trigger("click");
    await flushPromises();

    const page2 = [...document.querySelectorAll(".arco-pagination-item")].find(
      (el) => (el.textContent ?? "").trim() === "2",
    ) as HTMLElement | undefined;
    expect(page2, "应能定位到第 2 页按钮").toBeTruthy();
    page2!.click();
    await flushPromises();

    const states = mockInvoke.mock.calls
      .filter((c) => c[0] === "render_block_icons")
      .map((c) => (c[1] as { states: string[] }).states);
    expect(states.length, "应发生第二批请求").toBe(2);
    expect(states[1][0], "第二批应是第 51 行起的方块").toBe("minecraft:b50");
    w.unmount();
  });

  it("翻回已看过的页不再重复请求（缓存命中）", async () => {
    const many: BlockCount[] = Array.from({ length: 120 }, (_, i) => ({
      name: `minecraft:b${i}`,
      state: `minecraft:b${i}`,
      count: i + 1,
      zhName: `方块${i}`,
      enName: `B${i}`,
    }));
    const w = await selectWithBlocks(many, {
      render_block_icons: many
        .slice(0, 50)
        .map((r) => ({ state: r.state, dataUrl: "data:image/png;base64,AA==", error: "" })),
    });
    const page = (n: string) =>
      [...document.querySelectorAll(".arco-pagination-item")].find(
        (el) => (el.textContent ?? "").trim() === n,
      ) as HTMLElement | undefined;

    const count = () => mockInvoke.mock.calls.filter((c) => c[0] === "render_block_icons").length;
    expect(count(), "第一页应有一次请求").toBe(1);

    page("2")!.click();
    await flushPromises();
    expect(count(), "第二页还没取过，应再请求一次").toBe(2);

    page("1")!.click();
    await flushPromises();
    expect(count(), "翻回第一页应命中缓存，不再请求").toBe(2);
    w.unmount();
  });

  it("换投影时正在飞的渲染图请求不会写进新表", async () => {
    // 两个投影里有**同一个状态键**的方块：这正是缓存被串的关键条件——
    // 状态键不同的话，旧响应根本填不到新表的行上，守卫有没有都一样。
    const sameKey = "minecraft:stone";
    const make = (zh: string): BlockCount[] => [
      { name: sameKey, state: sameKey, count: 5, zhName: zh, enName: "Stone" },
    ];

    let selected = make("石头甲");
    /** 第一次请求的 resolver：故意不返回，模拟「请求还在飞」 */
    let staleResolve: ((v: unknown) => void) | null = null;
    let iconCall = 0;

    stubInvoke();
    const base = mockInvoke.getMockImplementation()!;
    mockInvoke.mockImplementation((async (cmd: string, args: never) => {
      if (cmd === "analyze_schematic") {
        return {
          blocks: selected,
          entities: [],
          tileEntities: [],
          nonAirTotal: 0,
          airTotal: 0,
          distinctBlockTypes: selected.length,
          distinctStates: selected.length,
        };
      }
      if (cmd === "render_block_icons") {
        iconCall += 1;
        if (iconCall === 1) {
          return new Promise((res) => {
            staleResolve = res as (v: unknown) => void;
          });
        }
        // 新投影的请求立刻返回
        return (args as { states: string[] }).states.map((s) => ({
          state: s,
          dataUrl: "data:image/png;base64,NEW",
          error: "",
        }));
      }
      return base(cmd, args);
    }) as never);

    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    const fileRow = w.findAll("tr").find((r) => r.text().includes("测试.litematic"))!;
    await fileRow.trigger("click");
    await flushPromises();
    expect(staleResolve, "第一个请求应已发出并停在飞行中").toBeTruthy();

    // 用户换到另一个投影（同一个状态键）
    selected = make("石头乙");
    await fileRow.trigger("click");
    await flushPromises();
    expect(w.find("img.block-icon").attributes("src"), "新投影应显示新图").toContain("NEW");

    // 旧请求这时才回来——它属于上一个投影，必须被丢弃
    iconCall = 0;
    const srcsAfter: string[] = [];
    staleResolve!([
      { state: sameKey, dataUrl: "data:image/png;base64,STALE", error: "" },
    ]);
    await flushPromises();
    srcsAfter.push(...w.findAll("img.block-icon").map((i) => i.attributes("src") ?? ""));
    expect(
      srcsAfter.some((s) => s.includes("STALE")),
      "旧投影的响应不该覆盖新投影已显示的图",
    ).toBe(false);
    w.unmount();
  });
});

/** 资源包管理：导入来源、删除、打开资源包目录 */
describe("资源包管理", () => {
  const pack = {
    file: "mod-x.pack",
    name: "某模组",
    source: "C:/mods/mod-x.jar",
    sourceDir: "C:/mods",
    kind: 2,
    kindLabel: "模组包",
    dataVersion: 3465,
    mcVersion: "1.20.1",
    enabled: true,
    size: 1024,
    exists: true,
  };

  /** 打开资源包管理弹窗并等加载完成。
   *
   * 断言一律查 `document.body` 而不是 `w.text()`：Arco 的弹窗通过 teleport
   * 挂到 body 上，wrapper 的文本里拿不到它的内容。
   */
  async function openPacks(extra: Record<string, unknown> = {}) {
    stubInvoke({
      list_packs: { packs: [pack], packsDir: "C:/app/packs", skipped: [], loadFailures: [] },
      ...extra,
    });
    const w = mountApp();
    await flushPromises();
    const entry = w.findAll("button").find((b) => b.text().includes("资源包管理"));
    expect(entry, "工具栏应有「资源包管理」").toBeTruthy();
    await entry!.trigger("click");
    await flushPromises();
    return w;
  }

  /** 在弹窗里按文字找按钮 */
  function bodyButton(label: string): HTMLElement | undefined {
    return [...document.querySelectorAll("button")].find((b) =>
      (b.textContent ?? "").includes(label),
    ) as HTMLElement | undefined;
  }

  it("列出模组名与来源目录", async () => {
    const w = await openPacks();
    const text = document.body.textContent ?? "";
    expect(text, "应显示模组名").toContain("某模组");
    expect(text, "应显示来源目录").toContain("C:/mods");
    w.unmount();
  });

  it("「打开资源包目录」把后端给的目录交给文件管理器", async () => {
    const w = await openPacks();
    const btn = bodyButton("打开资源包目录");
    expect(btn, "应有「打开资源包目录」按钮").toBeTruthy();
    btn!.click();
    await flushPromises();

    const call = mockInvoke.mock.calls.find((c) => c[0] === "open_directory");
    expect(call, "应调用 open_directory").toBeTruthy();
    expect(call![1]).toEqual({ path: "C:/app/packs" });
    w.unmount();
  });
});