"""Generate the application icon (multi-size .ico) with Pillow."""
import os
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(os.path.dirname(HERE), 'res', 'app.ico')


def rounded(draw, box, r, fill):
    x0, y0, x1, y1 = box
    draw.rounded_rectangle([x0, y0, x1, y1], radius=r, fill=fill)


def render(size):
    S = size * 8                      # 超采样绘制后再缩小
    img = Image.new('RGBA', (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # 背景：深蓝渐变圆角方块
    rounded(d, (0, 0, S - 1, S - 1), S // 5, (47, 111, 237, 255))
    # 顶部高光
    d.rounded_rectangle([0, 0, S - 1, int(S * 0.5)], radius=S // 5, fill=(82, 140, 246, 255))
    d.rectangle([0, int(S * 0.28), S - 1, int(S * 0.5)], fill=(82, 140, 246, 255))

    # 等轴测立方体
    cx, cy = S * 0.5, S * 0.53
    w = S * 0.30                      # 半宽
    hh = S * 0.17                     # 半高（顶面菱形）
    ht = S * 0.30                     # 侧面高度

    top = [(cx, cy - hh * 2 - ht * 0.32), (cx + w, cy - hh - ht * 0.32),
           (cx, cy - ht * 0.32), (cx - w, cy - hh - ht * 0.32)]
    left = [(cx - w, cy - hh - ht * 0.32), (cx, cy - ht * 0.32),
            (cx, cy + ht * 0.68), (cx - w, cy + ht * 0.68 - hh)]
    right = [(cx + w, cy - hh - ht * 0.32), (cx, cy - ht * 0.32),
             (cx, cy + ht * 0.68), (cx + w, cy + ht * 0.68 - hh)]

    d.polygon(left, fill=(219, 231, 255, 255))
    d.polygon(right, fill=(168, 195, 246, 255))
    d.polygon(top, fill=(255, 255, 255, 255))

    # 轮廓
    for poly in (top, left, right):
        d.line(poly + [poly[0]], fill=(47, 111, 237, 255), width=max(2, S // 110))

    # 立方体上的网格线（暗示"方块结构"）
    d.line([(cx, cy - hh - ht * 0.32), (cx, cy + ht * 0.68)], fill=(47, 111, 237, 90),
           width=max(1, S // 200))

    return img.resize((size, size), Image.LANCZOS)


def main():
    sizes = [16, 24, 32, 48, 64, 128, 256]
    imgs = [render(s) for s in sizes]
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    imgs[-1].save(OUT, format='ICO', sizes=[(s, s) for s in sizes])
    print('wrote', OUT, os.path.getsize(OUT), 'bytes')


if __name__ == '__main__':
    main()
