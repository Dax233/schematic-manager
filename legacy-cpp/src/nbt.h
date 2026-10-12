#pragma once
#include "common.h"

// Minecraft NBT（大端序）+ gzip 容器
namespace nbt {

enum TagType : u8 {
    TAG_End = 0, TAG_Byte, TAG_Short, TAG_Int, TAG_Long, TAG_Float, TAG_Double,
    TAG_ByteArray, TAG_String, TAG_List, TAG_Compound, TAG_IntArray, TAG_LongArray
};

struct Value {
    TagType type = TAG_End;
    i64     i = 0;          // Byte / Short / Int / Long
    double  d = 0;          // Float / Double
    std::string s;          // String (UTF-8)
    std::vector<u8>  bytes; // ByteArray
    std::vector<i32> ints;  // IntArray
    std::vector<i64> longs; // LongArray
    TagType listElem = TAG_End;
    std::vector<Value> list;                                    // List
    std::vector<std::pair<std::string, Value>> fields;          // Compound（保持顺序）

    Value() {}
    static Value MakeByte(i8 v)            { Value x; x.type = TAG_Byte;  x.i = v; return x; }
    static Value MakeShort(i16 v)          { Value x; x.type = TAG_Short; x.i = v; return x; }
    static Value MakeInt(i32 v)            { Value x; x.type = TAG_Int;   x.i = v; return x; }
    static Value MakeLong(i64 v)           { Value x; x.type = TAG_Long;  x.i = v; return x; }
    static Value MakeFloat(float v)        { Value x; x.type = TAG_Float; x.d = v; return x; }
    static Value MakeDouble(double v)      { Value x; x.type = TAG_Double;x.d = v; return x; }
    static Value MakeString(const std::string& v) { Value x; x.type = TAG_String; x.s = v; return x; }
    static Value MakeCompound()            { Value x; x.type = TAG_Compound; return x; }
    static Value MakeList(TagType elem)    { Value x; x.type = TAG_List; x.listElem = elem; return x; }
    static Value MakeByteArray(std::vector<u8> v)   { Value x; x.type = TAG_ByteArray; x.bytes = std::move(v); return x; }
    static Value MakeIntArray(std::vector<i32> v)   { Value x; x.type = TAG_IntArray;  x.ints  = std::move(v); return x; }
    static Value MakeLongArray(std::vector<i64> v)  { Value x; x.type = TAG_LongArray; x.longs = std::move(v); return x; }

    bool isCompound() const { return type == TAG_Compound; }
    bool isList()     const { return type == TAG_List; }
    bool isString()   const { return type == TAG_String; }

    Value*       find(const std::string& key);
    const Value* find(const std::string& key) const;
    bool         has(const std::string& key) const { return find(key) != nullptr; }

    void set(const std::string& key, Value v);      // 存在则替换，否则追加

    i64         asInt(i64 def = 0) const;
    i32         asInt32(i32 def = 0) const { return (i32)asInt(def); }
    double      asDouble(double def = 0) const;
    float       asFloat(float def = 0) const { return (float)asDouble(def); }
    std::string asString(const std::string& def = "") const;
    bool        asBool(bool def = false) const;

    // 便捷：读取 {x,y,z} 三元组
    void asXYZ(i32 out[3]) const;
};

// 从原始 NBT 字节解析
bool Parse(const u8* data, size_t size, Value& out, std::string& err);
// 序列化为原始 NBT 字节
bool Serialize(const Value& root, std::vector<u8>& out);

// gzip 容器（自动识别输入是否已压缩）
bool ParseMaybeGzip(const std::vector<u8>& fileData, Value& out, std::string& err);
bool SerializeGzip(const Value& root, std::vector<u8>& out, int level = 6);

// 独立 gzip 帮助函数
bool Gunzip(const u8* in, size_t inLen, std::vector<u8>& out);
bool Gzip(const u8* in, size_t inLen, std::vector<u8>& out, int level = 6);

const char* TagTypeName(TagType t);

} // namespace nbt
