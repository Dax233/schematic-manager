#pragma once
#include "common.h"

// 极简 JSON 解析器（只读），用于解析 Minecraft 的 blockstates / models / lang 文件
struct Json {
    enum Type { NUL = 0, BOOL, NUM, STR, ARR, OBJ };
    Type type = NUL;
    bool b = false;
    double num = 0;
    std::string str;                                        // UTF-8
    std::vector<Json> arr;
    std::vector<std::pair<std::string, Json>> obj;          // 保持原始顺序

    bool isNull() const { return type == NUL; }
    bool isObj()  const { return type == OBJ; }
    bool isArr()  const { return type == ARR; }
    bool isStr()  const { return type == STR; }
    bool isNum()  const { return type == NUM; }
    bool isBool() const { return type == BOOL; }

    const Json* find(const std::string& key) const;
    const Json& at(size_t i) const;                          // 越界返回静态空对象

    std::string  getStr(const std::string& def = "") const;
    double       getNum(double def = 0) const;
    i64          getInt(i64 def = 0) const;
    bool         getBool(bool def = false) const;

    size_t size() const { return type == ARR ? arr.size() : (type == OBJ ? obj.size() : 0); }
};

Json JsonParse(const char* data, size_t len, bool* ok = nullptr);
Json JsonParse(const std::string& s, bool* ok = nullptr);
