#include "render.h"
#include "fs.h"
#include <zlib.h>
#include <cmath>

namespace render {

// 全局亮度系数（放在命名空间文件作用域，不能放进匿名命名空间，否则外部无法链接）
static float g_brightness = 1.0f;
void  SetBrightness(float b) { g_brightness = (b < 0.5f) ? 0.5f : (b > 2.5f ? 2.5f : b); }
float GetBrightness() { return g_brightness; }

// ------------------------------------------------------------------ Image
void Image::Resize(int width, int height, u32 fill) {
    w = width; h = height;
    px.assign((size_t)w * (size_t)h, fill);
}

u32 MissingTextureColor(int x, int y) {
    // 我的世界默认"缺失贴图"紫黑格：每 8 像素交替
    bool magenta = (((x >> 3) + (y >> 3)) & 1) != 0;
    return magenta ? 0xFFF800F8u : 0xFF180018u;
}

const char* ViewIdName(ViewPreset v) {
    switch (v) {
        case VIEW_FRONT:   return "front";
        case VIEW_LEFT45:  return "left45";
        case VIEW_RIGHT45: return "right45";
        case VIEW_BACK:    return "back";
        default:           return "?";
    }
}

static void ViewAngles(int preset, float& yaw, float& pitch) {
    switch (preset) {
        case VIEW_FRONT:   yaw = 0.0f;    pitch = 12.0f; break;
        case VIEW_LEFT45:  yaw = -45.0f;  pitch = 30.0f; break;
        case VIEW_RIGHT45: yaw = 45.0f;   pitch = 30.0f; break;
        case VIEW_BACK:    yaw = 180.0f;  pitch = 12.0f; break;
        default:           yaw = 45.0f;   pitch = 30.0f; break;
    }
}

// ------------------------------------------------------------------ 几何
namespace {

struct V3 { float x, y, z; };
static inline V3 Sub(const V3& a, const V3& b) { return { a.x - b.x, a.y - b.y, a.z - b.z }; }
static inline V3 Cross(const V3& a, const V3& b) {
    return { a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x };
}
static inline float Dot(const V3& a, const V3& b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
static inline V3 Norm(const V3& a) {
    float l = sqrtf(Dot(a, a));
    if (l < 1e-9f) return { 0, 0, 0 };
    return { a.x / l, a.y / l, a.z / l };
}

// 一个待光栅化的面
struct Quad {
    V3    p[4];
    float au[4], av[4];      // 图集像素坐标
    u32   texIdx;            // 0xFFFFFFFF = 缺失
    bool  transparent;
    int   face;              // 用于明暗
    V3    normal;
};

struct AABB { float x0, y0, z0, x1, y1, z1; };

// 每个面的 4 个角（元素空间）与对应 uv（u1,v1,u2,v2 顺序见下方函数）
struct FaceCorners { V3 c[4]; float uv[4][2]; };

static void BuildFaceCorners(int face, const V3& a, const V3& b, const float uv[4], FaceCorners& out) {
    // a = from, b = to（元素空间 0..16）
    float x1 = a.x, y1 = a.y, z1 = a.z, x2 = b.x, y2 = b.y, z2 = b.z;
    float u1 = uv[0], v1 = uv[1], u2 = uv[2], v2 = uv[3];
    auto set = [&](int i, float x, float y, float z, float u, float v) {
        out.c[i] = { x, y, z }; out.uv[i][0] = u; out.uv[i][1] = v;
    };
    switch (face) {
        case mc::F_DOWN:   // u 随 +x, v 随 -z
            set(0, x1, y1, z2, u1, v1);
            set(1, x2, y1, z2, u2, v1);
            set(2, x2, y1, z1, u2, v2);
            set(3, x1, y1, z1, u1, v2);
            break;
        case mc::F_UP:     // u 随 +x, v 随 +z
            set(0, x1, y2, z1, u1, v1);
            set(1, x2, y2, z1, u2, v1);
            set(2, x2, y2, z2, u2, v2);
            set(3, x1, y2, z2, u1, v2);
            break;
        case mc::F_NORTH:  // u 随 -x, v 随 -y
            set(0, x2, y2, z1, u1, v1);
            set(1, x1, y2, z1, u2, v1);
            set(2, x1, y1, z1, u2, v2);
            set(3, x2, y1, z1, u1, v2);
            break;
        case mc::F_SOUTH:  // u 随 +x, v 随 -y
            set(0, x1, y2, z2, u1, v1);
            set(1, x2, y2, z2, u2, v1);
            set(2, x2, y1, z2, u2, v2);
            set(3, x1, y1, z2, u1, v2);
            break;
        case mc::F_WEST:   // u 随 +z, v 随 -y
            set(0, x1, y2, z1, u1, v1);
            set(1, x1, y2, z2, u2, v1);
            set(2, x1, y1, z2, u2, v2);
            set(3, x1, y1, z1, u1, v2);
            break;
        case mc::F_EAST:   // u 随 -z, v 随 -y
            set(0, x2, y2, z2, u1, v1);
            set(1, x2, y2, z1, u2, v1);
            set(2, x2, y1, z1, u2, v2);
            set(3, x2, y1, z2, u1, v2);
            break;
        default: break;
    }
}

static V3 FaceNormalModel(int face) {
    switch (face) {
        case mc::F_DOWN:  return { 0, -1, 0 };
        case mc::F_UP:    return { 0, 1, 0 };
        case mc::F_NORTH: return { 0, 0, -1 };
        case mc::F_SOUTH: return { 0, 0, 1 };
        case mc::F_WEST:  return { -1, 0, 0 };
        case mc::F_EAST:  return { 1, 0, 0 };
        default:          return { 0, 0, 0 };
    }
}

// 方块状态带来的旋转（Minecraft 语义，绕方块中心 8,8,8）
static V3 RotateBlock(const V3& p, int rotX, int rotY) {
    V3 q = p;
    switch (((rotX % 360) + 360) % 360) {
        case 90:  q = { q.x, 16 - q.z, q.y }; break;
        case 180: q = { q.x, 16 - q.y, 16 - q.z }; break;
        case 270: q = { q.x, q.z, 16 - q.y }; break;
        default: break;
    }
    switch (((rotY % 360) + 360) % 360) {
        case 90:  q = { 16 - q.z, q.y, q.x }; break;
        case 180: q = { 16 - q.x, q.y, 16 - q.z }; break;
        case 270: q = { q.z, q.y, 16 - q.x }; break;
        default: break;
    }
    return q;
}

// 元素自身的 rotation
static V3 RotateElement(const V3& p, const mc::ModelElement& e) {
    if (!e.hasRot) return p;
    V3 o = { e.rotOrigin[0], e.rotOrigin[1], e.rotOrigin[2] };
    V3 d = Sub(p, o);
    V3 r = d;
    if (e.rotKind == 0) {
        float a = e.angle * 3.14159265358979f / 180.0f;
        float c = cosf(a), s = sinf(a);
        switch (e.axis) {
            case 0: r = { d.x, d.y * c - d.z * s, d.y * s + d.z * c }; break;
            case 1: r = { d.x * c + d.z * s, d.y, -d.x * s + d.z * c }; break;
            default:r = { d.x * c - d.y * s, d.x * s + d.y * c, d.z }; break;
        }
    } else {
        auto rot = [&](V3 v, int axis, float deg) {
            float a = deg * 3.14159265358979f / 180.0f;
            float c = cosf(a), s = sinf(a);
            switch (axis) {
                case 0: return V3{ v.x, v.y * c - v.z * s, v.y * s + v.z * c };
                case 1: return V3{ v.x * c + v.z * s, v.y, -v.x * s + v.z * c };
                default:return V3{ v.x * c - v.y * s, v.x * s + v.y * c, v.z };
            }
        };
        r = rot(r, 0, e.rx);
        r = rot(r, 1, e.ry);
        r = rot(r, 2, e.rz);
    }
    return { r.x + o.x, r.y + o.y, r.z + o.z };
}

// 由方块 id + 属性判断是否"完全不透明整方块"（用于邻面剔除）
struct OpaqueCache {
    const mc::BlockDB* db = nullptr;
    const mc::VersionDef* ver = nullptr;
    std::vector<i8> cache;    // 每个调色板索引一项
};

static bool ComputeOpaque(const mc::BlockDB& db, const mc::VersionDef& ver, const ltm::BlockState& st) {
    std::vector<mc::BlockDB::ResolvedModel> res;
    db.ResolveModels(ver, st, res);
    if (res.size() != 1 || !res[0].model) return false;
    const mc::Model& m = *res[0].model;
    if (m.translucent) return false;
    if (m.elements.size() != 1) return false;
    const mc::ModelElement& e = m.elements[0];
    if (e.rescale || e.hasRot) return false;
    if (e.from[0] != 0 || e.from[1] != 0 || e.from[2] != 0) return false;
    if (e.to[0] != 16 || e.to[1] != 16 || e.to[2] != 16) return false;
    if (e.faceMask != 0x3F) return false;
    for (const mc::ModelFace& f : e.faces) {
        if (f.texStrIdx == 0xFFFFFFFF) return false;
        // 找该贴图在图集中的 an
        std::string path = db.Str(f.texStrIdx);
        u32 ti2 = db.FindTexture(ver, path);
        if (ti2 == 0xFFFFFFFFu) return false;
        if (db.TextureAt(ti2).hasAlpha) return false;
    }
    return true;
}

} // namespace

// ------------------------------------------------------------------ 采样
namespace {
static inline u32 SampleAtlas(const mc::BlockDB& db, const mc::TextureInfo& ti,
                              const u8* pix, float ax, float ay) {
    int x = (int)ax, y = (int)ay;
    if (x < 0) x = 0;
    if (y < 0) y = 0;
    if (x >= ti.w) x = ti.w - 1;
    if (y >= ti.h) y = ti.h - 1;
    size_t off = ti.offset + ((size_t)y * ti.w + (size_t)x) * 4;
    if (off + 3 >= ti.offset + ti.len) return 0xFFFFFFFFu;
    const u8* p = pix + off;
    return ((u32)p[3] << 24) | ((u32)p[0] << 16) | ((u32)p[1] << 8) | (u32)p[2];
}


static inline u32 ShadeColor(u32 c, float f) {
    f *= g_brightness;
    u32 a = (c >> 24) & 0xFF;
    u32 r = (u32)std::min(255.0f, ((c >> 16) & 0xFF) * f);
    u32 g = (u32)std::min(255.0f, ((c >> 8) & 0xFF) * f);
    u32 b = (u32)std::min(255.0f, (c & 0xFF) * f);
    return (a << 24) | (r << 16) | (g << 8) | b;
}

// 生物群系默认染色（平原）
static inline u32 ApplyTint(u32 c, int tintindex) {
    float tr = 1, tg = 1, tb = 1;
    if (tintindex == 0)      { tr = 0.568f; tg = 0.741f; tb = 0.349f; }   // 草 #91BD59
    else if (tintindex == 1) { tr = 0.466f; tg = 0.670f; tb = 0.184f; }   // 树叶 #77AB2F
    else if (tintindex == 2) { tr = 0.247f; tg = 0.463f; tb = 0.894f; }   // 水 #3F76E4
    else return c;
    u32 a = (c >> 24) & 0xFF;
    u32 r = (u32)(((c >> 16) & 0xFF) * tr);
    u32 g = (u32)(((c >> 8) & 0xFF) * tg);
    u32 b = (u32)((c & 0xFF) * tb);
    return (a << 24) | (r << 16) | (g << 8) | b;
}

static inline float FaceShade(int face) {
    switch (face) {
        case mc::F_UP:    return 1.00f;
        case mc::F_DOWN:  return 0.55f;
        case mc::F_NORTH: return 0.82f;
        case mc::F_SOUTH: return 0.72f;
        case mc::F_WEST:  return 0.88f;
        case mc::F_EAST:  return 0.68f;
        default:          return 0.85f;
    }
}
} // namespace

// ------------------------------------------------------------------ 主渲染
struct Camera {
    V3 R, U, D;      // 右、上、视线方向
    float scale = 1;
    float ox = 0, oy = 0;
    V3 Project(const V3& p, float& depth) const {
        depth = Dot(p, D);
        return { Dot(p, R), -Dot(p, U), 0 };
    }
};

void GetViewAngles(int preset, float& yaw, float& pitch) { ViewAngles(preset, yaw, pitch); }

static void BuildCamera(int preset, float yawOverride, float pitchDeg, Camera& cam) {
    float yaw, pitch;
    ViewAngles(preset, yaw, pitch);
    if (yawOverride > -9000) yaw = yawOverride;
    pitch = pitchDeg;
    float yr = yaw * 3.14159265358979f / 180.0f;
    float pr = pitch * 3.14159265358979f / 180.0f;
    float sy = sinf(yr), cy = cosf(yr), sp = sinf(pr), cp = cosf(pr);
    cam.D = { cp * sy, -sp, cp * cy };
    cam.R = { cy, 0, -sy };
    cam.U = Cross(cam.D, cam.R);
    cam.U = Norm(cam.U);
}

bool RenderSchematic(const ltm::Schematic& sc, const mc::BlockDB& db, int versionIdx,
                     const Options& opt, Image& out, std::string& err) {
    if (db.Empty()) { err = "方块数据库为空"; return false; }
    if (versionIdx < 0 || (size_t)versionIdx >= db.VersionCount()) versionIdx = 0;
    const mc::VersionDef& ver = db.Version(versionIdx);

    Camera cam;
    BuildCamera(opt.view, opt.yawOverride, opt.pitch, cam);

    // 1) 构建所有可见面
    std::vector<Quad> opaque, trans;
    const u8* pix = db.PixelBase();
    std::vector<Quad> local;

    for (const ltm::Region& r : sc.Regions()) {
        // 不透明缓存
        std::vector<i8> opaqueCache(r.palette.size(), -1);
        for (size_t i = 0; i < r.indices.size(); ++i) {
            i32 pi = r.indices[i];
            if (pi < 0 || (size_t)pi >= r.palette.size()) continue;
            const ltm::BlockState& st = r.palette[(size_t)pi];
            if (st.name == "minecraft:air" || st.name == "minecraft:cave_air" ||
                st.name == "minecraft:void_air" || st.name == "minecraft:structure_void") continue;

            std::vector<mc::BlockDB::ResolvedModel> res;
            db.ResolveModels(ver, st, res);
            if (res.empty()) continue;

            i32 bx, by, bz;
            i32 p3[3];
            r.PosFromIndex(i, p3);
            bx = p3[0]; by = p3[1]; bz = p3[2];

            for (const auto& rm : res) {
                if (!rm.model) continue;
                const mc::Model& m = *rm.model;
                for (const mc::ModelElement& e : m.elements) {
                    for (int face = 0; face < mc::F_COUNT; ++face) {
                        if (!(e.faceMask & (1u << face))) continue;
                        const mc::ModelFace* mf = nullptr;
                        int cnt = 0;
                        for (int f2 = 0; f2 < mc::F_COUNT; ++f2) {
                            if (e.faceMask & (1u << f2)) {
                                if (f2 == face) { mf = &e.faces[(size_t)cnt]; break; }
                                ++cnt;
                            }
                        }
                        if (!mf) continue;

                        // 邻面剔除
                        if (mf->cullface != 0xFF) {
                            int dx = 0, dy = 0, dz = 0;
                            switch (mf->cullface) {
                                case mc::F_DOWN:  dy = -1; break;
                                case mc::F_UP:    dy = 1; break;
                                case mc::F_NORTH: dz = -1; break;
                                case mc::F_SOUTH: dz = 1; break;
                                case mc::F_WEST:  dx = -1; break;
                                case mc::F_EAST:  dx = 1; break;
                            }
                            i32 nx = bx + dx, ny = by + dy, nz = bz + dz;
                            if (!r.Inside(nx, ny, nz)) {
                                // 区域外视为空气，不剔除（保证结构外表面完整）
                            } else {
                                size_t ni = r.Index(nx, ny, nz);
                                i32 npi = r.indices[ni];
                                if (npi >= 0 && (size_t)npi < r.palette.size()) {
                                    if (opaqueCache[(size_t)npi] < 0) {
                                        opaqueCache[(size_t)npi] =
                                            ComputeOpaque(db, ver, r.palette[(size_t)npi]) ? 1 : 0;
                                    }
                                    if (opaqueCache[(size_t)npi] == 1) continue;   // 被挡住
                                }
                            }
                        }

                        // 元素顶点（含元素自身旋转 + 方块旋转），单位 0..16
                        V3 a = { e.from[0], e.from[1], e.from[2] };
                        V3 b = { e.to[0], e.to[1], e.to[2] };
                        FaceCorners fc;
                        BuildFaceCorners(face, a, b, mf->uv, fc);
                        Quad q;
                        q.texIdx = mf->texStrIdx;
                        q.face = face;
                        q.transparent = m.translucent;

                        for (int k = 0; k < 4; ++k) {
                            V3 p = fc.c[k];
                            if (e.hasRot) p = RotateElement(p, e);
                            p = RotateBlock(p, rm.rotX, rm.rotY);
                            // 到世界坐标：方块位置 + 元素局部坐标/16
                            q.p[k] = { (float)bx + p.x / 16.0f,
                                       (float)by + p.y / 16.0f,
                                       (float)bz + p.z / 16.0f };
                        }

                        // 法线校正朝向 + 背面剔除
                        V3 n = Norm(Cross(Sub(q.p[1], q.p[0]), Sub(q.p[2], q.p[0])));
                        V3 want = FaceNormalModel(face);
                        want = RotateBlock({ want.x * 16 + 8, want.y * 16 + 8, want.z * 16 + 8 }, rm.rotX, rm.rotY);
                        want = { want.x - 8, want.y - 8, want.z - 8 };
                        want = Norm(want);
                        if (Dot(n, want) < 0) {
                            std::swap(q.p[1], q.p[3]);
                            std::swap(fc.uv[1], fc.uv[3]);
                            n = Norm(Cross(Sub(q.p[1], q.p[0]), Sub(q.p[2], q.p[0])));
                        }
                        q.normal = n;
                        if (Dot(n, cam.D) >= 0.0f) continue;    // 背面

                        // UV -> 图集像素
                        u32 texIdx = 0xFFFFFFFF;
                        mc::TextureInfo ti{};
                        if (mf->texStrIdx != 0xFFFFFFFF) {
                            std::string path = db.Str(mf->texStrIdx);
                            u32 found = db.FindTexture(ver, path);
                            if (found != 0xFFFFFFFFu) {
                                texIdx = found;
                                ti = db.TextureAt(texIdx);
                            }
                        }
                        q.texIdx = texIdx;
                        for (int k = 0; k < 4; ++k) {
                            float fu = fc.uv[k][0] / 16.0f;
                            float fv = fc.uv[k][1] / 16.0f;
                            // 贴图旋转
                            if (mf->rotation == 90)       { float t = fu; fu = fv; fv = 1 - t; }
                            else if (mf->rotation == 180) { fu = 1 - fu; fv = 1 - fv; }
                            else if (mf->rotation == 270) { float t = fu; fu = 1 - fv; fv = t; }
                            if (texIdx == 0xFFFFFFFF) {
                                q.au[k] = fu * 16.0f;
                                q.av[k] = fv * 16.0f;
                            } else {
                                q.au[k] = (float)ti.w * fu;
                                q.av[k] = (float)ti.h * fv;
                            }
                        }
                        if (ti.hasAlpha) q.transparent = true;
                        (q.transparent ? trans : opaque).push_back(q);
                    }
                }
            }
        }
    }

    if (opaque.empty() && trans.empty()) { err = "该投影没有可渲染的方块"; return false; }

    // 2) 计算屏幕包围盒（用于自动缩放居中）
    float minX = 1e30f, minY = 1e30f, maxX = -1e30f, maxY = -1e30f;
    auto accum = [&](const std::vector<Quad>& v) {
        for (const Quad& q : v)
            for (int k = 0; k < 4; ++k) {
                float d;
                V3 s = cam.Project(q.p[k], d);
                minX = std::min(minX, s.x); maxX = std::max(maxX, s.x);
                minY = std::min(minY, s.y); maxY = std::max(maxY, s.y);
            }
    };
    accum(opaque); accum(trans);

    int ss = std::max(1, std::min(4, opt.supersample));
    int W = opt.size * ss, H = opt.size * ss;
    float margin = 4.0f * (float)ss;
    float spanX = std::max(1e-4f, maxX - minX);
    float spanY = std::max(1e-4f, maxY - minY);
    float fitScale = std::min((W - margin * 2) / spanX, (H - margin * 2) / spanY);
    cam.scale = fitScale;
    cam.ox = (W - spanX * fitScale) * 0.5f - minX * fitScale;
    cam.oy = (H - spanY * fitScale) * 0.5f - minY * fitScale;

    // 3) 光栅化
    Image canvas;
    canvas.Resize(W, H, opt.transparentBg ? 0x00000000u : opt.bg);
    std::vector<float> zbuf((size_t)W * (size_t)H, 1e30f);

    auto project = [&](const V3& p, float& sx, float& sy, float& z) {
        V3 s = cam.Project(p, z);
        sx = s.x * cam.scale + cam.ox;
        sy = s.y * cam.scale + cam.oy;
    };

    struct SV { float x, y, z, au, av; };

    auto drawTri = [&](const SV& v0, const SV& v1, const SV& v2, u32 texIdx,
                       float shade, int tint, bool transparent) {
        float area = (v1.x - v0.x) * (v2.y - v0.y) - (v2.x - v0.x) * (v1.y - v0.y);
        if (fabsf(area) < 1e-9f) return;
        int x0 = (int)floorf(std::min({ v0.x, v1.x, v2.x }));
        int x1 = (int)ceilf(std::max({ v0.x, v1.x, v2.x }));
        int y0 = (int)floorf(std::min({ v0.y, v1.y, v2.y }));
        int y1 = (int)ceilf(std::max({ v0.y, v1.y, v2.y }));
        x0 = std::max(0, x0); y0 = std::max(0, y0);
        x1 = std::min(W - 1, x1); y1 = std::min(H - 1, y1);
        const mc::TextureInfo* ti = nullptr;
        if (texIdx != 0xFFFFFFFF) ti = &db.TextureAt(texIdx);
        float inv = 1.0f / area;
        for (int y = y0; y <= y1; ++y) {
            for (int x = x0; x <= x1; ++x) {
                float px = (float)x + 0.5f, py = (float)y + 0.5f;
                float w0 = ((v1.x - v0.x) * (py - v0.y) - (px - v0.x) * (v1.y - v0.y)) * inv;
                float w1 = ((px - v0.x) * (v2.y - v0.y) - (v2.x - v0.x) * (py - v0.y)) * inv;
                float w2 = 1.0f - w0 - w1;
                if (w0 < 0 || w1 < 0 || w2 < 0) continue;
                float z = w2 * v0.z + w1 * v1.z + w0 * v2.z;
                size_t zi = (size_t)y * (size_t)W + (size_t)x;
                if (z > zbuf[zi]) continue;
                float au = w2 * v0.au + w1 * v1.au + w0 * v2.au;
                float av = w2 * v0.av + w1 * v1.av + w0 * v2.av;
                u32 c;
                if (ti) c = SampleAtlas(db, *ti, pix, au, av);
                else    c = MissingTextureColor(x / ss, y / ss);
                if (c == 0xFFFFFFFFu) continue;
                if (((c >> 24) & 0xFF) == 0) continue;
                c = ShadeColor(c, shade);
                if (tint >= 0) c = ApplyTint(c, tint);
                if (!transparent) {
                    zbuf[zi] = z;
                    canvas.At(x, y) = c;
                } else {
                    u32 dst = canvas.At(x, y);
                    u32 sa = (c >> 24) & 0xFF;
                    u32 da = (dst >> 24) & 0xFF;
                    auto mix = [&](int sh) {
                        u32 s = (c >> sh) & 0xFF, d = (dst >> sh) & 0xFF;
                        return (u32)((s * sa + d * da * (255 - sa) / 255) / std::max(1u, sa + da * (255 - sa) / 255));
                    };
                    u32 na = sa + da * (255 - sa) / 255;
                    u32 nr = mix(16), ng = mix(8), nb = mix(0);
                    canvas.At(x, y) = (na << 24) | (nr << 16) | (ng << 8) | nb;
                }
            }
        }
    };

    auto emitQuad = [&](const Quad& q, bool transparent) {
        SV sv[4];
        for (int k = 0; k < 4; ++k) {
            float sx, sy, z;
            project(q.p[k], sx, sy, z);
            sv[k] = { sx, sy, z, q.au[k], q.av[k] };
        }
        float shade = FaceShade(q.face);
        // 用几何法线做一点方向光，让旋转过的面也有变化
        float lambert = 0.62f + 0.38f * std::max(0.0f, -q.normal.y * 0.65f + q.normal.z * 0.25f - q.normal.x * 0.2f + 0.5f);
        shade *= std::min(1.15f, std::max(0.6f, lambert));
        drawTri(sv[0], sv[1], sv[2], q.texIdx, shade, -1, transparent);
        drawTri(sv[0], sv[2], sv[3], q.texIdx, shade, -1, transparent);
    };

    for (const Quad& q : opaque) emitQuad(q, false);
    // 透明面按深度从远到近
    std::sort(trans.begin(), trans.end(), [&](const Quad& a, const Quad& b) {
        float da = 0, db2 = 0;
        V3 sa = cam.Project(a.p[0], da);
        V3 sb = cam.Project(b.p[0], db2);
        return da > db2;
    });
    for (const Quad& q : trans) emitQuad(q, true);

    // 4) 超采样降采样
    if (ss > 1) {
        Image small;
        small.Resize(opt.size, opt.size, 0);
        for (int y = 0; y < opt.size; ++y) {
            for (int x = 0; x < opt.size; ++x) {
                u64 r = 0, g = 0, b = 0, a = 0;
                for (int dy = 0; dy < ss; ++dy)
                    for (int dx = 0; dx < ss; ++dx) {
                        u32 c = canvas.At(x * ss + dx, y * ss + dy);
                        a += (c >> 24) & 0xFF;
                        r += (c >> 16) & 0xFF;
                        g += (c >> 8) & 0xFF;
                        b += c & 0xFF;
                    }
                u32 n = (u32)(ss * ss);
                small.At(x, y) = (u32)((a / n) << 24 | (r / n) << 16 | (g / n) << 8 | (b / n));
            }
        }
        out = std::move(small);
    } else {
        out = std::move(canvas);
    }
    return true;
}

// ------------------------------------------------------------------ 单方块图标
bool RenderBlockIcon(const mc::BlockDB& db, int versionIdx, const ltm::BlockState& st,
                     int size, Image& out, bool transparentBg) {
    if (db.Empty()) return false;
    if (versionIdx < 0 || (size_t)versionIdx >= db.VersionCount()) versionIdx = 0;
    const mc::VersionDef& ver = db.Version(versionIdx);

    // 临时构造一个只含该方块的小"投影"
    ltm::Schematic dummy;
    nbt::Value root = nbt::Value::MakeCompound();
    nbt::Value regions = nbt::Value::MakeCompound();
    nbt::Value r = nbt::Value::MakeCompound();
    nbt::Value pos = nbt::Value::MakeCompound();
    pos.set("x", nbt::Value::MakeInt(0)); pos.set("y", nbt::Value::MakeInt(0)); pos.set("z", nbt::Value::MakeInt(0));
    nbt::Value sz = nbt::Value::MakeCompound();
    sz.set("x", nbt::Value::MakeInt(1)); sz.set("y", nbt::Value::MakeInt(1)); sz.set("z", nbt::Value::MakeInt(1));
    r.set("Position", pos);
    r.set("Size", sz);
    nbt::Value pal = nbt::Value::MakeList(nbt::TAG_Compound);
    nbt::Value e = nbt::Value::MakeCompound();
    e.set("Name", nbt::Value::MakeString(st.name));
    if (!st.props.empty()) {
        nbt::Value pr = nbt::Value::MakeCompound();
        for (const auto& kv : st.props) pr.set(kv.first, nbt::Value::MakeString(kv.second));
        e.set("Properties", pr);
    }
    pal.list.push_back(e);
    r.set("BlockStatePalette", pal);
    r.set("BlockStates", nbt::Value::MakeLongArray({ 0 }));
    regions.set("r", r);
    root.set("Regions", regions);

    // 直接用内部结构而非走 Load（避免文件 IO）
    std::string err;
    {
        // 借助 Peek/Load 太重，这里手工构造 Region
        // 通过一个临时文件不可取，改为直接调用内部接口：复制 RenderSchematic 的构造逻辑过于冗长，
        // 因此这里用一个轻量适配：构造临时的 Schematic 需要 FromNbt，故用一个内部辅助。
    }
    (void)dummy; (void)root; (void)err;

    // 轻量实现：只渲染一个方块的模型
    Options opt;
    opt.size = size;
    opt.view = VIEW_LEFT45;
    opt.pitch = 30.0f;
    opt.transparentBg = transparentBg;
    opt.supersample = 2;
    opt.bg = 0xFFFFFFFFu;

    // 用一个只含单方块的内联渲染
    Camera cam;
    BuildCamera(opt.view, -9999, opt.pitch, cam);
    std::vector<Quad> quads;
    std::vector<mc::BlockDB::ResolvedModel> res;
    db.ResolveModels(ver, st, res);
    const u8* pix = db.PixelBase();
    for (const auto& rm : res) {
        if (!rm.model) continue;
        for (const mc::ModelElement& el : rm.model->elements) {
            for (int face = 0; face < mc::F_COUNT; ++face) {
                if (!(el.faceMask & (1u << face))) continue;
                const mc::ModelFace* mf = nullptr;
                int cnt = 0;
                for (int f2 = 0; f2 < mc::F_COUNT; ++f2) {
                    if (el.faceMask & (1u << f2)) { if (f2 == face) { mf = &el.faces[(size_t)cnt]; break; } ++cnt; }
                }
                if (!mf) continue;
                V3 a = { el.from[0], el.from[1], el.from[2] };
                V3 b = { el.to[0], el.to[1], el.to[2] };
                FaceCorners fc;
                BuildFaceCorners(face, a, b, mf->uv, fc);
                Quad q;
                q.face = face;
                q.transparent = rm.model->translucent;
                for (int k = 0; k < 4; ++k) {
                    V3 p = fc.c[k];
                    if (el.hasRot) p = RotateElement(p, el);
                    p = RotateBlock(p, rm.rotX, rm.rotY);
                    q.p[k] = { p.x / 16.0f - 0.5f, p.y / 16.0f - 0.5f, p.z / 16.0f - 0.5f };
                }
                V3 n = Norm(Cross(Sub(q.p[1], q.p[0]), Sub(q.p[2], q.p[0])));
                if (Dot(n, cam.D) >= 0) continue;
                q.normal = n;
                u32 texIdx = 0xFFFFFFFF;
                mc::TextureInfo ti{};
                if (mf->texStrIdx != 0xFFFFFFFF) {
                    std::string path = db.Str(mf->texStrIdx);
                    u32 found = db.FindTexture(ver, path);
                    if (found != 0xFFFFFFFFu) { texIdx = found; ti = db.TextureAt(texIdx); }
                }
                q.texIdx = texIdx;
                if (ti.hasAlpha) q.transparent = true;
                for (int k = 0; k < 4; ++k) {
                    float fu = fc.uv[k][0] / 16.0f, fv = fc.uv[k][1] / 16.0f;
                    if (texIdx == 0xFFFFFFFF) { q.au[k] = fu * 16.0f; q.av[k] = fv * 16.0f; }
                    else { q.au[k] = (float)ti.w * fu; q.av[k] = (float)ti.h * fv; }
                }
                quads.push_back(q);
            }
        }
    }
    if (quads.empty()) return false;

    float minX = 1e30f, minY = 1e30f, maxX = -1e30f, maxY = -1e30f;
    for (const Quad& q : quads)
        for (int k = 0; k < 4; ++k) {
            float d; V3 s = cam.Project(q.p[k], d);
            minX = std::min(minX, s.x); maxX = std::max(maxX, s.x);
            minY = std::min(minY, s.y); maxY = std::max(maxY, s.y);
        }
    int ss = 2, W = size * ss, H = size * ss;
    float spanX = std::max(1e-4f, maxX - minX), spanY = std::max(1e-4f, maxY - minY);
    float sc = std::min((W - 8.0f) / spanX, (H - 8.0f) / spanY);
    cam.scale = sc;
    cam.ox = (W - spanX * sc) * 0.5f - minX * sc;
    cam.oy = (H - spanY * sc) * 0.5f - minY * sc;

    Image canvas;
    canvas.Resize(W, H, transparentBg ? 0x00000000u : opt.bg);
    std::vector<float> zbuf((size_t)W * (size_t)H, 1e30f);
    struct SV2 { float x, y, z, au, av; };
    for (const Quad& q : quads) {
        SV2 sv[4];
        for (int k = 0; k < 4; ++k) {
            float d; V3 s = cam.Project(q.p[k], d);
            sv[k] = { s.x * cam.scale + cam.ox, s.y * cam.scale + cam.oy, d, q.au[k], q.av[k] };
        }
        for (int t = 0; t < 2; ++t) {
            const SV2& v0 = sv[0];
            const SV2& v1 = sv[t + 1];
            const SV2& v2 = sv[t + 2];
            float area = (v1.x - v0.x) * (v2.y - v0.y) - (v2.x - v0.x) * (v1.y - v0.y);
            if (fabsf(area) < 1e-9f) continue;
            int x0 = std::max(0, (int)floorf(std::min({ v0.x, v1.x, v2.x })));
            int x1 = std::min(W - 1, (int)ceilf(std::max({ v0.x, v1.x, v2.x })));
            int y0 = std::max(0, (int)floorf(std::min({ v0.y, v1.y, v2.y })));
            int y1 = std::min(H - 1, (int)ceilf(std::max({ v0.y, v1.y, v2.y })));
            const mc::TextureInfo* ti = (q.texIdx != 0xFFFFFFFF) ? &db.TextureAt(q.texIdx) : nullptr;
            float inv = 1.0f / area, shade = FaceShade(q.face);
            for (int y = y0; y <= y1; ++y)
                for (int x = x0; x <= x1; ++x) {
                    float px = x + 0.5f, py = y + 0.5f;
                    float w0 = ((v1.x - v0.x) * (py - v0.y) - (px - v0.x) * (v1.y - v0.y)) * inv;
                    float w1 = ((px - v0.x) * (v2.y - v0.y) - (v2.x - v0.x) * (py - v0.y)) * inv;
                    float w2 = 1 - w0 - w1;
                    if (w0 < 0 || w1 < 0 || w2 < 0) continue;
                    float z = w2 * v0.z + w1 * v1.z + w0 * v2.z;
                    size_t zi = (size_t)y * W + x;
                    if (z > zbuf[zi]) continue;
                    float au = w2 * v0.au + w1 * v1.au + w0 * v2.au;
                    float av = w2 * v0.av + w1 * v1.av + w0 * v2.av;
                    u32 c = ti ? SampleAtlas(db, *ti, pix, au, av) : MissingTextureColor(x / ss, y / ss);
                    if (c == 0xFFFFFFFFu || ((c >> 24) & 0xFF) == 0) continue;
                    c = ShadeColor(c, shade);
                    if (q.transparent) {
                        u32 dst = canvas.At(x, y);
                        u32 sa = (c >> 24) & 0xFF, da = (dst >> 24) & 0xFF;
                        u32 na = sa + da * (255 - sa) / 255;
                        auto mx = [&](int sh) {
                            u32 s = (c >> sh) & 0xFF, d = (dst >> sh) & 0xFF;
                            return (u32)((s * sa + d * da * (255 - sa) / 255) / std::max(1u, na));
                        };
                        canvas.At(x, y) = (na << 24) | (mx(16) << 16) | (mx(8) << 8) | mx(0);
                    } else {
                        zbuf[zi] = z;
                        canvas.At(x, y) = c;
                    }
                }
        }
    }

    Image small;
    small.Resize(size, size, 0);
    for (int y = 0; y < size; ++y)
        for (int x = 0; x < size; ++x) {
            u64 r = 0, g = 0, b = 0, a = 0;
            for (int dy = 0; dy < ss; ++dy)
                for (int dx = 0; dx < ss; ++dx) {
                    u32 c = canvas.At(x * ss + dx, y * ss + dy);
                    a += (c >> 24) & 0xFF; r += (c >> 16) & 0xFF;
                    g += (c >> 8) & 0xFF; b += c & 0xFF;
                }
            u32 n = (u32)(ss * ss);
            small.At(x, y) = ((a / n) << 24) | ((r / n) << 16) | ((g / n) << 8) | (b / n);
        }
    out = std::move(small);
    return true;
}

// ------------------------------------------------------------------ PNG 输出
namespace {
static void PutU32(std::vector<u8>& v, u32 x) {
    v.push_back((u8)(x >> 24)); v.push_back((u8)(x >> 16));
    v.push_back((u8)(x >> 8));  v.push_back((u8)x);
}
} // namespace

bool SavePng(const std::wstring& path, const Image& img) {
    if (img.w <= 0 || img.h <= 0) return false;
    std::vector<u8> raw;
    raw.reserve((size_t)img.h * ((size_t)img.w * 4 + 1));
    for (int y = 0; y < img.h; ++y) {
        raw.push_back(0);   // filter none
        for (int x = 0; x < img.w; ++x) {
            u32 c = img.At(x, y);
            raw.push_back((u8)((c >> 16) & 0xFF));
            raw.push_back((u8)((c >> 8) & 0xFF));
            raw.push_back((u8)(c & 0xFF));
            raw.push_back((u8)((c >> 24) & 0xFF));
        }
    }
    std::vector<u8> comp;
    {
        uLongf bound = compressBound((uLong)raw.size());
        comp.resize(bound);
        if (compress2(comp.data(), &bound, raw.data(), (uLong)raw.size(), 6) != Z_OK) return false;
        comp.resize(bound);
    }
    std::vector<u8> png = { 0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A };
    auto chunk = [&](const char* type, const std::vector<u8>& data) {
        PutU32(png, (u32)data.size());
        size_t start = png.size();
        png.insert(png.end(), type, type + 4);
        png.insert(png.end(), data.begin(), data.end());
        uLong crc = crc32(0L, Z_NULL, 0);
        crc = crc32(crc, png.data() + start, (uInt)(png.size() - start));
        PutU32(png, (u32)crc);
    };
    {
        std::vector<u8> ihdr;
        PutU32(ihdr, (u32)img.w); PutU32(ihdr, (u32)img.h);
        ihdr.push_back(8); ihdr.push_back(6); ihdr.push_back(0);
        ihdr.push_back(0); ihdr.push_back(0);
        chunk("IHDR", ihdr);
    }
    chunk("IDAT", comp);
    chunk("IEND", {});
    return fsx::WriteAllBytes(path, png);
}

} // namespace render
