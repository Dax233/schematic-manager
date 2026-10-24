import { afterEach, describe, expect, it, vi } from "vitest";
import { ref } from "vue";
import { mount, flushPromises } from "@vue/test-utils";
import ArcoVue from "@arco-design/web-vue";
import i18n from "../i18n";
import ContainerItems from "../components/ContainerItems.vue";
import { containerItems } from "../api";
import type { ContainerItem } from "../types";

const state = {selected: ref({path: "/a.litematic", fileName: "a.litematic", error: null}), saving: ref(false), activeTabId: ref("a")};
vi.mock("../store", () => ({useAppStore: () => state}));
vi.mock("../api", () => ({containerItems: vi.fn()}));
afterEach(() => {document.body.innerHTML = ""; vi.clearAllMocks();});
describe("容器统计弹窗", () => {
  it("展示物品数量与盒数并支持搜索", async () => {
    vi.mocked(containerItems).mockResolvedValue([{name:"minecraft:stone",zhName:"石头",enName:"Stone",count:1728,boxes:1}, {name:"mod:filter",zhName:"",enName:"",count:2,boxes:null}]);
    const w = mount(ContainerItems,{attachTo:document.body,global:{plugins:[ArcoVue,i18n]}});
    await w.find("button").trigger("click");await flushPromises();
    expect(containerItems).toHaveBeenCalledWith("/a.litematic");
    expect(document.body.textContent).toContain("1728");expect(document.body.textContent).toContain("mod:filter");
    const input=document.querySelector<HTMLInputElement>(".arco-modal input")!;
    input.value="石头";input.dispatchEvent(new Event("input"));await flushPromises();
    expect(document.body.textContent).not.toContain("mod:filter");
    w.unmount();
  });
  it("切换编辑页后忽略旧请求，并显示新请求错误", async () => {
    let finish!: (rows:ContainerItem[]) => void;
    vi.mocked(containerItems).mockImplementationOnce(() => new Promise(resolve => {finish=resolve;})).mockRejectedValueOnce(new Error("读取失败"));
    const w=mount(ContainerItems,{attachTo:document.body,global:{plugins:[ArcoVue,i18n]}});
    await w.find("button").trigger("click");
    state.activeTabId.value="b";state.selected.value={path:"/b.litematic",fileName:"b.litematic",error:null};
    await flushPromises();await w.find("button").trigger("click");await flushPromises();
    finish([{name:"old-item",zhName:"",enName:"",count:5,boxes:1}]);await flushPromises();
    expect(document.body.textContent).toContain("读取失败");expect(document.body.textContent).not.toContain("old-item");
    w.unmount();
  });
});
