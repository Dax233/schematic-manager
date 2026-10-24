import { describe, expect, it, vi } from "vitest";
import { computed, defineComponent, ref } from "vue";
import { mount } from "@vue/test-utils";
import { createVersionConvertSlice } from "../store/versionConvert";
import { applyConversion } from "../api";
import type { ConvertOutcome, SchematicSummary } from "../types";

vi.mock("../api", () => ({ applyConversion: vi.fn(), analyzeConversion: vi.fn(), convertTargets: vi.fn(), searchBlocks: vi.fn() }));
vi.mock("@arco-design/web-vue", () => ({ Message: { success: vi.fn(), error: vi.fn(), warning: vi.fn() } }));

describe("转换报告的编辑页归属", () => {
  it("后台转换晚于另一页完成时，分别保留两页的替换报告", async () => {
    const selected = ref({path: "/a.litematic"} as SchematicSummary);
    let slice!: ReturnType<typeof createVersionConvertSlice>;
    let token = "a";
    const wrapper = mount(defineComponent({setup() {
      slice = createVersionConvertSlice({selected, files: computed(() => []), rescanAll: async () => {},
        activeEditorToken: () => token, refreshEditorContent: vi.fn(), pruneChecks: vi.fn(), confirmDiscard: async () => true});
      return () => null;
    }}));
    const report = (path: string): ConvertOutcome => ({outputPath: path, replacedKinds: 0, replacedBlocks: 0, skippedKinds: 0,
      compatibilityReplacements: [{originalId: "minecraft:pale_oak_log", replacementId: "minecraft:birch_log", count: 1, location: path, path, slot: 0}]});
    let finishA!: (result: ConvertOutcome) => void;
    vi.mocked(applyConversion).mockImplementationOnce(() => new Promise(resolve => {finishA = resolve;})).mockResolvedValueOnce(report("/b-out.litematic"));
    slice.convTargetIdx.value = 0;
    slice.convAnalyzed.value = true;
    const pendingA = slice.doConvert();
    const snapshotA = slice.captureConvertEditor();
    slice.clearConvertEditor();
    expect(slice.convReportOpen.value).toBe(false);
    token = "b";
    selected.value = {path: "/b.litematic"} as SchematicSummary;
    slice.convTargetIdx.value = 0;
    slice.convAnalyzed.value = true;
    await slice.doConvert();
    await finishA(report("/a-out.litematic"));
    await pendingA;
    expect(slice.convReport.value?.outputPath).toBe("/b-out.litematic");
    expect(slice.convReportOpen.value).toBe(true);
    const snapshotB = slice.captureConvertEditor();
    slice.clearConvertEditor();
    slice.restoreConvertEditor(snapshotA);
    expect(slice.convReport.value?.outputPath).toBe("/a-out.litematic");
    slice.convReportOpen.value = false;
    slice.restoreConvertEditor(snapshotB);
    expect(slice.convReportOpen.value).toBe(true);
    wrapper.unmount();
  });
});
