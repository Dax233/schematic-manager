#include "json.h"

static const Json kEmptyJson;

const Json* Json::find(const std::string& key) const {
    if (type != OBJ) return nullptr;
    for (const auto& kv : obj) if (kv.first == key) return &kv.second;
    return nullptr;
}

const Json& Json::at(size_t i) const {
    if (type != ARR || i >= arr.size()) return kEmptyJson;
    return arr[i];
}

std::string Json::getStr(const std::string& def) const { return type == STR ? str : def; }
double      Json::getNum(double def) const { return type == NUM ? num : (type == BOOL ? (b ? 1.0 : 0.0) : def); }
i64         Json::getInt(i64 def) const {
    if (type == NUM) return (i64)(num < 0 ? num - 0.5 : num + 0.5);
    if (type == BOOL) return b ? 1 : 0;
    return def;
}
bool Json::getBool(bool def) const {
    if (type == BOOL) return b;
    if (type == NUM) return num != 0;
    return def;
}

namespace {

struct Parser {
    const char* p;
    const char* end;
    bool ok = true;

    void skipWs() {
        while (p < end) {
            char c = *p;
            if (c == ' ' || c == '\t' || c == '\r' || c == '\n') ++p;
            else break;
        }
    }
    bool eof() const { return p >= end; }
    char peek() const { return p < end ? *p : '\0'; }

    void fail() { ok = false; }

    static void AppendUtf8(std::string& out, unsigned cp) {
        if (cp < 0x80) out.push_back((char)cp);
        else if (cp < 0x800) {
            out.push_back((char)(0xC0 | (cp >> 6)));
            out.push_back((char)(0x80 | (cp & 0x3F)));
        } else if (cp < 0x10000) {
            out.push_back((char)(0xE0 | (cp >> 12)));
            out.push_back((char)(0x80 | ((cp >> 6) & 0x3F)));
            out.push_back((char)(0x80 | (cp & 0x3F)));
        } else {
            out.push_back((char)(0xF0 | (cp >> 18)));
            out.push_back((char)(0x80 | ((cp >> 12) & 0x3F)));
            out.push_back((char)(0x80 | ((cp >> 6) & 0x3F)));
            out.push_back((char)(0x80 | (cp & 0x3F)));
        }
    }

    unsigned ReadHex4() {
        unsigned v = 0;
        for (int i = 0; i < 4 && p < end; ++i) {
            char c = *p++;
            v <<= 4;
            if (c >= '0' && c <= '9') v |= (unsigned)(c - '0');
            else if (c >= 'a' && c <= 'f') v |= (unsigned)(c - 'a' + 10);
            else if (c >= 'A' && c <= 'F') v |= (unsigned)(c - 'A' + 10);
            else { fail(); return 0; }
        }
        return v;
    }

    std::string parseString() {
        std::string out;
        if (peek() != '"') { fail(); return out; }
        ++p;
        while (p < end) {
            char c = *p++;
            if (c == '"') return out;
            if (c == '\\') {
                if (p >= end) break;
                char e = *p++;
                switch (e) {
                    case '"':  out.push_back('"'); break;
                    case '\\': out.push_back('\\'); break;
                    case '/':  out.push_back('/'); break;
                    case 'b':  out.push_back('\b'); break;
                    case 'f':  out.push_back('\f'); break;
                    case 'n':  out.push_back('\n'); break;
                    case 'r':  out.push_back('\r'); break;
                    case 't':  out.push_back('\t'); break;
                    case 'u': {
                        unsigned cp = ReadHex4();
                        if (cp >= 0xD800 && cp <= 0xDBFF && p + 1 < end && p[0] == '\\' && p[1] == 'u') {
                            p += 2;
                            unsigned lo = ReadHex4();
                            if (lo >= 0xDC00 && lo <= 0xDFFF)
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                        }
                        AppendUtf8(out, cp);
                        break;
                    }
                    default: out.push_back(e); break;
                }
            } else {
                out.push_back(c);
            }
        }
        fail();
        return out;
    }

    Json parseValue(int depth) {
        Json v;
        if (depth > 200) { fail(); return v; }
        skipWs();
        if (eof()) { fail(); return v; }
        char c = peek();
        if (c == '{') {
            ++p;
            v.type = Json::OBJ;
            skipWs();
            if (peek() == '}') { ++p; return v; }
            while (p < end) {
                skipWs();
                std::string key = parseString();
                if (!ok) return v;
                skipWs();
                if (peek() != ':') { fail(); return v; }
                ++p;
                Json val = parseValue(depth + 1);
                if (!ok) return v;
                v.obj.emplace_back(std::move(key), std::move(val));
                skipWs();
                if (peek() == ',') { ++p; continue; }
                if (peek() == '}') { ++p; return v; }
                fail();
                return v;
            }
            fail();
            return v;
        }
        if (c == '[') {
            ++p;
            v.type = Json::ARR;
            skipWs();
            if (peek() == ']') { ++p; return v; }
            while (p < end) {
                Json val = parseValue(depth + 1);
                if (!ok) return v;
                v.arr.push_back(std::move(val));
                skipWs();
                if (peek() == ',') { ++p; continue; }
                if (peek() == ']') { ++p; return v; }
                fail();
                return v;
            }
            fail();
            return v;
        }
        if (c == '"') {
            v.type = Json::STR;
            v.str = parseString();
            return v;
        }
        if (c == 't') {
            if (end - p >= 4 && strncmp(p, "true", 4) == 0) { p += 4; v.type = Json::BOOL; v.b = true; return v; }
            fail(); return v;
        }
        if (c == 'f') {
            if (end - p >= 5 && strncmp(p, "false", 5) == 0) { p += 5; v.type = Json::BOOL; v.b = false; return v; }
            fail(); return v;
        }
        if (c == 'n') {
            if (end - p >= 4 && strncmp(p, "null", 4) == 0) { p += 4; v.type = Json::NUL; return v; }
            fail(); return v;
        }
        // number
        {
            const char* start = p;
            if (peek() == '-' || peek() == '+') ++p;
            while (p < end && ((*p >= '0' && *p <= '9') || *p == '.' || *p == 'e' || *p == 'E' || *p == '+' || *p == '-')) ++p;
            if (p == start) { fail(); return v; }
            std::string tmp(start, (size_t)(p - start));
            v.type = Json::NUM;
            v.num = strtod(tmp.c_str(), nullptr);
            return v;
        }
    }
};

} // namespace

Json JsonParse(const char* data, size_t len, bool* ok) {
    Parser ps;
    ps.p = data;
    ps.end = data + len;
    Json v = ps.parseValue(0);
    if (ok) *ok = ps.ok;
    return v;
}

Json JsonParse(const std::string& s, bool* ok) {
    return JsonParse(s.data(), s.size(), ok);
}
