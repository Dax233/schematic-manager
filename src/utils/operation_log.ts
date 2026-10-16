/**
 * 操作日志的一行解析成三段，供界面分行展示。
 *
 * 存储格式由后端的 `backup::append_log` 决定：`纳秒时间戳 \t 说明`。
 * 说明本身又以一个动词开头（「替换 …」「删除到回收站 …」「转换 …」），
 * 后面是文件名一类的细节。
 *
 * 为什么要在界面这一层拆：日志行是给文件存的，一整行读起来没有结构——
 * 时间戳是 20 位数字，说明里可能塞进几十个文件名，挤在一起无法逐条阅读。
 * 拆成「时间 / 动作 / 细节」后，时间可以格式化成日期、动作能单独成标签，
 * 每条之间的边界也就看得见了。
 */

/** 一条操作日志 */
export interface LogEntry {
  /** 纳秒时间戳（纯数字串）；解析不出来时为空串 */
  timeNs: string;
  /** 开头的动作词；取不到时为空串 */
  action: string;
  /** 动作之后的细节（文件名等） */
  detail: string;
}

/** 解析一行日志 */
export function parseLogLine(line: string): LogEntry {
  const tab = line.indexOf("\t");
  const rawTime = tab >= 0 ? line.slice(0, tab) : "";
  const text = tab >= 0 ? line.slice(tab + 1) : line;
  const space = text.indexOf(" ");
  const action = space >= 0 ? text.slice(0, space) : text;
  const detail = space >= 0 ? text.slice(space + 1) : "";
  return {
    // 时间戳必须是纯数字，否则「时间」这一列会显示成一个词
    timeNs: /^\d+$/.test(rawTime) ? rawTime : "",
    action: action.trim(),
    detail: detail.trim(),
  };
}
