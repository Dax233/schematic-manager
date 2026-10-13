import { describe, expect, it } from "vitest";
import { NamePool, previewStem } from "../render/export_name";

describe("导出文件名", () => {
  it("去掉 .litematic 后缀并带上尺寸", () => {
    expect(previewStem("A.litematic", 1024)).toBe("A_preview_1024");
    // 后缀大小写不敏感：真实文件里有 .LITEMATIC
    expect(previewStem("B.LITEMATIC", 512)).toBe("B_preview_512");
    // 不含后缀的文件名不应被截断
    expect(previewStem("无后缀", 2048)).toBe("无后缀_preview_2048");
    // 名字里本来就有点，只去末尾的 .litematic
    expect(previewStem("v1.2.litematic", 3072)).toBe("v1.2_preview_3072");
  });

  it("同一批内重名时自动加序号，互不覆盖", () => {
    // 这正是「导出 10 张最后只剩 1 张」的防线：不同目录下可以有同名投影
    const pool = new NamePool();
    const names = [
      pool.take("A_preview_512", ".png"),
      pool.take("A_preview_512", ".png"),
      pool.take("A_preview_512", ".png"),
    ];
    expect(names).toEqual([
      "A_preview_512.png",
      "A_preview_512 (2).png",
      "A_preview_512 (3).png",
    ]);
    expect(new Set(names).size, "三个名字必须互不相同").toBe(3);
  });

  it("不同名字互不影响", () => {
    const pool = new NamePool();
    expect(pool.take("A_preview_512", ".png")).toBe("A_preview_512.png");
    expect(pool.take("B_preview_512", ".png")).toBe("B_preview_512.png");
    expect(pool.take("A_preview_512", ".png")).toBe("A_preview_512 (2).png");
    expect(pool.take("B_preview_512", ".png")).toBe("B_preview_512 (2).png");
  });

  it("大量同名也能给出唯一名字", () => {
    const pool = new NamePool();
    const seen = new Set<string>();
    for (let i = 0; i < 300; i++) {
      const n = pool.take("same", ".png");
      expect(seen.has(n), `第 ${i} 个名字 ${n} 重复了`).toBe(false);
      seen.add(n);
    }
    expect(seen.size).toBe(300);
  });

  it("序号跳过的位置会被复用（前一个名字未被占用时）", () => {
    // 先占 (2)，再占原始名，此时应给出 (3) 而不是重复 (2)
    const pool = new NamePool();
    expect(pool.take("X", ".png")).toBe("X.png");
    expect(pool.take("X", ".png")).toBe("X (2).png");
    expect(pool.take("X", ".png")).toBe("X (3).png");
  });
});
