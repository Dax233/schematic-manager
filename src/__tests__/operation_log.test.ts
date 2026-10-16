/**
 * 操作日志行的解析。
 *
 * 存储格式由后端 `backup::append_log` 决定：`纳秒时间戳 \t 说明`。
 * 这里钉住的是「界面据此分行展示」所依赖的两个前提：时间戳能单独取出来
 * （且不是数字时不当成时间），说明里的动作词与细节能分开。
 */
import { describe, expect, it } from "vitest";
import { parseLogLine } from "../utils/operation_log";

describe("操作日志行解析", () => {
  it("拆出纳秒时间戳、动作与细节", () => {
    const e = parseLogLine(
      "17921400658821747800\t删除到回收站 24 个文件：a.litematic、b.litematic",
    );
    expect(e.timeNs).toBe("17921400658821747800");
    expect(e.action).toBe("删除到回收站");
    expect(e.detail).toBe("24 个文件：a.litematic、b.litematic");
  });

  it("动作只有一个词时细节为空", () => {
    const e = parseLogLine("17921400658821747800\t还原");
    expect(e.action).toBe("还原");
    expect(e.detail).toBe("");
  });

  it("没有制表符时整行当作说明，时间留空", () => {
    // 手工编辑过日志文件、或旧版本写下的行
    const e = parseLogLine("替换 a.litematic：石头 -> 泥土");
    expect(e.timeNs).toBe("");
    expect(e.action).toBe("替换");
    expect(e.detail).toBe("a.litematic：石头 -> 泥土");
  });

  it("时间戳不是数字时不当成时间，但其余部分照常拆开", () => {
    // 测试替身与手工编辑都可能写出这种行；若把「2026-10-15 12:00:00」
    // 当成纳秒时间戳，会显示成 1970 年
    const e = parseLogLine("2026-10-15 12:00:00\t替换 a.litematic：石头 -> 泥土");
    expect(e.timeNs).toBe("");
    expect(e.action).toBe("替换");
    expect(e.detail).toBe("a.litematic：石头 -> 泥土");
  });
});
