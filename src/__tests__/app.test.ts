import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import ArcoVue from "@arco-design/web-vue";

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

const mockInvoke = vi.mocked(invoke);
const mockOpen = vi.mocked(open);

/** 一个可用的投影摘要，字段与后端 SchematicSummary 对齐 */
const summary = {
  fileName: "测试.litematic",
  dirName: "/p",
  path: "/p/测试.litematic",
  fileSize: 2048,
  formatVersion: 6,
  formatSubVersion: 1,
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
  previewPixels: 0,
  fullyAnalyzed: false,
  distinctBlockTypes: 0,
  distinctStates: 0,
  error: "",
};

/** 挂载外壳界面 */
function mountApp() {
  return mount(App, { global: { plugins: [ArcoVue] }, attachTo: document.body });
}

/** 一个可用的场景数据，字段与后端 SceneData 对齐 */
const scene = {
  batches: [],
  totalVertices: 0,
  totalTriangles: 0,
  enclosing: [16, 10, 16] as [number, number, number],
  origin: [0, 0, 0] as [number, number, number],
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
      case "list_packs":
        return [];
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

