// 把技术动作映射成"小白能看懂的人话"

export function humanizeAction(kind: string, target: string): string {
  switch (kind) {
    case "reg-delete":
      if (target.includes("MyComputer\\NameSpace")) {
        return "从「我的电脑」里删了一个图标";
      }
      if (target.includes("Classes\\CLSID")) {
        return "删了一个软件的图标注册";
      }
      return "删了一项注册表设置";
    case "service-stop":
      return `关掉了后台服务「${prettyServiceName(target)}」`;
    case "service-disable":
      return `禁止「${prettyServiceName(target)}」开机自启`;
    case "task-disable":
      return `禁用了一个计划任务`;
    case "process-kill":
      return `结束了进程「${target}」`;
    case "file-delete":
      return `删除了「${prettyFileName(target)}」`;
    default:
      return `执行了 ${kind}`;
  }
}

function prettyServiceName(name: string): string {
  if (name.toLowerCase().includes("123synccloud")) return "123 云盘";
  if (name.toLowerCase().includes("baidunetdisk")) return "百度网盘";
  if (name.toLowerCase().includes("kugou")) return "酷狗音乐";
  if (name.toLowerCase().includes("wetype")) return "微信输入法";
  return name;
}

function prettyFileName(path: string): string {
  const idx = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return idx >= 0 ? path.slice(idx + 1) : path;
}

// AI tool 调用的人话翻译
export function humanizeToolCall(name: string, args: any): string {
  switch (name) {
    case "query_pc_namespace":
      return "🔍 看看「我的电脑」里有哪些图标";
    case "query_processes":
      return args?.name_substr
        ? `🔍 查含「${args.name_substr}」的进程`
        : "🔍 列出所有进程";
    case "query_services":
      return args?.name_substr
        ? `🔍 查含「${args.name_substr}」的后台服务`
        : "🔍 列出所有后台服务";
    case "query_registry_value":
      return `🔍 看一项系统设置`;
    case "reg_delete":
      return `🗑️ 删一项系统设置 (${args?.reason || ""})`;
    case "service_stop":
      return `⏸️ 停掉后台服务「${prettyServiceName(args?.name || "")}」`;
    case "service_disable":
      return `🔒 禁止「${prettyServiceName(args?.name || "")}」开机自启`;
    case "task_disable":
      return `⏹️ 禁用一个计划任务`;
    case "process_kill":
      return `❌ 杀掉「${args?.name || ""}」进程 (不可逆!)`;
    default:
      return name;
  }
}

export function humanizeVerdict(verdict: string): { icon: string; text: string; cls: string } {
  switch (verdict) {
    case "safe":
      return { icon: "✅", text: "检查过, 没问题", cls: "verdict-safe" };
    case "needs_approval":
      return { icon: "⚠", text: "有点风险, 你确认下", cls: "verdict-warn" };
    case "deny":
      return { icon: "🔥", text: "有破坏性, 仔细看看", cls: "verdict-danger" };
    default:
      return { icon: "❓", text: verdict, cls: "" };
  }
}
